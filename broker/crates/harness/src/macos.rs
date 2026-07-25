use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_WIDTH: u32 = 1920;
const MAX_HEIGHT: u32 = 1080;

struct RenderResult {
    report: String,
    output: PathBuf,
}

#[derive(Clone)]
struct GuiParameter {
    name: String,
    param_type: i64,
    value: f64,
    minimum: f64,
    maximum: f64,
    precision: usize,
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
        let worker = match guest_worker_path(&self.repository) {
            Ok(path) => path,
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
        let parameters = self
            .parameters
            .iter()
            .map(|parameter| (parameter.name.clone(), parameter.value))
            .collect::<Vec<_>>();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut command = Command::new(&worker);
            command.arg("render-png").arg(&aex).arg(&input).arg(&output);
            for (name, value) in parameters {
                command.arg(format!("{name}={value}"));
            }
            let result = command
                .output()
                .map_err(|error| format!("start guest worker: {error}"))
                .and_then(|process| {
                    if process.status.success() {
                        String::from_utf8(process.stdout)
                            .map_err(|error| format!("worker report is not UTF-8: {error}"))
                            .map(|report| RenderResult { report, output })
                    } else {
                        Err(format!(
                            "guest worker exited {}: {}",
                            process.status,
                            String::from_utf8_lossy(&process.stderr).trim()
                        ))
                    }
                });
            let _ = sender.send(result);
        });
        self.receiver = Some(receiver);
        self.busy = true;
        self.status = "Rendering AEX in the x64 guest worker...".into();
        self.report.clear();
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
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("AEXCompat").size(22.0));
                ui.weak("APPLE SILICON · X64 AEX CPU GUEST");
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
                if self.busy {
                    ui.spinner();
                }
                ui.label(&self.status);
            });
            ui.add_space(6.0);
        });

        egui::TopBottomPanel::top("parameters").show(ctx, |ui| {
            if self.parameters.is_empty() {
                ui.weak("AEX parameters appear here after selection.");
                return;
            }
            egui::Grid::new("mac-aex-parameters")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    for parameter in &mut self.parameters {
                        ui.label(&parameter.name);
                        match parameter.param_type {
                            4 => {
                                let mut checked = parameter.value != 0.0;
                                if ui
                                    .add_enabled(!self.busy, egui::Checkbox::new(&mut checked, ""))
                                    .changed()
                                {
                                    parameter.value = if checked { 1.0 } else { 0.0 };
                                }
                            }
                            7 => {
                                ui.add_enabled_ui(!self.busy, |ui| {
                                    egui::ComboBox::from_id_salt(&parameter.name)
                                        .selected_text(format!(
                                            "{}",
                                            parameter.value.round() as i64
                                        ))
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
                                });
                            }
                            _ => {
                                ui.add_enabled(
                                    !self.busy,
                                    egui::Slider::new(
                                        &mut parameter.value,
                                        parameter.minimum..=parameter.maximum,
                                    )
                                    .fixed_decimals(parameter.precision),
                                );
                            }
                        }
                        ui.end_row();
                    }
                });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(2, |columns| {
                show_texture(&mut columns[0], "Input PNG", self.input_texture.as_ref());
                show_texture(&mut columns[1], "AEX output", self.output_texture.as_ref());
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("AEX").strong());
                ui.monospace(
                    self.aex
                        .as_deref()
                        .map(Path::display)
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "not selected".into()),
                );
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("PNG").strong());
                ui.monospace(
                    self.input
                        .as_deref()
                        .map(Path::display)
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "not selected".into()),
                );
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Output").strong());
                ui.monospace(
                    self.output
                        .as_deref()
                        .map(Path::display)
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "not rendered".into()),
                );
            });
            egui::CollapsingHeader::new("Worker report")
                .default_open(true)
                .show(ui, |ui| {
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

fn show_texture(ui: &mut egui::Ui, label: &str, texture: Option<&egui::TextureHandle>) {
    ui.label(RichText::new(label).strong());
    let Some(texture) = texture else {
        ui.centered_and_justified(|ui| {
            ui.colored_label(Color32::GRAY, "Not available");
        });
        return;
    };
    ui.label(format!("{} × {}", texture.size()[0], texture.size()[1]));
    let available = ui.available_size().max(egui::vec2(1.0, 1.0));
    let source = egui::vec2(texture.size()[0] as f32, texture.size()[1] as f32);
    let scale = (available.x / source.x)
        .min((available.y.min(600.0) / source.y).max(0.01))
        .min(1.0);
    ui.image((texture.id(), source * scale));
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
    let worker = guest_worker_path(repository)?;
    let process = Command::new(worker)
        .arg("setup")
        .arg(aex)
        .output()
        .map_err(|error| format!("start guest worker setup: {error}"))?;
    if !process.status.success() {
        return Err(format!(
            "guest worker setup exited {}: {}",
            process.status,
            String::from_utf8_lossy(&process.stderr).trim()
        ));
    }
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
            minimum,
            maximum,
            precision: parameter["precision"].as_u64().unwrap_or(0).min(8) as usize,
        });
    }
    Ok((parameters, report))
}

fn guest_worker_path(repository: &Path) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("AEXCOMPAT_GUEST_WORKER").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "AEXCOMPAT_GUEST_WORKER does not identify a file: {}",
            path.display()
        ));
    }
    let candidates = [
        repository.join("guest/target/release/aex-guest-worker"),
        repository.join("guest/target/debug/aex-guest-worker"),
    ];
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            format!(
                "Build it first with: cargo build --release --manifest-path {}/guest/Cargo.toml -p aex-guest-worker",
                repository.display()
            )
        })
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
            assert!(guest_worker_path(&repository).is_ok());
        }
    }
}
