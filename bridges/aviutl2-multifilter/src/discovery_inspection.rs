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
    let inspect =
        |roots: Vec<PathBuf>| inspect_experimental_in_place(repository, plugin, &entry.sha, roots);
    let mut effective_roots = roots.clone();
    let inspected = match inspect(roots.clone()) {
        Err(original) if inspection_is_load_failure(&original) => {
            match registered_runtime_retry_roots(plugin, &roots, |basenames| {
                cached_matching_registered_runtime_roots(plugin, &entry.sha, basenames)
            }) {
                RuntimeRootResolution::Resolved(retry_roots) => {
                    effective_roots = retry_roots.clone();
                    inspect(retry_roots)
                }
                failure @ (RuntimeRootResolution::Ambiguous { .. }
                | RuntimeRootResolution::CapacityExceeded { .. }
                | RuntimeRootResolution::DiagnosticsTruncated) => {
                    Err(runtime_root_resolution_error(&failure))
                }
                RuntimeRootResolution::Unresolved => Err(original),
            }
        }
        result => result,
    };
    match inspected {
        Ok((params, diagnostics)) => {
            let identities = match plugin_data_identities(&diagnostics) {
                Ok(identities) => identities,
                Err(_) => return entry,
            };
            if !identities.is_empty()
                && diagnostics
                    .pointer("/plugin_data/selected_index")
                    .and_then(serde_json::Value::as_u64)
                    != Some(0)
            {
                return entry;
            }
            entry.demanded_suites.clear();
            if !merge_demanded_suites_from_report(&mut entry.demanded_suites, &diagnostics) {
                return entry;
            }
            entry.closure.roots = effective_roots
                .iter()
                .map(|root| root.to_string_lossy().into_owned())
                .collect();
            entry.closure_identity = Some(in_place_identity(&effective_roots));
            entry.out_flags2 = diagnostics
                .get("advertised_out_flags2")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as u32;
            // PF_OutFlag2_SUPPORTS_SMART_RENDER = bit 10.
            entry.smart = entry.out_flags2 & (1 << 10) != 0;
            entry.params = params;
            normalize_parameters_for_cache(&mut entry.params);
            entry.plugin_data_effect = identities.first().cloned();
            entry.additional_effects.clear();
            let mut all_effects_inspected = true;
            for identity in identities.iter().skip(1) {
                let selector = identity.selector();
                let Ok((mut params, effect_diagnostics)) =
                    inspect_experimental_in_place_plugin_data_effect(
                        repository,
                        plugin,
                        &entry.sha,
                        effective_roots.clone(),
                        &selector,
                    )
                else {
                    all_effects_inspected = false;
                    break;
                };
                if !selected_plugin_data_identity_matches(&effect_diagnostics, identity) {
                    all_effects_inspected = false;
                    break;
                }
                if !merge_demanded_suites_from_report(
                    &mut entry.demanded_suites,
                    &effect_diagnostics,
                ) {
                    all_effects_inspected = false;
                    break;
                }
                normalize_parameters_for_cache(&mut params);
                let out_flags2 = effect_diagnostics
                    .get("advertised_out_flags2")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32;
                entry.additional_effects.push(CachedPluginDataEffect {
                    identity: identity.clone(),
                    smart: out_flags2 & (1 << 10) != 0,
                    out_flags2,
                    params,
                    registered_name: None,
                });
            }
            if !all_effects_inspected {
                entry.additional_effects.clear();
                entry.plugin_data_effect = None;
                entry.failure_diagnostics = Some(serde_json::json!({
                    "inspection_error_kind": "plugin_data_effect_inspection_failed",
                }));
                return entry;
            }
            if !entry.additional_effects.is_empty() {
                // Existing cluster manifests identify DLL members, not effects.
                // Keep multi-registration bundles on exact per-effect sessions.
                entry.closure_identity = None;
            }
            entry.ok = true;
        }
        Err(error) => {
            if inspection_failure_diagnostics(&error)
                .and_then(|value| value.get("plugin_kind").cloned())
                .and_then(|value| value.as_str().map(str::to_owned))
                .as_deref()
                == Some("aegp_candidate")
            {
                return finish_aegp_discovery(repository, plugin, entry, &roots);
            }
            entry.failure_classification = inspection_failure_classification(&error);
            entry.failure_diagnostics = inspection_failure_diagnostics(&error);
            record_survey_for_failed_in_place(&mut entry, plugin, &effective_roots);
        }
    }
    entry
}

fn plugin_data_identities(diagnostics: &serde_json::Value) -> Result<Vec<PluginDataIdentity>, ()> {
    let Some(plugin_data) = diagnostics.get("plugin_data") else {
        return Ok(Vec::new());
    };
    if plugin_data.is_null() {
        return Ok(Vec::new());
    }
    let object = plugin_data.as_object().ok_or(())?;
    if object.len() != 2 || !object.contains_key("selected_index") {
        return Err(());
    }
    let registrations = object
        .get("registrations")
        .and_then(serde_json::Value::as_array)
        .ok_or(())?;
    if registrations.is_empty() || registrations.len() > 64 {
        return Err(());
    }
    let _selected_index = object
        .get("selected_index")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|index| *index < registrations.len())
        .ok_or(())?;
    let mut identities = Vec::with_capacity(registrations.len());
    for (expected_index, value) in registrations.iter().enumerate() {
        let registration = value.as_object().ok_or(())?;
        if registration.len() != 5 {
            return Err(());
        }
        let index = registration
            .get("index")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(())?;
        if index as usize != expected_index {
            return Err(());
        }
        let text = |key: &str| {
            registration
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .ok_or(())
        };
        let identity = PluginDataIdentity {
            index,
            name_hex: text("name_hex")?,
            match_name_hex: text("match_name_hex")?,
            category_hex: text("category_hex")?,
            entrypoint: text("entrypoint")?,
        };
        if identity.display_name().is_none()
            || decode_plugin_data_bytes(&identity.match_name_hex).is_none()
            || identity.category().is_none()
            || identity.entrypoint.is_empty()
            || identity.entrypoint.len() > 127
            || !identity
                .entrypoint
                .bytes()
                .enumerate()
                .all(|(position, byte)| {
                    byte == b'_'
                        || byte.is_ascii_alphabetic()
                        || (position != 0 && byte.is_ascii_digit())
                })
        {
            return Err(());
        }
        identities.push(identity);
    }
    Ok(identities)
}

