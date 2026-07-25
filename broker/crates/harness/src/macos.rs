use eframe::egui::{self, Color32, RichText};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::gui_state::{GuiParameter, LiveRenderState, ViewerMode, reset_all};

const MAX_WIDTH: u32 = 1920;
const MAX_HEIGHT: u32 = 1080;
const NATIVE_SETUP_DEADLINE: Duration = Duration::from_secs(2);
const RESIDENT_START_DEADLINE: Duration = Duration::from_secs(10);
const RESIDENT_RENDER_DEADLINE: Duration = Duration::from_secs(30);
const RESIDENT_CLOSE_DEADLINE: Duration = Duration::from_secs(2);

struct RenderResult {
    report: String,
    output: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResidentFrameOutput {
    width: u32,
    height: u32,
    rowbytes: u32,
    pixel_format: String,
    checksum: String,
    guards_intact: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResidentFrameDone {
    v: u32,
    #[serde(rename = "type")]
    kind: String,
    frame_index: u64,
    status: String,
    output: Option<ResidentFrameOutput>,
    render_error: i32,
    generation: Option<u64>,
}

fn validate_resident_frame(
    value: &Value,
    frame_index: u64,
    width: u32,
    height: u32,
) -> Result<String, String> {
    let done: ResidentFrameDone = serde_json::from_value(value.clone())
        .map_err(|error| format!("malformed resident frame response: {error}"))?;
    let expected_generation = frame_index
        .checked_add(1)
        .ok_or_else(|| "resident frame generation overflow".to_string())?;
    let Some(output) = done.output else {
        return Err(format!("resident frame has no output: {value}"));
    };
    if done.v != 1
        || done.kind != "frame_done"
        || done.frame_index != frame_index
        || done.status != "ok"
        || done.render_error != 0
        || done.generation != Some(expected_generation)
        || output.width != width
        || output.height != height
        || output.rowbytes != width * 4
        || output.pixel_format != "argb8"
        || output.checksum.len() != 64
        || !output.checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !output.guards_intact
    {
        return Err(format!(
            "resident frame response failed invariants: {value}"
        ));
    }
    Ok(output.checksum)
}

fn validate_resident_close(value: &Value, worker_pid: u32) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "resident close response is not an object".to_string())?;
    if object.len() != 5
        || value["v"].as_u64() != Some(1)
        || value["type"].as_str() != Some("session_closed")
        || value["worker_pid"].as_u64() != Some(worker_pid as u64)
        || !value["setup"].is_object()
    {
        return Err(format!("resident close envelope is invalid: {value}"));
    }
    let close = value["close"]
        .as_object()
        .ok_or_else(|| format!("resident close report is missing: {value}"))?;
    if close.len() != 10
        || close.get("session_clean").and_then(Value::as_bool) != Some(true)
        || close.get("frame_setdown_error").and_then(Value::as_i64) != Some(0)
        || close.get("sequence_setdown_error").and_then(Value::as_i64) != Some(0)
        || close.get("global_setdown_error").and_then(Value::as_i64) != Some(0)
    {
        return Err(format!("resident cleanup was not clean: {value}"));
    }
    Ok(())
}

fn validate_resident_ready(value: &Value, worker_pid: u32) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "resident ready response is not an object".to_string())?;
    if object.len() != 4
        || value["v"].as_u64() != Some(1)
        || value["type"].as_str() != Some("session_ready")
        || value["worker_pid"].as_u64() != Some(worker_pid as u64)
        || !value["setup"].is_object()
    {
        return Err(format!("resident ready envelope is invalid: {value}"));
    }
    Ok(())
}

fn validate_resident_probe(
    value: &Value,
    worker_pid: u32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "resident probe response is not an object".to_string())?;
    if object.len() != 6
        || value["v"].as_u64() != Some(1)
        || value["type"].as_str() != Some("session_probed")
        || value["worker_pid"].as_u64() != Some(worker_pid as u64)
        || value["status"].as_str() != Some("ok")
        || value["guards_intact"].as_bool() != Some(true)
        || value["render_error"].as_i64() != Some(0)
    {
        return Err(format!(
            "resident {width}x{height} probe failed invariants: {value}"
        ));
    }
    Ok(())
}

enum ResidentCommand {
    Render {
        frame_index: u64,
        parameters: Vec<GuiParameter>,
        output: PathBuf,
    },
    Close(Sender<Result<(), String>>),
}

