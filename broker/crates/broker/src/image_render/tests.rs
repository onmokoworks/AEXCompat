#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal facts for the public-report flattening; only the audio field
    /// varies across the audio projection tests below.
    fn audio_report_facts(audio_input_sha256: Option<&str>) -> InteractiveImageReportFacts {
        InteractiveImageReportFacts {
            plugin_id: "probe".into(),
            smart: false,
            pixel_format: RenderPixelFormat::Argb8,
            rendered_width: 4,
            rendered_height: 4,
            input_width: 4,
            input_height: 4,
            output_png: PathBuf::from("out.png"),
            timing: RenderTiming::default(),
            worker_classification: "ok".into(),
            diagnostics: json!({}),
            gpu_fallback_used: false,
            gpu_fallback_reason: None,
            gpu_attempt: None,
            secondary_layers: json!([]),
            empty_smart_result: false,
            output_raw: None,
            deep_png_output: false,
            deep_overrange_samples: None,
            world_dump_display: None,
            minidump_display: None,
            output_checksum_detail: false,
            output_origin_ok: true,
            parameter_count_ok: true,
            spatial_ok: true,
            audio_input_sha256: audio_input_sha256.map(str::to_owned),
        }
    }

    #[test]
    fn close_failure_diagnostic_names_a_bounded_invariant_without_worker_strings() {
        let close = json!({
            "invalidated": false,
            "worker": {"classification": "ok", "detail": "C:\\\\secret\\\\plugin.aex"},
            "final_report": {
                "suite_acquires": 8,
                "suite_releases": 7,
                "live_suite_lease_count": 1,
                "live_suite_leases": "C:\\\\secret\\\\plugin.aex"
            }
        });
        let diagnostic = close_failure_diagnostic(
            &close,
            crate::render_session::CloseReportInvariant::SuiteFaultObserved,
        );
        assert_eq!(
            diagnostic,
            "render session close rejected invariant=suite_fault_observed invalidated=false worker_ok=true suite_acquires=8 suite_releases=7 live_suite_lease_count=1 live_suite_reference_count=unknown"
        );
        assert!(!diagnostic.contains("secret"));
    }

    #[test]
    fn rejected_close_does_not_supply_stale_state_to_the_next_wrapper_report() {
        let rejected = json!({
            "invalidated": false,
            "worker": {"classification": "ok"},
            "final_report": {
                "status": "render_completed",
                "render_error": 0,
                "global_setdown_error": 0,
                "persistent_sequence_setup_error": 0,
                "persistent_sequence_setdown_error": 0,
                "guard_bytes_intact": true,
                "suite_leases_balanced": false,
                "suite_lease_warning": true,
                "suite_fault_observed": false,
                "suite_acquires": 8,
                "suite_releases": 7,
                "live_suite_lease_count": 1,
                "live_suite_reference_count": 1,
                "live_suite_leases": "C:\\\\secret\\\\plugin.aex",
                "handle_lifetimes_balanced": true,
                "world_lifetimes_balanced": true,
                "param_checkouts_balanced": true,
            }
        });
        assert_eq!(
            validated_wrapper_final_report(&rejected, false),
            Err(crate::render_session::CloseReportInvariant::SuiteLeaseList)
        );
        let diagnostic = close_failure_diagnostic(
            &rejected,
            crate::render_session::CloseReportInvariant::SuiteLeaseList,
        );
        assert!(!diagnostic.contains("secret"));

        let next_clean = json!({
            "invalidated": false,
            "worker": {"classification": "ok"},
            "final_report": {
                "status": "render_completed",
                "render_error": 0,
                "global_setdown_error": 0,
                "persistent_sequence_setup_error": 0,
                "persistent_sequence_setdown_error": 0,
                "guard_bytes_intact": true,
                "suite_leases_balanced": true,
                "suite_lease_warning": false,
                "suite_fault_observed": false,
                "suite_acquires": 2,
                "suite_releases": 2,
                "live_suite_lease_count": 0,
                "live_suite_leases": "",
                "handle_lifetimes_balanced": true,
                "world_lifetimes_balanced": true,
                "param_checkouts_balanced": true,
            }
        });
        let report = validated_wrapper_final_report(&next_clean, false)
            .expect("a new clean close must not inherit the rejected close");
        assert_eq!(report["live_suite_leases"], json!(""));
        assert_eq!(report["suite_lease_warning"], json!(false));
    }

    #[test]
    fn suite_call_slot_probe_keeps_shape_but_drops_raw_process_values() {
        let raw_sentinel = "0xfeedfacecafebeef";
        let worker_report = json!({
            "suite_call_slot_probe": {
                "enabled": true,
                "slot_count": 32,
                "maximum_targets": 8,
                "targets": [{
                    "name": "PF AE Private Effect Suite",
                    "version": 3,
                    "enabled": true,
                    "calls": [{
                        "slot": 7,
                        "call_count": 1,
                        "exception_code": 3762452487u64,
                        "argument_word_count": 8,
                        "nonzero_word_count": 4,
                        "registers": {
                            "rcx": "nonzero",
                            "rdx": "zero",
                            "r8": "nonzero",
                            "r9": "zero"
                        },
                        "stack": ["nonzero", "zero", "nonzero", "zero"],
                        "caller_rva": "0x0000000000001234",
                        "raw_sentinel": raw_sentinel
                    }, {
                        "slot": 8,
                        "call_count": 1,
                        "exception_code": 3762452488u64,
                        "argument_word_count": 8,
                        "nonzero_word_count": 8,
                        "registers": {
                            "rcx": raw_sentinel,
                            "rdx": raw_sentinel,
                            "r8": raw_sentinel,
                            "r9": raw_sentinel
                        },
                        "stack": [raw_sentinel, raw_sentinel, raw_sentinel, raw_sentinel],
                        "caller_rva": "0x0000000000001234"
                    }],
                    "truncated": false
                }],
                "configuration_truncated": false
            }
        });
        let mut diagnostics = json!({});
        propagate_suite_call_slot_probe(&mut diagnostics, &worker_report);
        let serialized = serde_json::to_string(&diagnostics).unwrap();
        assert!(!serialized.contains(raw_sentinel));
        assert_eq!(
            diagnostics["suite_call_slot_probe"]["targets"][0]["calls"][0]["registers"]["rcx"],
            "nonzero"
        );
        assert_eq!(
            diagnostics["suite_call_slot_probe"]["targets"][0]["calls"][0]["nonzero_word_count"],
            4
        );
        assert_eq!(
            diagnostics["suite_call_slot_probe"]["targets"][0]["calls"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            diagnostics["suite_call_slot_probe"]["targets"][0]["truncated"],
            true
        );
    }

    /// The session route synthesizes the public report from its own final
    /// report, so the audio telemetry must be projected from the same facts the
    /// one-shot uses. Before issue #339 the session passed `None` here and the
    /// whole audio block silently vanished from an audio render's report.
    #[test]
    fn audio_telemetry_is_projected_exactly_when_a_sidecar_was_supplied() {
        let worker_report = json!({
            "audio_usage_advertised": true,
            "audio_checkout_allowed": true,
            "audio_checkout_calls": 1,
            "audio_checkin_calls": 1,
            "audio_get_data_calls": 1,
            "invalid_audio_operations": 0,
            "audio_lifetimes_balanced": true,
            "last_audio_window_sample_count": 6,
        });
        let with_audio =
            build_interactive_image_report(&worker_report, audio_report_facts(Some("abc123")));
        assert_eq!(
            with_audio.get("audio_sidecar_transport"),
            Some(&json!("mono_f32le_44100"))
        );
        assert_eq!(
            with_audio.get("audio_sidecar_input_sha256"),
            Some(&json!("abc123"))
        );
        for field in [
            "audio_usage_advertised",
            "audio_checkout_allowed",
            "audio_checkout_calls",
            "audio_lifetimes_balanced",
            "last_audio_window_sample_count",
        ] {
            assert_eq!(
                with_audio.get(field),
                worker_report.get(field),
                "{field} must reach the public report"
            );
        }

        // A render without a sidecar must not grow audio keys, so the absence
        // of the block stays a reliable signal that no audio was carried.
        let without_audio =
            build_interactive_image_report(&worker_report, audio_report_facts(None));
        assert!(
            without_audio
                .as_object()
                .unwrap()
                .keys()
                .all(|key| !key.contains("audio")),
            "a render with no sidecar reported audio keys: {without_audio}"
        );
    }

    /// The audio gate rejects a plug-in that never advertised audio usage. The
    /// session route hardcoded `audio_present: false` before issue #339, which
    /// let exactly this report pass the session while the one-shot rejected it.
    #[test]
    fn the_audio_gate_rejects_an_unadvertised_plugin_when_a_sidecar_is_present() {
        let unadvertised = json!({
            "guard_bytes_intact": true,
            "render_error": 0,
            "gpu_memory_lifetimes_balanced": true,
            "pf_path_lifetimes_balanced": true,
            "pixel_format": "argb8",
            "audio_usage_advertised": false,
            "audio_checkout_allowed": false,
            "audio_source_available": true,
            "audio_lifetimes_balanced": true,
            "invalid_audio_operations": 0,
        });
        let unit = crate::render_request::RationalScale {
            numerator: 1,
            denominator: 1,
        };
        let facts = |audio_present| InteractiveGateFacts {
            smart: false,
            pixel_format: RenderPixelFormat::Argb8,
            spatial: crate::render_request::SpatialContext {
                downsample_x: unit,
                downsample_y: unit,
                pixel_aspect_ratio: unit,
                full_resolution_width: None,
                full_resolution_height: None,
                pre_effect_source_origin_x: None,
                pre_effect_source_origin_y: None,
            },
            expected_quality: 1,
            expected_field: 0,
            expected_shutter_angle: 0,
            expected_shutter_phase: 0,
            custom_ui_action: None,
            audio_present,
            interactive_parameters: None,
            classification: "ok",
            time_step: 1,
            input_width: 4,
            input_height: 4,
        };
        assert!(
            validate_interactive_worker_report(&unadvertised, &json!({}), &facts(true)).is_err(),
            "an unadvertised plug-in must not pass the gate once a sidecar is present"
        );
        assert!(
            validate_interactive_worker_report(&unadvertised, &json!({}), &facts(false)).is_ok(),
            "the same report is fine when the render carried no audio"
        );
    }

    #[test]
    fn smart_render_advertised_follows_out_flags2_bit_10() {
        assert!(!smart_render_advertised(0));
        assert!(smart_render_advertised(PF_OUTFLAG2_SUPPORTS_SMART_RENDER));
        // ntsc-rs (SmartFX-only) and the ONMK MaskOffset fixture advertise the
        // bit inside larger flag words (issue #105).
        assert!(smart_render_advertised(142_611_592));
        assert!(smart_render_advertised(525_312));
        // Every other flag set without bit 10 stays Classic.
        assert!(!smart_render_advertised(
            u64::MAX & !PF_OUTFLAG2_SUPPORTS_SMART_RENDER
        ));
    }

    #[test]
    fn conformance_alpha_mode_transforms_rgba_transports_consistently() {
        let source = [200, 100, 50, 128, 9, 8, 7, 0];
        for mode in ["straight", "premultiplied", "opaque"] {
            let mut primary = source;
            let mut secondary = source;
            let mut timed_secondary = source;
            apply_conformance_premultiplication(&mut primary, mode);
            apply_conformance_premultiplication(&mut secondary, mode);
            apply_conformance_premultiplication(&mut timed_secondary, mode);
            assert_eq!(secondary, primary);
            assert_eq!(timed_secondary, primary);
        }

        let mut premultiplied = source;
        apply_conformance_premultiplication(&mut premultiplied, "premultiplied");
        assert_eq!(premultiplied, [100, 50, 25, 128, 0, 0, 0, 0]);
        let mut opaque = source;
        apply_conformance_premultiplication(&mut opaque, "opaque");
        assert_eq!(opaque, [200, 100, 50, 255, 9, 8, 7, 255]);
    }

    #[test]
    fn bounded_decode_rejects_header_only_images() {
        let path = std::env::temp_dir().join(format!(
            "aexcompat-truncated-input-{}-{}.png",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255]))
            .save(&path)
            .unwrap();
        let complete = fs::read(&path).unwrap();
        let header_only = (33..complete.len())
            .find(|&length| {
                fs::write(&path, &complete[..length]).unwrap();
                matches!(image::image_dimensions(&path), Ok((1, 1)))
                    && decode_bounded_image(&path, "input preflight").is_err()
            })
            .expect("fixture with readable dimensions and truncated pixels");
        fs::write(&path, &complete[..header_only]).unwrap();

        assert_eq!(image::image_dimensions(&path).unwrap(), (1, 1));
        assert!(decode_bounded_image(&path, "input preflight").is_err());
        fs::remove_file(path).unwrap();
    }

    fn timed_layer(slot: u32, value: i32, scale: u32) -> TimedLayerImage {
        TimedLayerImage {
            slot,
            time: AnimationTime { value, scale },
            image_path: PathBuf::from("unused.png"),
        }
    }

    #[test]
    fn timed_layer_identities_are_slot_bound_bounded_and_rationally_unique() {
        let slots = HashSet::from([6]);
        validate_timed_layer_identities(&[timed_layer(6, 1, 2), timed_layer(6, 3, 4)], &slots)
            .unwrap();
        assert!(
            validate_timed_layer_identities(&[timed_layer(6, 1, 2), timed_layer(6, 2, 4)], &slots,)
                .is_err()
        );
        assert!(validate_timed_layer_identities(&[timed_layer(7, 1, 2)], &slots).is_err());
        assert!(validate_timed_layer_identities(&[timed_layer(6, 1, 0)], &slots).is_err());
        assert!(validate_timed_layer_identities(&vec![timed_layer(6, 1, 2); 65], &slots).is_err());
    }

    #[test]
    fn stale_transport_cleanup_removes_only_old_owned_regular_files() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-stale-transport-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let now = SystemTime::now();
        let old_time = now - Duration::from_secs(120);
        let old_names = [
            "input-123.rgba",
            "output-123.rgba",
            "audio-123.f32",
            "layer-123-0.rgba",
            "layer-session-123-0.rgba",
            "report-123.json",
            "parameter-animation-123.json",
            "aux-manifest-123.json",
            "aux-123-0.f32le",
        ];
        for name in old_names {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join(name))
                .unwrap();
            file.set_times(fs::FileTimes::new().set_modified(old_time))
                .unwrap();
        }
        let new_file = root.join("input-456.rgba");
        fs::write(&new_file, b"new").unwrap();
        let unknown = root.join("input-123.rgba.bak");
        fs::write(&unknown, b"unknown").unwrap();
        let directory = root.join("output-789.rgba");
        fs::create_dir(&directory).unwrap();

        cleanup_stale_image_transport_before(&root, now, Duration::from_secs(60)).unwrap();

        for name in old_names {
            assert!(
                !root.join(name).exists(),
                "old owned file was retained: {name}"
            );
        }
        assert!(new_file.exists());
        assert!(unknown.exists());
        assert!(directory.is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_transport_cleanup_keeps_symlinks() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-stale-transport-link-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("unknown-target");
        fs::write(&target, b"target").unwrap();
        let link = root.join("input-999.rgba");
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&target, &link).is_ok();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&target, &link).is_ok();
        #[cfg(not(any(windows, unix)))]
        let linked = false;

        cleanup_stale_image_transport_before(
            &root,
            SystemTime::now() + Duration::from_secs(120),
            Duration::from_secs(60),
        )
        .unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"target");
        if linked {
            assert!(
                fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    fn aux_fixture(root: &Path, name: &str, values: &[f32]) -> crate::render_request::AuxChannel {
        let path = root.join(name);
        let bytes = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>();
        fs::write(&path, bytes).unwrap();
        crate::render_request::AuxChannel {
            param_index: 0,
            channel: crate::render_request::AuxChannelDescriptor {
                channel_type: AUX_CHANNEL_DEPTH,
                name: "depth".into(),
                data_type: crate::render_request::AuxDataType::F32le,
                dimension: 1,
                width: values.len() as u32,
                height: 1,
                row_bytes: None,
                origin_x: 0,
                origin_y: 0,
                downsample_x: crate::render_request::RationalScale {
                    numerator: 1,
                    denominator: 1,
                },
                downsample_y: crate::render_request::RationalScale {
                    numerator: 1,
                    denominator: 1,
                },
                coordinate_space: "source_pixel".into(),
                units: "unitless".into(),
                samples: vec![crate::render_request::AuxChannelSample {
                    time: 0,
                    time_scale: 30,
                    path,
                    sampling: crate::render_request::AuxSampling::Exact,
                    interpretation: crate::render_request::AuxInterpretation::Depth,
                }],
            },
        }
    }

    #[test]
    fn aux_manifest_owns_raw_data_and_cleans_everything() {
        let repository = std::env::temp_dir().join(format!(
            "aexcompat-aux-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let transport_root = repository.join("transport");
        fs::create_dir_all(&transport_root).unwrap();
        let channel = aux_fixture(&repository, "source.f32", &[1.0, 2.5]);
        let transport = prepare_aux_transport(&repository, &[channel], &transport_root, 7)
            .unwrap()
            .unwrap();
        let manifest: Value =
            serde_json::from_slice(&fs::read(&transport.manifest_path).unwrap()).unwrap();
        assert_eq!(manifest["schema"], "aux-manifest-v1");
        assert_eq!(manifest["channels"][0]["data_type"], "f32le");
        assert_eq!(manifest["channels"][0]["samples"][0]["time_scale"], 30);
        assert_eq!(
            manifest["channels"][0]["samples"][0]["expected_byte_length"],
            8
        );
        let raw = PathBuf::from(
            manifest["channels"][0]["samples"][0]["path"]
                .as_str()
                .unwrap(),
        );
        let raw_bytes = fs::read(&raw).unwrap();
        assert_eq!(raw_bytes.len(), 8);
        assert_eq!(
            manifest["channels"][0]["samples"][0]["sha256"],
            format!("{:x}", Sha256::digest(&raw_bytes))
        );
        let manifest_path = transport.manifest_path.clone();
        drop(transport);
        assert!(!manifest_path.exists());
        assert!(!raw.exists());
        fs::remove_dir_all(repository).unwrap();
    }

    #[test]
    fn prepare_aux_transport_output_satisfies_the_session_aux_manifest_contract() {
        // #211: the length-1 session wrapper now carries aux channels through
        // the same broker-built manifest the one-shot path uses, passing its
        // path to SessionOpenRequest::aux_manifest. This guards the handoff:
        // the manifest prepare_aux_transport produces must satisfy the session's
        // `--aux-manifest-v1` precondition (render_session.rs requires an
        // absolute, existing file) and the worker's top-level manifest gate
        // (session_protocol_worker mirrors the real load_aux_manifest contract:
        // exactly {schema, nonce, channels}, the v1 schema string, a digit
        // nonce, and a non-empty channel list). A manifest that failed any of
        // these would make the wrapper's session route dead-on-arrival while the
        // one-shot route kept working, exactly the silent split #211 removes.
        let repository = std::env::temp_dir().join(format!(
            "aexcompat-aux-session-contract-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let transport_root = repository.join("transport");
        fs::create_dir_all(&transport_root).unwrap();
        let channel = aux_fixture(&repository, "depth.f32", &[0.0, 0.25, 1.0, 2.0]);
        let transport = prepare_aux_transport(&repository, &[channel], &transport_root, 42)
            .unwrap()
            .expect("aux channels present, so a manifest is produced");

        // The session gate rejects a non-absolute or missing manifest before it
        // ever launches the worker (render_session.rs `aux_manifest` handling).
        assert!(
            transport.manifest_path.is_absolute(),
            "session aux_manifest must be an absolute path"
        );
        assert!(
            transport.manifest_path.is_file(),
            "session aux_manifest must point at an existing file"
        );

        // The worker's top-level manifest gate.
        let document: Value =
            serde_json::from_slice(&fs::read(&transport.manifest_path).unwrap()).unwrap();
        let object = document.as_object().expect("manifest is a JSON object");
        assert_eq!(
            object.len(),
            3,
            "manifest carries exactly schema/nonce/channels"
        );
        assert_eq!(object["schema"], "aux-manifest-v1");
        let nonce = object["nonce"].as_str().expect("nonce is a string");
        assert!(
            !nonce.is_empty() && nonce.bytes().all(|byte| byte.is_ascii_digit()),
            "nonce is a non-empty digit string"
        );
        let channels = object["channels"].as_array().expect("channels is an array");
        assert!(
            !channels.is_empty() && channels.iter().all(Value::is_object),
            "channels is a non-empty list of objects"
        );

        drop(transport);
        fs::remove_dir_all(repository).unwrap();
    }

    // Issue #231: the worker's aux loader gates every declared path on
    // `absolute().lexically_normal() == canonical()`, and MSVC drops the `\\?\`
    // verbatim prefix in `canonical` but keeps it in `absolute`, so a manifest or
    // sidecar path carrying that prefix (as `Path::canonicalize()` produces on
    // Windows) is rejected and the render exits 3. prepare_aux_transport must
    // hand the worker plain absolute paths. This guards the de-verbatim without
    // needing the real worker; it is Windows-only because the prefix is.
    #[cfg(windows)]
    #[test]
    fn aux_transport_de_verbatims_manifest_and_sidecar_paths() {
        let repository = std::env::temp_dir().join(format!(
            "aexcompat-aux-verbatim-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&repository).unwrap();
        // canonicalize() yields the `\\?\C:\...` verbatim form that reproduced
        // the bug; derive the transport root from it exactly as the render path
        // does (repository.join("target/image-transport")).
        let canonical_repository = repository.canonicalize().unwrap();
        assert!(
            canonical_repository
                .as_os_str()
                .to_string_lossy()
                .starts_with(r"\\?\"),
            "canonicalize() is expected to produce a verbatim root on Windows"
        );
        let transport_root = canonical_repository.join("transport");
        fs::create_dir_all(&transport_root).unwrap();
        let channel = aux_fixture(&canonical_repository, "depth.f32", &[0.0, 0.5, 1.0, 2.0]);
        let transport =
            prepare_aux_transport(&canonical_repository, &[channel], &transport_root, 231)
                .unwrap()
                .expect("aux channels present, so a manifest is produced");

        assert!(
            !transport
                .manifest_path
                .as_os_str()
                .to_string_lossy()
                .starts_with(r"\\?\"),
            "manifest path handed to the worker must not carry the \\?\\ prefix"
        );
        let document: Value =
            serde_json::from_slice(&fs::read(&transport.manifest_path).unwrap()).unwrap();
        let sample_path = document["channels"][0]["samples"][0]["path"]
            .as_str()
            .expect("sample path is a string");
        assert!(
            !sample_path.starts_with(r"\\?\"),
            "sidecar path written into the manifest must not carry the \\?\\ prefix, got {sample_path}"
        );
        // The de-verbatimed manifest path still resolves to the written file.
        assert!(transport.manifest_path.is_file());
        assert!(Path::new(sample_path).is_file());

        drop(transport);
        fs::remove_dir_all(repository).unwrap();
    }

    #[test]
    fn aux_transport_rejects_nonfinite_duplicate_time_and_path_aliases() {
        let repository = std::env::temp_dir().join(format!(
            "aexcompat-aux-reject-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let transport_root = repository.join("transport");
        fs::create_dir_all(&transport_root).unwrap();
        let nonfinite = aux_fixture(&repository, "nan.f32", &[f32::NAN]);
        assert!(prepare_aux_transport(&repository, &[nonfinite], &transport_root, 1).is_err());

        let mut duplicate = aux_fixture(&repository, "valid.f32", &[1.0]);
        duplicate
            .channel
            .samples
            .push(duplicate.channel.samples[0].clone());
        assert!(prepare_aux_transport(&repository, &[duplicate], &transport_root, 2).is_err());

        let mut wrong_dimension = aux_fixture(&repository, "wrong-dimension.f32", &[1.0]);
        wrong_dimension.channel.dimension = 2;
        assert!(
            prepare_aux_transport(&repository, &[wrong_dimension], &transport_root, 4).is_err()
        );

        let first = aux_fixture(&repository, "alias.f32", &[1.0]);
        let mut second = first.clone();
        second.channel.name = "other".into();
        assert!(prepare_aux_transport(&repository, &[first, second], &transport_root, 3).is_err());
        assert!(fs::read_dir(&transport_root).unwrap().next().is_none());
        fs::remove_dir_all(repository).unwrap();
    }

    fn crashkit_parameters(mode: f64) -> Vec<InteractiveParameter> {
        [
            (1, "Fault mode", mode, 1.0, 5.0),
            (2, "Fault stage", 2.0, 1.0, 2.0),
        ]
        .into_iter()
        .map(
            |(slot, name, value, minimum, maximum)| InteractiveParameter {
                slot,
                name: name.into(),
                kind: "integer".into(),
                minimum,
                maximum,
                value,
                choices: vec![],
                color: [0; 4],
                components: [0.0; 3],
                component_count: 0,
                layer_path: None,
                enabled: true,
                visible: true,
                supervised: false,
                debug_summary: None,
                custom_ui_events: 4,
                control_size: [0, 0],
            },
        )
        .collect()
    }

    #[cfg(windows)]
    #[test]
    fn crashkit_event_crash_and_hang_remain_inside_the_worker() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let plugin = repository
            .join("target/instruments-sdk-build/pf-crashkit")
            .join(["pf_crashkit", "aex"].join("."));
        if !plugin.exists() {
            return;
        }
        let hash = format!("{:X}", Sha256::digest(fs::read(&plugin).unwrap()));

        let crash = probe_experimental_custom_ui_idle(
            repository,
            &plugin,
            &hash,
            &crashkit_parameters(2.0),
        )
        .unwrap_err();
        assert!(crash.to_string().contains("failed safely"));

        let started = Instant::now();
        let hang = probe_experimental_custom_ui_idle(
            repository,
            &plugin,
            &hash,
            &crashkit_parameters(3.0),
        )
        .unwrap_err();
        assert!(hang.to_string().contains("failed safely"));
        assert!(started.elapsed() < Duration::from_secs(8));
    }

    #[cfg(windows)]
    #[test]
    fn histogrid_draw_and_smartfx_render_share_one_isolated_worker() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let plugin = repository
            .join("target/sdk-fixtures/histogrid")
            .join(["HistoGrid", "aex"].join("."));
        if !plugin.exists()
            || !repository
                .join("target/minihost-build/aex_smart_worker.exe")
                .exists()
        {
            return;
        }
        let hash = format!("{:X}", Sha256::digest(fs::read(&plugin).unwrap()));
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let input = repository.join(format!("target/histogrid-broker-input-{nonce}.png"));
        let pixels = image::RgbaImage::from_fn(37, 23, |x, y| {
            image::Rgba([x as u8 * 7, y as u8 * 11, ((x + y) % 23) as u8 * 11, 255])
        });
        pixels.save(&input).unwrap();
        let formats = [
            RenderPixelFormat::Argb8,
            RenderPixelFormat::Argb16,
            RenderPixelFormat::Argb32f,
        ];
        let outputs = formats
            .iter()
            .map(|format| {
                repository.join(format!(
                    "target/histogrid-broker-output-{}-{nonce}.png",
                    format.report_name()
                ))
            })
            .collect::<Vec<_>>();

        let result = (|| {
            let parameters = inspect_experimental(repository, &plugin, &hash)?;
            formats
                .iter()
                .zip(&outputs)
                .map(|(format, output)| {
                    if *format == RenderPixelFormat::Argb32f {
                        render_experimental_image_at_time_with_format_context_ui_action_and_gpu_backend(
                            repository,
                            &plugin,
                            &hash,
                            &input,
                            output,
                            &parameters,
                            RenderTiming::default(),
                            true,
                            *format,
                            None,
                            Some(RenderUiAction::Draw),
                            RenderGpuBackend::Cpu,
                        )
                    } else {
                        render_experimental_image_at_time_with_format_context_and_ui_action(
                            repository,
                            &plugin,
                            &hash,
                            &input,
                            output,
                            &parameters,
                            RenderTiming::default(),
                            true,
                            *format,
                            None,
                            Some(RenderUiAction::Draw),
                        )
                    }
                })
                .collect::<io::Result<Vec<_>>>()
        })();
        let _ = fs::remove_file(&input);
        let outputs_exist = outputs.iter().all(|output| output.exists());
        for output in &outputs {
            let _ = fs::remove_file(output);
        }

        let reports = result.unwrap();
        assert!(outputs_exist);
        let mut output_hashes = std::collections::HashSet::new();
        for (report, format) in reports.iter().zip(formats) {
            assert_eq!(report["pixel_format"], format.report_name());
            assert_eq!(report["custom_ui_draw_dispatched"], true);
            assert_eq!(report["custom_ui_draw_error"], 0);
            assert_eq!(report["custom_ui_draw_out_flags"], 1);
            assert_eq!(report["custom_ui_lifecycle_errors"], json!([0, 0, 0, 0]));
            assert_eq!(report["custom_ui_context_closed"], true);
            assert_eq!(report["passed"], true);
            output_hashes.insert(report["output_sha256"].as_str().unwrap().to_owned());
        }
        assert_eq!(output_hashes.len(), 3);
    }

    #[test]
    fn default_interactive_payload_is_slot_bound_and_range_normalized() {
        let parameters = vec![InteractiveParameter {
            slot: 2,
            name: "Direction".into(),
            kind: "integer".into(),
            minimum: 1.0,
            maximum: 3.0,
            value: 2.0,
            choices: vec!["Horizontal".into(), "Vertical".into(), "Both".into()],
            color: [255, 0, 0, 0],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }];
        assert_eq!(
            encode_interactive_payload(&parameters).unwrap(),
            "v2|param_2@2:i32=2"
        );
        let mut invalid = parameters;
        invalid[0].value = 4.0;
        assert_eq!(
            encode_default_interactive_payload(&invalid).unwrap(),
            "v2|param_2@2:i32=3"
        );
        let mut malformed = invalid;
        malformed[0].minimum = 4.0;
        malformed[0].maximum = 1.0;
        assert!(encode_interactive_payload(&malformed).is_err());
    }

    #[test]
    fn empty_parameter_set_uses_valid_default_payload() {
        assert_eq!(encode_interactive_payload(&[]).unwrap(), "v2|");
    }

    #[test]
    fn arbitrary_payload_is_hex_encoded_and_bounded() {
        let parameter = InteractiveParameter {
            slot: 1,
            name: "Grid".into(),
            kind: "arbitrary_data".into(),
            minimum: 0.0,
            maximum: 0.0,
            value: 0.0,
            choices: vec![],
            color: [0; 4],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: Some("value=7".into()),
            custom_ui_events: 0,
            control_size: [0, 0],
        };
        assert_eq!(
            encode_interactive_payload(std::slice::from_ref(&parameter)).unwrap(),
            "v5|param_1@1:arbhex=76616c75653d37"
        );
        let mut invalid = parameter.clone();
        invalid.debug_summary = Some("\0".into());
        assert!(encode_interactive_payload(&[invalid]).is_err());
        let mut too_long = parameter;
        too_long.debug_summary = Some("x".repeat(4097));
        assert!(encode_interactive_payload(&[too_long]).is_err());
    }

    #[test]
    fn arbitrary_without_printable_text_keeps_the_plugin_default() {
        let parameter = InteractiveParameter {
            slot: 1,
            name: "Grid".into(),
            kind: "arbitrary_data".into(),
            // Scalar bounds do not describe an arbitrary-data value. Real
            // descriptors may leave these union bytes as unrelated data.
            minimum: 10.0,
            maximum: -10.0,
            value: f64::NAN,
            choices: vec![],
            color: [0; 4],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        };
        assert_eq!(
            encode_default_interactive_payload(&[parameter]).unwrap(),
            "v2|"
        );
    }

    #[test]
    fn ui_only_descriptors_are_filtered_but_path_is_render_assignable() {
        let parameters = [
            "group_start",
            "group_end",
            "button",
            "custom",
            "no_data",
            "path",
        ]
        .map(|kind| InteractiveParameter {
            slot: 1,
            name: kind.into(),
            kind: kind.into(),
            minimum: 0.0,
            maximum: 0.0,
            value: 0.0,
            choices: vec![],
            color: [0; 4],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        });
        assert_eq!(
            encode_interactive_payload(&parameters).unwrap(),
            "v2|param_1@1:i32=0"
        );
    }

    #[test]
    fn component_payload_is_slot_bound_and_range_checked() {
        let parameters = vec![InteractiveParameter {
            slot: 2,
            name: "Center".into(),
            kind: "point".into(),
            minimum: 0.0,
            maximum: 0.0,
            value: 0.0,
            choices: vec![],
            color: [0; 4],
            components: [50.0, 25.5, 0.0],
            component_count: 2,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }];
        assert_eq!(
            encode_interactive_payload(&parameters).unwrap(),
            "v4|param_2@2:point=50,25.5"
        );
        let mut invalid = parameters;
        invalid[0].components[1] = 40_000.0;
        assert!(encode_interactive_payload(&invalid).is_err());
    }

    #[test]
    fn render_timing_is_bounded_and_monotonic() {
        assert!(
            RenderTiming {
                current_time: 3,
                time_step: 1,
                total_time: 4,
                time_scale: 30,
            }
            .is_valid()
        );
        assert!(
            !RenderTiming {
                current_time: 3,
                time_step: 0,
                total_time: 2,
                time_scale: 0,
            }
            .is_valid()
        );
    }

    #[test]
    fn pixel_formats_expose_their_full_argb_stride() {
        assert_eq!(RenderPixelFormat::Argb8.bytes_per_pixel(), 4);
        assert_eq!(RenderPixelFormat::Argb16.bytes_per_pixel(), 8);
        assert_eq!(RenderPixelFormat::Argb32f.bytes_per_pixel(), 16);
    }

    #[test]
    fn world_dump_dir_is_fail_closed_under_the_target_tree() {
        let repository = std::env::temp_dir().join(format!(
            "aexcompat-world-dump-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(repository.join("target")).unwrap();

        let accepted =
            resolve_world_dump_dir(&repository, Path::new("target/world-dumps")).unwrap();
        assert!(accepted.path.is_dir());
        assert_eq!(accepted.display, "target/world-dumps");

        // The length-one session wrapper resolves the environment value once
        // before passing it to `RenderSession::open`, so the second resolver
        // receives the canonical Windows path. Keep that production shape in
        // the contract test (#372).
        let canonical_request = repository.join("target/canonical-world-dumps");
        fs::create_dir_all(&canonical_request).unwrap();
        let canonical_request = canonical_request.canonicalize().unwrap();
        let canonical_accepted =
            resolve_managed_dump_dir(&repository, &canonical_request, true).unwrap();
        assert_eq!(canonical_accepted.display, "target/canonical-world-dumps");

        // A non-empty directory is refused so stale snapshots cannot be
        // mistaken for the coming run's output.
        fs::write(
            accepted.path.join("000-classic-input-2x2.rgba8"),
            [0_u8; 16],
        )
        .unwrap();
        assert!(resolve_world_dump_dir(&repository, Path::new("target/world-dumps")).is_err());

        assert!(resolve_world_dump_dir(&repository, Path::new("")).is_err());
        assert!(resolve_world_dump_dir(&repository, Path::new("target/../escape")).is_err());
        assert!(resolve_world_dump_dir(&repository, Path::new("not-target/dumps")).is_err());
        let outside = std::env::temp_dir().join(format!(
            "aexcompat-world-dump-outside-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(resolve_world_dump_dir(&repository, &outside).is_err());
        assert!(!outside.exists() || fs::remove_dir_all(&outside).is_ok());
        fs::remove_dir_all(repository).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn world_dump_dir_accepts_existing_directory_beneath_target_junction() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let repository =
            std::env::temp_dir().join(format!("aexcompat-world-dump-junction-repository-{nonce}"));
        let junction_target =
            std::env::temp_dir().join(format!("aexcompat-world-dump-junction-target-{nonce}"));
        fs::create_dir_all(&repository).unwrap();
        fs::create_dir_all(junction_target.join("existing")).unwrap();
        let output = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(repository.join("target"))
            .arg(&junction_target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "mklink /J failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let accepted =
            resolve_managed_dump_dir(&repository, &repository.join("target/existing"), true)
                .unwrap();
        assert_eq!(
            accepted.path,
            strip_extended_prefix(&junction_target.join("existing").canonicalize().unwrap())
        );

        fs::remove_dir(repository.join("target")).unwrap();
        fs::remove_dir_all(repository).unwrap();
        fs::remove_dir_all(junction_target).unwrap();
    }

    #[test]
    fn deep16_png_expands_ae_range_and_counts_overrange_samples() {
        let rgba16 = [0u16, 16_384, 32_768, 65_535]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let (samples, overrange) = rgba16_transport_to_png16(&rgba16).unwrap();
        assert_eq!(samples, vec![0, 32_768, 65_535, 65_535]);
        assert_eq!(overrange, 1);
        assert!(rgba16_transport_to_png16(&rgba16[..6]).is_err());
    }

    #[test]
    fn deep16_png_rounds_to_the_same_8_bit_values_as_the_preview() {
        // The 16-bit PNG must stay interchangeable with the 8-bit preview:
        // rounding its full-range samples back to 8 bits has to reproduce the
        // preview quantization for every representable AE-range value.
        for value in 0..=32_768u32 {
            let bytes = (value as u16).to_le_bytes();
            let transport = [bytes[0], bytes[1], 0, 0, 0, 0, 0, 0];
            let (samples, overrange) = rgba16_transport_to_png16(&transport).unwrap();
            assert_eq!(overrange, 0);
            let png16 = u32::from(samples[0]);
            let rounded8 = (png16 * 255 + 32_767) / 65_535;
            let preview8 = (value * 255 + 16_384) / 32_768;
            assert_eq!(rounded8, preview8, "value {value}");
            // Full-range expansion must be lossless for AE-range data.
            assert_eq!((png16 * 32_768 + 32_767) / 65_535, value, "value {value}");
        }
    }

    #[test]
    fn native_depth_transport_keeps_raw_precision_and_builds_preview() {
        assert_eq!(RenderPixelFormat::Argb16.raw_extension(), Some("rgba16le"));
        assert_eq!(
            RenderPixelFormat::Argb32f.raw_extension(),
            Some("rgba32f-le")
        );
        let rgba16 = [0u16, 16_384, 32_768, 65_535]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            native_rgba_to_preview(&rgba16, RenderPixelFormat::Argb16).unwrap(),
            vec![0, 128, 255, 255]
        );
        let rgba32 = [-1.0f32, 0.5, 2.0, f32::NAN]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            native_rgba_to_preview(&rgba32, RenderPixelFormat::Argb32f).unwrap(),
            vec![0, 128, 255, 0]
        );
    }

    #[test]
    fn module_audit_summary_exposes_only_bounded_safe_unknown_basenames() {
        let summary = module_audit_summary(&json!({
            "status": "failed",
            "unknown_count": 4,
            "phase_count": 3,
            "observed_union": {
                "policy": ["approved.dll"],
                "unknown": ["outside.dll", "C:\\private\\leak.dll", "bad:name.dll"]
            }
        }))
        .unwrap();

        assert_eq!(
            summary["authorized_policy_modules"],
            json!(["approved.dll"])
        );
        assert_eq!(summary["unknown_modules"], json!(["outside.dll"]));
        assert!(!summary.to_string().contains("private"));
    }

    #[test]
    fn module_audit_failure_summary_bounds_and_revalidates_rejections() {
        let summary = module_audit_failure_summary(
            &json!({
                "module_audit_failure": {
                    "status": "failed",
                    "reason": "loaded_module_policy_rejection",
                    "unknown_count": 3,
                    "unattributed_count": 1,
                    "rejections": [
                        {
                            "basename": "outside.dll",
                            "canonical_path_token": "a".repeat(64),
                            "path_class": "external",
                            "reason": "outside_allowed_roots_or_unapproved_policy"
                        },
                        {
                            "basename": "C:\\private\\leak.dll",
                            "canonical_path_token": "b".repeat(64),
                            "path_class": "external",
                            "reason": "outside_allowed_roots_or_unapproved_policy"
                        }
                    ],
                    "rejections_truncated": false
                }
            }),
            Some("params_setup"),
        )
        .unwrap();

        assert_eq!(summary["selector_phase"], "params_setup");
        assert_eq!(summary["rejections"].as_array().unwrap().len(), 1);
        assert_eq!(summary["rejections"][0]["basename"], "outside.dll");
        assert_eq!(
            summary["rejections"][0]["canonical_path_token"],
            "a".repeat(64)
        );
        assert!(!summary.to_string().contains("private"));
    }

    #[test]
    fn selector_invocation_diagnostics_distinguish_normal_return_from_seh() {
        let report = json!({
            "selector_invocations": {
                "maximum_records": 64,
                "records": [
                    {
                        "selector": "PARAMS_SETUP",
                        "invocation_completed_normally": true,
                        "raw_return_code": 512,
                        "host_result_code": 512,
                        "seh_caught": false,
                        "seh_code": null,
                        "fault_module_class": null,
                        "fault_module": null,
                        "plugin_rva": null,
                        "access_type": null,
                        "fault_address": null,
                        "registers": null,
                        "stack_pointer_values": null,
                        "global_data_handoff": {
                            "input_at_entry": {
                                "state": "null",
                                "classification": "null",
                                "process_local_token": null
                            },
                            "output_after_return": {
                                "state": "non_null",
                                "classification": "heap_or_unknown",
                                "process_local_token": "ptr-0123456789abcdef"
                            },
                            "same_identity_as_previous_output": null
                        },
                        "effect_ref_at_entry": {
                            "state": "null",
                            "classification": "null",
                            "process_local_token": null,
                            "same_identity_as_global_setup_entry": null
                        },
                        "appl_id_at_entry": {
                            "printable_code": "FXTC",
                            "hex_u32": "0x46585443",
                            "same_value_as_global_setup_entry": null,
                            "host_setting_source": "worker_effect_bootstrap"
                        },
                        "version_at_entry": {
                            "raw_packed_u32": "0x001d000d",
                            "major": 13,
                            "minor": 29,
                            "same_value_as_global_setup_entry": null,
                            "host_setting_source": "worker_effect_bootstrap"
                        }
                    },
                    {
                        "selector": "PARAMS_SETUP",
                        "invocation_completed_normally": false,
                        "raw_return_code": null,
                        "host_result_code": 512,
                        "seh_caught": true,
                        "seh_code": 0xC0000005u32,
                        "fault_module_class": "plugin",
                        "fault_module": "synthetic.aex",
                        "plugin_rva": "0x0000000000001234",
                        "access_type": "read",
                        "fault_address": {
                            "classification": "low",
                            "module": null,
                            "relative_offset": null,
                            "token": null
                        },
                        "registers": {
                            "rcx": {
                                "classification": "low",
                                "module": null,
                                "relative_offset": null,
                                "token": null
                            },
                            "rdx": {
                                "classification": "null",
                                "module": null,
                                "relative_offset": null,
                                "token": null
                            },
                            "r8": {
                                "classification": "plugin",
                                "module": "synthetic.aex",
                                "relative_offset": "0x0000000000002000",
                                "token": null
                            },
                            "r9": {
                                "classification": "module",
                                "module": "kernel32.dll",
                                "relative_offset": "0x0000000000003000",
                                "token": null
                            },
                            "rsp": {
                                "classification": "heap_or_unknown",
                                "module": null,
                                "relative_offset": null,
                                "token": "ptr-0123456789abcdef"
                            }
                        },
                        "stack_pointer_values": [
                            {"offset_bytes": 0, "value": null},
                            {"offset_bytes": 8, "value": null},
                            {"offset_bytes": 16, "value": null},
                            {"offset_bytes": 24, "value": null},
                            {"offset_bytes": 32, "value": null},
                            {"offset_bytes": 40, "value": null}
                        ],
                        "global_data_handoff": {
                            "input_at_entry": {
                                "state": "non_null",
                                "classification": "heap_or_unknown",
                                "process_local_token": "ptr-0123456789abcdef"
                            },
                            "output_after_return": {
                                "state": "non_null",
                                "classification": "heap_or_unknown",
                                "process_local_token": "ptr-0123456789abcdef"
                            },
                            "same_identity_as_previous_output": true
                        },
                        "effect_ref_at_entry": {
                            "state": "non_null",
                            "classification": "heap_or_unknown",
                            "process_local_token": "ptr-fedcba9876543210",
                            "same_identity_as_global_setup_entry": true
                        },
                        "appl_id_at_entry": {
                            "printable_code": "FXTC",
                            "hex_u32": "0x46585443",
                            "same_value_as_global_setup_entry": true,
                            "host_setting_source": "worker_effect_bootstrap"
                        },
                        "version_at_entry": {
                            "raw_packed_u32": "0x001d000d",
                            "major": 13,
                            "minor": 29,
                            "same_value_as_global_setup_entry": true,
                            "host_setting_source": "worker_effect_bootstrap"
                        }
                    }
                ],
                "truncated": false
            }
        });
        let mut diagnostics = json!({});
        propagate_selector_invocations(&mut diagnostics, &report);
        let records = diagnostics["selector_invocations"]["records"]
            .as_array()
            .unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["invocation_completed_normally"], true);
        assert_eq!(records[0]["raw_return_code"], 512);
        assert_eq!(records[0]["seh_caught"], false);
        assert_eq!(records[1]["invocation_completed_normally"], false);
        assert_eq!(records[1]["raw_return_code"], Value::Null);
        assert_eq!(records[1]["seh_caught"], true);
        assert_eq!(records[1]["seh_code"], 0xC0000005u32);
        assert_eq!(records[1]["fault_module_class"], "plugin");
        assert_eq!(records[1]["plugin_rva"], "0x0000000000001234");
        assert_eq!(records[1]["access_type"], "read");
        assert_eq!(records[1]["fault_address"]["classification"], "low");
        assert_eq!(records[1]["registers"]["rdx"]["classification"], "null");
        assert_eq!(
            records[1]["registers"]["rsp"]["token"],
            "ptr-0123456789abcdef"
        );
        assert_eq!(
            records[1]["stack_pointer_values"].as_array().unwrap().len(),
            6
        );
        assert_eq!(
            records[0]["global_data_handoff"]["output_after_return"]["process_local_token"],
            "ptr-0123456789abcdef"
        );
        assert_eq!(
            records[1]["global_data_handoff"]["same_identity_as_previous_output"],
            true
        );
        assert_eq!(records[0]["effect_ref_at_entry"]["state"], "null");
        assert_eq!(
            records[1]["effect_ref_at_entry"]["process_local_token"],
            "ptr-fedcba9876543210"
        );
        assert_eq!(
            records[1]["effect_ref_at_entry"]["same_identity_as_global_setup_entry"],
            true
        );
        assert_eq!(records[0]["appl_id_at_entry"]["printable_code"], "FXTC");
        assert_eq!(records[1]["appl_id_at_entry"]["hex_u32"], "0x46585443");
        assert_eq!(
            records[1]["appl_id_at_entry"]["same_value_as_global_setup_entry"],
            true
        );
        assert_eq!(
            records[0]["version_at_entry"]["raw_packed_u32"],
            "0x001d000d"
        );
        assert_eq!(records[1]["version_at_entry"]["major"], 13);
        assert_eq!(records[1]["version_at_entry"]["minor"], 29);
        assert_eq!(
            records[1]["version_at_entry"]["same_value_as_global_setup_entry"],
            true
        );
        assert!(
            !records[1]["global_data_handoff"]
                .to_string()
                .contains("raw_pointer")
        );
    }

    #[test]
    fn selector_invocation_rejects_unbounded_global_data_handoff_fields() {
        let report = json!({
            "selector_invocations": {
                "maximum_records": 64,
                "records": [{
                    "selector": "GLOBAL_SETUP",
                    "invocation_completed_normally": true,
                    "raw_return_code": 0,
                    "host_result_code": 0,
                    "seh_caught": false,
                    "seh_code": null,
                    "fault_module_class": null,
                    "fault_module": null,
                    "plugin_rva": null,
                    "access_type": null,
                    "fault_address": null,
                    "registers": null,
                    "stack_pointer_values": null,
                    "global_data_handoff": {
                        "input_at_entry": null,
                        "output_after_return": {
                            "state": "non_null",
                            "classification": "heap_or_unknown",
                            "process_local_token": "ptr-0123456789abcdef",
                            "raw_pointer": "0x1234"
                        },
                        "same_identity_as_previous_output": null
                    },
                    "effect_ref_at_entry": {
                        "state": "non_null",
                        "classification": "heap_or_unknown",
                        "process_local_token": "ptr-0123456789abcdef",
                        "same_identity_as_global_setup_entry": null
                    },
                    "appl_id_at_entry": {
                        "printable_code": "FXTC",
                        "hex_u32": "0x46585443",
                        "same_value_as_global_setup_entry": null,
                        "host_setting_source": "worker_effect_bootstrap"
                    },
                    "version_at_entry": {
                        "raw_packed_u32": "0x001d000d",
                        "major": 13,
                        "minor": 29,
                        "same_value_as_global_setup_entry": null,
                        "host_setting_source": "worker_effect_bootstrap"
                    }
                }],
                "truncated": false
            }
        });
        let mut diagnostics = json!({});
        propagate_selector_invocations(&mut diagnostics, &report);
        assert!(
            diagnostics["selector_invocations"]["records"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(diagnostics["selector_invocations"]["truncated"], true);
    }

    #[test]
    fn selector_invocation_rejects_unbounded_effect_ref_fields() {
        let report = json!({
            "selector_invocations": {
                "maximum_records": 64,
                "records": [{
                    "selector": "GLOBAL_SETUP",
                    "invocation_completed_normally": true,
                    "raw_return_code": 0,
                    "host_result_code": 0,
                    "seh_caught": false,
                    "seh_code": null,
                    "fault_module_class": null,
                    "fault_module": null,
                    "plugin_rva": null,
                    "access_type": null,
                    "fault_address": null,
                    "registers": null,
                    "stack_pointer_values": null,
                    "global_data_handoff": {
                        "input_at_entry": null,
                        "output_after_return": null,
                        "same_identity_as_previous_output": null
                    },
                    "effect_ref_at_entry": {
                        "state": "non_null",
                        "classification": "heap_or_unknown",
                        "process_local_token": "ptr-0123456789abcdef",
                        "same_identity_as_global_setup_entry": null,
                        "raw_pointer": "0x1234"
                    },
                    "appl_id_at_entry": {
                        "printable_code": "FXTC",
                        "hex_u32": "0x46585443",
                        "same_value_as_global_setup_entry": null,
                        "host_setting_source": "worker_effect_bootstrap"
                    },
                    "version_at_entry": {
                        "raw_packed_u32": "0x001d000d",
                        "major": 13,
                        "minor": 29,
                        "same_value_as_global_setup_entry": null,
                        "host_setting_source": "worker_effect_bootstrap"
                    }
                }],
                "truncated": false
            }
        });
        let mut diagnostics = json!({});
        propagate_selector_invocations(&mut diagnostics, &report);
        assert!(
            diagnostics["selector_invocations"]["records"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(diagnostics["selector_invocations"]["truncated"], true);
        assert!(
            !diagnostics["selector_invocations"]
                .to_string()
                .contains("0x1234")
        );
    }

    #[test]
    fn application_id_entry_requires_canonical_code_and_source() {
        let expected = json!({
            "printable_code": "FXTC",
            "hex_u32": "0x46585443",
            "same_value_as_global_setup_entry": true,
            "host_setting_source": "worker_effect_bootstrap"
        });
        let escaped = json!({
            "printable_code": "\\x00\\x01\\x02\\x03",
            "hex_u32": "0x00010203",
            "same_value_as_global_setup_entry": false,
            "host_setting_source": "worker_effect_bootstrap"
        });
        assert_eq!(safe_application_id_entry(&expected), Some(expected));
        assert_eq!(safe_application_id_entry(&escaped), Some(escaped));
        assert!(
            safe_application_id_entry(&json!({
                "printable_code": "PrMr",
                "hex_u32": "0x46585443",
                "same_value_as_global_setup_entry": false,
                "host_setting_source": "worker_effect_bootstrap"
            }))
            .is_none()
        );
        assert!(
            safe_application_id_entry(&json!({
                "printable_code": "FXTC",
                "hex_u32": "0x46585443",
                "same_value_as_global_setup_entry": false,
                "host_setting_source": "unknown"
            }))
            .is_none()
        );
    }

    #[test]
    fn spec_version_entry_requires_consistent_packing_and_source() {
        let expected = json!({
            "raw_packed_u32": "0x001d000d",
            "major": 13,
            "minor": 29,
            "same_value_as_global_setup_entry": true,
            "host_setting_source": "worker_effect_bootstrap"
        });
        let changed = json!({
            "raw_packed_u32": "0x001c000d",
            "major": 13,
            "minor": 28,
            "same_value_as_global_setup_entry": false,
            "host_setting_source": "worker_effect_bootstrap"
        });
        let zero = json!({
            "raw_packed_u32": "0x00000000",
            "major": 0,
            "minor": 0,
            "same_value_as_global_setup_entry": true,
            "host_setting_source": "worker_effect_bootstrap"
        });
        let invalid = json!({
            "raw_packed_u32": "0xffffffff",
            "major": -1,
            "minor": -1,
            "same_value_as_global_setup_entry": false,
            "host_setting_source": "worker_effect_bootstrap"
        });
        assert_eq!(safe_spec_version_entry(&expected), Some(expected));
        assert_eq!(safe_spec_version_entry(&changed), Some(changed));
        assert_eq!(safe_spec_version_entry(&zero), Some(zero));
        assert_eq!(safe_spec_version_entry(&invalid), Some(invalid));
        assert!(
            safe_spec_version_entry(&json!({
                "raw_packed_u32": "0x001d000d",
                "major": 13,
                "minor": 28,
                "same_value_as_global_setup_entry": false,
                "host_setting_source": "worker_effect_bootstrap"
            }))
            .is_none()
        );
        assert!(
            safe_spec_version_entry(&json!({
                "raw_packed_u32": "0x001d000d",
                "major": 13,
                "minor": 29,
                "same_value_as_global_setup_entry": true,
                "host_setting_source": "unknown"
            }))
            .is_none()
        );
    }

    #[test]
    fn host_callback_timeline_normalizes_outcomes_and_rejects_arguments() {
        let report = json!({
            "host_callback_timeline": {
                "maximum_records": 128,
                "records": [
                    {
                        "sequence": 0,
                        "callback": "inter.extended_alloc",
                        "selector": "GLOBAL_SETUP",
                        "call_count": 1,
                        "status": "success",
                        "return_code": 0,
                        "classification": "implemented"
                    },
                    {
                        "sequence": 1,
                        "callback": "synthetic.unsupported",
                        "selector": "GLOBAL_SETUP",
                        "call_count": 2,
                        "status": "failure",
                        "return_code": 4,
                        "classification": "unsupported"
                    },
                    {
                        "sequence": 3,
                        "callback": "inter.extended_lookup",
                        "selector": "PARAMS_SETUP",
                        "call_count": 3,
                        "status": "failure",
                        "return_code": 4,
                        "classification": "fallback"
                    },
                    {
                        "sequence": 6,
                        "callback": "synthetic.raw",
                        "selector": "PARAMS_SETUP",
                        "call_count": 1,
                        "status": "success",
                        "return_code": 0,
                        "classification": "implemented",
                        "arguments": ["0x1234"]
                    }
                ],
                "truncated": false
            }
        });
        let mut diagnostics = json!({});
        propagate_host_callback_timeline(&mut diagnostics, &report);
        let records = diagnostics["host_callback_timeline"]["records"]
            .as_array()
            .unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0]["callback"], "inter.extended_alloc");
        assert_eq!(records[0]["status"], "success");
        assert_eq!(records[1]["classification"], "unsupported");
        assert_eq!(records[2]["classification"], "fallback");
        assert_eq!(records[2]["call_count"], 3);
        assert_eq!(diagnostics["host_callback_timeline"]["truncated"], true);
        assert!(
            !diagnostics["host_callback_timeline"]
                .to_string()
                .contains("0x1234")
        );
    }

    #[test]
    fn compute_cache_timeline_is_exact_key_bounded_and_fail_closed() {
        let report = json!({
            "compute_cache_timeline": {
                "maximum_records": 128,
                "records": [
                    {
                        "sequence": 0,
                        "selector": "GLOBAL_SETUP",
                        "slot": 0,
                        "operation": "class_register",
                        "outcome": "registered",
                        "return_code": 0,
                        "call_count": 1
                    },
                    {
                        "sequence": 1,
                        "selector": "RENDER",
                        "slot": 2,
                        "operation": "compute_if_needed_and_checkout",
                        "outcome": "compute_pending",
                        "return_code": 22,
                        "call_count": 3
                    },
                    {
                        "sequence": 2,
                        "selector": "GLOBAL_SETDOWN",
                        "slot": 1,
                        "operation": "class_unregister",
                        "outcome": "unregistered",
                        "return_code": 0,
                        "call_count": 1
                    }
                ],
                "truncated": false
            }
        });
        let mut diagnostics = json!({});
        propagate_compute_cache_timeline(&mut diagnostics, &report);
        assert_eq!(
            diagnostics["compute_cache_timeline"]["maximum_records"],
            128
        );
        assert_eq!(
            diagnostics["compute_cache_timeline"]["records"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            diagnostics["compute_cache_timeline"]["records"][1]["return_code"],
            22
        );
        assert_eq!(
            diagnostics["compute_cache_timeline"]["records"][1]["call_count"],
            3
        );
        assert_eq!(diagnostics["compute_cache_timeline"]["truncated"], false);

        let near_miss = json!({
            "compute_cache_timeline_extra": {
                "maximum_records": 128,
                "records": [],
                "truncated": false
            }
        });
        let mut near_miss_diagnostics = json!({});
        propagate_compute_cache_timeline(&mut near_miss_diagnostics, &near_miss);
        assert!(
            near_miss_diagnostics
                .get("compute_cache_timeline")
                .is_none()
        );

        let malformed = json!({
            "compute_cache_timeline": {
                "maximum_records": 128,
                "records": [{
                    "sequence": 0,
                    "selector": "GLOBAL_SETUP",
                    "slot": 6,
                    "operation": "class_register",
                    "outcome": "registered",
                    "return_code": 0,
                    "call_count": 1,
                    "address": "0x1234"
                }],
                "truncated": false
            }
        });
        let mut malformed_diagnostics = json!({});
        propagate_compute_cache_timeline(&mut malformed_diagnostics, &malformed);
        assert_eq!(
            malformed_diagnostics["compute_cache_timeline"]["records"],
            json!([])
        );
        assert_eq!(
            malformed_diagnostics["compute_cache_timeline"]["truncated"],
            true
        );
        assert!(
            !malformed_diagnostics["compute_cache_timeline"]
                .to_string()
                .contains("0x1234")
        );

        let wrong_container = json!({
            "compute_cache_timeline": {
                "maximum_records": 128,
                "records": [],
                "truncated": false,
                "absolute_path": "C:\\private\\effect.aex"
            }
        });
        let mut wrong_container_diagnostics = json!({});
        propagate_compute_cache_timeline(&mut wrong_container_diagnostics, &wrong_container);
        assert_eq!(
            wrong_container_diagnostics["compute_cache_timeline"]["records"],
            json!([])
        );
        assert_eq!(
            wrong_container_diagnostics["compute_cache_timeline"]["truncated"],
            true
        );
        assert!(
            !wrong_container_diagnostics["compute_cache_timeline"]
                .to_string()
                .contains("private")
        );

        let overflow_records = (0..=MAX_COMPUTE_CACHE_TIMELINE_RECORDS)
            .map(|sequence| {
                json!({
                    "sequence": sequence,
                    "selector": "RENDER",
                    "slot": 3,
                    "operation": "checkout_cached",
                    "outcome": "cache_hit",
                    "return_code": 0,
                    "call_count": 1
                })
            })
            .collect::<Vec<_>>();
        let overflow = json!({
            "compute_cache_timeline": {
                "maximum_records": 128,
                "records": overflow_records,
                "truncated": false
            }
        });
        let mut overflow_diagnostics = json!({});
        propagate_compute_cache_timeline(&mut overflow_diagnostics, &overflow);
        assert_eq!(
            overflow_diagnostics["compute_cache_timeline"]["records"]
                .as_array()
                .unwrap()
                .len(),
            MAX_COMPUTE_CACHE_TIMELINE_RECORDS
        );
        assert_eq!(
            overflow_diagnostics["compute_cache_timeline"]["truncated"],
            true
        );
    }

    #[test]
    fn extended_lookup_timeline_validates_states_ids_and_bounds_fail_closed() {
        let report = json!({
            "extended_lookup_timeline": {
                "maximum_records": 128,
                "records": [
                    {
                        "sequence": 0,
                        "selector": "GLOBAL_SETUP",
                        "call_count": 2,
                        "opaque_table_classification": "null",
                        "raw_private_table_state": "valid",
                        "windows_resource_source_state": "valid",
                        "lookup_id": 7,
                        "outcome": "found",
                        "return_code": 0
                    },
                    {
                        "sequence": 2,
                        "selector": "PARAMS_SETUP",
                        "call_count": 1,
                        "opaque_table_classification": "active_effect_module",
                        "raw_private_table_state": "valid",
                        "windows_resource_source_state": "none",
                        "lookup_id": 8,
                        "outcome": "missing",
                        "return_code": 4
                    },
                    {
                        "sequence": 3,
                        "selector": "PARAMS_SETUP",
                        "call_count": 1,
                        "opaque_table_classification": "other_loaded_sealed_module",
                        "raw_private_table_state": "none",
                        "windows_resource_source_state": "none",
                        "lookup_id": -1,
                        "outcome": "missing",
                        "return_code": 4
                    },
                    {
                        "sequence": 4,
                        "selector": "PARAMS_SETUP",
                        "call_count": 1,
                        "opaque_table_classification": "unrecognized",
                        "raw_private_table_state": "invalid",
                        "windows_resource_source_state": "none",
                        "lookup_id": 2147483647,
                        "outcome": "invalid",
                        "return_code": 4
                    }
                ],
                "truncated": false
            }
        });
        let mut diagnostics = json!({});
        propagate_extended_lookup_timeline(&mut diagnostics, &report);
        let records = diagnostics["extended_lookup_timeline"]["records"]
            .as_array()
            .unwrap();
        assert_eq!(records.len(), 4);
        assert_eq!(records[0]["call_count"], 2);
        assert_eq!(records[2]["lookup_id"], -1);
        assert_eq!(records[3]["lookup_id"], 2147483647_i64);
        assert_eq!(diagnostics["extended_lookup_timeline"]["truncated"], false);

        let near_miss_key = json!({
            "extended_lookup_timeline_extra": {
                "maximum_records": 128,
                "records": [],
                "truncated": false
            }
        });
        let mut near_miss_diagnostics = json!({});
        propagate_extended_lookup_timeline(&mut near_miss_diagnostics, &near_miss_key);
        assert!(
            near_miss_diagnostics
                .get("extended_lookup_timeline")
                .is_none()
        );

        let invalid_record = json!({
            "extended_lookup_timeline": {
                "maximum_records": 128,
                "records": [{
                    "sequence": 0,
                    "selector": "PARAMS_SETUP",
                    "call_count": 1,
                    "opaque_table_classification": "active_resource_module",
                    "raw_private_table_state": "valid",
                    "windows_resource_source_state": "none",
                    "lookup_id": 2147483648_i64,
                    "outcome": "found",
                    "return_code": 4,
                    "value": "must-not-propagate"
                }],
                "truncated": false
            }
        });
        let mut invalid_record_diagnostics = json!({});
        propagate_extended_lookup_timeline(&mut invalid_record_diagnostics, &invalid_record);
        assert_eq!(
            invalid_record_diagnostics["extended_lookup_timeline"]["records"],
            json!([])
        );
        assert_eq!(
            invalid_record_diagnostics["extended_lookup_timeline"]["truncated"],
            true
        );
        assert!(
            !invalid_record_diagnostics["extended_lookup_timeline"]
                .to_string()
                .contains("must-not-propagate")
        );

        let malformed = json!({
            "extended_lookup_timeline": {
                "maximum_records": 128,
                "records": [],
                "truncated": false,
                "absolute_path": "C:\\private\\plugin.aex"
            }
        });
        let mut malformed_diagnostics = json!({});
        propagate_extended_lookup_timeline(&mut malformed_diagnostics, &malformed);
        assert_eq!(
            malformed_diagnostics["extended_lookup_timeline"]["records"],
            json!([])
        );
        assert_eq!(
            malformed_diagnostics["extended_lookup_timeline"]["truncated"],
            true
        );
        assert!(
            !malformed_diagnostics["extended_lookup_timeline"]
                .to_string()
                .contains("private")
        );

        let overflow_records = (0..=MAX_EXTENDED_LOOKUP_TIMELINE_RECORDS)
            .map(|sequence| {
                json!({
                    "sequence": sequence,
                    "selector": "PARAMS_SETUP",
                    "call_count": 1,
                    "opaque_table_classification": "other_loaded_system_module",
                    "raw_private_table_state": "valid",
                    "windows_resource_source_state": "none",
                    "lookup_id": sequence,
                    "outcome": "missing",
                    "return_code": 4
                })
            })
            .collect::<Vec<_>>();
        let overflow = json!({
            "extended_lookup_timeline": {
                "maximum_records": 128,
                "records": overflow_records,
                "truncated": false
            }
        });
        let mut overflow_diagnostics = json!({});
        propagate_extended_lookup_timeline(&mut overflow_diagnostics, &overflow);
        assert_eq!(
            overflow_diagnostics["extended_lookup_timeline"]["records"]
                .as_array()
                .unwrap()
                .len(),
            MAX_EXTENDED_LOOKUP_TIMELINE_RECORDS
        );
        assert_eq!(
            overflow_diagnostics["extended_lookup_timeline"]["truncated"],
            true
        );
    }

    #[test]
    fn extended_allocation_timeline_normalizes_lifetime_without_pointer_details() {
        let allocation = json!({
            "allocation_token": "ptr-0123456789abcdef",
            "state": "live",
            "owner_selector": "GLOBAL_SETUP"
        });
        let report = json!({
            "extended_allocation_timeline": {
                "maximum_records": 128,
                "records": [
                    {
                        "sequence": 0,
                        "selector": "GLOBAL_SETUP",
                        "boundary": "entry",
                        "live_allocation_count": 0,
                        "new_allocations": 0,
                        "frees": 0,
                        "invalid_frees": 0,
                        "double_frees": 0,
                        "global_setup_live_allocation_count": 0,
                        "allocations": []
                    },
                    {
                        "sequence": 1,
                        "selector": "GLOBAL_SETUP",
                        "boundary": "exit",
                        "live_allocation_count": 1,
                        "new_allocations": 1,
                        "frees": 0,
                        "invalid_frees": 0,
                        "double_frees": 0,
                        "global_setup_live_allocation_count": 1,
                        "allocations": [allocation.clone()]
                    },
                    {
                        "sequence": 2,
                        "selector": "PARAMS_SETUP",
                        "boundary": "entry",
                        "live_allocation_count": 1,
                        "new_allocations": 0,
                        "frees": 0,
                        "invalid_frees": 0,
                        "double_frees": 0,
                        "global_setup_live_allocation_count": 1,
                        "allocations": [allocation]
                    },
                    {
                        "sequence": 3,
                        "selector": "PARAMS_SETUP",
                        "boundary": "exit",
                        "live_allocation_count": 1,
                        "new_allocations": 0,
                        "frees": 0,
                        "invalid_frees": 0,
                        "double_frees": 0,
                        "global_setup_live_allocation_count": 1,
                        "allocations": [{
                            "allocation_token": "ptr-0123456789abcdef",
                            "state": "live",
                            "owner_selector": "GLOBAL_SETUP",
                            "size": 4000
                        }]
                    }
                ],
                "truncated": false
            }
        });
        let mut diagnostics = json!({});
        propagate_extended_allocation_timeline(&mut diagnostics, &report);
        let records = diagnostics["extended_allocation_timeline"]["records"]
            .as_array()
            .unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[1]["new_allocations"], 1);
        assert_eq!(records[2]["selector"], "PARAMS_SETUP");
        assert_eq!(records[2]["global_setup_live_allocation_count"], 1);
        assert_eq!(records[2]["allocations"][0]["state"], "live");
        assert_eq!(
            diagnostics["extended_allocation_timeline"]["truncated"],
            true
        );
        assert!(
            !diagnostics["extended_allocation_timeline"]
                .to_string()
                .contains("size")
        );
    }

    #[test]
    fn worker_stage_diagnostics_identify_active_and_failed_selectors() {
        let diagnostics = worker_diagnostics(
            "untrusted C:\\private\\plugin\nstage:global_setup_begin\nstage:global_setup_end error=0\nstage:render_begin\n",
            false,
            "crash",
            0xC0000005,
            42,
        );
        assert_eq!(diagnostics["active_stage"], "render");
        assert_eq!(diagnostics["last_completed_stage"], "global_setup");
        assert_eq!(diagnostics["stage_events"].as_array().unwrap().len(), 3);

        let failed = worker_diagnostics(
            "stage:smart_render_begin\nstage:smart_render_end pre_error=0 render_error=25\n",
            false,
            "ok",
            0,
            8,
        );
        assert_eq!(failed["failure_stage"], "smart_render");
        assert_eq!(failed["first_failure_stage"], "smart_render");
        assert!(failed.to_string().find("private").is_none());

        let nested_crash = worker_diagnostics(
            "stage:render_begin\nstage:sequence_setup_begin\nstage:sequence_setup_end error=0\nstage:frame_setup_begin\nstage:frame_setup_end error=0\n",
            false,
            "crashed",
            0xC0000005,
            29,
        );
        assert_eq!(nested_crash["active_stage"], "render");
        assert_eq!(nested_crash["failure_stage"], "render");
        assert_eq!(nested_crash["first_failure_stage"], "render");
        assert_eq!(nested_crash["last_completed_stage"], "frame_setup");

        let cleanup_failure = worker_diagnostics(
            "stage:global_setup_begin\nstage:global_setup_end error=512\nstage:global_setdown_begin\nstage:global_setdown_end error=-1\n",
            false,
            "nonzero_exit",
            14,
            12,
        );
        assert_eq!(cleanup_failure["first_failure_stage"], "global_setup");
        assert_eq!(cleanup_failure["failure_stage"], "global_setdown");
    }

    /// A classic session's frame errors used to carry no stage at all: the
    /// `render` pair brackets the whole session, so a frame that failed came
    /// back with `first_failure_stage: null` and nothing said which selector
    /// did it. 29 of the 56 failing AE 2026 effects looked like that
    /// (issue #722).
    #[test]
    fn a_classic_frame_names_the_selector_that_failed() {
        // The plug-in's own RENDER refused this frame.
        let selector = worker_diagnostics(
            "stage:frame_setup_begin\nstage:frame_setup_end error=0\n\
             stage:classic_render_begin\nstage:classic_render_end error=512\n",
            false,
            "ok",
            0,
            9,
        );
        assert_eq!(selector["first_failure_stage"], "classic_render");
        assert_eq!(selector["failure_stage"], "classic_render");

        // The selector returned cleanly and the host's finalize added the
        // error, which has to read as a different stage - otherwise a host-side
        // refusal is filed against the plug-in.
        let finalize = worker_diagnostics(
            "stage:classic_render_begin\nstage:classic_render_end error=0\n\
             stage:classic_finalize_end error=4\n",
            false,
            "ok",
            0,
            9,
        );
        assert_eq!(finalize["first_failure_stage"], "classic_finalize");

        // The host refused the plug-in's requested output resize, so RENDER was
        // never dispatched. `prepare_output` runs inside the same
        // classic_execution::dispatch_render call as the selector but never
        // calls the plug-in, so it gets its own name instead of being filed
        // under RENDER. It is not the `output_validation` that session.rs
        // assigns from `output_pixels_valid` - that one is smart-only and means
        // the returned pixels were empty, untouched, or non-finite.
        let host_refused_resize = worker_diagnostics(
            "stage:frame_setup_begin\nstage:frame_setup_end error=0\n\
             stage:classic_output_resize_end error=4\n",
            false,
            "ok",
            0,
            9,
        );
        assert_eq!(
            host_refused_resize["first_failure_stage"],
            "classic_output_resize"
        );
        assert_eq!(
            host_refused_resize["failure_stage"],
            "classic_output_resize"
        );

        // A frame that never reached the selector carries no `classic_render`
        // pair, because the markers live inside the selector hook rather than
        // around the dispatch call. On a non-zero incoming error
        // `dispatch_render` skips the draw, prepare_output, and selector steps,
        // so bracketing the call would re-emit that error under the selector's
        // name; `failure_stage` takes the last failing stage and would blame a
        // RENDER the plug-in never saw. (It does not skip close_ui, which does
        // enter the plug-in and is still unbracketed - issue #735.)
        //
        // FRAME_SETUP is not yet one of the steps that gets skipped: the
        // `render_once` path drops its error instead of propagating it and
        // dispatches RENDER anyway (issue #725), so a real setup refusal still
        // shows both stages today - correctly, because RENDER really does run.
        // This trace is the one a lifecycle refusal produces once it
        // propagates, and the one `smart_render_runtime` already produces at
        // its `lifecycle.setup_error != 0` check, since that call site honors
        // the field the classic path drops.
        let lifecycle_refused = worker_diagnostics(
            "stage:frame_setup_begin\nstage:frame_setup_end error=512\n",
            false,
            "ok",
            0,
            9,
        );
        assert_eq!(lifecycle_refused["first_failure_stage"], "frame_setup");
        assert_eq!(lifecycle_refused["failure_stage"], "frame_setup");
    }

    /// The event cap bounds what is reported, not what is noticed. A frame that
    /// fails on every frame of a long session overruns the list, and that is
    /// exactly when the failure must still be attributed (issue #722).
    #[test]
    fn the_event_cap_does_not_hide_a_failure_past_it() {
        let mut trace = "stage:classic_render_begin\nstage:classic_render_end error=0\n"
            .repeat(MAX_STAGE_EVENTS);
        trace.push_str("stage:classic_render_begin\nstage:classic_render_end error=512\n");
        let diagnostics = worker_diagnostics(&trace, false, "ok", 0, 9);
        assert_eq!(
            diagnostics["stage_events"].as_array().unwrap().len(),
            MAX_STAGE_EVENTS
        );
        assert_eq!(diagnostics["failure_stage"], "classic_render");
        assert_eq!(diagnostics["last_completed_stage"], "classic_render");
    }

    #[test]
    fn worker_stage_diagnostics_are_bounded() {
        let trace = "stage:render_begin\n".repeat(MAX_STAGE_EVENTS + 20);
        let diagnostics = worker_diagnostics(&trace, true, "timeout", 1, 5_000);
        assert_eq!(
            diagnostics["stage_events"].as_array().unwrap().len(),
            MAX_STAGE_EVENTS
        );
        assert_eq!(diagnostics["stderr_truncated"], true);
    }

    #[test]
    fn load_failure_marker_accepts_only_path_free_worker_owned_stage_and_error() {
        let diagnostics = worker_diagnostics(
            "untrusted C:\\private\\plugin.aex\n\
             stage:load_failure stage=load_library win32_error=126\n",
            false,
            "nonzero_exit",
            11,
            4,
        );
        assert_eq!(
            diagnostics["load_failure"],
            json!({"stage": "load_library", "win32_error_code": 126})
        );
        assert!(!diagnostics["load_failure"].to_string().contains("private"));

        for marker in [
            "stage:load_failure stage=unknown win32_error=126",
            "stage:load_failure stage=load_library win32_error=0",
            "stage:load_failure stage=load_library win32_error=-1",
            "stage:load_failure stage=load_library win32_error=126 path=C:\\private",
            "stage:load_failure win32_error=126 stage=load_library",
        ] {
            assert_eq!(load_failure_marker(marker, 11), None);
        }
        assert_eq!(
            load_failure_marker(
                "stage:load_failure stage=add_dll_directory win32_error=87",
                11,
            ),
            Some(json!({"stage": "add_dll_directory", "win32_error_code": 87}))
        );
        assert_eq!(
            load_failure_marker(
                "stage:load_failure stage=set_default_dll_directories win32_error=5",
                11,
            ),
            Some(json!({
                "stage": "set_default_dll_directories",
                "win32_error_code": 5,
            }))
        );
        assert_eq!(
            load_failure_marker("stage:load_failure stage=load_library win32_error=126", 12,),
            None
        );
    }

    #[test]
    fn structured_worker_report_supplies_bounded_unique_missing_suites() {
        let mut trace = String::from(
            "stage:suite_acquire_failed name=PF World Suite version=2\n\
             stage:suite_acquire_failed name=PF World Suite version=2\n\
             stage:suite_acquire_failed name=C:\\private\\suite version=1\n\
             stage:suite_acquire_failed name=Bad Suite version=-1\n",
        );
        for index in 0..(MAX_MISSING_SUITES + 3) {
            trace.push_str(&format!(
                "stage:suite_acquire_failed name=Safe Suite {index} version=1\n"
            ));
        }
        trace.push_str(&format!(
            "stage:suite_acquire_failed name={} version=1\n",
            "A".repeat(MAX_SUITE_NAME_LEN + 1)
        ));

        let mut diagnostics = worker_diagnostics(&trace, false, "nonzero_exit", 1, 2);
        assert!(diagnostics["missing_suites"].as_array().unwrap().is_empty());
        let mut reported = vec![
            json!({"name": "PF World Suite", "version": 2}),
            json!({"name": "PF World Suite", "version": 2}),
            json!({"name": "C:\\private\\suite", "version": 1}),
            json!({"name": "Bad Suite", "version": -1}),
        ];
        for index in 0..(MAX_MISSING_SUITES + 3) {
            reported.push(json!({"name": format!("Safe Suite {index}"), "version": 1}));
        }
        propagate_missing_suites(&mut diagnostics, &json!({"missing_suites": reported}));
        let suites = diagnostics["missing_suites"].as_array().unwrap();
        assert_eq!(suites.len(), MAX_MISSING_SUITES);
        assert_eq!(diagnostics["missing_suites_truncated"], true);
        assert_eq!(suites[0], json!({"name": "PF World Suite", "version": 2}));
        assert_eq!(
            suites
                .iter()
                .filter(|suite| suite["name"] == "PF World Suite")
                .count(),
            1
        );
        assert!(!diagnostics.to_string().contains("private"));
    }

    #[test]
    fn structured_worker_report_supplies_bounded_unique_unsupported_suite_calls() {
        let mut diagnostics = json!({});
        let mut reported = vec![
            json!({"name": "AEGP Comp Suite", "version": 21, "slot": 7, "call_count": 2}),
            json!({"name": "AEGP Comp Suite", "version": 21, "slot": 7, "call_count": 9}),
            json!({"name": "C:\\private\\suite", "version": 1, "slot": 1, "call_count": 1}),
            json!({"name": "Bad Suite", "version": 1, "slot": 2048, "call_count": 1}),
        ];
        for index in 0..(MAX_UNSUPPORTED_SUITE_CALLS + 3) {
            reported.push(json!({
                "name": format!("Safe Suite {index}"),
                "version": 1,
                "slot": index,
                "call_count": 1,
            }));
        }

        propagate_unsupported_suite_calls(
            &mut diagnostics,
            &json!({
                "unsupported_suite_calls": reported,
                "callback_history": [
                    {"sequence": 4, "callback": "iterate", "result": 0, "reason": "none"},
                    {"sequence": 5, "callback": "checkout_output", "result": 4, "reason": "invalid_arguments"}
                ]
            }),
        );
        let calls = diagnostics["unsupported_suite_calls"].as_array().unwrap();
        assert_eq!(calls.len(), MAX_UNSUPPORTED_SUITE_CALLS);
        assert_eq!(diagnostics["unsupported_suite_calls_truncated"], true);
        assert_eq!(
            calls[0],
            json!({"name": "AEGP Comp Suite", "version": 21, "slot": 7, "call_count": 2})
        );
        assert_eq!(
            calls
                .iter()
                .filter(|call| call["name"] == "AEGP Comp Suite" && call["slot"] == 7)
                .count(),
            1
        );
        assert!(!diagnostics.to_string().contains("private"));
        assert_eq!(diagnostics["callback_history"].as_array().unwrap().len(), 2);
        assert_eq!(diagnostics["callback_history"][1]["sequence"], 5);
    }

    #[test]
    fn structured_worker_report_supplies_bounded_suite_timeline_without_rounding_failure() {
        let boundary_name = format!("A{}Z", "n".repeat(MAX_SUITE_NAME_LEN - 2));
        let boundary_selector = "S".repeat(MAX_SUITE_NAME_LEN);
        let mut reported = vec![
            json!({
                "sequence": 0,
                "action": "acquire",
                "name": boundary_name,
                "version": MAX_SUITE_VERSION,
                "selector": boundary_selector,
                "result": i32::MIN,
            }),
            json!({
                "sequence": 1,
                "action": "acquire",
                "name": "A".repeat(MAX_SUITE_NAME_LEN + 1),
                "version": 1,
                "selector": "HOST",
                "result": 0,
            }),
            json!({
                "sequence": 2,
                "action": "acquire",
                "name": "PF World Suite",
                "version": MAX_SUITE_VERSION + 1,
                "selector": "HOST",
                "result": 0,
            }),
        ];
        for sequence in 3..(MAX_SUITE_TIMELINE_EVENTS + 10) {
            reported.push(json!({
                "sequence": sequence,
                "action": if sequence % 2 == 0 { "acquire" } else { "release" },
                "name": "PF World Suite",
                "version": 2,
                "selector": "PF Cmd RENDER",
                "result": 0,
            }));
        }
        let mut diagnostics = worker_diagnostics("", false, "nonzero_exit", 13, 1);
        propagate_suite_timeline(
            &mut diagnostics,
            &json!({
                "suite_timeline": reported,
                "suite_timeline_truncated": false,
            }),
        );

        assert_eq!(
            diagnostics["suite_timeline"].as_array().unwrap().len(),
            MAX_SUITE_TIMELINE_EVENTS
        );
        assert_eq!(diagnostics["suite_timeline_truncated"], true);
        assert_eq!(diagnostics["classification"], "nonzero_exit");
        assert_eq!(diagnostics["exit_code"], 13);
        assert_eq!(
            diagnostics["suite_timeline"][0]["version"],
            MAX_SUITE_VERSION
        );
        assert_eq!(diagnostics["suite_timeline"][0]["result"], i32::MIN);
    }

    #[test]
    fn minidump_marker_accepts_only_worker_owned_shapes() {
        // Legitimate worker lines normalize to a path-free marker.
        assert_eq!(
            minidump_marker("stage:minidump_written bytes=51790"),
            Some("written bytes=51790".to_owned())
        );
        assert_eq!(
            minidump_marker("stage:minidump_failed reason=dbghelp_unavailable"),
            Some("failed reason=dbghelp_unavailable".to_owned())
        );
        assert_eq!(
            minidump_marker("stage:minidump_failed reason=create_failed code=5"),
            Some("failed reason=create_failed".to_owned())
        );
        assert_eq!(
            minidump_marker("stage:minidump_failed reason=writer_timeout"),
            Some("failed reason=writer_timeout".to_owned())
        );

        // A plug-in cannot smuggle a path or fake reason through the marker.
        assert_eq!(
            minidump_marker("stage:minidump_written name=C:\\Users\\secret\\a.dmp bytes=1"),
            None
        );
        assert_eq!(minidump_marker("stage:minidump_written bytes=../etc"), None);
        assert_eq!(
            minidump_marker("stage:minidump_failed reason=totally_made_up"),
            None
        );
        assert_eq!(minidump_marker("stage:minidump_written whatever"), None);
        assert_eq!(minidump_marker("stage:other"), None);
    }

    #[test]
    fn isolated_worker_diagnostics_expose_kill_reason_and_memory_peaks() {
        let isolated = crate::secure_launch::SecureLaunchResult {
            classification: crate::ExitClassification::NonzeroExit,
            exit_code: 42,
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            kill_reason: Some("memory_limit"),
            worker_peak_commit_bytes: Some(529_000_000),
            peak_process_memory_bytes: Some(530_000_000),
            peak_job_memory_bytes: Some(531_000_000),
            process_memory_limit_bytes: 536_870_912,
            memory_limit_reached: true,
            dismissed_windows: Vec::new(),
            worker_freshness_warning: Some("source_newer_than_worker"),
            module_audit_warning: Some("secure worker module audit did not pass".to_owned()),
        };
        let diagnostics = isolated_worker_diagnostics(&isolated, 1_234);
        assert_eq!(diagnostics["kill_reason"], "memory_limit");
        assert_eq!(
            diagnostics["worker_freshness_warning"],
            "source_newer_than_worker"
        );
        assert_eq!(
            diagnostics["module_audit_warning"],
            "secure worker module audit did not pass"
        );
        assert_eq!(diagnostics["memory_limit_reached"], true);
        assert_eq!(diagnostics["worker_peak_commit_bytes"], 529_000_000u64);
        assert_eq!(diagnostics["peak_process_memory_bytes"], 530_000_000u64);
        assert_eq!(diagnostics["peak_job_memory_bytes"], 531_000_000u64);
        assert_eq!(diagnostics["process_memory_limit_bytes"], 536_870_912u64);
        assert_eq!(diagnostics["classification"], "nonzero_exit");
        assert_eq!(diagnostics["elapsed_ms"], 1_234);

        let alive = crate::secure_launch::SecureLaunchResult {
            classification: crate::ExitClassification::Ok,
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            kill_reason: None,
            worker_peak_commit_bytes: Some(900_000),
            peak_process_memory_bytes: Some(1_000_000),
            peak_job_memory_bytes: Some(1_000_000),
            process_memory_limit_bytes: 536_870_912,
            memory_limit_reached: false,
            dismissed_windows: Vec::new(),
            worker_freshness_warning: None,
            module_audit_warning: None,
        };
        let diagnostics = isolated_worker_diagnostics(&alive, 5);
        assert_eq!(diagnostics["kill_reason"], Value::Null);
        assert_eq!(diagnostics["memory_limit_reached"], false);
        assert!(
            diagnostics.get("worker_freshness_warning").is_none(),
            "a fresh worker adds no freshness key"
        );
        assert!(
            diagnostics.get("module_audit_warning").is_none(),
            "a passing audit adds no audit key"
        );
    }

    #[test]
    fn runtime_module_backend_matches_the_worker_gpu_command() {
        assert_eq!(
            runtime_backend(RenderGpuBackend::Auto),
            Some(RuntimeBackend::Cuda)
        );
        assert_eq!(
            runtime_backend(RenderGpuBackend::Cuda),
            Some(RuntimeBackend::Cuda)
        );
        assert_eq!(
            runtime_backend(RenderGpuBackend::OpenCl),
            Some(RuntimeBackend::Opencl)
        );
        assert_eq!(
            runtime_backend(RenderGpuBackend::DirectX),
            Some(RuntimeBackend::Directx)
        );
        assert_eq!(runtime_backend(RenderGpuBackend::Cpu), None);
    }

    #[test]
    fn in_place_inspection_requires_search_dirs_before_touching_the_plugin() {
        // The empty-dirs rejection fires before any file access, so the fake
        // path is never read (issue #751).
        let error = inspect_experimental_in_place(
            std::path::Path::new("missing-repository"),
            std::path::Path::new("missing-plugin.aex"),
            &"0".repeat(64),
            Vec::new(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("requires at least one dependency search directory")
        );
    }

    #[test]
    fn in_place_aegp_initialization_requires_search_dirs_before_file_access() {
        let error = initialize_experimental_aegp_in_place(
            std::path::Path::new("missing-repository"),
            std::path::Path::new("missing-plugin.aex"),
            &"0".repeat(64),
            Vec::new(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("requires at least one dependency search directory")
        );
    }
}
#[test]
fn cleanup_contained_report_contract_rejects_mutations() {
    let valid = json!({
        "inspection_status": "parameters_inspected_cleanup_contained",
        "global_setup_error": 0,
        "params_setup_error": 0,
        "global_setdown_error": -1,
    });
    assert!(cleanup_contained_report_is_valid(&valid));
    for (field, replacement) in [
        ("inspection_status", json!("parameters_inspected")),
        ("global_setup_error", json!(4)),
        ("params_setup_error", json!(4)),
        ("global_setdown_error", json!(0)),
    ] {
        let mut mutated = valid.clone();
        mutated[field] = replacement;
        assert!(!cleanup_contained_report_is_valid(&mutated), "{field}");
        mutated.as_object_mut().unwrap().remove(field);
        assert!(
            !cleanup_contained_report_is_valid(&mutated),
            "missing {field}"
        );
    }
}