fn selected_plugin_data_identity_matches(
    diagnostics: &serde_json::Value,
    expected: &PluginDataIdentity,
) -> bool {
    plugin_data_identities(diagnostics)
        .ok()
        .and_then(|identities| identities.get(expected.index as usize).cloned())
        .is_some_and(|selected| selected == *expected)
        && diagnostics
            .get("plugin_data")
            .and_then(|value| value.get("selected_index"))
            .and_then(serde_json::Value::as_u64)
            == Some(u64::from(expected.index))
}

fn inspection_is_load_failure(error: &std::io::Error) -> bool {
    let Some(diagnostics) = inspection_failure_diagnostics(error) else {
        return false;
    };
    diagnostics
        .get("cluster_error_kind")
        .and_then(|value| value.as_str())
        == Some("load_failed")
        || diagnostics
            .get("load_failure")
            .and_then(|value| value.get("stage"))
            .and_then(|value| value.as_str())
            == Some("load_library")
}

#[derive(Debug, PartialEq)]
enum RuntimeRootResolution {
    Resolved(Vec<PathBuf>),
    Ambiguous {
        basename: String,
        candidate_count: usize,
    },
    CapacityExceeded {
        basename: String,
        root_count: usize,
    },
    DiagnosticsTruncated,
    Unresolved,
}

fn runtime_root_resolution_error(resolution: &RuntimeRootResolution) -> std::io::Error {
    let diagnostics = match resolution {
        RuntimeRootResolution::Ambiguous {
            basename,
            candidate_count,
        } => serde_json::json!({
            "classification": "registered_runtime_ambiguous",
            "reason": "different_provider_bytes",
            "basename": basename,
            "candidate_count": candidate_count,
        }),
        RuntimeRootResolution::CapacityExceeded {
            basename,
            root_count,
        } => serde_json::json!({
            "classification": "registered_runtime_capacity_exceeded",
            "reason": "search_root_limit",
            "basename": basename,
            "root_count": root_count,
        }),
        RuntimeRootResolution::DiagnosticsTruncated => serde_json::json!({
            "classification": "registered_runtime_diagnostics_truncated",
            "reason": "dependency_diagnostics_limit",
        }),
        _ => serde_json::Value::Null,
    };
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("diagnostics={diagnostics}"),
    )
}

/// Resolve a failed LoadLibrary closure only. Each basename must have exactly
/// one registered provider; ambiguity fails closed because AddDllDirectory does
/// not define precedence between multiple USER_DIRS providers. Re-survey after
/// every admitted root so transitive runtime dependencies reach a fixed point.
fn registered_runtime_retry_roots(
    plugin: &Path,
    initial_roots: &[PathBuf],
    mut resolve: impl FnMut(
        &[String],
    ) -> aexcompat_broker::installed_runtime_roots::RegisteredRuntimeLookup,
) -> RuntimeRootResolution {
    const MAX_SEARCH_ROOTS: usize = aexcompat_broker::plugin_dependency_closure::MAX_SEARCH_ROOTS;
    let mut roots = initial_roots.to_vec();
    let initial_len = roots.len();
    loop {
        let Ok(survey) = survey_dependency_closure(plugin, &roots) else {
            return RuntimeRootResolution::Unresolved;
        };
        if survey.dependency_diagnostics_truncated {
            return RuntimeRootResolution::DiagnosticsTruncated;
        }
        for diagnostic in &survey.dependency_diagnostics {
            if diagnostic.search_classification != "ambiguous" {
                continue;
            }
            let candidates: Vec<PathBuf> = roots
                .iter()
                .filter(|root| root.join(&diagnostic.import_basename).is_file())
                .cloned()
                .collect();
            let candidate_count = candidates.len();
            if equivalent_runtime_candidate(&diagnostic.import_basename, candidates).is_none() {
                return RuntimeRootResolution::Ambiguous {
                    basename: diagnostic.import_basename.clone(),
                    candidate_count,
                };
            }
        }
        if survey.unresolved.is_empty() {
            return if roots.len() > initial_len {
                RuntimeRootResolution::Resolved(roots)
            } else {
                RuntimeRootResolution::Unresolved
            };
        }
        let actionable_names: Vec<String> = survey
            .unresolved
            .iter()
            .filter(|basename| {
                !is_api_set(basename)
                    && !std::env::var_os("WINDIR")
                        .map(PathBuf::from)
                        .is_some_and(|windows| windows.join("System32").join(basename).is_file())
            })
            .cloned()
            .collect();
        if actionable_names.is_empty() {
            return if roots.len() > initial_len {
                RuntimeRootResolution::Resolved(roots)
            } else {
                RuntimeRootResolution::Unresolved
            };
        }
        // Resolve the whole dependency set in one indexed lookup. The cache key
        // includes this sorted set, the associated install roots, and the index
        // snapshot, so distinct plug-ins sharing one runtime closure reuse it.
        let candidates_by_basename = match resolve(&actionable_names) {
            aexcompat_broker::installed_runtime_roots::RegisteredRuntimeLookup::Found(found) => {
                found
            }
            aexcompat_broker::installed_runtime_roots::RegisteredRuntimeLookup::IndexTruncated => {
                return RuntimeRootResolution::DiagnosticsTruncated;
            }
        };
        let mut added = false;
        let mut deferred_ambiguity = None;
        for basename in &actionable_names {
            let candidates = candidates_by_basename
                .get(&basename.to_ascii_lowercase())
                .cloned()
                .unwrap_or_default();
            let candidate_count = candidates.len();
            let had_candidates = !candidates.is_empty();
            let Some(candidate) = equivalent_runtime_candidate(basename, candidates) else {
                if had_candidates {
                    deferred_ambiguity = Some(RuntimeRootResolution::Ambiguous {
                        basename: basename.clone(),
                        candidate_count,
                    });
                }
                continue;
            };
            let Ok(candidate) = candidate.canonicalize() else {
                continue;
            };
            if !candidate.is_dir()
                || roots.iter().any(|root| {
                    root.to_string_lossy()
                        .eq_ignore_ascii_case(&candidate.to_string_lossy())
                })
            {
                continue;
            }
            if roots.len() == MAX_SEARCH_ROOTS {
                return RuntimeRootResolution::CapacityExceeded {
                    basename: basename.clone(),
                    root_count: roots.len(),
                };
            }
            roots.push(candidate);
            added = true;
            // Recompute the entire closure after each admitted directory: it
            // may satisfy other names from this now-stale unresolved set.
            break;
        }
        if !added {
            if let Some(ambiguity) = deferred_ambiguity {
                return ambiguity;
            }
            return RuntimeRootResolution::Unresolved;
        }
    }
}

