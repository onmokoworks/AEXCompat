// The active stage uses the existing Effect Controls editor. Parked stages own
// complete editor snapshots, not just parameter values or an AEX basename.
const MAX_EFFECT_CHAIN_STAGES: usize = 8;

macro_rules! effect_editor_state {
    ($($field:ident: $ty:ty),+ $(,)?) => {
        #[derive(Clone)]
        struct EffectEditorState { $( $field: $ty, )+ }

        impl HarnessApp {
            fn capture_effect(&self) -> EffectEditorState {
                EffectEditorState { $( $field: self.$field.clone(), )+ }
            }

            fn restore_effect(&mut self, state: EffectEditorState) {
                $( self.$field = state.$field; )+
            }
        }
    };
}

effect_editor_state! {
    selection: Option<Selection>,
    session_approved: bool,
    dependencies: Vec<SessionDependency>,
    approved_dependencies: Vec<aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact>,
    runtime_dependency_roots: Vec<PathBuf>,
    approval_check: bool,
    trust_rebuilds: bool,
    selection_stale: bool,
    preflight_warnings: Vec<PreflightImportWarning>,
    diagnostic_history: DiagnosticHistory,
    diagnostic_warning: Option<String>,
    parameters: Vec<aexcompat_broker::image_render::InteractiveParameter>,
    parameter_defaults: Vec<aexcompat_broker::image_render::InteractiveParameter>,
    parameter_inspection_state: ParameterInspectionState,
    host_context: Option<aexcompat_broker::render_request::HostContext>,
    smart_render: bool,
    smart_render_advertised: Option<bool>,
    smart_render_capability: Option<InspectedRenderCapability>,
    smart_render_manual_override: bool,
    audio_input: Option<PathBuf>,
    audio_effect_only: bool,
    custom_ui_click_point: [u16; 2],
    custom_ui_click_color: [f32; 4],
    apply_custom_ui_click_to_render: bool,
    apply_custom_ui_draw_to_render: bool,
    custom_ui_drag_end: [u16; 2],
    custom_ui_drag_steps: u8,
    custom_ui_keycode: u32,
    custom_ui_key_modifiers: u16,
}

struct EffectChainSlot {
    enabled: bool,
    editor: EffectEditorState,
}

#[derive(Clone)]
struct ChainRenderStage {
    ordinal: usize,
    plugin: PathBuf,
    hash: String,
    parameters: Vec<aexcompat_broker::image_render::InteractiveParameter>,
    selection: aexcompat_broker::image_render::InteractiveSessionSelection,
    dependencies: Vec<aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact>,
    search_dirs: Vec<PathBuf>,
    host_context: Option<aexcompat_broker::render_request::HostContext>,
    custom_ui_action: Option<aexcompat_broker::image_render::RenderUiAction>,
}

impl HarnessApp {
    fn chain_edit_ready(&self) -> bool {
        !self.busy && self.pending_parameter_slot.is_none()
    }

    fn uses_chain_renderer(&self) -> bool {
        self.effect_chain.len() > 1 || self.effect_chain.first().is_some_and(|slot| !slot.enabled)
    }

    fn current_image_render_ready(&self) -> bool {
        if !self.chain_edit_ready() || self.input_image.is_none() {
            return false;
        }
        if self.uses_chain_renderer() {
            return self.chain_render_plan().is_ok();
        }
        !self.audio_effect_only
            && render_action_enabled(
                self.busy,
                self.selection.is_some(),
                self.input_image.is_some(),
                self.session_approved,
                self.selection_stale,
                self.smart_render_capability.is_some(),
            )
    }

    fn save_active_effect(&mut self) {
        if self.effect_chain.is_empty() {
            if self.selection.is_some() {
                self.effect_chain.push(EffectChainSlot {
                    enabled: true,
                    editor: self.capture_effect(),
                });
                self.active_effect = 0;
            }
        } else {
            self.effect_chain[self.active_effect].editor = self.capture_effect();
        }
    }

    fn invalidate_chain(&mut self) {
        self.close_live_session();
        self.pending_parameter_slot = None;
        self.render_after_parameter_change = false;
        self.pending_live_render = self.live_render;
        self.live_render_due = self
            .live_render
            .then(|| Instant::now() + Duration::from_millis(500));
        self.invalidate_render_output();
    }

    fn select_chain_effect(&mut self, index: usize) {
        if !self.chain_edit_ready()
            || index >= self.effect_chain.len()
            || index == self.active_effect
        {
            return;
        }
        self.save_active_effect();
        self.close_live_session();
        self.active_effect = index;
        self.restore_effect(self.effect_chain[index].editor.clone());
        self.pending_parameter_slot = None;
        self.render_after_parameter_change = false;
        self.last_identity_check = Instant::now() - Duration::from_secs(2);
        self.render_diagnostics = None;
        self.failure_diagnostics = None;
        self.matrix_results.clear();
        self.report.clear();
        self.status = format!(
            "Editing effect {}. Render preview applies the whole chain.",
            index + 1
        );
    }

