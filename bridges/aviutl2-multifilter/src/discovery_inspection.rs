/// Discovery state for the in-place-only route (#816).
struct PreparedDiscovery {
    entry: CacheEntry,
    identity: Option<String>,
}

/// The in-place cluster key (issue #751): plug-ins sharing one search-root
/// set (their own directory plus the configured dependency directories)
/// share one discovery session, and the render pool groups on the same key.
/// Removed with the staged paths in #870 while its callers stayed (issue
/// #872); in-place-only still needs the key.
fn in_place_identity(roots: &[PathBuf]) -> String {
    let mut keys: Vec<String> = roots
        .iter()
        .map(|root| root.to_string_lossy().to_lowercase())
        .collect();
    keys.sort();
    format!("in-place:{}", keys.join(";"))
}
fn prepare_discovery_in_place(
    plugin: &Path,
    dependency: &DependencyConfig,
    build: BuildFingerprint,
) -> PreparedDiscovery {
    let mut entry = negative_entry(plugin, build);
    let Ok(bytes) = std::fs::read(plugin) else {
        return PreparedDiscovery {
            entry,
            identity: None,
        };
    };
    entry.sha = hex_lower(&Sha256::digest(&bytes));
    // The menu category comes straight from the bytes (issue #871), before
    // any inspect runs; `keep_best` carries it across a failed
    // re-verification, so learning it does not need the inspect to succeed.
    entry.category = pipl_category(&bytes);
    let roots = search_roots_for(plugin, &dependency.dirs);
    entry.closure = CachedClosure {
        roots: roots
            .iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect(),
        ..CachedClosure::default()
    };
    let identity = in_place_identity(&roots);
    entry.closure_identity = Some(identity.clone());
    PreparedDiscovery {
        entry,
        identity: Some(identity),
    }
}

/// Records what the import graph wanted for a plug-in whose in-place
/// discovery failed (issue #751): the survey pays the directory walk only on
/// the failure path, and the recorded sealed/missing sets restore the
/// cache's dependency-change re-discovery triggers — a user who later drops
/// the missing DLL into a search root gets the plug-in re-discovered without
/// touching it. Successful discoveries never walk.
fn record_survey_for_failed_in_place(entry: &mut CacheEntry, plugin: &Path, roots: &[PathBuf]) {
    let recorded_roots: Vec<String> = roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect();
    entry.closure = match survey_dependency_closure(plugin, roots) {
        Ok(survey) => {
            let (sealed, vanished) = cached_dependencies(&survey.modules);
            let mut missing = cached_missing(&survey.unresolved);
            missing.extend(vanished);
            missing.sort();
            missing.dedup();
            CachedClosure {
                roots: recorded_roots,
                sealed,
                missing,
                provenance: cached_provenance(&survey.provenance),
            }
        }
        Err(_) => CachedClosure {
            roots: recorded_roots,
            ..CachedClosure::default()
        },
    };
}

/// The in-place per-plugin inspect (issue #751): the one-shot
/// `inspect_experimental_in_place` dispatch, used for singletons and as the
/// fail-closed fallback for in-place cluster members.
fn finish_one_shot_in_place(
    repository: &Path,
    plugin: &Path,
    prepared: PreparedDiscovery,
    dependency: &DependencyConfig,
) -> CacheEntry {
    let mut entry = prepared.entry;
    if entry.sha.is_empty() {
        return entry;
    }
    let roots = search_roots_for(plugin, &dependency.dirs);
    if roots.is_empty() {
        return entry;
    }
    match inspect_experimental_in_place(repository, plugin, &entry.sha, roots.clone()) {
        Ok((params, diagnostics)) => {
            // PF_OutFlag2_SUPPORTS_SMART_RENDER = bit 10.
            entry.smart = diagnostics
                .get("advertised_out_flags2")
                .and_then(|value| value.as_u64())
                .unwrap_or(0)
                & (1 << 10)
                != 0;
            entry.params = params;
            normalize_parameters_for_cache(&mut entry.params);
            entry.ok = true;
        }
        Err(error) => {
            entry.failure_classification = inspection_failure_classification(&error);
            record_survey_for_failed_in_place(&mut entry, plugin, &roots);
        }
    }
    entry
}

