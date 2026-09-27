//! Built-artifact contract for diagnostic world transformations (#1593).
//! The same public SmartFX AEX and frame must render identically when both
//! input and output worlds have padding. Missing artifacts skip local `cargo
//! test`; the explicit built-artifact gate supplies and exercises them.

#[cfg(windows)]
mod windows_e2e {
    use aexcompat_broker::image_render::{RenderGpuBackend, RenderPixelFormat};
    use aexcompat_broker::render_differential::{NativeWorld, compare_native_worlds};
    use aexcompat_broker::render_session::{
        DiagnosticWorldLayout, FrameStatus, RenderSession, SessionOpenRequest,
    };
    use aexcompat_broker::secure_launch::LaunchEnvironment;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .expect("repository root")
    }

    fn probe_artifact(root: &Path) -> PathBuf {
        let build = root.join("target/pf-smart-geometry-probe-build");
        [
            build.join("Release/pf_smart_geometry_probe.aex"),
            build.join("pf_smart_geometry_probe.aex"),
        ]
        .into_iter()
        .filter(|path| path.is_file())
        .max_by_key(|path| path.metadata().unwrap().modified().unwrap())
        .unwrap_or_else(|| build.join("Release/pf_smart_geometry_probe.aex"))
    }

    fn request<'a>(root: &'a Path, plugin: &'a Path, sha: &'a str) -> SessionOpenRequest<'a> {
        SessionOpenRequest {
            repository: root,
            plugin_path: plugin,
            plugin_sha256: sha,
            parameters: None,
            payload_override: None,
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
            companions: Vec::new(),
            dependency_search_dirs: vec![plugin.parent().unwrap().to_path_buf()],
            width: 16,
            height: 12,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 5,
            time_scale: 1,
            frame_deadline: Duration::from_secs(30),
            smart: true,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
            launch_environment: LaunchEnvironment::default(),
        }
    }

    struct Rendered {
        pixels: Vec<u8>,
        width: u32,
        height: u32,
        origin_x: i32,
        origin_y: i32,
    }

    fn render(mut session: RenderSession, input: &[u8]) -> Rendered {
        let outcome = session.render_frame(0, 0, input).expect("render frame");
        let frame = match outcome.status {
            FrameStatus::Rendered {
                pixels,
                width,
                height,
                origin_x,
                origin_y,
            } if width > 0 && height > 0 && !pixels.is_empty() => Rendered {
                pixels,
                width,
                height,
                origin_x,
                origin_y,
            },
            other => panic!("unexpected frame outcome: {other:?}"),
        };
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
        frame
    }

    fn world(frame: &Rendered) -> NativeWorld<'_> {
        NativeWorld {
            pixels: &frame.pixels,
            format: RenderPixelFormat::Argb8,
            width: frame.width,
            height: frame.height,
            rowbytes: frame.width as usize * 4,
            origin_x: frame.origin_x,
            origin_y: frame.origin_y,
        }
    }

    struct VariantFile(PathBuf);

    impl Drop for VariantFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn variant(plugin: &Path, marker: &str) -> VariantFile {
        let name = format!(
            "pf_smart_geometry_probe-difftile{marker}-{}-{:032x}.aex",
            std::process::id(),
            rand::random::<u128>()
        );
        let path = std::env::temp_dir().join(name);
        std::fs::copy(plugin, &path).expect("copy public probe variant");
        VariantFile(path)
    }

    #[test]
    fn smartfx_padded_worlds_preserve_native_pixels() {
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_worker.exe");
        let plugin = probe_artifact(&root);
        if !worker.is_file() || !plugin.is_file() {
            eprintln!("skipping built-artifact diagnostic render: build worker and probe first");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&plugin).unwrap()));
        let input: Vec<u8> = (0..16 * 12 * 4).map(|index| (index % 251) as u8).collect();
        let baseline = render(
            RenderSession::open(request(&root, &plugin, &sha)).unwrap(),
            &input,
        );
        let transformed = render(
            RenderSession::open_diagnostic(
                request(&root, &plugin, &sha),
                DiagnosticWorldLayout {
                    input_row_padding: 12,
                    output_row_padding: 20,
                    ..DiagnosticWorldLayout::default()
                },
            )
            .unwrap(),
            &input,
        );
        let shifted = render(
            RenderSession::open_diagnostic(
                request(&root, &plugin, &sha),
                DiagnosticWorldLayout {
                    input_origin_x: 3,
                    input_origin_y: 2,
                    extent_hint: Some([1, 1, 15, 11]),
                    ..DiagnosticWorldLayout::default()
                },
            )
            .unwrap(),
            &input,
        );
        assert_eq!(baseline.pixels.len(), 16 * 12 * 4);
        assert_eq!(
            compare_native_worlds(world(&baseline), &[world(&transformed)])
                .unwrap()
                .differing_pixels,
            0
        );
        assert_eq!(
            compare_native_worlds(world(&baseline), &[world(&shifted)])
                .unwrap()
                .differing_pixels,
            0
        );
    }

    #[test]
    fn smartfx_horizontal_and_vertical_tiles_cover_the_full_frame() {
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_worker.exe");
        let plugin = probe_artifact(&root);
        if !worker.is_file() || !plugin.is_file() {
            eprintln!("skipping built-artifact tile render: build worker and probe first");
            return;
        }
        let variant = variant(&plugin, "");
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&variant.0).unwrap()));
        let input: Vec<u8> = (0..16 * 12 * 4).map(|index| (index % 251) as u8).collect();
        let baseline = render(
            RenderSession::open(request(&root, &variant.0, &sha)).unwrap(),
            &input,
        );
        assert_eq!(
            [
                baseline.width,
                baseline.height,
                baseline.origin_x as u32,
                baseline.origin_y as u32
            ],
            [16, 12, 0, 0]
        );
        let run_tile = |rect: [i32; 4]| {
            let frame = render(
                RenderSession::open_diagnostic(
                    request(&root, &variant.0, &sha),
                    DiagnosticWorldLayout {
                        input_row_padding: 12,
                        output_row_padding: 12,
                        request_rect: Some(rect),
                        ..DiagnosticWorldLayout::default()
                    },
                )
                .unwrap(),
                &input,
            );
            assert_eq!(
                [
                    frame.origin_x,
                    frame.origin_y,
                    frame.origin_x + frame.width as i32,
                    frame.origin_y + frame.height as i32
                ],
                rect,
                "the AEX must honor the requested tile before pixels are compared"
            );
            frame
        };
        let left = run_tile([0, 0, 7, 12]);
        let right = run_tile([7, 0, 16, 12]);
        let top = run_tile([0, 0, 16, 5]);
        let bottom = run_tile([0, 5, 16, 12]);
        let horizontal = compare_native_worlds(world(&baseline), &[world(&left), world(&right)])
            .expect("horizontal tiles cover exactly once");
        let vertical = compare_native_worlds(world(&baseline), &[world(&top), world(&bottom)])
            .expect("vertical tiles cover exactly once");
        assert_eq!(horizontal.compared_pixels, 16 * 12);
        assert_eq!(horizontal.differing_pixels, 0);
        assert_eq!(vertical.compared_pixels, 16 * 12);
        assert_eq!(vertical.differing_pixels, 0);
    }

    #[test]
    fn mutated_origin_and_extent_are_detected_as_pixel_differences() {
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_worker.exe");
        let plugin = probe_artifact(&root);
        if !worker.is_file() || !plugin.is_file() {
            eprintln!("skipping built-artifact mutation render: build worker and probe first");
            return;
        }
        let input: Vec<u8> = (0..16 * 12 * 4).map(|index| (index % 251) as u8).collect();
        for (marker, layout) in [
            (
                "-originbug",
                DiagnosticWorldLayout {
                    request_rect: Some([7, 0, 16, 12]),
                    ..DiagnosticWorldLayout::default()
                },
            ),
            (
                "-extentbug",
                DiagnosticWorldLayout {
                    extent_hint: Some([1, 1, 15, 11]),
                    ..DiagnosticWorldLayout::default()
                },
            ),
        ] {
            let mutated = variant(&plugin, marker);
            let sha = format!("{:x}", Sha256::digest(std::fs::read(&mutated.0).unwrap()));
            let baseline = render(
                RenderSession::open(request(&root, &mutated.0, &sha)).unwrap(),
                &input,
            );
            let transformed = render(
                RenderSession::open_diagnostic(request(&root, &mutated.0, &sha), layout).unwrap(),
                &input,
            );
            let report = if marker == "-originbug" {
                let correct = variant(&plugin, "");
                let correct_sha =
                    format!("{:x}", Sha256::digest(std::fs::read(&correct.0).unwrap()));
                let left = render(
                    RenderSession::open_diagnostic(
                        request(&root, &correct.0, &correct_sha),
                        DiagnosticWorldLayout {
                            request_rect: Some([0, 0, 7, 12]),
                            ..DiagnosticWorldLayout::default()
                        },
                    )
                    .unwrap(),
                    &input,
                );
                compare_native_worlds(world(&baseline), &[world(&left), world(&transformed)])
                    .expect("mutated tiles still cover the same region")
            } else {
                compare_native_worlds(world(&baseline), &[world(&transformed)]).unwrap()
            };
            assert!(report.differing_pixels > 0, "{marker}: {report:?}");
            assert!(report.difference_bbox.is_some(), "{marker}: {report:?}");
        }
    }

    #[test]
    fn mutated_output_stride_cannot_be_certified_as_rendered() {
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_worker.exe");
        let plugin = probe_artifact(&root);
        if !worker.is_file() || !plugin.is_file() {
            eprintln!("skipping built-artifact stride mutation: build worker and probe first");
            return;
        }
        let variant = variant(&plugin, "-stridebug");
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&variant.0).unwrap()));
        let input: Vec<u8> = (0..16 * 12 * 4).map(|index| (index % 251) as u8).collect();
        let mut session = RenderSession::open_diagnostic(
            request(&root, &variant.0, &sha),
            DiagnosticWorldLayout {
                output_row_padding: 12,
                request_rect: Some([7, 0, 16, 12]),
                ..DiagnosticWorldLayout::default()
            },
        )
        .unwrap();
        let outcome = session.render_frame(0, 0, &input);
        assert!(
            !matches!(
                outcome,
                Ok(aexcompat_broker::render_session::FrameOutcome {
                    status: FrameStatus::Rendered { .. },
                    ..
                })
            ),
            "a tight-stride write into a padded world must not be certified"
        );
        let _ = session.close();
    }
}
