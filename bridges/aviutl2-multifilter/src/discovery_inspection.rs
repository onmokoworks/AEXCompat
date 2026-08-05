/// Everything discovery learns about one AEX before any worker inspects it:
/// the negative-by-default cache entry (sha + closure recorded) and, when
/// the closure resolved, the closure itself plus its cluster identity.
struct PreparedDiscovery {
    entry: CacheEntry,
    closure: Option<ResolvedDependencyClosure>,
    identity: Option<String>,
    /// Extra authenticated inputs beyond the import closure (issue #362):
    /// host facility DLLs the plug-in loads lazily (BIB.dll).
    extra_dependencies: Vec<ApprovedImageArtifact>,
    /// Authenticated data resources staged into `<sealed root>/<subdir>/`
    /// (issue #362: `Film Stocks`-type data files).
    sealed_resources: Vec<SealedResourceEntry>,
}

/// Bounds for one plug-in's sealed data resources (issue #362, design
/// docs/SEALED_DATA_RESOURCE_POLICY_2026-07-25.md §4): exceeding either is a
/// fail-closed discovery failure, never a partial staging.
const MAX_SEALED_RESOURCES: usize = 256;
const MAX_SEALED_RESOURCE_BYTES: u64 = 64 * 1024 * 1024;

/// Extra authenticated inputs beyond the import closure: host facility DLLs
/// (BIB.dll, lazily loaded by the #485 bounded path) and data resources the
/// plug-in reads from `<its dir>/<subdir>/`.
struct ExtraSealedInputs {
    dependencies: Vec<ApprovedImageArtifact>,
    resources: Vec<SealedResourceEntry>,
}

/// Gathers the extra sealed inputs for one plug-in (issue #362):
///
/// - BIB.dll: closures that never link BIB statically (Scribble) leave the
///   bounded #485 load with nothing to find in the sealed root. When the
///   closure lacks BIB.dll, the copy beside the plug-in or in a dependency
///   search root is added as one authenticated dependency.
/// - `Film Stocks`: when the plug-in's own directory holds a `Film Stocks`
///   subdirectory, every plain file directly inside it becomes a data
///   resource (`Film Stocks/<basename>`), count/size-bounded; a symlink or
///   reparse entry fails closed.
fn gather_extra_sealed_inputs(
    plugin: &Path,
    closure_dependencies: &[ApprovedImageArtifact],
    roots: &[PathBuf],
) -> Result<ExtraSealedInputs, String> {
    let mut dependencies = Vec::new();
    let has_bib = closure_dependencies.iter().any(|dependency| {
        dependency
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("bib.dll"))
    });
    if !has_bib {
        let mut candidates: Vec<PathBuf> = plugin
            .parent()
            .map(|parent| parent.join("BIB.dll"))
            .into_iter()
            .collect();
        candidates.extend(roots.iter().map(|root| root.join("BIB.dll")));
        if let Some(bib) = candidates.into_iter().find(|path| path.is_file()) {
            let bytes =
                std::fs::read(&bib).map_err(|error| format!("BIB.dll is unreadable: {error}"))?;
            dependencies.push(ApprovedImageArtifact {
                path: bib,
                expected_sha256: Sha256::digest(&bytes).into(),
                expected_size: bytes.len() as u64,
            });
        }
    }

    let mut resources = Vec::new();
    let film_stocks = plugin
        .parent()
        .map(|parent| parent.join("Film Stocks"))
        .filter(|dir| dir.is_dir());
    if let Some(dir) = film_stocks {
        let mut total_bytes = 0u64;
        for entry in std::fs::read_dir(&dir).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                return Err(format!(
                    "sealed resource entry is not a plain file: {}",
                    path.display()
                ));
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .ok_or_else(|| "sealed resource has a non-UTF-8 name".to_owned())?
                .to_owned();
            let bytes = std::fs::read(&path)
                .map_err(|error| format!("sealed resource unreadable: {error}"))?;
            total_bytes = total_bytes.saturating_add(bytes.len() as u64);
            if resources.len() >= MAX_SEALED_RESOURCES || total_bytes > MAX_SEALED_RESOURCE_BYTES {
                return Err("sealed resource limit exceeded".to_owned());
            }
            resources.push(SealedResourceEntry {
                source: path,
                relative_path: format!("Film Stocks/{name}"),
                expected_sha256: Sha256::digest(&bytes).into(),
                expected_size: bytes.len() as u64,
            });
        }
        resources.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    }
    Ok(ExtraSealedInputs {
        dependencies,
        resources,
    })
}