struct ResidentSessionHandle {
    aex: PathBuf,
    input: PathBuf,
    sender: Sender<ResidentCommand>,
    receiver: Receiver<Result<RenderResult, String>>,
    next_frame: u64,
    join: Option<thread::JoinHandle<()>>,
}

impl ResidentSessionHandle {
    fn shutdown(&mut self) -> Result<(), String> {
        let Some(join) = self.join.take() else {
            return Ok(());
        };
        let (reply_sender, reply_receiver) = mpsc::channel();
        let send_result = self
            .sender
            .send(ResidentCommand::Close(reply_sender))
            .map_err(|error| format!("request resident close: {error}"));
        let join_result = join
            .join()
            .map_err(|_| "resident session thread panicked during close".to_string());
        if let Err(error) = send_result {
            join_result?;
            return Err(error);
        }
        join_result?;
        reply_receiver
            .recv()
            .map_err(|error| format!("resident close result was lost: {error}"))?
    }
}

impl Drop for ResidentSessionHandle {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("aexcompat resident cleanup failed: {error}");
        }
    }
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
    resident: Option<ResidentSessionHandle>,
}

impl Drop for MacHarnessApp {
    fn drop(&mut self) {
        if let Err(error) = self.close_resident() {
            eprintln!("aexcompat resident cleanup failed during app exit: {error}");
        }
    }
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
            resident: None,
        }
    }

    fn close_resident(&mut self) -> Result<(), String> {
        let Some(mut session) = self.resident.take() else {
            return Ok(());
        };
        session.shutdown()
    }

    fn choose_aex(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("After Effects plug-in", &["aex"])
            .pick_file()
        {
            if let Err(error) = self.close_resident() {
                self.status = "Could not cleanly close the previous AEX session.".into();
                self.report = error;
                return;
            }
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
        if let Err(error) = self.close_resident() {
            self.status = "Could not cleanly close the previous input session.".into();
            self.report = error;
            return;
        }
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
        let needs_session = self
            .resident
            .as_ref()
            .is_none_or(|session| session.aex != aex || session.input != input);
        if needs_session {
            if let Err(error) = self.close_resident() {
                self.status = "Could not cleanly replace the resident session.".into();
                self.report = error;
                return;
            }
            match start_resident_session(&workers, &aex, &input, &output_directory) {
                Ok(session) => self.resident = Some(session),
                Err(error) => {
                    self.status = "Could not start resident guest session.".into();
                    self.report = error;
                    return;
                }
            }
        }
        let session = self
            .resident
            .as_mut()
            .expect("resident session was created");
        let frame_index = session.next_frame;
        session.next_frame += 1;
        if let Err(error) = session.sender.send(ResidentCommand::Render {
            frame_index,
            parameters: self.parameters.clone(),
            output,
        }) {
            self.resident.take();
            self.status = "Resident guest session stopped.".into();
            self.report = error.to_string();
            return;
        }
        self.busy = true;
        self.status = format!("Rendering frame {frame_index} in the resident x64 guest...");
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
            .resident
            .as_ref()
            .and_then(|session| session.receiver.try_recv().ok());
        let Some(result) = result else {
            if self.busy {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            return;
        };
        self.busy = false;
        match result {
            Ok(result) => match load_texture(ctx, "mac-output", &result.output) {
                Ok((texture, width, height)) => {
                    let had_output = self.output_texture.is_some();
                    self.output = Some(result.output);
                    self.output_texture = Some(texture);
                    self.viewer_mode = self.viewer_mode.after_successful_render(had_output);
                    self.status = format!("Completed: {width}x{height} ARGB8 output");
                    self.report = result.report;
                }
                Err(error) => {
                    self.status = "Worker completed but output PNG could not be opened.".into();
                    self.report = error;
                }
            },
            Err(error) => {
                let cleanup_error = self.close_resident().err();
                self.status = "Render failed.".into();
                self.report = cleanup_error
                    .map(|cleanup| format!("{error}\nresident cleanup: {cleanup}"))
                    .unwrap_or(error);
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

fn write_control_message(writer: &mut ChildStdin, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| format!("serialize resident request: {error}"))?;
    let length = u32::try_from(bytes.len())
        .map_err(|_| "resident request exceeds u32 framing".to_string())?;
    writer
        .write_all(&length.to_le_bytes())
        .and_then(|()| writer.write_all(&bytes))
        .and_then(|()| writer.flush())
        .map_err(|error| format!("write resident request: {error}"))
}

fn read_control_message(reader: &mut impl Read) -> Result<Option<Value>, String> {
    let mut prefix = [0u8; 4];
    match reader.read_exact(&mut prefix) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(format!("read resident response prefix: {error}")),
    }
    let length = u32::from_le_bytes(prefix) as usize;
    if length == 0 || length > 64 * 1024 {
        return Err(format!("resident response length is invalid: {length}"));
    }
    let mut bytes = vec![0u8; length];
    reader
        .read_exact(&mut bytes)
        .map_err(|error| format!("read resident response: {error}"))?;
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        format!(
            "parse resident response JSON: {error}; payload={}",
            String::from_utf8_lossy(&bytes)
        )
    })
}

fn parameter_payload(parameters: &[GuiParameter]) -> String {
    let assignments = parameters
        .iter()
        .map(|parameter| {
            let (kind, value) = if matches!(parameter.param_type, 1 | 4 | 7) {
                ("i32", format!("{}", parameter.value.round() as i32))
            } else {
                ("f64", format!("{}", parameter.value))
            };
            format!("param_{}@{}:{kind}={value}", parameter.slot, parameter.slot)
        })
        .collect::<Vec<_>>();
    format!("v2|{}", assignments.join(";"))
}

fn write_argb8_slot(input: &Path, slot: &Path) -> Result<(u32, u32), String> {
    let rgba = image::open(input)
        .map_err(|error| format!("open resident input PNG: {error}"))?
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err(format!(
            "resident input dimensions must be 1x1..={MAX_WIDTH}x{MAX_HEIGHT}; got {width}x{height}"
        ));
    }
    let mut argb = Vec::with_capacity(rgba.as_raw().len());
    for pixel in rgba.as_raw().chunks_exact(4) {
        argb.extend_from_slice(&[pixel[3], pixel[0], pixel[1], pixel[2]]);
    }
    std::fs::write(slot, argb).map_err(|error| format!("write resident input slot: {error}"))?;
    Ok((width, height))
}

