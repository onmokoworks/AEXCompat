use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::gui_state::{GuiParameter, LiveRenderState, ViewerMode, reset_all};

const MAX_WIDTH: u32 = 1920;
const MAX_HEIGHT: u32 = 1080;
const NATIVE_SETUP_DEADLINE: Duration = Duration::from_secs(2);
const NATIVE_RENDER_DEADLINE: Duration = Duration::from_secs(5);

struct RenderResult {
    report: String,
    output: PathBuf,
}

pub fn run() -> eframe::Result<()> {
    let repository = repository_root()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "AEXCompat Image Harness",
        options,
        Box::new(move |_cc| Ok(Box::new(MacHarnessApp::new(repository)))),
    )
}

struct MacHarnessApp {
    repository: PathBuf,
    aex: Option<PathBuf>,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    input_texture: Option<egui::TextureHandle>,
    output_texture: Option<egui::TextureHandle>,
    status: String,
    report: String,
    parameters: Vec<GuiParameter>,
    live_render: LiveRenderState,
    viewer_mode: ViewerMode,
    viewer_zoom: f32,
    viewer_pan: egui::Vec2,
    busy: bool,
    receiver: Option<Receiver<Result<RenderResult, String>>>,
}

impl MacHarnessApp {
    fn new(repository: PathBuf) -> Self {
        Self {
            repository,
            aex: None,
            input: None,
            output: None,
            input_texture: None,
            output_texture: None,
            status: "Select an x64 AEX and a PNG image.".into(),
            report: String::new(),
            parameters: Vec::new(),
            live_render: LiveRenderState::default(),
            viewer_mode: ViewerMode::Input,
            viewer_zoom: 1.0,
            viewer_pan: egui::Vec2::ZERO,
            busy: false,
            receiver: None,
        }
    }

    fn choose_aex(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("After Effects plug-in", &["aex"])
            .pick_file()
        {
            match discover_parameters(&self.repository, &path) {
                Ok((parameters, report)) => {
                    self.status = format!(
                        "Selected AEX with {} editable parameters: {}",
                        parameters.len(),
                        path.display()
                    );
                    self.report = report;
                    self.parameters = parameters;
                    self.aex = Some(path);
                    self.viewer_mode = ViewerMode::Input;
                }
                Err(error) => {
                    self.status = "Could not inspect AEX parameters.".into();
                    self.report = error;
                    self.parameters.clear();
                    self.aex = None;
                }
            }
            self.output = None;
            self.output_texture = None;
        }
    }

    fn choose_input(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG image", &["png"])
            .pick_file()
        else {
            return;
        };
        match load_texture(ctx, "mac-input", &path) {
            Ok((texture, width, height)) if width <= MAX_WIDTH && height <= MAX_HEIGHT => {
                self.input = Some(path);
                self.input_texture = Some(texture);
                self.output = None;
                self.output_texture = None;
                self.viewer_mode = ViewerMode::Input;
                self.status = format!("Input ready: {width}x{height}");
            }
            Ok((_, width, height)) => {
                self.status = "Input exceeds the initial Full HD boundary.".into();
                self.report = format!(
                    "PNG must be within 1x1..={MAX_WIDTH}x{MAX_HEIGHT}; got {width}x{height}"
                );
            }
            Err(error) => {
                self.status = "Could not open input PNG.".into();
                self.report = error;
            }
        }
    }