/// Multiple registered directories are equivalent only when the requested DLL
/// itself is byte-identical in every one. The deterministic first directory is
/// then safe for this import; differing providers remain ambiguous.
fn equivalent_runtime_candidate(basename: &str, mut candidates: Vec<PathBuf>) -> Option<PathBuf> {
    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());
    let mut expected = None;
    for directory in &candidates {
        let bytes = std::fs::read(directory.join(basename)).ok()?;
        let digest = Sha256::digest(bytes);
        if expected.as_ref().is_some_and(|value| value != &digest) {
            return None;
        }
        expected = Some(digest);
    }
    candidates.into_iter().next()
}

/// Completes discovery for a PiPL that the PF worker positively identified as
/// an AEGP. A successful AEGP initialization is a readable AEX, but not an
/// effect: the cache kind keeps it out of AviUtl2 filter registration and PF
/// render sessions while allowing the shipping discovery report to distinguish
/// it from an entrypoint failure.
fn finish_aegp_discovery(
    repository: &Path,
    plugin: &Path,
    mut entry: CacheEntry,
    roots: &[PathBuf],
) -> CacheEntry {
    match initialize_experimental_aegp_in_place(repository, plugin, &entry.sha, roots.to_vec()) {
        Ok(report) => {
            entry.provided_suites = report
                .get("dynamic_suites")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|suite| {
                    let name = suite.get("name")?.as_str()?;
                    let api_version = i32::try_from(suite.get("api_version")?.as_i64()?).ok()?;
                    let internal_version =
                        i32::try_from(suite.get("internal_version")?.as_i64()?).ok()?;
                    (name.as_bytes().len() <= 255
                        && !name.is_empty()
                        && api_version > 0
                        && internal_version >= 0)
                        .then(|| ProvidedSuite {
                            name: name.to_owned(),
                            api_version,
                            internal_version,
                        })
                })
                .collect();
            entry.ok = true;
            entry.plugin_kind = DiscoveredPluginKind::Aegp;
            entry.smart = false;
            entry.out_flags2 = 0;
            entry.params.clear();
            entry.failure_classification = None;
            entry.failure_diagnostics = None;
        }
        Err(error) => {
            entry.failure_classification = inspection_failure_classification(&error);
            entry.failure_diagnostics = inspection_failure_diagnostics(&error);
            record_survey_for_failed_in_place(&mut entry, plugin, roots);
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

/// The report fields a discovery failure record carries out of the worker's
/// (partial) L2 report: which selector refused and with what code, and the
/// parameter-count contract inputs. Bounded by construction (scalars plus the
/// worker's own bounded `missing_suites` list), so the record stays cache- and
/// report-sized whatever the plug-in did.
fn inspect_report_failure_fields(
    diagnostics: &mut serde_json::Map<String, serde_json::Value>,
    report: &serde_json::Value,
) {
    if let Some(status) = report.get("status").and_then(serde_json::Value::as_str) {
        diagnostics.insert("inspection_status".to_owned(), status.into());
    }
    for field in [
        "global_setup_error",
        "params_setup_error",
        "global_setdown_error",
        "reported_num_params",
    ] {
        if let Some(value) = report.get(field).and_then(serde_json::Value::as_i64) {
            diagnostics.insert(field.to_owned(), value.into());
        }
    }
    if let Some(parameters) = report
        .get("parameters")
        .and_then(serde_json::Value::as_array)
    {
        diagnostics.insert("parameter_count".to_owned(), parameters.len().into());
    }
    if let Some(missing) = report
        .get("missing_suites")
        .and_then(serde_json::Value::as_array)
    {
        diagnostics.insert(
            "missing_suites".to_owned(),
            serde_json::Value::Array(missing.clone()),
        );
    }
}

/// The `failure_diagnostics` of a session-path member whose inspect came back
/// as `InspectError` (issue #1063): the same `classification` /
/// `cluster_error_kind` / `exit_code` / `plugin_kind` shape as before, plus
/// the worker's partial report fields when it sent one. `classification` is
/// present but null when the failure is unclassified (`identity_changed`,
/// `hash_unavailable`), so a reader can tell "unclassified by design" from
/// "no diagnostics recorded".
fn cluster_inspect_error_diagnostics(
    classification: Option<&str>,
    error_kind: &str,
    exit_code: Option<i64>,
    plugin_kind: Option<&str>,
    report: Option<&serde_json::Value>,
) -> serde_json::Value {
    let mut diagnostics = serde_json::Map::new();
    diagnostics.insert("classification".to_owned(), classification.into());
    diagnostics.insert("cluster_error_kind".to_owned(), error_kind.into());
    if let Some(exit_code) = exit_code {
        diagnostics.insert("exit_code".to_owned(), exit_code.into());
    }
    if let Some(plugin_kind) = plugin_kind {
        diagnostics.insert("plugin_kind".to_owned(), plugin_kind.into());
    }
    if let Some(report) = report {
        inspect_report_failure_fields(&mut diagnostics, report);
    }
    serde_json::Value::Object(diagnostics)
}

/// Folds a successful cluster-inspect report into a member's cache entry,
/// the cluster counterpart of the one-shot inspect tail. A report whose
/// PARAMS_SETUP did not succeed is a plugin-local failure with no
/// classification (retried), exactly like the one-shot
/// "AEX rejected PF_PARAMS_SETUP"; a report without the key at all reads as
/// success, since a failure is always reported through the `error` status
/// the broker maps to `InspectError`. Either way the entry records why it
/// stayed negative (issue #1063): a `status: ok` report the host could not
/// use is otherwise indistinguishable from no report at all.
fn fill_entry_from_inspect_report(entry: &mut CacheEntry, report: &serde_json::Value) {
    let unusable = |reason: &str| {
        let mut diagnostics = serde_json::Map::new();
        diagnostics.insert("classification".to_owned(), serde_json::Value::Null);
        diagnostics.insert(
            "cluster_error_kind".to_owned(),
            "inspected_report_unusable".into(),
        );
        diagnostics.insert("reason".to_owned(), reason.into());
        inspect_report_failure_fields(&mut diagnostics, report);
        Some(serde_json::Value::Object(diagnostics))
    };
    if report
        .get("params_setup_error")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0)
        != 0
    {
        entry.failure_diagnostics = unusable("params_setup_rejected");
        return;
    }
    let params = match parameters_from_inspect_report(report) {
        Ok(params) => params,
        Err(reason) => {
            entry.failure_diagnostics = unusable(&reason);
            return;
        }
    };
    entry.out_flags2 = report
        .get("out_flags2")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    // PF_OutFlag2_SUPPORTS_SMART_RENDER = bit 10.
    entry.smart = entry.out_flags2 & (1 << 10) != 0;
    entry.params = params;
    entry.demanded_suites = demanded_suites_from_report(report);
    normalize_parameters_for_cache(&mut entry.params);
    entry.ok = true;
}

fn demanded_suites_from_report(report: &serde_json::Value) -> Vec<ProvidedSuite> {
    let report = report
        .get("final_report")
        .or_else(|| report.get("worker_report"))
        .unwrap_or(report);
    report
        .get("missing_suites")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|suite| {
            let name = suite.get("name")?.as_str()?;
            let api_version = i32::try_from(suite.get("version")?.as_i64()?).ok()?;
            (api_version > 0 && !name.is_empty()).then(|| ProvidedSuite {
                name: name.to_owned(),
                api_version,
                internal_version: 0,
            })
        })
        .collect()
}