fn save_argb8_slot(slot: &Path, output: &Path, width: u32, height: u32) -> Result<String, String> {
    let argb =
        std::fs::read(slot).map_err(|error| format!("read resident output slot: {error}"))?;
    let expected = width as usize * height as usize * 4;
    if argb.len() != expected {
        return Err(format!(
            "resident output has {} bytes, expected {expected}",
            argb.len()
        ));
    }
    let mut rgba = Vec::with_capacity(argb.len());
    for pixel in argb.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[1], pixel[2], pixel[3], pixel[0]]);
    }
    let image = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "resident output dimensions do not match bytes".to_string())?;
    image
        .save(output)
        .map_err(|error| format!("save resident output PNG: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(&argb)))
}

struct StartedResidentWorker {
    child: Child,
    stdin: ChildStdin,
    response_receiver: Receiver<Result<Option<Value>, String>>,
    stderr_receiver: Receiver<String>,
    worker_pid: u32,
}

fn start_resident_worker(
    candidates: &[GuestWorkerCandidate],
    arguments: &[String],
    width: u32,
    height: u32,
) -> Result<StartedResidentWorker, String> {
    let mut failures = Vec::new();
    for candidate in candidates {
        let mut child = match Command::new(&candidate.path)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                failures.push(format!("{}: {error}", candidate.path.display()));
                continue;
            }
        };
        let worker_pid = child.id();
        let Some(mut stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            failures.push(format!(
                "{}: stdin is unavailable",
                candidate.path.display()
            ));
            continue;
        };
        let Some(mut stdout) = child.stdout.take() else {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
            failures.push(format!(
                "{}: stdout is unavailable",
                candidate.path.display()
            ));
            continue;
        };
        let Some(mut stderr) = child.stderr.take() else {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
            failures.push(format!(
                "{}: stderr is unavailable",
                candidate.path.display()
            ));
            continue;
        };
        let (response_sender, response_receiver) = mpsc::channel();
        let (stderr_sender, stderr_receiver) = mpsc::channel();
        thread::spawn(move || {
            loop {
                let response = read_control_message(&mut stdout);
                let terminal = !matches!(response, Ok(Some(_)));
                if response_sender.send(response).is_err() || terminal {
                    break;
                }
            }
        });
        thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            let _ = stderr_sender.send(text);
        });
        let readiness = response_receiver
            .recv_timeout(RESIDENT_START_DEADLINE)
            .map_err(|error| format!("ready response timeout: {error}"))
            .and_then(|response| {
                response.map_err(|error| format!("ready response reader: {error}"))
            })
            .and_then(|response| {
                response.ok_or_else(|| "worker closed before session_ready".to_string())
            })
            .and_then(|response| validate_resident_ready(&response, worker_pid));
        let admission = readiness.and_then(|()| {
            write_control_message(&mut stdin, &json!({"v": 1, "type": "probe"}))?;
            let response = response_receiver
                .recv_timeout(RESIDENT_RENDER_DEADLINE)
                .map_err(|error| format!("probe response timeout: {error}"))?
                .map_err(|error| format!("probe response reader: {error}"))?
                .ok_or_else(|| "worker closed before session_probed".to_string())?;
            validate_resident_probe(&response, worker_pid, width, height)
        });
        if admission.is_ok() {
            return Ok(StartedResidentWorker {
                child,
                stdin,
                response_receiver,
                stderr_receiver,
                worker_pid,
            });
        }
        drop(stdin);
        let _ = child.kill();
        let _ = child.wait();
        let stderr = stderr_receiver
            .recv_timeout(Duration::from_millis(200))
            .unwrap_or_default();
        failures.push(format!(
            "{} ({}): {}{}",
            candidate.path.display(),
            if candidate.native {
                "native"
            } else {
                "fallback"
            },
            admission.unwrap_err(),
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!("; stderr: {}", stderr.trim())
            }
        ));
    }
    Err(format!(
        "all resident guest workers failed readiness: {}",
        failures.join(" | ")
    ))
}

