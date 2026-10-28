// Behavioral tests included in macos::tests.
fn transport_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("aexcompat-x07-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn check_input_format(name: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    assert!(supported_input_image(&path));
    let ctx = egui::Context::default();
    let mut app = MacHarnessApp::new(PathBuf::new());
    let _ = ctx.run(
        egui::RawInput {
            dropped_files: vec![egui::DroppedFile {
                path: Some(path.clone()),
                ..Default::default()
            }],
            ..Default::default()
        },
        |ctx| app.accept_dropped_input(ctx),
    );
    assert_eq!(app.input, Some(path.clone()), "{}", app.report);
    assert!(app.input_texture.is_some());
    for format in [
        MacRenderFormat::PngArgb8,
        MacRenderFormat::RawArgb16,
        MacRenderFormat::ExrArgb32f,
    ] {
        let (width, height, argb) = encode_resident_image(&path, format).unwrap();
        assert_eq!((width, height), (3, 2));
        let pixel = match format {
            MacRenderFormat::PngArgb8 => vec![255, 0, 0, 0],
            MacRenderFormat::RawArgb16 => [32768u16, 0, 0, 0]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect(),
            MacRenderFormat::ExrArgb32f => [1f32, 0., 0., 0.]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect(),
        };
        assert_eq!(argb, pixel.repeat(6));
    }
}

#[test]
fn gui_admits_jpeg() {
    check_input_format("black.jpg");
}
#[test]
fn gui_admits_bmp() {
    check_input_format("black.bmp");
}
#[test]
fn gui_admits_tiff() {
    check_input_format("black.tiff");
}
#[test]
fn gui_admits_webp() {
    check_input_format("black.webp");
}