    fn add_chain_effect(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("After Effects plug-in", &["aex"])
            .pick_file()
        else {
            return;
        };
        self.add_chain_effect_path(path);
    }

    fn add_chain_effect_path(&mut self, path: PathBuf) {
        if !self.chain_edit_ready() {
            return;
        }
        self.save_active_effect();
        if self.effect_chain.len() >= MAX_EFFECT_CHAIN_STAGES {
            self.status = format!("At most {MAX_EFFECT_CHAIN_STAGES} effects can be chained.");
            return;
        }
        self.close_selected_aex();
        self.active_effect = self.effect_chain.len();
        self.effect_chain.push(EffectChainSlot {
            enabled: true,
            editor: self.capture_effect(),
        });
        self.choose_aex_path(path);
    }

    fn remove_chain_effect(&mut self) {
        if !self.chain_edit_ready() {
            return;
        }
        self.save_active_effect();
        if self.effect_chain.is_empty() {
            return;
        }
        self.effect_chain.remove(self.active_effect);
        self.close_selected_aex();
        self.active_effect = self
            .active_effect
            .min(self.effect_chain.len().saturating_sub(1));
        if let Some(slot) = self.effect_chain.get(self.active_effect) {
            self.restore_effect(slot.editor.clone());
        }
        self.invalidate_chain();
    }

    fn move_chain_effect(&mut self, target: usize) {
        if !self.chain_edit_ready() || target >= self.effect_chain.len() {
            return;
        }
        self.save_active_effect();
        self.effect_chain.swap(self.active_effect, target);
        self.active_effect = target;
        self.invalidate_chain();
    }

