struct HarnessApp {
    repository: PathBuf,
    selection: Option<Selection>,
    session_approved: bool,
    dependencies: Vec<SessionDependency>,
    approved_dependencies: Vec<aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact>,
    approval_check: bool,
    trust_rebuilds: bool,
    selection_stale: bool,
    last_identity_check: Instant,
    input_image: Option<PathBuf>,
    audio_input: Option<PathBuf>,
    audio_effect_only: bool,
    input_preview: Option<egui::TextureHandle>,
    output_image: Option<PathBuf>,
    preview: Option<egui::TextureHandle>,
    reference_image: Option<PathBuf>,
    reference_preview: Option<egui::TextureHandle>,
    viewer_open: bool,
    viewer_mode: u8,
    viewer_zoom: f32,
    viewer_pan: egui::Vec2,
    pixel_comparison: Option<Result<PixelComparison, String>>,
    parameters: Vec<aexcompat_broker::image_render::InteractiveParameter>,
    parameter_defaults: Vec<aexcompat_broker::image_render::InteractiveParameter>,
    host_context: Option<aexcompat_broker::render_request::HostContext>,
    smart_render: bool,
    smart_render_advertised: Option<bool>,
    smart_render_capability: Option<InspectedRenderCapability>,
    smart_render_manual_override: bool,
    pixel_format: aexcompat_broker::image_render::RenderPixelFormat,
    gpu_backend: aexcompat_broker::image_render::RenderGpuBackend,
    frame: i32,
    frames_per_second: u32,
    frame_time_step: i32,
    duration_frames: i32,
    custom_ui_click_point: [u16; 2],
    custom_ui_click_color: [f32; 4],
    apply_custom_ui_click_to_render: bool,
    apply_custom_ui_draw_to_render: bool,
    custom_ui_drag_end: [u16; 2],
    custom_ui_drag_steps: u8,
    custom_ui_keycode: u32,
    custom_ui_key_modifiers: u16,
    busy: bool,
    rendering: bool,
    status: String,
    report: String,
    render_diagnostics: Option<RenderDiagnostics>,
    failure_diagnostics: Option<FailureDiagnostics>,
    matrix_results: Vec<MatrixCase>,
    receiver: Option<Receiver<TaskResult>>,
    task_kind: TaskKind,
    inspect_after_refresh: bool,
    diagnostic_history: DiagnosticHistory,
    missing_suite_aggregate: MissingSuiteAggregate,
    preflight_warnings: Vec<PreflightImportWarning>,
    diagnostic_warning: Option<String>,
    live_render: bool,
    pending_parameter_slot: Option<u32>,
    pending_live_render: bool,
    live_render_due: Option<Instant>,
    render_after_parameter_change: bool,
    /// Command channel into the background session thread (issue #107); the
    /// resident worker lives on the other side of it. Dropped with the app,
    /// which closes the session gracefully. The session transport is
    /// Windows-only; other targets keep the one-shot path.
    #[cfg(windows)]
    live_session: Option<LiveSessionHandle>,
}

impl HarnessApp {
    fn new(repository: PathBuf) -> Self {
        let missing_suite_aggregate = aggregate_missing_suites(&repository);
        Self {
            repository,
            selection: None,
            session_approved: false,
            dependencies: Vec::new(),
            approved_dependencies: Vec::new(),
            approval_check: false,
            trust_rebuilds: false,
            selection_stale: false,
            last_identity_check: Instant::now(),
            input_image: None,
            audio_input: None,
            audio_effect_only: false,
            input_preview: None,
            output_image: None,
            preview: None,
            reference_image: None,
            reference_preview: None,
            viewer_open: false,
            viewer_mode: 0,
            viewer_zoom: 1.0,
            viewer_pan: egui::Vec2::ZERO,
            pixel_comparison: None,
            parameters: Vec::new(),
            parameter_defaults: Vec::new(),
            host_context: None,
            smart_render: false,
            smart_render_advertised: None,
            smart_render_capability: None,
            smart_render_manual_override: false,
            pixel_format: aexcompat_broker::image_render::RenderPixelFormat::Argb8,
            gpu_backend: aexcompat_broker::image_render::RenderGpuBackend::Auto,
            frame: 0,
            frames_per_second: 30,
            frame_time_step: 1,
            duration_frames: 300,
            custom_ui_click_point: [20, 20],
            custom_ui_click_color: [0.125, 0.25, 0.75, 1.0],
            apply_custom_ui_click_to_render: false,
            apply_custom_ui_draw_to_render: false,
            custom_ui_drag_end: [20, 20],
            custom_ui_drag_steps: 4,
            custom_ui_keycode: 0x8000_0041,
            custom_ui_key_modifiers: 0,
            busy: false,
            rendering: false,
            status: "Select an AEX file. Selection does not execute native code.".into(),
            report: String::new(),
            render_diagnostics: None,
            failure_diagnostics: None,
            matrix_results: Vec::new(),
            receiver: None,
            task_kind: TaskKind::Generic,
            inspect_after_refresh: false,
            diagnostic_history: DiagnosticHistory::default(),
            missing_suite_aggregate,
            preflight_warnings: Vec::new(),
            diagnostic_warning: None,
            live_render: true,
            pending_parameter_slot: None,
            pending_live_render: false,
            live_render_due: None,
            render_after_parameter_change: false,
            #[cfg(windows)]
            live_session: None,
        }
    }

    /// Tells the session thread to close the resident worker. Required
    /// whenever the AEX selection or its approval changes: the worker must
    /// not outlive the selection it was opened for.
    fn close_live_session(&mut self) {
        #[cfg(windows)]
        if let Some(handle) = &self.live_session {
            if handle.sender.send(LiveCommand::Close).is_err() {
                self.live_session = None;
            }
        }
    }

    fn accept_adjacent_discovery(&mut self, discovery: AdjacentImportDiscovery) {
        self.dependencies = discovery.dependencies;
        self.preflight_warnings = discovery.warnings;
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let identity = DispatchIdentity {
            sha256: selection.sha256.clone(),
            size: selection.size,
        };
        if selection.profile.is_none() {
            if let Err(error) =
                persist_preflight_warnings(&self.repository, &identity, &self.preflight_warnings)
            {
                self.diagnostic_warning = Some(format!("Diagnostic save warning: {error}"));
            }
        }
        self.diagnostic_history = load_diagnostic_history(&self.repository, &identity.sha256);
        self.missing_suite_aggregate = aggregate_missing_suites(&self.repository);
    }

