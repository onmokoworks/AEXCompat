#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_SIGNAL and local Release worker"]
    fn real_gui_native_default_reset_matches_fresh_session_pixels() {
        let plugin =
            PathBuf::from(std::env::var_os("AEXCOMPAT_TEST_SIGNAL").expect("explicit AEX"));
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-native-default-reset");
        let input = directory.join("input.png");
        image::RgbaImage::from_pixel(256, 144, image::Rgba([32, 64, 128, 255]))
            .save(&input)
            .unwrap();
        let mut app = HarnessApp::new(repository);
        app.live_render = true;
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: fs::metadata(&plugin).unwrap().modified().ok(),
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        let seed = app.parameters.iter().position(|p| p.slot == 1).unwrap();
        assert_eq!(app.parameters[seed].value, 0.0);
        assert_eq!(app.parameter_defaults[seed].value, 0.0);
        // Known row0 variability is a separate unresolved effect issue. This
        // explicit comparison setting is not an automatic product workaround.
        app.parameters
            .iter_mut()
            .find(|p| p.slot == 38)
            .unwrap()
            .value = 0.0;
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, input);
        let _ = ctx.end_pass();
        let mut images = Vec::new();
        for step in 0..3 {
            if step == 0 {
                app.parameters[seed].value = 7.0;
            }
            if step == 1 {
                assert!(reset_parameter(
                    &mut app.parameters[seed],
                    &app.parameter_defaults[seed]
                ));
            }
            if step == 2 {
                app.close_live_session();
            }
            let output = directory.join(format!("{step}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            let report: serde_json::Value = serde_json::from_str(&app.report).unwrap();
            assert!(report.get("resident_session").is_some(), "{}", app.report);
            let image = image::open(output).unwrap().to_rgba8();
            assert_eq!(image.dimensions(), (256, 144));
            assert!(image.pixels().all(|p| p[3] == 255));
            images.push(image);
        }
        assert_ne!(images[0], images[1], "seed edit must affect pixels");
        assert_eq!(
            images[1], images[2],
            "reset must match fresh native default"
        );
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_OLM_BLUR and local Release worker"]
    fn real_aex_gui_state_renders_and_publishes_preview() {
        let plugin =
            PathBuf::from(std::env::var_os("AEXCOMPAT_TEST_OLM_BLUR").expect("explicit AEX path"));
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let directory = temporary_directory("real-gui-render");
        let input = directory.join("input.png");
        let output = directory.join("output.png");
        image::RgbaImage::from_fn(256, 144, |x, y| {
            image::Rgba([
                if ((x / 8) ^ (y / 8)) & 1 == 1 {
                    240
                } else {
                    16
                },
                x as u8,
                y as u8,
                255,
            ])
        })
        .save(&input)
        .unwrap();
        let mut app = HarnessApp::new(repository);
        app.live_render = false;
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: fs::metadata(&plugin).unwrap().modified().ok(),
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert!(app.smart_render_capability.is_some(), "{}", app.report);
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, input.clone());
        let _ = ctx.end_pass();
        app.render_to(output.clone());
        drain(&mut app);
        assert_eq!(app.status, "AEX output ready.", "{}", app.report);
        assert_eq!(app.output_image.as_ref(), Some(&output));
        assert_eq!(app.preview.as_ref().unwrap().size(), [256, 144]);
        let pixels = image::open(&output).unwrap().to_rgba8();
        assert!(pixels.pixels().all(|pixel| pixel.0[3] == 255));
        let original = image::open(input).unwrap().to_rgba8();
        assert_ne!(pixels, original);
        let edge_energy = |image: &image::RgbaImage| -> u64 {
            (0..144)
                .flat_map(|y| (1..256).map(move |x| (x, y)))
                .map(|(x, y)| {
                    image.get_pixel(x, y).0[0].abs_diff(image.get_pixel(x - 1, y).0[0]) as u64
                })
                .sum()
        };
        assert!(edge_energy(&pixels) < edge_energy(&original));
        let green_min = pixels.pixels().map(|p| p.0[1]).min().unwrap();
        let green_max = pixels.pixels().map(|p| p.0[1]).max().unwrap();
        assert!(
            green_max - green_min > 64,
            "blur must retain the input gradient"
        );
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn repository_root_prefers_worktree_cwd_to_a_separated_binary_location() {
        let fixture = std::env::temp_dir().join(format!(
            "aexcompat-harness-root-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = fixture.join("worktree");
        std::fs::create_dir_all(root.join("guest")).unwrap();
        std::fs::create_dir_all(root.join("broker/crates/harness")).unwrap();
        std::fs::write(root.join("guest/Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(root.join("broker/Cargo.toml"), "[workspace]\n").unwrap();

        let cwd = root.join("broker/crates/harness");
        let separated_exe = fixture.join("target/debug/aexcompat-harness.exe");
        assert_eq!(
            repository_root_from_runtime_paths(Some(cwd), Some(separated_exe)),
            Some(root)
        );
        std::fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_MAKE_ALPHA and local Release worker"]
    fn real_multilayer_aex_gui_preserves_alpha_source_pixels() {
        let plugin = PathBuf::from(
            std::env::var_os("AEXCOMPAT_TEST_MAKE_ALPHA").expect("explicit AEX path"),
        );
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-make-alpha");
        let input = directory.join("input.png");
        let secondary = directory.join("secondary.png");
        image::RgbaImage::from_pixel(256, 144, image::Rgba([173, 57, 219, 255]))
            .save(&input)
            .unwrap();
        image::RgbaImage::from_fn(256, 144, |x, y| image::Rgba([x as u8, y as u8, 37, 255]))
            .save(&secondary)
            .unwrap();
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert!(app.smart_render_capability.is_some(), "{}", app.report);
        for (slot, name) in [
            (6, "Host Layer"),
            (8, "Alpha Source Layer"),
            (9, "Alpha From Channel"),
        ] {
            assert_eq!(
                app.parameters.iter().find(|p| p.slot == slot).unwrap().name,
                name
            );
        }
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, input.clone());
        let _ = ctx.end_pass();
        for (choice, inverse) in [(2, false), (9, true)] {
            app.apply_debug_request_document(&serde_json::json!({
                "schema_version":1,"timing":{"frame":0,"fps":30,"duration_frames":300},
                "assignments":[{"slot":6,"layer":input},{"slot":8,"layer":secondary},{"slot":9,"value":choice}]
            }),&directory.join("request.json")).unwrap();
            let output = directory.join(format!("output-{choice}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(app.preview.as_ref().unwrap().size(), [256, 144]);
            let pixels = image::open(&output).unwrap().to_rgba8();
            assert_eq!(pixels.dimensions(), (256, 144));
            for (x, _, pixel) in pixels.enumerate_pixels() {
                let alpha = if inverse { 255 - x as u8 } else { x as u8 };
                assert_eq!(pixel.0[3], alpha);
                if alpha > 0 {
                    assert_eq!(&pixel.0[..3], &[173, 57, 219]);
                }
            }
        }
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_COMPOSITE and local Release worker"]
    fn real_composite_gui_preserves_cpu_matte_pixels() {
        let plugin = PathBuf::from(
            std::env::var("AEXCOMPAT_TEST_COMPOSITE")
                .expect("explicit AEX path")
                .replace('\\', "/"),
        );
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-composite");
        let input = directory.join("input.png");
        let secondary = directory.join("secondary.png");
        image::RgbaImage::from_pixel(256, 144, image::Rgba([173, 57, 219, 255]))
            .save(&input)
            .unwrap();
        image::RgbaImage::from_fn(256, 144, |x, y| image::Rgba([x as u8, y as u8, 37, 255]))
            .save(&secondary)
            .unwrap();
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert!(app.smart_render_capability.is_some(), "{}", app.report);
        for (slot, name) in [(181, "Host Layer"), (329, "Input"), (330, "Use")] {
            assert_eq!(
                app.parameters.iter().find(|p| p.slot == slot).unwrap().name,
                name
            );
        }
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, input.clone());
        let _ = ctx.end_pass();
        for (choice, inverse) in [(0, false), (1, true)] {
            app.apply_debug_request_document(&serde_json::json!({
                "schema_version":1,"timing":{"frame":0,"fps":30,"duration_frames":300},
                "assignments":[{"slot":181,"layer":input},{"slot":329,"layer":secondary},{"slot":330,"value":2},{"slot":331,"value":choice},{"slot":402,"value":4}]
            }),&directory.join("request.json")).unwrap();
            let output = directory.join(format!("output-{choice}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(app.preview.as_ref().unwrap().size(), [256, 144]);
            let pixels = image::open(&output).unwrap().to_rgba8();
            assert_eq!(pixels.dimensions(), (256, 144));
            for (x, _, pixel) in pixels.enumerate_pixels() {
                let alpha = if inverse { 255 - x as u8 } else { x as u8 };
                assert_eq!(pixel.0[3], alpha);
                if alpha > 0 {
                    assert_eq!(&pixel.0[..3], &[173, 57, 219]);
                }
            }
        }
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_UNMULT_RS and local Release worker"]
    fn real_unmult_gui_preserves_black_key_pixels() {
        let plugin = PathBuf::from(
            std::env::var("AEXCOMPAT_TEST_UNMULT_RS")
                .expect("explicit AEX path")
                .replace('\\', "/"),
        );
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-unmult");
        let input = directory.join("input.png");
        let palette = [
            [0, 0, 0],
            [255, 255, 255],
            [128, 128, 128],
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [64, 128, 192],
            [192, 128, 64],
        ];
        let original = image::RgbaImage::from_fn(256, 144, |x, y| {
            let rgb = palette[((x / 32 + y / 18) % 8) as usize];
            image::Rgba([rgb[0], rgb[1], rgb[2], 255])
        });
        // Opaque-input black-key oracle only; no general rounding/AE claim.
        let expected = image::RgbaImage::from_fn(256, 144, |x, y| {
            let pixel = original.get_pixel(x, y).0;
            let alpha = *pixel[..3].iter().max().unwrap();
            let channel = |c: u8| {
                if alpha == 0 {
                    0
                } else {
                    ((u32::from(c) * 255 + u32::from(alpha) / 2) / u32::from(alpha)) as u8
                }
            };
            image::Rgba([
                channel(pixel[0]),
                channel(pixel[1]),
                channel(pixel[2]),
                alpha,
            ])
        });
        original.save(&input).unwrap();
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert_eq!(
            app.parameters.iter().find(|p| p.slot == 1).unwrap().name,
            "White Key"
        );
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, input);
        let _ = ctx.end_pass();
        app.apply_debug_request_document(&serde_json::json!({
            "schema_version":1,"timing":{"frame":0,"fps":30,"duration_frames":300},
            "assignments":[{"slot":1,"value":0},{"slot":2,"value":1},{"slot":3,"value":0},{"slot":4,"value":0}]
        }), &directory.join("request.json")).unwrap();
        let output = directory.join("output.png");
        app.render_to(output.clone());
        drain(&mut app);
        assert_eq!(app.status, "AEX output ready.", "{}", app.report);
        assert_eq!(app.output_image.as_ref(), Some(&output));
        assert_eq!(app.preview.as_ref().unwrap().size(), [256, 144]);
        assert_eq!(image::open(output).unwrap().to_rgba8(), expected);
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_FRAMESLICE and local Release worker"]
    fn real_frameslice_gui_renders_timed_primary_bands() {
        assert_real_temporal_bands("AEXCOMPAT_TEST_FRAMESLICE", "gui-frameslice");
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_TIMESLICE and local Release worker"]
    fn real_timeslice_gui_renders_timed_primary_bands() {
        assert_real_temporal_bands("AEXCOMPAT_TEST_TIMESLICE", "gui-timeslice");
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_COLORFUL_ECHO and local Release worker"]
    fn real_colorful_echo_gui_composites_two_history_frames() {
        let plugin = PathBuf::from(
            std::env::var("AEXCOMPAT_TEST_COLORFUL_ECHO")
                .expect("explicit AEX path")
                .replace('\\', "/"),
        );
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-colorful-echo");
        let patch = |index: u32| {
            image::RgbaImage::from_fn(256, 144, |x, y| {
                image::Rgba(
                    if (16 + index * 80..48 + index * 80).contains(&x) && (40..104).contains(&y) {
                        [x as u8, y as u8, (71 + index * 40) as u8, 255]
                    } else {
                        [0, 0, 0, 0]
                    },
                )
            })
        };
        for index in 0..3 {
            patch(index)
                .save(directory.join(format!("{index}.png")))
                .unwrap();
        }
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert_eq!(
            app.parameters.iter().find(|p| p.slot == 1).unwrap().name,
            "Number of Echoes"
        );
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, directory.join("0.png"));
        let _ = ctx.end_pass();
        for opacity in [0, 100] {
            app.apply_debug_request_document(&serde_json::json!({
                "schema_version":1,"timing":{"frame":60,"fps":30,"duration_frames":300},
                "assignments":[{"slot":1,"value":2},{"slot":2,"value":1},{"slot":3,"value":100},
                    {"slot":6,"value":0},{"slot":7,"value":1},{"slot":8,"value":2},{"slot":9,"value":opacity}],
                "timed_layers":[{"slot":0,"time":59,"time_scale":30,"image":directory.join("1.png")},
                    {"slot":0,"time":58,"time_scale":30,"image":directory.join("2.png")}]
            }), &directory.join("request.json")).unwrap();
            assert_eq!(app.timed_layers.len(), 2);
            let output = directory.join(format!("output{opacity}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(app.preview.as_ref().unwrap().size(), [256, 144]);
            let mut expected = image::RgbaImage::new(256, 144);
            for index in (if opacity == 0 { 1 } else { 0 })..3 {
                for (x, y, pixel) in patch(index).enumerate_pixels() {
                    if pixel[3] != 0 {
                        expected.put_pixel(x, y, *pixel);
                    }
                }
            }
            assert_eq!(image::open(output).unwrap().to_rgba8(), expected);
        }
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_POSTERIZE_TIME and local Release worker"]
    fn real_posterize_time_gui_holds_quantized_frames() {
        let plugin = PathBuf::from(
            std::env::var("AEXCOMPAT_TEST_POSTERIZE_TIME")
                .expect("explicit AEX path")
                .replace('\\', "/"),
        );
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-posterize-time");
        let frame = |number: u8| {
            image::RgbaImage::from_fn(256, 144, |x, y| {
                image::Rgba([x as u8, y as u8, number * 30, 255])
            })
        };
        for number in 4..9 {
            frame(number)
                .save(directory.join(format!("{number}.png")))
                .unwrap();
        }
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert_eq!(
            app.parameters.iter().find(|p| p.slot == 4).unwrap().name,
            "Frame Separation"
        );
        for (current, separation, selected) in [(5, 1, 5), (5, 2, 4), (6, 2, 6), (7, 2, 6)] {
            ctx.begin_pass(Default::default());
            app.load_input_path(&ctx, directory.join(format!("{current}.png")));
            let _ = ctx.end_pass();
            let samples = (4..9)
                .filter(|t| *t != current)
                .map(|t| {
                    serde_json::json!({
                        "slot":0,"time":t,"time_scale":30,"image":directory.join(format!("{t}.png"))
                    })
                })
                .collect::<Vec<_>>();
            app.apply_debug_request_document(
                &serde_json::json!({
                    "schema_version":1,"timing":{"frame":current,"fps":30,"duration_frames":300},
                    "assignments":[{"slot":4,"value":separation}],"timed_layers":samples
                }),
                &directory.join("request.json"),
            )
            .unwrap();
            assert_eq!(app.timed_layers.len(), 4);
            let output = directory.join(format!("output-{current}-{separation}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(app.preview.as_ref().unwrap().size(), [256, 144]);
            assert_eq!(image::open(output).unwrap().to_rgba8(), frame(selected));
        }
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_TEMPORAL_BLUR and local Release worker"]
    fn real_temporal_blur_gui_averages_neighbor_frames() {
        let plugin = PathBuf::from(
            std::env::var("AEXCOMPAT_TEST_TEMPORAL_BLUR")
                .expect("explicit AEX path")
                .replace('\\', "/"),
        );
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-temporal-blur");
        let pixels = |blue: u8| {
            image::RgbaImage::from_fn(256, 144, |x, y| image::Rgba([x as u8, y as u8, blue, 255]))
        };
        for (time, blue) in [(3, 10), (4, 40), (5, 100), (6, 220), (7, 250)] {
            pixels(blue)
                .save(directory.join(format!("{time}.png")))
                .unwrap();
        }
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert_eq!(
            app.parameters.iter().find(|p| p.slot == 3).unwrap().name,
            "Amount (Frames)"
        );
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, directory.join("5.png"));
        let _ = ctx.end_pass();
        for (amount, direction, blue) in [(0, 3, 100), (1, 3, 100), (2, 3, 70), (2, 2, 160)] {
            let samples = [3, 4, 6, 7]
                .into_iter()
                .map(|t| {
                    serde_json::json!({
                        "slot":0,"time":t,"time_scale":30,"image":directory.join(format!("{t}.png"))
                    })
                })
                .collect::<Vec<_>>();
            let assignments = [
                (3, amount),
                (4, 1),
                (5, 1),
                (6, 0),
                (7, direction),
                (8, 2),
                (9, 0),
                (10, 0),
                (11, 0),
                (12, 1),
                (16, 100),
                (18, 0),
            ]
            .into_iter()
            .map(|(slot, value)| serde_json::json!({"slot":slot,"value":value}))
            .collect::<Vec<_>>();
            app.apply_debug_request_document(
                &serde_json::json!({
                    "schema_version":1,"timing":{"frame":5,"fps":30,"duration_frames":300},
                    "assignments":assignments,"timed_layers":samples
                }),
                &directory.join("request.json"),
            )
            .unwrap();
            assert_eq!(app.timed_layers.len(), 4);
            let output = directory.join(format!("output-{amount}-{direction}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(app.preview.as_ref().unwrap().size(), [256, 144]);
            assert_eq!(image::open(output).unwrap().to_rgba8(), pixels(blue));
        }
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_TIME_DISPLACEMENT and local Release worker"]
    fn real_time_displacement_gui_combines_map_and_primary_times() {
        let plugin = PathBuf::from(
            std::env::var("AEXCOMPAT_TEST_TIME_DISPLACEMENT")
                .expect("explicit AEX path")
                .replace('\\', "/"),
        );
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-time-displacement");
        for time in 3..=7 {
            image::RgbaImage::from_fn(256, 144, |x, y| {
                image::Rgba([x as u8, y as u8, time * 30, 255])
            })
            .save(directory.join(format!("{time}.png")))
            .unwrap();
        }
        for reverse in [false, true] {
            image::RgbaImage::from_fn(256, 144, |x, _| {
                let value = if (x >= 128) != reverse { 255 } else { 0 };
                image::Rgba([value, value, value, 255])
            })
            .save(directory.join(format!("map-{reverse}.png")))
            .unwrap();
        }
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert_eq!(
            app.parameters.iter().find(|p| p.slot == 3).unwrap().name,
            "Map Layer"
        );
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, directory.join("5.png"));
        let _ = ctx.end_pass();
        for (amount, reverse) in [(0, false), (2, false), (2, true), (-2, false), (-2, true)] {
            let samples = [3, 4, 6, 7].into_iter().map(|t| {
                serde_json::json!({"slot":0,"time":t,"time_scale":30,"image":directory.join(format!("{t}.png"))})
            }).collect::<Vec<_>>();
            let mut assignments = [
                (5, 0.0),
                (7, 0.0),
                (8, 255.0),
                (9, 1.0),
                (11, 0.0),
                (12, 4.0),
                (13, amount as f64),
                (14, 127.5),
                (15, 0.0),
                (16, 0.0),
                (17, 3.0),
                (18, 1.0),
                (20, 1.0),
            ]
            .into_iter()
            .map(|(slot, value)| serde_json::json!({"slot":slot,"value":value}))
            .collect::<Vec<_>>();
            assignments.push(
                serde_json::json!({"slot":3,"layer":directory.join(format!("map-{reverse}.png"))}),
            );
            app.apply_debug_request_document(
                &serde_json::json!({"schema_version":1,"timing":{"frame":5,"fps":30,"duration_frames":300},"assignments":assignments,"timed_layers":samples}),
                &directory.join("request.json"),
            ).unwrap();
            assert_eq!(app.timed_layers.len(), 4);
            let output = directory.join(format!("output-{amount}-{reverse}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(app.preview.as_ref().unwrap().size(), [256, 144]);
            let expected = image::RgbaImage::from_fn(256, 144, |x, y| {
                let later = ((x >= 128) != reverse) != (amount < 0);
                let time = if amount == 0 {
                    5
                } else if later {
                    6
                } else {
                    4
                };
                image::Rgba([x as u8, y as u8, time * 30, 255])
            });
            assert_eq!(image::open(output).unwrap().to_rgba8(), expected);
        }
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_TRAILS and local Release worker"]
    fn real_trails_gui_composites_explicit_input_history() {
        assert_real_trails_history(false, false);
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_TRAILS and local Release worker"]
    fn real_trails_gui_composites_default_self_history() {
        assert_real_trails_history(true, false);
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_TRAILS_BASIC and local Release worker"]
    fn real_trails_basic_gui_composites_explicit_input_history() {
        assert_real_trails_history(false, true);
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_TRAILS_BASIC and local Release worker"]
    fn real_trails_basic_gui_composites_default_self_history() {
        assert_real_trails_history(true, true);
    }

    fn assert_real_trails_history(default_self: bool, basic: bool) {
        let plugin_env = if basic {
            "AEXCOMPAT_TEST_TRAILS_BASIC"
        } else {
            "AEXCOMPAT_TEST_TRAILS"
        };
        let plugin = PathBuf::from(
            std::env::var(plugin_env)
                .expect("explicit AEX path")
                .replace('\\', "/"),
        );
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-trails");
        let pixels = |times: &[u8]| {
            image::RgbaImage::from_fn(256, 144, |x, y| {
                for &time in times {
                    let left = 16 + u32::from(time - 3) * 80;
                    if (left..left + 32).contains(&x) && (40..104).contains(&y) {
                        return image::Rgba([x as u8, y as u8, time * 30, 255]);
                    }
                }
                image::Rgba([0, 0, 0, 0])
            })
        };
        for time in [3, 4, 5] {
            pixels(&[time])
                .save(directory.join(format!("{time}.png")))
                .unwrap();
        }
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert_eq!(
            app.parameters.iter().find(|p| p.slot == 2).unwrap().name,
            "Input Layer"
        );
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, directory.join("5.png"));
        let _ = ctx.end_pass();
        for (count, mode) in [(0, 1), (1, 1), (2, 1), (1, 3), (2, 3)] {
            let sample_slot = if default_self { 0 } else { 2 };
            let samples = [3,4].into_iter().map(|t| serde_json::json!({"slot":sample_slot,"time":t,"time_scale":30,"image":directory.join(format!("{t}.png"))})).collect::<Vec<_>>();
            let values = if basic {
                vec![
                    (18, 1),
                    (19, count),
                    (20, 1),
                    (21, 100),
                    (22, 0),
                    (24, 0),
                    (25, 100),
                    (28, 100),
                    (36, 1),
                    (37, 100),
                    (38, 1),
                    (39, 1),
                    (40, mode),
                    (41, 1),
                    (42, 100),
                ]
            } else {
                vec![
                    (17, 1),
                    (18, count),
                    (19, 1),
                    (20, 100),
                    (21, 0),
                    (22, 100),
                    (23, 1),
                    (26, 100),
                    (37, 100),
                    (117, 1),
                    (118, 100),
                    (120, 1),
                    (121, mode),
                    (123, 100),
                ]
            };
            let mut assignments = values
                .into_iter()
                .map(|(slot, value)| serde_json::json!({"slot":slot,"value":value}))
                .collect::<Vec<_>>();
            if !default_self {
                assignments.push(serde_json::json!({"slot":2,"layer":directory.join("5.png")}));
            }
            app.apply_debug_request_document(&serde_json::json!({"schema_version":1,"timing":{"frame":5,"fps":30,"duration_frames":300},"assignments":assignments,"timed_layers":samples}), &directory.join("request.json")).unwrap();
            assert_eq!(app.timed_layers.len(), 2);
            let output = directory.join(format!("output-{count}-{mode}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(app.preview.as_ref().unwrap().size(), [256, 144]);
            let mut times = ((5 - count) as u8..5).collect::<Vec<_>>();
            if mode == 1 {
                times.push(5);
            }
            assert_eq!(image::open(output).unwrap().to_rgba8(), pixels(&times));
        }
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_VELOCITY_REMAP and local Release worker"]
    fn real_velocity_gui_selects_default_self_frames() {
        assert_real_velocity_frames(true);
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_VELOCITY_REMAP and local Release worker"]
    fn real_velocity_gui_selects_explicit_frames() {
        assert_real_velocity_frames(false);
    }

    fn assert_real_velocity_frames(default_self: bool) {
        let plugin = PathBuf::from(
            std::env::var("AEXCOMPAT_TEST_VELOCITY_REMAP")
                .expect("explicit AEX path")
                .replace('\\', "/"),
        );
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-velocity-remap");
        let pixels = |time: u8| {
            image::RgbaImage::from_fn(256, 144, |x, y| {
                image::Rgba([x as u8, y as u8, time * 25, 255])
            })
        };
        for time in [0, 2, 4, 6, 8] {
            pixels(time)
                .save(directory.join(format!("{time}.png")))
                .unwrap();
        }
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert_eq!(
            app.parameters.iter().find(|p| p.slot == 3).unwrap().name,
            "Velocity"
        );
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, directory.join("4.png"));
        let _ = ctx.end_pass();
        for (velocity, start, expected) in
            [(0, 0, 0), (50, 0, 2), (100, 0, 4), (200, 0, 8), (100, 2, 6)]
        {
            let sample_slot = if default_self { 0 } else { 2 };
            let samples = [0, 2, 6, 8]
                .into_iter()
                .map(|t| {
                    serde_json::json!({"slot":sample_slot,"time":t,"time_scale":30,
                    "image":directory.join(format!("{t}.png"))})
                })
                .collect::<Vec<_>>();
            let mut assignments = [(3, velocity), (4, start), (5, 0), (11, 1)]
                .into_iter()
                .map(|(slot, value)| serde_json::json!({"slot":slot,"value":value}))
                .collect::<Vec<_>>();
            if !default_self {
                assignments.push(serde_json::json!({"slot":2,"layer":directory.join("4.png")}));
            }
            app.apply_debug_request_document(
                &serde_json::json!({"schema_version":1,
                "timing":{"frame":4,"fps":30,"duration_frames":300},
                "assignments":assignments,"timed_layers":samples}),
                &directory.join("request.json"),
            )
            .unwrap();
            assert_eq!(app.timed_layers.len(), 4);
            let output = directory.join(format!("output-{velocity}-{start}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(app.preview.as_ref().unwrap().size(), [256, 144]);
            assert_eq!(image::open(output).unwrap().to_rgba8(), pixels(expected));
        }
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_TIMESMEAR and local Release worker"]
    fn real_timesmear_gui_preserves_history_bypass_and_spatial_map() {
        let plugin = PathBuf::from(
            std::env::var("AEXCOMPAT_TEST_TIMESMEAR")
                .expect("explicit AEX path")
                .replace('\\', "/"),
        );
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-timesmear");
        let pixels = |t: u32| {
            image::RgbaImage::from_fn(256, 144, |x, y| {
                image::Rgba([
                    ((x + 30 * (4 - t)) % 256) as u8,
                    ((y + 20 * (4 - t)) % 256) as u8,
                    (t * 25) as u8,
                    255,
                ])
            })
        };
        for t in 0..5 {
            pixels(t).save(directory.join(format!("{t}.png"))).unwrap();
        }
        image::RgbaImage::from_pixel(256, 144, image::Rgba([255u8, 255, 255, 255]))
            .save(directory.join("white.png"))
            .unwrap();
        image::RgbaImage::from_fn(256, 144, |x, _| {
            if x < 128 {
                image::Rgba([0u8, 0, 0, 255])
            } else {
                image::Rgba([255, 255, 255, 255])
            }
        })
        .save(directory.join("split.png"))
        .unwrap();
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert_eq!(
            app.parameters.iter().find(|p| p.slot == 20).unwrap().name,
            "Map Layer"
        );
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, directory.join("4.png"));
        let _ = ctx.end_pass();
        let mut outputs = std::collections::HashMap::new();
        for (name, same, amount, mix, map) in [
            ("same", true, 100, 0, None),
            ("full", false, 100, 0, None),
            ("amount-zero", false, 0, 0, None),
            ("original-full", false, 100, 100, None),
            ("white", false, 100, 0, Some("white")),
            ("split", false, 100, 0, Some("split")),
        ] {
            let mut assignments = [
                (1, amount),
                (2, 2),
                (3, 3),
                (4, 1),
                (5, 360),
                (6, 0),
                (8, mix),
                (17, 1),
                (18, 0),
                (21, if map.is_some() { 100 } else { 0 }),
                (22, 0),
                (23, 0),
            ]
            .into_iter()
            .map(|(slot, value)| serde_json::json!({"slot":slot,"value":value}))
            .collect::<Vec<_>>();
            if let Some(map) = map {
                assignments.push(
                    serde_json::json!({"slot":20,"layer":directory.join(format!("{map}.png"))}),
                );
            }
            let samples=(0..4).map(|t|serde_json::json!({"slot":0,"time":t,"time_scale":30,"image":directory.join(format!("{}.png",if same {4}else{t}))})).collect::<Vec<_>>();
            app.apply_debug_request_document(&serde_json::json!({"schema_version":1,"timing":{"frame":4,"fps":30,"duration_frames":300},"assignments":assignments,"timed_layers":samples}),&directory.join("request.json")).unwrap();
            assert_eq!(app.timed_layers.len(), 4);
            let output = directory.join(format!("output-{name}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(app.preview.as_ref().unwrap().size(), [256, 144]);
            let result = image::open(output).unwrap().to_rgba8();
            assert_eq!(result.dimensions(), (256, 144));
            assert!(result.pixels().any(|p| p[3] != 0));
            outputs.insert(name, result);
        }
        let current = pixels(4);
        assert_ne!(outputs["full"], outputs["same"]);
        assert_eq!(outputs["amount-zero"], current);
        assert_eq!(outputs["original-full"], current);
        assert_eq!(outputs["white"], outputs["full"]);
        for (left, right) in [(0, 128), (128, 256)] {
            assert!((0..144).any(|y| {
                (left..right).any(|x| outputs["full"].get_pixel(x, y) != current.get_pixel(x, y))
            }));
        }
        let expected = image::RgbaImage::from_fn(256, 144, |x, y| {
            *if x < 128 {
                current.get_pixel(x, y)
            } else {
                outputs["full"].get_pixel(x, y)
            }
        });
        assert_eq!(outputs["split"], expected);
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_SCATTERMAP and local Release worker"]
    fn real_scattermap_gui_matches_recorded_argb8_reference() {
        let plugin = PathBuf::from(
            std::env::var("AEXCOMPAT_TEST_SCATTERMAP")
                .expect("explicit AEX path")
                .replace('\\', "/"),
        );
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-scattermap");
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert_eq!(
            app.parameters.iter().find(|p| p.slot == 6).unwrap().name,
            "Scatter Map"
        );
        // Recorded opaque ARGB8 AE 25.2 hashes, not deep/float reference claims.
        for (name, amount, direction, seed, mix, map, expected) in [
            (
                "default",
                5,
                3,
                0,
                100,
                None,
                "19cea826f356e0d94bc29ff10cb9e7f5a770fe5b288cb3d190a58372353102d9",
            ),
            (
                "identity",
                0,
                3,
                0,
                100,
                None,
                "863d238f52f81aba4017c198af4d748cb57fe369e6216fdbacf45fd94037ecf7",
            ),
            (
                "horizontal",
                5,
                1,
                0,
                100,
                None,
                "10a2a95a0ae27ca5fe3a6f6f92eeddfe611885fa72afa0902a24e8bea5d2198f",
            ),
            (
                "vertical",
                5,
                2,
                0,
                100,
                None,
                "6d6198506967e18f619e57cf79e65c52c8f8c65c0ef89710344af2f1045e091c",
            ),
            (
                "amount_max",
                500,
                3,
                0,
                100,
                None,
                "8e535435c74a9521d816a3b836db578a2ae942efbd80a55447b97610dc26b794",
            ),
            (
                "seed_max",
                5,
                3,
                10000,
                100,
                None,
                "e31ba13264e801de7ccce4d6863215e54c0dc0c7ff4a918e45ee75bc59e817ec",
            ),
            (
                "mix_zero",
                5,
                3,
                0,
                0,
                None,
                "863d238f52f81aba4017c198af4d748cb57fe369e6216fdbacf45fd94037ecf7",
            ),
            (
                "connected",
                5,
                3,
                0,
                100,
                Some((5u32, 3u32, 0)),
                "a38568761441c209940f81a8c2792dad50566c66eda1463bdcf071cca614891b",
            ),
            (
                "inverted",
                5,
                3,
                0,
                100,
                Some((11, 7, 1)),
                "3bc0c5172b880a8a83cec24177b78721e9f0619d5330f6a26aaa02b9cc057a08",
            ),
        ] {
            let (width, height) = if map.is_some() {
                (11u32, 7u32)
            } else {
                (16, 12)
            };
            let input = directory.join(format!("input-{name}.png"));
            image::RgbaImage::from_fn(width, height, |x, y| {
                image::Rgba([
                    (x * 255 / (width - 1)) as u8,
                    (y * 255 / (height - 1)) as u8,
                    ((x + y) * 255 / (width + height - 2)) as u8,
                    255,
                ])
            })
            .save(&input)
            .unwrap();
            ctx.begin_pass(Default::default());
            app.load_input_path(&ctx, input);
            let _ = ctx.end_pass();
            let mut assignments = [(1, amount), (2, direction), (3, seed), (5, mix)]
                .into_iter()
                .map(|(slot, value)| serde_json::json!({"slot":slot,"value":value}))
                .collect::<Vec<_>>();
            if let Some((mw, mh, invert)) = map {
                let path = directory.join(format!("map-{name}.png"));
                image::RgbaImage::from_fn(mw, mh, |x, y| {
                    let v = ((x + y) * 255 / (mw + mh - 2)) as u8;
                    image::Rgba([v, v, v, 255])
                })
                .save(&path)
                .unwrap();
                assignments.extend([
                    serde_json::json!({"slot":6,"layer":path}),
                    serde_json::json!({"slot":7,"value":invert}),
                ]);
            }
            app.apply_debug_request_document(&serde_json::json!({"schema_version":1,"timing":{"frame":0,"fps":24,"duration_frames":1},"assignments":assignments}),&directory.join("request.json")).unwrap();
            let output = directory.join(format!("output-{name}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{name}: {}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(
                app.preview.as_ref().unwrap().size(),
                [width as usize, height as usize]
            );
            let result = image::open(output).unwrap().to_rgba8();
            assert_eq!(result.dimensions(), (width, height));
            let argb = result
                .pixels()
                .flat_map(|p| [p[3], p[0], p[1], p[2]])
                .collect::<Vec<_>>();
            assert_eq!(format!("{:x}", Sha256::digest(&argb)), expected, "{name}");
        }
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_AEGPULAB and local Release worker"]
    fn real_aegpulab_gui_copy_and_box_pixels() {
        let plugin = PathBuf::from(std::env::var("AEXCOMPAT_TEST_AEGPULAB").unwrap());
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-aegpulab");
        let input = directory.join("input.png");
        let source = image::RgbaImage::from_fn(32, 24, |x, y| {
            image::Rgba([
                ((x * 7 + y * 3) % 256) as u8,
                ((y * 9 + x * 2) % 256) as u8,
                ((x * 5 + y * 11) % 256) as u8,
                255,
            ])
        });
        source.save(&input).unwrap();
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert_eq!(
            app.parameters.iter().find(|p| p.slot == 1).unwrap().name,
            "Effect"
        );
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, input);
        let _ = ctx.end_pass();
        for mode in [1, 3] {
            app.apply_debug_request_document(
                &serde_json::json!({"schema_version":1,
                "timing":{"frame":0,"fps":30,"duration_frames":300},
                "assignments":[{"slot":1,"value":mode},{"slot":2,"value":1},
                    {"slot":4,"value":3},{"slot":5,"value":1}]}),
                &directory.join("request.json"),
            )
            .unwrap();
            let output = directory.join(format!("mode-{mode}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(app.preview.as_ref().unwrap().size(), [32, 24]);
            let result = image::open(output).unwrap().to_rgba8();
            assert_eq!(result.dimensions(), (32, 24));
            for (x, y, actual) in result.enumerate_pixels() {
                let mut expected = *source.get_pixel(x, y);
                if mode == 3 {
                    for c in 0..4 {
                        let mut sum = 0u32;
                        for dy in -3..=3 {
                            for dx in -3..=3 {
                                sum += source.get_pixel(
                                    (x as i32 + dx).clamp(0, 31) as u32,
                                    (y as i32 + dy).clamp(0, 23) as u32,
                                )[c] as u32;
                            }
                        }
                        expected[c] = (sum / 49) as u8;
                    }
                }
                assert_eq!(*actual, expected, "mode={mode} x={x} y={y}");
            }
        }
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_AEGPUPROBE and local Release worker"]
    fn real_aegpuprobe_gui_float_invert_pixels() {
        let plugin = PathBuf::from(std::env::var("AEXCOMPAT_TEST_AEGPUPROBE").unwrap());
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-aegpuprobe");
        let input = directory.join("input.png");
        let source = image::RgbaImage::from_fn(32, 24, |x, y| {
            image::Rgba([(x * 7) as u8, (y * 9) as u8, ((x ^ y) * 5) as u8, 255])
        });
        source.save(&input).unwrap();
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        app.pixel_format = aexcompat_broker::image_render::RenderPixelFormat::Argb32f;
        app.gpu_backend = aexcompat_broker::image_render::RenderGpuBackend::Auto;
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, input);
        let _ = ctx.end_pass();
        app.apply_debug_request_document(&serde_json::json!({"schema_version":1,
            "assignments":[{"slot":1,"value":1},{"slot":2,"value":1},{"slot":3,"value":0},{"slot":4,"value":1}]}),&directory.join("request.json")).unwrap();
        let output = directory.join("invert.png");
        app.render_to(output.clone());
        drain(&mut app);
        fs::write(directory.join("report.txt"), &app.report).unwrap();
        eprintln!("AeGpuProbe evidence: {}", directory.display());
        assert_eq!(app.status, "AEX output ready.", "{}", app.report);
        let result = image::open(output).unwrap().to_rgba8();
        assert_eq!(result.dimensions(), source.dimensions());
        for (x, y, p) in result.enumerate_pixels() {
            let original = source.get_pixel(x, y);
            for c in 0..3 {
                assert!(
                    p[c].abs_diff(255 - original[c]) <= 1,
                    "x={x} y={y} c={c} actual={} original={}",
                    p[c],
                    original[c]
                );
            }
            assert_eq!(p[3], original[3]);
        }
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires AEXCOMPAT_TEST_AEGPUPROBE, observed AEXCOMPAT_TEST_GPU_POLICY, CUDA and Release worker"]
    fn real_aegpuprobe_explicit_cuda_invert_pixels() {
        use aexcompat_broker::image_render as render;
        let plugin = PathBuf::from(std::env::var("AEXCOMPAT_TEST_AEGPUPROBE").unwrap());
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("aegpuprobe-explicit-cuda");
        let input = directory.join("input.png");
        let output = directory.join("invert.png");
        let source = image::RgbaImage::from_fn(32, 24, |x, y| {
            image::Rgba([(x * 7) as u8, (y * 9) as u8, ((x ^ y) * 5) as u8, 255])
        });
        source.save(&input).unwrap();
        let sha = format!("{:x}", Sha256::digest(read_bounded_pe(&plugin).unwrap()));
        let mut parameters = render::inspect_experimental(&repository, &plugin, &sha).unwrap();
        for (slot, name, value) in [
            (1, "Enable GPU Probe", 1.0),
            (2, "Request GPU SmartRender", 1.0),
            (3, "CUDA Copy Output", 0.0),
            (4, "CUDA Invert Output", 1.0),
        ] {
            let parameter = parameters.iter_mut().find(|p| p.slot == slot).unwrap();
            assert_eq!(parameter.name, name);
            parameter.value = value;
        }
        let policy = aexcompat_broker::runtime_module_policy::parse_and_validate(
            &fs::read(std::env::var("AEXCOMPAT_TEST_GPU_POLICY").unwrap()).unwrap(),
        )
        .unwrap();
        let prepared = render::prepare_gpu_runtime_policy(
            &repository,
            &plugin,
            &sha,
            render::RenderGpuBackend::Cuda,
            policy,
            Vec::new(),
        )
        .unwrap();
        let result =
            render::render_experimental_image_with_approved_dependencies_and_gpu_runtime_policy(
                &repository,
                &plugin,
                &sha,
                &input,
                &output,
                &parameters,
                render::RenderTiming::default(),
                true,
                render::RenderPixelFormat::Argb32f,
                None,
                None,
                render::RenderGpuBackend::Cuda,
                Vec::new(),
                Some(prepared.as_input()),
            );
        fs::write(directory.join("report.txt"), format!("{result:#?}")).unwrap();
        eprintln!("AeGpuProbe explicit CUDA evidence: {}", directory.display());
        let report = result.unwrap();
        assert_eq!(report["gpu_render_dispatched"], true);
        let pixels = image::open(output).unwrap().to_rgba8();
        assert_eq!(pixels.dimensions(), source.dimensions());
        for (x, y, pixel) in pixels.enumerate_pixels() {
            let original = source.get_pixel(x, y);
            assert_eq!(pixel[3], original[3]);
            for c in 0..3 {
                assert!(
                    pixel[c].abs_diff(255 - original[c]) <= 1,
                    "x={x} y={y} c={c} actual={} original={}",
                    pixel[c],
                    original[c]
                );
            }
        }
        fs::remove_dir_all(directory).unwrap();
    }

    fn assert_real_temporal_bands(plugin_env: &str, temporary_prefix: &str) {
        let plugin = PathBuf::from(
            std::env::var(plugin_env)
                .expect("explicit AEX path")
                .replace('\\', "/"),
        );
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory(temporary_prefix);
        let input = directory.join("current.png");
        let past = directory.join("past.png");
        image::RgbaImage::from_fn(256, 144, |x, y| image::Rgba([x as u8, y as u8, 193, 255]))
            .save(&input)
            .unwrap();
        image::RgbaImage::from_fn(256, 144, |x, y| image::Rgba([x as u8, y as u8, 71, 255]))
            .save(&past)
            .unwrap();
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert_eq!(
            app.parameters.iter().find(|p| p.slot == 1).unwrap().name,
            "Time Frames"
        );
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, input);
        let _ = ctx.end_pass();
        for mode in [1, 2] {
            app.apply_debug_request_document(&serde_json::json!({
                "schema_version":1,"timing":{"frame":2,"fps":30,"duration_frames":300},
                "assignments":[{"slot":1,"value":2},{"slot":2,"value":1},{"slot":3,"value":1},
                    {"slot":4,"value":mode},{"slot":5,"value":0},{"slot":7,"value":1},{"slot":8,"value":100}],
                "timed_layers":[{"slot":0,"time":1,"time_scale":30,"image":past}]
            }),&directory.join("request.json")).unwrap();
            assert_eq!(app.timed_layers.len(), 1);
            let output = directory.join(format!("output{mode}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(app.preview.as_ref().unwrap().size(), [256, 144]);
            let expected = image::RgbaImage::from_fn(256, 144, |x, y| {
                image::Rgba([
                    x as u8,
                    y as u8,
                    if (mode == 1 && x < 128) || (mode == 2 && y < 72) {
                        71
                    } else {
                        193
                    },
                    255,
                ])
            });
            assert_eq!(image::open(output).unwrap().to_rgba8(), expected);
        }
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_MEDIAN_PRO and local Release worker"]
    fn real_median_pro_gui_preserves_structure_and_removes_impulses() {
        let plugin = PathBuf::from(
            std::env::var("AEXCOMPAT_TEST_MEDIAN_PRO")
                .expect("explicit AEX path")
                .replace('\\', "/"),
        );
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-median-pro");
        let input = directory.join("input.png");
        let clean = image::RgbaImage::from_fn(256, 144, |x, _| {
            image::Rgba(if x < 128 {
                [83, 127, 191, 255]
            } else {
                [173, 61, 107, 255]
            })
        });
        let mut original = clean.clone();
        for (x, y, v) in [(32, 32, 0), (96, 64, 255), (160, 96, 0), (224, 112, 255)] {
            original.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
        original.save(&input).unwrap();
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert_eq!(
            app.parameters
                .iter()
                .find(|p| p.slot == 5)
                .expect("Mix parameter")
                .name,
            "Mix with Original"
        );
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, input.clone());
        let _ = ctx.end_pass();
        for mix in [0, 100] {
            app.apply_debug_request_document(&serde_json::json!({
                "schema_version":1,"timing":{"frame":0,"fps":30,"duration_frames":300},
                "assignments":[{"slot":1,"value":1},{"slot":2,"value":1},{"slot":3,"value":0},{"slot":4,"value":1},{"slot":5,"value":mix}]
            }), &directory.join("request.json")).unwrap();
            let output = directory.join(format!("output-{mix}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(app.preview.as_ref().unwrap().size(), [256, 144]);
            assert_eq!(
                image::open(&output).unwrap().to_rgba8(),
                if mix == 0 {
                    original.clone()
                } else {
                    clean.clone()
                }
            );
        }
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires explicit AEXCOMPAT_TEST_MINIMAX_MAP and local Release worker"]
    fn real_minimax_gui_applies_radius_map_and_signed_extrema() {
        let plugin = PathBuf::from(
            std::env::var("AEXCOMPAT_TEST_MINIMAX_MAP")
                .expect("explicit AEX path")
                .replace('\\', "/"),
        );
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let directory = temporary_directory("gui-minimax-map");
        let input = directory.join("input.png");
        let clean = image::RgbaImage::from_fn(256, 144, |x, _| {
            image::Rgba(if x < 128 {
                [83, 127, 191, 255]
            } else {
                [173, 61, 107, 255]
            })
        });
        let mut original = clean.clone();
        for (x, y, v) in [(32, 32, 0), (96, 64, 255), (160, 96, 0), (224, 112, 255)] {
            original.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
        original.save(&input).unwrap();
        let map_path = directory.join("map.png");
        image::RgbaImage::from_pixel(256, 144, image::Rgba([255, 255, 255, 255]))
            .save(&map_path)
            .unwrap();
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert_eq!(
            app.parameters
                .iter()
                .find(|p| p.slot == 8)
                .expect("Radius map parameter")
                .name,
            "Radius Map"
        );
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, input.clone());
        let _ = ctx.end_pass();
        for amount in [-1, 1] {
            app.apply_debug_request_document(&serde_json::json!({
                "schema_version":1,"timing":{"frame":0,"fps":30,"duration_frames":300},
                "assignments":[{"slot":1,"value":amount},{"slot":2,"value":1},{"slot":3,"value":1},{"slot":4,"value":1},{"slot":5,"value":1},{"slot":6,"value":1},{"slot":7,"value":100},{"slot":8,"layer":map_path}]
            }), &directory.join("request.json")).unwrap();
            let output = directory.join(format!("output-{amount}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(app.preview.as_ref().unwrap().size(), [256, 144]);
            let expected = image::RgbaImage::from_fn(256, 144, |x, y| {
                let mut pixel = *original.get_pixel(x, y);
                for c in 0..3 {
                    let values = [-1_i32, 0, 1].map(|dx| {
                        original
                            .get_pixel((x as i32 + dx).clamp(0, 255) as u32, y)
                            .0[c]
                    });
                    pixel.0[c] = if amount > 0 {
                        *values.iter().max().unwrap()
                    } else {
                        *values.iter().min().unwrap()
                    };
                }
                pixel
            });
            assert_eq!(image::open(&output).unwrap().to_rgba8(), expected);
        }
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn aegp_roundtrip_gui_exposes_and_routes_each_shipping_action() {
        let ui_kit = AexUiKit::default();
        for expected in AegpRoundtripAction::ALL {
            let click = |busy| {
                let ctx = egui::Context::default();
                let mut rect = egui::Rect::NOTHING;
                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500.0, 500.0),
                    )),
                    ..Default::default()
                });
                egui::CentralPanel::default().show(&ctx, |ui| {
                    let (clicked, buttons) = show_aegp_roundtrip_actions(ui, busy, &ui_kit);
                    assert_eq!(clicked, None);
                    assert_eq!(
                        buttons
                            .iter()
                            .map(|(action, _)| *action)
                            .collect::<Vec<_>>(),
                        AegpRoundtripAction::ALL
                    );
                    rect = buttons
                        .into_iter()
                        .find(|(action, _)| *action == expected)
                        .unwrap()
                        .1;
                });
                let _ = ctx.end_pass();

                ctx.begin_pass(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(500.0, 500.0),
                    )),
                    events: vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                });
                let mut clicked = None;
                egui::CentralPanel::default().show(&ctx, |ui| {
                    clicked = show_aegp_roundtrip_actions(ui, busy, &ui_kit).0;
                });
                let _ = ctx.end_pass();
                clicked
            };
            assert_eq!(click(false), Some(expected));
            assert_eq!(click(true), None, "busy GUI must reject a second task");
        }

        let expected = [
            (
                "dispatch_aegp_keyframe_roundtrip",
                aexcompat_broker::image_render::dispatch_experimental_aegp_keyframe_roundtrip
                    as AegpRoundtripDispatch,
            ),
            (
                "dispatch_aegp_seek_roundtrip",
                aexcompat_broker::image_render::dispatch_experimental_aegp_seek_roundtrip
                    as AegpRoundtripDispatch,
            ),
            (
                "dispatch_aegp_trim_roundtrip",
                aexcompat_broker::image_render::dispatch_experimental_aegp_trim_roundtrip
                    as AegpRoundtripDispatch,
            ),
            (
                "dispatch_aegp_switch_roundtrip",
                aexcompat_broker::image_render::dispatch_experimental_aegp_switch_roundtrip
                    as AegpRoundtripDispatch,
            ),
        ];
        for (action, (operation, dispatch)) in AegpRoundtripAction::ALL.into_iter().zip(expected) {
            let spec = action.spec();
            assert_eq!(spec.operation, operation);
            assert!(std::ptr::fn_addr_eq(spec.dispatch, dispatch));
        }
    }

    #[test]
    fn aegp_roundtrip_app_dispatch_preserves_selection_and_task_lifecycle() {
        let root = temporary_directory("ui-aegp-roundtrip-dispatch");
        let plugin = root.join("provider.aex");
        fs::write(&plugin, b"fixture").unwrap();
        let mut app = HarnessApp::new(root.clone());
        app.selection = Some(Selection {
            path: plugin.clone(),
            sha256: "approved-hash".into(),
            size: 7,
            modified: Some(SystemTime::UNIX_EPOCH),
        });
        let (observed_sender, observed_receiver) = mpsc::channel();
        app.dispatch_aegp_roundtrip_with(
            AegpRoundtripAction::Seek,
            move |repository, path, hash| {
                observed_sender
                    .send((
                        repository.to_path_buf(),
                        path.to_path_buf(),
                        hash.to_owned(),
                    ))
                    .unwrap();
                Ok(serde_json::json!({"event_requested": "seek_roundtrip"}))
            },
        );

        assert!(app.busy);
        assert_eq!(app.status, AegpRoundtripAction::Seek.status());
        let observed = observed_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert_eq!(observed, (root.clone(), plugin, "approved-hash".into()));
        let result = app
            .receiver
            .as_ref()
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert!(result.success);
        assert_eq!(
            result.operation.as_deref(),
            Some(AegpRoundtripAction::Seek.spec().operation)
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result.body).unwrap()["event_requested"],
            "seek_roundtrip"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn input_image_path_uses_one_state_transition_for_picker_and_drop() {
        let root = temporary_directory("ui-dropped-input");
        let valid = root.join("frame.PNG");
        let invalid = root.join("notes.txt");
        image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]))
            .save(&valid)
            .unwrap();
        fs::write(&invalid, b"not an image").unwrap();
        assert!(is_supported_input_image(&valid));
        assert!(!is_supported_input_image(&invalid));
        assert!(!is_supported_input_image(&root.join("missing.png")));
        let path_drop = egui::DroppedFile {
            path: Some(valid.clone()),
            ..Default::default()
        };
        let memory_drop = egui::DroppedFile {
            name: "clipboard.png".into(),
            bytes: Some(std::sync::Arc::from(&b"payload"[..])),
            ..Default::default()
        };
        assert_eq!(
            crate::shared_ui::single_supported_dropped_path(
                std::slice::from_ref(&path_drop),
                is_supported_input_image,
            ),
            Some(valid.clone())
        );
        assert!(
            crate::shared_ui::single_supported_dropped_path(
                std::slice::from_ref(&memory_drop),
                is_supported_input_image,
            )
            .is_none()
        );
        assert!(
            crate::shared_ui::single_supported_dropped_path(
                &[path_drop, memory_drop],
                is_supported_input_image,
            )
            .is_none()
        );

        let ctx = egui::Context::default();
        let mut app = HarnessApp::new(root.clone());
        app.output_image = Some(root.join("stale-output.png"));
        app.viewer_mode = 2;
        app.load_input_path(&ctx, valid.clone());
        assert_eq!(app.input_image.as_deref(), Some(valid.as_path()));
        assert_eq!(app.input_preview.as_ref().unwrap().size(), [3, 2]);
        assert!(app.output_image.is_none());
        assert_eq!(app.viewer_mode, 0);
        assert_eq!(app.status, "Input image loaded. Ready to render.");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn analysis_panel_uses_shared_bounded_state() {
        let mut state = crate::gui_state::AnalysisPaneState::default();
        state.set_width(0.0);
        assert_eq!(state.width, crate::gui_state::AnalysisPaneState::MIN_WIDTH);
        state.set_width(f32::MAX);
        assert_eq!(state.width, crate::gui_state::AnalysisPaneState::MAX_WIDTH);
        state.set_width(540.0 + 8.0);
        assert_eq!(state.width, 548.0);
    }

    #[test]
    fn render_success_requires_a_displayable_output_image() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-ui-output-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let valid = root.join("valid.png");
        image::RgbaImage::from_pixel(2, 3, image::Rgba([1, 2, 3, 255]))
            .save(&valid)
            .unwrap();
        let prepared = prepare_render_preview(true, Some("render_image"), Some(&valid))
            .expect("valid PNG must complete the UI render boundary")
            .expect("image render must publish a preview");
        assert_eq!(prepared.0, valid);
        assert_eq!(prepared.1.size, [2, 3]);

        let missing = root.join("missing.png");
        assert!(
            prepare_render_preview(true, Some("render_image"), Some(&missing))
                .unwrap_err()
                .contains("not displayable")
        );
        assert!(
            prepare_render_preview(true, Some("render_image"), None)
                .unwrap_err()
                .contains("without an output image")
        );
        assert!(
            prepare_render_preview(false, Some("render_image"), Some(&missing))
                .unwrap()
                .is_none(),
            "a native failure must retain its original report"
        );
        assert!(
            prepare_render_preview(true, Some("render_audio"), Some(&missing))
                .unwrap()
                .is_none(),
            "non-image outputs must not be decoded as previews"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ui_output_failure_preserves_native_result_and_surfaces_details() {
        let report = report_ui_output_failure(
            r#"{"passed":true,"worker_diagnostics":{"stage":"render"}}"#,
            "native render output is not displayable",
        );
        let value: serde_json::Value = serde_json::from_str(&report).unwrap();
        assert_eq!(value["native_passed"], true);
        assert_eq!(value["passed"], false);
        assert_eq!(value["worker_diagnostics"]["stage"], "render");
        assert_eq!(value["ui_output"]["displayable"], false);
        assert_eq!(
            native_failure_status(Some("render_image"), &report),
            "AEX output could not be loaded."
        );
        assert_eq!(
            visible_report_summary(&report).as_deref(),
            Some("native render output is not displayable")
        );

        let missing_worker = serde_json::json!({
            "passed": false,
            "error": "session open failed: local worker binary is missing or unreadable"
        });
        let missing_worker = missing_worker.to_string();
        assert_eq!(
            native_failure_status(Some("render_image"), &missing_worker),
            "Required render worker is missing or unreadable."
        );
        assert!(
            visible_report_summary(&missing_worker)
                .unwrap()
                .contains("local worker binary is missing")
        );
    }

    #[test]
    fn render_worker_preflight_and_disconnected_tasks_are_explicit() {
        let repository = Path::new("C:/aexcompat");
        assert_eq!(
            required_render_worker_path(repository, false),
            repository.join("target/minihost-build/aex_worker.exe")
        );
        assert_eq!(
            required_render_worker_path(repository, true),
            repository.join("target/minihost-build/aex_worker.exe")
        );

        let (sender, receiver) = mpsc::channel();
        drop(sender);
        let result = receive_task_result(&receiver).expect("disconnect must become a UI result");
        assert!(!result.success);
        assert!(
            visible_report_summary(&result.body)
                .unwrap()
                .contains("ended without returning a result")
        );
    }

    #[test]
    fn poll_connects_native_image_result_to_ui_preview_and_failure_state() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-ui-poll-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let output = root.join("output.png");
        image::RgbaImage::from_pixel(4, 5, image::Rgba([9, 8, 7, 255]))
            .save(&output)
            .unwrap();
        let mut app = HarnessApp::new(root.clone());
        let (sender, receiver) = mpsc::channel();
        sender
            .send(TaskResult {
                success: true,
                body: serde_json::json!({ "passed": true }).to_string(),
                output: Some(output.clone()),
                identity: None,
                operation: Some("render_image".into()),
                diagnostic_eligible: false,
            })
            .unwrap();
        app.receiver = Some(receiver);
        app.busy = true;
        app.rendering = true;
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        app.poll(&ctx);
        let _ = ctx.end_pass();
        assert_eq!(app.output_image.as_deref(), Some(output.as_path()));
        assert_eq!(app.preview.as_ref().unwrap().size(), [4, 5]);
        assert_eq!(app.viewer_mode, 1);
        assert_eq!(app.status, "AEX output ready.");
        assert!(!app.busy);
        assert!(!app.rendering);

        let (sender, receiver) = mpsc::channel();
        sender
            .send(TaskResult {
                success: false,
                body: serde_json::json!({
                    "passed": false,
                    "error": "session open failed: local worker binary is missing or unreadable"
                })
                .to_string(),
                output: None,
                identity: None,
                operation: Some("render_image".into()),
                diagnostic_eligible: false,
            })
            .unwrap();
        app.receiver = Some(receiver);
        app.busy = true;
        app.rendering = true;
        ctx.begin_pass(Default::default());
        app.poll(&ctx);
        let _ = ctx.end_pass();
        assert!(app.output_image.is_none());
        assert!(app.preview.is_none());
        assert_eq!(
            app.status,
            "Required render worker is missing or unreadable."
        );
        assert!(
            visible_report_summary(&app.report)
                .unwrap()
                .contains("local worker binary is missing")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn auto_update_render_completion_preserves_compare_view() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-ui-compare-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let output = root.join("output.png");
        image::RgbaImage::from_pixel(4, 5, image::Rgba([9, 8, 7, 255]))
            .save(&output)
            .unwrap();
        let ctx = egui::Context::default();
        let mut app = HarnessApp::new(root.clone());
        app.input_preview = Some(ctx.load_texture(
            "compare-input",
            egui::ColorImage::new([4, 5], vec![egui::Color32::BLACK; 20]),
            egui::TextureOptions::LINEAR,
        ));
        app.viewer_mode = 2;
        let (sender, receiver) = mpsc::channel();
        sender
            .send(TaskResult {
                success: true,
                body: serde_json::json!({ "passed": true }).to_string(),
                output: Some(output),
                identity: None,
                operation: Some("render_image".into()),
                diagnostic_eligible: false,
            })
            .unwrap();
        app.receiver = Some(receiver);
        app.busy = true;
        app.rendering = true;
        ctx.begin_pass(Default::default());
        app.poll(&ctx);
        let _ = ctx.end_pass();
        assert!(app.preview.is_some());
        assert_eq!(app.viewer_mode, 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn closing_aex_clears_plugin_authority_and_output_but_keeps_input() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-ui-close-aex-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let ctx = egui::Context::default();
        let mut app = HarnessApp::new(root.clone());
        app.selection = Some(Selection {
            path: root.join("effect.aex"),
            size: 123,
            sha256: "ABC".into(),
            modified: None,
        });
        app.session_approved = true;
        app.input_image = Some(root.join("input.png"));
        app.output_image = Some(root.join("output.png"));
        app.preview = Some(ctx.load_texture(
            "close-output",
            egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]),
            egui::TextureOptions::LINEAR,
        ));
        app.viewer_mode = 2;
        app.pending_live_render = true;
        app.render_after_parameter_change = true;

        app.close_selected_aex();

        assert!(app.selection.is_none());
        assert!(!app.session_approved);
        assert!(app.parameters.is_empty());
        assert!(app.smart_render_capability.is_none());
        assert!(app.output_image.is_none());
        assert!(app.preview.is_none());
        assert_eq!(app.viewer_mode, 0);
        assert!(!app.pending_live_render);
        assert!(!app.render_after_parameter_change);
        assert_eq!(
            app.input_image.as_deref(),
            Some(root.join("input.png").as_path())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn individual_reset_queues_the_same_supervised_or_live_transition_as_an_edit() {
        let mut app = HarnessApp::new(std::env::temp_dir());
        let now = Instant::now();

        app.queue_parameter_transition(7, true, now);
        assert_eq!(app.pending_parameter_slot, Some(7));
        assert!(!app.pending_live_render);
        assert_eq!(
            app.live_render_due,
            Some(now + std::time::Duration::from_millis(500))
        );

        app.pending_parameter_slot = None;
        app.live_render_due = None;
        app.live_render = true;
        app.queue_parameter_transition(8, false, now);
        assert_eq!(app.pending_parameter_slot, None);
        assert!(app.pending_live_render);
        assert_eq!(
            app.live_render_due,
            Some(now + std::time::Duration::from_millis(500))
        );
    }

    #[test]
    fn reset_all_preserves_runtime_ui_state_and_queues_live_render_only_on_change() {
        let mut app = HarnessApp::new(std::env::temp_dir());
        let mut default = parameter(1, "integer");
        default.value = 10.0;
        let mut current = default.clone();
        current.value = 40.0;
        current.enabled = false;
        current.visible = false;
        current.supervised = true;
        current.minimum = -100.0;
        current.maximum = 500.0;
        app.parameter_defaults = vec![default];
        app.parameters = vec![current];
        app.live_render = true;
        let now = Instant::now();

        assert!(app.reset_all_parameters(now));
        assert_eq!(app.parameters[0].value, 10.0);
        assert!(!app.parameters[0].enabled);
        assert!(!app.parameters[0].visible);
        assert!(app.parameters[0].supervised);
        assert_eq!(
            (app.parameters[0].minimum, app.parameters[0].maximum),
            (-100.0, 500.0)
        );
        assert!(app.pending_live_render);
        assert_eq!(
            app.live_render_due,
            Some(now + std::time::Duration::from_millis(500))
        );

        app.pending_live_render = false;
        app.live_render_due = None;
        assert!(!app.reset_all_parameters(now));
        assert!(!app.pending_live_render);
        assert_eq!(app.live_render_due, None);
    }

    #[test]
    fn render_input_changes_invalidate_the_previous_ui_output() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-ui-invalidate-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let mut app = HarnessApp::new(root.clone());
        let ctx = egui::Context::default();
        app.output_image = Some(root.join("old.png"));
        app.pixel_comparison = Some(Err("old comparison".into()));
        app.viewer_mode = 2;
        app.preview = Some(ctx.load_texture(
            "old-output",
            egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]),
            egui::TextureOptions::LINEAR,
        ));
        app.invalidate_render_output();
        assert!(app.output_image.is_none());
        assert!(app.preview.is_none());
        assert!(app.pixel_comparison.is_none());
        assert_eq!(app.viewer_mode, 0);

        app.output_image = Some(root.join("old-again.png"));
        app.preview = Some(ctx.load_texture(
            "old-output-again",
            egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]),
            egui::TextureOptions::LINEAR,
        ));
        app.viewer_mode = 2;
        app.clear_render_output();
        assert!(app.output_image.is_none());
        assert!(app.preview.is_none());
        assert_eq!(
            app.viewer_mode, 2,
            "Auto Update invalidation must preserve the selected compare view"
        );

        let before = app.current_render_input_fingerprint();
        app.frame += 1;
        assert_ne!(app.current_render_input_fingerprint(), before);
        app.apply_custom_ui_click_to_render = true;
        let before_click_edit = app.current_render_input_fingerprint();
        app.custom_ui_click_point[0] += 1;
        assert_ne!(app.current_render_input_fingerprint(), before_click_edit);
        let before_color_edit = app.current_render_input_fingerprint();
        app.custom_ui_click_color[2] = 0.5;
        assert_ne!(app.current_render_input_fingerprint(), before_color_edit);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cli_inspection_paths_are_absolute_and_deverbatim() {
        let relative = Path::new("Cargo.toml");
        let canonical = canonical_deverbatim(relative).unwrap();
        assert!(canonical.is_absolute());
        assert!(!canonical.as_os_str().to_string_lossy().starts_with(r"\\?\"));
        let roots = inspect_dependency_roots(relative, &[]).unwrap();
        assert_eq!(roots, vec![canonical.parent().unwrap().to_path_buf()]);
    }

    #[test]
    fn inspected_defaults_are_normalized_before_ui_or_cli_rendering() {
        let parameter = aexcompat_broker::image_render::InteractiveParameter {
            slot: 5,
            name: "Brightness Gain".to_owned(),
            kind: "float".to_owned(),
            minimum: 1.0,
            maximum: 100.0,
            value: 0.1,
            choices: Vec::new(),
            color: [0; 4],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0; 2],
        };
        let arbitrary = aexcompat_broker::image_render::InteractiveParameter {
            slot: 14,
            name: "Ramp".to_owned(),
            kind: "arbitrary_data".to_owned(),
            debug_summary: None,
            ..parameter.clone()
        };
        let descriptor = |slot, kind: &str| aexcompat_broker::image_render::InteractiveParameter {
            slot,
            name: kind.to_owned(),
            kind: kind.to_owned(),
            minimum: 0.0,
            maximum: 0.0,
            value: 0.0,
            ..parameter.clone()
        };
        let radial_blur_parameters = [
            parameter.clone(),
            arbitrary,
            descriptor(3, "group_start"),
            descriptor(8, "group_end"),
            descriptor(26, "layer"),
        ];
        let normalized = normalize_inspected_ui_parameters(&radial_blur_parameters);
        assert_eq!(normalized[0].value, 0.1);
        assert_eq!(normalized.len(), 5, "UI keeps every discovered descriptor");
        assert_eq!(normalized[1].kind, "arbitrary_data");
        let defaults = normalized.clone();
        let sendable = parameters_for_native_action(&normalized, &defaults);
        assert_eq!(
            sendable.len(),
            0,
            "render omits unsendable defaults and display-only descriptors"
        );
        assert!(
            aexcompat_broker::image_render::encode_interactive_payload(&sendable).is_ok(),
            "normalized discovery defaults must be renderable"
        );
        assert!(
            aexcompat_broker::image_render::encode_interactive_payload(&[parameter]).is_err(),
            "an explicit out-of-range edit remains fail-closed"
        );
        let mut displayed = normalized.clone();
        displayed[1].debug_summary = Some(String::new());
        assert_eq!(
            parameters_for_native_action(&displayed, &defaults).len(),
            0,
            "opening an empty arbitrary editor does not make it sendable"
        );
        displayed[1].debug_summary = Some("edited ramp".to_owned());
        assert_eq!(
            parameters_for_native_action(&displayed, &defaults).len(),
            1,
            "an explicit printable arbitrary edit is retained"
        );
    }

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
            dependency_search_dirs: vec![],
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
        let mut rooted = key(auto);
        rooted.dependency_search_dirs = vec![PathBuf::from(r"C:\runtime")];
        assert!(
            key(auto) != rooted,
            "a runtime-root change must reopen the resident worker"
        );
        assert!(selected_interactive_session_selection(smart, false, true).is_err());

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
            selected_interactive_session_selection(classic, false, true)
                .expect("an explicit same-path Classic selection is retained")
                .source
                .report_name(),
            "manual_classic"
        );
        assert!(selected_interactive_session_selection(classic, true, true).is_err());
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

    #[test]
    fn parameter_inspection_state_distinguishes_zero_filtering_and_failure_inputs() {
        let parameter = |visible: bool| {
            serde_json::from_value::<aexcompat_broker::image_render::InteractiveParameter>(
                serde_json::json!({
                    "slot": 1, "name": "Amount", "kind": "float",
                    "minimum": 0.0, "maximum": 100.0, "value": 25.0,
                    "choices": [], "color": [0, 0, 0, 0],
                    "components": [0.0, 0.0, 0.0], "component_count": 0,
                    "layer_path": null, "enabled": true, "visible": visible,
                    "supervised": false,
                }),
            )
            .unwrap()
        };
        let report = |raw_count: usize| {
            serde_json::json!({
                "worker_diagnostics": {
                    "parameter_metadata": (0..raw_count)
                        .map(|index| serde_json::json!({"index": index + 1}))
                        .collect::<Vec<_>>()
                }
            })
        };

        assert_eq!(
            parameter_inspection_state(&report(0), &[]).unwrap(),
            ParameterInspectionState::ZeroParameters
        );
        assert_eq!(
            parameter_inspection_state(&report(1), &[]).unwrap(),
            ParameterInspectionState::UnsupportedParameters
        );
        assert_eq!(
            parameter_inspection_state(&report(1), &[parameter(false)]).unwrap(),
            ParameterInspectionState::HiddenParameters
        );
        assert_eq!(
            parameter_inspection_state(&report(1), &[parameter(true)]).unwrap(),
            ParameterInspectionState::Ready
        );
        assert!(parameter_inspection_state(&serde_json::json!({}), &[]).is_err());
        assert!(parameter_inspection_state(&report(0), &[parameter(true)]).is_err());

        assert_eq!(
            parameter_inspection_message(ParameterInspectionState::ZeroParameters),
            Some((
                "This effect intentionally declared no parameters.",
                "このエフェクトはパラメーターを定義していません。"
            ))
        );
        assert_eq!(
            parameter_inspection_message(ParameterInspectionState::UnsupportedParameters),
            Some((
                "This effect declared parameters, but none use supported control types.",
                "パラメーターはありますが、対応しているコントロール形式がありません。"
            ))
        );
        assert_eq!(
            parameter_inspection_message(ParameterInspectionState::HiddenParameters),
            Some((
                "This effect declared controls, but all are hidden by the plug-in.",
                "コントロールはありますが、プラグインによってすべて非表示です。"
            ))
        );
        assert_eq!(
            parameter_inspection_message(ParameterInspectionState::Failed),
            Some((
                "Effect Controls inspection failed. See the diagnostic report below.",
                "エフェクトコントロールの検査に失敗しました。下の診断レポートを確認してください。"
            ))
        );
        assert_eq!(
            parameter_inspection_message(ParameterInspectionState::Ready),
            None
        );
        assert_eq!(
            parameter_inspection_status(
                ParameterInspectionState::HiddenParameters,
                &[parameter(false)],
                false
            ),
            "Effect Controls inspected: all declared controls are hidden. Render path: Classic."
        );
        assert_eq!(
            parameter_inspection_status(
                ParameterInspectionState::Ready,
                &[parameter(false), parameter(true)],
                true
            ),
            "Effect Controls ready: 1 visible parameter(s). Render path: SmartFX."
        );
    }

    #[test]
    fn render_action_requires_current_successful_inspection() {
        assert!(render_action_enabled(false, true, true, true, false, true));
        assert!(!render_action_enabled(
            false, true, true, true, false, false
        ));
        assert!(!render_action_enabled(
            false, true, true, false, false, true
        ));
        assert!(!render_action_enabled(false, true, true, true, true, true));
        assert!(!render_action_enabled(true, true, true, true, false, true));
        assert!(!render_action_enabled(
            false, false, true, true, false, true
        ));
        assert!(!render_action_enabled(
            false, true, false, true, false, true
        ));
    }

    #[test]
    fn selection_failure_keeps_the_open_time_snapshot() {
        let selection = selected_interactive_session_selection(
            InspectedRenderCapability {
                smart_render_advertised: true,
                out_flags2: 1 << 10,
            },
            true,
            true,
        )
        .expect("same-path manual SmartFX selection");
        let report: serde_json::Value = serde_json::from_str(&interactive_selection_failure(
            selection,
            "session invalidated; fallback failed".into(),
        ))
        .expect("structured failure report");
        assert_eq!(report["passed"], false);
        assert_eq!(report["render_path"], "smartfx");
        assert_eq!(report["smart_capability_source"], "manual_smart");
        assert_eq!(report["smart_capability_identity"], 1 << 10);
        assert_eq!(report["smart_capability_version"], 1);
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
            modified: None,
        });
        app.spawn_native("race_test", || Ok(("ok".into(), None)));
        app.selection = Some(Selection {
            path: PathBuf::from("second.aex"),
            size: 22,
            sha256: "22".repeat(32),
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
    fn gui_unedited_native_default_is_not_replayed_as_a_clamped_edit() {
        let mut seed = parameter(1, "float");
        seed.minimum = 1.0;
        seed.maximum = 1_000_000.0;
        let displayed = normalize_inspected_ui_parameters(&[seed]);
        let defaults = displayed.clone();
        let payload = parameters_for_image_render(&displayed, &defaults);
        assert!(
            payload.is_empty(),
            "opening controls must not author a seed edit"
        );
    }

    #[test]
    fn gui_saved_native_default_restores_after_a_later_edit() {
        let mut seed = parameter(1, "float");
        seed.minimum = 1.0;
        seed.maximum = 100.0;
        let mut app = HarnessApp::new(std::env::temp_dir());
        app.parameters = vec![seed.clone()];
        app.parameter_defaults = vec![seed];
        let saved = typed_request_document(
            &parameters_for_image_render(&app.parameters, &app.parameter_defaults),
            0,
            30,
            1,
            300,
            None,
        );
        app.parameters[0].value = 7.0;
        app.apply_debug_request_document(&saved, &std::env::temp_dir().join("native-default.json"))
            .unwrap();
        assert_eq!(
            app.parameters[0].value, 0.0,
            "saved native default must replace later edit"
        );
    }

    #[test]
    fn typed_request_omitted_scalar_defaults_are_not_replayed() {
        let mut seed = parameter(1, "float");
        seed.minimum = 1.0;
        seed.maximum = 1_000_000.0;
        let parameters = vec![seed, parameter(2, "layer")];
        let document = serde_json::json!({"schema_version":1,"assignments":[]});
        let payload = typed_render_request_parameters(&parameters, &document, None).unwrap();
        assert_eq!(payload.len(), 1, "unassigned scalar must stay native");
        assert_eq!(
            payload[0].kind, "layer",
            "timed layer declaration must survive"
        );
        assert!(aexcompat_broker::image_render::encode_interactive_payload(&payload).is_ok());
        assert_eq!(parameters[0].value, 0.0, "inspection model is unchanged");
        for value in [0.0, 1_000_001.0] {
            let invalid =
                serde_json::json!({"schema_version":1,"assignments":[{"slot":1,"value":value}]});
            assert!(
                typed_render_request_parameters(&parameters, &invalid, None)
                    .unwrap_err()
                    .contains("out of range")
            );
        }
        let valid = serde_json::json!({"schema_version":1,"assignments":[{"slot":1,"value":7.0}]});
        let edited = typed_render_request_parameters(&parameters, &valid, None).unwrap();
        assert_eq!(edited.len(), 2);
        assert_eq!(edited[0].value, 7.0);
        assert!(
            aexcompat_broker::image_render::encode_interactive_payload(&edited)
                .unwrap()
                .contains("f64=7")
        );
    }

    #[test]
    fn typed_assignment_preserves_opaque_native_defaults_and_explicit_edits() {
        let defaults = vec![parameter(1, "arbitrary_data"), parameter(2, "layer")];
        let mut omitted = defaults.clone();
        apply_typed_assignments(
            &mut omitted,
            &serde_json::json!({
                "schema_version": 1, "assignments": [{"slot": 2, "layer": "map.png"}]
            }),
            None,
        )
        .unwrap();
        assert_eq!(omitted.len(), 2);
        let payload = typed_parameters_for_render(&omitted);
        assert_eq!(payload.len(), 1);
        assert_eq!(payload[0].slot, 2);
        assert_eq!(payload[0].layer_path.as_deref(), Some(Path::new("map.png")));

        let mut edited = omitted;
        apply_typed_assignments(
            &mut edited,
            &serde_json::json!({
                "schema_version": 1, "assignments": [{"slot": 1, "text": "curve=7"}]
            }),
            None,
        )
        .unwrap();
        assert_eq!(edited.len(), 2);
        assert_eq!(edited[0].debug_summary.as_deref(), Some("curve=7"));
        assert_eq!(typed_parameters_for_render(&edited).len(), 2);

        let mut rejected = defaults.clone();
        assert!(
            apply_typed_assignments(
                &mut rejected,
                &serde_json::json!({
                    "schema_version": 1, "assignments": [{"slot": 1, "text": ""}]
                }),
                None
            )
            .is_err()
        );
        assert_eq!(rejected.len(), 2);
        assert!(rejected[0].debug_summary.is_none());
    }

    #[test]
    fn timed_request_samples_are_bounded_typed_and_rationally_unique() {
        let path = Path::new("C:/bundle/request.json");
        let sample = serde_json::json!({"slot":1,"time":1,"time_scale":3,"image":"frame.png"});
        let document = serde_json::json!({"timed_layers":[sample.clone()]});
        let parsed = typed_request_timed_layers(&document, path).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].slot, 1);
        assert_eq!(parsed[0].time.value, 1);
        assert_eq!(parsed[0].time.scale, 3);
        assert_eq!(parsed[0].image_path, Path::new("C:/bundle/frame.png"));
        assert!(
            typed_request_timed_layers(&serde_json::json!({}), path)
                .unwrap()
                .is_empty()
        );
        for (field, value) in [
            ("slot", serde_json::json!(1025)),
            ("time_scale", serde_json::json!(0)),
            ("time", serde_json::json!(1.5)),
            ("time", serde_json::json!(2147483648_i64)),
            ("image", serde_json::json!("")),
            ("extra", serde_json::json!(true)),
        ] {
            let mut invalid = sample.clone();
            invalid[field] = value;
            assert!(
                typed_request_timed_layers(&serde_json::json!({"timed_layers":[invalid]}), path)
                    .is_err()
            );
        }
        let mut equivalent = sample.clone();
        equivalent["time"] = 2.into();
        equivalent["time_scale"] = 6.into();
        assert!(
            typed_request_timed_layers(
                &serde_json::json!({"timed_layers":[sample.clone(),equivalent]}),
                path
            )
            .is_err()
        );
        assert!(
            typed_request_timed_layers(&serde_json::json!({"timed_layers":vec![sample;65]}), path)
                .is_err()
        );
        for invalid in [
            serde_json::Value::Null,
            serde_json::json!({}),
            serde_json::json!([null]),
        ] {
            assert!(
                typed_request_timed_layers(&serde_json::json!({"timed_layers":invalid}), path)
                    .is_err()
            );
        }
    }

    #[test]
    fn gui_debug_request_accepts_temporal_samples() {
        let mut app = HarnessApp::new(PathBuf::from("C:/repo"));
        app.parameters = vec![parameter(8, "layer")];
        app.apply_debug_request_document(
            &serde_json::json!({
                "schema_version":1, "assignments":[],
                "timing":{"frame":30,"fps":30,"duration_frames":300},
                "timed_layers":[
                    {"slot":8,"time":29,"time_scale":30,"image":"29.png"},
                    {"slot":8,"time":30,"time_scale":30,"image":"30.png"}
                ]
            }),
            Path::new("C:/samples/request.json"),
        )
        .unwrap();
        assert_eq!(app.frame, 30);
        assert_eq!(app.timed_layers.len(), 2);
        assert_eq!(app.timed_layers[0].time.value, 29);
        assert_eq!(app.timed_layers[1].time.value, 30);
        assert_eq!(
            app.timed_layers[0].image_path,
            Path::new("C:/samples/29.png")
        );
        let mut saved = typed_request_document(
            &app.parameters,
            app.frame,
            app.frames_per_second,
            app.frame_time_step,
            app.duration_frames,
            None,
        );
        write_timed_layers_to_document(&mut saved, &app.timed_layers);
        let mut restored = HarnessApp::new(PathBuf::from("C:/repo"));
        restored.parameters = app.parameters.clone();
        restored
            .apply_debug_request_document(&saved, Path::new("C:/elsewhere/copy.json"))
            .unwrap();
        assert_eq!(
            restored.timed_layers[1].image_path,
            Path::new("C:/samples/30.png")
        );
        assert_eq!(restored.timed_layers[1].time.scale, 30);
        let before = saved.clone();
        for slot in [9, 1025] {
            let mut invalid = saved.clone();
            invalid["timed_layers"][0]["slot"] = slot.into();
            invalid["timing"]["frame"] = 31.into();
            assert!(
                app.apply_debug_request_document(&invalid, Path::new("C:/bad.json"))
                    .is_err()
            );
            assert_eq!(app.frame, 30);
            let mut after = typed_request_document(
                &app.parameters,
                app.frame,
                app.frames_per_second,
                app.frame_time_step,
                app.duration_frames,
                None,
            );
            write_timed_layers_to_document(&mut after, &app.timed_layers);
            assert_eq!(before, after);
        }
        saved.as_object_mut().unwrap().remove("timed_layers");
        app.apply_debug_request_document(&saved, Path::new("C:/static.json"))
            .unwrap();
        assert!(app.timed_layers.is_empty());
        restored.close_selected_aex();
        assert!(restored.timed_layers.is_empty());
    }

    #[test]
    fn gui_imports_primary_time_samples_without_secondary_metadata() {
        let mut app = HarnessApp::new(PathBuf::from("C:/repo"));
        app.apply_debug_request_document(
            &serde_json::json!({
                "schema_version":1,"assignments":[],
                "timed_layers":[{"slot":0,"time":-1,"time_scale":30,"image":"past.png"}]
            }),
            Path::new("C:/samples/request.json"),
        )
        .unwrap();
        assert_eq!(app.timed_layers.len(), 1);
        assert_eq!(app.timed_layers[0].slot, 0);
        assert_eq!(app.timed_layers[0].time.value, -1);
        assert_eq!(
            app.timed_layers[0].image_path,
            Path::new("C:/samples/past.png")
        );
    }

    #[test]
    fn reinspection_replaces_temporal_state_on_success_and_failure() {
        for outcome in ["success", "failure", "malformed"] {
            let mut app = HarnessApp::new(PathBuf::from("C:/repo"));
            app.timed_layers
                .push(aexcompat_broker::image_render::TimedLayerImage {
                    slot: 8,
                    time: aexcompat_broker::image_render::AnimationTime {
                        value: 1,
                        scale: 30,
                    },
                    image_path: PathBuf::from("C:/old.png"),
                });
            let body = if outcome == "success" {
                serde_json::json!({"parameters":[],"worker_diagnostics":{
                    "advertised_out_flags2":0,"smart_render_advertised":false,
                    "parameter_metadata":[]
                }})
                .to_string()
            } else {
                "{}".into()
            };
            let (sender, receiver) = mpsc::channel();
            sender
                .send(TaskResult {
                    success: outcome != "failure",
                    body,
                    output: None,
                    identity: None,
                    operation: None,
                    diagnostic_eligible: false,
                })
                .unwrap();
            app.receiver = Some(receiver);
            app.busy = true;
            app.task_kind = TaskKind::InspectParameters;
            let ctx = egui::Context::default();
            ctx.begin_pass(Default::default());
            app.poll(&ctx);
            let _ = ctx.end_pass();
            assert!(
                app.timed_layers.is_empty(),
                "{outcome}: stale samples survived"
            );
            assert_eq!(
                app.parameter_inspection_state,
                if outcome == "success" {
                    ParameterInspectionState::ZeroParameters
                } else {
                    ParameterInspectionState::Failed
                }
            );
        }
    }

    #[test]
    #[ignore = "requires local Release worker and timed multilayer probe"]
    fn gui_temporal_samples_reach_native_pixels() {
        let repository =
            canonical_deverbatim(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .unwrap();
        let plugin = repository.join("target/pf-smart-timed-multilayer-probe-build/Release/pf_smart_timed_multilayer_probe.aex");
        assert!(plugin.is_file());
        let directory = temporary_directory("gui-temporal");
        let input = directory.join("input.png");
        image::RgbaImage::from_pixel(48, 32, image::Rgba([11, 22, 33, 255]))
            .save(&input)
            .unwrap();
        let mut app = HarnessApp::new(repository);
        let bytes = read_bounded_pe(&plugin).unwrap();
        app.selection = Some(Selection {
            path: plugin.clone(),
            size: bytes.len() as u64,
            sha256: format!("{:X}", Sha256::digest(&bytes)),
            modified: None,
        });
        app.accept_adjacent_discovery(discover_adjacent_imports(&plugin).unwrap());
        app.approve_session().unwrap();
        let ctx = egui::Context::default();
        let drain = |app: &mut HarnessApp| {
            while app.busy {
                ctx.begin_pass(Default::default());
                app.poll(&ctx);
                let _ = ctx.end_pass();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.inspect_parameters_async();
        drain(&mut app);
        assert!(app.smart_render_capability.is_some(), "{}", app.report);
        ctx.begin_pass(Default::default());
        app.load_input_path(&ctx, input);
        let _ = ctx.end_pass();
        let mut outputs = Vec::new();
        for shift in [0u8, 17] {
            let samples: Vec<_> = [(6, 8, 10u8), (1, 3, 40), (5, 4, 70)]
                .into_iter()
                .enumerate()
                .map(|(index, (time, scale, seed))| {
                    let path = directory.join(format!("sample-{shift}-{index}.png"));
                    image::RgbaImage::from_fn(48, 32, |x, y| {
                        image::Rgba([
                            x as u8 + seed + shift,
                            y as u8 * 2 + seed + shift,
                            seed + shift,
                            255,
                        ])
                    })
                    .save(&path)
                    .unwrap();
                    serde_json::json!({"slot":1,"time":time,"time_scale":scale,"image":path})
                })
                .collect();
            app.apply_debug_request_document(
                &serde_json::json!({
                    "schema_version":1,"assignments":[],
                    "timing":{"frame":0,"fps":30,"duration_frames":300},
                    "timed_layers":samples
                }),
                &directory.join("request.json"),
            )
            .unwrap();
            let output = directory.join(format!("output-{shift}.png"));
            app.render_to(output.clone());
            drain(&mut app);
            assert_eq!(app.status, "AEX output ready.", "{}", app.report);
            assert_eq!(app.output_image.as_ref(), Some(&output));
            assert_eq!(app.preview.as_ref().unwrap().size(), [48, 32]);
            let pixels = image::open(output).unwrap().to_rgba8();
            assert!(pixels.pixels().all(|p| p.0[3] == 255));
            for (x, y, pixel) in pixels.enumerate_pixels() {
                assert_eq!(
                    pixel.0,
                    [
                        x as u8 + 50 + shift,
                        y as u8 * 2 + 50 + shift,
                        50 + shift,
                        255
                    ]
                );
            }
            outputs.push(pixels);
        }
        assert_ne!(
            outputs[0], outputs[1],
            "temporal image changes must reach the AEX"
        );
        app.close_selected_aex();
        drop(app);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn image_render_projection_keeps_static_and_temporal_layer_declarations() {
        let mut static_layer = parameter(2, "layer");
        static_layer.layer_path = Some(PathBuf::from("C:/sample.png"));
        let parameters = vec![parameter(1, "layer"), static_layer, parameter(3, "float")];
        let native = parameters_for_native_action(&parameters, &parameters);
        assert_eq!(native.len(), 1);
        let image = parameters_for_image_render(&parameters, &parameters);
        assert_eq!(image.len(), 3);
        assert!(image[0].layer_path.is_none());
        assert_eq!(
            image[1].layer_path.as_deref(),
            Some(Path::new("C:/sample.png"))
        );
        assert_eq!(image[2].slot, 3);
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

        let retained_hash = refreshed.sha256.clone();
        let parameter = serde_json::from_value::<
            aexcompat_broker::image_render::InteractiveParameter,
        >(serde_json::json!({
            "slot": 1, "name": "Amount", "kind": "float",
            "minimum": 0.0, "maximum": 100.0, "value": 25.0,
            "choices": [], "color": [0, 0, 0, 0],
            "components": [0.0, 0.0, 0.0], "component_count": 0,
            "layer_path": null, "enabled": true, "visible": true,
            "supervised": false,
        }))
        .unwrap();
        app.parameters = vec![parameter.clone()];
        app.parameter_defaults = vec![parameter];
        app.parameter_inspection_state = ParameterInspectionState::Ready;
        app.inspect_after_refresh = false;
        app.refresh_aex();
        assert_eq!(app.selection.as_ref().unwrap().sha256, retained_hash);
        assert!(app.session_approved);
        assert!(app.inspect_after_refresh);
        assert_eq!(
            app.parameter_inspection_state,
            ParameterInspectionState::Loading
        );
        assert!(app.parameters.is_empty());
        assert!(app.parameter_defaults.is_empty());

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