    fn render(&mut self) {
        let (Some(aex), Some(input)) = (self.aex.clone(), self.input.clone()) else {
            return;
        };
        let workers = match guest_worker_candidates(&self.repository) {
            Ok(paths) => paths,
            Err(error) => {
                self.status = "Guest worker is not built.".into();
                self.report = error;
                return;
            }
        };
        let output_directory = self.repository.join("target/harness-output");
        if let Err(error) = std::fs::create_dir_all(&output_directory) {
            self.status = "Could not create output directory.".into();
            self.report = error.to_string();
            return;
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let output = output_directory.join(format!("mac-aex-{nonce}.png"));
        let parameter_arguments = render_parameter_arguments(&self.parameters);
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut arguments = vec![
                "render-png".to_string(),
                aex.to_string_lossy().into_owned(),
                input.to_string_lossy().into_owned(),
                output.to_string_lossy().into_owned(),
            ];
            arguments.extend(parameter_arguments);
            let result = run_guest_workers(&workers, &arguments, NATIVE_RENDER_DEADLINE).and_then(
                |process| {
                    String::from_utf8(process.stdout)
                        .map_err(|error| format!("worker report is not UTF-8: {error}"))
                        .map(|report| RenderResult { report, output })
                },
            );
            let _ = sender.send(result);
        });
        self.receiver = Some(receiver);
        self.busy = true;
        self.status = "Rendering AEX in the x64 guest worker...".into();
        self.report.clear();
    }

    fn parameter_changed(&mut self) {
        self.live_render.parameter_changed(Instant::now());
    }

    fn dispatch_live_render(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let ready = self.aex.is_some() && self.input.is_some();
        if self.live_render.take_due(now, self.busy, ready) {
            self.render();
        } else if ready
            && !self.busy
            && let Some(remaining) = self.live_render.remaining(now)
        {
            ctx.request_repaint_after(remaining.min(Duration::from_millis(60)));
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let result = self
            .receiver
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok());
        let Some(result) = result else {
            if self.busy {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            return;
        };
        self.busy = false;
        self.receiver = None;
        match result {
            Ok(result) => match load_texture(ctx, "mac-output", &result.output) {
                Ok((texture, width, height)) => {
                    self.output = Some(result.output);
                    self.output_texture = Some(texture);
                    self.viewer_mode = ViewerMode::Output;
                    self.status = format!("Completed: {width}x{height} ARGB8 output");
                    self.report = result.report;
                }
                Err(error) => {
                    self.status = "Worker completed but output PNG could not be opened.".into();
                    self.report = error;
                }
            },
            Err(error) => {
                self.status = "Render failed.".into();
                self.report = error;
            }
        }
    }
}

impl eframe::App for MacHarnessApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_visuals(egui::Visuals::dark());
        self.poll(ctx);
        self.dispatch_live_render(ctx);
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("AEXCompat").size(22.0));
                ui.weak("EFFECT LAB · APPLE SILICON X64 GUEST");
                ui.separator();
                if ui
                    .add_enabled(!self.busy, egui::Button::new("AEX..."))
                    .clicked()
                {
                    self.choose_aex();
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("PNG..."))
                    .clicked()
                {
                    self.choose_input(ctx);
                }
                let ready = !self.busy && self.aex.is_some() && self.input.is_some();
                if ui.add_enabled(ready, egui::Button::new("Render")).clicked() {
                    self.render();
                }
                ui.separator();
                let mut live_render = self.live_render.enabled();
                if ui.checkbox(&mut live_render, "Auto Update").changed() {
                    self.live_render.set_enabled(live_render);
                }
                if self.busy {
                    ui.spinner();
                }
                ui.label(&self.status);
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
            egui::CollapsingHeader::new("Paths and worker report")
                .default_open(false)
                .show(ui, |ui| {
                    for (label, path) in [
                        ("AEX", self.aex.as_deref()),
                        ("Input", self.input.as_deref()),
                        ("Output", self.output.as_deref()),
                    ] {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(label).strong());
                            ui.monospace(
                                path.map(Path::display)
                                    .map(|value| value.to_string())
                                    .unwrap_or_else(|| "not available".into()),
                            );
                        });
                    }
                    ui.add(
                        egui::TextEdit::multiline(&mut self.report)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(12),
                    );
                });
        });
    }
}