    fn show_image_viewer(&mut self, ctx: &egui::Context) {
        if !self.viewer_open {
            return;
        }
        let mut open = self.viewer_open;
        egui::Window::new("FHD Image Viewer")
            .open(&mut open)
            .default_size(egui::vec2(1600.0, 900.0))
            .min_size(egui::vec2(640.0, 360.0))
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.viewer_mode, 0, "Input");
                    ui.selectable_value(&mut self.viewer_mode, 1, "AEX output");
                    ui.selectable_value(&mut self.viewer_mode, 2, "Compare");
                    ui.separator();
                    ui.label("FHD canvas / aspect-fit");
                    ui.separator();
                    ui.monospace(format!("{:.0}%", self.viewer_zoom * 100.0));
                    if ui.small_button("Fit").clicked() {
                        self.viewer_zoom = 1.0;
                        self.viewer_pan = egui::Vec2::ZERO;
                    }
                });
                ui.separator();
                match self.viewer_mode {
                    0 => show_viewer_texture(
                        ui,
                        "Input",
                        self.input_preview.as_ref(),
                        &mut self.viewer_zoom,
                        &mut self.viewer_pan,
                    ),
                    1 => show_viewer_texture(
                        ui,
                        "AEX output",
                        self.preview.as_ref(),
                        &mut self.viewer_zoom,
                        &mut self.viewer_pan,
                    ),
                    _ => ui.columns(2, |columns| {
                        show_viewer_texture(
                            &mut columns[0],
                            "Input",
                            self.input_preview.as_ref(),
                            &mut self.viewer_zoom,
                            &mut self.viewer_pan,
                        );
                        show_viewer_texture(
                            &mut columns[1],
                            "AEX output",
                            self.preview.as_ref(),
                            &mut self.viewer_zoom,
                            &mut self.viewer_pan,
                        );
                    }),
                }
            });
        self.viewer_open = open;
    }

    fn reset_and_choose_aex(&mut self) {
        // The selection is gone the moment the reset starts; the resident
        // worker for it must not outlive a cancelled or failed re-pick.
        self.close_live_session();
        self.selection = None;
        self.session_approved = false;
        self.approved_dependencies.clear();
        self.trust_rebuilds = false;
        self.selection_stale = false;
        self.diagnostic_history = DiagnosticHistory::default();
        self.diagnostic_warning = None;
        self.preflight_warnings.clear();
        self.parameters.clear();
        self.parameter_defaults.clear();
        self.audio_input = None;
        self.audio_effect_only = false;
        self.smart_render = false;
        self.smart_render_advertised = None;
        self.smart_render_capability = None;
        self.smart_render_manual_override = false;
        self.host_context = None;
        self.choose_aex();
    }

    fn show_workspace_viewer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.viewer_mode, 0, "INPUT");
            ui.selectable_value(&mut self.viewer_mode, 1, "AEX OUTPUT");
            ui.selectable_value(&mut self.viewer_mode, 2, "COMPARE");
            ui.separator();
            let label = match self.viewer_mode {
                0 => self.input_preview.as_ref().map(|texture| texture.size()),
                1 => self.preview.as_ref().map(|texture| texture.size()),
                _ => None,
            };
            if let Some([width, height]) = label {
                ui.monospace(format!("{width} x {height}"));
            } else {
                ui.weak("FHD workspace / aspect fit");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Pop out").clicked() {
                    self.viewer_open = true;
                }
                if ui.small_button("Fit").clicked() {
                    self.viewer_zoom = 1.0;
                    self.viewer_pan = egui::Vec2::ZERO;
                }
                ui.monospace(format!("{:.0}%", self.viewer_zoom * 100.0));
                if self.rendering {
                    ui.weak("Rendering...");
                    ui.spinner();
                }
            });
        });
        ui.separator();

        let viewer_height = (ui.available_height() * 0.62).clamp(280.0, 860.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), viewer_height),
            egui::Layout::top_down(egui::Align::Center),
            |ui| match self.viewer_mode {
                0 => show_viewer_texture(
                    ui,
                    "Input",
                    self.input_preview.as_ref(),
                    &mut self.viewer_zoom,
                    &mut self.viewer_pan,
                ),
                1 if self.preview.is_some() => show_viewer_texture(
                    ui,
                    "AEX output",
                    self.preview.as_ref(),
                    &mut self.viewer_zoom,
                    &mut self.viewer_pan,
                ),
                1 => {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            if self.busy {
                                ui.spinner();
                                ui.label("Rendering AEX output...");
                            } else {
                                ui.colored_label(
                                    Color32::from_rgb(225, 155, 65),
                                    RichText::new("AEX output is not available").strong(),
                                );
                                ui.label(&self.status);
                                if let Some(first_line) = self.report.lines().next() {
                                    ui.monospace(first_line);
                                }
                            }
                        });
                    });
                }
                _ => ui.columns(2, |columns| {
                    show_viewer_texture(
                        &mut columns[0],
                        "Input",
                        self.input_preview.as_ref(),
                        &mut self.viewer_zoom,
                        &mut self.viewer_pan,
                    );
                    show_viewer_texture(
                        &mut columns[1],
                        "AEX output",
                        self.preview.as_ref(),
                        &mut self.viewer_zoom,
                        &mut self.viewer_pan,
                    );
                }),
            },
        );
    }

    fn spawn<F>(&mut self, work: F)
    where
        F: FnOnce() -> Result<(String, Option<PathBuf>), String> + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = work();
            let _ = sender.send(match result {
                Ok((body, output)) => TaskResult {
                    success: true,
                    body,
                    output,
                    identity: None,
                    operation: None,
                    diagnostic_eligible: false,
                },
                Err(body) => TaskResult {
                    success: false,
                    body,
                    output: None,
                    identity: None,
                    operation: None,
                    diagnostic_eligible: false,
                },
            });
        });
        self.receiver = Some(receiver);
        self.busy = true;
        self.task_kind = TaskKind::Generic;
    }

    fn spawn_native<F>(&mut self, operation: &'static str, work: F)
    where
        F: FnOnce() -> Result<(String, Option<PathBuf>), String> + Send + 'static,
    {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let identity = DispatchIdentity {
            sha256: selection.sha256.clone(),
            size: selection.size,
        };
        let diagnostic_eligible = selection.profile.is_none();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = work();
            let (success, body, output) = match result {
                Ok((body, output)) => (true, body, output),
                Err(body) => (false, body, None),
            };
            let _ = sender.send(TaskResult {
                success,
                body,
                output,
                identity: Some(identity),
                operation: Some(operation.into()),
                diagnostic_eligible,
            });
        });
        self.receiver = Some(receiver);
        self.busy = true;
        self.task_kind = TaskKind::Generic;
    }

    fn choose_aex(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("After Effects plug-in", &["aex"])
            .pick_file()
        else {
            return;
        };
        self.status = "Computing AEX identity...".into();
        self.spawn(move || {
            let bytes = read_bounded_pe(&path)?;
            let hash = format!("{:X}", Sha256::digest(&bytes));
            Ok((
                format!("{}\n{}\n{}", path.display(), bytes.len(), hash),
                None,
            ))
        });
        self.task_kind = TaskKind::IdentifyAex;
    }

    fn add_dependency(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Windows dependency", &["dll"])
            .pick_file()
        else {
            return;
        };
        match read_bounded_pe(&path) {
            Ok(bytes) => {
                let key = path.to_string_lossy().to_lowercase();
                if self
                    .dependencies
                    .iter()
                    .any(|item| item.path.to_string_lossy().to_lowercase() == key)
                {
                    self.status = "That dependency DLL is already listed.".into();
                    return;
                }
                self.dependencies.push(SessionDependency {
                    path,
                    size: bytes.len() as u64,
                    sha256: format!("{:X}", Sha256::digest(&bytes)),
                });
                match self.approve_session() {
                    Ok(()) => self.status = "Dependency manifest refreshed.".into(),
                    Err(error) => {
                        self.invalidate_session_approval("Dependency manifest validation failed.");
                        self.report = error;
                    }
                }
            }
            Err(error) => {
                self.status = "Dependency DLL could not be read.".into();
                self.report = error.to_string();
            }
        }
    }

    fn invalidate_session_approval(&mut self, status: &str) {
        self.session_approved = false;
        self.approval_check = false;
        self.approved_dependencies.clear();
        self.status = status.into();
        self.close_live_session();
    }

    fn approve_session(&mut self) -> Result<(), String> {
        // Every dependency-set change funnels through re-approval; the
        // resident worker still holds the previous approved set and must not
        // idle past it, so close eagerly rather than lazily at the next
        // render.
        self.close_live_session();
        let selection = self.selection.as_ref().ok_or("No AEX is selected")?;
        let main = aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact {
            path: selection.path.clone(),
            expected_sha256: decode_sha256(&selection.sha256)?,
            expected_size: selection.size,
        };
        let dependencies = self
            .dependencies
            .iter()
            .map(|item| {
                serde_json::json!({
                    "path": item.path,
                    "basename": item.path.file_name().and_then(|name| name.to_str()).unwrap_or(""),
                    "sha256": item.sha256,
                    "size": item.size,
                })
            })
            .collect::<Vec<_>>();
        let json = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "dependencies": dependencies,
        }))
        .map_err(|error| error.to_string())?;
        let manifest =
            aexcompat_broker::session_dependency_manifest::parse_and_validate(&json, &main)
                .map_err(|error| error.to_string())?;
        self.approved_dependencies = manifest.into_approved_image_artifacts();
        self.session_approved = true;
        Ok(())
    }

    fn choose_input(&mut self, ctx: &egui::Context) {
        let selected = rfd::FileDialog::new()
            .add_filter(
                "Image",
                &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"],
            )
            .pick_file();
        let Some(path) = selected else {
            return;
        };
        match load_preview(ctx, "input", &path) {
            Ok(preview) => {
                self.input_image = Some(path);
                self.input_preview = Some(preview);
                self.output_image = None;
                self.preview = None;
                self.pixel_comparison = None;
                self.status = "Input image loaded. Ready to render.".into();
                self.start_live_render_if_ready();
            }
            Err(error) => {
                self.status = "Input image could not be decoded.".into();
                self.report = error;
            }
        }
    }

    fn start_live_render_if_ready(&mut self) {
        if self.live_render
            && !self.busy
            && !self.audio_effect_only
            && self.selection.is_some()
            && self.input_image.is_some()
        {
            self.quick_render();
            self.viewer_mode = 1;
        }
    }

    fn choose_audio_input(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Mono float32 LE audio", &["f32"])
            .pick_file()
        else {
            return;
        };
        self.audio_input = Some(path);
        self.status = "Audio input selected. Ready to render.".into();
    }

    fn choose_reference(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "AE reference image",
                &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"],
            )
            .pick_file()
        else {
            return;
        };
        match load_preview(ctx, "reference", &path) {
            Ok(preview) => {
                self.reference_image = Some(path);
                self.reference_preview = Some(preview);
                self.refresh_pixel_comparison();
                self.status = "AE reference image loaded.".into();
            }
            Err(error) => {
                self.status = "AE reference image could not be decoded.".into();
                self.report = error;
            }
        }
    }

    fn load_debug_request(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AEXCompat debug request", &["json"])
            .pick_file()
        else {
            return;
        };
        let result = (|| {
            let bytes = fs::read(&path).map_err(|error| error.to_string())?;
            if bytes.len() > 64 * 1024 {
                return Err("assignment document exceeds 64 KiB".to_owned());
            }
            let document: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            let timing = typed_request_timing(&document)?;
            let mut parameters = self.parameters.clone();
            apply_typed_assignments(&mut parameters, &document, Some(path.as_path()))?;
            let host_context = typed_request_host_context(&document)?;
            Ok((parameters, timing, host_context))
        })();
        match result {
            Ok((parameters, timing, host_context)) => {
                self.parameters = parameters;
                self.host_context = host_context;
                self.frame = timing.current_time / timing.time_step;
                self.frames_per_second = timing.time_scale;
                self.frame_time_step = timing.time_step;
                self.duration_frames = timing.total_time / timing.time_step;
                self.status = format!("Loaded debug request: {}", path.display());
            }
            Err(error) => {
                self.status = "Debug request was rejected without changing controls.".into();
                self.report = error;
            }
        }
    }

    fn save_debug_request(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AEXCompat debug request", &["json"])
            .set_file_name("aex-debug-request.json")
            .save_file()
        else {
            return;
        };
        let document = typed_request_document(
            &self.parameters,
            self.frame,
            self.frames_per_second,
            self.frame_time_step,
            self.duration_frames,
            self.host_context.as_ref(),
        );
        match serde_json::to_vec_pretty(&document)
            .map_err(|error| error.to_string())
            .and_then(|bytes| fs::write(&path, bytes).map_err(|error| error.to_string()))
        {
            Ok(()) => self.status = format!("Saved debug request: {}", path.display()),
            Err(error) => {
                self.status = "Debug request could not be saved.".into();
                self.report = error;
            }
        }
    }

    fn refresh_pixel_comparison(&mut self) {
        self.pixel_comparison = match (&self.reference_image, &self.output_image) {
            (Some(reference), Some(output)) => Some(compare_images(reference, output)),
            _ => None,
        };
    }

    fn refresh_aex(&mut self) {
        let Some(selected) = &self.selection else {
            return;
        };
        let path = selected.path.clone();
        let previous_hash = selected.sha256.clone();
        match read_bounded_pe(&path) {
            Ok(bytes) => {
                let hash = format!("{:X}", Sha256::digest(&bytes));
                let metadata = fs::metadata(&path).ok();
                let profile = profile_for_hash(&hash);
                let identity_changed = hash != previous_hash;
                if identity_changed {
                    // Do not keep a worker holding the previous build alive.
                    self.close_live_session();
                }
                self.selection = Some(Selection {
                    path: path.clone(),
                    size: bytes.len() as u64,
                    sha256: hash,
                    profile,
                    modified: metadata.and_then(|value| value.modified().ok()),
                });
                self.selection_stale = false;
                self.parameters.clear();
                self.audio_input = None;
                self.audio_effect_only = false;
                self.smart_render = false;
                self.smart_render_advertised = None;
                self.smart_render_capability = None;
                self.smart_render_manual_override = false;
                self.host_context = None;
                self.output_image = None;
                self.preview = None;
                if identity_changed {
                    self.approved_dependencies.clear();
                    match discover_adjacent_imports(&path) {
                        Ok(discovery) => {
                            self.accept_adjacent_discovery(discovery);
                            match self.approve_session() {
                                Ok(()) => {
                                    self.inspect_after_refresh = true;
                                    self.status =
                                        "Rebuilt AEX identity and dependencies refreshed.".into();
                                }
                                Err(error) => {
                                    self.invalidate_session_approval(
                                        "Rebuilt dependency manifest validation failed.",
                                    );
                                    self.report = error;
                                }
                            }
                        }
                        Err(error) => {
                            self.invalidate_session_approval(
                                "Rebuilt dependency discovery failed safely.",
                            );
                            self.report = error;
                        }
                    }
                } else {
                    self.status = "AEX identity is unchanged.".into();
                }
            }
            Err(error) => {
                self.status = "Could not reload the selected AEX.".into();
                self.report = error.to_string();
            }
        }
    }

    fn check_selected_identity(&mut self) {
        if self.last_identity_check.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.last_identity_check = Instant::now();
        let Some(selected) = &self.selection else {
            return;
        };
        let Ok(metadata) = fs::metadata(&selected.path) else {
            self.selection_stale = true;
            self.status = "Selected AEX is unavailable. Reload after the build finishes.".into();
            self.close_live_session();
            return;
        };
        let modified = metadata.modified().ok();
        let hash_changed = read_bounded_pe(&selected.path).map_or(true, |bytes| {
            !format!("{:X}", Sha256::digest(bytes)).eq_ignore_ascii_case(&selected.sha256)
        });
        if metadata.len() != selected.size || modified != selected.modified || hash_changed {
            if !self.selection_stale {
                self.status =
                    "AEX build changed. Reload its identity before native execution.".into();
            }
            self.selection_stale = true;
            self.session_approved = false;
            self.approved_dependencies.clear();
            // The resident worker still holds the previous build; a rebuilt
            // AEX must go through a fresh session open.
            self.close_live_session();
        }
        if self.dependencies.iter().any(|dependency| {
            read_bounded_pe(&dependency.path).map_or(true, |bytes| {
                bytes.len() as u64 != dependency.size
                    || !format!("{:X}", Sha256::digest(&bytes))
                        .eq_ignore_ascii_case(&dependency.sha256)
            })
        }) {
            self.invalidate_session_approval(
                "A dependency DLL changed. Re-add it and approve the session again.",
            );
        }
    }

    fn inspect_parameters_async(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Loading Effect Controls...".into();
        self.spawn_native("inspect_parameters", move || {
            let (parameters, diagnostics) =
                aexcompat_broker::image_render::inspect_experimental_with_diagnostics(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            let report = serde_json::json!({
                "stage": "parameter_inspection",
                "parameters": parameters,
                "worker_diagnostics": diagnostics,
            });
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
        self.task_kind = TaskKind::InspectParameters;
    }

    fn inspect_external_dependencies(&mut self, missing_only: bool) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = if missing_only {
            "Inspecting missing external dependencies..."
        } else {
            "Inspecting all external dependencies..."
        }
        .into();
        self.spawn_native(
            if missing_only {
                "inspect_missing_dependencies"
            } else {
                "inspect_dependencies"
            },
            move || {
                let report =
                    aexcompat_broker::image_render::inspect_experimental_external_dependencies(
                        &repository,
                        &plugin_path,
                        &hash,
                        missing_only,
                    )
                    .map_err(|error| error.to_string())?;
                Ok((
                    serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                    None,
                ))
            },
        );
    }

    fn probe_options_dialog(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing the advertised options dialog...".into();
        self.spawn_native("probe_options_dialog", move || {
            let report = aexcompat_broker::image_render::probe_experimental_options_dialog(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_automatic_options_dialog(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing the sequence-requested options dialog...".into();
        self.spawn_native("probe_automatic_options_dialog", move || {
            let report =
                aexcompat_broker::image_render::probe_experimental_automatic_options_dialog(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_nop_render(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing NOP_RENDER source passthrough...".into();
        self.spawn_native("probe_nop_render", move || {
            let report = aexcompat_broker::image_render::probe_experimental_nop_render(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_smart_nop_render(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing SmartFX NOP_RENDER source passthrough...".into();
        self.spawn_native("probe_smart_nop_render", move || {
            let report = aexcompat_broker::image_render::probe_experimental_smart_nop_render(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_input_buffer_write(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing advertised input-buffer write access...".into();
        self.spawn_native("probe_input_buffer_write", move || {
            let report = aexcompat_broker::image_render::probe_experimental_input_buffer_write(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_smart_input_buffer_write(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing SmartFX input-buffer write access...".into();
        self.spawn_native("probe_smart_input_buffer_write", move || {
            let report =
                aexcompat_broker::image_render::probe_experimental_smart_input_buffer_write(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_frame_resize(&mut self, expand: bool) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = if expand {
            "Probing advertised FRAME_SETUP expansion...".into()
        } else {
            "Probing advertised FRAME_SETUP shrink...".into()
        };
        self.spawn_native(
            if expand {
                "probe_frame_expansion"
            } else {
                "probe_frame_shrink"
            },
            move || {
                let report = if expand {
                    aexcompat_broker::image_render::probe_experimental_expand_buffer(
                        &repository,
                        &plugin_path,
                        &hash,
                    )
                } else {
                    aexcompat_broker::image_render::probe_experimental_shrink_buffer(
                        &repository,
                        &plugin_path,
                        &hash,
                    )
                }
                .map_err(|error| error.to_string())?;
                Ok((
                    serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                    None,
                ))
            },
        );
    }

    fn probe_persistent_sequence(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing two frames in one isolated sequence...".into();
        self.spawn_native("probe_persistent_sequence", move || {
            let report = aexcompat_broker::image_render::probe_experimental_persistent_sequence(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_flattened_sequence(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing sequence save/reload ownership...".into();
        self.spawn_native("probe_flattened_sequence", move || {
            let report = aexcompat_broker::image_render::probe_experimental_flattened_sequence(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_copied_flattened_sequence(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing non-destructive sequence save...".into();
        self.spawn_native("probe_copied_flattened_sequence", move || {
            let report =
                aexcompat_broker::image_render::probe_experimental_copied_flattened_sequence(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_custom_ui_cursor(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching an isolated custom UI cursor event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_cursor(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI requested the eyedropper cursor.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI cursor event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_draw(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Recording an isolated custom UI draw event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_draw(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI draw commands were recorded safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
                self.apply_custom_ui_draw_to_render = true;
                self.apply_custom_ui_click_to_render = false;
            }
            Err(error) => {
                self.status = "Custom UI draw event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_lifecycle(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching an isolated custom UI lifecycle...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_lifecycle(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI lifecycle completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI lifecycle failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_idle(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching one custom UI idle event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_idle(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI idle lifecycle completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI idle event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_keydown(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching one custom UI key event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_keydown(
            &self.repository,
            &selection.path,
            &selection.sha256,
            self.custom_ui_click_point,
            self.custom_ui_keycode,
            self.custom_ui_key_modifiers,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI key lifecycle completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI key event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_mouse_exited(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching a Layer/Comp custom UI mouse-exited event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_mouse_exited(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI mouse-exited lifecycle completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI mouse-exited event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_click(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching an isolated custom UI click event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_click(
            &self.repository,
            &selection.path,
            &selection.sha256,
            self.custom_ui_click_point,
            self.custom_ui_click_color,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI click changed the effect value safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
                self.apply_custom_ui_click_to_render = true;
                self.apply_custom_ui_draw_to_render = false;
            }
            Err(error) => {
                self.status = "Custom UI click failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_drag(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching a bounded custom UI drag sequence...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_drag(
            &self.repository,
            &selection.path,
            &selection.sha256,
            self.custom_ui_click_point,
            self.custom_ui_drag_end,
            self.custom_ui_drag_steps,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI drag sequence completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI drag failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn render_to(&mut self, output: PathBuf) {
        let Some(selection) = &self.selection else {
            return;
        };
        let Some(input) = self.input_image.clone() else {
            return;
        };
        let Some(capability) = self.smart_render_capability else {
            self.status =
                "Render blocked: no valid SmartFX/classic capability inspection is available."
                    .into();
            self.report = "render capability is missing, malformed, or stale; reload Effect Controls before rendering".into();
            return;
        };
        let interactive_selection = match selected_interactive_session_selection(
            capability,
            self.smart_render,
            self.smart_render_manual_override,
        ) {
            Ok(selection) => selection,
            Err(error) => {
                self.status =
                    "Render blocked: selected path is unsupported by the inspected AEX.".into();
                self.report = error;
                return;
            }
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        let registered = selection.profile == Some("scattermap");
        let parameters = self.parameters.clone();
        let host_context = self.host_context.clone();
        let smart = interactive_selection.path.is_smart();
        let pixel_format = self.pixel_format;
        let gpu_backend = self.gpu_backend;
        let audio_sidecar = self.audio_input.clone();
        let dependencies = self.approved_dependencies.clone();
        let custom_ui_action = if self.apply_custom_ui_click_to_render {
            Some(aexcompat_broker::image_render::RenderUiAction::Click {
                point: self.custom_ui_click_point,
                color: self.custom_ui_click_color,
            })
        } else if self.apply_custom_ui_draw_to_render {
            Some(aexcompat_broker::image_render::RenderUiAction::Draw)
        } else {
            None
        };
        let timing = match render_timing(
            self.frame,
            self.duration_frames,
            self.frames_per_second,
            self.frame_time_step,
        ) {
            Ok(timing) => timing,
            Err(error) => {
                self.status = "Render timing is invalid.".into();
                self.report = error;
                return;
            }
        };
        if audio_sidecar.is_some()
            && (smart
                || pixel_format != aexcompat_broker::image_render::RenderPixelFormat::Argb8
                || host_context.is_some()
                || custom_ui_action.is_some())
        {
            self.status = "Audio sidecar requires plain classic ARGB8 rendering.".into();
            self.report = "Disable SmartFX, deep color, mask/spatial/render context, and custom UI actions before rendering with audio.".into();
            return;
        }
        if audio_sidecar.is_some() && !dependencies.is_empty() {
            self.status =
                "Dependency DLLs are not supported by the audio-sidecar render path.".into();
            self.report =
                "Remove dependencies or disable the audio sidecar before rendering.".into();
            return;
        }
        let use_registered_default = registered
            && parameters.is_empty()
            && self.frame == 0
            && custom_ui_action.is_none()
            && pixel_format == aexcompat_broker::image_render::RenderPixelFormat::Argb8
            && audio_sidecar.is_none();
        // The resident-session selector path follows the existing inspection
        // result and explicit GUI override.  SmartFX-capable AEXes must open a
        // Smart session: routing them through Classic RENDER can yield a
        // no-op frame that looks successful (#606). The session transport is
        // Windows-only; other targets always render one-shot.
        #[cfg(windows)]
        {
            let live_eligible = host_context.is_none()
                && custom_ui_action.is_none()
                && audio_sidecar.is_none()
                && gpu_backend == aexcompat_broker::image_render::RenderGpuBackend::Auto
                && !use_registered_default
                && !parameters.iter().any(|parameter| parameter.kind == "layer");
            if live_eligible {
                let identity = DispatchIdentity {
                    sha256: hash.clone(),
                    size: self
                        .selection
                        .as_ref()
                        .map(|item| item.size)
                        .unwrap_or_default(),
                };
                let diagnostic_eligible = self
                    .selection
                    .as_ref()
                    .is_some_and(|item| item.profile.is_none());
                let (respond, receiver) = mpsc::channel();
                let request = LiveRenderRequest {
                    repository,
                    plugin_path,
                    plugin_sha256: hash,
                    dependencies,
                    parameters,
                    selection: interactive_selection,
                    input_path: input,
                    timing,
                    pixel_format,
                    output,
                    respond,
                    identity,
                    diagnostic_eligible,
                };
                if self.live_session.is_none() {
                    let (sender, commands) = mpsc::channel();
                    spawn_live_session_thread(commands);
                    self.live_session = Some(LiveSessionHandle { sender });
                }
                let sent = self
                    .live_session
                    .as_ref()
                    .expect("session handle just ensured")
                    .sender
                    .send(LiveCommand::Render(Box::new(request)));
                if sent.is_ok() {
                    self.status = "Rendering through the resident session...".into();
                    self.rendering = true;
                    self.receiver = Some(receiver);
                    self.busy = true;
                    self.task_kind = TaskKind::Generic;
                    return;
                }
                // The session thread is gone; drop the handle so the next render
                // starts a fresh one. Nothing rendered on this attempt.
                self.live_session = None;
                self.status = "The session thread had exited; press Render to retry.".into();
                return;
            }
        }
        self.status = "Rendering in an isolated worker...".into();
        self.rendering = true;
        self.spawn_native("render_image", move || {
            let report = if let Some(audio) = audio_sidecar {
                aexcompat_broker::image_render::render_experimental_image_with_audio_sidecar(
                    &repository,
                    &plugin_path,
                    &hash,
                    &input,
                    &audio,
                    &output,
                    &parameters,
                    timing,
                )
            } else if !smart && use_registered_default && dependencies.is_empty() {
                aexcompat_broker::image_render::render_scattermap_fixture(
                    &repository,
                    "scattermap",
                    &input,
                    &output,
                )
            } else {
                aexcompat_broker::image_render::render_experimental_image_with_approved_dependencies(
                    &repository,
                    &plugin_path,
                    &hash,
                    &input,
                    &output,
                    &parameters,
                    timing,
                    smart,
                    pixel_format,
                    host_context.as_ref(),
                    custom_ui_action,
                    gpu_backend,
                    dependencies,
                )
            }
            .map_err(|error| interactive_selection_failure(interactive_selection, error.to_string()))?;
            let mut report = report;
            aexcompat_broker::image_render::annotate_interactive_selection(
                &mut report,
                interactive_selection,
            );
            let body = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
            Ok((body, Some(output)))
        });
    }

    fn render_and_save(&mut self) {
        let Some(output) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .set_file_name("aex-output.png")
            .save_file()
        else {
            return;
        };
        self.render_to(output);
    }

    fn render_audio_and_save(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let Some(input) = self.audio_input.clone() else {
            return;
        };
        let Some(output) = rfd::FileDialog::new()
            .add_filter("Mono float32 LE audio", &["f32"])
            .set_file_name("aex-output.f32")
            .save_file()
        else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        let parameters = self.parameters.clone();
        self.status = "Rendering audio in an isolated worker...".into();
        self.spawn_native("render_audio", move || {
            let report = aexcompat_broker::image_render::render_experimental_audio(
                &repository,
                &plugin_path,
                &hash,
                &input,
                &output,
                &parameters,
            )
            .map_err(|error| error.to_string())?;
            let mut report = report;
            report["published_output"] = serde_json::json!(output);
            let body = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
            Ok((body, None))
        });
    }

    fn quick_render(&mut self) {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let output = self
            .repository
            .join("target/harness-output")
            .join(format!("render-{nonce}.png"));
        self.render_to(output);
    }

    fn run_compatibility_matrix(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let Some(input) = self.input_image.clone() else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        let parameters = self.parameters.clone();
        let host_context = self.host_context.clone();
        let timing = match render_timing(
            self.frame,
            self.duration_frames,
            self.frames_per_second,
            self.frame_time_step,
        ) {
            Ok(timing) => timing,
            Err(error) => {
                self.status = "Matrix timing is invalid.".into();
                self.report = error;
                return;
            }
        };
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let output_root = repository
            .join("target/harness-matrix")
            .join(nonce.to_string());
        self.status = "Running six isolated Effect compatibility cases...".into();
        self.matrix_results.clear();
        self.spawn_native("compatibility_matrix", move || {
            let report = run_effect_matrix(
                &repository,
                &plugin_path,
                &hash,
                &input,
                &output_root,
                &parameters,
                timing,
                None,
                host_context.as_ref(),
            );
            let body = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
            Ok((body, None))
        });
    }

    fn trigger_button(&mut self, slot: u32) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        let parameters = self.parameters.clone();
        self.status = format!("Dispatching PF_Cmd_USER_CHANGED_PARAM for slot {slot}...");
        self.spawn_native("user_changed_parameter", move || {
            let report = aexcompat_broker::image_render::trigger_experimental_button(
                &repository,
                &plugin_path,
                &hash,
                slot,
                &parameters,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn initialize_aegp(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Initializing AEGP in an isolated worker...".into();
        self.spawn_native("initialize_aegp", move || {
            let report = aexcompat_broker::image_render::initialize_experimental_aegp(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn update_aegp_menu(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Dispatching an isolated AEGP update-menu event...".into();
        self.spawn_native("update_aegp_menu", move || {
            let report = aexcompat_broker::image_render::dispatch_experimental_aegp_update_menu(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn dispatch_aegp_idle(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Dispatching one isolated AEGP idle event...".into();
        self.spawn_native("dispatch_aegp_idle", move || {
            let report = aexcompat_broker::image_render::dispatch_experimental_aegp_idle(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn dispatch_aegp_command_roundtrip(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Dispatching an isolated AEGP command ON/OFF roundtrip...".into();
        self.spawn_native("dispatch_aegp_command_roundtrip", move || {
            let report =
                aexcompat_broker::image_render::dispatch_experimental_aegp_command_roundtrip(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn dispatch_aegp_active_idle_roundtrip(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Running AEGP ON / active idle / OFF in isolation...".into();
        self.spawn_native("dispatch_aegp_active_idle_roundtrip", move || {
            let report =
                aexcompat_broker::image_render::dispatch_experimental_aegp_active_idle_roundtrip(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn dispatch_aegp_comp_idle_roundtrip(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Running AEGP ON / comp idle / OFF in isolation...".into();
        self.spawn_native("dispatch_aegp_comp_idle_roundtrip", move || {
            let report =
                aexcompat_broker::image_render::dispatch_experimental_aegp_comp_idle_roundtrip(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn show_effect_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(RichText::new("Effect Controls").size(20.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !self.busy && !self.parameter_defaults.is_empty(),
                        egui::Button::new("Reset All"),
                    )
                    .clicked()
                {
                    self.parameters = self.parameter_defaults.clone();
                    self.pending_parameter_slot = None;
                    self.pending_live_render = self.live_render;
                    self.live_render_due = self
                        .live_render
                        .then(|| Instant::now() + std::time::Duration::from_millis(500));
                }
            });
        });
        if let Some(selection) = &self.selection {
            ui.label(
                selection
                    .path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Selected AEX"),
            );
        } else {
            ui.label("Select an AEX to load its parameters.");
        }
        ui.separator();
        if self.busy && self.task_kind == TaskKind::InspectParameters {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading parameters...");
            });
        } else if self.selection.is_some() && self.parameters.is_empty() {
            ui.label("This effect exposed no editable parameters.");
            if ui.small_button("Reload controls").clicked() {
                self.inspect_parameters_async();
            }
        }

        let mut clicked_button = None;
        let parameter_defaults = self.parameter_defaults.clone();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for parameter in &mut self.parameters {
                    if !parameter.visible {
                        continue;
                    }
                    if parameter.kind == "group_start" {
                        ui.add_space(8.0);
                        ui.label(RichText::new(&parameter.name).strong());
                        continue;
                    }
                    if parameter.kind == "group_end" {
                        ui.separator();
                        continue;
                    }
                    if parameter.kind == "button" {
                        if ui
                            .add_enabled(
                                parameter.enabled && !self.busy,
                                egui::Button::new(&parameter.name),
                            )
                            .clicked()
                        {
                            clicked_button = Some(parameter.slot);
                        }
                        continue;
                    }
                    if matches!(parameter.kind.as_str(), "custom" | "no_data") {
                        ui.label(&parameter.name);
                        ui.small(format!("{} (read-only)", parameter.kind));
                        continue;
                    }

                    let previous_value = parameter.value;
                    let previous_color = parameter.color;
                    let previous_components = parameter.components;
                    let previous_layer = parameter.layer_path.clone();
                    let previous_summary = parameter.debug_summary.clone();
                    ui.add_enabled_ui(parameter.enabled && !self.busy, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&parameter.name).small());
                            if let Some(default) = parameter_defaults
                                .iter()
                                .find(|default| default.slot == parameter.slot)
                            {
                                let changed_from_default = parameter.value != default.value
                                    || parameter.color != default.color
                                    || parameter.components != default.components
                                    || parameter.layer_path != default.layer_path
                                    || parameter.debug_summary != default.debug_summary;
                                if ui
                                    .add_enabled(
                                        changed_from_default,
                                        egui::Button::new("Reset").small(),
                                    )
                                    .clicked()
                                {
                                    parameter.value = default.value;
                                    parameter.color = default.color;
                                    parameter.components = default.components;
                                    parameter.layer_path = default.layer_path.clone();
                                    parameter.debug_summary = default.debug_summary.clone();
                                }
                            }
                        });
                        if parameter.kind == "layer" {
                            ui.horizontal(|ui| {
                                if ui.small_button("Choose image").clicked() {
                                    parameter.layer_path = rfd::FileDialog::new()
                                        .add_filter(
                                            "Image",
                                            &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"],
                                        )
                                        .pick_file();
                                }
                                ui.label(
                                    parameter
                                        .layer_path
                                        .as_ref()
                                        .and_then(|path| path.file_name())
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("Not connected"),
                                );
                            });
                        } else if parameter.kind == "arbitrary_data" {
                            ui.add(
                                egui::TextEdit::singleline(
                                    parameter.debug_summary.get_or_insert_with(String::new),
                                )
                                .desired_width(f32::INFINITY),
                            );
                        } else if parameter.kind == "path" {
                            ui.add(
                                egui::DragValue::new(&mut parameter.value)
                                    .range(0.0..=parameter.maximum),
                            );
                        } else if matches!(parameter.kind.as_str(), "angle" | "point" | "point3d") {
                            ui.horizontal(|ui| {
                                for (index, label) in ["X", "Y", "Z"]
                                    .iter()
                                    .enumerate()
                                    .take(parameter.component_count)
                                {
                                    ui.label(*label);
                                    ui.add(
                                        egui::DragValue::new(&mut parameter.components[index])
                                            .speed(0.1),
                                    );
                                }
                            });
                        } else if parameter.kind == "color" {
                            let mut color = Color32::from_rgba_unmultiplied(
                                parameter.color[1],
                                parameter.color[2],
                                parameter.color[3],
                                parameter.color[0],
                            );
                            if ui.color_edit_button_srgba(&mut color).changed() {
                                parameter.color = [color.a(), color.r(), color.g(), color.b()];
                            }
                        } else if !parameter.choices.is_empty() {
                            let mut selected = parameter.value as usize;
                            egui::ComboBox::from_id_salt(("effect-control", parameter.slot))
                                .selected_text(
                                    parameter
                                        .choices
                                        .get(selected.saturating_sub(1))
                                        .map(String::as_str)
                                        .unwrap_or("Unknown"),
                                )
                                .show_ui(ui, |ui| {
                                    for (index, choice) in parameter.choices.iter().enumerate() {
                                        ui.selectable_value(&mut selected, index + 1, choice);
                                    }
                                });
                            parameter.value = selected as f64;
                        } else if parameter.kind == "integer"
                            && parameter.minimum == 0.0
                            && parameter.maximum == 1.0
                        {
                            let mut checked = parameter.value != 0.0;
                            if ui.checkbox(&mut checked, "Enabled").changed() {
                                parameter.value = f64::from(checked);
                            }
                        } else {
                            ui.add(
                                egui::Slider::new(
                                    &mut parameter.value,
                                    parameter.minimum..=parameter.maximum,
                                )
                                .show_value(true),
                            );
                        }
                    });
                    let changed = parameter.value != previous_value
                        || parameter.color != previous_color
                        || parameter.components != previous_components
                        || parameter.layer_path != previous_layer
                        || parameter.debug_summary != previous_summary;
                    if changed {
                        if parameter.supervised {
                            self.pending_parameter_slot = Some(parameter.slot);
                            self.live_render_due =
                                Some(Instant::now() + std::time::Duration::from_millis(500));
                        } else if self.live_render {
                            self.pending_live_render = true;
                            self.live_render_due =
                                Some(Instant::now() + std::time::Duration::from_millis(500));
                        }
                    }
                    ui.add_space(4.0);
                }
            });
        if let Some(slot) = clicked_button {
            self.pending_parameter_slot = Some(slot);
            self.pending_live_render = self.live_render;
            self.live_render_due = Some(Instant::now() + std::time::Duration::from_millis(500));
        }
    }

    fn dispatch_pending_parameter_change(&mut self, ctx: &egui::Context) {
        let Some(due) = self.live_render_due else {
            return;
        };
        if self.busy || Instant::now() < due {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
            return;
        }
        self.live_render_due = None;
        if let Some(slot) = self.pending_parameter_slot.take() {
            self.render_after_parameter_change = self.live_render && self.input_image.is_some();
            self.trigger_button(slot);
        } else if self.pending_live_render && self.live_render && self.input_image.is_some() {
            self.pending_live_render = false;
            self.quick_render();
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let result = self
            .receiver
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok());
        let Some(result) = result else {
            if self.busy {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            return;
        };
        let task_kind = self.task_kind;
        self.busy = false;
        self.rendering = false;
        if result.diagnostic_eligible {
            if let (Some(identity), Some(operation)) = (&result.identity, &result.operation) {
                let summary = diagnostic_summary(result.success, &result.body);
                let details = diagnostic_details(result.success, &result.body);
                match persist_diagnostic(
                    &self.repository,
                    identity,
                    operation,
                    result.success,
                    &summary,
                    &details,
                ) {
                    Ok(_) => {
                        self.diagnostic_warning = None;
                        self.diagnostic_history = self
                            .selection
                            .as_ref()
                            .map(|selection| {
                                load_diagnostic_history(&self.repository, &selection.sha256)
                            })
                            .unwrap_or_default();
                        self.missing_suite_aggregate = aggregate_missing_suites(&self.repository);
                    }
                    Err(error) => {
                        self.diagnostic_warning = Some(format!("Diagnostic save warning: {error}"))
                    }
                }
            }
        }
        self.failure_diagnostics = (!result.success)
            .then(|| failure_diagnostics(&result.body))
            .flatten();
        if !result.success {
            self.render_diagnostics = None;
        }
        self.matrix_results = serde_json::from_str(&result.body)
            .ok()
            .as_ref()
            .and_then(compatibility_matrix)
            .unwrap_or_default();
        self.status = if result.success {
            "Completed"
        } else {
            "Failed safely"
        }
        .into();
        let mut inspect_selected_aex = false;
        if task_kind == TaskKind::IdentifyAex && result.success {
            let mut lines = result.body.lines();
            if let (Some(path), Some(size), Some(hash)) = (lines.next(), lines.next(), lines.next())
            {
                let profile = profile_for_hash(hash);
                // A newly selected AEX replaces whatever the resident worker
                // was opened for.
                self.close_live_session();
                self.selection = Some(Selection {
                    path: path.into(),
                    size: size.parse().unwrap_or(0),
                    sha256: hash.into(),
                    profile,
                    modified: fs::metadata(path)
                        .ok()
                        .and_then(|value| value.modified().ok()),
                });
                self.session_approved = true;
                self.approved_dependencies.clear();
                self.approval_check = false;
                self.selection_stale = false;
                self.diagnostic_history = load_diagnostic_history(&self.repository, hash);
                match discover_adjacent_imports(Path::new(path)) {
                    Ok(discovery) => {
                        self.accept_adjacent_discovery(discovery);
                        match self.approve_session() {
                            Ok(()) => inspect_selected_aex = true,
                            Err(error) => {
                                self.invalidate_session_approval(
                                    "Automatic dependency manifest validation failed.",
                                );
                                self.report = error;
                            }
                        }
                    }
                    Err(error) => {
                        self.invalidate_session_approval(
                            "Automatic adjacent dependency discovery failed safely.",
                        );
                        self.report = error;
                    }
                }
            }
        }
        let mut effect_controls_ready = false;
        let mut inspection_blocker = None;
        if task_kind == TaskKind::InspectParameters {
            // Inspection is the sole capability authority.  Clear every
            // path/source/override before accepting new facts so a failed,
            // missing, malformed, or contradictory result cannot inherit the
            // previously selected plug-in's route.
            self.close_live_session();
            self.smart_render = false;
            self.smart_render_advertised = None;
            self.smart_render_capability = None;
            self.smart_render_manual_override = false;
            if result.success {
                let accepted = (|| -> Result<_, String> {
                    let report = serde_json::from_str::<serde_json::Value>(&result.body)
                        .map_err(|error| format!("inspection report is invalid JSON: {error}"))?;
                    let capability = inspected_render_capability(&report)?;
                    let parameters = serde_json::from_value(report["parameters"].clone())
                        .map_err(|error| format!("inspection parameters are invalid: {error}"))?;
                    let audio_effect_only = report["worker_diagnostics"]["audio_effect_only"]
                        .as_bool()
                        .unwrap_or(false);
                    Ok((capability, parameters, audio_effect_only))
                })();
                match accepted {
                    Ok((capability, parameters, audio_effect_only)) => {
                        self.parameters = parameters;
                        self.parameter_defaults = self.parameters.clone();
                        self.audio_effect_only = audio_effect_only;
                        self.smart_render = capability.smart_render_advertised;
                        self.smart_render_advertised = Some(capability.smart_render_advertised);
                        self.smart_render_capability = Some(capability);
                        effect_controls_ready = true;
                        self.status = format!(
                            "Effect Controls ready: {} editable parameter(s). Render path: {}.",
                            self.parameters.len(),
                            if self.smart_render {
                                "SmartFX"
                            } else {
                                "Classic"
                            }
                        );
                    }
                    Err(error) => {
                        self.parameters.clear();
                        self.parameter_defaults.clear();
                        self.audio_effect_only = false;
                        self.status = "Effect Controls capability inspection failed safely; rendering is blocked.".into();
                        inspection_blocker = Some(error);
                    }
                }
            }
        }
        if let Some(output) = result.output {
            self.render_diagnostics = serde_json::from_str(&result.body)
                .ok()
                .as_ref()
                .and_then(render_diagnostics);
            self.output_image = Some(output.clone());
            if let Ok(preview) = load_preview(ctx, "output", &output) {
                self.preview = Some(preview);
                self.viewer_mode = 1;
            }
            self.refresh_pixel_comparison();
        }
        if let Ok(report) = serde_json::from_str(&result.body) {
            apply_dynamic_ui_report(&mut self.parameters, &report);
        }
        self.report = inspection_blocker.unwrap_or(result.body);
        self.receiver = None;
        self.task_kind = TaskKind::Generic;
        if inspect_selected_aex {
            self.inspect_parameters_async();
        } else if effect_controls_ready {
            self.start_live_render_if_ready();
        } else if self.render_after_parameter_change && result.success {
            self.render_after_parameter_change = false;
            self.quick_render();
        } else {
            self.render_after_parameter_change = false;
        }
    }
}

impl eframe::App for HarnessApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_visuals(egui::Visuals::dark());
        self.poll(ctx);
        self.dispatch_pending_parameter_change(ctx);
        self.check_selected_identity();
        if self.inspect_after_refresh && !self.busy {
            self.inspect_after_refresh = false;
            self.inspect_parameters_async();
        }
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("AEXCompat").size(22.0));
                ui.weak("EFFECT LAB");
                ui.separator();
                ui.label(RichText::new("SOURCE").small().strong());
                if ui
                    .add_enabled(!self.busy, egui::Button::new("AEX..."))
                    .clicked()
                {
                    self.reset_and_choose_aex();
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Image..."))
                    .clicked()
                {
                    self.choose_input(ctx);
                    self.viewer_mode = 0;
                }
                ui.separator();
                ui.label(RichText::new("PREVIEW").small().strong());
                let can_render =
                    !self.busy && self.selection.is_some() && self.input_image.is_some();
                if ui
                    .add_enabled(can_render, egui::Button::new("Render"))
                    .clicked()
                {
                    self.quick_render();
                }
                ui.separator();
                let live_render_changed =
                    ui.checkbox(&mut self.live_render, "Auto Update").changed();
                if live_render_changed && !self.live_render {
                    self.pending_live_render = false;
                    if self.pending_parameter_slot.is_none() {
                        self.live_render_due = None;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.busy {
                        ui.spinner();
                    }
                    ui.label(RichText::new(&self.status).small());
                });
            });
            ui.add_space(6.0);
        });
        egui::SidePanel::left("effect_controls")
            .default_width(340.0)
            .min_width(260.0)
            .max_width(460.0)
            .resizable(true)
            .show(ctx, |ui| self.show_effect_controls(ui));
        egui::CentralPanel::default().show(ctx, |ui| {
            self.show_workspace_viewer(ui);
            ui.separator();
            egui::CollapsingHeader::new("Analysis, render settings and diagnostics")
                .default_open(false)
                .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Change AEX source..."))
                .clicked()
            {
                self.reset_and_choose_aex();
            }
            if let Some(selected) = &self.selection {
                let selected_path = selected.path.display().to_string();
                let selected_size = selected.size;
                let selected_hash = selected.sha256.clone();
                let selected_profile = selected.profile;
                ui.group(|ui| {
                    ui.label(RichText::new(selected_path).strong());
                    ui.collapsing("Binary details and dependency DLLs", |ui| {
                    ui.label(format!("{} bytes", selected_size));
                    ui.monospace(&selected_hash);
                    if self.selection_stale {
                        ui.colored_label(Color32::from_rgb(210, 75, 55), "Build changed: native execution is paused until reload");
                    }
                    if let Some(profile) = selected_profile {
                        ui.colored_label(Color32::from_rgb(30, 150, 95), format!("Registered profile: {profile}"));
                    }
                    ui.separator();
                    ui.label(RichText::new("Local diagnostics").strong());
                    ui.label(format!("Events for selected SHA: {}", self.diagnostic_history.count));
                    ui.label(format!("Latest: {}", self.diagnostic_history.latest.as_deref().unwrap_or("none")));
                    if let Some(warning) = &self.diagnostic_warning {
                        ui.colored_label(Color32::from_rgb(210, 145, 40), warning);
                    }
                    if ui.button("Reload diagnostics").clicked() {
                        self.diagnostic_history = load_diagnostic_history(&self.repository, &selected_hash);
                        self.missing_suite_aggregate = aggregate_missing_suites(&self.repository);
                    }
                    let aggregate = &self.missing_suite_aggregate;
                    ui.label(RichText::new("Missing Suite gaps across all SHA directories").strong());
                    ui.label(format!(
                        "Coverage: {}/{} SHA directories; {} validated failure events",
                        aggregate.scanned_sha_count,
                        aggregate.discovered_sha_count,
                        aggregate.valid_failure_event_count
                    ));
                    ui.label(format!(
                        "Skipped entries/files: {}; truncated: {}",
                        aggregate.skipped_count,
                        if aggregate.truncated { "yes" } else { "no" }
                    ));
                    for gap in &aggregate.top {
                        ui.monospace(format!(
                            "{}@{}  SHA gaps={}  events={}",
                            gap.name, gap.version, gap.sha_count, gap.event_count
                        ));
                    }
                    ui.separator();
                    ui.label(RichText::new("Session dependency DLLs").strong());
                    if self.dependencies.is_empty() {
                        ui.label("No additional DLLs selected.");
                    }
                    for warning in &self.preflight_warnings {
                        ui.colored_label(
                            Color32::from_rgb(210, 145, 40),
                            format!(
                                "Preflight note: {} ({}) was not found beside the AEX; it may be supplied by the runtime environment.",
                                warning.basename,
                                warning.kind.as_str()
                            ),
                        );
                    }
                    let mut remove = None;
                    for (index, dependency) in self.dependencies.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let basename = dependency.path.file_name().and_then(|name| name.to_str()).unwrap_or("<invalid>");
                            let short_hash = dependency.sha256.get(..12).unwrap_or(&dependency.sha256);
                            ui.monospace(format!("{basename}  {short_hash}...  {} bytes", dependency.size));
                            if ui.add_enabled(!self.busy, egui::Button::new("Remove")).clicked() {
                                remove = Some(index);
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.busy, egui::Button::new("Add DLL")).clicked() {
                            self.add_dependency();
                        }
                        if ui.add_enabled(!self.busy && !self.dependencies.is_empty(), egui::Button::new("Clear all")).clicked() {
                            self.dependencies.clear();
                            self.preflight_warnings.clear();
                            self.approved_dependencies.clear();
                            self.session_approved = true;
                            self.status = "Dependency list cleared.".into();
                            self.close_live_session();
                        }
                    });
                    if let Some(index) = remove {
                        self.dependencies.remove(index);
                        if self.dependencies.is_empty() {
                            self.approved_dependencies.clear();
                            self.session_approved = true;
                            self.close_live_session();
                        } else if let Err(error) = self.approve_session() {
                            self.invalidate_session_approval("Dependency manifest validation failed.");
                            self.report = error;
                        }
                    }
                    if selected_profile.is_none() {
                        ui.colored_label(Color32::from_rgb(215, 145, 40), "Unregistered AEX: direct isolated execution enabled");
                        ui.label("The selected binary is hashed automatically and runs in a timeout-limited restricted worker. This is not a complete security sandbox.");
                    }
                    if ui.add_enabled(!self.busy, egui::Button::new("Reload rebuilt AEX")).clicked() {
                        self.refresh_aex();
                    }
                    });
                });
                if self.session_approved && !self.selection_stale {
                    ui.add_space(10.0);
                    if ui.add_enabled(!self.busy, egui::Button::new("Reload Effect Controls")).clicked() { self.inspect_parameters_async(); }
                    ui.collapsing("Developer probes and diagnostics", |ui| {
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.busy, egui::Button::new("Inspect all dependencies")).clicked() { self.inspect_external_dependencies(false); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Inspect missing dependencies")).clicked() { self.inspect_external_dependencies(true); }
                    });
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe 2-frame persistent sequence")).clicked() { self.probe_persistent_sequence(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe sequence save/reload")).clicked() { self.probe_flattened_sequence(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe non-destructive sequence save")).clicked() { self.probe_copied_flattened_sequence(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe options dialog")).clicked() { self.probe_options_dialog(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe automatic options dialog")).clicked() { self.probe_automatic_options_dialog(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe NOP_RENDER passthrough")).clicked() { self.probe_nop_render(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe SmartFX NOP_RENDER passthrough")).clicked() { self.probe_smart_nop_render(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe input-buffer write access")).clicked() { self.probe_input_buffer_write(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe SmartFX input-buffer write access")).clicked() { self.probe_smart_input_buffer_write(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe FRAME_SETUP expansion")).clicked() { self.probe_frame_resize(true); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe FRAME_SETUP shrink")).clicked() { self.probe_frame_resize(false); }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events & 4 != 0)
                        && ui.add_enabled(!self.busy, egui::Button::new("Probe custom UI cursor")).clicked()
                    {
                        self.probe_custom_ui_cursor();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0)
                        && ui.add_enabled(!self.busy, egui::Button::new("Record custom UI draw")).clicked()
                    {
                        self.probe_custom_ui_draw();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0) {
                        let changed = ui.checkbox(
                            &mut self.apply_custom_ui_draw_to_render,
                            "Draw custom UI before each render",
                        ).changed();
                        if changed && self.apply_custom_ui_draw_to_render {
                            self.apply_custom_ui_click_to_render = false;
                        }
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0)
                        && ui.add_enabled(!self.busy, egui::Button::new("Test custom UI lifecycle")).clicked()
                    {
                        self.probe_custom_ui_lifecycle();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0)
                        && ui.add_enabled(!self.busy, egui::Button::new("Dispatch custom UI idle")).clicked()
                    {
                        self.probe_custom_ui_idle();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0) {
                        ui.group(|ui| {
                            ui.label(RichText::new("Custom UI key event").strong());
                            ui.horizontal(|ui| {
                                ui.label("Keycode");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_keycode).range(0u32..=0xC000_FFFFu32));
                                ui.label(format!("0x{:08X}", self.custom_ui_keycode));
                                ui.label("Modifiers");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_key_modifiers));
                                if ui.add_enabled(!self.busy, egui::Button::new("Dispatch key")).clicked() {
                                    self.probe_custom_ui_keydown();
                                }
                            });
                            ui.label("Default is printable A. The custom UI click X/Y values are used as the screen point.");
                        });
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events & 4 != 0) {
                        ui.group(|ui| {
                            ui.label(RichText::new("Custom UI click").strong());
                            ui.horizontal(|ui| {
                                ui.label("X");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_click_point[0]).range(0..=8192));
                                ui.label("Y");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_click_point[1]).range(0..=8192));
                                ui.color_edit_button_rgba_unmultiplied(&mut self.custom_ui_click_color);
                                if ui.add_enabled(!self.busy, egui::Button::new("Dispatch click")).clicked() {
                                    self.probe_custom_ui_click();
                                }
                            });
                            let changed = ui.checkbox(
                                &mut self.apply_custom_ui_click_to_render,
                                "Apply this click before each render",
                            ).changed();
                            if changed && self.apply_custom_ui_click_to_render {
                                self.apply_custom_ui_draw_to_render = false;
                            }
                        });
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events & 3 != 0) {
                        ui.group(|ui| {
                            ui.label(RichText::new("Comp / Layer custom UI drag").strong());
                            ui.horizontal(|ui| {
                                ui.label("End X");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_drag_end[0]).range(0..=8192));
                                ui.label("End Y");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_drag_end[1]).range(0..=8192));
                                ui.label("Steps");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_drag_steps).range(1..=32));
                                if ui.add_enabled(!self.busy, egui::Button::new("Dispatch drag")).clicked() {
                                    self.probe_custom_ui_drag();
                                }
                            });
                            ui.label("The custom UI click X/Y values above are used as the drag start.");
                            if ui.add_enabled(!self.busy, egui::Button::new("Dispatch mouse exited")).clicked() {
                                self.probe_custom_ui_mouse_exited();
                            }
                        });
                    }
                    ui.collapsing("AEGP diagnostics (advanced)", |ui| {
                        if ui.add_enabled(!self.busy, egui::Button::new("Initialize as AEGP")).clicked() { self.initialize_aegp(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Dispatch AEGP update-menu")).clicked() { self.update_aegp_menu(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Dispatch one AEGP idle tick")).clicked() { self.dispatch_aegp_idle(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Run AEGP command ON/OFF roundtrip")).clicked() { self.dispatch_aegp_command_roundtrip(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Run AEGP active-idle roundtrip")).clicked() { self.dispatch_aegp_active_idle_roundtrip(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Run AEGP comp-idle roundtrip")).clicked() { self.dispatch_aegp_comp_idle_roundtrip(); }
                    });
                    });
                    // Effect parameters live in the persistent left-side Effect Controls panel.
                    if false {
                    let mut clicked_button = None;
                    for parameter in &mut self.parameters {
                        if !parameter.visible {
                            continue;
                        }
                        if parameter.kind == "group_start" {
                            ui.add_space(6.0);
                            ui.label(RichText::new(&parameter.name).strong().size(16.0));
                            continue;
                        }
                        if parameter.kind == "group_end" {
                            ui.separator();
                            continue;
                        }
                        if parameter.kind == "button" {
                            if ui
                                .add_enabled(
                                    parameter.enabled && !self.busy,
                                    egui::Button::new(&parameter.name),
                                )
                                .clicked()
                            {
                                clicked_button = Some(parameter.slot);
                            }
                            continue;
                        }
                        if matches!(
                            parameter.kind.as_str(),
                            "custom" | "no_data"
                        ) {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(&parameter.name);
                                ui.monospace(format!("{} (read-only)", parameter.kind));
                                if parameter.custom_ui_events != 0 {
                                    ui.monospace(format!(
                                        "custom UI {}x{}, events=0x{:X}",
                                        parameter.control_size[0],
                                        parameter.control_size[1],
                                        parameter.custom_ui_events
                                    ));
                                }
                                if let Some(summary) = &parameter.debug_summary {
                                    ui.collapsing("Observed value", |ui| {
                                        ui.monospace(summary);
                                    });
                                } else {
                                    ui.label("No printable value exposed by the effect.");
                                }
                            });
                            continue;
                        }
                        let previous_value = parameter.value;
                        let previous_color = parameter.color;
                        let previous_components = parameter.components;
                        let previous_layer = parameter.layer_path.clone();
                        let previous_summary = parameter.debug_summary.clone();
                        ui.add_enabled_ui(parameter.enabled, |ui| ui.horizontal(|ui| {
                            ui.label(&parameter.name);
                            if parameter.kind == "layer" {
                                if ui.button("Select image").clicked() {
                                    parameter.layer_path = rfd::FileDialog::new()
                                        .add_filter("Image", &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"])
                                        .pick_file();
                                }
                                if let Some(path) = &parameter.layer_path {
                                    ui.monospace(path.display().to_string());
                                } else {
                                    ui.label("Not connected");
                                }
                            } else if parameter.kind == "arbitrary_data" {
                                let text = parameter.debug_summary.get_or_insert_with(String::new);
                                ui.add(egui::TextEdit::singleline(text).desired_width(320.0));
                                ui.label("PRINT/SCAN text");
                            } else if parameter.kind == "path" {
                                ui.add(egui::DragValue::new(&mut parameter.value).range(0.0..=parameter.maximum).speed(1.0));
                                ui.label("0=None, 1..N=mask index");
                            } else if matches!(parameter.kind.as_str(), "angle" | "point" | "point3d") {
                                let labels = ["X", "Y", "Z"];
                                for (index, label) in labels.iter().enumerate().take(parameter.component_count) {
                                    ui.label(*label);
                                    ui.add(egui::DragValue::new(&mut parameter.components[index]).speed(0.1).range(-32768.0..=32768.0));
                                }
                            } else if parameter.kind == "color" {
                                let mut color = Color32::from_rgba_unmultiplied(parameter.color[1], parameter.color[2], parameter.color[3], parameter.color[0]);
                                if ui.color_edit_button_srgba(&mut color).changed() {
                                    parameter.color = [color.a(), color.r(), color.g(), color.b()];
                                }
                            } else if !parameter.choices.is_empty() {
                                let mut selected = parameter.value as usize;
                                egui::ComboBox::from_id_salt(parameter.slot)
                                    .selected_text(parameter.choices.get(selected.saturating_sub(1)).map(String::as_str).unwrap_or("Unknown"))
                                    .show_ui(ui, |ui| {
                                        for (index, choice) in parameter.choices.iter().enumerate() {
                                            ui.selectable_value(&mut selected, index + 1, choice);
                                        }
                                    });
                                parameter.value = selected as f64;
                            } else if parameter.kind == "integer" && parameter.minimum == 0.0 && parameter.maximum == 1.0 {
                                let mut checked = parameter.value != 0.0;
                                if ui.checkbox(&mut checked, "").changed() { parameter.value = if checked { 1.0 } else { 0.0 }; }
                            } else {
                                ui.add(egui::Slider::new(&mut parameter.value, parameter.minimum..=parameter.maximum));
                            }
                        }));
                        if parameter.supervised
                            && (parameter.value != previous_value
                                || parameter.color != previous_color
                                || parameter.components != previous_components
                                || parameter.layer_path != previous_layer
                                || parameter.debug_summary != previous_summary)
                        {
                            clicked_button = Some(parameter.slot);
                        }
                    }
                    if let Some(slot) = clicked_button {
                        self.trigger_button(slot);
                    }
                    }
                    ui.horizontal(|ui| {
                        ui.label("Render path:");
                        let classic_changed = ui
                            .selectable_value(&mut self.smart_render, false, "Classic")
                            .changed();
                        let smart_changed = ui
                            .selectable_value(&mut self.smart_render, true, "SmartFX")
                            .changed();
                        if classic_changed || smart_changed {
                            // Preserve the pre-resident GUI policy: choosing a
                            // different path is an explicit override, and
                            // changing it always tears down the old worker.
                            self.smart_render_manual_override = self
                                .smart_render_advertised
                                .is_some_and(|advertised| self.smart_render != advertised);
                            self.close_live_session();
                        }
                        if let Some(advertised) = self.smart_render_advertised {
                            ui.weak(if advertised { "advertised: SmartFX" } else { "advertised: Classic" });
                            if self.smart_render != advertised {
                                ui.colored_label(egui::Color32::from_rgb(230, 180, 60), "manual override");
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        use aexcompat_broker::image_render::RenderPixelFormat;
                        ui.label("Pixel depth:");
                        ui.selectable_value(&mut self.pixel_format, RenderPixelFormat::Argb8, "8 bpc");
                        ui.selectable_value(&mut self.pixel_format, RenderPixelFormat::Argb16, "16 bpc");
                        ui.selectable_value(&mut self.pixel_format, RenderPixelFormat::Argb32f, "32 bpc float");
                    });
                    ui.horizontal(|ui| {
                        use aexcompat_broker::image_render::RenderGpuBackend;
                        ui.label("GPU backend:");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::Auto, "Auto");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::Cuda, "CUDA");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::OpenCl, "OpenCL");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::DirectX, "DirectX");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::Cpu, "CPU");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Frame:");
                        if ui.add(egui::DragValue::new(&mut self.frame).range(0..=10_000_000)).changed() {
                            self.duration_frames = self.duration_frames.max(self.frame.saturating_add(1));
                        }
                        ui.label("Duration frames:");
                        ui.add(egui::DragValue::new(&mut self.duration_frames).range(self.frame.saturating_add(1)..=10_000_001));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Time scale:");
                        ui.add(egui::DragValue::new(&mut self.frames_per_second).range(1..=1_000_000));
                        ui.label("Frame step:");
                        ui.add(egui::DragValue::new(&mut self.frame_time_step).range(1..=100_000));
                        ui.label(format!("{:.5} fps", self.frames_per_second as f64 / self.frame_time_step as f64));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Rate presets:");
                        for (label, scale, step) in [("23.976", 24_000, 1_001), ("29.97", 30_000, 1_001), ("59.94", 60_000, 1_001)] {
                            if ui.button(label).clicked() {
                                self.frames_per_second = scale;
                                self.frame_time_step = step;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        let enabled = self.host_context.as_ref().and_then(|context| context.spatial).is_some();
                        let mut requested = enabled;
                        if ui.checkbox(&mut requested, "Spatial context").changed() {
                            if requested {
                                let context = self.host_context.get_or_insert_with(|| aexcompat_broker::render_request::HostContext {
                                    mask_scene: aexcompat_broker::render_request::MaskScene { masks: Vec::new() },
                                    spatial: None,
                                    render_environment: None,
                                    aux_channels: Vec::new(),
                                    alpha_as_coverage_params: Vec::new(),
                                });
                                context.spatial = Some(aexcompat_broker::render_request::SpatialContext {
                                    downsample_x: aexcompat_broker::render_request::RationalScale { numerator: 1, denominator: 1 },
                                    downsample_y: aexcompat_broker::render_request::RationalScale { numerator: 1, denominator: 1 },
                                    pixel_aspect_ratio: aexcompat_broker::render_request::RationalScale { numerator: 1, denominator: 1 },
                                    full_resolution_width: None,
                                    full_resolution_height: None,
                                    pre_effect_source_origin_x: None,
                                    pre_effect_source_origin_y: None,
                                });
                            } else if let Some(context) = &mut self.host_context {
                                context.spatial = None;
                                if context.mask_scene.masks.is_empty() { self.host_context = None; }
                            }
                        }
                        if enabled {
                            for (label, x, y, par) in [
                                ("Full", (1, 1), (1, 1), (1, 1)),
                                ("Half", (1, 2), (1, 2), (1, 1)),
                                ("Quarter", (1, 4), (1, 4), (1, 1)),
                                ("D1/DV NTSC", (1, 1), (1, 1), (10, 11)),
                            ] {
                                if ui.button(label).clicked() {
                                    if let Some(spatial) = self.host_context.as_mut().and_then(|context| context.spatial.as_mut()) {
                                        spatial.downsample_x = aexcompat_broker::render_request::RationalScale { numerator: x.0, denominator: x.1 };
                                        spatial.downsample_y = aexcompat_broker::render_request::RationalScale { numerator: y.0, denominator: y.1 };
                                        spatial.pixel_aspect_ratio = aexcompat_broker::render_request::RationalScale { numerator: par.0, denominator: par.1 };
                                        if let Some((width, height)) = self.input_image.as_ref().and_then(|path| image::image_dimensions(path).ok()) {
                                            spatial.full_resolution_width = width.checked_mul(x.1 as u32).and_then(|value| value.checked_div(x.0 as u32));
                                            spatial.full_resolution_height = height.checked_mul(y.1 as u32).and_then(|value| value.checked_div(y.0 as u32));
                                        }
                                    }
                                }
                            }
                        }
                    });
                    if let Some(spatial) = self.host_context.as_mut().and_then(|context| context.spatial.as_mut()) {
                        ui.horizontal(|ui| {
                            for (label, ratio) in [
                                ("Downsample X", &mut spatial.downsample_x),
                                ("Downsample Y", &mut spatial.downsample_y),
                                ("Pixel aspect", &mut spatial.pixel_aspect_ratio),
                            ] {
                                ui.label(label);
                                ui.add(egui::DragValue::new(&mut ratio.numerator).range(1..=1_000_000));
                                ui.label("/");
                                ui.add(egui::DragValue::new(&mut ratio.denominator).range(1..=1_000_000));
                            }
                        });
                        ui.horizontal(|ui| {
                            let mut explicit = spatial.full_resolution_width.is_some() && spatial.full_resolution_height.is_some();
                            if ui.checkbox(&mut explicit, "Explicit full-resolution size").changed() {
                                if explicit {
                                    let dimensions = self.input_image.as_ref().and_then(|path| image::image_dimensions(path).ok()).unwrap_or((1, 1));
                                    spatial.full_resolution_width = Some(dimensions.0);
                                    spatial.full_resolution_height = Some(dimensions.1);
                                } else {
                                    spatial.full_resolution_width = None;
                                    spatial.full_resolution_height = None;
                                }
                            }
                            if let (Some(width), Some(height)) = (&mut spatial.full_resolution_width, &mut spatial.full_resolution_height) {
                                ui.add(egui::DragValue::new(width).range(1..=32768));
                                ui.label("x");
                                ui.add(egui::DragValue::new(height).range(1..=32768));
                            }
                        });
                        ui.horizontal(|ui| {
                            let mut explicit = spatial.pre_effect_source_origin_x.is_some()
                                && spatial.pre_effect_source_origin_y.is_some();
                            if ui.checkbox(&mut explicit, "Pre-effect source origin").changed() {
                                if explicit {
                                    spatial.pre_effect_source_origin_x = Some(0);
                                    spatial.pre_effect_source_origin_y = Some(0);
                                } else {
                                    spatial.pre_effect_source_origin_x = None;
                                    spatial.pre_effect_source_origin_y = None;
                                }
                            }
                            if let (Some(x), Some(y)) = (
                                &mut spatial.pre_effect_source_origin_x,
                                &mut spatial.pre_effect_source_origin_y,
                            ) {
                                ui.label("X");
                                ui.add(egui::DragValue::new(x).range(-32768..=32768));
                                ui.label("Y");
                                ui.add(egui::DragValue::new(y).range(-32768..=32768));
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        let enabled = self.host_context.as_ref().and_then(|context| context.render_environment).is_some();
                        let mut requested = enabled;
                        if ui.checkbox(&mut requested, "Render environment").changed() {
                            if requested {
                                let context = self.host_context.get_or_insert_with(|| aexcompat_broker::render_request::HostContext {
                                    mask_scene: aexcompat_broker::render_request::MaskScene { masks: Vec::new() },
                                    spatial: None,
                                    render_environment: None,
                                    aux_channels: Vec::new(),
                                    alpha_as_coverage_params: Vec::new(),
                                });
                                context.render_environment = Some(aexcompat_broker::render_request::RenderEnvironment {
                                    quality: aexcompat_broker::render_request::RenderQuality::High,
                                    field: aexcompat_broker::render_request::RenderField::Frame,
                                    shutter_angle: 0.0,
                                    shutter_phase: 0.0,
                                });
                            } else if let Some(context) = &mut self.host_context {
                                context.render_environment = None;
                                if context.mask_scene.masks.is_empty() && context.spatial.is_none() { self.host_context = None; }
                            }
                        }
                    });
                    if let Some(environment) = self.host_context.as_mut().and_then(|context| context.render_environment.as_mut()) {
                        ui.horizontal(|ui| {
                            use aexcompat_broker::render_request::{RenderField, RenderQuality};
                            ui.label("Quality:");
                            ui.selectable_value(&mut environment.quality, RenderQuality::Low, "Low");
                            ui.selectable_value(&mut environment.quality, RenderQuality::High, "High");
                            ui.label("Field:");
                            ui.selectable_value(&mut environment.field, RenderField::Frame, "Frame");
                            ui.selectable_value(&mut environment.field, RenderField::Upper, "Upper");
                            ui.selectable_value(&mut environment.field, RenderField::Lower, "Lower");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Shutter angle:");
                            ui.add(egui::DragValue::new(&mut environment.shutter_angle).speed(0.01).range(0.0..=1.0));
                            ui.label("Shutter phase:");
                            ui.add(egui::DragValue::new(&mut environment.shutter_phase).speed(0.01).range(-1.0..=1.0));
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.busy && !self.parameters.is_empty(), egui::Button::new("Load debug request...")).clicked() { self.load_debug_request(); }
                        if ui.add_enabled(!self.busy && !self.parameters.is_empty(), egui::Button::new("Save debug request...")).clicked() { self.save_debug_request(); }
                    });
                    if let Some(mask_count) = self.host_context.as_ref().map(|context| context.mask_scene.masks.len()) {
                        ui.horizontal(|ui| {
                            ui.label(format!("Host mask context: {mask_count} mask(s)"));
                            if ui.add_enabled(!self.busy, egui::Button::new("Clear masks")).clicked() {
                                if let Some(context) = &mut self.host_context {
                                    context.mask_scene.masks.clear();
                                    if context.spatial.is_none() && context.render_environment.is_none() { self.host_context = None; }
                                }
                            }
                        });
                    }
                    if self.audio_effect_only {
                        ui.colored_label(Color32::from_rgb(30, 120, 170), RichText::new("Audio-only Effect").strong());
                        ui.label("Transport: 44.1 kHz, mono, float32 little-endian raw samples");
                        if ui.add_enabled(!self.busy, egui::Button::new("Change audio source (.f32)...")).clicked() { self.choose_audio_input(); }
                        if let Some(path) = &self.audio_input { ui.monospace(path.display().to_string()); }
                        if ui.add_enabled(!self.busy && self.audio_input.is_some(), egui::Button::new("4. Render and save audio (.f32)...")).clicked() { self.render_audio_and_save(); }
                    } else {
                        if ui.add_enabled(!self.busy, egui::Button::new("Change image source...")).clicked() { self.choose_input(ctx); }
                        if let Some(path) = &self.input_image { ui.monospace(path.display().to_string()); }
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!self.busy, egui::Button::new("Select visual audio sidecar (.f32, optional)")).clicked() { self.choose_audio_input(); }
                            if self.audio_input.is_some() && ui.add_enabled(!self.busy, egui::Button::new("Clear sidecar")).clicked() { self.audio_input = None; }
                        });
                        if let Some(path) = &self.audio_input { ui.monospace(format!("Audio sidecar: {}", path.display())); }
                        if self.audio_input.is_some() {
                            ui.label("Sidecar mode: classic ARGB8, mono float32 LE, 44.1 kHz");
                        }
                        if ui.add_enabled(!self.busy, egui::Button::new("Select AE reference output (optional)")).clicked() { self.choose_reference(ctx); }
                        if let Some(path) = &self.reference_image { ui.monospace(format!("Reference: {}", path.display())); }
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!self.busy && self.input_image.is_some(), egui::Button::new("Render current frame")).clicked() { self.quick_render(); }
                            if ui.add_enabled(!self.busy && self.input_image.is_some(), egui::Button::new("Render and save PNG...")).clicked() { self.render_and_save(); }
                            if ui.add_enabled(!self.busy && self.input_image.is_some(), egui::Button::new("Run 6-case compatibility matrix")).clicked() { self.run_compatibility_matrix(); }
                        });
                    }
                }
            }
            ui.separator();
            ui.label(RichText::new(&self.status).strong());
            if let Some(path) = &self.output_image { ui.monospace(format!("Output: {}", path.display())); }
            if self.input_preview.is_some() || self.preview.is_some() || self.reference_preview.is_some() {
                if ui.button("Open FHD image viewer").clicked() {
                    self.viewer_open = true;
                    self.viewer_mode = if self.preview.is_some() { 2 } else { 0 };
                }
                ui.columns(3, |columns| {
                    show_preview(&mut columns[0], "Input", self.input_preview.as_ref());
                    show_preview(&mut columns[1], "AEX output", self.preview.as_ref());
                    show_preview(&mut columns[2], "AE reference", self.reference_preview.as_ref());
                });
            }
            if let Some(comparison) = &self.pixel_comparison {
                ui.group(|ui| match comparison {
                    Ok(comparison) => {
                        let color = if comparison.exact() {
                            Color32::from_rgb(30, 150, 95)
                        } else {
                            Color32::from_rgb(215, 145, 40)
                        };
                        ui.colored_label(
                            color,
                            RichText::new(if comparison.exact() {
                                "Pixel-exact match with AE reference"
                            } else {
                                "Pixel difference from AE reference"
                            })
                            .strong(),
                        );
                        ui.monospace(format!(
                            "{}x{} | differing pixels: {} / {} | max channel error: {} | MAE: {:.6}",
                            comparison.width,
                            comparison.height,
                            comparison.differing_pixels,
                            u64::from(comparison.width) * u64::from(comparison.height),
                            comparison.max_channel_error,
                            comparison.mean_absolute_error
                        ));
                    }
                    Err(error) => {
                        ui.colored_label(Color32::from_rgb(210, 75, 55), RichText::new("AE reference comparison unavailable").strong());
                        ui.label(error);
                    }
                });
            }
            if let Some(diagnostics) = &self.render_diagnostics {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Effect render diagnostics").strong());
                        ui.monospace(format!(
                            "{} / {} / {}",
                            diagnostics.render_path,
                            diagnostics.pixel_format,
                            diagnostics.worker_classification
                        ));
                    });
                    if diagnostics.gpu_fallback_used {
                        ui.colored_label(
                            Color32::from_rgb(215, 145, 40),
                            format!(
                                "GPU attempt {} at {}; output rejected, fresh CPU worker succeeded",
                                diagnostics
                                    .gpu_attempt_classification
                                    .as_deref()
                                    .unwrap_or("failed"),
                                diagnostics.gpu_failure_stage.as_deref().unwrap_or("unknown stage")
                            ),
                        );
                    }
                    ui.collapsing("Final selector timeline", |ui| {
                        for stage in &diagnostics.final_stages {
                            ui.monospace(stage);
                        }
                    });
                    if !diagnostics.gpu_stages.is_empty() {
                        ui.collapsing("Rejected GPU selector timeline", |ui| {
                            for stage in &diagnostics.gpu_stages {
                                ui.monospace(stage);
                            }
                        });
                    }
                });
            }
            if let Some(diagnostics) = &self.failure_diagnostics {
                ui.group(|ui| {
                    let worker_succeeded = diagnostics.classification == "ok";
                    ui.colored_label(
                        if worker_succeeded {
                            Color32::from_rgb(205, 135, 35)
                        } else {
                            Color32::from_rgb(210, 75, 55)
                        },
                        RichText::new(if worker_succeeded {
                            "Effect worker succeeded; host validation stopped the result"
                        } else {
                            "Effect worker failed safely"
                        })
                        .strong(),
                    );
                    ui.monospace(format!(
                        "classification={} stage={} selector_error={} exit={} elapsed={}ms",
                        diagnostics.classification,
                        diagnostics.failure_stage.as_deref().unwrap_or("unknown"),
                        diagnostics
                            .selector_error
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".into()),
                        diagnostics
                            .exit_code
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".into()),
                        diagnostics
                            .elapsed_ms
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".into()),
                    ));
                    if let Some(selector) = &diagnostics.last_seh_selector {
                        ui.monospace(format!(
                            "seh_selector={} seh_error={} exception_code={}",
                            selector,
                            diagnostics
                                .last_seh_error
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "unknown".into()),
                            diagnostics
                                .last_seh_exception_code
                                .map(|value| format!("0x{value:08X}"))
                                .unwrap_or_else(|| "unknown".into()),
                        ));
                    }
                    if !diagnostics.missing_suites.is_empty() {
                        ui.label(RichText::new("Missing suites").strong());
                        for suite in &diagnostics.missing_suites {
                            ui.monospace(format!("suite:{}@{}", suite.name, suite.version));
                        }
                    }
                    ui.collapsing("Completed selector timeline", |ui| {
                        for stage in &diagnostics.stages {
                            ui.monospace(stage);
                        }
                    });
                });
            }
            if !self.matrix_results.is_empty() {
                ui.group(|ui| {
                    ui.label(RichText::new("Effect compatibility matrix").strong());
                    egui::Grid::new("effect_compatibility_matrix")
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Path");
                            ui.label("Depth");
                            ui.label("Result");
                            ui.label("Details");
                            ui.end_row();
                            for case in &self.matrix_results {
                                ui.monospace(&case.render_path);
                                ui.monospace(&case.pixel_format);
                                if case.passed {
                                    ui.colored_label(Color32::from_rgb(30, 150, 95), "PASS");
                                    let relation = match (
                                        case.output_relation.as_deref(),
                                        case.differing_input_pixels,
                                    ) {
                                        (Some("pixels_changed"), Some(count)) => {
                                            format!("pixels changed: {count}")
                                        }
                                        (Some(value), _) => value.replace('_', " "),
                                        _ => case.output_png.clone().unwrap_or_default(),
                                    };
                                    ui.monospace(relation);
                                } else if !case.applicable {
                                    ui.colored_label(Color32::from_rgb(215, 145, 40), "UNSUPPORTED");
                                    ui.monospace("AEX did not advertise this pixel depth");
                                } else {
                                    ui.colored_label(Color32::from_rgb(210, 75, 55), "FAIL");
                                    let mut details = format!(
                                        "{} / {} / error {}",
                                        case.classification,
                                        case.failure_stage.as_deref().unwrap_or("unknown"),
                                        case.selector_error
                                            .map(|value| value.to_string())
                                            .unwrap_or_else(|| "unknown".into())
                                    );
                                    if let Some(error) = &case.error {
                                        details.push_str(" / ");
                                        details.push_str(error);
                                    }
                                    ui.monospace(details);
                                }
                                ui.end_row();
                            }
                        });
                });
            }
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.report).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
            });
                });
                });
        });
        self.show_image_viewer(ctx);
    }
}
