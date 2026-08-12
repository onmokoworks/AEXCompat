const COLUMN_HEADER_HEIGHT: f32 = 84.0;
const ANALYSIS_PANEL_PADDING: f32 = 12.0;
const ANALYSIS_PANEL_ACTION_WIDTH: f32 = 268.0;
const ANALYSIS_PANEL_COLLAPSED_WIDTH: f32 = 18.0;
const ANALYSIS_PANEL_MAX_WIDTH: f32 = 620.0;

fn analysis_panel_min_width() -> f32 {
    ANALYSIS_PANEL_ACTION_WIDTH + ANALYSIS_PANEL_PADDING * 2.0
}

fn clamp_analysis_panel_width(width: f32) -> f32 {
    width.clamp(analysis_panel_min_width(), ANALYSIS_PANEL_MAX_WIDTH)
}

fn analysis_panel_display_width(width: f32, openness: f32) -> f32 {
    egui::lerp(
        ANALYSIS_PANEL_COLLAPSED_WIDTH..=clamp_analysis_panel_width(width),
        openness.clamp(0.0, 1.0),
    )
}

fn resized_analysis_panel_width(stored_width: f32, delta_x: f32) -> f32 {
    clamp_analysis_panel_width(stored_width + delta_x)
}

fn analysis_section_heading(ui: &mut egui::Ui, title: &str, color: Color32) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).small().strong().color(color));
    ui.separator();
}

fn analysis_action_button(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    help: &str,
) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(label))
        .on_hover_text(help)
}

fn analysis_setting_row(ui: &mut egui::Ui, label: &str, add_controls: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized(
            egui::vec2(112.0, ui.spacing().interact_size.y),
            egui::Label::new(label),
        );
        ui.add_space(8.0);
        add_controls(ui);
    });
}

fn fixed_column_header(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let (header_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), COLUMN_HEADER_HEIGHT),
        egui::Sense::hover(),
    );
    let mut header_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(header_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    add_contents(&mut header_ui);
}

fn fit_size_to_aspect(available: egui::Vec2, aspect: f32) -> egui::Vec2 {
    let available = available.max(egui::vec2(1.0, 1.0));
    let width = available.x.min(available.y * aspect);
    egui::vec2(width, width / aspect)
}

fn normalize_inspected_ui_parameters(
    parameters: &[aexcompat_broker::image_render::InteractiveParameter],
) -> Vec<aexcompat_broker::image_render::InteractiveParameter> {
    parameters
        .iter()
        .cloned()
        .map(|mut parameter| {
            if matches!(parameter.kind.as_str(), "integer" | "float" | "path")
                && parameter.value.is_finite()
                && parameter.minimum.is_finite()
                && parameter.maximum.is_finite()
                && parameter.minimum <= parameter.maximum
            {
                parameter.value = parameter.value.clamp(parameter.minimum, parameter.maximum);
            }
            parameter
        })
        .collect()
}

fn parameters_for_native_action(
    parameters: &[aexcompat_broker::image_render::InteractiveParameter],
    defaults: &[aexcompat_broker::image_render::InteractiveParameter],
) -> Vec<aexcompat_broker::image_render::InteractiveParameter> {
    parameters
        .iter()
        .filter(|parameter| {
            if parameter.kind != "arbitrary_data" {
                return true;
            }
            let default_was_unsendable = defaults.iter().any(|default| {
                default.slot == parameter.slot
                    && default.kind == "arbitrary_data"
                    && default.debug_summary.is_none()
            });
            !(default_was_unsendable
                && parameter.debug_summary.as_deref().is_none_or(str::is_empty))
        })
        .cloned()
        .collect()
}

fn canonical_runtime_dependency_root(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("Runtime dependency folder must be an absolute path.".into());
    }
    let canonical = canonical_deverbatim(path)
        .map_err(|error| format!("Runtime dependency folder is unavailable: {error}"))?;
    if !canonical.is_dir() {
        return Err("Runtime dependency folder is not a directory.".into());
    }
    let text = canonical
        .to_str()
        .ok_or("Runtime dependency folder must be UTF-8.")?;
    if text.contains(';') {
        return Err("Runtime dependency folder must not contain ';'.".into());
    }
    Ok(canonical)
}

