#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_signature_ignores_values_but_pins_structure() {
        let parameter = |slot: u32, kind: &str, value: f64| {
            serde_json::from_value::<aexcompat_broker::image_render::InteractiveParameter>(
                serde_json::json!({
                    "slot": slot, "name": "amount", "kind": kind,
                    "minimum": 0.0, "maximum": 100.0, "value": value,
                    "choices": [], "color": [0, 0, 0, 0],
                    "components": [0.0, 0.0, 0.0], "component_count": 0,
                    "layer_path": null, "enabled": true, "visible": true,
                    "supervised": false,
                }),
            )
            .expect("parameter fixture")
        };
        // A value change is a per-frame update, never a session reopen.
        assert_eq!(
            parameter_structure_signature(&[parameter(1, "float", 1.0)]),
            parameter_structure_signature(&[parameter(1, "float", 99.0)]),
        );
        // Structure changes must produce a different key.
        assert_ne!(
            parameter_structure_signature(&[parameter(1, "float", 1.0)]),
            parameter_structure_signature(&[parameter(2, "float", 1.0)]),
        );
        assert_ne!(
            parameter_structure_signature(&[parameter(1, "float", 1.0)]),
            parameter_structure_signature(&[parameter(1, "integer", 1.0)]),
        );
    }

    #[test]
    fn resident_session_selection_rejects_unknown_or_unsupported_capabilities() {
        let smart = InspectedRenderCapability {
            smart_render_advertised: true,
            out_flags2: 1 << 10,
        };
        let auto = selected_interactive_session_selection(smart, true, false)
            .expect("advertised SmartFX selects SmartFX");
        assert_eq!(auto.path.report_name(), "smartfx");
        assert_eq!(auto.source.report_name(), "advertised_smart");
        let manual = selected_interactive_session_selection(smart, true, true)
            .expect("a valid manual SmartFX choice is retained as manual");
        assert_eq!(manual.source.report_name(), "manual_smart");
        let key = |selection| LiveSessionKey {
            plugin_sha256: "a".repeat(64),
            dependency_identities: vec![],
            parameter_signature: "[]".into(),
            selection,
            width: 16,
            height: 16,
            pixel_format: aexcompat_broker::image_render::RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 1,
            time_scale: 1,
        };
        // The only difference is the typed provenance. This must change the
        // resident key so `render_live_request` closes and reopens instead of
        // reporting a new source from a stale session.
        assert!(key(auto) != key(manual));
        assert_eq!(
            selected_interactive_session_selection(smart, false, true)
                .expect("the established GUI Classic override remains available")
                .source
                .report_name(),
            "manual_classic"
        );

        let classic = InspectedRenderCapability {
            smart_render_advertised: false,
            out_flags2: 0,
        };
        assert_eq!(
            selected_interactive_session_selection(classic, false, false)
                .expect("advertised classic selects classic")
                .source
                .report_name(),
            "advertised_classic"
        );
        assert_eq!(
            selected_interactive_session_selection(classic, true, true)
                .expect("the established GUI SmartFX override remains available")
                .source
                .report_name(),
            "manual_smart"
        );
    }

    #[test]
    fn inspection_capability_rejects_missing_malformed_and_contradictory_facts() {
        for report in [
            serde_json::json!({}),
            serde_json::json!({"worker_diagnostics":{"advertised_out_flags2":"1024","smart_render_advertised":true}}),
            serde_json::json!({"worker_diagnostics":{"advertised_out_flags2":0,"smart_render_advertised":true}}),
        ] {
            assert!(inspected_render_capability(&report).is_err(), "{report}");
        }
    }

    fn temporary_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "aexcompat-diagnostics-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn test_identity(byte: u8) -> DispatchIdentity {
        DispatchIdentity {
            sha256: format!("{byte:02x}").repeat(32),
            size: 123,
        }
    }

    fn persist_missing_suite_event(
        root: &Path,
        identity: &DispatchIdentity,
        nonce: &str,
        suites: serde_json::Value,
    ) {
        persist_diagnostic_with_nonce(
            root,
            identity,
            "render",
            false,
            "failed safely",
            &serde_json::json!({"classification":"failed", "missing_suites":suites}),
            nonce,
        )
        .unwrap();
    }

    #[test]
    fn missing_suite_aggregate_prefers_sha_coverage_and_separates_case_and_version() {
        let root = temporary_directory("aggregate-bias");
        let first = test_identity(0x10);
        let second = test_identity(0x20);
        for nonce in ["a", "b", "c"] {
            persist_missing_suite_event(
                &root,
                &first,
                nonce,
                serde_json::json!([{"name":"Repeated Suite","version":1}]),
            );
        }
        persist_missing_suite_event(
            &root,
            &first,
            "d",
            serde_json::json!([
                {"name":"Covered Suite","version":1},
                {"name":"covered suite","version":1},
                {"name":"Covered Suite","version":2}
            ]),
        );
        persist_missing_suite_event(
            &root,
            &second,
            "e",
            serde_json::json!([{"name":"Covered Suite","version":1}]),
        );
        let aggregate = aggregate_missing_suites(&root);
        assert_eq!(aggregate.top[0].name, "Covered Suite");
        assert_eq!(
            (aggregate.top[0].sha_count, aggregate.top[0].event_count),
            (2, 2)
        );
        assert!(aggregate.top.iter().any(|gap| gap.name == "covered suite"));
        assert!(
            aggregate
                .top
                .iter()
                .any(|gap| gap.name == "Covered Suite" && gap.version == 2)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_suite_aggregate_skips_corrupt_oversize_and_sha_mismatch_without_private_fields() {
        let root = temporary_directory("aggregate-bounds");
        let identity = test_identity(0x30);
        persist_missing_suite_event(
            &root,
            &identity,
            "valid",
            serde_json::json!([{"name":"PF World Suite","version":2}]),
        );
        let directory = diagnostic_directory(&root, &identity.sha256).unwrap();
        fs::write(directory.join("corrupt.local.json"), b"not-json").unwrap();
        fs::File::create(directory.join("oversize.local.json"))
            .unwrap()
            .set_len(MAX_DIAGNOSTIC_FILE_BYTES + 1)
            .unwrap();
        let mismatch = fs::read(directory.join("valid.local.json")).unwrap();
        let mut mismatch: serde_json::Value = serde_json::from_slice(&mismatch).unwrap();
        mismatch["identity"]["sha256"] = serde_json::json!("ff".repeat(32));
        fs::write(
            directory.join("mismatch.local.json"),
            serde_json::to_vec(&mismatch).unwrap(),
        )
        .unwrap();
        let aggregate = aggregate_missing_suites(&root);
        assert_eq!(aggregate.valid_failure_event_count, 1);
        assert!(aggregate.skipped_count >= 3);
        let debug = format!("{aggregate:?}");
        assert!(!debug.contains(&identity.sha256));
        assert!(!debug.contains("local.json"));
        assert!(!debug.contains("failed safely"));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn missing_suite_aggregate_rejects_reparse_sha_directories() {
        use std::os::windows::fs::symlink_dir;
        let root = temporary_directory("aggregate-reparse");
        let target = temporary_directory("aggregate-reparse-target");
        let diagnostics = root.join("target/harness-diagnostics");
        fs::create_dir_all(&diagnostics).unwrap();
        let link = diagnostics.join("ab".repeat(32));
        if symlink_dir(&target, &link).is_ok() {
            let aggregate = aggregate_missing_suites(&root);
            assert_eq!(aggregate.scanned_sha_count, 0);
            assert_eq!(aggregate.skipped_count, 1);
        }
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn adjacent_import_resolution_tracks_normal_delay_missing_present_and_system() {
        let mut adjacent = std::collections::HashMap::new();
        adjacent.insert("present.dll".into(), PathBuf::from("present.dll"));
        let mut warnings = std::collections::BTreeMap::new();
        assert!(
            resolve_adjacent_import("present.dll", ImportKind::Normal, &adjacent, &mut warnings)
                .is_some()
        );
        assert!(
            resolve_adjacent_import("missing.dll", ImportKind::Normal, &adjacent, &mut warnings)
                .is_none()
        );
        assert!(
            resolve_adjacent_import("missing.dll", ImportKind::Delay, &adjacent, &mut warnings)
                .is_none()
        );
        resolve_adjacent_import("missing.dll", ImportKind::Normal, &adjacent, &mut warnings);
        resolve_adjacent_import(
            "api-ms-win-core-file-l1-1-0.dll",
            ImportKind::Delay,
            &adjacent,
            &mut warnings,
        );
        assert_eq!(warnings.len(), 2);
        assert!(warnings.contains_key(&("missing.dll".into(), ImportKind::Normal)));
        assert!(warnings.contains_key(&("missing.dll".into(), ImportKind::Delay)));
        resolve_adjacent_import(
            "C:\\private\\secret.dll",
            ImportKind::Normal,
            &adjacent,
            &mut warnings,
        );
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn preflight_warning_event_contains_only_basenames_and_kinds() {
        let root = temporary_directory("preflight-privacy");
        let identity = test_identity(0x42);
        persist_preflight_warnings(
            &root,
            &identity,
            &[PreflightImportWarning {
                basename: "helper.dll".into(),
                kind: ImportKind::Delay,
            }],
        )
        .unwrap();
        let directory = diagnostic_directory(&root, &identity.sha256).unwrap();
        let path = fs::read_dir(directory)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(value["success"], true);
        assert_eq!(
            value["diagnostics"]["preflight_warnings"][0],
            serde_json::json!({
                "basename":"helper.dll", "kind":"delay"
            })
        );
        let diagnostics = value["diagnostics"].to_string();
        assert!(!diagnostics.contains("path"));
        assert!(!diagnostics.contains("error"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_dispatch_keeps_identity_when_selection_changes() {
        let mut app = HarnessApp::new(temporary_directory("identity-race"));
        app.selection = Some(Selection {
            path: PathBuf::from("first.aex"),
            size: 11,
            sha256: "11".repeat(32),
            profile: None,
            modified: None,
        });
        app.spawn_native("race_test", || Ok(("ok".into(), None)));
        app.selection = Some(Selection {
            path: PathBuf::from("second.aex"),
            size: 22,
            sha256: "22".repeat(32),
            profile: None,
            modified: None,
        });
        let result = app.receiver.take().unwrap().recv().unwrap();
        assert_eq!(
            result.identity,
            Some(DispatchIdentity {
                sha256: "11".repeat(32),
                size: 11
            })
        );
        assert_eq!(result.operation.as_deref(), Some("race_test"));
    }

    #[test]
    fn diagnostic_dto_is_private_bounded_and_collision_safe() {
        let root = temporary_directory("privacy");
        let identity = test_identity(0x33);
        let secret = "C:\\Users\\private\\effect.aex RAW_STDERR image-pixels ";
        assert_eq!(diagnostic_summary(false, secret), "failed safely");
        let summary = secret.to_owned();
        let path = persist_diagnostic_with_nonce(
            &root,
            &identity,
            "render_image",
            false,
            &summary,
            &diagnostic_details(false, secret),
            "same",
        )
        .unwrap();
        assert!(
            persist_diagnostic_with_nonce(
                &root,
                &identity,
                "render_image",
                true,
                "new",
                &serde_json::json!({"classification":"completed"}),
                "same"
            )
            .is_err()
        );
        let bytes = fs::read(path).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(!text.contains("Users"));
        assert!(!text.contains("RAW_STDERR"));
        assert!(!text.contains("image-pixels"));
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["summary"], "redacted");
        let keys = value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            keys,
            [
                "identity",
                "diagnostics",
                "operation",
                "schema",
                "success",
                "summary",
                "timestamp",
                "version"
            ]
            .into_iter()
            .map(str::to_owned)
            .collect()
        );
        assert!(value["summary"].as_str().unwrap().len() <= MAX_DIAGNOSTIC_SUMMARY_BYTES);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn diagnostic_reader_ignores_corrupt_oversize_sha_mismatch_and_temp() {
        let root = temporary_directory("reader");
        let identity = test_identity(0x44);
        let directory = diagnostic_directory(&root, &identity.sha256).unwrap();
        fs::create_dir_all(&directory).unwrap();
        persist_diagnostic_with_nonce(
            &root,
            &identity,
            "valid",
            true,
            "valid summary",
            &serde_json::json!({"classification":"completed"}),
            "001",
        )
        .unwrap();
        fs::write(directory.join("002.local.json"), b"not-json").unwrap();
        fs::write(
            directory.join("003.local.json"),
            vec![b'x'; MAX_DIAGNOSTIC_FILE_BYTES as usize + 1],
        )
        .unwrap();
        let mismatch = serde_json::json!({"schema":DIAGNOSTIC_SCHEMA,"version":DIAGNOSTIC_VERSION,
            "identity":{"sha256":"55".repeat(32),"size":1},"summary":"wrong"});
        fs::write(
            directory.join("004.local.json"),
            serde_json::to_vec(&mismatch).unwrap(),
        )
        .unwrap();
        fs::write(directory.join("005.tmp"), b"ignored").unwrap();
        let history = load_diagnostic_history(&root, &identity.sha256);
        assert_eq!(
            history,
            DiagnosticHistory {
                count: 1,
                latest: Some("valid summary".into())
            }
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn diagnostic_details_preserve_bounded_compatibility_keys() {
        let body = concat!(
            r#"failed: diagnostics={"classification":"nonzero_exit","failure_stage":"render","exit_code":7,"missing_suites":[{"name":"PF World Suite","version":2}]}, report="#,
            r#"{"last_seh_selector":"RENDER","last_seh_error":512,"last_seh_exception_code":3221225477}"#,
        );
        let details = diagnostic_details(false, body);
        assert_eq!(details["classification"], "nonzero_exit");
        assert_eq!(details["failure_stage"], "render");
        assert_eq!(details["last_seh_selector"], "RENDER");
        assert_eq!(details["missing_suites"][0]["name"], "PF World Suite");
        assert_eq!(details["missing_suites"][0]["version"], 2);
        assert!(!details.to_string().contains("failed: diagnostics="));
    }

    fn temporary_aex(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "aexcompat-harness-{name}-{}-{}.{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            "aex"
        ))
    }

    fn temporary_png(name: &str) -> PathBuf {
        temporary_aex(name).with_extension("png")
    }

    fn parameter(slot: u32, kind: &str) -> aexcompat_broker::image_render::InteractiveParameter {
        aexcompat_broker::image_render::InteractiveParameter {
            slot,
            name: format!("Parameter {slot}"),
            kind: kind.into(),
            minimum: 0.0,
            maximum: 1.0,
            value: 0.0,
            choices: Vec::new(),
            color: [255, 0, 0, 0],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: kind == "button",
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }
    }

    #[test]
    fn layer_cli_assignments_are_multi_slot_and_fail_closed() {
        let assignments = ["2", "map.png", "9", "background.png"].map(std::ffi::OsString::from);
        let mut parameters = vec![
            parameter(1, "float"),
            parameter(2, "layer"),
            parameter(9, "layer"),
        ];
        assign_layer_paths(&mut parameters, &assignments).unwrap();
        assert_eq!(
            parameters[1].layer_path.as_deref(),
            Some(Path::new("map.png"))
        );
        assert_eq!(
            parameters[2].layer_path.as_deref(),
            Some(Path::new("background.png"))
        );

        let duplicate = ["2", "a.png", "2", "b.png"].map(std::ffi::OsString::from);
        let mut rejected = vec![parameter(2, "layer")];
        assert!(
            assign_layer_paths(&mut rejected, &duplicate)
                .unwrap_err()
                .contains("more than once")
        );
        assert!(rejected[0].layer_path.is_none());
        let wrong_type = ["1", "a.png"].map(std::ffi::OsString::from);
        assert!(
            assign_layer_paths(&mut parameters, &wrong_type)
                .unwrap_err()
                .contains("not a Layer input")
        );
        let unknown = ["77", "a.png"].map(std::ffi::OsString::from);
        assert!(
            assign_layer_paths(&mut parameters, &unknown)
                .unwrap_err()
                .contains("no parameter")
        );
    }

    #[test]
    fn typed_assignment_document_is_strict_typed_and_atomic() {
        let mut parameters = vec![
            parameter(1, "integer"),
            parameter(2, "color"),
            parameter(3, "point"),
            parameter(4, "layer"),
            parameter(5, "arbitrary_data"),
        ];
        parameters[0].maximum = 10.0;
        parameters[2].component_count = 2;
        let document = serde_json::json!({
            "schema_version": 1,
            "assignments": [
                {"slot": 1, "value": 7},
                {"slot": 2, "color": [255, 20, 40, 60]},
                {"slot": 3, "components": [320.0, 180.0]},
                {"slot": 4, "layer": "map.png"},
                {"slot": 5, "text": "value=7"}
            ]
        });
        apply_typed_assignments(&mut parameters, &document, None).unwrap();
        let default_timing = typed_request_timing(&document).unwrap();
        assert_eq!(default_timing.current_time, 0);
        assert_eq!(default_timing.time_scale, 1);
        assert_eq!(parameters[0].value, 7.0);
        assert_eq!(parameters[1].color, [255, 20, 40, 60]);
        assert_eq!(parameters[2].components[..2], [320.0, 180.0]);
        assert_eq!(
            parameters[3].layer_path.as_deref(),
            Some(Path::new("map.png"))
        );
        assert_eq!(parameters[4].debug_summary.as_deref(), Some("value=7"));
        let saved = typed_request_document(&parameters, 12, 60, 1, 600, None);
        let saved_timing = typed_request_timing(&saved).unwrap();
        assert_eq!(saved_timing.current_time, 12);
        assert_eq!(saved_timing.time_scale, 60);
        assert_eq!(saved_timing.total_time, 600);
        let mut roundtripped = vec![
            parameter(1, "integer"),
            parameter(2, "color"),
            parameter(3, "point"),
            parameter(4, "layer"),
            parameter(5, "arbitrary_data"),
        ];
        roundtripped[0].maximum = 10.0;
        roundtripped[2].component_count = 2;
        apply_typed_assignments(&mut roundtripped, &saved, None).unwrap();
        assert_eq!(
            serde_json::to_value(&roundtripped).unwrap(),
            serde_json::to_value(&parameters).unwrap()
        );

        let before = parameters.clone();
        for invalid in [
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":1,"value":3},{"slot":1,"value":4}
            ]}),
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":1,"value":3,"unknown":true}
            ]}),
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":2,"value":3}
            ]}),
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":1,"value":11}
            ]}),
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":5,"text":""}
            ]}),
        ] {
            assert!(apply_typed_assignments(&mut parameters, &invalid, None).is_err());
            assert_eq!(
                serde_json::to_value(&parameters).unwrap(),
                serde_json::to_value(&before).unwrap()
            );
        }

        let timed = serde_json::json!({
            "schema_version": 1,
            "timing": {"frame": 30, "fps": 24},
            "assignments": []
        });
        let timing = typed_request_timing(&timed).unwrap();
        assert_eq!(timing.current_time, 30);
        assert_eq!(timing.time_step, 1);
        assert_eq!(timing.total_time, 31);
        assert_eq!(timing.time_scale, 24);
        let duration_timing = typed_request_timing(&serde_json::json!({
            "timing":{"frame":30,"fps":24,"duration_frames":240}
        }))
        .unwrap();
        assert_eq!(duration_timing.total_time, 240);
        let fractional_timing = typed_request_timing(&serde_json::json!({
            "timing":{
                "frame":30,"time_scale":30000,"time_step":1001,"duration_frames":300
            }
        }))
        .unwrap();
        assert_eq!(fractional_timing.current_time, 30_030);
        assert_eq!(fractional_timing.time_step, 1_001);
        assert_eq!(fractional_timing.total_time, 300_300);
        assert_eq!(fractional_timing.time_scale, 30_000);
        for invalid_timing in [
            serde_json::json!({"timing":{"frame":-1,"fps":30}}),
            serde_json::json!({"timing":{"frame":1,"fps":0}}),
            serde_json::json!({"timing":{"frame":1,"fps":30,"extra":true}}),
            serde_json::json!({"timing":{"frame":30,"fps":30,"duration_frames":30}}),
            serde_json::json!({"timing":{"frame":1,"time_scale":30000}}),
            serde_json::json!({"timing":{"frame":1,"fps":30,"time_scale":30000,"time_step":1001}}),
            serde_json::json!({"timing":{"frame":10000000,"time_scale":1000000,"time_step":100000,"duration_frames":10000001}}),
        ] {
            assert!(typed_request_timing(&invalid_timing).is_err());
        }
    }

    #[test]
    fn typed_dependencies_are_bundle_bound_and_identity_pinned() {
        let root = temporary_directory("typed-dependencies");
        let requests = root.join("requests");
        let artifacts = root.join("artifacts");
        fs::create_dir_all(&requests).unwrap();
        fs::create_dir_all(&artifacts).unwrap();
        let dependency = artifacts.join("helper.dll");
        fs::write(&dependency, b"dependency").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"dependency"));
        let document = serde_json::json!({
            "dependencies": [{
                "path": "artifacts/helper.dll",
                "sha256": digest,
                "size_bytes": 10
            }]
        });
        let approved = typed_request_dependencies(&document, &requests.join("argb8.json")).unwrap();
        assert_eq!(approved.len(), 1);
        assert_eq!(approved[0].path, dependency.canonicalize().unwrap());
        assert_eq!(approved[0].expected_size, 10);

        let escaped = serde_json::json!({
            "dependencies": [{
                "path": "../outside.dll",
                "sha256": format!("{:064x}", 0),
                "size_bytes": 0
            }]
        });
        assert!(typed_request_dependencies(&escaped, &requests.join("argb8.json")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn conformance_render_settings_are_supported_or_rejected_before_render() {
        let supported = serde_json::json!({
            "render_settings": {
                "premultiplication": "premultiplied",
                "color_management": {"enabled": false, "working_space": null},
                "linear_light": false,
                "renderer": "AEXCompat CPU"
            }
        });
        assert_eq!(
            typed_request_render_settings(&supported)
                .unwrap()
                .as_deref(),
            Some("v1|premultiplied|0|-|0|AEXCompat CPU")
        );
        for unsupported in [
            serde_json::json!({"render_settings": {"premultiplication":"straight","color_management":{"enabled":true,"working_space":null},"linear_light":false,"renderer":"AEXCompat CPU"}}),
            serde_json::json!({"render_settings": {"premultiplication":"straight","color_management":{"enabled":false,"working_space":null},"linear_light":true,"renderer":"AEXCompat CPU"}}),
            serde_json::json!({"render_settings": {"premultiplication":"straight","color_management":{"enabled":false,"working_space":null},"linear_light":false,"renderer":"GPU"}}),
        ] {
            assert!(typed_request_render_settings(&unsupported).is_err());
        }
    }

    #[test]
    fn supervised_change_applies_dynamic_ui_flags_atomically() {
        let mut parameters = vec![parameter(1, "integer"), parameter(2, "float")];
        parameters[0].supervised = true;
        let report = serde_json::json!({
            "user_changed_param_requested": true,
            "user_changed_param_error": 0,
            "parameters": [
                {"index": 1, "ui_flags": 1 << 5},
                {"index": 2, "ui_flags": 1 << 9}
            ]
        });
        assert!(apply_dynamic_ui_report(&mut parameters, &report));
        assert!(!parameters[0].enabled);
        assert!(parameters[0].visible);
        assert!(parameters[1].enabled);
        assert!(!parameters[1].visible);

        let before = serde_json::to_value(&parameters).unwrap();
        let incomplete = serde_json::json!({
            "user_changed_param_requested": true,
            "user_changed_param_error": 0,
            "parameters": [{"index": 1, "ui_flags": 0}]
        });
        assert!(!apply_dynamic_ui_report(&mut parameters, &incomplete));
        assert_eq!(serde_json::to_value(&parameters).unwrap(), before);
    }

    #[test]
    fn ae_reference_comparison_reports_exact_and_bounded_pixel_error() {
        let reference_path = temporary_png("reference");
        let exact_path = temporary_png("exact");
        let changed_path = temporary_png("changed");
        let reference =
            image::RgbaImage::from_raw(2, 1, vec![10, 20, 30, 255, 40, 50, 60, 128]).unwrap();
        reference.save(&reference_path).unwrap();
        reference.save(&exact_path).unwrap();
        let changed =
            image::RgbaImage::from_raw(2, 1, vec![10, 20, 30, 255, 40, 54, 60, 128]).unwrap();
        changed.save(&changed_path).unwrap();

        let exact = compare_images(&reference_path, &exact_path).unwrap();
        assert!(exact.exact());
        assert_eq!(exact.max_channel_error, 0);
        assert_eq!(exact.mean_absolute_error, 0.0);

        let changed = compare_images(&reference_path, &changed_path).unwrap();
        assert!(!changed.exact());
        assert_eq!(changed.differing_pixels, 1);
        assert_eq!(changed.max_channel_error, 4);
        assert_eq!(changed.mean_absolute_error, 0.5);

        for path in [reference_path, exact_path, changed_path] {
            fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn ae_reference_comparison_rejects_dimension_mismatch() {
        let reference_path = temporary_png("reference-size");
        let output_path = temporary_png("output-size");
        image::RgbaImage::new(2, 2).save(&reference_path).unwrap();
        image::RgbaImage::new(3, 2).save(&output_path).unwrap();
        let error = compare_images(&reference_path, &output_path).unwrap_err();
        assert!(error.contains("AE reference is 2x2"));
        assert!(error.contains("AEX output is 3x2"));
        fs::remove_file(reference_path).unwrap();
        fs::remove_file(output_path).unwrap();
    }

    #[test]
    fn plugin_hash_reads_bytes_and_formats_sha256() {
        let path = temporary_aex("cli-hash");
        let bytes = b"synthetic AEX bytes";
        fs::write(&path, bytes).unwrap();

        let hash = read_plugin_hash(&path).unwrap();
        assert_eq!(hash, format!("{:X}", Sha256::digest(bytes)));

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn plugin_read_diagnostic_is_structured_and_does_not_export_path() {
        let path = Path::new(r"C:privatemissing-plugin.aex");
        let error = std::io::Error::from(std::io::ErrorKind::NotFound);
        let diagnostic = plugin_read_diagnostic(path, &error);

        assert_eq!(diagnostic["schema"], DIAGNOSTIC_SCHEMA);
        assert_eq!(diagnostic["version"], DIAGNOSTIC_VERSION);
        assert_eq!(diagnostic["success"], false);
        assert_eq!(diagnostic["classification"], "input_error");
        assert_eq!(diagnostic["failure_stage"], "input_validation");
        assert_eq!(diagnostic["operation"], "read_plugin");
        assert_eq!(diagnostic["path_kind"], "aex");
        assert_eq!(diagnostic["error_kind"], "NotFound");
        assert!(!diagnostic.to_string().contains("missing-plugin.aex"));
        assert!(!diagnostic.to_string().contains(r"C:private"));
    }

    #[test]
    fn plugin_hash_rejects_missing_and_directory_paths_without_panicking() {
        let missing = temporary_aex("cli-missing");
        let missing_error = read_plugin_hash(&missing).unwrap_err();
        assert_eq!(missing_error.kind(), std::io::ErrorKind::NotFound);

        let directory = temporary_aex("cli-directory");
        fs::create_dir(&directory).unwrap();
        let directory_error = read_plugin_hash(&directory).unwrap_err();
        assert!(!directory_error.to_string().is_empty());

        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn only_exact_registered_hashes_are_recognized() {
        assert_ne!(SCATTERMAP_HASH, MASKOFFSET_HASH);
        assert_eq!(SCATTERMAP_HASH.len(), 64);
    }

    #[test]
    fn effect_diagnostics_separate_rejected_gpu_and_final_cpu_timelines() {
        let report = serde_json::json!({
            "stage": "interactive_image_render",
            "render_path": "smartfx",
            "pixel_format": "argb32f",
            "worker_classification": "ok",
            "gpu_fallback_used": true,
            "worker_diagnostics": { "stage_events": [
                { "stage": "smart_pre_render", "state": "end", "errors": { "error": 0 } },
                { "stage": "smart_render_cpu", "state": "end", "errors": { "error": 0 } }
            ]},
            "gpu_attempt": {
                "worker_classification": "nonzero_exit",
                "worker_diagnostics": {
                    "failure_stage": "gpu_device_setdown",
                    "stage_events": [
                        { "stage": "smart_render_gpu", "state": "end", "errors": { "error": 0 } },
                        { "stage": "gpu_device_setdown", "state": "end", "errors": { "error": 512 } }
                    ]
                }
            }
        });
        let diagnostics = render_diagnostics(&report).unwrap();
        assert_eq!(diagnostics.render_path, "smartfx");
        assert_eq!(diagnostics.pixel_format, "argb32f");
        assert!(diagnostics.gpu_fallback_used);
        assert_eq!(
            diagnostics.gpu_attempt_classification.as_deref(),
            Some("nonzero_exit")
        );
        assert_eq!(
            diagnostics.gpu_failure_stage.as_deref(),
            Some("gpu_device_setdown")
        );
        assert!(diagnostics.final_stages[1].starts_with("smart_render_cpu"));
        assert!(diagnostics.gpu_stages[0].starts_with("smart_render_gpu"));
        assert!(diagnostics.gpu_stages[1].contains("512"));
    }

    #[test]
    fn failed_worker_diagnostics_are_extracted_from_bounded_error_text() {
        let message = concat!(
            "isolated AEX image render failed validation: diagnostics=",
            r#"{"classification":"nonzero_exit","failure_stage":"smart_render_cpu","exit_code":22,"elapsed_ms":19,"stage_events":[{"stage":"smart_pre_render","state":"end","errors":{"error":0}},{"stage":"smart_render_cpu","state":"end","errors":{"error":25}}]}"#,
            r#", report={"smart_render_error":25}"#,
        );
        let diagnostics = failure_diagnostics(message).unwrap();
        assert_eq!(diagnostics.classification, "nonzero_exit");
        assert_eq!(
            diagnostics.failure_stage.as_deref(),
            Some("smart_render_cpu")
        );
        assert_eq!(diagnostics.exit_code, Some(22));
        assert_eq!(diagnostics.elapsed_ms, Some(19));
        assert_eq!(diagnostics.selector_error, Some(25));
        assert_eq!(diagnostics.stages.len(), 2);
        assert!(diagnostics.stages[1].contains("25"));
    }

    #[test]
    fn typed_failure_document_preserves_structured_native_evidence() {
        let message = concat!(
            r#"failed: diagnostics={"classification":"nonzero_exit","failure_stage":"render","exit_code":7,"missing_suites":[{"name":"PF World Suite","version":2}],"suite_timeline":[{"sequence":1,"operation":"acquire","name":"PF World Suite","version":2,"result":25}]}, report="#,
            r#"{"render_error":25,"smart_render_supported":true,"depth_supported":true}"#,
        );
        let document = typed_failure_document(message).expect("structured failure");
        assert_eq!(document["classification"], "nonzero_exit");
        assert_eq!(document["failure_stage"], "render");
        assert_eq!(document["render_error"], 25);
        assert_eq!(document["missing_suites"][0]["name"], "PF World Suite");
        assert_eq!(document["suite_timeline"][0]["result"], 25);
    }

    #[test]
    fn host_request_validation_failure_preserves_immutable_parameter_metadata() {
        let metadata = serde_json::json!([{
            "index": 1,
            "type": "float_slider",
            "initial_value": 25.0,
            "host_range": {"minimum": 0.0, "maximum": 100.0},
            "user_range": {"minimum": 10.0, "maximum": 90.0}
        }]);
        let document = host_request_validation_failure(&metadata);
        assert_eq!(document["classification"], "host_validation_error");
        assert_eq!(document["failure_stage"], "request_validation");
        assert_eq!(document["parameter_metadata"], metadata);
    }

    #[test]
    fn typed_failure_document_preserves_inspection_worker_evidence() {
        let message = concat!(
            "AEX parameter inspection worker failed safely: ",
            r#"{"classification":"crashed","failure_stage":"parameter_inspection","exit_code":3221225477,"plugin_kind":"unknown_no_effect_entrypoint","missing_suites":[{"name":"PF Handle Suite","version":1}]}"#,
        );
        let document = typed_failure_document(message).expect("structured inspection failure");
        assert_eq!(document["classification"], "crashed");
        assert_eq!(document["failure_stage"], "parameter_inspection");
        assert_eq!(document["exit_code"], 3221225477u64);
        assert_eq!(document["plugin_kind"], "unknown_no_effect_entrypoint");
        assert_eq!(document["missing_suites"][0]["name"], "PF Handle Suite");
    }

    #[test]
    fn cli_inspection_failure_document_preserves_diagnostics_and_redacts_paths() {
        let generic = cli_inspection_failure_document("file must have one link");
        assert_eq!(generic["classification"], "inspection_error");
        assert_eq!(generic["failure_stage"], "parameter_inspection");
        assert_eq!(generic["message"], "file must have one link");

        let structured = cli_inspection_failure_document(concat!(
            "AEX parameter inspection worker failed safely: ",
            r#"{"classification":"crashed","failure_stage":"parameter_inspection","exit_code":12}"#,
        ));
        assert_eq!(structured["classification"], "crashed");
        assert_eq!(structured["failure_stage"], "parameter_inspection");
        assert_eq!(structured["exit_code"], 12);

        let path = cli_inspection_failure_document(r#"could not open C:\private\bad.aex"#);
        assert_eq!(path["message"], "redacted");
    }

    #[test]
    fn failure_diagnostics_keep_only_bounded_suites_and_seh_fields() {
        let message = concat!(
            r#"failed: diagnostics={"classification":"crashed","missing_suites":[{"name":"PF World Suite","version":2},{"name":"PF World Suite","version":2},{"name":"C:\\private\\suite","version":1},{"name":"Bad","version":-1}]}, report="#,
            r#"{"last_seh_selector":"SMART_RENDER_GPU","last_seh_error":512,"last_seh_exception_code":3221225477}"#,
        );
        let diagnostics = failure_diagnostics(message).unwrap();
        assert_eq!(
            diagnostics.missing_suites,
            vec![MissingSuite {
                name: "PF World Suite".into(),
                version: 2
            }]
        );
        assert_eq!(
            diagnostics.last_seh_selector.as_deref(),
            Some("SMART_RENDER_GPU")
        );
        assert_eq!(diagnostics.last_seh_error, Some(512));
        assert_eq!(diagnostics.last_seh_exception_code, Some(0xC0000005));
    }

    #[test]
    fn failure_diagnostics_reject_unbounded_seh_and_suite_values() {
        let message = format!(
            "failed: diagnostics={{\"classification\":\"crashed\",\"missing_suites\":[{{\"name\":\"{}\",\"version\":1}}]}}, report={{\"last_seh_selector\":\"{}\",\"last_seh_error\":4294967296,\"last_seh_exception_code\":4294967296}}",
            "A".repeat(97),
            "A".repeat(33),
        );
        let diagnostics = failure_diagnostics(&message).unwrap();
        assert!(diagnostics.missing_suites.is_empty());
        assert_eq!(diagnostics.last_seh_selector, None);
        assert_eq!(diagnostics.last_seh_error, None);
        assert_eq!(diagnostics.last_seh_exception_code, None);
    }

    #[test]
    fn malformed_failure_diagnostics_do_not_escape_the_ui_boundary() {
        assert!(failure_diagnostics("worker report unavailable: not-json").is_none());
        assert!(failure_diagnostics("unrelated error").is_none());
    }

    #[test]
    fn invalid_smartfx_rect_is_reported_without_copying_the_full_worker_report() {
        let message = concat!(
            r#"failed: diagnostics={"classification":"nonzero_exit","failure_stage":null}, report="#,
            r#"{"result_rects_valid":false,"width":0,"height":0,"large":"payload"}"#,
        );
        let diagnostics = failure_diagnostics(message).unwrap();
        assert_eq!(
            diagnostics.failure_stage.as_deref(),
            Some("result_rect_validation")
        );
        assert_eq!(
            matrix_error_summary(message),
            "SmartFX did not return a valid result rectangle"
        );
    }

    #[test]
    fn unsupported_depth_is_not_misreported_as_a_selector_or_rect_failure() {
        let message = concat!(
            r#"failed: diagnostics={"classification":"nonzero_exit","failure_stage":"render"}, report="#,
            r#"{"depth_supported":false,"smart_render_error":-1,"result_rects_valid":false}"#,
        );
        let diagnostics = failure_diagnostics(message).unwrap();
        assert_eq!(diagnostics.classification, "unsupported_pixel_depth");
        assert_eq!(
            diagnostics.failure_stage.as_deref(),
            Some("pixel_depth_negotiation")
        );
        assert_eq!(diagnostics.selector_error, None);
        assert_eq!(
            matrix_error_summary(message),
            "AEX did not advertise support for the requested pixel depth"
        );
    }

    #[test]
    fn unsupported_smart_render_path_precedes_depth_and_selector_failures() {
        let message = concat!(
            r#"failed: diagnostics={"classification":"nonzero_exit","failure_stage":"smart_render"}, report="#,
            r#"{"smart_render_supported":false,"depth_supported":false,"smart_render_error":-1,"result_rects_valid":false}"#,
        );
        let diagnostics = failure_diagnostics(message).unwrap();
        assert_eq!(diagnostics.classification, "unsupported_render_path");
        assert_eq!(
            diagnostics.failure_stage.as_deref(),
            Some("render_path_negotiation")
        );
        assert_eq!(diagnostics.selector_error, None);
        assert_eq!(
            matrix_error_summary(message),
            "AEX did not advertise SmartFX render support"
        );
    }

    #[test]
    fn matrix_rows_preserve_success_and_failure_diagnostics() {
        let report = serde_json::json!({
            "stage": "effect_compatibility_matrix",
            "cases": [
                {"render_path":"classic","pixel_format":"argb8","passed":true,
                 "classification":"ok","output_png":"classic.png",
                 "output_relation":"pixels_changed","differing_input_pixels":42},
                {"render_path":"smartfx","pixel_format":"argb32f","passed":false,
                 "classification":"crashed","failure_stage":"smart_render_gpu",
                 "selector_error":512,"error":"GPU selector crashed"},
                {"render_path":"classic","pixel_format":"argb16","passed":false,
                 "applicable":false,"classification":"unsupported_pixel_depth",
                 "failure_stage":"pixel_depth_negotiation"}
            ]
        });
        let cases = compatibility_matrix(&report).unwrap();
        assert_eq!(cases.len(), 3);
        assert!(cases[0].passed);
        assert!(cases[0].applicable);
        assert_eq!(cases[0].output_png.as_deref(), Some("classic.png"));
        assert_eq!(cases[0].output_relation.as_deref(), Some("pixels_changed"));
        assert_eq!(cases[0].differing_input_pixels, Some(42));
        assert!(!cases[1].passed);
        assert_eq!(cases[1].classification, "crashed");
        assert_eq!(cases[1].failure_stage.as_deref(), Some("smart_render_gpu"));
        assert_eq!(cases[1].selector_error, Some(512));
        assert_eq!(cases[1].error.as_deref(), Some("GPU selector crashed"));
        assert!(!cases[2].passed);
        assert!(!cases[2].applicable);
        assert_eq!(cases[2].classification, "unsupported_pixel_depth");
    }

    #[test]
    fn rebuilt_dev_binary_is_rehashed_and_automatically_enabled() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-reload-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("reload.aex");
        let mut first_build = fs::read(std::env::current_exe().unwrap()).unwrap();
        fs::write(&path, &first_build).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let first_hash = format!("{:X}", Sha256::digest(&first_build));
        let mut app = HarnessApp::new(std::env::temp_dir());
        app.selection = Some(Selection {
            path: path.clone(),
            size: metadata.len(),
            sha256: first_hash.clone(),
            profile: None,
            modified: metadata.modified().ok(),
        });
        app.session_approved = true;
        app.trust_rebuilds = true;
        app.last_identity_check = Instant::now() - Duration::from_secs(1);

        first_build.extend_from_slice(b"second build");
        fs::write(&path, &first_build).unwrap();
        app.check_selected_identity();
        assert!(app.selection_stale);

        app.refresh_aex();
        let refreshed = app.selection.as_ref().unwrap();
        assert_ne!(refreshed.sha256, first_hash);
        assert!(!app.selection_stale);
        assert!(app.session_approved, "{} / {}", app.status, app.report);
        assert!(app.inspect_after_refresh);
        assert!(app.parameters.is_empty());
        assert!(app.preview.is_none());

        app.trust_rebuilds = false;
        first_build.extend_from_slice(b"third build");
        fs::write(&path, &first_build).unwrap();
        app.refresh_aex();
        assert!(app.session_approved);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adjacent_import_discovery_accepts_a_valid_pe_and_rejects_malformed_input() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-import-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let valid = root.join("valid.aex");
        fs::copy(std::env::current_exe().unwrap(), &valid).unwrap();
        let discovery = discover_adjacent_imports(&valid).unwrap();
        assert!(discovery.dependencies.is_empty());

        let malformed = root.join("malformed.aex");
        fs::write(&malformed, b"not a PE image").unwrap();
        assert!(
            discover_adjacent_imports(&malformed)
                .unwrap_err()
                .contains("Could not inspect PE imports")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn delay_import_table_is_bounded_terminated_and_basename_only() {
        let mut bytes = vec![0u8; 96];
        bytes[0..4].copy_from_slice(&1u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[64..75].copy_from_slice(b"helper.dll\0");
        assert_eq!(
            parse_delay_import_table(&bytes, 0, 64, 0x400000, true, |rva| {
                (rva == 0x1000).then_some(64)
            })
            .unwrap(),
            ["helper.dll"]
        );

        assert!(
            parse_delay_import_table(&bytes, 0, 63, 0x400000, true, |_| Some(64))
                .unwrap_err()
                .contains("invalid size")
        );
        assert!(
            parse_delay_import_table(&bytes, 0, 32, 0x400000, true, |_| Some(64))
                .unwrap_err()
                .contains("zero terminator")
        );

        bytes[64..75].copy_from_slice(b"..\\bad.dll\0");
        assert!(
            parse_delay_import_table(&bytes, 0, 64, 0x400000, true, |_| Some(64))
                .unwrap_err()
                .contains("DLL basename")
        );
    }

    #[test]
    fn automatic_dependency_discovery_excludes_system_names_and_oversized_pe_files() {
        assert!(is_system_import_name("api-ms-win-core-file-l1-1-0.dll"));
        if std::env::var_os("WINDIR").is_some() {
            assert!(is_system_import_name("kernel32.dll"));
        }

        let path = temporary_aex("oversized");
        fs::File::create(&path)
            .unwrap()
            .set_len(MAX_DISCOVERY_FILE_BYTES + 1)
            .unwrap();
        assert!(
            read_bounded_pe(&path)
                .unwrap_err()
                .contains("PE image size")
        );
        fs::remove_file(path).unwrap();
    }
}