    fn show_effect_chain(&mut self, ui: &mut egui::Ui) {
        let edit_ready = self.chain_edit_ready();
        if self.effect_chain.is_empty() {
            self.save_active_effect();
        }
        ui.label(
            RichText::new(self.ui_kit.text(
                "Effect chain · top to bottom",
                "エフェクトチェーン · 上から順に適用",
            ))
            .strong(),
        );
        let mut select = None;
        let mut changed = false;
        egui::ScrollArea::vertical()
            .id_salt("effect-chain")
            .max_height(168.0)
            .show(ui, |ui| {
                for (index, slot) in self.effect_chain.iter_mut().enumerate() {
                    let selection = if index == self.active_effect {
                        self.selection.as_ref()
                    } else {
                        slot.editor.selection.as_ref()
                    };
                    let name = selection
                        .and_then(|s| s.path.file_stem())
                        .and_then(|name| name.to_str())
                        .unwrap_or("Choose AEX...");
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add_enabled(
                                edit_ready,
                                egui::Checkbox::without_text(&mut slot.enabled),
                            )
                            .on_hover_text(
                                self.ui_kit.text("Enable this stage", "この段を有効にする"),
                            )
                            .changed();
                        if ui
                            .add_enabled(
                                edit_ready,
                                egui::Button::selectable(
                                    index == self.active_effect,
                                    format!("{}  {name}", index + 1),
                                ),
                            )
                            .clicked()
                        {
                            select = Some(index);
                        }
                    });
                }
            });
        if let Some(index) = select {
            self.select_chain_effect(index);
        }
        if changed {
            self.invalidate_chain();
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    edit_ready && self.effect_chain.len() < MAX_EFFECT_CHAIN_STAGES,
                    egui::Button::new(self.ui_kit.text("Add...", "追加...")),
                )
                .clicked()
            {
                self.add_chain_effect();
            }
            if ui
                .add_enabled(
                    edit_ready && self.active_effect > 0,
                    egui::Button::new(self.ui_kit.text("Up", "上へ")),
                )
                .clicked()
            {
                self.move_chain_effect(self.active_effect - 1);
            }
            if ui
                .add_enabled(
                    edit_ready && self.active_effect + 1 < self.effect_chain.len(),
                    egui::Button::new(self.ui_kit.text("Down", "下へ")),
                )
                .clicked()
            {
                self.move_chain_effect(self.active_effect + 1);
            }
            if ui
                .add_enabled(
                    edit_ready && !self.effect_chain.is_empty(),
                    egui::Button::new(self.ui_kit.text("Remove", "削除")),
                )
                .clicked()
            {
                self.remove_chain_effect();
            }
        });
        if self.effect_chain.len() > 1 {
            ui.small(self.ui_kit.text(
                "Chain preview/export: ARGB8. Each stage keeps its own controls.",
                "チェーンのプレビュー／書き出しはARGB8です。各段の設定は個別に保持されます。",
            ));
        }
        ui.separator();
        if self.pending_parameter_slot.is_some() {
            ui.small(self.ui_kit.text(
                "Finishing the current stage update...",
                "現在の段の更新を完了しています...",
            ));
        }
    }

    fn chain_render_plan(&self) -> Result<Vec<ChainRenderStage>, String> {
        use aexcompat_broker::image_render::{RenderGpuBackend, RenderPixelFormat, RenderUiAction};
        if self.pixel_format != RenderPixelFormat::Argb8 {
            return Err("Effect chains currently require ARGB8. Deep single-effect rendering remains available; no 16/32-bit intermediate will be converted to 8-bit.".into());
        }
        if !matches!(
            self.gpu_backend,
            RenderGpuBackend::Auto | RenderGpuBackend::Cpu
        ) {
            return Err("ARGB8 effect chains require Auto or CPU backend.".into());
        }
        let mut plan = Vec::new();
        let active = self.capture_effect();
        for (index, slot) in self
            .effect_chain
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.enabled)
        {
            let editor = if index == self.active_effect {
                &active
            } else {
                &slot.editor
            };
            let stage = index + 1;
            let selection = editor
                .selection
                .as_ref()
                .ok_or_else(|| format!("Effect {stage}: no AEX selected."))?;
            if !editor.session_approved || editor.selection_stale {
                return Err(format!(
                    "Effect {stage}: reload this AEX and its Effect Controls before rendering."
                ));
            }
            if editor.audio_input.is_some() || editor.audio_effect_only {
                return Err(format!(
                    "Effect {stage}: audio is not supported in image effect chains."
                ));
            }
            let capability = editor.smart_render_capability.ok_or_else(|| {
                format!("Effect {stage}: Effect Controls inspection is not ready.")
            })?;
            let selected = selected_interactive_session_selection(
                capability,
                editor.smart_render,
                editor.smart_render_manual_override,
            )
            .map_err(|error| format!("Effect {stage}: {error}"))?;
            let search_dirs = approved_dependency_search_dirs(
                &selection.path,
                &editor.approved_dependencies,
                &editor.runtime_dependency_roots,
            )
            .map_err(|error| format!("Effect {stage}: {error}"))?;
            let custom_ui_action = if editor.apply_custom_ui_click_to_render {
                Some(RenderUiAction::Click {
                    point: editor.custom_ui_click_point,
                    color: editor.custom_ui_click_color,
                })
            } else if editor.apply_custom_ui_draw_to_render {
                Some(RenderUiAction::Draw)
            } else {
                None
            };
            plan.push(ChainRenderStage {
                ordinal: stage,
                plugin: selection.path.clone(),
                hash: selection.sha256.clone(),
                parameters: parameters_for_native_action(
                    &editor.parameters,
                    &editor.parameter_defaults,
                )
                .into_iter()
                .chain(
                    editor
                        .parameters
                        .iter()
                        .filter(|parameter| {
                            parameter.kind == "layer" && parameter.layer_path.is_some()
                        })
                        .cloned(),
                )
                .collect(),
                selection: selected,
                dependencies: editor.approved_dependencies.clone(),
                search_dirs,
                host_context: editor.host_context.clone(),
                custom_ui_action,
            });
        }
        if plan.is_empty() {
            return Err("Enable at least one effect in the chain.".into());
        }
        Ok(plan)
    }

    fn render_chain_to(&mut self, output: PathBuf) {
        self.clear_render_output();
        let plan = match self.chain_render_plan() {
            Ok(plan) => plan,
            Err(error) => {
                self.status = error.clone();
                self.report = error;
                return;
            }
        };
        let Some(input) = self.input_image.clone() else {
            return;
        };
        let timing = match render_timing(
            self.frame,
            self.duration_frames,
            self.frames_per_second,
            self.frame_time_step,
        ) {
            Ok(timing) => timing,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        self.close_live_session();
        let repository = self.repository.clone();
        let gpu_backend = self.gpu_backend;
        self.chain_render_fingerprint = Some(self.current_render_input_fingerprint());
        self.status = format!("Rendering {} effects in order...", plan.len());
        // A chain report describes all stage identities. Do not attribute it to
        // whichever stage happens to be selected in the editor.
        self.spawn(move || {
            let report = run_effect_chain(&repository, &input, &output, &plan, |stage, input, output| {
                let mut report = aexcompat_broker::image_render::render_experimental_image_with_approved_dependencies_and_search_dirs(
                    &repository, &stage.plugin, &stage.hash, input, output, &stage.parameters, timing,
                    stage.selection.path.is_smart(), aexcompat_broker::image_render::RenderPixelFormat::Argb8,
                    stage.host_context.as_ref(), stage.custom_ui_action.clone(), gpu_backend,
                    stage.dependencies.clone(), stage.search_dirs.clone(),
                ).map_err(|error| error.to_string())?;
                aexcompat_broker::image_render::annotate_interactive_selection(&mut report, stage.selection);
                Ok(report)
            });
            let passed = report["passed"] == true;
            let body = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
            if passed { Ok((body, Some(output))) } else { Err(body) }
        });
        self.rendering = true;
        self.task_kind = TaskKind::RenderChain;
    }
}