impl MacHarnessApp {
    fn show_effect_controls(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.heading(RichText::new("Effect Controls").size(20.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !self.busy
                            && self
                                .parameters
                                .iter()
                                .any(|parameter| !parameter.is_default()),
                        egui::Button::new("Reset All"),
                    )
                    .clicked()
                {
                    changed |= reset_all(&mut self.parameters);
                }
            });
        });
        ui.label(
            self.aex
                .as_deref()
                .and_then(Path::file_stem)
                .and_then(|name| name.to_str())
                .unwrap_or("Select an AEX to load its parameters."),
        );
        ui.separator();
        if self.parameters.is_empty() {
            ui.weak("This effect exposed no supported editable parameters.");
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for parameter in &mut self.parameters {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&parameter.name).small());
                        if ui
                            .add_enabled(
                                !self.busy && !parameter.is_default(),
                                egui::Button::new("Reset").small(),
                            )
                            .clicked()
                        {
                            changed |= parameter.reset();
                        }
                    });
                    let previous = parameter.value;
                    ui.add_enabled_ui(!self.busy, |ui| match parameter.param_type {
                        4 => {
                            let mut checked = parameter.value != 0.0;
                            if ui.checkbox(&mut checked, "Enabled").changed() {
                                parameter.value = f64::from(checked);
                            }
                        }
                        7 => {
                            egui::ComboBox::from_id_salt(("mac-effect-control", &parameter.name))
                                .selected_text(format!("{}", parameter.value.round() as i64))
                                .show_ui(ui, |ui| {
                                    for choice in
                                        parameter.minimum as i64..=parameter.maximum as i64
                                    {
                                        ui.selectable_value(
                                            &mut parameter.value,
                                            choice as f64,
                                            choice.to_string(),
                                        );
                                    }
                                });
                        }
                        _ => {
                            ui.add(
                                egui::Slider::new(
                                    &mut parameter.value,
                                    parameter.minimum..=parameter.maximum,
                                )
                                .fixed_decimals(parameter.precision)
                                .show_value(true),
                            );
                        }
                    });
                    changed |= parameter.value != previous;
                    ui.add_space(6.0);
                }
            });
        if changed {
            self.parameter_changed();
        }
    }

    fn show_workspace_viewer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.viewer_mode, ViewerMode::Input, "INPUT");
            ui.selectable_value(&mut self.viewer_mode, ViewerMode::Output, "AEX OUTPUT");
            ui.selectable_value(&mut self.viewer_mode, ViewerMode::Compare, "COMPARE");
            ui.separator();
            ui.weak("FHD workspace / aspect fit");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Fit").clicked() {
                    self.viewer_zoom = 1.0;
                    self.viewer_pan = egui::Vec2::ZERO;
                }
                ui.monospace(format!("{:.0}%", self.viewer_zoom * 100.0));
                if self.busy {
                    ui.spinner();
                    ui.weak("Rendering...");
                }
            });
        });
        ui.separator();
        let viewer_height = (ui.available_height() * 0.68).clamp(300.0, 860.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), viewer_height),
            egui::Layout::top_down(egui::Align::Center),
            |ui| match self.viewer_mode {
                ViewerMode::Input => show_viewer_texture(
                    ui,
                    "Input",
                    self.input_texture.as_ref(),
                    &mut self.viewer_zoom,
                    &mut self.viewer_pan,
                ),
                ViewerMode::Output => show_viewer_texture(
                    ui,
                    "AEX output",
                    self.output_texture.as_ref(),
                    &mut self.viewer_zoom,
                    &mut self.viewer_pan,
                ),
                ViewerMode::Compare => ui.columns(2, |columns| {
                    show_viewer_texture(
                        &mut columns[0],
                        "Input",
                        self.input_texture.as_ref(),
                        &mut self.viewer_zoom,
                        &mut self.viewer_pan,
                    );
                    show_viewer_texture(
                        &mut columns[1],
                        "AEX output",
                        self.output_texture.as_ref(),
                        &mut self.viewer_zoom,
                        &mut self.viewer_pan,
                    );
                }),
            },
        );
    }
}

fn show_viewer_texture(
    ui: &mut egui::Ui,
    label: &str,
    texture: Option<&egui::TextureHandle>,
    zoom: &mut f32,
    pan: &mut egui::Vec2,
) {
    let Some(texture) = texture else {
        ui.centered_and_justified(|ui| {
            ui.colored_label(Color32::GRAY, format!("{label} is not available"));
        });
        return;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).strong());
        ui.monospace(format!("{} x {}", texture.size()[0], texture.size()[1]));
        ui.weak("Wheel to zoom / drag to pan");
    });
    let available = ui.available_size().max(egui::vec2(1.0, 1.0));
    let source = egui::vec2(texture.size()[0] as f32, texture.size()[1] as f32);
    let (viewport, response) = ui.allocate_exact_size(available, egui::Sense::click_and_drag());
    if response.hovered() {
        let scroll = ui.input(|input| input.raw_scroll_delta.y);
        if scroll != 0.0 {
            let previous_zoom = *zoom;
            *zoom = (*zoom * (scroll * 0.0025).exp()).clamp(0.25, 8.0);
            if let Some(pointer) = response.hover_pos() {
                let pointer_from_center = pointer - viewport.center();
                *pan = pointer_from_center - (pointer_from_center - *pan) * (*zoom / previous_zoom);
            }
        }
    }
    if response.dragged_by(egui::PointerButton::Primary)
        || response.dragged_by(egui::PointerButton::Middle)
    {
        *pan += response.drag_delta();
    }
    let fit_scale = (available.x / source.x).min(available.y / source.y);
    let display = source * fit_scale * *zoom;
    let image_rect = egui::Rect::from_center_size(viewport.center() + *pan, display);
    ui.painter().with_clip_rect(viewport).image(
        texture.id(),
        image_rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

fn load_texture(
    ctx: &egui::Context,
    name: &str,
    path: &Path,
) -> Result<(egui::TextureHandle, u32, u32), String> {
    let rgba = image::open(path)
        .map_err(|error| error.to_string())?
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return Err("PNG dimensions must be nonzero".into());
    }
    let texture = ctx.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], rgba.as_raw()),
        egui::TextureOptions::LINEAR,
    );
    Ok((texture, width, height))
}