fn merge_demanded_suites_from_report(
    demanded: &mut Vec<ProvidedSuite>,
    report: &serde_json::Value,
) -> bool {
    merge_demanded_suites(demanded, demanded_suites_from_report(report))
}

fn merge_demanded_suites(
    demanded: &mut Vec<ProvidedSuite>,
    incoming: impl IntoIterator<Item = ProvidedSuite>,
) -> bool {
    const MAX_PLUGIN_DATA_DEMANDED_SUITES: usize = 64;
    for suite in incoming {
        if demanded.iter().any(|existing| {
            existing.name == suite.name && existing.api_version == suite.api_version
        }) {
            continue;
        }
        if demanded.len() == MAX_PLUGIN_DATA_DEMANDED_SUITES {
            return false;
        }
        demanded.push(suite);
    }
    true
}

fn finish_companion_demand_probe(
    entry: &mut CacheEntry,
    probed_suites: Vec<ProvidedSuite>,
) -> bool {
    entry.demanded_suites.clear();
    if !merge_demanded_suites(&mut entry.demanded_suites, probed_suites) {
        return false;
    }
    entry.companion_demand_probe_complete = true;
    true
}

fn reject_plugin_data_bundle(entry: &mut CacheEntry) {
    entry.ok = false;
    entry.plugin_data_effect = None;
    entry.additional_effects.clear();
    entry.closure_identity = None;
}

fn confirmed_plugin_data_demands<T>(
    effects: &[(Option<PluginDataEffectSelector>, bool, u32)],
    mut provider_free: impl FnMut(
        Option<&PluginDataEffectSelector>,
        bool,
        u32,
    ) -> Option<Vec<ProvidedSuite>>,
    mut providers_for: impl FnMut(&[ProvidedSuite]) -> Option<T>,
    mut with_provider: impl FnMut(Option<&PluginDataEffectSelector>, bool, u32, T) -> bool,
) -> Option<Vec<ProvidedSuite>> {
    let mut confirmed = Vec::new();
    for (selector, smart, out_flags2) in effects {
        let candidates = provider_free(selector.as_ref(), *smart, *out_flags2)?;
        if candidates.is_empty() {
            continue;
        }
        let providers = providers_for(&candidates)?;
        if with_provider(selector.as_ref(), *smart, *out_flags2, providers)
            && !merge_demanded_suites(&mut confirmed, candidates)
        {
            return None;
        }
    }
    Some(confirmed)
}

fn companion_demand_from_probe_report(close: &serde_json::Value) -> Option<Vec<ProvidedSuite>> {
    let bounded_report = close.get("final_report").is_some_and(|report| {
        report.get("missing_suites").is_some()
            && report.get("missing_suites_truncated") == Some(&serde_json::Value::Bool(false))
    });
    bounded_report.then(|| {
        if close.get("session_clean") == Some(&serde_json::Value::Bool(true)) {
            Vec::new()
        } else {
            demanded_suites_from_report(close)
        }
    })
}

/// One unit of discovery work (issue #405): with the same worker budget as
/// before, the work items are same-closure clusters (one DiscoverySession
/// sweep) and singleton plug-ins (the legacy per-plugin inspect).
enum DiscoveryTask {
    Single(usize),
    Cluster(Vec<usize>),
}

/// What the planner needs to know about one prepared plug-in: its validated
/// in-place search-root identity, when one could be established.
struct PlannedMember {
    identity: Option<String>,
}