/// The per-plugin inspect: dispatches the one-shot worker for one prepared
/// AEX and folds the outcome into its cache entry. This is the legacy
/// discovery path, kept for singleton identities and as the fail-closed
/// fallback for cluster members (issue #405, design §6).
fn parameters_from_inspect_report(
    report: &serde_json::Value,
) -> Result<Vec<InteractiveParameter>, String> {
    use serde_json::Value;
    let rows = report
        .get("parameters")
        .and_then(Value::as_array)
        .ok_or_else(|| "inspection report has no parameters".to_owned())?;
    let custom_ui_events = report
        .get("custom_ui")
        .and_then(|value| value.get("events"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let mut parameters = Vec::new();
    for row in rows {
        let observed_type = row
            .get("type")
            .and_then(Value::as_i64)
            .ok_or_else(|| "inspection parameter has no numeric type".to_owned())?;
        let observed_index = row
            .get("index")
            .and_then(Value::as_u64)
            .filter(|value| *value <= u64::from(u16::MAX))
            .ok_or_else(|| "inspection parameter has no bounded index".to_owned())?;
        let default = row.get("default").and_then(Value::as_f64).unwrap_or(0.0);
        let ui_flags = row.get("ui_flags").and_then(Value::as_u64).unwrap_or(0);
        let default_color = row.get("default_color");
        let channel = |name: &str| {
            default_color
                .and_then(|value| value.get(name))
                .and_then(Value::as_u64)
                .unwrap_or(if name == "alpha" { 255 } else { 0 }) as u8
        };
        let runtime_kind = match observed_type {
            0 => "layer",
            3 => "angle",
            5 => "color",
            6 => "point",
            2 | 10 => "float",
            8 => "custom",
            9 => "no_data",
            11 => "arbitrary_data",
            12 => "path",
            13 => "group_start",
            14 => "group_end",
            15 => "button",
            18 => "point3d",
            _ => "integer",
        };
        let host_minimum = if observed_type == 12 {
            0.0
        } else {
            row.get("valid_min")
                .and_then(Value::as_f64)
                .unwrap_or(default)
        };
        let host_maximum = if observed_type == 12 {
            1024.0
        } else {
            row.get("valid_max")
                .and_then(Value::as_f64)
                .unwrap_or(default)
        };
        let component_count = match observed_type {
            3 => 1,
            6 => 2,
            18 => 3,
            _ => 0,
        };
        if !matches!(
            observed_type,
            0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 18
        ) {
            continue;
        }
        parameters.push(InteractiveParameter {
            slot: observed_index as u32,
            name: row
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Parameter")
                .to_owned(),
            kind: runtime_kind.into(),
            minimum: host_minimum,
            maximum: host_maximum,
            value: default,
            choices: row
                .get("choices")
                .and_then(Value::as_str)
                .map(|text| text.split('|').map(str::to_owned).collect())
                .unwrap_or_default(),
            color: [
                channel("alpha"),
                channel("red"),
                channel("green"),
                channel("blue"),
            ],
            components: {
                let mut result = [0.0; 3];
                if let Some(values) = row.get("default_components").and_then(Value::as_array) {
                    for (index, value) in values.iter().take(3).enumerate() {
                        result[index] = value.as_f64().unwrap_or(0.0);
                    }
                }
                result
            },
            component_count,
            layer_path: None,
            enabled: ui_flags & (1 << 5) == 0,
            visible: ui_flags & (1 << 9) == 0,
            supervised: row.get("flags").and_then(Value::as_u64).unwrap_or(0) & (1 << 6) != 0,
            debug_summary: row
                .get("arbitrary_summary")
                .and_then(Value::as_str)
                .map(str::to_owned),
            custom_ui_events,
            control_size: [
                row.get("ui_width").and_then(Value::as_u64).unwrap_or(0) as u16,
                row.get("ui_height").and_then(Value::as_u64).unwrap_or(0) as u16,
            ],
        });
    }
    Ok(parameters)
}

/// Folds a successful cluster-inspect report into a member's cache entry,
/// the cluster counterpart of the one-shot inspect tail. A report whose
/// PARAMS_SETUP did not succeed is a plugin-local failure with no
/// classification (retried), exactly like the one-shot
/// "AEX rejected PF_PARAMS_SETUP"; a report without the key at all reads as
/// success, since a failure is always reported through the `error` status
/// the broker maps to `InspectError`.
fn fill_entry_from_inspect_report(entry: &mut CacheEntry, report: &serde_json::Value) {
    if report
        .get("params_setup_error")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0)
        != 0
    {
        return;
    }
    let Ok(params) = parameters_from_inspect_report(report) else {
        return;
    };
    // PF_OutFlag2_SUPPORTS_SMART_RENDER = bit 10.
    entry.smart = report
        .get("out_flags2")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        & (1 << 10)
        != 0;
    entry.params = params;
    normalize_parameters_for_cache(&mut entry.params);
    entry.ok = true;
}

/// One unit of discovery work (issue #405): with the same worker budget as
/// before, the work items are same-closure clusters (one DiscoverySession
/// sweep) and singleton plug-ins (the legacy per-plugin inspect).
enum DiscoveryTask {
    Single(usize),
    Cluster(Vec<usize>),
}

/// What the planner needs to know about one prepared plug-in: its closure
/// identity (when the closure resolved) and how many dependency modules that
/// closure carries.
struct PlannedMember {
    identity: Option<String>,
    dependency_count: usize,
}

/// Groups the prepared plug-ins into tasks by closure identity. Identities
/// with 2..=MAX_CLUSTER_PLUGINS members form one cluster task; singletons
/// whose closure would exceed the one-shot module-audit cap form a
/// one-member cluster task (issue #362: the cluster session's declared-set
/// audit replaces the fixed 512-module cap with the launch-authenticated
/// module bound); everything else — small singletons, failed resolutions (no
/// identity), oversized clusters — stays on the per-plugin path
/// (fail-closed, design §6). Deterministic: clusters in first-seen identity
/// order, then the remaining singles in scan order.
fn plan_tasks(members: &[PlannedMember]) -> Vec<DiscoveryTask> {
    let mut groups: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut first_seen: Vec<&str> = Vec::new();
    for (index, member) in members.iter().enumerate() {
        let Some(identity) = &member.identity else {
            continue;
        };
        groups
            .entry(identity.as_str())
            .or_insert_with(|| {
                first_seen.push(identity.as_str());
                Vec::new()
            })
            .push(index);
    }
    let mut clustered: HashSet<usize> = HashSet::new();
    let mut tasks = Vec::new();
    for identity in first_seen {
        let members = &groups[identity];
        if members.len() >= 2 && members.len() <= MAX_CLUSTER_PLUGINS {
            clustered.extend(members.iter().copied());
            tasks.push(DiscoveryTask::Cluster(members.clone()));
        }
    }
    for (index, member) in members.iter().enumerate() {
        if clustered.contains(&index) {
            continue;
        }
        // A singleton whose closure would trip the one-shot audit cap takes a
        // one-member cluster session (issue #362): same launch trust, but the
        // audit is validated against the declared module bound instead of the
        // fixed 512-module one-shot cap.
        if member.identity.is_some()
            && member.dependency_count + SYSTEM_TAIL_ESTIMATE > ONESHOT_AUDIT_MODULE_LIMIT
        {
            tasks.push(DiscoveryTask::Cluster(vec![index]));
        } else {
            tasks.push(DiscoveryTask::Single(index));
        }
    }
    tasks
}

/// Shards in-place cluster tasks across the worker budget (issue #751). The
/// search-root identity groups an entire plug-in directory into one cluster,
/// and a single session would sweep it serially; splitting it into up to
/// `parallelism` chunks keeps the per-session amortization (one search-set
/// admission, one Adobe runtime init per chunk) while restoring the parallel
/// sweep the staged planner got from its many closure identities. Chunks
/// that would shrink to one member become per-plugin tasks.
fn shard_in_place_clusters(tasks: Vec<DiscoveryTask>, parallelism: usize) -> Vec<DiscoveryTask> {
    let parallelism = parallelism.max(1);
    let mut sharded = Vec::with_capacity(tasks.len());
    for task in tasks {
        match task {
            DiscoveryTask::Cluster(members) if members.len() > 2 => {
                let chunk_count = parallelism.min(members.len() / 2).max(1);
                let chunk_size = members.len().div_ceil(chunk_count);
                for chunk in members.chunks(chunk_size) {
                    if chunk.len() == 1 {
                        sharded.push(DiscoveryTask::Single(chunk[0]));
                    } else {
                        sharded.push(DiscoveryTask::Cluster(chunk.to_vec()));
                    }
                }
            }
            other => sharded.push(other),
        }
    }
    sharded
}

/// Decodes a 64-hex SHA-256 (as stored in `CacheEntry.sha`) into raw bytes
/// for a launch-approved artifact.
fn decode_sha256_hex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut digest = [0u8; 32];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        digest[index] = ((high << 4) | low) as u8;
    }
    Some(digest)
}