/// The pre-inspect half of discovery (the legacy `discover_one` up to the
/// worker dispatch): read + hash the plug-in, resolve and record its
/// dependency closure. A plug-in whose closure fails records the surveyed
/// negative here and never reaches an inspect — per-plugin or clustered.
fn prepare_discovery(
    plugin: &Path,
    dependency: &DependencyConfig,
    build: BuildFingerprint,
) -> PreparedDiscovery {
    let mut entry = negative_entry(plugin, build);
    let Ok(bytes) = std::fs::read(plugin) else {
        return PreparedDiscovery {
            entry,
            closure: None,
            identity: None,
            extra_dependencies: Vec::new(),
            sealed_resources: Vec::new(),
        };
    };
    entry.sha = hex_lower(&Sha256::digest(&bytes));
    // Seal the plug-in's dependency DLLs with it, so an effect whose imports live
    // in its installed runtime folder can load inside the isolated sealed root at
    // all (issue #304). A closure that cannot be resolved is a failed discovery,
    // not a dependency-free retry.
    let roots = search_roots_for(plugin, &dependency.dirs);
    let recorded_roots: Vec<String> = roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect();
    let Ok(closure) = dependency_closure_for(plugin, dependency, &roots) else {
        // A resolution that failed outright — over an operator's ceiling, an
        // unreadable image — still records what it looked at, so the negative both
        // converges (no re-walk every launch) and is redone once the reason it
        // failed could have gone away. Surveying costs a walk without the hashing
        // or copying, which is what the failure saved in the first place.
        entry.closure = match survey_dependency_closure(plugin, &roots) {
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
        return PreparedDiscovery {
            entry,
            closure: None,
            identity: None,
            extra_dependencies: Vec::new(),
            sealed_resources: Vec::new(),
        };
    };
    let sealed_paths: Vec<PathBuf> = closure
        .dependencies()
        .iter()
        .map(|sealed| sealed.path.clone())
        .collect();
    let (sealed, vanished) = cached_dependencies(&sealed_paths);
    let provenance = cached_provenance(closure.provenance());
    let mut missing = cached_missing(closure.unresolved());
    missing.extend(vanished);
    missing.sort();
    missing.dedup();
    entry.closure = CachedClosure {
        roots: recorded_roots.clone(),
        sealed,
        missing,
        provenance,
    };
    // Extra sealed inputs beyond the import closure (issue #362): the lazily
    // loaded host facility DLL and `Film Stocks`-type data resources. A
    // gathering failure is a fail-closed discovery failure, not a partial
    // staging that would leave the plug-in to fail opaquely inside the worker.
    let extra = match gather_extra_sealed_inputs(plugin, closure.dependencies(), &roots) {
        Ok(extra) => extra,
        Err(reason) => {
            entry.failure_classification = Some(format!("sealed_resource_limit:{reason}"));
            return PreparedDiscovery {
                entry,
                closure: None,
                identity: None,
                extra_dependencies: Vec::new(),
                sealed_resources: Vec::new(),
            };
        }
    };
    // The identity covers everything sealed into the tree, extras included,
    // so two plug-ins cluster only when every sealed input matches.
    let mut identity_artifacts: Vec<ApprovedImageArtifact> = closure.dependencies().to_vec();
    identity_artifacts.extend(extra.dependencies.iter().cloned());
    let identity = closure_identity_of(&identity_artifacts);
    entry.closure_identity = Some(identity.clone());
    PreparedDiscovery {
        entry,
        closure: Some(closure),
        identity: Some(identity),
        extra_dependencies: extra.dependencies,
        sealed_resources: extra.resources,
    }
}

/// Diagnostics entry into the crate's real discovery pass (issue #751): the
/// same `discover_all` the AviUtl2 registration runs, callable from the
/// measurement examples so an in-place vs staged A/B exercises the code that
/// ships instead of a reimplementation. Not part of the bridge API.
#[doc(hidden)]
pub fn discover_all_for_diagnostics(
    repository: &Path,
    paths: &[PathBuf],
    dependency_dirs: Vec<PathBuf>,
) -> Vec<(PathBuf, bool, Option<String>)> {
    let dependency = DependencyConfig {
        dirs: dependency_dirs,
        module_limit: None,
        byte_limit: None,
    };
    let build = build_fingerprint(repository, &dependency);
    discover_all(repository, paths, &dependency, build)
        .into_iter()
        .map(|(path, entry)| (path, entry.ok, entry.failure_classification))
        .collect()
}

/// Whether discovery loads plug-ins in place (issue #751, the default):
/// no dependency-closure walk, no sealed staging — the loader resolves the
/// closure through the search roots. The staged pipeline stays available for
/// A/B measurement and as an escape hatch.
fn in_place_discovery_enabled() -> bool {
    !std::env::var("AEXCOMPAT_MULTIFILTER_STAGED_DISCOVERY").is_ok_and(|value| value == "1")
}

/// Whether render sessions load plug-ins in place (issue #751, the default):
/// the session opens on the plug-in's real path with the search roots
/// admitted, and pooled cluster sessions ride the v2 manifest. The staged
/// pipeline (closure walk + sealed tree) stays selectable for A/B.
fn in_place_render_enabled() -> bool {
    !std::env::var("AEXCOMPAT_MULTIFILTER_STAGED_RENDER").is_ok_and(|value| value == "1")
}

/// The in-place cluster key (issue #751): plug-ins sharing one search-root
/// set (their own directory plus the configured dependency directories)
/// share one discovery session. Replaces the closure identity, which needed
/// the import walk this mode removes.
fn in_place_identity(roots: &[PathBuf]) -> String {
    let mut keys: Vec<String> = roots
        .iter()
        .map(|root| root.to_string_lossy().to_lowercase())
        .collect();
    keys.sort();
    format!("in-place:{}", keys.join(";"))
}

/// The pre-inspect half of in-place discovery (issue #751): read + hash the
/// plug-in and record the search roots. No closure walk, no extra sealed
/// inputs — BIB resolves through the admitted search set inside the worker
/// and data files sit beside the real plug-in already.
///
/// `closure_identity` records the search-root identity: the render cluster
/// pool groups effects by it and swaps them inside one in-place session
/// (issue #751 step 3).
fn prepare_discovery_in_place(
    plugin: &Path,
    dependency: &DependencyConfig,
    build: BuildFingerprint,
) -> PreparedDiscovery {
    let mut entry = negative_entry(plugin, build);
    let Ok(bytes) = std::fs::read(plugin) else {
        return PreparedDiscovery {
            entry,
            closure: None,
            identity: None,
            extra_dependencies: Vec::new(),
            sealed_resources: Vec::new(),
        };
    };
    entry.sha = hex_lower(&Sha256::digest(&bytes));
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
        closure: None,
        identity: Some(identity),
        extra_dependencies: Vec::new(),
        sealed_resources: Vec::new(),
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
fn finish_one_shot(
    repository: &Path,
    plugin: &Path,
    mut prepared: PreparedDiscovery,
) -> CacheEntry {
    let Some(closure) = prepared.closure.take() else {
        return prepared.entry;
    };
    let mut entry = prepared.entry;
    // Extra sealed inputs (issue #362): the lazily loaded host facility DLL
    // rides the dependency list; data resources stage into `<root>/<subdir>/`.
    let mut dependencies = closure.into_dependencies();
    dependencies.extend(prepared.extra_dependencies);
    match inspect_experimental_with_approved_dependencies_and_resources(
        repository,
        plugin,
        &entry.sha,
        dependencies,
        prepared.sealed_resources,
    ) {
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
        }
    }
    entry
}

/// Discovers one AEX, always returning a cache entry (cache-all): `ok = true` for
/// a discoverable effect, `ok = false` for any failure (a genuine non-effect, or
/// an AEX the compat host cannot load, or a timeout). Discovery runs on the
/// background thread, so caching every outcome — even a timeout — means it is not
/// re-probed on later launches; a spurious negative is cleared by re-touching the
/// AEX or deleting the cache file (documented in the README).
fn discover_one(
    repository: &Path,
    plugin: &Path,
    dependency: &DependencyConfig,
    build: BuildFingerprint,
) -> CacheEntry {
    finish_one_shot(
        repository,
        plugin,
        prepare_discovery(plugin, dependency, build),
    )
}

/// Parses the parameter rows of an `--l2-params-only`-shape report (what a
/// discovery session's `inspect_done` carries, design §4.2) into broker
/// parameters. Mirrors the conversion the one-shot broker path applies to
/// the same report, so a cluster-inspected effect caches byte-identical
/// parameters to a one-shot-inspected one.
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
fn fallback_members(
    repository: &Path,
    members: Vec<(PathBuf, PreparedDiscovery)>,
    at_member: u32,
    reason: &str,
) -> Vec<(PathBuf, CacheEntry)> {
    members
        .into_iter()
        .map(|(path, prepared)| {
            let mut entry = finish_one_shot(repository, &path, prepared);
            entry.cluster_fallback = Some(ClusterFallback {
                at_member,
                reason: reason.to_owned(),
                resolution: "one_shot_fallback".to_owned(),
            });
            (path, entry)
        })
        .collect()
}

/// In-place counterpart of `fallback_members` (issue #751): re-inspects
/// members through the in-place one-shot path after a session failure.
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
fn discover_cluster(
    repository: &Path,
    dependency: &DependencyConfig,
    build: BuildFingerprint,
    members: Vec<(PathBuf, PreparedDiscovery)>,
) -> Vec<(PathBuf, CacheEntry)> {
    let member_count = members.len();
    // Every member's closure resolved to the same identity, so the first
    // member's dependency set (plus the shared extra inputs — issue #362:
    // the identity covers them, so they are identical across members) stands
    // for the whole cluster.
    let shared_dependencies = members[0].1.closure.as_ref().map(|closure| {
        let mut dependencies = closure.dependencies().to_vec();
        dependencies.extend(members[0].1.extra_dependencies.iter().cloned());
        dependencies
    });
    // Data resources merge across members: an identical entry (same path,
    // same bytes — the Film Stocks siblings share one folder) stages once;
    // the same path with different bytes cannot coexist in one flat tree, so
    // the whole cluster falls back per-plugin (fail-closed).
    let mut sealed_resources: Vec<SealedResourceEntry> = Vec::new();
    for (_, prepared) in &members {
        for resource in &prepared.sealed_resources {
            let key = resource.relative_path.to_lowercase();
            match sealed_resources
                .iter()
                .find(|existing| existing.relative_path.to_lowercase() == key)
            {
                None => sealed_resources.push(resource.clone()),
                Some(existing)
                    if existing.expected_sha256 == resource.expected_sha256
                        && existing.expected_size == resource.expected_size => {}
                Some(_) => {
                    return fallback_members(
                        repository,
                        members,
                        0,
                        "cluster data resource collision",
                    );
                }
            }
        }
    }
    let mut plugins = Vec::with_capacity(member_count);
    for (path, prepared) in &members {
        let Some(expected_sha256) = decode_sha256_hex(&prepared.entry.sha) else {
            return fallback_members(repository, members, 0, "member sha256 undecodable");
        };
        plugins.push(ApprovedImageArtifact {
            path: path.clone(),
            expected_sha256,
            expected_size: prepared.entry.len,
        });
    }
    let Some(shared_dependencies) = shared_dependencies else {
        // Identity exists only when the closure resolved, so this cannot
        // happen; stay fail-closed anyway.
        return fallback_members(repository, members, 0, "cluster closure unavailable");
    };
    let declared = member_count + shared_dependencies.len();
    if member_count > MAX_CLUSTER_PLUGINS
        || declared + CLUSTER_MODULE_HEADROOM > MAX_CLUSTER_MODULE_BOUND
    {
        return fallback_members(repository, members, 0, "cluster_infeasible");
    }
    let mut session = match DiscoverySession::open(DiscoverySessionOpenRequest {
        repository,
        plugins,
        dependencies: shared_dependencies,
        sealed_resources,
        module_bound: (declared + CLUSTER_MODULE_HEADROOM) as u32,
        inspect_deadline: CLUSTER_INSPECT_DEADLINE,
    }) {
        Ok(session) => session,
        Err(error) => {
            return fallback_members(
                repository,
                members,
                0,
                &format!("cluster session open failed: {error}"),
            );
        }
    };

    let mut results: Vec<(PathBuf, CacheEntry)> = Vec::with_capacity(member_count);
    // Indices whose entry came from a session exchange; a close-time audit
    // rejection makes exactly these untrusted.
    let mut session_produced: Vec<usize> = Vec::new();
    let mut invalidated: Option<(u32, String)> = None;
    for (index, (path, prepared)) in members.into_iter().enumerate() {
        if let Some((at_member, reason)) = &invalidated {
            let (path, entry) =
                fallback_members(repository, vec![(path, prepared)], *at_member, reason)
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
                session_produced.push(results.len());
                results.push((path, entry));
            }
            // A parameter-local failure (design §4.2): the session stays
            // usable, the member records the same verdict the one-shot path
            // would produce.
            Ok(InspectOutcome::InspectError { error_kind, .. }) => {
                let mut entry = prepared.entry;
                entry.failure_classification = match error_kind.as_str() {
                    // The one-shot exit-12 equivalent: genuinely not an
                    // effect (or no resolvable entrypoint) — deterministic.
                    "entrypoint_unresolved" => Some("nonzero_exit".to_owned()),
                    // PARAMS_SETUP rejected: no classification, retried like
                    // the one-shot "AEX rejected PF_PARAMS_SETUP".
                    _ => None,
                };
                results.push((path, entry));
            }
            // Session invalidation (design §6): this member is recorded as a
            // structured failure, the remaining members fall back per-plugin.
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
        // The exchanges completed but close-time validation (the declared-set
        // module audit, design §5) rejected the session: results produced
        // inside it cannot be trusted, so every session-inspected member is
        // redone per-plugin with a fallback note.
        let reason = close
            .get("invalidated_reason")
            .and_then(|invalidation| invalidation.get("reason"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("session close not clean")
            .to_owned();
        for position in session_produced {
            let (path, _) = results[position].clone();
            let mut entry = discover_one(repository, &path, dependency, build);
            entry.cluster_fallback = Some(ClusterFallback {
                at_member: member_count as u32,
                reason: reason.clone(),
                resolution: "one_shot_fallback".to_owned(),
            });
            results[position] = (path, entry);
        }
    }
    results
}

/// Keep broker parameters JSON-round-trippable before they enter the persistent
/// discovery cache (#322). `serde_json` writes non-finite `f64` values as `null`,
/// which the typed `InteractiveParameter` loader rejects on the next launch and
/// sends the same effect through discovery again forever.
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

/// Discovers the given AEX with low bounded parallelism (to keep each discovery
/// under the worker deadline — high concurrency causes contention false-timeouts),
/// caching every result. Stops promptly when `DISCOVERY_SHUTDOWN` is set (plugin
/// unload); unprocessed paths stay misses and are retried next launch.
///
/// Two phases with the same worker budget (`MAX_DISCOVERY_PARALLELISM`)
/// throughout (issue #405):
///
/// 1. Prepare, work-stealing per plug-in: read + hash and resolve the
///    dependency closure (the pre-inspect half of the legacy path), yielding
///    the closure identity each plug-in clusters on.
/// 2. Inspect, work-stealing per *task*: plug-ins sharing one closure
///    identity form a cluster task swept by a single DiscoverySession
///    (design §8); singletons and failed resolutions keep the per-plugin
///    one-shot inspect.
///
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
    // In-place discovery (issue #751) is the default; the staged pipeline
    // stays selectable for A/B measurement.
    let in_place = in_place_discovery_enabled();

    // Phase 1: prepare every plug-in (read + closure resolution) in parallel.
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
                        if in_place {
                            prepare_discovery_in_place(plugin, dependency, build)
                        } else {
                            prepare_discovery(plugin, dependency, build)
                        }
                    }))
                    .unwrap_or_else(|_| PreparedDiscovery {
                        entry: negative_entry(plugin, build),
                        closure: None,
                        identity: None,
                        extra_dependencies: Vec::new(),
                        sealed_resources: Vec::new(),
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
                    dependency_count: prepared
                        .closure
                        .as_ref()
                        .map_or(0, |closure| closure.dependencies().len()),
                },
            )
        })
        .collect();
    let tasks = plan_tasks(&planned);
    let tasks = if in_place {
        shard_in_place_clusters(tasks, parallelism)
    } else {
        tasks
    };

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
                                    if in_place {
                                        finish_one_shot_in_place(
                                            repository, &plugin, prepared, dependency,
                                        )
                                    } else {
                                        finish_one_shot(repository, &plugin, prepared)
                                    }
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
                                        if in_place {
                                            finish_one_shot_in_place(
                                                repository, &plugin, prepared, dependency,
                                            )
                                        } else {
                                            finish_one_shot(repository, &plugin, prepared)
                                        }
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
                                    if in_place {
                                        discover_cluster_in_place(
                                            repository, dependency, build, members,
                                        )
                                    } else {
                                        discover_cluster(repository, dependency, build, members)
                                    }
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
        label: std::ptr::null(),
        information: wide_leak(&format!("AEXCompat multi-filter: {name}")),
        items: items.as_ptr(),
        func_proc_video: Some(func_proc_video),
        func_proc_audio: None,
    }));

    unsafe { ((*host).register_filter_plugin)(table) };
}
