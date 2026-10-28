// Included in macos.rs: GUI chains use the same typed ARGB worlds as the guest.
#[derive(Clone)]
struct MacEffectEditor {
    plugin: Option<PathBuf>,
    parameters: Vec<InteractiveParameter>,
    defaults: Vec<InteractiveParameter>,
}

struct MacChainWorld {
    width: u32,
    height: u32,
    format: MacRenderFormat,
    argb: Vec<u8>,
}

impl MacChainWorld {
    fn write_slot(&self, slot: &Path, format: MacRenderFormat) -> Result<(u32, u32), String> {
        if self.format != format
            || self.width == 0
            || self.height == 0
            || self.width > MAX_WIDTH
            || self.height > MAX_HEIGHT
            || self.argb.len()
                != self.width as usize * self.height as usize * format.bytes_per_pixel()
        {
            return Err("chain world dimensions, depth or byte length is invalid".into());
        }
        std::fs::write(slot, &self.argb).map_err(|error| format!("write chain world: {error}"))?;
        Ok((self.width, self.height))
    }
}

struct MacChainTask {
    receiver: Receiver<Result<RenderResult, String>>,
    cancel: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl Drop for MacChainTask {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        // Each stage has bounded startup/render/close; never leave a chain's
        // workers running after the application exits.
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl MacHarnessApp {
    fn capture_effect(&self) -> MacEffectEditor {
        MacEffectEditor {
            plugin: self.plugin_path.clone(),
            parameters: self.parameters.clone(),
            defaults: self.parameter_defaults.clone(),
        }
    }

    fn save_active_effect(&mut self) {
        let editor = self.capture_effect();
        if self.effect_chain.is_empty() {
            if editor.plugin.is_some() {
                self.effect_chain.push(crate::gui_state::EffectChainSlot {
                    enabled: true,
                    editor,
                });
                self.active_effect = 0;
            }
        } else {
            self.effect_chain[self.active_effect].editor = editor;
        }
    }

    fn restore_active_effect(&mut self) {
        let editor = self.effect_chain[self.active_effect].editor.clone();
        self.plugin_path = editor.plugin;
        self.parameters = editor.parameters;
        self.parameter_defaults = editor.defaults;
    }

    fn uses_chain_renderer(&self) -> bool {
        self.effect_chain.len() > 1 || self.effect_chain.first().is_some_and(|slot| !slot.enabled)
    }

    fn invalidate_chain(&mut self) {
        self.output = None;
        self.output_texture = None;
        self.parameter_changed();
    }

    fn select_chain_effect(&mut self, index: usize) -> Result<(), String> {
        if self.occupied() || index >= self.effect_chain.len() {
            return Err("chain selection is unavailable".into());
        }
        self.close_resident()?;
        self.save_active_effect();
        self.active_effect = index;
        self.restore_active_effect();
        Ok(())
    }

    fn add_chain_effect_path(&mut self, path: PathBuf) -> Result<(), String> {
        if self.occupied() || self.effect_chain.len() >= crate::gui_state::MAX_EFFECT_CHAIN_STAGES {
            return Err("chain is busy or has eight stages".into());
        }
        let (parameters, defaults, report, _) = discover_parameters(&self.repository, &path)?;
        self.close_resident()?;
        self.save_active_effect();
        self.effect_chain.push(crate::gui_state::EffectChainSlot {
            enabled: true,
            editor: MacEffectEditor {
                plugin: Some(path),
                parameters,
                defaults,
            },
        });
        self.active_effect = self.effect_chain.len() - 1;
        self.restore_active_effect();
        self.report = report;
        self.invalidate_chain();
        Ok(())
    }

    fn show_chain_controls(&mut self, ui: &mut egui::Ui) {
        self.save_active_effect();
        let mut selected = None;
        let mut bypass_changed = false;
        let mut movement = None;
        let mut remove = false;
        let mut add = false;
        ui.add_enabled_ui(!self.occupied(), |ui| {
            ui.heading("Effect chain");
            for (index, slot) in self.effect_chain.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    bypass_changed |= ui
                        .checkbox(&mut slot.enabled, "")
                        .on_hover_text("Enabled")
                        .changed();
                    let name = slot
                        .editor
                        .plugin
                        .as_deref()
                        .and_then(Path::file_stem)
                        .and_then(|name| name.to_str())
                        .unwrap_or("No AEX");
                    if ui
                        .selectable_label(
                            index == self.active_effect,
                            format!("{}. {name}", index + 1),
                        )
                        .clicked()
                    {
                        selected = Some(index);
                    }
                });
            }
            ui.horizontal(|ui| {
                add = ui
                    .add_enabled(
                        self.effect_chain.len() < crate::gui_state::MAX_EFFECT_CHAIN_STAGES,
                        egui::Button::new("Add AEX..."),
                    )
                    .clicked();
                if ui
                    .add_enabled(self.active_effect > 0, egui::Button::new("Up"))
                    .clicked()
                {
                    movement = Some(self.active_effect - 1);
                }
                if ui
                    .add_enabled(
                        self.active_effect + 1 < self.effect_chain.len(),
                        egui::Button::new("Down"),
                    )
                    .clicked()
                {
                    movement = Some(self.active_effect + 1);
                }
                remove = ui
                    .add_enabled(self.effect_chain.len() > 1, egui::Button::new("Remove"))
                    .clicked();
            });
        });
        let result = (|| -> Result<(), String> {
            if let Some(index) = selected {
                self.select_chain_effect(index)?;
            }
            if bypass_changed || movement.is_some() || remove {
                self.close_resident()?;
                if let Some(index) = movement {
                    self.effect_chain.swap(self.active_effect, index);
                    self.active_effect = index;
                }
                if remove {
                    self.effect_chain.remove(self.active_effect);
                    self.active_effect = self.active_effect.min(self.effect_chain.len() - 1);
                    self.restore_active_effect();
                }
                self.invalidate_chain();
            }
            if add
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("After Effects plug-in", &["aex"])
                    .pick_file()
            {
                self.add_chain_effect_path(path)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.status = "Could not edit effect chain.".into();
            self.report = error;
        }
        ui.separator();
    }

    fn render_chain(&mut self) {
        if self.occupied() {
            return;
        }
        let Some(input) = self.input.clone() else {
            return;
        };
        let result = self
            .close_resident()
            .and_then(|()| guest_worker_candidates(&self.repository));
        let workers = match result {
            Ok(workers) => workers,
            Err(error) => {
                self.status = "Could not start effect chain.".into();
                self.report = error;
                return;
            }
        };
        let stages = self.effect_chain.clone();
        let format = self.render_format;
        let output_root = self.repository.join("target/harness-output");
        let cancel = Arc::new(AtomicBool::new(false));
        let task_cancel = Arc::clone(&cancel);
        let (sender, receiver) = mpsc::channel();
        let join = thread::spawn(move || {
            let result = render_mac_chain(
                &workers,
                &stages,
                &input,
                format,
                &output_root,
                &task_cancel,
            );
            let _ = sender.send(result);
        });
        self.chain_task = Some(MacChainTask {
            receiver,
            cancel,
            join: Some(join),
        });
        self.busy = true;
        self.status = "Rendering effect chain in the x64 guest...".into();
    }

    fn poll_chain(&mut self, ctx: &egui::Context) {
        let result = self
            .chain_task
            .as_ref()
            .and_then(|task| match task.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("chain controller stopped without a result".into()))
                }
            });
        let Some(result) = result else {
            return;
        };
        self.chain_task = None;
        self.busy = false;
        match result {
            Ok(result) => {
                let preview = result
                    .preview
                    .as_deref()
                    .map(|path| load_texture(ctx, "mac-chain-output", path))
                    .transpose();
                self.output = Some(result.output);
                self.report = result.report;
                match preview {
                    Ok(texture) => {
                        let had_output = self.output_texture.is_some();
                        self.output_texture = texture.map(|(texture, _, _)| texture);
                        self.viewer_mode = self.viewer_mode.after_successful_render(had_output);
                        self.status = "Completed: effect chain artifact".into();
                    }
                    Err(error) => {
                        self.output_texture = None;
                        self.status = "Chain preview failed.".into();
                        self.report.push_str(&format!("\n{error}"));
                    }
                }
            }
            Err(error) => {
                self.output = None;
                self.output_texture = None;
                self.status = "Effect chain failed.".into();
                self.report = error;
            }
        }
    }
}