/// Re-inspects members per-plugin after a cluster session failure, recording
/// the fallback on every entry (issue #405, design §6). The failed member
/// itself is not retried here — the design records it as a failure, since
/// the same fault would likely recur and cost another full closure staging.
fn fallback_members_in_place(
    repository: &Path,
    members: Vec<(PathBuf, PreparedDiscovery)>,
    dependency: &DependencyConfig,
    at_member: u32,
    reason: &str,
) -> Vec<(PathBuf, CacheEntry)> {
    members
        .into_iter()
        .map(|(path, prepared)| {
            let mut entry = finish_one_shot_in_place(repository, &path, prepared, dependency);
            entry.cluster_fallback = Some(ClusterFallback {
                at_member,
                reason: reason.to_owned(),
                resolution: "one_shot_fallback".to_owned(),
            });
            (path, entry)
        })
        .collect()
}

/// Discovers one same-search-root cluster through an in-place
/// DiscoverySession (issue #751): no closure walk, no staging — the worker
/// admits the search roots once and loads each member from its real path.
/// Failure handling mirrors the sealed cluster path: an infeasible cluster,
/// a failed open, or an invalidation mid-sweep falls the unprocessed members
/// back to the in-place one-shot path with a structured `cluster_fallback`
/// note.
fn discover_cluster_in_place(
    repository: &Path,
    dependency: &DependencyConfig,
    build: BuildFingerprint,
    members: Vec<(PathBuf, PreparedDiscovery)>,
) -> Vec<(PathBuf, CacheEntry)> {
    let member_count = members.len();
    // Every member shares one search-root set by construction (the in-place
    // identity), so the first member's roots stand for the cluster.
    let search_dirs = search_roots_for(&members[0].0, &dependency.dirs);
    if search_dirs.is_empty() {
        return fallback_members_in_place(
            repository,
            members,
            dependency,
            0,
            "cluster search roots unavailable",
        );
    }
    let mut plugins = Vec::with_capacity(member_count);
    for (path, prepared) in &members {
        let Some(expected_sha256) = decode_sha256_hex(&prepared.entry.sha) else {
            return fallback_members_in_place(
                repository,
                members,
                dependency,
                0,
                "member sha256 undecodable",
            );
        };
        plugins.push(ApprovedImageArtifact {
            path: path.clone(),
            expected_sha256,
            expected_size: prepared.entry.len,
        });
    }
    if member_count > MAX_CLUSTER_PLUGINS {
        return fallback_members_in_place(repository, members, dependency, 0, "cluster_infeasible");
    }
    let mut session = match DiscoverySession::open_in_place(InPlaceDiscoverySessionOpenRequest {
        repository,
        plugins,
        dependency_search_dirs: search_dirs.clone(),
        // The recorded audit's bounded enumeration capacity: real closures
        // load in place and retired members stay mapped (deferred release),
        // so the capacity is the manifest maximum rather than a declared set.
        module_bound: MAX_CLUSTER_MODULE_BOUND as u32,
        inspect_deadline: CLUSTER_INSPECT_DEADLINE,
        launch_environment: Default::default(),
    }) {
        Ok(session) => session,
        Err(error) => {
            return fallback_members_in_place(
                repository,
                members,
                dependency,
                0,
                &format!("cluster session open failed: {error}"),
            );
        }
    };

    let mut results: Vec<(PathBuf, CacheEntry)> = Vec::with_capacity(member_count);
    let mut invalidated: Option<(u32, String)> = None;
    for (index, (path, prepared)) in members.into_iter().enumerate() {
        if let Some((at_member, reason)) = &invalidated {
            let (path, entry) = fallback_members_in_place(
                repository,
                vec![(path, prepared)],
                dependency,
                *at_member,
                reason,
            )
            .into_iter()
            .next()
            .expect("one member yields one entry");
            results.push((path, entry));
            continue;
        }
        let request_index = index as u32;
        match session.inspect_plugin(request_index, request_index) {
            Ok(InspectOutcome::Inspected { report }) => {
                let mut entry = prepared.entry;
                fill_entry_from_inspect_report(&mut entry, &report);
                results.push((path, entry));
            }
            Ok(InspectOutcome::InspectError { error_kind, .. }) => {
                let mut entry = prepared.entry;
                entry.failure_classification = match error_kind.as_str() {
                    // Deterministic non-effects converge like the one-shot
                    // exit-12 path.
                    "entrypoint_unresolved" => Some("nonzero_exit".to_owned()),
                    // The loader could not bring the plug-in up from its real
                    // path (a missing dependency outside the search roots);
                    // deterministic until the configuration changes, so the
                    // survey records what the import graph wanted and the
                    // cache re-discovers when it appears.
                    "load_failed" => {
                        record_survey_for_failed_in_place(&mut entry, &path, &search_dirs);
                        Some("nonzero_exit".to_owned())
                    }
                    // The bytes changed after this sweep hashed them (#309
                    // state transition), or could not be read at all: leave
                    // unclassified so the next launch re-discovers.
                    "identity_changed" | "hash_unavailable" => None,
                    // PARAMS_SETUP rejected: retried like the one-shot path.
                    _ => None,
                };
                results.push((path, entry));
            }
            Err(error) => {
                let reason = format!("{error}");
                let mut entry = prepared.entry;
                entry.failure_classification = Some("cluster_session_invalidated".to_owned());
                entry.cluster_fallback = Some(ClusterFallback {
                    at_member: request_index,
                    reason: reason.clone(),
                    resolution: "invalidated".to_owned(),
                });
                results.push((path, entry));
                invalidated = Some((request_index, reason));
            }
        }
    }
    let close = session.close();
    if invalidated.is_none() && close.get("session_clean") != Some(&serde_json::Value::Bool(true)) {
        // The exchanges completed but the close was not clean; in-place
        // results are re-proved through the one-shot path with a fallback
        // note, mirroring the sealed cluster contract.
        let reason = close
            .get("invalidated_reason")
            .and_then(|invalidation| invalidation.get("reason"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("session close not clean")
            .to_owned();
        let redo: Vec<(PathBuf, CacheEntry)> = std::mem::take(&mut results);
        for (path, _) in redo {
            let prepared = prepare_discovery_in_place(&path, dependency, build);
            let mut entry = finish_one_shot_in_place(repository, &path, prepared, dependency);
            entry.cluster_fallback = Some(ClusterFallback {
                at_member: member_count as u32,
                reason: reason.clone(),
                resolution: "one_shot_fallback".to_owned(),
            });
            results.push((path, entry));
        }
    }
    results
}

/// Discovers one same-closure cluster through a single DiscoverySession
/// (issue #405, design §4.2): the closure is sealed and mapped once, and
/// each member is inspected by index. A cluster of one is the issue #362
/// case — a singleton whose closure would exceed the one-shot module-audit
/// cap, inspected under the declared-set audit instead. Every failure is
/// fail-closed — an infeasible cluster, a failed open, or an invalidation
/// mid-sweep falls the not-yet-processed members back to the per-plugin path
/// with a structured `cluster_fallback` note, and a close-time audit
/// rejection redoes every session-inspected member per-plugin.
fn normalize_parameters_for_cache(parameters: &mut [InteractiveParameter]) {
    for parameter in parameters {
        let minimum = parameter.minimum.is_finite().then_some(parameter.minimum);
        let maximum = parameter.maximum.is_finite().then_some(parameter.maximum);
        if let (Some(minimum), Some(maximum)) = (minimum, maximum)
            && minimum < maximum
        {
            parameter.minimum = minimum;
            parameter.maximum = maximum;
        } else {
            parameter.minimum = 0.0;
            parameter.maximum = 1.0;
        }
        if !parameter.value.is_finite() {
            parameter.value = parameter.minimum;
        }
        for component in &mut parameter.components {
            if !component.is_finite() {
                *component = 0.0;
            }
        }
    }
}

/// The three fields [`discover_records_for_diagnostics`] flattens to for a
/// caller that only counts outcomes, kept because `discovery_ab_diag` reports
/// exactly those. (Removed with the staged paths in #870 while that example
/// kept calling it — issue #872.)
#[doc(hidden)]
pub fn discover_all_for_diagnostics(
    repository: &Path,
    paths: &[PathBuf],
    dependency_dirs: Vec<PathBuf>,
) -> Vec<(PathBuf, bool, Option<String>)> {
    discover_records_for_diagnostics(repository, paths, dependency_dirs)
        .into_iter()
        .map(|record| (record.path, record.ok, record.failure_classification))
        .collect()
}

/// One plug-in's discovery outcome with everything a render sweep needs to open
/// a session on it afterwards (issue #957): the parameters the plug-in declared,
/// which render path it advertises, and the search roots its cluster keyed on.
///
/// The cache entry itself is not exposed: these are the fields a sweep reads,
/// copied out, so widening `CacheEntry` does not widen this.
#[doc(hidden)]
pub struct DiagnosticDiscovery {
    pub path: PathBuf,
    /// The plug-in was inspected and its parameters are the declared ones.
    pub ok: bool,
    pub sha256: String,
    pub byte_size: u64,
    /// `PF_OutFlag2_SUPPORTS_SMART_RENDER`; which render path a session opens on.
    pub smart: bool,
    /// The PiPL `catg` property (issue #871), read from the bytes without loading.
    pub category: Option<String>,
    pub parameters: Vec<InteractiveParameter>,
    /// The in-place DLL search roots, in resolution order.
    pub search_roots: Vec<PathBuf>,
    pub failure_classification: Option<String>,
    /// `reason/resolution` of a cluster-session fallback, when this entry came
    /// out of one (issue #405).
    pub cluster_fallback: Option<String>,
}

/// Diagnostics entry into the crate's real discovery pass: the same
/// `discover_all` the AviUtl2 registration runs — same bounded parallelism,
/// same clustering, same fallbacks — callable from the measurement examples so
/// a sweep exercises the code that ships instead of a reimplementation. Not
/// part of the bridge API.
///
/// Neither reads nor writes the discovery cache file, so a sweep cannot demote
/// the entries a running AviUtl2 registration depends on.
#[doc(hidden)]
pub fn discover_records_for_diagnostics(
    repository: &Path,
    paths: &[PathBuf],
    dependency_dirs: Vec<PathBuf>,
) -> Vec<DiagnosticDiscovery> {
    let dependency = DependencyConfig {
        dirs: dependency_dirs,
        module_limit: None,
        byte_limit: None,
    };
    let build = build_fingerprint(repository, &dependency);
    discover_all(repository, paths, &dependency, build)
        .into_iter()
        .map(|(path, entry)| DiagnosticDiscovery {
            path,
            ok: entry.ok,
            sha256: entry.sha,
            byte_size: entry.len,
            smart: entry.smart,
            category: entry.category,
            parameters: entry.params,
            search_roots: entry.closure.roots.iter().map(PathBuf::from).collect(),
            failure_classification: entry.failure_classification,
            cluster_fallback: entry.cluster_fallback.map(|fallback| {
                format!("{}/{}", fallback.reason, fallback.resolution)
            }),
        })
        .collect()
}

/// A panic in any task (arbitrary third-party AEX) is caught and turned into
/// negative entries, so one bad plug-in cannot abort the process by
/// unwinding out of the scoped thread.
fn discover_all(
    repository: &Path,
    paths: &[PathBuf],
    dependency: &DependencyConfig,
    build: BuildFingerprint,
) -> Vec<(PathBuf, CacheEntry)> {
    let parallelism = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(MAX_DISCOVERY_PARALLELISM)
        .min(paths.len().max(1));
    // Issue #816: discovery is in-place only. Phase 1 reads and hashes every
    // plug-in without resolving or staging a dependency closure.
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<(PathBuf, PreparedDiscovery)>>> =
        Mutex::new((0..paths.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..parallelism {
            scope.spawn(|| {
                let slots = &slots;
                loop {
                    if DISCOVERY_SHUTDOWN.load(Ordering::Relaxed) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= paths.len() {
                        break;
                    }
                    let plugin = &paths[index];
                    let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        prepare_discovery_in_place(plugin, dependency, build)
                    }))
                    .unwrap_or_else(|_| PreparedDiscovery {
                        entry: negative_entry(plugin, build),
                        identity: None,
                    });
                    if let Ok(mut slots) = slots.lock() {
                        slots[index] = Some((plugin.clone(), prepared));
                    }
                }
            });
        }
    });
    let slots = slots
        .into_inner()
        .unwrap_or_else(|poison| poison.into_inner());
    let planned: Vec<PlannedMember> = slots
        .iter()
        .map(|slot| {
            slot.as_ref().map_or(
                PlannedMember {
                    identity: None,
                    dependency_count: 0,
                },
                |(_, prepared)| PlannedMember {
                    identity: prepared.identity.clone(),
                    dependency_count: 0,
                },
            )
        })
        .collect();
    let tasks = shard_in_place_clusters(plan_tasks(&planned), parallelism);

    // Phase 2: process tasks with the same worker budget. A cluster is one
    // unit of work: its members are inspected sequentially inside one
    // DiscoverySession on the worker that picked the task up.
    let next = AtomicUsize::new(0);
    let slots = Mutex::new(slots);
    let results: Mutex<Vec<(PathBuf, CacheEntry)>> = Mutex::new(Vec::with_capacity(paths.len()));
    std::thread::scope(|scope| {
        for _ in 0..parallelism {
            scope.spawn(|| {
                let slots = &slots;
                let results = &results;
                loop {
                    if DISCOVERY_SHUTDOWN.load(Ordering::Relaxed) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= tasks.len() {
                        break;
                    }
                    match &tasks[index] {
                        DiscoveryTask::Single(slot_index) => {
                            let taken = slots
                                .lock()
                                .ok()
                                .and_then(|mut slots| slots[*slot_index].take());
                            let Some((plugin, prepared)) = taken else {
                                continue;
                            };
                            let entry =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    finish_one_shot_in_place(
                                        repository, &plugin, prepared, dependency,
                                    )
                                }))
                                .unwrap_or_else(|_| negative_entry(&plugin, build));
                            if let Ok(mut results) = results.lock() {
                                results.push((plugin, entry));
                            }
                        }
                        DiscoveryTask::Cluster(indices) => {
                            let mut members = Vec::with_capacity(indices.len());
                            if let Ok(mut slots) = slots.lock() {
                                for slot_index in indices {
                                    if let Some(member) = slots[*slot_index].take() {
                                        members.push(member);
                                    }
                                }
                            }
                            if members.is_empty() {
                                continue;
                            }
                            // A shutdown gap can deplete a multi-member
                            // cluster to a single prepared member; it takes
                            // the per-plugin path. A task planned as a
                            // one-member cluster (issue #362) has
                            // `indices.len() == 1` by construction and goes
                            // to the session below.
                            if members.len() == 1 && indices.len() > 1 {
                                let (plugin, prepared) = members.pop().expect("one member");
                                let entry =
                                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                        finish_one_shot_in_place(
                                            repository, &plugin, prepared, dependency,
                                        )
                                    }))
                                    .unwrap_or_else(|_| negative_entry(&plugin, build));
                                if let Ok(mut results) = results.lock() {
                                    results.push((plugin, entry));
                                }
                                continue;
                            }
                            let member_paths: Vec<PathBuf> =
                                members.iter().map(|(path, _)| path.clone()).collect();
                            let cluster_results =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    discover_cluster_in_place(
                                        repository, dependency, build, members,
                                    )
                                }))
                                .unwrap_or_else(|_| {
                                    member_paths
                                        .into_iter()
                                        .map(|path| (path.clone(), negative_entry(&path, build)))
                                        .collect()
                                });
                            if let Ok(mut results) = results.lock() {
                                results.extend(cluster_results);
                            }
                        }
                    }
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_else(|poison| poison.into_inner())
}