#[test]
fn gui_image_filter_aliases_and_invalid_input() {
    for ext in INPUT_IMAGE_EXTENSIONS {
        assert!(supported_input_image(Path::new(&format!(
            "image.{}",
            ext.to_uppercase()
        ))));
    }
    assert!(!supported_input_image(Path::new("image.gif")));
    let root = transport_root("invalid-image");
    let path = root.join("bad.jpg");
    std::fs::write(&path, b"not a JPEG").unwrap();
    assert!(encode_resident_image(&path, MacRenderFormat::PngArgb8).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn gui_adapter_negotiates_geometry_and_exports_deep_preview() {
    let root = transport_root("adapter");
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resident_transport.py");
    let workers = vec![GuestWorkerCandidate {
        path: fixture,
        native: false,
    }];
    let plugin = root.join("protocol-double.aex");
    std::fs::write(&plugin, b"transport double, not executable AEX").unwrap();
    let input = root.join("input.png");
    image::RgbaImage::new(2, 1).save(&input).unwrap();
    for negotiate in [false, true] {
        for format in [
            MacRenderFormat::PngArgb8,
            MacRenderFormat::RawArgb16,
            MacRenderFormat::ExrArgb32f,
        ] {
            // The same admission adapter used by the GUI passes the launch flag,
            // probes, validates the frame, exports, and closes the worker.
            let admission = begin_resident_admission(
                workers.clone(),
                plugin.clone(),
                input.clone(),
                root.clone(),
                format,
                Vec::new(),
                negotiate,
            );
            let mut session = admission
                .receiver
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            assert_eq!(session.negotiate_output, negotiate);
            for frame_index in 0..2 {
                let output = root.join(format!(
                    "{negotiate}-{}-{frame_index}.png",
                    format.pixel_format()
                ));
                session
                    .sender
                    .send(ResidentCommand::Render {
                        frame_index,
                        parameters: Vec::new(),
                        output: output.clone(),
                    })
                    .unwrap();
                let result = session
                    .receiver
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap()
                    .unwrap();
                let preview = result.preview.as_ref().unwrap();
                let decoded = image::open(preview).unwrap();
                let dims = if negotiate { (3, 2) } else { (2, 1) };
                assert_eq!((decoded.width(), decoded.height()), dims);
                let expected_pixel: Vec<u8> = match format {
                    MacRenderFormat::PngArgb8 => vec![255, 11, 22, 33],
                    MacRenderFormat::RawArgb16 => [32768u16, 1, 16384, 65535]
                        .into_iter()
                        .flat_map(u16::to_le_bytes)
                        .collect(),
                    MacRenderFormat::ExrArgb32f => {
                        [0x3f800000u32, 0x80000000, 0x3f000000, 0x40000000]
                            .into_iter()
                            .flat_map(u32::to_le_bytes)
                            .collect()
                    }
                };
                if format == MacRenderFormat::RawArgb16 {
                    assert_eq!(decoded.color(), image::ColorType::Rgba16);
                    assert_eq!(
                        decoded.to_rgba16().get_pixel(0, 0).0,
                        [2, 32768, 65535, 65535]
                    );
                } else {
                    let pixel = if format == MacRenderFormat::PngArgb8 {
                        [11, 22, 33, 255]
                    } else {
                        [0, 128, 255, 255]
                    };
                    assert_eq!(decoded.to_rgba8().get_pixel(0, 0).0, pixel);
                }
                if format != MacRenderFormat::PngArgb8 {
                    let raw = output.join(if format == MacRenderFormat::RawArgb16 {
                        "output.bin"
                    } else {
                        "output.argb"
                    });
                    let bytes = std::fs::read(raw).unwrap();
                    assert_eq!(bytes, expected_pixel.repeat((dims.0 * dims.1) as usize));
                    let metadata: Value =
                        serde_json::from_slice(&std::fs::read(output.join("output.json")).unwrap())
                            .unwrap();
                    assert_eq!(
                        metadata["origin"],
                        if negotiate {
                            json!({"x":-1,"y":2})
                        } else {
                            json!({"x":0,"y":0})
                        }
                    );
                    // Existing artifact bytes remain identical to the default exporter.
                    if !negotiate {
                        let slot = root.join("verified.argb");
                        std::fs::write(&slot, &bytes).unwrap();
                        let legacy =
                            root.join(format!("legacy-{}-{frame_index}", format.pixel_format()));
                        let meta = &metadata["comparison_identity"];
                        let hash = format!("{:x}", Sha256::digest(&bytes));
                        let save = if format == MacRenderFormat::RawArgb16 {
                            save_argb16_raw_slot
                        } else {
                            save_argb32f_exr_slot
                        };
                        save(
                            &slot,
                            &legacy,
                            dims.0,
                            dims.1,
                            meta["plugin_sha256"].as_str().unwrap(),
                            meta["input_sha256"].as_str().unwrap(),
                            "",
                            "classic",
                            &hash,
                        )
                        .unwrap();
                        let file = if format == MacRenderFormat::RawArgb16 {
                            "output.bin"
                        } else {
                            "output.exr"
                        };
                        assert_eq!(
                            std::fs::read(legacy.join(file)).unwrap(),
                            std::fs::read(output.join(file)).unwrap()
                        );
                    }
                }
            }
            session.shutdown().unwrap();
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn negotiated_frame_rejects_malformed_geometry_and_transport() {
    let valid = json!({"v":1,"type":"frame_done","frame_index":0,"status":"ok",
        "render_error":0,"generation":1,"output":{"rect":[-1,2,2,4],"width":3,"height":2,
        "rowbytes":24,"pixel_format":"argb16","render_path":"smartfx","checksum":"a".repeat(64),"guards_intact":true}});
    assert!(validate_resident_frame_geometry(&valid, 0, None, MacRenderFormat::RawArgb16).is_ok());
    assert!(validate_resident_frame(&valid, 0, 2, 1, MacRenderFormat::RawArgb16).is_err());
    for (key, value) in [
        ("rect", json!(null)),
        ("rect", json!([i32::MIN, 2, i32::MAX, 4])),
        ("rect", json!([-1, 2, 3, 4])),
        ("width", json!(0)),
        ("width", json!(MAX_WIDTH + 1)),
        ("height", json!(MAX_HEIGHT + 1)),
        ("rowbytes", json!(23)),
        ("pixel_format", json!("argb8")),
        ("guards_intact", json!(false)),
        ("checksum", json!("bad")),
    ] {
        let mut bad = valid.clone();
        bad["output"][key] = value;
        assert!(
            validate_resident_frame_geometry(&bad, 0, None, MacRenderFormat::RawArgb16).is_err(),
            "{bad}"
        );
    }
    let mut stale = valid;
    stale["generation"] = json!(0);
    assert!(validate_resident_frame_geometry(&stale, 0, None, MacRenderFormat::RawArgb16).is_err());
}

#[test]
fn deep_preview_preserves_raw_and_rejects_corrupt_slots() {
    let root = transport_root("deep-bounds");
    let slot = root.join("slot");
    let png = root.join("preview.png");
    // HDR, negative zero and NaN payload survive in raw; display follows Windows.
    let bytes: Vec<_> = [0x3f800000u32, 0x80000000, 0x7fc00042, 0x40000000]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    std::fs::write(&slot, &bytes).unwrap();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    assert!(
        save_deep_preview(
            &slot,
            &png,
            1,
            1,
            MacRenderFormat::ExrArgb32f,
            &"0".repeat(64)
        )
        .is_err()
    );
    assert!(!png.exists());
    assert!(save_deep_preview(&slot, &png, 2, 1, MacRenderFormat::ExrArgb32f, &hash).is_err());
    assert!(!png.exists());
    save_deep_preview(&slot, &png, 1, 1, MacRenderFormat::ExrArgb32f, &hash).unwrap();
    assert_eq!(
        image::open(&png).unwrap().into_rgba8().into_raw(),
        [0, 0, 255, 255]
    );
    assert_eq!(std::fs::read(&slot).unwrap(), bytes);
    let saved = std::fs::read(&png).unwrap();
    assert!(save_deep_preview(&slot, &png, 1, 1, MacRenderFormat::ExrArgb32f, &hash).is_err());
    assert_eq!(std::fs::read(&png).unwrap(), saved);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn deep_chain_exports_png_without_quantizing_intermediate() {
    let root = transport_root("chain");
    let workers = vec![GuestWorkerCandidate {
        path: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resident_transport.py"),
        native: false,
    }];
    let plugin = root.join("double.aex");
    std::fs::write(&plugin, b"protocol double").unwrap();
    let stage = crate::gui_state::EffectChainSlot {
        enabled: true,
        editor: MacEffectEditor {
            plugin: Some(plugin),
            parameters: Vec::new(),
            defaults: Vec::new(),
        },
    };
    let input = root.join("input.png");
    image::RgbaImage::new(2, 1).save(&input).unwrap();
    for format in [MacRenderFormat::RawArgb16, MacRenderFormat::ExrArgb32f] {
        let result = render_mac_chain(
            &workers,
            &[stage.clone(), stage.clone()],
            &input,
            format,
            &root,
            &AtomicBool::new(false),
        )
        .unwrap();
        let preview = result.preview.unwrap();
        let decoded = image::open(&preview).unwrap();
        assert_eq!(
            decoded.color(),
            if format == MacRenderFormat::RawArgb16 {
                image::ColorType::Rgba16
            } else {
                image::ColorType::Rgba8
            }
        );
        let raw = std::fs::read(preview.with_extension("argb")).unwrap();
        let report: Value = serde_json::from_str(&result.report).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&raw)),
            report["world_sha256"]
        );
        assert_eq!(report["stages"].as_array().unwrap().len(), 2);
    }
    std::fs::remove_dir_all(root).unwrap();
}