fn wait_or_kill_resident_child(
    child: &mut Child,
    deadline: Duration,
) -> Result<std::process::ExitStatus, String> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(status),
            Ok(Some(status)) => {
                return Err(format!("resident worker exited with {status}"));
            }
            Ok(None) if started.elapsed() < deadline => {
                thread::sleep(Duration::from_millis(5));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "resident worker exceeded the {} ms close deadline and was terminated",
                    deadline.as_millis()
                ));
            }
        }
    }
}

fn start_resident_session(
    candidates: &[GuestWorkerCandidate],
    aex: &Path,
    input: &Path,
    output_directory: &Path,
) -> Result<ResidentSessionHandle, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let input_slot = output_directory.join(format!("resident-{nonce}-input.argb8"));
    let output_slot = output_directory.join(format!("resident-{nonce}-output.argb8"));
    let (width, height) = write_argb8_slot(input, &input_slot)?;
    std::fs::write(
        &output_slot,
        vec![0u8; width as usize * height as usize * 4],
    )
    .map_err(|error| format!("initialize resident output slot: {error}"))?;
    let arguments = [
        "session".to_string(),
        aex.to_string_lossy().into_owned(),
        input_slot.to_string_lossy().into_owned(),
        output_slot.to_string_lossy().into_owned(),
        width.to_string(),
        height.to_string(),
        "30".to_string(),
    ];
    let started = match start_resident_worker(candidates, &arguments, width, height) {
        Ok(started) => started,
        Err(error) => {
            let _ = std::fs::remove_file(&input_slot);
            let _ = std::fs::remove_file(&output_slot);
            return Err(error);
        }
    };
    let mut child = started.child;
    let worker_pid = started.worker_pid;
    let mut stdin = started.stdin;
    let response_receiver = started.response_receiver;
    let stderr_receiver = started.stderr_receiver;
    let (command_sender, command_receiver) = mpsc::channel();
    let (result_sender, result_receiver) = mpsc::channel();
    let join = thread::spawn(move || {
        let mut running = true;
        let mut close_reply = None;
        while running {
            match command_receiver.recv() {
                Ok(ResidentCommand::Render {
                    frame_index,
                    parameters,
                    output,
                }) => {
                    let request = json!({
                        "v": 2,
                        "type": "render_frame",
                        "frame_index": frame_index,
                        "current_time": {"value": 0, "scale": 30},
                        "parameters": parameter_payload(&parameters),
                    });
                    let result = write_control_message(&mut stdin, &request).and_then(|()| {
                        let response = response_receiver
                            .recv_timeout(RESIDENT_RENDER_DEADLINE)
                            .map_err(|error| format!("resident render response timeout: {error}"))?
                            .map_err(|error| format!("resident response reader: {error}"))?
                            .ok_or_else(|| "resident worker closed stdout".to_string())?;
                        let expected_checksum =
                            validate_resident_frame(&response, frame_index, width, height)?;
                        let observed_checksum =
                            save_argb8_slot(&output_slot, &output, width, height)?;
                        if observed_checksum != expected_checksum {
                            return Err(format!(
                                "resident output checksum mismatch: response={expected_checksum} slot={observed_checksum}"
                            ));
                        }
                        Ok(RenderResult {
                            report: serde_json::to_string_pretty(&json!({
                                "schema": "aexcompat.macos-resident-render",
                                "worker_pid": worker_pid,
                                "frame": response,
                            }))
                            .expect("resident report is serializable"),
                            output,
                        })
                    });
                    let _ = result_sender.send(result);
                }
                Ok(ResidentCommand::Close(reply)) => {
                    close_reply = Some(reply);
                    running = false;
                }
                Err(_) => running = false,
            }
        }
        let mut close_errors = Vec::new();
        let close_result = write_control_message(&mut stdin, &json!({"v": 1, "type": "close"}));
        drop(stdin);
        match close_result {
            Ok(()) => match response_receiver.recv_timeout(RESIDENT_CLOSE_DEADLINE) {
                Ok(Ok(Some(response))) => {
                    if let Err(error) = validate_resident_close(&response, worker_pid) {
                        close_errors.push(error);
                    }
                }
                Ok(Ok(None)) => {
                    close_errors.push("resident worker closed before session_closed".into())
                }
                Ok(Err(error)) => {
                    close_errors.push(format!("resident close response reader: {error}"))
                }
                Err(error) => {
                    close_errors.push(format!("resident close response timeout: {error}"))
                }
            },
            Err(error) => close_errors.push(error),
        }
        if let Err(error) = wait_or_kill_resident_child(&mut child, RESIDENT_CLOSE_DEADLINE) {
            close_errors.push(error);
        }
        let stderr = stderr_receiver
            .recv_timeout(RESIDENT_CLOSE_DEADLINE)
            .unwrap_or_else(|error| format!("stderr collection failed: {error}"));
        if !stderr.trim().is_empty() {
            close_errors.push(format!(
                "resident worker {worker_pid} stderr: {}",
                stderr.trim()
            ));
        }
        let close_outcome = if close_errors.is_empty() {
            Ok(())
        } else {
            Err(close_errors.join(" | "))
        };
        if let Some(reply) = close_reply {
            let _ = reply.send(close_outcome.clone());
        } else if let Err(error) = close_outcome {
            let _ = result_sender.send(Err(error));
        }
        let _ = std::fs::remove_file(input_slot);
        let _ = std::fs::remove_file(output_slot);
    });
    Ok(ResidentSessionHandle {
        aex: aex.to_path_buf(),
        input: input.to_path_buf(),
        sender: command_sender,
        receiver: result_receiver,
        next_frame: 0,
        join: Some(join),
    })
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
        let slot = parameter["slot"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| "parameter has no numeric slot".to_string())?;
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
            slot,
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
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                failures.push(format!(
                    "{} exited {}: {}; worker stdout: {}",
                    candidate.path.display(),
                    output.status,
                    stderr.trim(),
                    stdout.trim()
                ));
            }
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
    fn failed_worker_preserves_structured_stdout_diagnostics() {
        let workers = [GuestWorkerCandidate {
            path: PathBuf::from("/bin/sh"),
            native: false,
        }];
        let error = run_guest_workers(
            &workers,
            &[
                "-c".to_string(),
                "printf '%s' '{\"unsupported_suite_calls\":[{\"name\":\"AEGP Utility Suite\",\"version\":13,\"slot\":11,\"call_count\":1}]}' >&1; printf '%s' 'selector returned 4' >&2; exit 1".to_string(),
            ],
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(error.contains("selector returned 4"));
        assert!(error.contains(
            "worker stdout: {\"unsupported_suite_calls\":[{\"name\":\"AEGP Utility Suite\",\"version\":13,\"slot\":11,\"call_count\":1}]}"
        ));
    }

    #[test]
    fn resident_parameter_payload_preserves_slots_and_numeric_kinds() {
        let parameters = [
            GuiParameter {
                slot: 1,
                name: "Amount".into(),
                param_type: 10,
                value: 50.25,
                default_value: 5.0,
                minimum: 0.0,
                maximum: 100.0,
                precision: 2,
            },
            GuiParameter {
                slot: 2,
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
            parameter_payload(&parameters),
            "v2|param_1@1:f64=50.25;param_2@2:i32=1"
        );
    }

    #[test]
    fn resident_reader_treats_worker_eof_as_session_invalidation() {
        assert!(read_control_message(&mut &[][..]).unwrap().is_none());
    }

    #[test]
    fn resident_frame_response_is_strict_and_generation_bound() {
        let valid = json!({
            "v": 1,
            "type": "frame_done",
            "frame_index": 0,
            "status": "ok",
            "output": {
                "width": 2,
                "height": 1,
                "rowbytes": 8,
                "pixel_format": "argb8",
                "checksum": "0".repeat(64),
                "guards_intact": true
            },
            "render_error": 0,
            "generation": 1
        });
        assert!(validate_resident_frame(&valid, 0, 2, 1).is_ok());
        let mut stale = valid.clone();
        stale["generation"] = json!(0);
        assert!(validate_resident_frame(&stale, 0, 2, 1).is_err());
        let mut unknown = valid;
        unknown["unexpected"] = json!(true);
        assert!(validate_resident_frame(&unknown, 0, 2, 1).is_err());
    }

    #[test]
    fn resident_ready_response_is_strict_and_pid_bound() {
        let ready = json!({
            "v": 1,
            "type": "session_ready",
            "worker_pid": 42,
            "setup": {},
        });
        assert!(validate_resident_ready(&ready, 42).is_ok());
        assert!(validate_resident_ready(&ready, 41).is_err());
        let mut unknown = ready;
        unknown["unexpected"] = json!(true);
        assert!(validate_resident_ready(&unknown, 42).is_err());
    }

    #[test]
    fn resident_probe_response_requires_success_and_guards() {
        let probe = json!({
            "v": 1,
            "type": "session_probed",
            "worker_pid": 42,
            "status": "ok",
            "guards_intact": true,
            "render_error": 0,
        });
        assert!(validate_resident_probe(&probe, 42, 2, 1).is_ok());
        let mut corrupted = probe;
        corrupted["guards_intact"] = json!(false);
        assert!(validate_resident_probe(&corrupted, 42, 2, 1).is_err());
    }

    #[test]
    fn resident_close_timeout_terminates_the_worker() {
        let mut child = Command::new("/bin/sleep").arg("5").spawn().unwrap();
        let started = Instant::now();
        assert!(wait_or_kill_resident_child(&mut child, Duration::from_millis(20)).is_err());
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    #[ignore = "requires AEXCOMPAT_TEST_AEX and AEXCOMPAT_TEST_INPUT_PNG"]
    fn real_resident_gui_adapter_reuses_pid_and_updates_output() {
        let aex = PathBuf::from(std::env::var_os("AEXCOMPAT_TEST_AEX").unwrap());
        let input = PathBuf::from(std::env::var_os("AEXCOMPAT_TEST_INPUT_PNG").unwrap());
        let repository = repository_root().unwrap();
        let output_directory = std::env::temp_dir().join(format!(
            "aexcompat-resident-gui-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&output_directory).unwrap();
        let mut workers = guest_worker_candidates(&repository).unwrap();
        workers.insert(
            0,
            GuestWorkerCandidate {
                path: PathBuf::from("/usr/bin/false"),
                native: true,
            },
        );
        let mut session =
            start_resident_session(&workers, &aex, &input, &output_directory).unwrap();
        let parameter = |value| GuiParameter {
            slot: 5,
            name: "Amount".into(),
            param_type: 1,
            value,
            default_value: 0.0,
            minimum: 0.0,
            maximum: 4000.0,
            precision: 0,
        };
        let mut reports = Vec::new();
        let mut outputs = Vec::new();
        for (frame_index, value) in [(0, 0.0), (1, 200.0)] {
            let output = output_directory.join(format!("frame-{frame_index}.png"));
            session
                .sender
                .send(ResidentCommand::Render {
                    frame_index,
                    parameters: vec![parameter(value)],
                    output: output.clone(),
                })
                .unwrap();
            let result = session
                .receiver
                .recv_timeout(RESIDENT_RENDER_DEADLINE)
                .unwrap()
                .unwrap();
            reports.push(serde_json::from_str::<Value>(&result.report).unwrap());
            outputs.push(std::fs::read(output).unwrap());
        }
        assert_eq!(reports[0]["worker_pid"], reports[1]["worker_pid"]);
        assert_eq!(reports[0]["frame"]["generation"], 1);
        assert_eq!(reports[1]["frame"]["generation"], 2);
        assert_ne!(outputs[0], outputs[1]);
        session.shutdown().unwrap();
    }
}