/// A numerically-comparable key for a version token ("25.0" > "7.0", unlike a
/// lexical compare), falling back to 0 for non-numeric components.
fn version_key(version: &str) -> Vec<u64> {
    version
        .split(['.', ' '])
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

/// The AviUtl2 filter name for each plug-in, in the same order.
///
/// AviUtl2 refuses a filter registered under a name it already holds, and says so
/// with a modal dialog that blocks startup until someone dismisses it. Two
/// same-named `.aex` in different folders is not hypothetical — After Effects
/// ships both `Effects\Threshold.aex` and `Effects\CycoreFXHD\Threshold.aex` — so
/// deriving the name from the file stem alone hangs an unattended launch and
/// leaves one of the two effects unavailable (issue #661).
///
/// A stem is qualified only when it collides, and with the folder that
/// distinguishes it, so every other filter keeps the name the user already knows.
/// Comparison is case-insensitive because Windows filenames are: treating
/// `Threshold` and `threshold` as distinct would hand the host a pair it may
/// still reject.
///
/// `also_known` holds plug-ins that are not being registered but must still count
/// toward collisions — what the cache remembers under the scan roots. Counting
/// only the registered set would let a folder that could not be read this launch
/// change a *surviving* filter's name, and AviUtl2 resolves saved objects by
/// filter name, so that renaming drops them from saved projects for good
/// (issues #307, #321).
///
/// A name still depends on which same-stem plug-ins exist, so installing or
/// uninstalling one renames the others — the trade-off recorded in issue #662.
/// The final numeric pass is global, so a plug-in whose own stem already reads
/// like a generated name (`Threshold (Effects).aex`) can be renamed by an
/// unrelated collision too.
fn unique_filter_names(plugins: &[PathBuf], also_known: &[PathBuf]) -> Vec<String> {
    let mut counted = HashSet::<String>::new();
    let mut counts = HashMap::<String, usize>::new();
    for plugin in plugins.iter().chain(also_known) {
        // A path present in both sets is one plug-in, not a collision with
        // itself. Lowercased because Windows paths are case-insensitive; the
        // other way one file reaches here under two spellings — a re-keyed alias
        // — is dropped by `cached_naming_peers` before it gets this far.
        if !counted.insert(plugin.to_string_lossy().to_lowercase()) {
            continue;
        }
        *counts
            .entry(filter_stem(plugin).to_lowercase())
            .or_default() += 1;
    }

    let mut used = HashSet::<String>::new();
    plugins
        .iter()
        .map(|plugin| {
            let stem = filter_stem(plugin);
            let collides = counts
                .get(&stem.to_lowercase())
                .copied()
                .unwrap_or_default()
                > 1;
            let folder = plugin
                .parent()
                .and_then(Path::file_name)
                .and_then(|folder| folder.to_str());
            let base = match (collides, folder) {
                (true, Some(folder)) => format!("{stem} ({folder})"),
                // Nothing to qualify with (a root-level plug-in, or a folder name
                // that is not UTF-8); the numeric pass below still separates them.
                _ => stem.to_owned(),
            };
            let mut candidate = base.clone();
            let mut disambiguator = 2u32;
            while !used.insert(candidate.to_lowercase()) {
                candidate = format!("{base} [{disambiguator}]");
                disambiguator = disambiguator.saturating_add(1);
            }
            candidate
        })
        .collect()
}

/// The plug-in's file stem, or `AEX` when it has none or is not UTF-8.
fn filter_stem(plugin: &Path) -> &str {
    plugin
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("AEX")
}

/// Registers one discovered AEX as an AviUtl2 filter. Runs on the RegisterPlugin
/// (host callback) thread only. `name` comes from [`unique_filter_names`], which
/// is what keeps the host from being handed two filters under one name.
fn register_discovered(
    host: *mut HOST_APP_TABLE,
    repository: &Path,
    plugin: &Path,
    dependency: &DependencyConfig,
    entry: &CacheEntry,
    name: &str,
) {
    // Build config items + readers + normalized defaults from the exposed params.
    let mut items: Vec<*const c_void> = Vec::new();
    let mut readers: Vec<ItemReader> = Vec::new();
    let mut defaults: Vec<InteractiveParameter> = Vec::new();
    let item_names = unique_item_names(&entry.params);
    for (parameter, item_name) in entry.params.iter().zip(item_names) {
        let Some(item_name) = item_name else {
            continue;
        };
        if let Some((item_ptr, reader, sent)) = build_item(parameter, &item_name) {
            items.push(item_ptr);
            readers.push(reader);
            defaults.push(sent);
        }
    }
    items.push(std::ptr::null());
    let items: &'static [*const c_void] = Box::leak(items.into_boxed_slice());

    let userdata = Box::leak(Box::new(FilterCtx {
        repository: repository.to_path_buf(),
        plugin: plugin.to_path_buf(),
        dependency: dependency.clone(),
        sha: entry.sha.clone(),
        smart: entry.smart,
        closure_identity: entry.closure_identity.clone(),
        // From the raw discovery parameters, NOT from `defaults`: `build_item`
        // maps only value-carrying kinds (float/integer/color) into config
        // items, so a layer parameter never reaches `defaults`.
        layer_slots: layer_slots_of(&entry.params),
        defaults: defaults.clone(),
        readers,
        sessions: Mutex::new(HashMap::new()),
    }));
    // Register this filter's session map so UninitializePlugin can drain it.
    if let Ok(mut maps) = SESSION_MAPS.lock() {
        maps.push(&userdata.sessions);
    }
    // Register the closure-identity cluster membership (issue #405), so the
    // render pool can open one cluster session covering every registered AEX
    // that shares this entry's dependency closure.
    if let Some(identity) = &entry.closure_identity
        && let Ok(mut registry) = CLUSTER_REGISTRY.lock()
    {
        registry
            .get_or_insert_with(HashMap::new)
            .entry(identity.clone())
            .or_default()
            .push(ClusterMember {
                plugin: plugin.to_path_buf(),
                sha: entry.sha.clone(),
                smart: entry.smart,
                defaults,
            });
    }

    let cif = Cif::new([Type::pointer()], Type::u8());
    let closure = Box::leak(Box::new(Closure::new(cif, render_callback, userdata)));
    let code: unsafe extern "C" fn() = *closure.code_ptr();
    let func_proc_video: extern "C" fn(*mut FILTER_PROC_VIDEO) -> bool =
        unsafe { std::mem::transmute(code) };

    let table = Box::leak(Box::new(FILTER_PLUGIN_TABLE {
        flag: 1 | 8, // FLAG_VIDEO | FLAG_FILTER
        name: wide_leak(&format!("AEX: {name}")),
        // The INITIAL menu category (issue #871): hundreds of AE effects do
        // not belong under the default 「加工」 among the built-ins, so they
        // nest under "AEXCompat" by their own PiPL category. Initial only —
        // once AviUtl2 has persisted an effect's label in aviutl2.ini, that
        // (user-editable) value wins on every later launch.
        label: wide_leak(&filter_label(entry.category.as_deref())),
        information: wide_leak(&format!("AEXCompat multi-filter: {name}")),
        items: items.as_ptr(),
        func_proc_video: Some(func_proc_video),
        func_proc_audio: None,
    }));

    unsafe { ((*host).register_filter_plugin)(table) };
}