/// Groups prepared plug-ins by validated identity. A group larger than one
/// manifest is split into bounded clusters instead of degenerating every
/// member to a one-member session. Every validated member therefore keeps a
/// discovery-session cleanup checkpoint; only unresolved identities use the
/// one-shot path.
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
        if members.len() >= 2 {
            for chunk in members.chunks(MAX_CLUSTER_PLUGINS) {
                clustered.extend(chunk.iter().copied());
                tasks.push(DiscoveryTask::Cluster(chunk.to_vec()));
            }
        }
    }
    for (index, member) in members.iter().enumerate() {
        if clustered.contains(&index) {
            continue;
        }
        if member.identity.is_some() {
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
/// sweep the staged planner got from its many closure identities. A one-member
/// tail remains a session so the checkpoint contract does not change by size.
fn shard_in_place_clusters(tasks: Vec<DiscoveryTask>, parallelism: usize) -> Vec<DiscoveryTask> {
    let parallelism = parallelism.max(1);
    let mut sharded = Vec::with_capacity(tasks.len());
    for task in tasks {
        match task {
            DiscoveryTask::Cluster(members) if members.len() > 2 => {
                let lane_count = parallelism.min(members.len() / 2).max(1);
                let chunk_size = members.len().div_ceil(lane_count).min(DISCOVERY_SAVE_CHUNK);
                for chunk in members.chunks(chunk_size) {
                    sharded.push(DiscoveryTask::Cluster(chunk.to_vec()));
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
    override_search_dirs: Option<Vec<PathBuf>>,
) -> Vec<(PathBuf, CacheEntry)> {
    let member_count = members.len();
    // Every member shares one search-root set by construction (the in-place
    // identity), so the first member's roots stand for the cluster.
    let search_dirs =
        override_search_dirs.unwrap_or_else(|| search_roots_for(&members[0].0, &dependency.dirs));
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
        inspect_deadline: Some(CLUSTER_INSPECT_DEADLINE),
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
    let mut invalidated: Option<(u32, String, bool)> = None;
    let mut next_request_index = 0u32;
    for (index, (path, prepared)) in members.into_iter().enumerate() {
        if let Some((at_member, reason, cleanup_crash)) = &invalidated {
            let (path, entry) = if *cleanup_crash {
                // A cleanup crash consumes the worker, but does not demote
                // later members to the ordinary one-shot path (which would
                // repeat the same uncontained setdown). Give each remaining
                // member a fresh ordinary session so it must independently
                // earn its own checkpoint before the one-shot fallback.
                discover_cluster_in_place(
                    repository,
                    dependency,
                    build,
                    vec![(path, prepared)],
                    None,
                )
                .into_iter()
                .next()
                .expect("one member yields one entry")
            } else {
                fallback_members_in_place(
                    repository,
                    vec![(path, prepared)],
                    dependency,
                    *at_member,
                    reason,
                )
                .into_iter()
                .next()
                .expect("one member yields one entry")
            };
            results.push((path, entry));
            continue;
        }
        let plugin_index = index as u32;
        let request_index = next_request_index;
        next_request_index += 1;
        match session.inspect_plugin(plugin_index, request_index) {
            Ok(InspectOutcome::Inspected { report }) => {
                let mut entry = prepared.entry;
                let identities = match plugin_data_identities(&report) {
                    Ok(identities) => identities,
                    Err(()) => {
                        entry.failure_diagnostics = Some(serde_json::json!({
                            "cluster_error_kind": "inspected_report_unusable",
                            "reason": "plugin_data_inventory_invalid",
                        }));
                        results.push((path, entry));
                        continue;
                    }
                };
                if !identities.is_empty()
                    && report
                        .pointer("/plugin_data/selected_index")
                        .and_then(serde_json::Value::as_u64)
                        != Some(0)
                {
                    entry.failure_diagnostics = Some(serde_json::json!({
                        "cluster_error_kind": "inspected_report_unusable",
                        "reason": "plugin_data_default_selection_mismatch",
                    }));
                    results.push((path, entry));
                    continue;
                }
                fill_entry_from_inspect_report(&mut entry, &report);
                // A PluginData bundle is atomic: the primary report alone has
                // not earned a registerable cache entry until every advertised
                // secondary identity has been inspected and matched.
                entry.ok = false;
                entry.demanded_suites.clear();
                if !merge_demanded_suites_from_report(&mut entry.demanded_suites, &report) {
                    entry.failure_diagnostics = Some(serde_json::json!({
                        "cluster_error_kind": "inspected_report_unusable",
                        "reason": "plugin_data_suite_demand_overflow",
                    }));
                    results.push((path, entry));
                    continue;
                }
                entry.plugin_data_effect = identities.first().cloned();
                entry.additional_effects.clear();
                let mut all_effects_inspected = true;
                for identity in identities.iter().skip(1) {
                    let selector = identity.selector();
                    let outcome = session.inspect_plugin_effect(
                        plugin_index,
                        next_request_index,
                        Some(&selector),
                    );
                    next_request_index += 1;
                    let Ok(InspectOutcome::Inspected { report }) = outcome else {
                        all_effects_inspected = false;
                        break;
                    };
                    if !selected_plugin_data_identity_matches(&report, identity) {
                        all_effects_inspected = false;
                        break;
                    }
                    if !merge_demanded_suites_from_report(&mut entry.demanded_suites, &report) {
                        all_effects_inspected = false;
                        break;
                    }
                    let Ok(mut params) = parameters_from_inspect_report(&report) else {
                        all_effects_inspected = false;
                        break;
                    };
                    normalize_parameters_for_cache(&mut params);
                    let out_flags2 = report
                        .get("out_flags2")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0) as u32;
                    entry.additional_effects.push(CachedPluginDataEffect {
                        identity: identity.clone(),
                        smart: out_flags2 & (1 << 10) != 0,
                        out_flags2,
                        params,
                        registered_name: None,
                    });
                }
                if !all_effects_inspected {
                    reject_plugin_data_bundle(&mut entry);
                    entry.failure_diagnostics = Some(serde_json::json!({
                        "cluster_error_kind": "inspected_report_unusable",
                        "reason": "plugin_data_effect_inspection_failed",
                    }));
                    results.push((path, entry));
                    continue;
                }
                if !entry.additional_effects.is_empty() {
                    entry.closure_identity = None;
                }
                entry.ok = true;
                results.push((path, entry));
            }
            Ok(InspectOutcome::InspectError { error_kind, report }) => {
                if error_kind == "load_failed" {
                    match registered_runtime_retry_roots(&path, &search_dirs, |basenames| {
                        cached_matching_registered_runtime_roots(
                            &path,
                            &prepared.entry.sha,
                            basenames,
                        )
                    }) {
                        RuntimeRootResolution::Resolved(retry_roots) => {
                            let mut prepared = prepared;
                            prepared.entry.closure.roots = retry_roots
                                .iter()
                                .map(|root| root.to_string_lossy().into_owned())
                                .collect();
                            prepared.entry.closure_identity = Some(in_place_identity(&retry_roots));
                            let retried = discover_cluster_in_place(
                                repository,
                                dependency,
                                build,
                                vec![(path, prepared)],
                                Some(retry_roots),
                            )
                            .into_iter()
                            .next()
                            .expect("one member yields one entry");
                            results.push(retried);
                            continue;
                        }
                        failure @ (RuntimeRootResolution::Ambiguous { .. }
                        | RuntimeRootResolution::CapacityExceeded { .. }
                        | RuntimeRootResolution::DiagnosticsTruncated) => {
                            let mut entry = prepared.entry;
                            let error = runtime_root_resolution_error(&failure);
                            entry.failure_classification =
                                inspection_failure_classification(&error);
                            entry.failure_diagnostics = inspection_failure_diagnostics(&error);
                            results.push((path, entry));
                            continue;
                        }
                        RuntimeRootResolution::Unresolved => {}
                    }
                }
                let mut entry = prepared.entry;
                let exit_code = match error_kind.as_str() {
                    "load_failed" => Some(11),
                    "entrypoint_unresolved" => Some(12),
                    "selector_error" => Some(20),
                    _ => None,
                };
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
                    // The inspect column ran but a selector refused
                    // (GLOBAL_SETUP / PARAMS_SETUP / GLOBAL_SETDOWN nonzero,
                    // or the parameter-count contract failed): the one-shot
                    // worker exits 20 for the same outcome and the broker
                    // classifies that `nonzero_exit`, so the session path
                    // converges the same way (issue #1063; the former `None`
                    // here left the sweep with `not_discovered:unknown` and no
                    // diagnostics at all). Consequence, accepted for parity
                    // with the one-shot path: `keep_best` treats `nonzero_exit`
                    // as deterministic, so an `ok && stale` entry re-checked
                    // through a session that answers `selector_error` converges
                    // to negative. A session-path selector error can depend on
                    // the members before it (the #1063 U.dll case, fixed in the
                    // worker); such a negative is re-discovered when the host
                    // build, the bytes, or the resolved closure change, exactly
                    // as a one-shot exit-20 negative is.
                    "selector_error" => Some("nonzero_exit".to_owned()),
                    _ => None,
                };
                let plugin_kind = report
                    .as_ref()
                    .and_then(|report| report.get("plugin_kind"))
                    .and_then(serde_json::Value::as_str)
                    .filter(|kind| {
                        matches!(
                            *kind,
                            "aegp_candidate" | "invalid_pipl" | "unknown_no_effect_entrypoint"
                        )
                    });
                if plugin_kind == Some("aegp_candidate") {
                    entry = finish_aegp_discovery(repository, &path, entry, &search_dirs);
                    results.push((path, entry));
                    continue;
                }
                // Every session-path failure carries diagnostics, classified
                // or not: an unclassified `identity_changed` still says why
                // the entry is being re-discovered, and a `selector_error`
                // carries the worker's partial report so the failing selector
                // and its error code are legible without a one-shot re-run.
                entry.failure_diagnostics = Some(cluster_inspect_error_diagnostics(
                    entry.failure_classification.as_deref(),
                    &error_kind,
                    exit_code,
                    plugin_kind,
                    report.as_ref(),
                ));
                results.push((path, entry));
            }
            Ok(InspectOutcome::CleanupCrashCheckpoint { authorization }) => {
                let mut entry = prepared.entry;
                match inspect_experimental_cleanup_contained_in_place(authorization, repository) {
                    Ok((parameters, diagnostics)) => {
                        // The fallback is authorized by the authenticated
                        // in-flight checkpoint, but only its independently
                        // recomputed and fully validated report is accepted.
                        // Retain generic evidence that cleanup was contained;
                        // neither the checkpoint alone nor partial worker
                        // output can make discovery succeed.
                        entry.out_flags2 = diagnostics
                            .get("advertised_out_flags2")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0) as u32;
                        entry.smart = entry.out_flags2 & (1 << 10) != 0;
                        entry.params = parameters;
                        normalize_parameters_for_cache(&mut entry.params);
                        entry.ok = true;
                        entry.failure_diagnostics = Some(serde_json::json!({
                            "classification": "cleanup_crash_contained",
                            "fallback_status": diagnostics.get("inspection_status"),
                        }));
                        results.push((path, entry));
                    }
                    Err(error) => {
                        entry.failure_classification =
                            Some("cleanup_contained_retry_failed".to_owned());
                        entry.failure_diagnostics = Some(serde_json::json!({
                            "classification": "cleanup_contained_retry_failed",
                            "error": error.to_string(),
                        }));
                        results.push((path, entry));
                    }
                }
                invalidated = Some((
                    request_index,
                    "worker crashed during GLOBAL_SETDOWN after an authenticated checkpoint"
                        .to_owned(),
                    true,
                ));
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
                invalidated = Some((request_index, reason, false));
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

/// Whether a SmartFX advertisement is internally usable for render routing.
/// Adobe documents MUTABLE_RENDER_SEQUENCE_DATA_SLOWER as the opt-in escape
/// hatch for a threaded renderer. A mutable-only advertisement has no render
/// thread contract to clone, and old BCC3DO effects return
/// PF_Interrupt_CANCEL immediately after their first Smart input checkout on
/// that path. Select their supported classic path up front; this is capability
/// negotiation, not an error fallback.
#[doc(hidden)]
pub fn smart_render_route_supported(advertised_smart: bool, out_flags2: u32) -> bool {
    if !advertised_smart {
        return false;
    }
    // Preserve pre-field cache entries until normal background re-verification
    // records the exact flags. Zero cannot be a current Smart advertisement.
    if out_flags2 == 0 {
        return true;
    }
    const THREADED: u32 = 1 << 27;
    const MUTABLE: u32 = 1 << 28;
    out_flags2 & MUTABLE == 0 || out_flags2 & THREADED != 0
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
    pub plugin_kind: DiscoveredPluginKind,
    pub provided_suites: Vec<ProvidedSuite>,
    pub demanded_suites: Vec<ProvidedSuite>,
    pub companion_demand_probe_complete: bool,
    pub sha256: String,
    pub byte_size: u64,
    /// `PF_OutFlag2_SUPPORTS_SMART_RENDER`; which render path a session opens on.
    pub smart: bool,
    pub out_flags2: u32,
    /// The PiPL `catg` property (issue #871), read from the bytes without loading.
    pub category: Option<String>,
    pub parameters: Vec<InteractiveParameter>,
    /// The in-place DLL search roots, in resolution order.
    pub search_roots: Vec<PathBuf>,
    /// SHA-256 of the resolved dependency-closure identity used by shipping
    /// cluster pooling. The raw identity contains local absolute paths, so the
    /// diagnostic boundary exposes equality evidence without those paths.
    pub closure_identity_sha256: Option<String>,
    pub failure_classification: Option<String>,
    pub failure_diagnostics: Option<serde_json::Value>,
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
    discover_records_for_diagnostics_with_progress(repository, paths, dependency_dirs, |_| {})
}

/// Diagnostic entry point with task-completion evidence. The callback runs
/// after each shipping discovery task has produced its complete batch (one
/// singleton or one cluster), so a long corpus pass can persist progress
/// without changing clustering, worker deadlines, or failure classification.
#[doc(hidden)]
pub fn discover_records_for_diagnostics_with_progress(
    repository: &Path,
    paths: &[PathBuf],
    dependency_dirs: Vec<PathBuf>,
    on_completed: impl Fn(Vec<DiagnosticDiscovery>) + Sync,
) -> Vec<DiagnosticDiscovery> {
    let dependency = DependencyConfig {
        dirs: dependency_dirs,
        module_limit: None,
        byte_limit: None,
    };
    let build = build_fingerprint(repository, &dependency);
    let report_completed = |completed: &[(PathBuf, CacheEntry)]| {
        on_completed(
            completed
                .iter()
                .cloned()
                .map(|(path, entry)| diagnostic_discovery(path, entry))
                .collect(),
        );
    };
    let discovered =
        discover_all_with_progress(repository, paths, &dependency, build, &report_completed);
    // Diagnostic/shipping CLI callers do not have the persistent UI cache
    // orchestrator. Complete the same demand phase once, after their entire
    // selected discovery set is available, so provider/effect chunk ordering
    // cannot change the result.
    let mut combined: HashMap<String, CacheEntry> = discovered
        .into_iter()
        .map(|(path, entry)| (path.to_string_lossy().into_owned(), entry))
        .collect();
    let _ = complete_companion_demand_probes(repository, &mut combined);
    combined
        .into_iter()
        .map(|(path, entry)| diagnostic_discovery(PathBuf::from(path), entry))
        .collect()
}

fn diagnostic_discovery(path: PathBuf, entry: CacheEntry) -> DiagnosticDiscovery {
    DiagnosticDiscovery {
        path,
        ok: entry.ok,
        plugin_kind: entry.plugin_kind,
        provided_suites: entry.provided_suites,
        demanded_suites: entry.demanded_suites,
        companion_demand_probe_complete: entry.companion_demand_probe_complete,
        sha256: entry.sha,
        byte_size: entry.len,
        smart: entry.smart,
        out_flags2: entry.out_flags2,
        category: entry.category,
        parameters: entry.params,
        search_roots: entry.closure.roots.iter().map(PathBuf::from).collect(),
        closure_identity_sha256: entry
            .closure_identity
            .as_deref()
            .map(|identity| hex_lower(&Sha256::digest(identity.as_bytes()))),
        failure_classification: entry.failure_classification,
        failure_diagnostics: entry.failure_diagnostics,
        cluster_fallback: entry
            .cluster_fallback
            .map(|fallback| format!("{}/{}", fallback.reason, fallback.resolution)),
    }
}

/// A panic in any task (arbitrary third-party AEX) is caught and turned into
/// negative entries, so one bad plug-in cannot abort the process by
/// unwinding out of the scoped thread.
#[cfg(test)]
fn discover_all(
    repository: &Path,
    paths: &[PathBuf],
    dependency: &DependencyConfig,
    build: BuildFingerprint,
) -> Vec<(PathBuf, CacheEntry)> {
    discover_all_with_progress(repository, paths, dependency, build, &|_| {})
}

fn discover_all_with_progress(
    repository: &Path,
    paths: &[PathBuf],
    dependency: &DependencyConfig,
    build: BuildFingerprint,
    on_completed: &(dyn Fn(&[(PathBuf, CacheEntry)]) + Sync),
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
            slot.as_ref()
                .map_or(PlannedMember { identity: None }, |(_, prepared)| {
                    PlannedMember {
                        identity: prepared.identity.clone(),
                    }
                })
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
                            let completed = [(plugin, entry)];
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                on_completed(&completed);
                            }));
                            if let Ok(mut results) = results.lock() {
                                results.extend(completed);
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
                            let member_paths: Vec<PathBuf> =
                                members.iter().map(|(path, _)| path.clone()).collect();
                            let cluster_results =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    discover_cluster_in_place(
                                        repository, dependency, build, members, None,
                                    )
                                }))
                                .unwrap_or_else(|_| {
                                    member_paths
                                        .into_iter()
                                        .map(|path| (path.clone(), negative_entry(&path, build)))
                                        .collect()
                                });
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                on_completed(&cluster_results);
                            }));
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