struct ChainScratch {
    root: PathBuf,
    outputs: Vec<PathBuf>,
}

impl ChainScratch {
    fn new(repository: &Path, stages: usize) -> std::io::Result<Self> {
        let parent = repository.join("target/chain-transport");
        fs::create_dir_all(&parent)?;
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_nanos();
        let root = parent.join(format!("chain-{}-{nonce}", std::process::id()));
        fs::create_dir(&root)?;
        let outputs = (0..stages)
            .map(|index| root.join(format!("stage-{index}.png")))
            .collect();
        Ok(Self { root, outputs })
    }

    fn cleanup(&self) -> std::io::Result<()> {
        for path in &self.outputs {
            match fs::remove_file(path) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error),
            }
        }
        match fs::remove_dir(&self.root) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl Drop for ChainScratch {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

// Shared by GUI dispatch and behavioral tests. Never publish a partial chain;
// all intermediate outputs live in one freshly owned directory and are removed.
fn run_effect_chain(
    repository: &Path,
    input: &Path,
    output: &Path,
    plan: &[ChainRenderStage],
    mut render: impl FnMut(&ChainRenderStage, &Path, &Path) -> Result<serde_json::Value, String>,
) -> serde_json::Value {
    let mut report = serde_json::json!({
        "schema_version": 1, "stage": "effect_chain_render", "pixel_format": "argb8",
        "passed": false, "stages": [],
    });
    let result = (|| -> Result<(), String> {
        if plan.is_empty() || plan.len() > MAX_EFFECT_CHAIN_STAGES {
            return Err("Invalid enabled effect count.".into());
        }
        if output.exists() {
            return Err("Output image already exists.".into());
        }
        let scratch =
            ChainScratch::new(repository, plan.len()).map_err(|error| error.to_string())?;
        let mut current = input.to_path_buf();
        for (index, stage) in plan.iter().enumerate() {
            report["failed_stage"] = serde_json::json!(stage.ordinal);
            let rendered = render(stage, &current, &scratch.outputs[index]);
            let mut evidence = serde_json::json!({ "stage": stage.ordinal, "plugin_path": stage.plugin, "inspection_plugin_sha256": stage.hash, "plugin_sha256": null });
            match rendered {
                Ok(native) => {
                    let passed = native["passed"] == true;
                    evidence["plugin_sha256"] = native
                        .get("plugin_sha256")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    evidence["render"] = native;
                    report["stages"].as_array_mut().unwrap().push(evidence);
                    if !passed {
                        return Err(format!(
                            "Effect {} failed; later effects were not rendered.",
                            stage.ordinal
                        ));
                    }
                }
                Err(error) => {
                    evidence["error"] = serde_json::json!(error);
                    report["stages"].as_array_mut().unwrap().push(evidence);
                    return Err(format!("Effect {}: {error}", stage.ordinal));
                }
            }
            let image = aexcompat_broker::image_render::decode_bounded_image(
                &scratch.outputs[index],
                "chain stage",
            )
            .map_err(|error| {
                format!(
                    "Effect {} output is not a valid image: {error}",
                    stage.ordinal
                )
            })?;
            if image.width() == 0 || image.height() == 0 {
                return Err(format!("Effect {} output is empty.", stage.ordinal));
            }
            current = scratch.outputs[index].clone();
        }
        let final_png = fs::read(&current).map_err(|error| error.to_string())?;
        scratch
            .cleanup()
            .map_err(|error| format!("Chain intermediate cleanup failed: {error}"))?;
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)
            .map_err(|error| error.to_string())?;
        if let Err(error) = file.write_all(&final_png).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(output);
            return Err(format!("Final chain output save failed: {error}"));
        }
        report["failed_stage"] = serde_json::Value::Null;
        report["output_png"] = serde_json::json!(output);
        Ok(())
    })();
    match result {
        Ok(()) => report["passed"] = serde_json::json!(true),
        Err(error) => report["error"] = serde_json::json!(error),
    }
    report
}