fn discover_parameters(
    repository: &Path,
    aex: &Path,
) -> Result<(Vec<GuiParameter>, String), String> {
    let workers = guest_worker_candidates(repository)?;
    let process = run_guest_workers(
        &workers,
        &["setup".to_string(), aex.to_string_lossy().into_owned()],
        NATIVE_SETUP_DEADLINE,
    )?;
    let report = String::from_utf8(process.stdout)
        .map_err(|error| format!("worker setup report is not UTF-8: {error}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&report).map_err(|error| format!("parse setup report: {error}"))?;
    let declared = value["parameters"]
        .as_array()
        .ok_or_else(|| "setup report has no parameters array".to_string())?;
    let mut parameters = Vec::new();
    for parameter in declared {
        let param_type = parameter["param_type"]
            .as_i64()
            .ok_or_else(|| "parameter has no numeric param_type".to_string())?;
        if !matches!(param_type, 1 | 2 | 4 | 7 | 10) {
            continue;
        }
        let name = parameter["name"]
            .as_str()
            .ok_or_else(|| "parameter has no name".to_string())?
            .to_string();
        let value = parameter["default_value"]
            .as_f64()
            .ok_or_else(|| format!("editable parameter {name:?} has no default value"))?;
        let minimum = parameter["slider_min"]
            .as_f64()
            .or_else(|| parameter["valid_min"].as_f64())
            .ok_or_else(|| format!("editable parameter {name:?} has no minimum"))?;
        let maximum = parameter["slider_max"]
            .as_f64()
            .or_else(|| parameter["valid_max"].as_f64())
            .ok_or_else(|| format!("editable parameter {name:?} has no maximum"))?;
        if !minimum.is_finite() || !maximum.is_finite() || minimum > maximum {
            return Err(format!(
                "editable parameter {name:?} has an invalid range {minimum}..={maximum}"
            ));
        }
        parameters.push(GuiParameter {
            name,
            param_type,
            value: value.clamp(minimum, maximum),
            default_value: value.clamp(minimum, maximum),
            minimum,
            maximum,
            precision: parameter["precision"].as_u64().unwrap_or(0).min(8) as usize,
        });
    }
    Ok((parameters, report))
}

fn render_parameter_arguments(parameters: &[GuiParameter]) -> Vec<String> {
    parameters
        .iter()
        .map(|parameter| format!("{}={}", parameter.name, parameter.value))
        .collect()
}

#[derive(Clone, Debug)]
struct GuestWorkerCandidate {
    path: PathBuf,
    native: bool,
}

fn guest_worker_candidates(repository: &Path) -> Result<Vec<GuestWorkerCandidate>, String> {
    if let Some(path) = std::env::var_os("AEXCOMPAT_GUEST_WORKER").map(PathBuf::from) {
        if path.is_file() {
            return Ok(vec![GuestWorkerCandidate {
                path,
                native: false,
            }]);
        }
        return Err(format!(
            "AEXCOMPAT_GUEST_WORKER does not identify a file: {}",
            path.display()
        ));
    }
    let mut candidates = Vec::new();
    if std::env::var_os("AEXCOMPAT_NATIVE_CARRIER").is_some_and(|value| value == "1") {
        candidates.push(GuestWorkerCandidate {
            path: repository.join("guest/target/x86_64-apple-darwin/release/aex-guest-worker"),
            native: true,
        });
    }
    candidates.extend([
        GuestWorkerCandidate {
            path: repository.join("guest/target/release/aex-guest-worker"),
            native: false,
        },
        GuestWorkerCandidate {
            path: repository.join("guest/target/debug/aex-guest-worker"),
            native: false,
        },
    ]);
    let existing = candidates
        .into_iter()
        .filter(|candidate| candidate.path.is_file())
        .collect::<Vec<_>>();
    if existing.is_empty() {
        Err({
            format!(
                "Build a guest worker first. Native: cargo build --release --target x86_64-apple-darwin --features native-carrier --manifest-path {}/guest/Cargo.toml -p aex-guest-worker; Unicorn fallback: cargo build --release --manifest-path {}/guest/Cargo.toml -p aex-guest-worker",
                repository.display(),
                repository.display()
            )
        })
    } else {
        Ok(existing)
    }
}

fn run_guest_workers(
    candidates: &[GuestWorkerCandidate],
    arguments: &[String],
    native_deadline: Duration,
) -> Result<Output, String> {
    let mut failures = Vec::new();
    for candidate in candidates {
        let deadline = candidate.native.then_some(native_deadline);
        match run_worker(&candidate.path, arguments, deadline) {
            Ok(output) if output.status.success() => return Ok(output),
            Ok(output) => failures.push(format!(
                "{} exited {}: {}",
                candidate.path.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(error) => failures.push(format!("{}: {error}", candidate.path.display())),
        }
    }
    Err(format!(
        "all guest workers failed: {}",
        failures.join(" | ")
    ))
}

fn run_worker(
    worker: &Path,
    arguments: &[String],
    deadline: Option<Duration>,
) -> Result<Output, String> {
    let mut child = Command::new(worker)
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("start guest worker: {error}"))?;
    if let Some(deadline) = deadline {
        let started = Instant::now();
        loop {
            match child
                .try_wait()
                .map_err(|error| format!("poll guest worker: {error}"))?
            {
                Some(_) => break,
                None if started.elapsed() < deadline => {
                    thread::sleep(Duration::from_millis(5));
                }
                None => {
                    let _ = child.kill();
                    return Err(format!(
                        "native worker exceeded {} ms and was terminated",
                        deadline.as_millis()
                    ));
                }
            }
        }
    }
    child
        .wait_with_output()
        .map_err(|error| format!("collect guest worker output: {error}"))
}

fn repository_root() -> Option<PathBuf> {
    let mut starts = Vec::new();
    if let Ok(current) = std::env::current_dir() {
        starts.push(current);
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            starts.push(parent.to_path_buf());
        }
    }
    for start in starts {
        for ancestor in start.ancestors() {
            if ancestor.join("guest/Cargo.toml").is_file()
                && ancestor.join("broker/Cargo.toml").is_file()
            {
                return Some(ancestor.to_path_buf());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_root_is_found_from_worktree() {
        assert!(repository_root().is_some());
    }

    #[test]
    fn worker_resolution_accepts_the_workspace_debug_binary() {
        let repository = repository_root().unwrap();
        if repository
            .join("guest/target/debug/aex-guest-worker")
            .is_file()
        {
            assert!(guest_worker_candidates(&repository).is_ok());
        }
    }

    #[test]
    fn failed_native_candidate_falls_back_to_the_next_worker() {
        let workers = [
            GuestWorkerCandidate {
                path: PathBuf::from("/usr/bin/false"),
                native: true,
            },
            GuestWorkerCandidate {
                path: PathBuf::from("/usr/bin/true"),
                native: false,
            },
        ];
        let output = run_guest_workers(&workers, &[], Duration::from_millis(100)).unwrap();
        assert!(output.status.success());
    }

    #[test]
    fn timed_out_native_candidate_falls_back_without_waiting_for_its_deadline_twice() {
        let workers = [
            GuestWorkerCandidate {
                path: PathBuf::from("/bin/sleep"),
                native: true,
            },
            GuestWorkerCandidate {
                path: PathBuf::from("/usr/bin/true"),
                native: false,
            },
        ];
        let started = Instant::now();
        let output =
            run_guest_workers(&workers, &["1".to_string()], Duration::from_millis(20)).unwrap();
        assert!(output.status.success());
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn render_arguments_preserve_discovered_parameter_names_and_values() {
        let parameters = [
            GuiParameter {
                name: "Amount".into(),
                param_type: 10,
                value: 50.25,
                default_value: 5.0,
                minimum: 0.0,
                maximum: 100.0,
                precision: 2,
            },
            GuiParameter {
                name: "Legacy".into(),
                param_type: 4,
                value: 1.0,
                default_value: 0.0,
                minimum: 0.0,
                maximum: 1.0,
                precision: 0,
            },
        ];
        assert_eq!(
            render_parameter_arguments(&parameters),
            ["Amount=50.25", "Legacy=1"]
        );
    }
}