/// Completes provider-free demand probes against the combined discovery cache.
/// This deliberately runs after all discovery chunks have been merged: an
/// effect and its sibling AEGP provider may land on opposite chunk boundaries.
/// Returns whether any cache entry changed.
fn complete_companion_demand_probes(
    repository: &Path,
    completed: &mut HashMap<String, CacheEntry>,
) -> bool {
    let targets = companion_demand_probe_targets(completed);
    let mut changed = false;
    for path in targets {
        if DISCOVERY_SHUTDOWN.load(Ordering::Relaxed) {
            break;
        }
        let Some(entry) = completed.get(&path).cloned() else {
            continue;
        };
        if !entry.ok
            || entry.plugin_kind != DiscoveredPluginKind::Effect
            || entry.companion_demand_probe_complete
        {
            continue;
        }
        let roots: Vec<PathBuf> = entry.closure.roots.iter().map(PathBuf::from).collect();
        let mut effects = vec![(None, entry.smart, entry.out_flags2)];
        effects.extend(entry.additional_effects.iter().map(|effect| {
            (
                Some(effect.identity.selector()),
                effect.smart,
                effect.out_flags2,
            )
        }));
        let run_probe = |selector: Option<&PluginDataEffectSelector>,
                         smart: bool,
                         out_flags2: u32,
                         companions| {
            let request = SessionOpenRequest {
                repository,
                plugin_path: Path::new(&path),
                plugin_sha256: &entry.sha,
                parameters: None,
                parameter_animation: None,
                aux_manifest: None,
                world_dump_dir: None,
                output_checksum_detail: false,
                mask_trailer: None,
                spatial_trailer: None,
                render_environment_trailer: None,
                audio_trailer: None,
                alpha_as_coverage_params: &[],
                conformance_render_settings: None,
                layers: &[],
                dependencies: Vec::new(),
                companions,
                dependency_search_dirs: roots.clone(),
                width: 1,
                height: 1,
                pixel_format: RenderPixelFormat::Argb8,
                time_step: 1,
                total_time: 1,
                time_scale: 1,
                frame_deadline: Duration::from_secs(5),
                smart: smart_render_route_supported(smart, out_flags2),
                gpu_backend: RenderGpuBackend::Auto,
                gpu_runtime_policy: None,
                payload_override: None,
                launch_environment: Default::default(),
            };
            match selector {
                Some(selector) => RenderSession::open_plugin_data_effect(request, selector),
                None => RenderSession::open(request),
            }
        };
        let confirmed_suites = confirmed_plugin_data_demands(
            &effects,
            |selector, smart, out_flags2| {
                let mut session = run_probe(selector, smart, out_flags2, Vec::new()).ok()?;
                let _ = session.render_frame(0, 0, &[255, 0, 0, 0]);
                companion_demand_from_probe_report(&session.close())
            },
            |candidate_suites| {
                let mut candidate_cache = completed.clone();
                let candidate = candidate_cache.get_mut(&path)?;
                candidate.demanded_suites.clear();
                merge_demanded_suites(
                    &mut candidate.demanded_suites,
                    candidate_suites.iter().cloned(),
                )
                .then_some(())?;
                candidate.companion_demand_probe_complete = true;
                companion_providers_for(Path::new(&path), &candidate_cache).ok()
            },
            |selector, smart, out_flags2, companions| {
                let Ok(mut session) = run_probe(selector, smart, out_flags2, companions) else {
                    return false;
                };
                let _ = session.render_frame(0, 0, &[255, 0, 0, 0]);
                session.close().get("session_clean") == Some(&serde_json::Value::Bool(true))
            },
        );
        if let Some(confirmed_suites) = confirmed_suites
            && let Some(entry) = completed.get_mut(&path)
            && finish_companion_demand_probe(entry, confirmed_suites)
        {
            changed = true;
        }
    }
    changed
}