fn same_windows_path(left: &Path, right: &Path) -> bool {
    left.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

fn approved_dependency_search_dirs(
    plugin_path: &Path,
    dependencies: &[aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact],
    runtime_roots: &[PathBuf],
) -> Result<Vec<PathBuf>, String> {
    let plugin_parent = plugin_path
        .parent()
        .ok_or("Selected AEX has no parent folder.")?;
    let mut roots = vec![plugin_parent.to_path_buf()];
    for dependency in dependencies {
        let parent = dependency
            .path
            .parent()
            .ok_or("Approved dependency has no parent folder.")?;
        if !roots.iter().any(|root| same_windows_path(root, parent)) {
            roots.push(parent.to_path_buf());
        }
    }
    for root in runtime_roots {
        if !roots.iter().any(|seen| same_windows_path(seen, root)) {
            roots.push(root.clone());
        }
    }
    if roots.len() > 16 {
        return Err("At most 16 dependency search folders may be approved.".into());
    }
    Ok(roots)
}

fn single_supported_dropped_path(dropped: &[egui::DroppedFile]) -> Option<PathBuf> {
    if dropped.len() != 1 {
        return None;
    }
    dropped[0]
        .path
        .as_ref()
        .filter(|path| is_supported_input_image(path))
        .cloned()
}

struct HarnessApp {
    ui_kit: AexUiKit,
    repository: PathBuf,
    selection: Option<Selection>,
    session_approved: bool,
    dependencies: Vec<SessionDependency>,
    approved_dependencies: Vec<aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact>,
    runtime_dependency_roots: Vec<PathBuf>,
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
    show_analysis_panel: bool,
    analysis_panel_width: f32,
    pixel_comparison: Option<Result<PixelComparison, String>>,
    parameters: Vec<aexcompat_broker::image_render::InteractiveParameter>,
    parameter_defaults: Vec<aexcompat_broker::image_render::InteractiveParameter>,
    parameter_inspection_state: ParameterInspectionState,
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
            ui_kit: AexUiKit::default(),
            repository,
            selection: None,
            session_approved: false,
            dependencies: Vec::new(),
            approved_dependencies: Vec::new(),
            runtime_dependency_roots: Vec::new(),
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
            show_analysis_panel: true,
            analysis_panel_width: 380.0,
            pixel_comparison: None,
            parameters: Vec::new(),
            parameter_defaults: Vec::new(),
            parameter_inspection_state: ParameterInspectionState::NotSelected,
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
            status: "Select an AEX file. Effect Controls inspection runs in an isolated worker."
                .into(),
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

    fn clear_render_output(&mut self) {
        self.output_image = None;
        self.preview = None;
        self.pixel_comparison = None;
    }

    fn invalidate_render_output(&mut self) {
        self.clear_render_output();
        self.viewer_mode = 0;
    }

    fn close_selected_aex(&mut self) {
        self.close_live_session();
        self.selection = None;
        self.session_approved = false;
        self.dependencies.clear();
        self.approved_dependencies.clear();
        self.runtime_dependency_roots.clear();
        self.approval_check = false;
        self.trust_rebuilds = false;
        self.selection_stale = false;
        self.diagnostic_history = DiagnosticHistory::default();
        self.diagnostic_warning = None;
        self.preflight_warnings.clear();
        self.parameters.clear();
        self.parameter_defaults.clear();
        self.parameter_inspection_state = ParameterInspectionState::NotSelected;
        self.audio_input = None;
        self.audio_effect_only = false;
        self.smart_render = false;
        self.smart_render_advertised = None;
        self.smart_render_capability = None;
        self.smart_render_manual_override = false;
        self.host_context = None;
        self.inspect_after_refresh = false;
        self.pending_parameter_slot = None;
        self.pending_live_render = false;
        self.live_render_due = None;
        self.render_after_parameter_change = false;
        self.render_diagnostics = None;
        self.failure_diagnostics = None;
        self.matrix_results.clear();
        self.invalidate_render_output();
        self.status =
            "Select an AEX file. Effect Controls inspection runs in an isolated worker.".into();
        self.report.clear();
    }

    fn current_render_input_fingerprint(&self) -> String {
        render_input_fingerprint(
            &self.parameters,
            self.smart_render,
            self.pixel_format,
            self.gpu_backend,
            self.frame,
            self.duration_frames,
            self.frames_per_second,
            self.frame_time_step,
            self.host_context.as_ref(),
            self.audio_input.as_deref(),
            self.apply_custom_ui_click_to_render,
            self.apply_custom_ui_draw_to_render,
            self.custom_ui_click_point,
            self.custom_ui_click_color,
        )
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
        let unresolved = self
            .preflight_warnings
            .iter()
            .map(|warning| warning.basename.clone())
            .collect::<Vec<_>>();
        self.runtime_dependency_roots =
            aexcompat_broker::installed_runtime_roots::matching_registered_runtime_roots(
                &unresolved,
            )
            .into_iter()
            .filter_map(|path| canonical_runtime_dependency_root(&path).ok())
            .take(15)
            .collect();
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let identity = DispatchIdentity {
            sha256: selection.sha256.clone(),
            size: selection.size,
        };
        {
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
                    let input_clicked = ui
                        .add_enabled_ui(self.input_preview.is_some(), |ui| {
                            self.ui_kit.tab_button(ui, "Input", self.viewer_mode == 0)
                        })
                        .inner
                        .clicked();
                    if input_clicked {
                        self.viewer_mode = 0;
                    }
                    let output_clicked = ui
                        .add_enabled_ui(self.preview.is_some(), |ui| {
                            self.ui_kit
                                .tab_button(ui, "AEX output", self.viewer_mode == 1)
                        })
                        .inner
                        .clicked();
                    if output_clicked {
                        self.viewer_mode = 1;
                    }
                    let compare_clicked = ui
                        .add_enabled_ui(
                            self.input_preview.is_some() && self.preview.is_some(),
                            |ui| self.ui_kit.tab_button(ui, "Compare", self.viewer_mode == 2),
                        )
                        .inner
                        .clicked();
                    if compare_clicked {
                        self.viewer_mode = 2;
                    }
                    ui.separator();
                    ui.label("FHD canvas / aspect-fit");
                    ui.separator();
                    ui.monospace(format!("{:.0}%", self.viewer_zoom * 100.0));
                    let active_view_available = match self.viewer_mode {
                        0 => self.input_preview.is_some(),
                        1 => self.preview.is_some(),
                        _ => self.input_preview.is_some() && self.preview.is_some(),
                    };
                    if self
                        .ui_kit
                        .compact_button(ui, "Fit", active_view_available)
                        .clicked()
                    {
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
        self.close_selected_aex();
        self.choose_aex();
    }

    fn show_workspace_viewer(&mut self, ui: &mut egui::Ui) {
        fixed_column_header(ui, |ui| {
            ui.horizontal(|ui| {
                if self
                    .ui_kit
                    .tab_button(
                        ui,
                        self.ui_kit.text("VIEW", "ビュー"),
                        self.viewer_mode != 2,
                    )
                    .clicked()
                {
                    self.viewer_mode = if self.preview.is_some() { 1 } else { 0 };
                }
                let compare_clicked = ui
                    .add_enabled_ui(
                        self.input_preview.is_some() && self.preview.is_some(),
                        |ui| {
                            self.ui_kit.tab_button(
                                ui,
                                self.ui_kit.text("COMPARE", "比較"),
                                self.viewer_mode == 2,
                            )
                        },
                    )
                    .inner
                    .clicked();
                if compare_clicked {
                    self.viewer_mode = 2;
                }
            });
            ui.horizontal(|ui| {
                let label = match self.viewer_mode {
                    0 => self.input_preview.as_ref().map(|texture| texture.size()),
                    1 => self.preview.as_ref().map(|texture| texture.size()),
                    _ => None,
                };
                if let Some([width, height]) = label {
                    ui.monospace(format!("{width} x {height}"));
                } else {
                    ui.weak(
                        self.ui_kit
                            .text("FHD workspace / aspect fit", "FHD表示 / 比率を維持"),
                    );
                }
                if self.rendering {
                    ui.spinner();
                    ui.weak(self.ui_kit.text("Rendering...", "レンダー中..."));
                }
            });
        });
        ui.separator();

        let toolbar_height = 42.0;
        let viewer_available = egui::vec2(
            ui.available_width(),
            (ui.available_height() - toolbar_height).max(1.0),
        );
        let viewer_size = fit_size_to_aspect(viewer_available, 16.0 / 9.0);
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - viewer_size.x) * 0.5).max(0.0));
            ui.allocate_ui_with_layout(
                viewer_size,
                egui::Layout::top_down(egui::Align::Center),
                |ui| match self.viewer_mode {
                0 if self.input_preview.is_some() => show_viewer_texture(
                    ui,
                    "Input",
                    self.input_preview.as_ref(),
                    &mut self.viewer_zoom,
                    &mut self.viewer_pan,
                ),
                0 => {
                    let ui_kit = self.ui_kit.clone();
                    ui.centered_and_justified(|ui| {
                        ui_kit.onboarding_card(ui, |ui| {
                                ui.set_max_width(560.0);
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new(ui_kit.text(
                                            "Start a render workspace",
                                            "レンダーワークスペースを開始",
                                        ))
                                            .size(24.0)
                                            .strong(),
                                    );
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new(
                                            ui_kit.text(
                                                "Choose an After Effects plug-in and an input image. AEXCompat will load the effect controls before any native render runs.",
                                                "After Effectsプラグインと入力画像を選択してください。ネイティブレンダーの前にエフェクトコントロールを読み込みます。",
                                            ),
                                        )
                                        .color(ui_kit.muted_foreground()),
                                    );
                                    ui.add_space(20.0);
                                    ui.horizontal(|ui| {
                                        if ui_kit
                                            .primary_button(
                                                ui,
                                                ui_kit.text("1  Choose AEX...", "1  AEXを選択..."),
                                                !self.busy,
                                            )
                                            .clicked()
                                        {
                                            self.reset_and_choose_aex();
                                        }
                                        if ui_kit
                                            .secondary_button(
                                                ui,
                                                ui_kit.text(
                                                    "2  Choose image...",
                                                    "2  画像を選択...",
                                                ),
                                                !self.busy,
                                            )
                                            .clicked()
                                        {
                                            self.choose_input(ui.ctx());
                                        }
                                    });
                                    ui.add_space(12.0);
                                    ui.weak(ui_kit.text(
                                        "Selecting an AEX scans its identity and dependencies, then inspects Effect Controls in an isolated worker. Rendering stays disabled until both inputs are ready.",
                                        "AEX選択後、識別情報と依存関係を確認し、隔離ワーカーでエフェクトコントロールを検査します。両方の入力が揃うまでレンダーは無効です。",
                                    ));
                                });
                            });
                    });
                }
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
                            if self.rendering {
                                ui.spinner();
                                ui.label(
                                    self.ui_kit.text(
                                        "Rendering AEX output...",
                                        "AEX出力をレンダーしています...",
                                    ),
                                );
                            } else {
                                ui.colored_label(
                                    Color32::from_rgb(225, 155, 65),
                                    RichText::new(self.ui_kit.text(
                                        "AEX output is not available",
                                        "AEX出力はまだありません",
                                    ))
                                    .strong(),
                                );
                                ui.label(self.ui_kit.status_text(&self.status));
                                if let Some(summary) = visible_report_summary(&self.report) {
                                    ui.monospace(summary);
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
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.monospace(format!("{:.0}%", self.viewer_zoom * 100.0));
            if self.viewer_mode != 2 {
                let showing_before = self.viewer_mode == 0;
                let eye = self
                    .ui_kit
                    .eye_button(
                        ui,
                        !showing_before,
                        self.preview.is_some(),
                        self.ui_kit.text(
                            if showing_before {
                                "Show effect result"
                            } else {
                                "Show before effect"
                            },
                            if showing_before {
                                "エフェクト適用後を表示"
                            } else {
                                "エフェクト適用前を表示"
                            },
                        ),
                    )
                    .on_hover_text(self.ui_kit.text(
                        if showing_before {
                            "Show effect result"
                        } else {
                            "Show before effect"
                        },
                        if showing_before {
                            "エフェクト適用後を表示"
                        } else {
                            "エフェクト適用前を表示"
                        },
                    ));
                if eye.clicked() {
                    self.viewer_mode = if showing_before && self.preview.is_some() {
                        1
                    } else {
                        0
                    };
                }
            }
            let active_view_available = match self.viewer_mode {
                0 => self.input_preview.is_some(),
                1 => self.preview.is_some(),
                _ => self.input_preview.is_some() && self.preview.is_some(),
            };
            if self
                .ui_kit
                .compact_button(
                    ui,
                    self.ui_kit.text("Fit", "全体表示"),
                    active_view_available,
                )
                .clicked()
            {
                self.viewer_zoom = 1.0;
                self.viewer_pan = egui::Vec2::ZERO;
            }
            if self
                .ui_kit
                .compact_button(
                    ui,
                    self.ui_kit.text("Pop out", "別ウィンドウ"),
                    self.input_preview.is_some() || self.preview.is_some(),
                )
                .clicked()
            {
                self.viewer_open = true;
            }
        });
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
        let diagnostic_eligible = true;
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

    fn invalidate_effect_controls_for_dependency_change(&mut self) {
        self.close_live_session();
        self.parameters.clear();
        self.parameter_defaults.clear();
        self.parameter_inspection_state = if self.selection.is_some() {
            ParameterInspectionState::Failed
        } else {
            ParameterInspectionState::NotSelected
        };
        self.audio_effect_only = false;
        self.smart_render = false;
        self.smart_render_advertised = None;
        self.smart_render_capability = None;
        self.smart_render_manual_override = false;
        self.pending_parameter_slot = None;
        self.pending_live_render = false;
        self.live_render_due = None;
        self.render_after_parameter_change = false;
        self.invalidate_render_output();
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
        self.load_input_path(ctx, path);
    }

    fn load_input_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        match load_preview(ctx, "input", &path) {
            Ok(preview) => {
                self.input_image = Some(path);
                self.input_preview = Some(preview);
                self.invalidate_render_output();
                self.status = "Input image loaded. Ready to render.".into();
                self.start_live_render_if_ready();
            }
            Err(error) => {
                self.status = "Input image could not be decoded.".into();
                self.report = error;
            }
        }
    }

    fn accept_dropped_input(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|input| input.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        if self.busy {
            self.status = "Image drop ignored while a native task is running.".into();
            return;
        }
        let Some(path) = single_supported_dropped_path(&dropped) else {
            self.status = "Drop exactly one supported image file.".into();
            return;
        };
        self.load_input_path(ctx, path);
    }

    fn start_live_render_if_ready(&mut self) {
        if self.live_render
            && !self.audio_effect_only
            && render_action_enabled(
                self.busy,
                self.selection.is_some(),
                self.input_image.is_some(),
                self.session_approved,
                self.selection_stale,
                self.smart_render_capability.is_some(),
            )
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
                let identity_changed = hash != previous_hash;
                if identity_changed {
                    // Do not keep a worker holding the previous build alive.
                    self.close_live_session();
                }
                self.selection = Some(Selection {
                    path: path.clone(),
                    size: bytes.len() as u64,
                    sha256: hash,
                    modified: metadata.and_then(|value| value.modified().ok()),
                });
                self.selection_stale = false;
                self.parameters.clear();
                self.parameter_defaults.clear();
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
                    self.inspect_after_refresh = true;
                    self.parameter_inspection_state = ParameterInspectionState::Loading;
                    self.status = "AEX identity is unchanged; reloading Effect Controls.".into();
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
        let plugin_size = selection.size;
        let dependency_search_dirs = match approved_dependency_search_dirs(
            &plugin_path,
            &self.approved_dependencies,
            &self.runtime_dependency_roots,
        ) {
            Ok(roots) => roots,
            Err(error) => {
                self.invalidate_effect_controls_for_dependency_change();
                self.status = "Effect Controls inspection has invalid dependency folders.".into();
                self.report = error;
                return;
            }
        };
        self.status = "Loading Effect Controls...".into();
        self.parameter_inspection_state = ParameterInspectionState::Loading;
        self.spawn_native("inspect_parameters", move || {
            let expected_sha256 = decode_sha256(&hash)?;
            let (parameters, diagnostics) =
                aexcompat_broker::image_render::inspect_experimental_via_discovery_in_place(
                    &repository,
                    aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact {
                        path: plugin_path,
                        expected_sha256,
                        expected_size: plugin_size,
                    },
                    dependency_search_dirs,
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
        let parameters = parameters_for_native_action(&self.parameters, &self.parameter_defaults);
        match aexcompat_broker::image_render::probe_experimental_custom_ui_cursor(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &parameters,
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
        let parameters = parameters_for_native_action(&self.parameters, &self.parameter_defaults);
        match aexcompat_broker::image_render::probe_experimental_custom_ui_draw(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &parameters,
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
        let parameters = parameters_for_native_action(&self.parameters, &self.parameter_defaults);
        match aexcompat_broker::image_render::probe_experimental_custom_ui_lifecycle(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &parameters,
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
        let parameters = parameters_for_native_action(&self.parameters, &self.parameter_defaults);
        match aexcompat_broker::image_render::probe_experimental_custom_ui_idle(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &parameters,
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
        let parameters = parameters_for_native_action(&self.parameters, &self.parameter_defaults);
        match aexcompat_broker::image_render::probe_experimental_custom_ui_keydown(
            &self.repository,
            &selection.path,
            &selection.sha256,
            self.custom_ui_click_point,
            self.custom_ui_keycode,
            self.custom_ui_key_modifiers,
            &parameters,
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
        let parameters = parameters_for_native_action(&self.parameters, &self.parameter_defaults);
        match aexcompat_broker::image_render::probe_experimental_custom_ui_mouse_exited(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &parameters,
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
        let parameters = parameters_for_native_action(&self.parameters, &self.parameter_defaults);
        match aexcompat_broker::image_render::probe_experimental_custom_ui_click(
            &self.repository,
            &selection.path,
            &selection.sha256,
            self.custom_ui_click_point,
            self.custom_ui_click_color,
            &parameters,
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
        let parameters = parameters_for_native_action(&self.parameters, &self.parameter_defaults);
        match aexcompat_broker::image_render::probe_experimental_custom_ui_drag(
            &self.repository,
            &selection.path,
            &selection.sha256,
            self.custom_ui_click_point,
            self.custom_ui_drag_end,
            self.custom_ui_drag_steps,
            &parameters,
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
        if !self.session_approved || self.selection_stale {
            self.status =
                "Render blocked: the selected AEX session is not approved or is stale.".into();
            self.report =
                "reload Effect Controls and approve the current AEX identity before rendering"
                    .into();
            return;
        }
        let Some((plugin_path, hash, selection_size)) = self.selection.as_ref().map(|selection| {
            (
                selection.path.clone(),
                selection.sha256.clone(),
                selection.size,
            )
        }) else {
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
        // A render attempt owns the output state. Once inputs or controls
        // request a new frame, the previous frame is no longer authoritative.
        let preserve_compare = self.viewer_mode == 2 && self.input_preview.is_some();
        self.clear_render_output();
        self.viewer_mode = if preserve_compare { 2 } else { 1 };
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
        let worker =
            required_render_worker_path(&self.repository, interactive_selection.path.is_smart());
        if !worker.is_file() {
            self.status = "Required render worker is missing or unreadable.".into();
            self.report = serde_json::json!({
                "passed": false,
                "error": "required render worker is missing or unreadable",
                "expected_worker": worker,
                "render_path": if interactive_selection.path.is_smart() { "smartfx" } else { "classic" },
                "stage": "ui_render_preflight",
            })
            .to_string();
            return;
        }
        let repository = self.repository.clone();
        let parameters = parameters_for_native_action(&self.parameters, &self.parameter_defaults);
        let host_context = self.host_context.clone();
        let smart = interactive_selection.path.is_smart();
        let pixel_format = self.pixel_format;
        let gpu_backend = self.gpu_backend;
        let audio_sidecar = self.audio_input.clone();
        let dependencies = self.approved_dependencies.clone();
        let dependency_search_dirs = match approved_dependency_search_dirs(
            &plugin_path,
            &dependencies,
            &self.runtime_dependency_roots,
        ) {
            Ok(roots) => roots,
            Err(error) => {
                self.status = "Render blocked: dependency folders are invalid.".into();
                self.report = error;
                return;
            }
        };
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
        if audio_sidecar.is_some()
            && (!dependencies.is_empty() || !self.runtime_dependency_roots.is_empty())
        {
            self.status =
                "Dependency DLLs or runtime folders are not supported by the audio-sidecar render path.".into();
            self.report =
                "Remove dependencies and runtime folders or disable the audio sidecar before rendering.".into();
            return;
        }
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
                && !parameters.iter().any(|parameter| parameter.kind == "layer");
            if live_eligible {
                let identity = DispatchIdentity {
                    sha256: hash.clone(),
                    size: selection_size,
                };
                let diagnostic_eligible = self.selection.is_some();
                let (respond, receiver) = mpsc::channel();
                let request = LiveRenderRequest {
                    repository,
                    plugin_path,
                    plugin_sha256: hash,
                    dependencies,
                    dependency_search_dirs,
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
            } else {
                aexcompat_broker::image_render::render_experimental_image_with_approved_dependencies_and_search_dirs(
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
                    dependency_search_dirs,
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
        let parameters = parameters_for_native_action(&self.parameters, &self.parameter_defaults);
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
        let parameters = parameters_for_native_action(&self.parameters, &self.parameter_defaults);
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
        let parameters = parameters_for_native_action(&self.parameters, &self.parameter_defaults);
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
        fixed_column_header(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(
                    RichText::new(
                        self.ui_kit
                            .text("Effect Controls", "エフェクトコントロール"),
                    )
                    .size(20.0),
                );
                if self.busy && self.task_kind == TaskKind::InspectParameters {
                    ui.spinner();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self
                        .ui_kit
                        .compact_button(
                            ui,
                            self.ui_kit.text("Reset", "リセット"),
                            !self.busy && !self.parameter_defaults.is_empty(),
                        )
                        .clicked()
                    {
                        self.parameters = self.parameter_defaults.clone();
                        self.clear_render_output();
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
                ui.label(self.ui_kit.text(
                    "Select an AEX to load its parameters.",
                    "AEXを選択するとパラメーターを読み込みます。",
                ));
            }
        });
        ui.separator();
        if self.busy && self.task_kind == TaskKind::InspectParameters {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    self.ui_kit
                        .text("Loading parameters...", "パラメーターを読み込んでいます..."),
                );
            });
        } else if let Some((english, japanese)) =
            parameter_inspection_message(self.parameter_inspection_state)
        {
            ui.label(self.ui_kit.text(english, japanese));
            if ui
                .small_button(self.ui_kit.text("Reload controls", "再読込"))
                .clicked()
            {
                self.inspect_parameters_async();
            }
        }

        let mut clicked_button = None;
        let mut controls_changed = false;
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
                                if ui
                                    .small_button(self.ui_kit.text("Choose image", "画像を選択"))
                                    .clicked()
                                {
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
                                        .unwrap_or(self.ui_kit.text("Not connected", "未接続")),
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
                            self.ui_kit.modern_slider(
                                ui,
                                &mut parameter.value,
                                parameter.minimum..=parameter.maximum,
                                &parameter.name,
                            );
                        }
                    });
                    let changed = parameter.value != previous_value
                        || parameter.color != previous_color
                        || parameter.components != previous_components
                        || parameter.layer_path != previous_layer
                        || parameter.debug_summary != previous_summary;
                    if changed {
                        controls_changed = true;
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
            controls_changed = true;
            self.pending_parameter_slot = Some(slot);
            self.pending_live_render = self.live_render;
            self.live_render_due = Some(Instant::now() + std::time::Duration::from_millis(500));
        }
        if controls_changed {
            self.clear_render_output();
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
        let result = self.receiver.as_ref().and_then(receive_task_result);
        let Some(mut result) = result else {
            if self.busy {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            return;
        };
        let prepared_preview = match prepare_render_preview(
            result.success,
            result.operation.as_deref(),
            result.output.as_deref(),
        ) {
            Ok(preview) => preview,
            Err(error) => {
                result.success = false;
                result.output = None;
                result.body = report_ui_output_failure(&result.body, &error);
                None
            }
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
            native_failure_status(result.operation.as_deref(), &result.body)
        }
        .into();
        let mut inspect_selected_aex = false;
        if task_kind == TaskKind::IdentifyAex && result.success {
            let mut lines = result.body.lines();
            if let (Some(path), Some(size), Some(hash)) = (lines.next(), lines.next(), lines.next())
            {
                // A newly selected AEX replaces whatever the resident worker
                // was opened for.
                self.close_live_session();
                self.selection = Some(Selection {
                    path: path.into(),
                    size: size.parse().unwrap_or(0),
                    sha256: hash.into(),
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
                    let parameters = serde_json::from_value::<
                        Vec<aexcompat_broker::image_render::InteractiveParameter>,
                    >(report["parameters"].clone())
                    .map_err(|error| format!("inspection parameters are invalid: {error}"))?;
                    let audio_effect_only = report["worker_diagnostics"]["audio_effect_only"]
                        .as_bool()
                        .unwrap_or(false);
                    let inspection_state = parameter_inspection_state(&report, &parameters)?;
                    Ok((capability, parameters, audio_effect_only, inspection_state))
                })();
                match accepted {
                    Ok((capability, parameters, audio_effect_only, inspection_state)) => {
                        self.parameters = normalize_inspected_ui_parameters(&parameters);
                        self.parameter_defaults = self.parameters.clone();
                        self.parameter_inspection_state = inspection_state;
                        self.audio_effect_only = audio_effect_only;
                        self.smart_render = capability.smart_render_advertised;
                        self.smart_render_advertised = Some(capability.smart_render_advertised);
                        self.smart_render_capability = Some(capability);
                        effect_controls_ready = true;
                        self.status = parameter_inspection_status(
                            inspection_state,
                            &self.parameters,
                            self.smart_render,
                        );
                    }
                    Err(error) => {
                        self.parameters.clear();
                        self.parameter_defaults.clear();
                        self.parameter_inspection_state = ParameterInspectionState::Failed;
                        self.audio_effect_only = false;
                        self.status = "Effect Controls capability inspection failed safely; rendering is blocked.".into();
                        inspection_blocker = Some(error);
                    }
                }
            } else {
                self.parameters.clear();
                self.parameter_defaults.clear();
                self.parameter_inspection_state = ParameterInspectionState::Failed;
                self.audio_effect_only = false;
            }
        }
        if let Some((output, image)) = prepared_preview {
            self.render_diagnostics = serde_json::from_str(&result.body)
                .ok()
                .as_ref()
                .and_then(render_diagnostics);
            self.output_image = Some(output.clone());
            self.preview = Some(ctx.load_texture("output", image, egui::TextureOptions::LINEAR));
            self.viewer_mode = if self.viewer_mode == 2 && self.input_preview.is_some() {
                2
            } else {
                1
            };
            self.status = "AEX output ready.".into();
            self.refresh_pixel_comparison();
        } else if result.operation.as_deref() == Some("render_image") {
            self.output_image = None;
            self.preview = None;
            self.pixel_comparison = None;
            self.viewer_mode = 1;
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
        self.ui_kit.install(ctx);
        self.accept_dropped_input(ctx);
        self.poll(ctx);
        self.dispatch_pending_parameter_change(ctx);
        self.check_selected_identity();
        if self.inspect_after_refresh && !self.busy {
            self.inspect_after_refresh = false;
            self.inspect_parameters_async();
        }
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self
                        .ui_kit
                        .segmented_toggle(
                            ui,
                            "language-toggle",
                            self.ui_kit.language_is(UiLanguage::Japanese),
                            "EN",
                            "JA",
                        )
                        .clicked()
                    {
                        let language = if self.ui_kit.language_is(UiLanguage::Japanese) {
                            UiLanguage::English
                        } else {
                            UiLanguage::Japanese
                        };
                        self.ui_kit.set_language(language);
                    }
                    if self
                        .ui_kit
                        .segmented_toggle(
                            ui,
                            "color-mode-toggle",
                            !self.ui_kit.dark_mode,
                            "Dark",
                            "Light",
                        )
                        .clicked()
                    {
                        self.ui_kit.toggle_color_mode();
                    }
                    if self.busy {
                        ui.spinner();
                    }
                });
            });
            ui.separator();
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                let aex_ready = self.selection.is_some();
                let image_ready = self.input_image.is_some();
                let output_ready = self.preview.is_some();
                let aex_label = if aex_ready {
                    self.ui_kit.text("1  AEX  SELECTED", "1  AEX  選択済み")
                } else {
                    "1  AEX"
                };
                self.ui_kit.workflow_label(
                    ui,
                    aex_label,
                    if aex_ready {
                        self.ui_kit.success_foreground()
                    } else {
                        self.ui_kit.muted_foreground()
                    },
                );
                if self
                    .ui_kit
                    .compact_button(
                        ui,
                        self.ui_kit.text("Choose AEX...", "AEXを選択..."),
                        !self.busy,
                    )
                    .clicked()
                {
                    self.reset_and_choose_aex();
                }
                if self
                    .ui_kit
                    .compact_button(
                        ui,
                        self.ui_kit.text("Close", "閉じる"),
                        aex_ready && !self.busy,
                    )
                    .clicked()
                {
                    self.close_selected_aex();
                }
                ui.separator();
                let image_label = if image_ready {
                    self.ui_kit.text("2  INPUT  READY", "2  入力  準備完了")
                } else {
                    self.ui_kit.text("2  INPUT", "2  入力")
                };
                self.ui_kit.workflow_label(
                    ui,
                    image_label,
                    if image_ready {
                        self.ui_kit.success_foreground()
                    } else {
                        self.ui_kit.muted_foreground()
                    },
                );
                if self
                    .ui_kit
                    .compact_button(
                        ui,
                        self.ui_kit.text("Choose image...", "画像を選択..."),
                        !self.busy,
                    )
                    .clicked()
                {
                    self.choose_input(ctx);
                    self.viewer_mode = 0;
                }
                ui.separator();
                let output_label = if output_ready {
                    self.ui_kit.text("3  OUTPUT  READY", "3  出力  準備完了")
                } else {
                    self.ui_kit.text("3  RENDER", "3  レンダー")
                };
                self.ui_kit.workflow_label(
                    ui,
                    output_label,
                    if output_ready {
                        self.ui_kit.success_foreground()
                    } else {
                        self.ui_kit.muted_foreground()
                    },
                );
                let can_render = render_action_enabled(
                    self.busy,
                    self.selection.is_some(),
                    self.input_image.is_some(),
                    self.session_approved,
                    self.selection_stale,
                    self.smart_render_capability.is_some(),
                );
                if ui
                    .scope(|ui| {
                        self.ui_kit.compact_primary_button(
                            ui,
                            self.ui_kit.text("Render preview", "プレビューをレンダー"),
                            can_render,
                        )
                    })
                    .inner
                    .clicked()
                {
                    self.quick_render();
                }
                ui.separator();
                let live_render_changed = self
                    .ui_kit
                    .checkbox(
                        ui,
                        &mut self.live_render,
                        self.ui_kit.text("Auto Update", "自動更新"),
                        true,
                    )
                    .changed();
                if live_render_changed && !self.live_render {
                    self.pending_live_render = false;
                    if self.pending_parameter_slot.is_none() {
                        self.live_render_due = None;
                    }
                }
            });
            ui.add_space(8.0);
        });
        let render_input_before = self.current_render_input_fingerprint();
        let analysis_openness = ctx.animate_bool(
            egui::Id::new("analysis_and_logs_animation"),
            self.show_analysis_panel,
        );
        self.analysis_panel_width = clamp_analysis_panel_width(self.analysis_panel_width);
        let analysis_width =
            analysis_panel_display_width(self.analysis_panel_width, analysis_openness);
        let analysis_response = egui::SidePanel::left("analysis_and_logs")
            .exact_width(analysis_width)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(
                    ANALYSIS_PANEL_PADDING as i8,
                    0,
                )),
            )
            .show(ctx, |ui| {
                if analysis_openness > 0.12 {
                    fixed_column_header(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui.heading(self.ui_kit.text("Analysis & Logs", "解析・ログ"));
                    });
                    ui.label(self.ui_kit.text(
                        "Diagnostics, render settings and command output",
                        "診断、レンダー設定、コマンド出力",
                    ));
                        },
                    );
                    ui.separator();
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
            analysis_section_heading(
                ui,
                self.ui_kit.text(
                    "PROJECT / SESSION SETTINGS",
                    "プロジェクト／セッション設定",
                ),
                self.ui_kit.muted_foreground(),
            );
            if analysis_action_button(
                ui,
                !self.busy,
                self.ui_kit.text("Change AEX source...", "AEXを変更..."),
                self.ui_kit.text(
                    "Close the current plug-in session and choose another AEX.",
                    "現在のプラグインセッションを閉じ、別のAEXを選択します。",
                ),
            )
            .clicked()
            {
                self.reset_and_choose_aex();
            }
            if let Some(selected) = &self.selection {
                let selected_path = selected.path.display().to_string();
                let selected_size = selected.size;
                let selected_hash = selected.sha256.clone();
                ui.group(|ui| {
                    ui.label(RichText::new(selected_path).strong());
                    ui.collapsing(self.ui_kit.text("Binary details and dependency DLLs", "バイナリ詳細と依存DLL"), |ui| {
                    ui.label(format!("{} bytes", selected_size));
                    ui.monospace(&selected_hash);
                    if self.selection_stale {
                        ui.colored_label(Color32::from_rgb(210, 75, 55), "Build changed: native execution is paused until reload");
                    }
                    ui.separator();
                    ui.label(RichText::new(self.ui_kit.text("Local diagnostics", "ローカル診断")).strong());
                    ui.label(format!("Events for selected SHA: {}", self.diagnostic_history.count));
                    ui.label(format!("Latest: {}", self.diagnostic_history.latest.as_deref().unwrap_or("none")));
                    if let Some(warning) = &self.diagnostic_warning {
                        ui.colored_label(Color32::from_rgb(210, 145, 40), warning);
                    }
                    if analysis_action_button(
                        ui,
                        true,
                        self.ui_kit.text("Reload diagnostics", "診断を再読込"),
                        self.ui_kit.text(
                            "Reload saved diagnostics for the selected AEX identity.",
                            "選択中AEXの識別情報に紐づく保存済み診断を再読込します。",
                        ),
                    ).clicked() {
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
                    ui.label(RichText::new(self.ui_kit.text("Session dependency DLLs", "セッション依存DLL")).strong());
                    if self.dependencies.is_empty() {
                        ui.label(self.ui_kit.text("No additional DLLs selected.", "追加DLLは選択されていません。"));
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
                            if ui.add_enabled(!self.busy, egui::Button::new(self.ui_kit.text("Remove", "削除"))).clicked() {
                                remove = Some(index);
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.busy, egui::Button::new(self.ui_kit.text("Add DLL", "DLLを追加"))).clicked() {
                            self.add_dependency();
                        }
                        if ui.add_enabled(!self.busy && !self.dependencies.is_empty(), egui::Button::new(self.ui_kit.text("Clear all", "すべて消去"))).clicked() {
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
                    ui.label("The selected binary is hashed automatically and runs in a crash-contained worker. This is not a security sandbox.");
                    if analysis_action_button(ui, !self.busy, self.ui_kit.text("Reload rebuilt AEX", "再ビルドしたAEXを再読込"), self.ui_kit.text("Re-hash the selected file and rebuild its approved session.", "選択中ファイルを再ハッシュし、承認済みセッションを作り直します。")).clicked() {
                        self.refresh_aex();
                    }
                    });
                });
                if self.session_approved && !self.selection_stale {
                    ui.add_space(10.0);
                    if analysis_action_button(ui, !self.busy, self.ui_kit.text("Reload Effect Controls", "エフェクトコントロールを再読込"), self.ui_kit.text("Inspect the selected AEX controls again in an isolated worker.", "隔離ワーカーで選択中AEXのコントロールを再検査します。")).clicked() { self.inspect_parameters_async(); }
                    analysis_section_heading(
                        ui,
                        self.ui_kit.text("ADVANCED", "高度な機能"),
                        self.ui_kit.muted_foreground(),
                    );
                    ui.collapsing(self.ui_kit.text("Developer probes and diagnostics", "開発者向けプローブと診断"), |ui| {
                    ui.label(RichText::new(self.ui_kit.text("DEPENDENCIES", "依存関係")).small().strong());
                    ui.horizontal(|ui| {
                        if analysis_action_button(ui, !self.busy, self.ui_kit.text("Inspect all", "すべて検査"), self.ui_kit.text("Inspect every imported dependency in isolation.", "読み込まれるすべての依存DLLを隔離検査します。")).clicked() { self.inspect_external_dependencies(false); }
                        if analysis_action_button(ui, !self.busy, self.ui_kit.text("Inspect missing", "不足分を検査"), self.ui_kit.text("Inspect only dependencies that are currently unresolved.", "現在不足している依存DLLだけを検査します。")).clicked() { self.inspect_external_dependencies(true); }
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new(self.ui_kit.text("SEQUENCE STATE", "シーケンス状態")).small().strong());
                    if analysis_action_button(ui, !self.busy, self.ui_kit.text("Two-frame persistence", "2フレーム永続性"), self.ui_kit.text("Check whether sequence state survives across two frames.", "シーケンス状態が2フレーム間で維持されるか検査します。")).clicked() { self.probe_persistent_sequence(); }
                    if analysis_action_button(ui, !self.busy, self.ui_kit.text("Save / reload", "保存／再読込"), self.ui_kit.text("Round-trip flattened sequence data through save and reload.", "シーケンスデータを保存・再読込して復元性を検査します。")).clicked() { self.probe_flattened_sequence(); }
                    if analysis_action_button(ui, !self.busy, self.ui_kit.text("Non-destructive save", "非破壊保存"), self.ui_kit.text("Verify that saving a copy does not mutate the live sequence.", "コピー保存が実行中のシーケンスを変更しないか検査します。")).clicked() { self.probe_copied_flattened_sequence(); }
                    ui.add_space(8.0);
                    ui.label(RichText::new(self.ui_kit.text("RENDER CONTRACT", "レンダー契約")).small().strong());
                    if analysis_action_button(ui, !self.busy, self.ui_kit.text("Options dialog", "オプション画面"), self.ui_kit.text("Open the effect options dialog in the isolated worker.", "隔離ワーカーでエフェクトのオプション画面を検査します。")).clicked() { self.probe_options_dialog(); }
                    if analysis_action_button(ui, !self.busy, self.ui_kit.text("Automatic options", "自動オプション"), self.ui_kit.text("Exercise the automatic options-dialog selector path.", "自動オプションダイアログのセレクター経路を検査します。")).clicked() { self.probe_automatic_options_dialog(); }
                    if analysis_action_button(ui, !self.busy, self.ui_kit.text("Classic NOP_RENDER", "Classic NOP_RENDER"), self.ui_kit.text("Verify classic NOP_RENDER input passthrough.", "Classic経路のNOP_RENDER入力パススルーを検査します。")).clicked() { self.probe_nop_render(); }
                    if analysis_action_button(ui, !self.busy, self.ui_kit.text("SmartFX NOP_RENDER", "SmartFX NOP_RENDER"), self.ui_kit.text("Verify SmartFX NOP_RENDER input passthrough.", "SmartFX経路のNOP_RENDER入力パススルーを検査します。")).clicked() { self.probe_smart_nop_render(); }
                    if analysis_action_button(ui, !self.busy, self.ui_kit.text("Classic input write", "Classic入力書込"), self.ui_kit.text("Detect writes to the protected classic input buffer.", "保護されたClassic入力バッファーへの書込みを検査します。")).clicked() { self.probe_input_buffer_write(); }
                    if analysis_action_button(ui, !self.busy, self.ui_kit.text("SmartFX input write", "SmartFX入力書込"), self.ui_kit.text("Detect writes to the protected SmartFX input buffer.", "保護されたSmartFX入力バッファーへの書込みを検査します。")).clicked() { self.probe_smart_input_buffer_write(); }
                    if analysis_action_button(ui, !self.busy, self.ui_kit.text("FRAME_SETUP expand", "FRAME_SETUP拡張"), self.ui_kit.text("Probe an effect that requests a larger output frame.", "出力フレームの拡張要求を検査します。")).clicked() { self.probe_frame_resize(true); }
                    if analysis_action_button(ui, !self.busy, self.ui_kit.text("FRAME_SETUP shrink", "FRAME_SETUP縮小"), self.ui_kit.text("Probe an effect that requests a smaller output frame.", "出力フレームの縮小要求を検査します。")).clicked() { self.probe_frame_resize(false); }
                    ui.add_space(8.0);
                    ui.label(RichText::new(self.ui_kit.text("CUSTOM UI EVENTS", "カスタムUIイベント")).small().strong());
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events & 4 != 0)
                        && analysis_action_button(ui, !self.busy, self.ui_kit.text("Cursor query", "カーソル照会"), self.ui_kit.text("Ask the effect which cursor should be shown at the test point.", "指定位置で表示するカーソルをエフェクトへ照会します。")).clicked()
                    {
                        self.probe_custom_ui_cursor();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0)
                        && analysis_action_button(ui, !self.busy, self.ui_kit.text("Record draw", "描画を記録"), self.ui_kit.text("Capture the effect's custom-control draw commands.", "カスタムコントロールの描画命令を記録します。")).clicked()
                    {
                        self.probe_custom_ui_draw();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0) {
                        let changed = ui.checkbox(
                            &mut self.apply_custom_ui_draw_to_render,
                            self.ui_kit.text("Draw custom UI before each render", "各レンダー前にカスタムUIを描画"),
                        ).changed();
                        if changed && self.apply_custom_ui_draw_to_render {
                            self.apply_custom_ui_click_to_render = false;
                        }
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0)
                        && analysis_action_button(ui, !self.busy, self.ui_kit.text("Lifecycle", "ライフサイクル"), self.ui_kit.text("Run the custom UI open, draw, and close lifecycle.", "カスタムUIの開始・描画・終了を順に検査します。")).clicked()
                    {
                        self.probe_custom_ui_lifecycle();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0)
                        && analysis_action_button(ui, !self.busy, self.ui_kit.text("Dispatch idle", "アイドル送信"), self.ui_kit.text("Send one idle event to the custom UI.", "カスタムUIへアイドルイベントを1回送信します。")).clicked()
                    {
                        self.probe_custom_ui_idle();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0) {
                        ui.group(|ui| {
                            ui.label(RichText::new(self.ui_kit.text("Custom UI key event", "カスタムUIキーイベント")).strong());
                            ui.horizontal(|ui| {
                                ui.label(self.ui_kit.text("Keycode", "キーコード"));
                                ui.add(egui::DragValue::new(&mut self.custom_ui_keycode).range(0u32..=0xC000_FFFFu32));
                                ui.label(format!("0x{:08X}", self.custom_ui_keycode));
                                ui.label(self.ui_kit.text("Modifiers", "修飾キー"));
                                ui.add(egui::DragValue::new(&mut self.custom_ui_key_modifiers));
                                if analysis_action_button(ui, !self.busy, self.ui_kit.text("Dispatch key", "キーを送信"), self.ui_kit.text("Send this key event to the custom control.", "このキーイベントをカスタムコントロールへ送信します。")).clicked() {
                                    self.probe_custom_ui_keydown();
                                }
                            });
                            ui.label(self.ui_kit.text("Default is printable A. The custom UI click X/Y values are used as the screen point.", "初期値は入力可能なAです。カスタムUIクリックのX/Yを画面座標として使います。"));
                        });
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events & 4 != 0) {
                        ui.group(|ui| {
                            ui.label(RichText::new(self.ui_kit.text("Custom UI click", "カスタムUIクリック")).strong());
                            ui.horizontal(|ui| {
                                ui.label("X");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_click_point[0]).range(0..=8192));
                                ui.label("Y");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_click_point[1]).range(0..=8192));
                                ui.color_edit_button_rgba_unmultiplied(&mut self.custom_ui_click_color);
                                if analysis_action_button(ui, !self.busy, self.ui_kit.text("Dispatch click", "クリックを送信"), self.ui_kit.text("Send a click with the selected point and color.", "指定した位置と色でクリックを送信します。")).clicked() {
                                    self.probe_custom_ui_click();
                                }
                            });
                            let changed = ui.checkbox(
                                &mut self.apply_custom_ui_click_to_render,
                                self.ui_kit.text("Apply this click before each render", "各レンダー前にこのクリックを適用"),
                            ).changed();
                            if changed && self.apply_custom_ui_click_to_render {
                                self.apply_custom_ui_draw_to_render = false;
                            }
                        });
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events & 3 != 0) {
                        ui.group(|ui| {
                            ui.label(RichText::new(self.ui_kit.text("Comp / Layer custom UI drag", "コンポ／レイヤーのカスタムUIドラッグ")).strong());
                            ui.horizontal(|ui| {
                                ui.label(self.ui_kit.text("End X", "終了X"));
                                ui.add(egui::DragValue::new(&mut self.custom_ui_drag_end[0]).range(0..=8192));
                                ui.label(self.ui_kit.text("End Y", "終了Y"));
                                ui.add(egui::DragValue::new(&mut self.custom_ui_drag_end[1]).range(0..=8192));
                                ui.label(self.ui_kit.text("Steps", "ステップ数"));
                                ui.add(egui::DragValue::new(&mut self.custom_ui_drag_steps).range(1..=32));
                                if analysis_action_button(ui, !self.busy, self.ui_kit.text("Dispatch drag", "ドラッグを送信"), self.ui_kit.text("Send the configured multi-step drag gesture.", "設定した複数ステップのドラッグ操作を送信します。")).clicked() {
                                    self.probe_custom_ui_drag();
                                }
                            });
                            ui.label(self.ui_kit.text("The custom UI click X/Y values above are used as the drag start.", "上のクリックX/Yをドラッグ開始位置として使います。"));
                            if analysis_action_button(ui, !self.busy, self.ui_kit.text("Mouse exited", "マウス退出"), self.ui_kit.text("Notify the custom control that the pointer left its bounds.", "ポインターが領域外へ出たことを通知します。")).clicked() {
                                self.probe_custom_ui_mouse_exited();
                            }
                        });
                    }
                    ui.collapsing(self.ui_kit.text("AEGP diagnostics (advanced)", "AEGP診断（高度）"), |ui| {
                        if analysis_action_button(ui, !self.busy, self.ui_kit.text("Initialize", "初期化"), self.ui_kit.text("Initialize the selected binary through the AEGP entry path.", "選択中のバイナリをAEGP経路で初期化します。")).clicked() { self.initialize_aegp(); }
                        if analysis_action_button(ui, !self.busy, self.ui_kit.text("Update menu", "メニュー更新"), self.ui_kit.text("Dispatch one AEGP update-menu callback.", "AEGPメニュー更新コールバックを1回送信します。")).clicked() { self.update_aegp_menu(); }
                        if analysis_action_button(ui, !self.busy, self.ui_kit.text("Idle tick", "アイドル1回"), self.ui_kit.text("Dispatch one AEGP idle callback.", "AEGPアイドルコールバックを1回送信します。")).clicked() { self.dispatch_aegp_idle(); }
                        if analysis_action_button(ui, !self.busy, self.ui_kit.text("Command ON / OFF", "コマンドON／OFF"), self.ui_kit.text("Run an AEGP command enable/disable round trip.", "AEGPコマンドの有効・無効往復を検査します。")).clicked() { self.dispatch_aegp_command_roundtrip(); }
                        if analysis_action_button(ui, !self.busy, self.ui_kit.text("Active idle", "アクティブアイドル"), self.ui_kit.text("Run the active-item idle callback round trip.", "アクティブ項目のアイドル往復を検査します。")).clicked() { self.dispatch_aegp_active_idle_roundtrip(); }
                        if analysis_action_button(ui, !self.busy, self.ui_kit.text("Comp idle", "コンポアイドル"), self.ui_kit.text("Run the composition idle callback round trip.", "コンポジションのアイドル往復を検査します。")).clicked() { self.dispatch_aegp_comp_idle_roundtrip(); }
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
                                self.ui_kit.modern_slider(
                                    ui,
                                    &mut parameter.value,
                                    parameter.minimum..=parameter.maximum,
                                    &parameter.name,
                                );
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
                    analysis_section_heading(
                        ui,
                        self.ui_kit.text("RENDER SETTINGS", "レンダー設定"),
                        self.ui_kit.muted_foreground(),
                    );
                    analysis_setting_row(ui, self.ui_kit.text("Render path", "レンダー方式"), |ui| {
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
                                ui.colored_label(egui::Color32::from_rgb(230, 180, 60), "unsupported override: render blocked");
                            }
                        }
                    });
                    analysis_setting_row(ui, self.ui_kit.text("Pixel depth", "色深度"), |ui| {
                        use aexcompat_broker::image_render::RenderPixelFormat;
                        ui.selectable_value(&mut self.pixel_format, RenderPixelFormat::Argb8, "8 bpc");
                        ui.selectable_value(&mut self.pixel_format, RenderPixelFormat::Argb16, "16 bpc");
                        ui.selectable_value(&mut self.pixel_format, RenderPixelFormat::Argb32f, "32 bpc float");
                    });
                    analysis_setting_row(ui, self.ui_kit.text("GPU backend", "GPUバックエンド"), |ui| {
                        use aexcompat_broker::image_render::RenderGpuBackend;
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::Auto, "Auto");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::Cuda, "CUDA");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::OpenCl, "OpenCL");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::DirectX, "DirectX");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::Cpu, "CPU");
                    });
                    analysis_setting_row(ui, self.ui_kit.text("Frame / duration", "フレーム／長さ"), |ui| {
                        if ui.add(egui::DragValue::new(&mut self.frame).range(0..=10_000_000)).changed() {
                            self.duration_frames = self.duration_frames.max(self.frame.saturating_add(1));
                        }
                        ui.label(self.ui_kit.text("Duration", "長さ"));
                        ui.add(egui::DragValue::new(&mut self.duration_frames).range(self.frame.saturating_add(1)..=10_000_001));
                    });
                    analysis_setting_row(ui, self.ui_kit.text("Time base", "時間基準"), |ui| {
                        ui.add(egui::DragValue::new(&mut self.frames_per_second).range(1..=1_000_000));
                        ui.label(self.ui_kit.text("Frame step:", "フレーム間隔:"));
                        ui.add(egui::DragValue::new(&mut self.frame_time_step).range(1..=100_000));
                        ui.label(format!("{:.5} fps", self.frames_per_second as f64 / self.frame_time_step as f64));
                    });
                    analysis_setting_row(ui, self.ui_kit.text("Rate presets", "フレームレート"), |ui| {
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
                        if ui.checkbox(&mut requested, self.ui_kit.text("Spatial context", "空間設定")).changed() {
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
                                (self.ui_kit.text("Downsample X", "ダウンサンプルX"), &mut spatial.downsample_x),
                                (self.ui_kit.text("Downsample Y", "ダウンサンプルY"), &mut spatial.downsample_y),
                                (self.ui_kit.text("Pixel aspect", "ピクセル縦横比"), &mut spatial.pixel_aspect_ratio),
                            ] {
                                ui.label(label);
                                ui.add(egui::DragValue::new(&mut ratio.numerator).range(1..=1_000_000));
                                ui.label("/");
                                ui.add(egui::DragValue::new(&mut ratio.denominator).range(1..=1_000_000));
                            }
                        });
                        ui.horizontal(|ui| {
                            let mut explicit = spatial.full_resolution_width.is_some() && spatial.full_resolution_height.is_some();
                        if ui.checkbox(&mut explicit, self.ui_kit.text("Explicit full-resolution size", "フル解像度を指定")).changed() {
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
                        if ui.checkbox(&mut explicit, self.ui_kit.text("Pre-effect source origin", "エフェクト前の原点を指定")).changed() {
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
                        if ui.checkbox(&mut requested, self.ui_kit.text("Render environment", "レンダー環境")).changed() {
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
                            ui.label(self.ui_kit.text("Quality:", "品質:"));
                            ui.selectable_value(&mut environment.quality, RenderQuality::Low, self.ui_kit.text("Low", "低"));
                            ui.selectable_value(&mut environment.quality, RenderQuality::High, self.ui_kit.text("High", "高"));
                            ui.label(self.ui_kit.text("Field:", "フィールド:"));
                            ui.selectable_value(&mut environment.field, RenderField::Frame, self.ui_kit.text("Frame", "フレーム"));
                            ui.selectable_value(&mut environment.field, RenderField::Upper, self.ui_kit.text("Upper", "上"));
                            ui.selectable_value(&mut environment.field, RenderField::Lower, self.ui_kit.text("Lower", "下"));
                        });
                        ui.horizontal(|ui| {
                            ui.label(self.ui_kit.text("Shutter angle:", "シャッター角度:"));
                            ui.add(egui::DragValue::new(&mut environment.shutter_angle).speed(0.01).range(0.0..=1.0));
                            ui.label(self.ui_kit.text("Shutter phase:", "シャッターフェーズ:"));
                            ui.add(egui::DragValue::new(&mut environment.shutter_phase).speed(0.01).range(-1.0..=1.0));
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.busy && !self.parameters.is_empty(), egui::Button::new(self.ui_kit.text("Load debug request...", "デバッグ要求を読込..."))).clicked() { self.load_debug_request(); }
                        if ui.add_enabled(!self.busy && !self.parameters.is_empty(), egui::Button::new(self.ui_kit.text("Save debug request...", "デバッグ要求を保存..."))).clicked() { self.save_debug_request(); }
                    });
                    if let Some(mask_count) = self.host_context.as_ref().map(|context| context.mask_scene.masks.len()) {
                        ui.horizontal(|ui| {
                            ui.label(format!("Host mask context: {mask_count} mask(s)"));
                            if ui.add_enabled(!self.busy, egui::Button::new(self.ui_kit.text("Clear masks", "マスクを消去"))).clicked() {
                                if let Some(context) = &mut self.host_context {
                                    context.mask_scene.masks.clear();
                                    if context.spatial.is_none() && context.render_environment.is_none() { self.host_context = None; }
                                }
                            }
                        });
                    }
                    if self.audio_effect_only {
                        ui.colored_label(Color32::from_rgb(30, 120, 170), RichText::new(self.ui_kit.text("Audio-only Effect", "音声専用エフェクト")).strong());
                        ui.label(self.ui_kit.text("Transport: 44.1 kHz, mono, float32 little-endian raw samples", "転送形式: 44.1 kHz、モノラル、float32リトルエンディアンRAW"));
                        if ui.add_enabled(!self.busy, egui::Button::new(self.ui_kit.text("Change audio source (.f32)...", "音声ソースを変更（.f32）..."))).clicked() { self.choose_audio_input(); }
                        if let Some(path) = &self.audio_input { ui.monospace(path.display().to_string()); }
                        if ui.add_enabled(!self.busy && self.audio_input.is_some(), egui::Button::new(self.ui_kit.text("4. Render and save audio (.f32)...", "4 音声をレンダーして保存（.f32）..."))).clicked() { self.render_audio_and_save(); }
                    } else {
                        if ui.add_enabled(!self.busy, egui::Button::new(self.ui_kit.text("Change image source...", "画像ソースを変更..."))).clicked() { self.choose_input(ctx); }
                        if let Some(path) = &self.input_image { ui.monospace(path.display().to_string()); }
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!self.busy, egui::Button::new(self.ui_kit.text("Select visual audio sidecar (.f32, optional)", "音声サイドカーを選択（.f32、任意）"))).clicked() { self.choose_audio_input(); }
                            if self.audio_input.is_some() && ui.add_enabled(!self.busy, egui::Button::new(self.ui_kit.text("Clear sidecar", "サイドカーを解除"))).clicked() { self.audio_input = None; }
                        });
                        if let Some(path) = &self.audio_input { ui.monospace(format!("Audio sidecar: {}", path.display())); }
                        if self.audio_input.is_some() {
                            ui.label("Sidecar mode: classic ARGB8, mono float32 LE, 44.1 kHz");
                        }
                        if ui.add_enabled(!self.busy, egui::Button::new(self.ui_kit.text("Select AE reference output (optional)", "AE参照出力を選択（任意）"))).clicked() { self.choose_reference(ctx); }
                        if let Some(path) = &self.reference_image { ui.monospace(format!("Reference: {}", path.display())); }
                        let native_actions_ready = render_action_enabled(
                            self.busy,
                            self.selection.is_some(),
                            self.input_image.is_some(),
                            self.session_approved,
                            self.selection_stale,
                            self.smart_render_capability.is_some(),
                        );
                        ui.horizontal(|ui| {
                            if ui.add_enabled(native_actions_ready, egui::Button::new(self.ui_kit.text("Render current frame", "現在のフレームをレンダー"))).clicked() { self.quick_render(); }
                            if ui.add_enabled(native_actions_ready, egui::Button::new(self.ui_kit.text("Render and save PNG...", "レンダーしてPNG保存..."))).clicked() { self.render_and_save(); }
                            if ui.add_enabled(native_actions_ready, egui::Button::new(self.ui_kit.text("Run 6-case compatibility matrix", "6条件の互換性マトリクスを実行"))).clicked() { self.run_compatibility_matrix(); }
                        });
                    }
                }
            }
            analysis_section_heading(
                ui,
                self.ui_kit.text("ANALYSIS / LOG OUTPUT", "解析／ログ出力"),
                self.ui_kit.muted_foreground(),
            );
            ui.label(RichText::new(self.ui_kit.status_text(&self.status)).strong());
            if let Some(path) = &self.output_image { ui.monospace(format!("Output: {}", path.display())); }
            if self.input_preview.is_some() || self.preview.is_some() || self.reference_preview.is_some() {
                if ui.button(self.ui_kit.text("Open FHD image viewer", "FHD画像ビューアーを開く")).clicked() {
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
                }
            });
        if self.current_render_input_fingerprint() != render_input_before {
            self.clear_render_output();
        }
        let analysis_rect = analysis_response.response.rect;
        let rail_x = analysis_rect.right();
        egui::Area::new(egui::Id::new("analysis_and_logs_rail"))
            .fixed_pos(egui::pos2(rail_x - 8.0, analysis_rect.top()))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let (rail_rect, rail) = ui.allocate_exact_size(
                    egui::vec2(16.0, analysis_rect.height()),
                    egui::Sense::click_and_drag(),
                );
                let rail = rail.on_hover_text(self.ui_kit.text(
                    "Click to collapse or reopen. Drag to resize.",
                    "クリックで開閉、ドラッグで幅を変更します。",
                ));
                rail.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Checkbox,
                        true,
                        self.show_analysis_panel,
                        self.ui_kit
                            .text("Show Analysis and Logs panel", "解析・ログパネルを表示"),
                    )
                });
                let line_x = rail_rect.center().x;
                ui.painter().vline(
                    line_x,
                    rail_rect.y_range(),
                    egui::Stroke::new(1.0, self.ui_kit.separator_color()),
                );
                if self.show_analysis_panel && rail.dragged() {
                    let delta_x = ui.input(|input| input.pointer.delta().x);
                    self.analysis_panel_width =
                        resized_analysis_panel_width(self.analysis_panel_width, delta_x);
                    ctx.request_repaint();
                }
                if rail.clicked() {
                    self.show_analysis_panel = !self.show_analysis_panel;
                }
                let center = egui::pos2(line_x, rail_rect.top() + 16.0);
                let points = if self.show_analysis_panel {
                    vec![
                        egui::pos2(center.x + 3.5, center.y - 5.5),
                        egui::pos2(center.x - 3.5, center.y),
                        egui::pos2(center.x + 3.5, center.y + 5.5),
                    ]
                } else {
                    vec![
                        egui::pos2(center.x - 3.5, center.y - 5.5),
                        egui::pos2(center.x + 3.5, center.y),
                        egui::pos2(center.x - 3.5, center.y + 5.5),
                    ]
                };
                ui.painter().add(egui::Shape::convex_polygon(
                    points,
                    if rail.hovered() {
                        ui.visuals().text_color()
                    } else {
                        self.ui_kit.muted_foreground()
                    },
                    egui::Stroke::NONE,
                ));
            });
        egui::SidePanel::left("effect_controls")
            .default_width(340.0)
            .min_width(260.0)
            .max_width(460.0)
            .resizable(true)
            .show(ctx, |ui| self.show_effect_controls(ui));
        egui::CentralPanel::default().show(ctx, |ui| {
            self.show_workspace_viewer(ui);
        });
        self.show_image_viewer(ctx);
        if ctx.input(|input| !input.raw.hovered_files.is_empty()) {
            let rect = ctx.content_rect().shrink(18.0);
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("input-image-drop-target"),
            ));
            painter.rect(
                rect,
                12.0,
                self.ui_kit.theme.palette.background.gamma_multiply(0.92),
                egui::Stroke::new(2.0, self.ui_kit.theme.palette.primary),
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                self.ui_kit
                    .text("Drop one input image", "入力画像を1枚ドロップ"),
                egui::FontId::proportional(22.0),
                self.ui_kit.theme.palette.foreground,
            );
        }
    }
}