fn run_mac_chain_stages(
    stages: &[crate::gui_state::EffectChainSlot<MacEffectEditor>],
    mut world: MacChainWorld,
    mut render: impl FnMut(
        usize,
        &MacEffectEditor,
        &MacChainWorld,
    ) -> Result<(MacChainWorld, Value), String>,
) -> Result<(MacChainWorld, Vec<Value>), String> {
    let mut reports = Vec::new();
    for (index, stage) in crate::gui_state::enabled_chain(stages) {
        let (next, report) = render(index, stage, &world)
            .map_err(|error| format!("chain stage {}: {error}", index + 1))?;
        if next.width != world.width
            || next.height != world.height
            || next.format != world.format
            || next.argb.len() != world.argb.len()
        {
            return Err(format!("chain stage {} changed world layout", index + 1));
        }
        world = next;
        reports.push(report);
    }
    Ok((world, reports))
}

fn render_mac_chain_stage(
    workers: &[GuestWorkerCandidate],
    stage: &MacEffectEditor,
    world: &MacChainWorld,
    cancel: &AtomicBool,
) -> Result<(MacChainWorld, Value), String> {
    if cancel.load(Ordering::Acquire) {
        return Err("chain cancelled".into());
    }
    let aex = stage.plugin.as_deref().ok_or("enabled stage has no AEX")?;
    let payload = parameter_payload(&stage.parameters)?;
    let layers = selected_layer_paths(&stage.parameters);
    if layers.len() > 8 {
        return Err("chain stage exceeds eight secondary layers".into());
    }
    let launch = MacFixtureLaunch {
        layers: &layers,
        worlds: None,
        smart: false,
        time_scale: 30,
        raw_primary: Some(world),
    };
    let mut started = start_resident_worker(
        workers,
        aex,
        Path::new(""),
        world.format,
        Some(&launch),
        false,
    )?;
    let result = (|| -> Result<_, String> {
        write_control_message(
            &mut started.stdin,
            &json!({"v":2,"type":"render_frame","frame_index":0,"current_time":{"value":0,"scale":30},"parameters":payload}),
        )?;
        let deadline = Instant::now() + RESIDENT_RENDER_DEADLINE;
        let response = loop {
            if cancel.load(Ordering::Acquire) {
                return Err("chain cancelled".into());
            }
            audit_process(started.worker_pid)?;
            if Instant::now() >= deadline {
                return Err("chain stage render timed out".into());
            }
            match started
                .response_receiver
                .recv_timeout(RESIDENT_RESPONSE_POLL)
            {
                Ok(Ok(Some(response))) => break response,
                Ok(Ok(None)) => return Err("chain worker closed stdout".into()),
                Ok(Err(error)) => return Err(error),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("chain response reader disconnected".into());
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        };
        let checksum =
            validate_resident_frame(&response, 0, world.width, world.height, world.format)?;
        started
            .session
            .audit_tree_with_additional_files(started.additional_session_files)?;
        let mut argb = Vec::new();
        std::fs::File::open(&started.output_slot)
            .map_err(|e| e.to_string())?
            .take(world.argb.len() as u64 + 1)
            .read_to_end(&mut argb)
            .map_err(|e| e.to_string())?;
        if argb.len() != world.argb.len() || format!("{:x}", Sha256::digest(&argb)) != checksum {
            return Err("chain output size/checksum mismatch".into());
        }
        let artifact_identity = fixture_conditions(
            &started.plugin_sha256,
            &started.input_sha256,
            &argb,
            world.format.pixel_format(),
            response["output"]["render_path"].as_str().unwrap(),
            &json!([payload]),
            "premultiplied",
            0,
            1,
            1,
            30,
            None,
        )
        .comparison_identity;
        Ok((
            MacChainWorld {
                width: world.width,
                height: world.height,
                format: world.format,
                argb,
            },
            json!({
                "plugin_sha256": started.plugin_sha256, "input_sha256": started.input_sha256,
                "parameters": payload, "frame": response,
            "artifact_identity": artifact_identity,
                "security_tier": started.security_tier.as_str(), "fallback_reasons": started.fallback_reasons,
            }),
        ))
    })();
    match result {
        Ok(value) => {
            close_probe_worker(started)?;
            Ok(value)
        }
        Err(error) => {
            let stderr = terminate_resident_worker(started);
            Err(format!(
                "{error}; worker stderr: {}",
                stderr.unwrap_or_default()
            ))
        }
    }
}

fn render_mac_chain(
    workers: &[GuestWorkerCandidate],
    stages: &[crate::gui_state::EffectChainSlot<MacEffectEditor>],
    input: &Path,
    format: MacRenderFormat,
    output_root: &Path,
    cancel: &AtomicBool,
) -> Result<RenderResult, String> {
    let (width, height, argb) = encode_resident_image(input, format)?;
    let input_sha256 = format!("{:x}", Sha256::digest(&argb));
    let world = MacChainWorld {
        width,
        height,
        format,
        argb,
    };
    let (world, reports) = run_mac_chain_stages(stages, world, |_, stage, world| {
        render_mac_chain_stage(workers, stage, world, cancel)
    })?;
    if cancel.load(Ordering::Acquire) {
        return Err("chain cancelled".into());
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let output = output_root.join(format!("mac-chain-{}-{nonce}", std::process::id()));
    let staging = fixture_staging_path(&output)?;
    let result = (|| -> Result<RenderResult, String> {
        let checksum = format!("{:x}", Sha256::digest(&world.argb));
        let report = json!({"schema":"aexcompat.macos-effect-chain", "input_sha256":input_sha256,
            "pixel_format": format.pixel_format(), "width":width, "height":height,
            "world_sha256":checksum, "stages":reports,
            "enabled":stages.iter().map(|slot| slot.enabled).collect::<Vec<_>>()});
        // Always retain the exact ARGB intermediate, including NaN payloads,
        // AE16 values above 32768 and signed zeros, independent of preview.
        let raw = staging.join("output.argb");
        world.write_slot(&raw, format)?;
        let preview = if format == MacRenderFormat::PngArgb8 {
            save_argb8_slot(&raw, &staging.join("output.png"), width, height, &checksum)?;
            Some(output.join("output.png"))
        } else {
            save_deep_preview(
                &raw,
                &staging.join("output.png"),
                width,
                height,
                format,
                &checksum,
            )?;
            Some(output.join("output.png"))
        };
        let final_output = match format {
            MacRenderFormat::PngArgb8 => output.join("output.png"),
            MacRenderFormat::RawArgb16 | MacRenderFormat::ExrArgb32f => {
                let conditions = RenderArtifactConditions {
                    premultiplication: "premultiplied".into(),
                    working_space: "None".into(),
                    render_mode: "software".into(),
                    comparison_identity: json!({
                        "chain": reports.iter().map(|stage| stage["artifact_identity"].clone()).collect::<Vec<_>>(),
                        "input_sha256": input_sha256, "world_sha256": checksum,
                        "pixel_format": format.pixel_format(), "origin": {"x":0,"y":0},
                    }),
                };
                let rgba = argb_to_rgba_words(&world.argb, format.bytes_per_pixel() / 4);
                let artifact = staging.join("artifact");
                if format == MacRenderFormat::ExrArgb32f {
                    write_float32_exr_artifact(&artifact, &rgba, width, height, 0, 0, conditions)
                        .map_err(|error| format!("chain EXR artifact: {error}"))?;
                    output.join("artifact/output.exr")
                } else {
                    write_raw_world_artifact(
                        &artifact,
                        &rgba,
                        width,
                        height,
                        format.artifact_format(),
                        0,
                        0,
                        conditions,
                    )
                    .map_err(|error| format!("chain raw artifact: {error}"))?;
                    output.join("artifact/output.bin")
                }
            }
        };
        std::fs::write(
            staging.join("chain.json"),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if output.exists() {
            return Err("chain output already exists".into());
        }
        std::fs::rename(&staging, &output).map_err(|e| e.to_string())?;
        Ok(RenderResult {
            report: serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?,
            output: final_output,
            preview,
        })
    })();
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| format!("chain staging cleanup: {e}"))?;
    }
    result
}

#[cfg(test)]
mod chain_tests {
    use super::*;
    fn stage(name: &str, enabled: bool) -> crate::gui_state::EffectChainSlot<MacEffectEditor> {
        crate::gui_state::EffectChainSlot {
            enabled,
            editor: MacEffectEditor {
                plugin: Some(PathBuf::from(name)),
                parameters: Vec::new(),
                defaults: Vec::new(),
            },
        }
    }
    fn world(format: MacRenderFormat) -> MacChainWorld {
        let argb = match format {
            MacRenderFormat::PngArgb8 => vec![255, 1, 2, 3],
            MacRenderFormat::RawArgb16 => [32768u16, 10001, 7, 65535]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect(),
            MacRenderFormat::ExrArgb32f => [
                1.0f32.to_bits(),
                (-0.0f32).to_bits(),
                0x7fc01234,
                123.25f32.to_bits(),
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect(),
        };
        MacChainWorld {
            width: 1,
            height: 1,
            format,
            argb,
        }
    }
    #[test]
    fn chain_order_bypass_and_failure_short_circuit() {
        let mut stages = vec![
            stage("add", true),
            stage("bypass", false),
            stage("double", true),
        ];
        let run = |stages: &[_]| {
            run_mac_chain_stages(
                stages,
                world(MacRenderFormat::PngArgb8),
                |index, stage, input| {
                    let mut output = world(input.format);
                    output.argb = input.argb.clone();
                    match stage.plugin.as_ref().unwrap().to_str().unwrap() {
                        "add" => output.argb[1] += 3,
                        "double" => output.argb[1] *= 2,
                        _ => panic!("bypassed stage dispatched"),
                    }
                    Ok((output, json!(index)))
                },
            )
            .unwrap()
        };
        let (result, reports) = run(&stages);
        assert_eq!(result.argb[1], 8);
        assert_eq!(reports, vec![json!(0), json!(2)]);
        stages.swap(0, 2);
        assert_eq!(run(&stages).0.argb[1], 5);
        let mut called = Vec::new();
        let error =
            run_mac_chain_stages(&stages, world(MacRenderFormat::PngArgb8), |index, _, _| {
                called.push(index);
                Err("injected render failure".into())
            })
            .err()
            .unwrap();
        assert_eq!(called, vec![0]);
        assert!(error.contains("chain stage 1: injected render failure"));
        for stage in &mut stages {
            stage.enabled = false;
        }
        assert_eq!(run(&stages).0.argb, world(MacRenderFormat::PngArgb8).argb);
    }
    #[test]
    fn chain_raw_transport_keeps_deep_bits_and_rejects_bad_layout() {
        let root =
            std::env::temp_dir().join(format!("aexcompat-chain-bits-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for format in [
            MacRenderFormat::PngArgb8,
            MacRenderFormat::RawArgb16,
            MacRenderFormat::ExrArgb32f,
        ] {
            let input = world(format);
            let expected = input.argb.clone();
            let (mut result, _) = run_mac_chain_stages(
                &[stage("copy", true), stage("copy", true)],
                input,
                |_, _, input| {
                    let slot = root.join("slot");
                    input.write_slot(&slot, input.format)?;
                    let mut output = world(input.format);
                    output.argb = std::fs::read(slot).unwrap();
                    Ok((output, json!(null)))
                },
            )
            .unwrap();
            assert_eq!(result.argb, expected);
            result.argb.pop();
            assert!(result.write_slot(&root.join("invalid"), format).is_err());
            assert!(!root.join("invalid").exists());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn chain_selection_parks_independent_editor_and_layer_values() {
        let mut app = MacHarnessApp::new(PathBuf::new());
        app.effect_chain = vec![stage("first", true), stage("second", true)];
        app.restore_active_effect();
        app.parameters = vec![InteractiveParameter {
            slot: 4,
            name: "Layer".into(),
            kind: "layer".into(),
            minimum: 0.0,
            maximum: 1.0,
            value: 1.0,
            choices: Vec::new(),
            use_declared_default: false,
            color_float: None,
            color: [0; 4],
            components: [0.0; 3],
            component_count: 0,
            layer_path: Some(PathBuf::from("first-layer.png")),
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0; 2],
        }];
        app.parameter_defaults = app.parameters.clone();
        app.select_chain_effect(1).unwrap();
        assert!(app.parameters.is_empty());
        app.select_chain_effect(0).unwrap();
        assert_eq!(
            selected_layer_paths(&app.parameters),
            vec![(4, PathBuf::from("first-layer.png"))]
        );
        assert_eq!(app.parameter_defaults[0].value, 1.0);
        app.busy = true;
        assert!(app.select_chain_effect(1).is_err());
        assert_eq!(app.active_effect, 0);
    }
    #[test]
    fn supervised_chain_warmup_does_not_publish_single_stage_when_auto_update_is_off() {
        let mut app = MacHarnessApp::new(PathBuf::new());
        app.effect_chain = vec![stage("first", true), stage("second", true)];
        app.restore_active_effect();
        app.live_render.set_enabled(false);
        app.output = Some(PathBuf::from("chain-output.png"));
        app.report = "chain report".into();
        app.pending_user_changed = Some(1);
        let (sender, commands) = mpsc::channel();
        let (results, receiver) = mpsc::channel();
        app.resident = ResidentState::Ready(ResidentSessionHandle {
            plugin_path: PathBuf::from("first"),
            input: PathBuf::new(),
            format: MacRenderFormat::PngArgb8,
            negotiate_output: false,
            layers: Vec::new(),
            sender,
            receiver,
            next_frame: 1,
            child: Arc::new(Mutex::new(None)),
            shutdown_requested: Arc::new(AtomicBool::new(false)),
            join: None,
        });
        results
            .send(Ok(RenderResult {
                report: "stage-only report".into(),
                output: PathBuf::from("stage-only.bin"),
                preview: None,
            }))
            .unwrap();
        app.poll(&egui::Context::default());
        assert_eq!(app.output, Some(PathBuf::from("chain-output.png")));
        assert_eq!(app.report, "chain report");
        assert!(!app.live_render.enabled());
        assert!(matches!(
            commands.try_recv().unwrap(),
            ResidentCommand::UserChanged { slot: 1, .. }
        ));
    }
    #[test]
    #[ignore = "requires AEXCOMPAT_TEST_AEX and AEXCOMPAT_GUEST_WORKER"]
    fn real_chain_links_worker_world_hashes_at_all_depths() {
        let aex = PathBuf::from(std::env::var_os("AEXCOMPAT_TEST_AEX").unwrap());
        let root =
            std::env::temp_dir().join(format!("aexcompat-real-chain-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("input.png");
        image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(
            3,
            2,
            image::Rgba([20001, 7, 40001, 65535]),
        )
        .save(&input)
        .unwrap();
        let workers = guest_worker_candidates(&repository_root().unwrap()).unwrap();
        let mut first = stage("unused", true);
        first.editor.plugin = Some(aex);
        let stages = vec![first.clone(), stage("must-not-load.aex", false), first];
        for format in [
            MacRenderFormat::PngArgb8,
            MacRenderFormat::RawArgb16,
            MacRenderFormat::ExrArgb32f,
        ] {
            let result = render_mac_chain(
                &workers,
                &stages,
                &input,
                format,
                &root,
                &AtomicBool::new(false),
            )
            .unwrap();
            assert!(result.output.is_file());
            match format {
                MacRenderFormat::PngArgb8 => {
                    assert_eq!(result.output.extension().unwrap(), "png");
                    assert_eq!(image::open(&result.output).unwrap().width(), 3);
                }
                MacRenderFormat::RawArgb16 => {
                    assert_eq!(result.output.extension().unwrap(), "bin");
                    assert_eq!(std::fs::metadata(&result.output).unwrap().len(), 3 * 2 * 8);
                }
                MacRenderFormat::ExrArgb32f => {
                    assert_eq!(result.output.extension().unwrap(), "exr");
                    assert_eq!(
                        &std::fs::read(&result.output).unwrap()[..4],
                        &[0x76, 0x2f, 0x31, 0x01]
                    );
                }
            }
            let report: Value = serde_json::from_str(&result.report).unwrap();
            assert_eq!(report["stages"].as_array().unwrap().len(), 2);
            assert_eq!(report["input_sha256"], report["stages"][0]["input_sha256"]);
            assert_eq!(
                report["stages"][0]["frame"]["output"]["checksum"],
                report["stages"][1]["input_sha256"]
            );
            assert_eq!(
                report["stages"][1]["frame"]["output"]["checksum"],
                report["world_sha256"]
            );
            assert_eq!(
                format!(
                    "{:x}",
                    Sha256::digest(
                        std::fs::read(if format == MacRenderFormat::PngArgb8 {
                            result.output.parent().unwrap().join("output.argb")
                        } else {
                            result
                                .output
                                .parent()
                                .unwrap()
                                .parent()
                                .unwrap()
                                .join("output.argb")
                        })
                        .unwrap()
                    )
                ),
                report["world_sha256"]
            );
        }
        // The identity route must export every depth without loading any AEX.
        for format in [
            MacRenderFormat::PngArgb8,
            MacRenderFormat::RawArgb16,
            MacRenderFormat::ExrArgb32f,
        ] {
            let result = render_mac_chain(
                &[],
                &[stage("missing.aex", false)],
                &input,
                format,
                &root,
                &AtomicBool::new(false),
            )
            .unwrap();
            let report: Value = serde_json::from_str(&result.report).unwrap();
            assert_eq!(report["input_sha256"], report["world_sha256"]);
            assert!(report["stages"].as_array().unwrap().is_empty());
            assert!(result.output.is_file());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