fn companion_demand_probe_targets(completed: &HashMap<String, CacheEntry>) -> Vec<String> {
    let provider_parents: std::collections::HashSet<PathBuf> = completed
        .iter()
        .filter(|(_, entry)| {
            entry.ok
                && entry.plugin_kind == DiscoveredPluginKind::Aegp
                && !entry.provided_suites.is_empty()
        })
        .filter_map(|(path, _)| Path::new(path).parent().map(Path::to_path_buf))
        .collect();
    completed
        .iter()
        .filter(|(path, entry)| {
            entry.ok
                && entry.plugin_kind == DiscoveredPluginKind::Effect
                && !entry.companion_demand_probe_complete
                && Path::new(path)
                    .parent()
                    .is_some_and(|parent| provider_parents.contains(parent))
        })
        .map(|(path, _)| path.clone())
        .collect()
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
/// The final numeric pass is global, so a plug-in whose own stem already reads
/// like a generated name (`Threshold (Effects).aex`) can be renamed by an
/// unrelated collision too.
#[cfg(test)]
fn unique_filter_names(plugins: &[PathBuf], also_known: &[PathBuf]) -> Vec<String> {
    stable_filter_names(plugins, also_known, &vec![None; plugins.len()])
}

fn stable_filter_names(
    plugins: &[PathBuf],
    also_known: &[PathBuf],
    remembered: &[Option<String>],
) -> Vec<String> {
    debug_assert_eq!(plugins.len(), remembered.len());
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
    let mut names = vec![None; plugins.len()];
    for (index, name) in remembered.iter().enumerate() {
        let Some(name) = name.as_deref().filter(|name| valid_registered_name(name)) else {
            continue;
        };
        if used.insert(name.to_lowercase()) {
            names[index] = Some(name.to_owned());
        }
    }
    plugins
        .iter()
        .enumerate()
        .map(|(index, plugin)| {
            if let Some(name) = names[index].take() {
                return name;
            }
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

fn valid_registered_name(name: &str) -> bool {
    !name.is_empty() && name.encode_utf16().count() <= 255 && !name.chars().any(char::is_control)
}

fn bounded_filter_name(name: &str) -> String {
    let mut result = String::new();
    let mut units = 0usize;
    for ch in name.chars() {
        let width = ch.len_utf16();
        if units + width > 255 {
            break;
        }
        result.push(ch);
        units += width;
    }
    result
}

fn remember_secondary_filter_names(
    cache: &mut HashMap<String, CacheEntry>,
    names: &HashMap<(String, u32), String>,
) -> bool {
    let mut changed = false;
    for ((plugin, index), name) in names {
        let Some(effect) = cache.get_mut(plugin).and_then(|entry| {
            entry
                .additional_effects
                .iter_mut()
                .find(|effect| effect.identity.index == *index)
        }) else {
            continue;
        };
        if effect.registered_name.as_deref() != Some(name) {
            effect.registered_name = Some(name.clone());
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
fn remembered_filter_names(
    plugins: &[PathBuf],
    cache: &HashMap<String, CacheEntry>,
) -> Vec<Option<String>> {
    plugins
        .iter()
        .map(|plugin| {
            let key = plugin.to_string_lossy();
            cache
                .get(key.as_ref())
                .or_else(|| {
                    let folded = key.to_lowercase();
                    cache.iter().find_map(|(cached_key, entry)| {
                        let targets_plugin = cached_key.to_lowercase() == folded
                            || entry
                                .alias_target
                                .as_deref()
                                .is_some_and(|target| target.to_lowercase() == folded);
                        targets_plugin.then_some(entry)
                    })
                })
                .and_then(|entry| entry.registered_name.clone())
        })
        .collect()
}

fn remember_filter_names(
    cache: &mut HashMap<String, CacheEntry>,
    plugins: &[PathBuf],
    names: &[String],
) -> bool {
    let mut changed = false;
    for (plugin, name) in plugins.iter().zip(names) {
        let Some(entry) = cache.get_mut(plugin.to_string_lossy().as_ref()) else {
            continue;
        };
        if is_registerable_effect(entry) && entry.registered_name.as_deref() != Some(name) {
            entry.registered_name = Some(name.clone());
            changed = true;
        }
    }
    changed
}

/// The plug-in's file stem, or `AEX` when it has none or is not UTF-8.
fn filter_stem(plugin: &Path) -> &str {
    plugin
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("AEX")
}

/// Computes the exact initial menu label handed to AviUtl2. Keeping the
/// PiPL/PluginData category, bundled-effect fallback, and configured language
/// in one planner prevents discovery and registration from drifting apart.
fn registration_label(plugin: &Path, entry: &CacheEntry, japanese_categories: bool) -> String {
    filter_label(
        entry
            .category
            .as_deref()
            .or_else(|| ae_builtin_category(filter_stem(plugin))),
        japanese_categories,
    )
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
    companions: Vec<ApprovedCompanion>,
    name: &str,
    plugin_data_selector: Option<PluginDataEffectSelector>,
    japanese_categories: bool,
) {
    let mut resolved_dependency = dependency.clone();
    if !entry.closure.roots.is_empty() {
        resolved_dependency.dirs = entry.closure.roots.iter().map(PathBuf::from).collect();
    }
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
        dependency: resolved_dependency,
        sha: entry.sha.clone(),
        smart: smart_render_route_supported(entry.smart, entry.out_flags2),
        plugin_data_selector,
        companions: companions.clone(),
        closure_identity: entry.closure_identity.clone(),
        // From the raw discovery parameters, NOT from `defaults`: `build_item`
        // maps only value-carrying kinds (float/integer/color) into config
        // items, so a layer parameter never reaches `defaults`.
        layer_slots: layer_slots_of(&entry.params),
        defaults: defaults.clone(),
        readers,
        sessions: Mutex::new(HashMap::new()),
        classic_fallbacks: Mutex::new(HashMap::new()),
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
                companions,
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
        // nest under "AEXCompat" by their own PiPL category — or, for AE's
        // bundled effects, whose files carry no PiPL at all, by the shipped
        // stem table (issue #876). Initial only — once AviUtl2 has persisted
        // an effect's label in aviutl2.ini, that (user-editable) value wins
        // on every later launch.
        label: wide_leak(&registration_label(plugin, entry, japanese_categories)),
        information: wide_leak(&format!("AEXCompat multi-filter: {name}")),
        items: items.as_ptr(),
        func_proc_video: Some(func_proc_video),
        func_proc_audio: None,
    }));

    unsafe { ((*host).register_filter_plugin)(table) };
}
