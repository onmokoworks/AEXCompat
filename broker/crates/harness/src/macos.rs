use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::gui_state::{GuiParameter, LiveRenderState, ViewerMode, reset_all};
use crate::macos_worker_controller::{
    MAX_STDERR_BYTES, ResourceLimits, SecurityTier, WorkerSession, audit_process, read_bounded,
    run_staged_setup, terminate_process_group,
};
use aexcompat_broker::render_artifacts::{
    RenderArtifactConditions, write_float32_exr_artifact, write_raw_world_artifact,
    write_raw_world_checkpoint_artifact,
};
use aexcompat_broker::render_fixture::{
    FixtureFinalArtifact, FixturePixelFormat, FixtureTiming, InteractiveParameter,
    load_render_fixture,
};
use aexcompat_broker::render_pixel_format::RenderPixelFormat;

const MAX_WIDTH: u32 = 1920;
const MAX_HEIGHT: u32 = 1080;
const NATIVE_SETUP_DEADLINE: Duration = Duration::from_secs(2);
const RESIDENT_START_DEADLINE: Duration = Duration::from_secs(10);
const RESIDENT_RENDER_DEADLINE: Duration = Duration::from_secs(30);
const RESIDENT_CLOSE_DEADLINE: Duration = Duration::from_secs(2);
const RESIDENT_RESPONSE_POLL: Duration = Duration::from_millis(10);
const MAX_RESIDENT_PROTOCOL_BYTES: usize = 1024 * 1024;

struct RenderResult {
    report: String,
    output: PathBuf,
    preview: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum MacRenderFormat {
    #[default]
    PngArgb8,
    RawArgb16,
    ExrArgb32f,
}

impl MacRenderFormat {
    fn pixel_format(self) -> &'static str {
        match self {
            Self::PngArgb8 => "argb8",
            Self::RawArgb16 => "argb16",
            Self::ExrArgb32f => "argb32f",
        }
    }

    fn bytes_per_pixel(self) -> usize {
        match self {
            Self::PngArgb8 => 4,
            Self::RawArgb16 => 8,
            Self::ExrArgb32f => 16,
        }
    }

    fn validate_bytes(self, width: u32, height: u32, bytes: &[u8]) -> Result<(), String> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(self.bytes_per_pixel()))
            .ok_or_else(|| "fixture world byte count overflow".to_string())?;
        if bytes.len() != expected {
            return Err(format!(
                "fixture world has {} bytes, expected {expected}",
                bytes.len()
            ));
        }
        Ok(())
    }

    fn artifact_format(self) -> RenderPixelFormat {
        match self {
            Self::PngArgb8 => RenderPixelFormat::Argb8,
            Self::RawArgb16 => RenderPixelFormat::Argb16,
            Self::ExrArgb32f => RenderPixelFormat::Argb32f,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResidentFrameOutput {
    width: u32,
    height: u32,
    rowbytes: u32,
    pixel_format: String,
    render_path: String,
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
    format: MacRenderFormat,
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
        || output.rowbytes != width * format.bytes_per_pixel() as u32
        || output.pixel_format != format.pixel_format()
        || !matches!(output.render_path.as_str(), "classic" | "smartfx")
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
    plugin_path: PathBuf,
    input: PathBuf,
    format: MacRenderFormat,
    sender: Sender<ResidentCommand>,
    receiver: Receiver<Result<RenderResult, String>>,
    next_frame: u64,
    child: SharedResidentChild,
    shutdown_requested: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

type SharedResidentChild = Arc<Mutex<Option<Child>>>;

struct PendingResidentRender {
    parameters: Vec<GuiParameter>,
    output: PathBuf,
    format: MacRenderFormat,
}

struct ResidentAdmissionHandle {
    plugin_path: PathBuf,
    input: PathBuf,
    format: MacRenderFormat,
    receiver: Receiver<Result<ResidentSessionHandle, String>>,
}

enum ResidentState {
    Idle,
    Starting {
        admission: ResidentAdmissionHandle,
        pending: PendingResidentRender,
    },
    Ready(ResidentSessionHandle),
    Failed,
}

impl ResidentState {
    fn is_starting(&self) -> bool {
        matches!(self, Self::Starting { .. })
    }
}

impl ResidentSessionHandle {
    fn shutdown(&mut self) -> Result<(), String> {
        self.shutdown_with_deadline(RESIDENT_CLOSE_DEADLINE)
    }

    fn shutdown_with_deadline(&mut self, deadline: Duration) -> Result<(), String> {
        let Some(join) = self.join.take() else {
            return Ok(());
        };
        self.shutdown_requested.store(true, Ordering::Release);
        let (reply_sender, reply_receiver) = mpsc::channel();
        if let Err(error) = self.sender.send(ResidentCommand::Close(reply_sender)) {
            terminate_shared_resident_child(&self.child);
            enqueue_resident_join(join);
            return Err(format!("request resident close: {error}"));
        }
        match reply_receiver.recv_timeout(deadline) {
            Ok(result) => {
                // The close outcome is sent only after protocol cleanup and child
                // ownership have been settled. Joining is never allowed to block
                // the GUI thread, even for that normally-complete case.
                enqueue_resident_join(join);
                result
            }
            Err(error) => {
                let ownership_transferred = terminate_shared_resident_child(&self.child);
                enqueue_resident_join(join);
                Err(format!(
                    "resident close exceeded the {} ms shutdown deadline: {error}; child termination {} and cleanup continues in background",
                    deadline.as_millis(),
                    if ownership_transferred {
                        "was transferred from the owner"
                    } else {
                        "remains with the controller"
                    }
                ))
            }
        }
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
    plugin_path: Option<PathBuf>,
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
    render_format: MacRenderFormat,
    resident: ResidentState,
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
            plugin_path: None,
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
            render_format: MacRenderFormat::PngArgb8,
            resident: ResidentState::Idle,
        }
    }

    fn close_resident(&mut self) -> Result<(), String> {
        match std::mem::replace(&mut self.resident, ResidentState::Idle) {
            ResidentState::Ready(mut session) => session.shutdown(),
            // Dropping the receiver is the cancellation boundary. The detached
            // admission thread owns any worker it creates and closes it if the
            // GUI no longer accepts the result.
            ResidentState::Idle | ResidentState::Starting { .. } | ResidentState::Failed => Ok(()),
        }
    }

    fn occupied(&self) -> bool {
        self.busy || self.resident.is_starting()
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
                    self.plugin_path = Some(path);
                    self.viewer_mode = ViewerMode::Input;
                }
                Err(error) => {
                    self.status = "Could not inspect AEX parameters.".into();
                    self.report = error;
                    self.parameters.clear();
                    self.plugin_path = None;
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
        let (Some(aex), Some(input)) = (self.plugin_path.clone(), self.input.clone()) else {
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
        let output = match self.render_format {
            MacRenderFormat::PngArgb8 => output_directory.join(format!("mac-aex-{nonce}.png")),
            MacRenderFormat::RawArgb16 => {
                output_directory.join(format!("mac-aex-{nonce}.raw-artifact"))
            }
            MacRenderFormat::ExrArgb32f => {
                output_directory.join(format!("mac-aex-{nonce}.exr-artifact"))
            }
        };
        let same_ready = matches!(
            &self.resident,
            ResidentState::Ready(session)
                if session.plugin_path == aex && session.input == input && session.format == self.render_format
        );
        let same_starting = matches!(
            &self.resident,
            ResidentState::Starting { admission, .. }
                if admission.plugin_path == aex && admission.input == input && admission.format == self.render_format
        );
        let pending = PendingResidentRender {
            parameters: self.parameters.clone(),
            output,
            format: self.render_format,
        };
        if same_starting {
            if let ResidentState::Starting {
                pending: queued, ..
            } = &mut self.resident
            {
                *queued = pending;
            }
            self.status = "Waiting for resident x64 guest admission...".into();
            return;
        }
        if !same_ready {
            if let Err(error) = self.close_resident() {
                self.status = "Could not cleanly replace the resident session.".into();
                self.report = error;
                return;
            }
            self.resident = ResidentState::Starting {
                admission: begin_resident_admission(
                    workers,
                    aex,
                    input,
                    output_directory,
                    self.render_format,
                ),
                pending,
            };
            self.status = "Starting and probing resident x64 guest...".into();
            self.report.clear();
            return;
        }
        self.dispatch_resident_render(pending);
    }

    fn dispatch_resident_render(&mut self, pending: PendingResidentRender) {
        let ResidentState::Ready(session) = &mut self.resident else {
            return;
        };
        if session.format != pending.format {
            self.status = "Resident format changed; reopen the session.".into();
            return;
        }
        let frame_index = session.next_frame;
        session.next_frame += 1;
        if let Err(error) = session.sender.send(ResidentCommand::Render {
            frame_index,
            parameters: pending.parameters,
            output: pending.output,
        }) {
            self.resident = ResidentState::Failed;
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
        let ready = self.plugin_path.is_some() && self.input.is_some();
        let occupied = self.occupied();
        if self.live_render.take_due(now, occupied, ready) {
            self.render();
        } else if ready
            && !occupied
            && let Some(remaining) = self.live_render.remaining(now)
        {
            ctx.request_repaint_after(remaining.min(Duration::from_millis(60)));
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let admission_result = match &self.resident {
            ResidentState::Starting { admission, .. } => match admission.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "resident admission thread stopped without a result".into(),
                )),
            },
            _ => None,
        };
        if let Some(result) = admission_result {
            let state = std::mem::replace(&mut self.resident, ResidentState::Idle);
            let ResidentState::Starting { pending, .. } = state else {
                unreachable!("only a starting admission can produce an admission result");
            };
            match result {
                Ok(session) => {
                    self.resident = ResidentState::Ready(session);
                    self.dispatch_resident_render(pending);
                }
                Err(error) => {
                    self.status = "Could not start resident guest session.".into();
                    self.report = error.clone();
                    self.resident = ResidentState::Failed;
                }
            }
        }
        let result = match &self.resident {
            ResidentState::Ready(session) => session.receiver.try_recv().ok(),
            _ => None,
        };
        let Some(result) = result else {
            if self.busy || self.resident.is_starting() {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            return;
        };
        self.busy = false;
        match result {
            Ok(result) => match result.preview.as_deref() {
                Some(preview) => match load_texture(ctx, "mac-output", preview) {
                    Ok((texture, width, height)) => {
                        let had_output = self.output_texture.is_some();
                        self.output = Some(result.output);
                        self.output_texture = Some(texture);
                        self.viewer_mode = self.viewer_mode.after_successful_render(had_output);
                        self.status = format!("Completed: {width}x{height} output");
                        self.report = result.report;
                    }
                    Err(error) => {
                        self.status =
                            "Worker completed but output preview could not be opened.".into();
                        self.report = error;
                    }
                },
                None => {
                    self.output = Some(result.output);
                    self.output_texture = None;
                    self.status = "Completed: FLOAT32 EXR artifact".into();
                    self.report = result.report;
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
                let occupied = self.occupied();
                if ui
                    .add_enabled(!occupied, egui::Button::new("AEX..."))
                    .clicked()
                {
                    self.choose_aex();
                }
                if ui
                    .add_enabled(!occupied, egui::Button::new("PNG..."))
                    .clicked()
                {
                    self.choose_input(ctx);
                }
                ui.add_enabled_ui(!occupied, |ui| {
                    egui::ComboBox::from_id_salt("mac-render-format")
                        .selected_text(match self.render_format {
                            MacRenderFormat::PngArgb8 => "ARGB8 PNG",
                            MacRenderFormat::RawArgb16 => "ARGB16 raw",
                            MacRenderFormat::ExrArgb32f => "FLOAT32 EXR",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.render_format,
                                MacRenderFormat::PngArgb8,
                                "ARGB8 PNG",
                            );
                            ui.selectable_value(
                                &mut self.render_format,
                                MacRenderFormat::ExrArgb32f,
                                "FLOAT32 EXR",
                            );
                        });
                });
                let ready = !occupied && self.plugin_path.is_some() && self.input.is_some();
                if ui.add_enabled(ready, egui::Button::new("Render")).clicked() {
                    self.render();
                }
                ui.separator();
                let mut live_render = self.live_render.enabled();
                if ui.checkbox(&mut live_render, "Auto Update").changed() {
                    self.live_render.set_enabled(live_render);
                }
                if occupied {
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
                        ("AEX", self.plugin_path.as_deref()),
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
        let occupied = self.occupied();
        ui.horizontal(|ui| {
            ui.heading(RichText::new("Effect Controls").size(20.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !occupied
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
            self.plugin_path
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
                                !occupied && !parameter.is_default(),
                                egui::Button::new("Reset").small(),
                            )
                            .clicked()
                        {
                            changed |= parameter.reset();
                        }
                    });
                    let previous_value = parameter.value;
                    let previous_color = parameter.color;
                    ui.add_enabled_ui(!occupied, |ui| match parameter.param_type {
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
                        5 => {
                            let mut rgba =
                                argb8_to_rgba8(parameter.color.unwrap_or([255, 0, 0, 0]));
                            if ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed() {
                                parameter.color = Some(rgba8_to_argb8(rgba));
                            }
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
                    changed |=
                        parameter.value != previous_value || parameter.color != previous_color;
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
                if self.occupied() {
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
    let mut total = 0;
    read_control_message_accounted(reader, &mut total)
}

fn read_control_message_accounted(
    reader: &mut impl Read,
    total: &mut usize,
) -> Result<Option<Value>, String> {
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
    *total = total
        .checked_add(length + prefix.len())
        .ok_or_else(|| "macos_worker_protocol_limit: byte accounting overflow".to_string())?;
    if *total > MAX_RESIDENT_PROTOCOL_BYTES {
        return Err(format!(
            "macos_worker_protocol_limit: resident responses exceeded {MAX_RESIDENT_PROTOCOL_BYTES} bytes"
        ));
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

fn parameter_payload(parameters: &[GuiParameter]) -> Result<String, String> {
    let assignments = parameters
        .iter()
        .map(|parameter| {
            let (kind, value) = if parameter.param_type == 5 {
                let [alpha, red, green, blue] = parameter.color.ok_or_else(|| {
                    format!("color parameter slot {} has no ARGB8 value", parameter.slot)
                })?;
                ("argb8", format!("{alpha},{red},{green},{blue}"))
            } else if matches!(parameter.param_type, 1 | 4 | 7) {
                ("i32", format!("{}", parameter.value.round() as i32))
            } else {
                ("f64", format!("{}", parameter.value))
            };
            Ok(format!(
                "param_{}@{}:{kind}={value}",
                parameter.slot, parameter.slot
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(format!("v2|{}", assignments.join(";")))
}

fn argb8_to_rgba8([alpha, red, green, blue]: [u8; 4]) -> [u8; 4] {
    [red, green, blue, alpha]
}

fn rgba8_to_argb8([red, green, blue, alpha]: [u8; 4]) -> [u8; 4] {
    [alpha, red, green, blue]
}

fn write_resident_input_slot(
    input: &Path,
    slot: &Path,
    format: MacRenderFormat,
) -> Result<(u32, u32), String> {
    let (width, height, argb) = encode_resident_image(input, format)?;
    std::fs::write(slot, argb).map_err(|error| format!("write resident input slot: {error}"))?;
    Ok((width, height))
}

fn encode_resident_image(
    input: &Path,
    format: MacRenderFormat,
) -> Result<(u32, u32, Vec<u8>), String> {
    if format == MacRenderFormat::PngArgb8 {
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
        return Ok((width, height, argb));
    }
    let image = image::open(input)
        .map_err(|error| format!("open resident input PNG: {error}"))?
        .to_rgba8();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err(format!(
            "resident input dimensions must be 1x1..={MAX_WIDTH}x{MAX_HEIGHT}; got {width}x{height}"
        ));
    }
    let mut argb = Vec::with_capacity(width as usize * height as usize * format.bytes_per_pixel());
    for pixel in image.as_raw().chunks_exact(4) {
        for sample in [pixel[3], pixel[0], pixel[1], pixel[2]] {
            match format {
                MacRenderFormat::RawArgb16 => {
                    let ae_word = (u32::from(sample) * 32768 + 127) / 255;
                    argb.extend_from_slice(&(ae_word as u16).to_le_bytes());
                }
                MacRenderFormat::ExrArgb32f => {
                    argb.extend_from_slice(&(f32::from(sample) / 255.0).to_le_bytes());
                }
                MacRenderFormat::PngArgb8 => unreachable!(),
            }
        }
    }
    Ok((width, height, argb))
}

fn argb_to_rgba_words(bytes: &[u8], component_bytes: usize) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(bytes.len());
    for pixel in bytes.chunks_exact(component_bytes * 4) {
        rgba.extend_from_slice(&pixel[component_bytes..component_bytes * 4]);
        rgba.extend_from_slice(&pixel[..component_bytes]);
    }
    rgba
}

fn save_argb8_slot(
    slot: &Path,
    output: &Path,
    width: u32,
    height: u32,
    expected_checksum: &str,
) -> Result<String, String> {
    let argb =
        std::fs::read(slot).map_err(|error| format!("read resident output slot: {error}"))?;
    let expected = width as usize * height as usize * 4;
    if argb.len() != expected {
        return Err(format!(
            "resident output has {} bytes, expected {expected}",
            argb.len()
        ));
    }
    let checksum = format!("{:x}", Sha256::digest(&argb));
    if checksum != expected_checksum {
        return Err(format!(
            "resident output checksum mismatch: response={expected_checksum} slot={checksum}"
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
    Ok(checksum)
}

fn save_argb32f_exr_slot(
    slot: &Path,
    directory: &Path,
    width: u32,
    height: u32,
    plugin_sha256: &str,
    input_sha256: &str,
    parameters: &str,
    render_path: &str,
    expected_checksum: &str,
) -> Result<String, String> {
    let argb = std::fs::read(slot)
        .map_err(|error| format!("read resident ARGB32F output slot: {error}"))?;
    let expected = width as usize * height as usize * 16;
    if argb.len() != expected {
        return Err(format!(
            "resident ARGB32F output has {} bytes, expected {expected}",
            argb.len()
        ));
    }
    let world_sha256 = format!("{:x}", Sha256::digest(&argb));
    if world_sha256 != expected_checksum {
        return Err(format!(
            "resident output checksum mismatch: response={expected_checksum} slot={world_sha256}"
        ));
    }
    let mut rgba = Vec::with_capacity(argb.len());
    for pixel in argb.chunks_exact(16) {
        rgba.extend_from_slice(&pixel[4..16]);
        rgba.extend_from_slice(&pixel[0..4]);
    }
    let conditions = RenderArtifactConditions {
        premultiplication: "premultiplied".into(),
        working_space: "None".into(),
        render_mode: "software".into(),
        comparison_identity: json!({
            "plugin_sha256": plugin_sha256,
            "input_sha256": input_sha256,
            "world_sha256": world_sha256,
            "render_path": render_path,
            "pixel_format": "argb32f",
            "timing": {"current_time": 0, "time_step": 1, "total_time": 1, "time_scale": 30},
            "requested_parameters": [parameters],
            "origin": {"x": 0, "y": 0}
        }),
    };
    write_float32_exr_artifact(directory, &rgba, width, height, 0, 0, conditions)
        .map_err(|error| format!("write FLOAT32 EXR artifact: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(&argb)))
}

struct StartedResidentWorker {
    child: Child,
    stdin: ChildStdin,
    response_receiver: Receiver<Result<Option<Value>, String>>,
    stderr_receiver: Receiver<Result<String, String>>,
    worker_pid: u32,
    session: WorkerSession,
    output_slot: PathBuf,
    width: u32,
    height: u32,
    security_tier: SecurityTier,
    fallback_reasons: Vec<String>,
    plugin_sha256: String,
    input_sha256: String,
    additional_session_files: usize,
    fixture_input_world: Option<PathBuf>,
    fixture_layer_worlds: Vec<(u32, u32, u32, PathBuf)>,
}

struct MacFixtureLaunch<'a> {
    layers: &'a [(u32, PathBuf)],
    smart: bool,
    time_scale: u32,
}

#[derive(Serialize)]
struct StagedLayerManifest<'a> {
    v: u32,
    layers: &'a [StagedLayerEntry],
}

#[derive(Serialize)]
struct StagedLayerEntry {
    slot: u32,
    width: u32,
    height: u32,
    path: PathBuf,
}

fn start_resident_worker(
    candidates: &[GuestWorkerCandidate],
    aex: &Path,
    input: &Path,
    format: MacRenderFormat,
    fixture: Option<&MacFixtureLaunch<'_>>,
) -> Result<StartedResidentWorker, String> {
    let mut failures = Vec::new();
    for candidate in candidates {
        let mut probe_worker =
            match launch_resident_candidate(candidate, aex, input, format, fixture) {
                Ok(worker) => worker,
                Err(error) => {
                    failures.push(format!(
                        "{} ({}): {error}",
                        candidate.path.display(),
                        if candidate.native {
                            "native"
                        } else {
                            "fallback"
                        },
                    ));
                    continue;
                }
            };
        let admission =
            write_control_message(&mut probe_worker.stdin, &json!({"v": 1, "type": "probe"}))
                .and_then(|()| {
                    let response = probe_worker
                        .response_receiver
                        .recv_timeout(RESIDENT_RENDER_DEADLINE)
                        .map_err(|error| format!("probe response timeout: {error}"))?
                        .map_err(|error| format!("probe response reader: {error}"))?
                        .ok_or_else(|| "worker closed before session_probed".to_string())?;
                    validate_resident_probe(
                        &response,
                        probe_worker.worker_pid,
                        probe_worker.width,
                        probe_worker.height,
                    )
                });
        if let Err(error) = admission {
            let stderr = terminate_resident_worker(probe_worker);
            failures.push(format!(
                "{} ({}): {error}{}",
                candidate.path.display(),
                if candidate.native {
                    "native"
                } else {
                    "fallback"
                },
                stderr
                    .filter(|stderr| !stderr.trim().is_empty())
                    .map(|stderr| format!("; stderr: {}", stderr.trim()))
                    .unwrap_or_default(),
            ));
            continue;
        }
        if let Err(error) = close_probe_worker(probe_worker) {
            failures.push(format!(
                "{} ({}): probe cleanup failed: {error}",
                candidate.path.display(),
                if candidate.native {
                    "native"
                } else {
                    "fallback"
                },
            ));
            continue;
        }
        match launch_resident_candidate(candidate, aex, input, format, fixture) {
            Ok(mut fresh_worker) => {
                fresh_worker.fallback_reasons = failures;
                return Ok(fresh_worker);
            }
            Err(error) => {
                failures.push(format!(
                    "{} ({}): fresh session after probe failed: {error}",
                    candidate.path.display(),
                    if candidate.native {
                        "native"
                    } else {
                        "fallback"
                    },
                ));
            }
        }
    }
    Err(format!(
        "all resident guest workers failed readiness: {}",
        failures.join(" | ")
    ))
}

fn launch_resident_candidate(
    candidate: &GuestWorkerCandidate,
    aex: &Path,
    input: &Path,
    format: MacRenderFormat,
    fixture: Option<&MacFixtureLaunch<'_>>,
) -> Result<StartedResidentWorker, String> {
    let session = WorkerSession::create()?;
    let worker = session.stage_file(&candidate.path, "worker")?;
    let plugin = session.stage_file(aex, "plugin.aex")?;
    let input_slot = session
        .root()
        .join(format!("input.{}", format.pixel_format()));
    let output_slot = session
        .root()
        .join(format!("output.{}", format.pixel_format()));
    let (width, height) = write_resident_input_slot(input, &input_slot, format)?;
    let plugin_sha256 = format!(
        "{:x}",
        Sha256::digest(
            std::fs::read(&plugin).map_err(|error| format!("hash staged AEX: {error}"))?
        )
    );
    let input_sha256 = format!(
        "{:x}",
        Sha256::digest(
            std::fs::read(&input_slot)
                .map_err(|error| format!("hash staged resident input: {error}"))?
        )
    );
    std::fs::write(
        &output_slot,
        vec![0u8; width as usize * height as usize * format.bytes_per_pixel()],
    )
    .map_err(|error| format!("initialize resident output slot: {error}"))?;
    let mut arguments = vec![
        "session".to_string(),
        plugin.to_string_lossy().into_owned(),
        input_slot.to_string_lossy().into_owned(),
        output_slot.to_string_lossy().into_owned(),
        width.to_string(),
        height.to_string(),
        fixture.map_or(30, |fixture| fixture.time_scale).to_string(),
        "--pixel-format".to_string(),
        format.pixel_format().to_string(),
    ];
    let mut fixture_layer_worlds = Vec::new();
    if let Some(fixture) = fixture {
        let mut staged_layers = Vec::with_capacity(fixture.layers.len());
        for (slot, source) in fixture.layers {
            let path = session
                .root()
                .join(format!("layer-{slot}.{}", format.pixel_format()));
            let (layer_width, layer_height) = write_resident_input_slot(source, &path, format)?;
            staged_layers.push(StagedLayerEntry {
                slot: *slot,
                width: layer_width,
                height: layer_height,
                path,
            });
            fixture_layer_worlds.push((
                *slot,
                layer_width,
                layer_height,
                session
                    .root()
                    .join(format!("fixture-layer-slot{slot}-world.bin")),
            ));
        }
        let manifest_path = session.root().join("fixture-layers-v1.json");
        let manifest = serde_json::to_vec(&StagedLayerManifest {
            v: 1,
            layers: &staged_layers,
        })
        .map_err(|error| format!("serialize fixture layer manifest: {error}"))?;
        std::fs::write(&manifest_path, manifest)
            .map_err(|error| format!("write fixture layer manifest: {error}"))?;
        arguments.extend([
            "--fixture-layers-v1".into(),
            manifest_path.to_string_lossy().into_owned(),
            "--fixture-render-path".into(),
            if fixture.smart { "smart" } else { "classic" }.into(),
        ]);
    }
    let mut command = session.command(
        &worker,
        candidate.security_tier(),
        ResourceLimits::default(),
    );
    let mut child = command
        .args(arguments)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("macos_worker_launch: {error}"))?;
    let worker_pid = child.id();
    let (Some(stdin), Some(mut stdout), Some(mut stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        let _ = terminate_process_group(&mut child);
        return Err("one or more resident worker pipes are unavailable".into());
    };
    let (response_sender, response_receiver) = mpsc::channel();
    let (stderr_sender, stderr_receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut protocol_bytes = 0;
        loop {
            let response = read_control_message_accounted(&mut stdout, &mut protocol_bytes);
            let terminal = !matches!(response, Ok(Some(_)));
            if response_sender.send(response).is_err() || terminal {
                break;
            }
        }
    });
    thread::spawn(move || {
        let text =
            read_bounded(&mut stderr, MAX_STDERR_BYTES, "resident stderr").and_then(|bytes| {
                String::from_utf8(bytes)
                    .map_err(|error| format!("resident stderr is not UTF-8: {error}"))
            });
        let _ = stderr_sender.send(text);
    });
    let ready = response_receiver
        .recv_timeout(RESIDENT_START_DEADLINE)
        .map_err(|error| format!("ready response timeout: {error}"))
        .and_then(|response| response.map_err(|error| format!("ready response reader: {error}")))
        .and_then(|response| {
            response.ok_or_else(|| "worker closed before session_ready".to_string())
        })
        .and_then(|response| validate_resident_ready(&response, worker_pid));
    if let Err(error) = ready {
        drop(stdin);
        let _ = terminate_process_group(&mut child);
        let stderr = stderr_receiver
            .recv_timeout(Duration::from_millis(200))
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        return Err(format!(
            "{error}{}",
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!("; stderr: {}", stderr.trim())
            }
        ));
    }
    let additional_session_files = fixture.map_or(0, |fixture| fixture.layers.len() * 2 + 2);
    if let Err(error) = session.audit_tree_with_additional_files(additional_session_files) {
        drop(stdin);
        let _ = terminate_process_group(&mut child);
        return Err(error);
    }
    let fixture_input_world = fixture.map(|_| session.root().join("fixture-input-world.bin"));
    Ok(StartedResidentWorker {
        child,
        stdin,
        response_receiver,
        stderr_receiver,
        worker_pid,
        session,
        output_slot,
        width,
        height,
        security_tier: candidate.security_tier(),
        fallback_reasons: Vec::new(),
        plugin_sha256,
        input_sha256,
        additional_session_files,
        fixture_input_world,
        fixture_layer_worlds,
    })
}

fn terminate_resident_worker(mut worker: StartedResidentWorker) -> Option<String> {
    drop(worker.stdin);
    let _ = terminate_process_group(&mut worker.child);
    worker
        .stderr_receiver
        .recv_timeout(Duration::from_millis(200))
        .ok()
        .and_then(Result::ok)
}

fn close_probe_worker(mut worker: StartedResidentWorker) -> Result<(), String> {
    let mut errors = Vec::new();
    let close_request = write_control_message(&mut worker.stdin, &json!({"v": 1, "type": "close"}));
    drop(worker.stdin);
    match close_request {
        Ok(()) => match worker
            .response_receiver
            .recv_timeout(RESIDENT_CLOSE_DEADLINE)
        {
            Ok(Ok(Some(response))) => {
                if let Err(error) = validate_resident_close(&response, worker.worker_pid) {
                    errors.push(error);
                }
            }
            Ok(Ok(None)) => errors.push("probe worker closed before session_closed".into()),
            Ok(Err(error)) => errors.push(format!("probe close response reader: {error}")),
            Err(error) => errors.push(format!("probe close response timeout: {error}")),
        },
        Err(error) => errors.push(error),
    }
    if let Err(error) = wait_or_kill_resident_child(worker.child, RESIDENT_CLOSE_DEADLINE) {
        errors.push(error);
    }
    let stderr = worker
        .stderr_receiver
        .recv_timeout(RESIDENT_CLOSE_DEADLINE)
        .unwrap_or_else(|error| Err(format!("probe stderr collection failed: {error}")))
        .unwrap_or_else(|error| error);
    if !stderr.trim().is_empty() {
        errors.push(format!("probe worker stderr: {}", stderr.trim()));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join(" | "))
    }
}

struct FixtureParameterPayload {
    transport: String,
    identity: Value,
}

fn fixture_parameter_payload(
    parameters: &[InteractiveParameter],
) -> Result<FixtureParameterPayload, String> {
    let mut assignments = Vec::new();
    let mut identity = Vec::new();
    let mut component_payload = false;
    for parameter in parameters {
        let (encoded, kind, value) = match parameter.kind.as_str() {
            "layer" | "group_start" | "group_end" | "button" | "custom" | "no_data" => {
                continue;
            }
            "integer" | "path" => {
                if !parameter.value.is_finite() || parameter.value.fract() != 0.0 {
                    return Err(format!(
                        "fixture scalar slot {} requires an integer value",
                        parameter.slot
                    ));
                }
                (
                    format!("i32={}", parameter.value as i64),
                    "integer",
                    json!(parameter.value as i64),
                )
            }
            "float" => {
                if !parameter.value.is_finite() {
                    return Err(format!(
                        "fixture scalar slot {} requires a finite value",
                        parameter.slot
                    ));
                }
                (
                    format!("f64={}", parameter.value),
                    "float",
                    json!(parameter.value),
                )
            }
            "color" => (
                format!(
                    "argb8={},{},{},{}",
                    parameter.color[0], parameter.color[1], parameter.color[2], parameter.color[3]
                ),
                "color",
                json!({
                    "alpha": parameter.color[0],
                    "red": parameter.color[1],
                    "green": parameter.color[2],
                    "blue": parameter.color[3]
                }),
            ),
            "angle" | "point" | "point3d" => {
                let expected = match parameter.kind.as_str() {
                    "angle" => 1,
                    "point" => 2,
                    _ => 3,
                };
                if parameter.component_count != expected
                    || parameter.components[..expected]
                        .iter()
                        .any(|value| !value.is_finite() || !(-32768.0..=32768.0).contains(value))
                {
                    return Err(format!(
                        "fixture component slot {} is invalid",
                        parameter.slot
                    ));
                }
                component_payload = true;
                let components = parameter.components[..expected].to_vec();
                (
                    format!(
                        "{}={}",
                        parameter.kind,
                        components
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                    parameter.kind.as_str(),
                    json!(components),
                )
            }
            other => {
                return Err(format!(
                    "macOS fixture parameter kind {other:?} is not supported"
                ));
            }
        };
        if parameter.slot == 0
            || parameter.value < parameter.minimum
            || parameter.value > parameter.maximum
        {
            return Err(format!(
                "fixture scalar slot {} is outside its declared range",
                parameter.slot
            ));
        }
        assignments.push(format!(
            "param_{}@{}:{encoded}",
            parameter.slot, parameter.slot
        ));
        identity.push(json!({
            "id": format!("param_{}", parameter.slot),
            "slot": parameter.slot,
            "kind": kind,
            "value": value
        }));
    }
    Ok(FixtureParameterPayload {
        transport: format!(
            "{}|{}",
            if component_payload { "v4" } else { "v2" },
            assignments.join(";")
        ),
        identity: Value::Array(identity),
    })
}

fn fixture_staging_path(output: &Path) -> Result<PathBuf, String> {
    let parent = output
        .parent()
        .ok_or_else(|| "fixture output has no parent".to_string())?;
    let name = output
        .file_name()
        .ok_or_else(|| "fixture output has no name".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create fixture output parent: {error}"))?;
    for nonce in 0..1024u32 {
        let candidate = parent.join(format!(
            ".{}.fixture-tmp-{}-{nonce}",
            name.to_string_lossy(),
            std::process::id()
        ));
        if !candidate.exists() {
            std::fs::create_dir(&candidate)
                .map_err(|error| format!("create fixture staging directory: {error}"))?;
            return Ok(candidate);
        }
    }
    Err("no fixture staging name available".into())
}

fn fixture_conditions(
    plugin_sha256: &str,
    input_sha256: &str,
    world: &[u8],
    pixel_format: &str,
    render_path: &str,
    requested_parameters: &Value,
    premultiplication: &str,
    current_time: i32,
    time_step: i32,
    total_time: i32,
    time_scale: u32,
) -> RenderArtifactConditions {
    RenderArtifactConditions {
        premultiplication: premultiplication.into(),
        working_space: "None".into(),
        render_mode: "software".into(),
        comparison_identity: json!({
            "plugin_sha256": plugin_sha256,
            "input_sha256": input_sha256,
            "world_sha256": format!("{:x}", Sha256::digest(world)),
            "render_path": render_path,
            "pixel_format": pixel_format,
            "timing": {
                "current_time": current_time,
                "time_step": time_step,
                "total_time": total_time,
                "time_scale": time_scale
            },
            "requested_parameters": requested_parameters,
            "origin": {"x": 0, "y": 0}
        }),
    }
}

fn fixture_render_request(timing: &FixtureTiming, parameters: &str) -> Value {
    json!({
        "v": 4,
        "type": "render_frame",
        "frame_index": 0,
        "current_time": {
            "value": timing.current_time,
            "step": timing.time_step,
            "total": timing.total_time,
            "scale": timing.time_scale
        },
        "parameters": parameters
    })
}

pub fn render_fixture_headless(
    repository: &Path,
    aex: &Path,
    fixture_path: &Path,
    output_directory: &Path,
) -> Result<Value, String> {
    if output_directory.exists() {
        return Err("fixture output exists".into());
    }
    let loaded = load_render_fixture(fixture_path)
        .map_err(|error| format!("load declarative fixture: {error}"))?;
    let fixture = &loaded.document;
    let format = match fixture.pixel_format {
        FixturePixelFormat::Argb8 => MacRenderFormat::PngArgb8,
        FixturePixelFormat::Argb16 => MacRenderFormat::RawArgb16,
        FixturePixelFormat::Argb32f => MacRenderFormat::ExrArgb32f,
    };
    let parameters = fixture_parameter_payload(&loaded.parameters)?;
    let layer_paths = loaded
        .parameters
        .iter()
        .filter(|parameter| parameter.kind == "layer")
        .filter_map(|parameter| {
            parameter
                .layer_path
                .as_ref()
                .map(|path| (parameter.slot, path.clone()))
        })
        .collect::<Vec<_>>();
    if layer_paths.len() > 8 {
        return Err("fixture secondary layer count exceeds 8".into());
    }
    let launch = MacFixtureLaunch {
        layers: &layer_paths,
        smart: fixture.render_path == "smart",
        time_scale: fixture.timing.time_scale,
    };
    let workers = guest_worker_candidates(repository)?;
    let mut started =
        start_resident_worker(&workers, aex, &loaded.primary_layer, format, Some(&launch))?;
    let primary_width = started.width;
    let primary_height = started.height;
    let request = fixture_render_request(&fixture.timing, &parameters.transport);
    if let Err(error) = write_control_message(&mut started.stdin, &request) {
        let _ = close_probe_worker(started);
        return Err(error);
    }
    let response = match started
        .response_receiver
        .recv_timeout(RESIDENT_RENDER_DEADLINE)
    {
        Ok(Ok(Some(response))) => response,
        Ok(Ok(None)) => {
            let _ = close_probe_worker(started);
            return Err("resident worker closed before fixture frame response".into());
        }
        Ok(Err(error)) => {
            let _ = close_probe_worker(started);
            return Err(format!("resident fixture response reader: {error}"));
        }
        Err(error) => {
            let _ = close_probe_worker(started);
            return Err(format!("resident fixture response timeout: {error}"));
        }
    };
    let plugin_sha256 = started.plugin_sha256.clone();
    let input_sha256 = started.input_sha256.clone();
    let inspected = (|| -> Result<_, String> {
        let expected_checksum =
            validate_resident_frame(&response, 0, primary_width, primary_height, format)?;
        let actual_path = response["output"]["render_path"]
            .as_str()
            .ok_or_else(|| "fixture response has no render path".to_string())?;
        let expected_path = if fixture.render_path == "smart" {
            "smartfx"
        } else {
            "classic"
        };
        if actual_path != expected_path {
            return Err(format!(
                "fixture requested {} but worker reported {actual_path}",
                fixture.render_path
            ));
        }
        started
            .session
            .audit_tree_with_additional_files(started.additional_session_files)?;
        let output = std::fs::read(&started.output_slot)
            .map_err(|error| format!("read fixture output slot: {error}"))?;
        if format!("{:x}", Sha256::digest(&output)) != expected_checksum {
            return Err("fixture output checksum differs from worker response".into());
        }
        let input_path = started
            .fixture_input_world
            .as_ref()
            .ok_or_else(|| "fixture input world dump path is unavailable".to_string())?;
        let input_world = std::fs::read(input_path)
            .map_err(|error| format!("read fixture input world dump: {error}"))?;
        format
            .validate_bytes(primary_width, primary_height, &input_world)
            .map_err(|error| format!("validate fixture input world dump: {error}"))?;
        let mut layer_worlds = Vec::with_capacity(started.fixture_layer_worlds.len());
        for (slot, width, height, path) in &started.fixture_layer_worlds {
            let bytes = std::fs::read(path)
                .map_err(|error| format!("read fixture layer slot {slot} world dump: {error}"))?;
            format
                .validate_bytes(*width, *height, &bytes)
                .map_err(|error| {
                    format!("validate fixture layer slot {slot} world dump: {error}")
                })?;
            layer_worlds.push((*slot, *width, *height, bytes));
        }
        Ok((output, input_world, layer_worlds))
    })();
    let close = close_probe_worker(started);
    let (output_argb, primary_argb, layer_worlds) = inspected?;
    close?;

    let staging = fixture_staging_path(output_directory)?;
    let result = (|| -> Result<Value, String> {
        let component_bytes = format.bytes_per_pixel() / 4;
        let artifact_render_path = if fixture.render_path == "smart" {
            "smartfx"
        } else {
            "classic"
        };
        let output_rgba = argb_to_rgba_words(&output_argb, component_bytes);
        let conditions = fixture_conditions(
            &plugin_sha256,
            &input_sha256,
            &output_argb,
            format.pixel_format(),
            artifact_render_path,
            &parameters.identity,
            &fixture.premultiplication,
            fixture.timing.current_time,
            fixture.timing.time_step,
            fixture.timing.total_time,
            fixture.timing.time_scale,
        );
        let final_metadata = match fixture.final_artifact {
            FixtureFinalArtifact::Raw => write_raw_world_artifact(
                &staging.join("final"),
                &output_rgba,
                primary_width,
                primary_height,
                format.artifact_format(),
                0,
                0,
                conditions.clone(),
            ),
            FixtureFinalArtifact::Exr => write_float32_exr_artifact(
                &staging.join("final"),
                &output_rgba,
                primary_width,
                primary_height,
                0,
                0,
                conditions.clone(),
            ),
        }
        .map_err(|error| format!("write fixture final artifact: {error}"))?;

        let mut checkpoint_reports = serde_json::Map::new();
        for checkpoint in &fixture.checkpoints {
            let suffix = checkpoint
                .stage
                .strip_prefix(&format!("{}-", fixture.render_path))
                .ok_or_else(|| "checkpoint stage does not match render path".to_string())?;
            let (width, height, argb) = match suffix {
                "input" => (primary_width, primary_height, primary_argb.as_slice()),
                "output" => (primary_width, primary_height, output_argb.as_slice()),
                layer if layer.starts_with("layer-slot") => {
                    let slot = layer["layer-slot".len()..]
                        .parse::<u32>()
                        .map_err(|_| "checkpoint layer slot is invalid".to_string())?;
                    let (_, width, height, bytes) = layer_worlds
                        .iter()
                        .find(|(candidate, _, _, _)| *candidate == slot)
                        .ok_or_else(|| {
                            format!("requested checkpoint layer slot {slot} was not staged")
                        })?;
                    (*width, *height, bytes.as_slice())
                }
                _ => return Err("checkpoint stage is unsupported".into()),
            };
            let rgba = argb_to_rgba_words(argb, component_bytes);
            let checkpoint_conditions = fixture_conditions(
                &plugin_sha256,
                &input_sha256,
                argb,
                format.pixel_format(),
                artifact_render_path,
                &parameters.identity,
                &fixture.premultiplication,
                fixture.timing.current_time,
                fixture.timing.time_step,
                fixture.timing.total_time,
                fixture.timing.time_scale,
            );
            let metadata = write_raw_world_checkpoint_artifact(
                &staging.join("checkpoints").join(&checkpoint.id),
                &rgba,
                width,
                height,
                format.artifact_format(),
                0,
                0,
                checkpoint_conditions,
                &checkpoint.id,
                &checkpoint.stage,
                &loaded.sha256,
            )
            .map_err(|error| format!("write fixture checkpoint: {error}"))?;
            checkpoint_reports.insert(checkpoint.id.clone(), metadata);
        }
        std::fs::rename(&staging, output_directory)
            .map_err(|error| format!("publish fixture output: {error}"))?;
        Ok(json!({
            "schema": "aexcompat.render_fixture_report",
            "schema_version": 1,
            "pixel_format": format.pixel_format(),
            "render_path": fixture.render_path,
            "final_artifact": final_metadata,
            "checkpoints": checkpoint_reports
        }))
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn wait_or_kill_resident_child(
    mut child: Child,
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
                let termination = terminate_process_group(&mut child);
                return Err(format!(
                    "resident worker exceeded the {} ms close deadline; process-group cleanup: {}",
                    deadline.as_millis(),
                    termination
                        .map(|()| "complete".to_string())
                        .unwrap_or_else(|error| error)
                ));
            }
        }
    }
}

fn spawn_resident_admission<T, F>(start: F) -> Receiver<Result<T, String>>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        // If the GUI has moved to another AEX/input while admission was in
        // flight, send failure drops the completed session here. Its Drop
        // implementation performs the normal bounded cleanup.
        let _ = sender.send(start());
    });
    receiver
}

fn begin_resident_admission(
    candidates: Vec<GuestWorkerCandidate>,
    aex: PathBuf,
    input: PathBuf,
    output_directory: PathBuf,
    format: MacRenderFormat,
) -> ResidentAdmissionHandle {
    let plugin_path = aex.clone();
    let admission_input = input.clone();
    let receiver = spawn_resident_admission(move || {
        start_resident_session(&candidates, &aex, &input, &output_directory, format)
    });
    ResidentAdmissionHandle {
        plugin_path,
        input: admission_input,
        format,
        receiver,
    }
}

fn recv_resident_response(
    receiver: &Receiver<Result<Option<Value>, String>>,
    child: &SharedResidentChild,
    shutdown_requested: &AtomicBool,
    deadline: Duration,
) -> Result<Option<Value>, String> {
    let started = Instant::now();
    loop {
        if shutdown_requested.load(Ordering::Acquire) {
            terminate_shared_resident_child(child);
            return Err("resident render cancelled for session shutdown".into());
        }
        let worker_pid = match child.lock() {
            Ok(child) => child.as_ref().map(Child::id),
            Err(error) => error.into_inner().as_ref().map(Child::id),
        };
        if let Some(worker_pid) = worker_pid
            && let Err(error) = audit_process(worker_pid)
        {
            terminate_shared_resident_child(child);
            return Err(error);
        }
        let remaining = deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            terminate_shared_resident_child(child);
            return Err(format!(
                "resident render response timeout after {} ms",
                deadline.as_millis()
            ));
        }
        match receiver.recv_timeout(remaining.min(RESIDENT_RESPONSE_POLL)) {
            Ok(Ok(response)) => return Ok(response),
            Ok(Err(error)) => {
                terminate_shared_resident_child(child);
                return Err(error);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                terminate_shared_resident_child(child);
                return Err("resident response reader disconnected".into());
            }
        }
    }
}

fn start_resident_session(
    candidates: &[GuestWorkerCandidate],
    aex: &Path,
    input: &Path,
    _output_directory: &Path,
    format: MacRenderFormat,
) -> Result<ResidentSessionHandle, String> {
    let started = start_resident_worker(candidates, aex, input, format, None)?;
    let plugin_sha256 = started.plugin_sha256.clone();
    let input_sha256 = started.input_sha256.clone();
    let width = started.width;
    let height = started.height;
    let output_slot = started.output_slot.clone();
    let session = started.session;
    let security_tier = started.security_tier;
    let fallback_reasons = started.fallback_reasons;
    let child = Arc::new(Mutex::new(Some(started.child)));
    let controller_child = Arc::clone(&child);
    let worker_pid = started.worker_pid;
    let mut stdin = started.stdin;
    let response_receiver = started.response_receiver;
    let stderr_receiver = started.stderr_receiver;
    let (command_sender, command_receiver) = mpsc::channel();
    let (result_sender, result_receiver) = mpsc::channel();
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let worker_shutdown_requested = Arc::clone(&shutdown_requested);
    let join = thread::spawn(move || {
        // Session ownership follows the controller thread so all exits remove
        // staged worker/plugin/slots only after process-group cleanup.
        let mut session = session;
        let mut running = true;
        let mut close_reply = None;
        while running {
            match command_receiver.recv() {
                Ok(ResidentCommand::Render {
                    frame_index,
                    parameters,
                    output,
                }) => {
                    let result = parameter_payload(&parameters).and_then(|payload| {
                        let request = json!({
                        "v": 2,
                        "type": "render_frame",
                        "frame_index": frame_index,
                        "current_time": {"value": 0, "scale": 30},
                        "parameters": payload,
                        });
                        write_control_message(&mut stdin, &request).map(|()| payload)
                    }).and_then(|payload| {
                        let response = recv_resident_response(
                            &response_receiver,
                            &controller_child,
                            &worker_shutdown_requested,
                            RESIDENT_RENDER_DEADLINE,
                        )?
                            .ok_or_else(|| "resident worker closed stdout".to_string())?;
                        let expected_checksum =
                            validate_resident_frame(&response, frame_index, width, height, format)?;
                        let render_path = response["output"]["render_path"]
                            .as_str()
                            .expect("validated resident render path");
                        session.audit_tree()?;
                        let (observed_checksum, final_output, preview) = match format {
                            MacRenderFormat::PngArgb8 => (
                                save_argb8_slot(
                                    &output_slot,
                                    &output,
                                    width,
                                    height,
                                    &expected_checksum,
                                )?,
                                output.clone(),
                                Some(output.clone()),
                            ),
                            MacRenderFormat::ExrArgb32f => {
                                let checksum = save_argb32f_exr_slot(
                                    &output_slot,
                                    &output,
                                    width,
                                    height,
                                    &plugin_sha256,
                                    &input_sha256,
                                    &payload,
                                    render_path,
                                    &expected_checksum,
                                )?;
                                (checksum, output.join("output.exr"), None)
                            }
                            MacRenderFormat::RawArgb16 => {
                                return Err(
                                    "ARGB16 raw is available through --render-fixture only"
                                        .into(),
                                );
                            }
                        };
                        debug_assert_eq!(observed_checksum, expected_checksum);
                        Ok(RenderResult {
                            report: serde_json::to_string_pretty(&json!({
                                "schema": "aexcompat.macos-resident-render",
                                "worker_pid": worker_pid,
                                "security_tier": security_tier.as_str(),
                                "native_carrier": security_tier == SecurityTier::NativeCarrierTrustedOnly,
                                "native_carrier_trusted_only": security_tier == SecurityTier::NativeCarrierTrustedOnly,
                                "fallback_reasons": fallback_reasons,
                                "frame": response,
                            }))
                            .expect("resident report is serializable"),
                            output: final_output,
                            preview,
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
        if let Some(child) = take_shared_resident_child(&controller_child)
            && let Err(error) = wait_or_kill_resident_child(child, RESIDENT_CLOSE_DEADLINE)
        {
            close_errors.push(error);
        }
        let stderr = stderr_receiver
            .recv_timeout(RESIDENT_CLOSE_DEADLINE)
            .unwrap_or_else(|error| Err(format!("stderr collection failed: {error}")))
            .unwrap_or_else(|error| error);
        if !stderr.trim().is_empty() {
            close_errors.push(format!(
                "resident worker {worker_pid} stderr: {}",
                stderr.trim()
            ));
        }
        if let Err(error) = session.cleanup() {
            close_errors.push(error);
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
    });
    Ok(ResidentSessionHandle {
        plugin_path: aex.to_path_buf(),
        input: input.to_path_buf(),
        format,
        sender: command_sender,
        receiver: result_receiver,
        next_frame: 0,
        child,
        shutdown_requested,
        join: Some(join),
    })
}

fn discover_parameters(
    repository: &Path,
    aex: &Path,
) -> Result<(Vec<GuiParameter>, String), String> {
    let workers = guest_worker_candidates(repository)?;
    let mut failures = Vec::new();
    let mut process = None;
    let mut selected_tier = None;
    for candidate in &workers {
        let deadline = if candidate.native {
            NATIVE_SETUP_DEADLINE
        } else {
            RESIDENT_RENDER_DEADLINE
        };
        match run_staged_setup(&candidate.path, aex, candidate.security_tier(), deadline) {
            Ok(output) if output.status.success() => {
                process = Some(output);
                selected_tier = Some(candidate.security_tier());
                break;
            }
            Ok(output) => failures.push(format!(
                "{} [{}] exited {}: {}; worker stdout: {}",
                candidate.path.display(),
                candidate.security_tier().as_str(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim(),
                String::from_utf8_lossy(&output.stdout).trim(),
            )),
            Err(error) => failures.push(format!(
                "{} [{}]: {error}",
                candidate.path.display(),
                candidate.security_tier().as_str(),
            )),
        }
    }
    let process = process
        .ok_or_else(|| format!("all staged setup workers failed: {}", failures.join(" | ")))?;
    let report = String::from_utf8(process.stdout)
        .map_err(|error| format!("worker setup report is not UTF-8: {error}"))?;
    let mut value: serde_json::Value =
        serde_json::from_str(&report).map_err(|error| format!("parse setup report: {error}"))?;
    let tier = selected_tier.expect("a successful process records its security tier");
    let object = value
        .as_object_mut()
        .ok_or_else(|| "setup report root is not an object".to_string())?;
    object.insert(
        "macos_security".into(),
        json!({
            "security_tier": tier.as_str(),
            "native_carrier": tier == SecurityTier::NativeCarrierTrustedOnly,
            "native_carrier_trusted_only": tier == SecurityTier::NativeCarrierTrustedOnly,
            "fallback_reasons": failures,
        }),
    );
    let report = serde_json::to_string_pretty(&value)
        .map_err(|error| format!("serialize macOS setup report: {error}"))?;
    Ok((gui_parameters_from_setup(&value)?, report))
}

fn gui_parameters_from_setup(value: &Value) -> Result<Vec<GuiParameter>, String> {
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
        if !matches!(param_type, 1 | 2 | 4 | 5 | 7 | 10) {
            continue;
        }
        let name = parameter["name"]
            .as_str()
            .ok_or_else(|| "parameter has no name".to_string())?
            .to_string();
        if param_type == 5 {
            let current = parse_argb8_parameter(parameter, "current_color", &name)?;
            let default = parse_argb8_parameter(parameter, "default_color", &name)?;
            parameters.push(GuiParameter {
                slot,
                name,
                param_type,
                value: 0.0,
                default_value: 0.0,
                color: Some(current),
                default_color: Some(default),
                minimum: 0.0,
                maximum: 255.0,
                precision: 0,
            });
            continue;
        }
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
            color: None,
            default_color: None,
            minimum,
            maximum,
            precision: parameter["precision"].as_u64().unwrap_or(0).min(8) as usize,
        });
    }
    Ok(parameters)
}

fn parse_argb8_parameter(parameter: &Value, field: &str, name: &str) -> Result<[u8; 4], String> {
    let components = parameter[field]
        .as_array()
        .ok_or_else(|| format!("color parameter {name:?} has no {field} ARGB8 array"))?;
    if components.len() != 4 {
        return Err(format!(
            "color parameter {name:?} {field} must contain four components"
        ));
    }
    let mut color = [0u8; 4];
    for (destination, component) in color.iter_mut().zip(components) {
        *destination = component
            .as_u64()
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| {
                format!("color parameter {name:?} {field} components must be 0..=255")
            })?;
    }
    Ok(color)
}

#[derive(Clone, Debug)]
struct GuestWorkerCandidate {
    path: PathBuf,
    native: bool,
}

impl GuestWorkerCandidate {
    fn security_tier(&self) -> SecurityTier {
        if self.native {
            SecurityTier::NativeCarrierTrustedOnly
        } else {
            SecurityTier::UnicornGuest
        }
    }
}

fn guest_worker_candidates(repository: &Path) -> Result<Vec<GuestWorkerCandidate>, String> {
    if let Some(path) = std::env::var_os("AEXCOMPAT_GUEST_WORKER").map(PathBuf::from) {
        if path.is_file() {
            let native = thin_macho_is_x86_64(&path)?;
            if native && !native_carrier_opted_in()? {
                return Err("x86_64 AEXCOMPAT_GUEST_WORKER requires AEXCOMPAT_NATIVE_CARRIER=1 and AEXCOMPAT_NATIVE_CARRIER_TRUSTED=1".into());
            }
            return Ok(vec![GuestWorkerCandidate { path, native }]);
        }
        return Err(format!(
            "AEXCOMPAT_GUEST_WORKER does not identify a file: {}",
            path.display()
        ));
    }
    let mut candidates = Vec::new();
    if native_carrier_opted_in()? {
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

fn native_carrier_opted_in() -> Result<bool, String> {
    let enabled = std::env::var_os("AEXCOMPAT_NATIVE_CARRIER");
    let trusted = std::env::var_os("AEXCOMPAT_NATIVE_CARRIER_TRUSTED");
    native_carrier_opted_in_values(enabled.as_deref(), trusted.as_deref())
}

fn native_carrier_opted_in_values(
    enabled: Option<&std::ffi::OsStr>,
    trusted: Option<&std::ffi::OsStr>,
) -> Result<bool, String> {
    if enabled.is_none() && trusted.is_none() {
        return Ok(false);
    }
    if enabled.as_deref() != Some(std::ffi::OsStr::new("1")) {
        return Err("AEXCOMPAT_NATIVE_CARRIER must be exactly 1 when set".into());
    }
    if trusted.as_deref() != Some(std::ffi::OsStr::new("1")) {
        return Err("native carrier is trusted-plug-ins-only; set AEXCOMPAT_NATIVE_CARRIER_TRUSTED=1 to acknowledge that boundary".into());
    }
    Ok(true)
}

fn thin_macho_is_x86_64(path: &Path) -> Result<bool, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("read guest worker architecture {}: {error}", path.display()))?;
    if bytes.len() < 8 {
        return Err(format!(
            "guest worker is too small to be Mach-O: {}",
            path.display()
        ));
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().expect("four-byte slice"));
    let cpu = match magic {
        0xfeedfacf => u32::from_le_bytes(bytes[4..8].try_into().expect("four-byte slice")),
        0xcffaedfe => u32::from_be_bytes(bytes[4..8].try_into().expect("four-byte slice")),
        _ => return Ok(false),
    };
    Ok(cpu == 0x0100_0007)
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

const NATIVE_DEADLINE_REAP_BUDGET: Duration = Duration::from_millis(20);

fn take_shared_resident_child(child: &SharedResidentChild) -> Option<Child> {
    match child.try_lock() {
        Ok(mut child) => child.take(),
        Err(TryLockError::Poisoned(error)) => error.into_inner().take(),
        Err(TryLockError::WouldBlock) => None,
    }
}

fn terminate_shared_resident_child(child: &SharedResidentChild) -> bool {
    let Some(mut child) = take_shared_resident_child(child) else {
        return false;
    };
    if let Err(error) = terminate_process_group(&mut child) {
        eprintln!("aexcompat resident process-group cleanup failed: {error}");
    }
    true
}

fn resident_join_reaper() -> &'static Sender<thread::JoinHandle<()>> {
    static REAPER: OnceLock<Sender<thread::JoinHandle<()>>> = OnceLock::new();
    REAPER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<thread::JoinHandle<()>>();
        thread::Builder::new()
            .name("aexcompat-resident-thread-reaper".into())
            .spawn(move || {
                while let Ok(join) = receiver.recv() {
                    let _ = join.join();
                }
            })
            .expect("start resident thread reaper");
        sender
    })
}

fn enqueue_resident_join(join: thread::JoinHandle<()>) {
    if let Err(error) = resident_join_reaper().send(join) {
        // The receiver can disappear only after its reaper panics. Dropping
        // the recovered handle still detaches it without blocking the caller.
        drop(error.0);
    }
}

struct BackgroundReapRequest {
    child: Child,
    #[cfg(test)]
    completion: Option<Sender<u32>>,
}

fn background_reaper() -> &'static Sender<BackgroundReapRequest> {
    static REAPER: OnceLock<Sender<BackgroundReapRequest>> = OnceLock::new();
    REAPER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<BackgroundReapRequest>();
        thread::Builder::new()
            .name("aexcompat-native-child-reaper".into())
            .spawn(move || {
                let mut pending = Vec::new();
                loop {
                    match receiver.recv_timeout(Duration::from_millis(10)) {
                        Ok(request) => pending.push(request),
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) if pending.is_empty() => break,
                        Err(RecvTimeoutError::Disconnected) => {}
                    }
                    pending.retain_mut(|request| match request.child.try_wait() {
                        Ok(Some(_)) => {
                            #[cfg(test)]
                            if let Some(completion) = request.completion.take() {
                                let _ = completion.send(request.child.id());
                            }
                            false
                        }
                        Ok(None) | Err(_) => true,
                    });
                }
            })
            .expect("start native child reaper");
        sender
    })
}

fn enqueue_background_reap(child: Child, #[cfg(test)] completion: Option<Sender<u32>>) {
    let request = BackgroundReapRequest {
        child,
        #[cfg(test)]
        completion,
    };
    background_reaper()
        .send(request)
        .expect("native child reaper remains available");
}

fn kill_and_reap_or_transfer(mut child: Child, budget: Duration) -> bool {
    let _ = child.kill();
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if started.elapsed() < budget => thread::yield_now(),
            Ok(None) | Err(_) => {
                enqueue_background_reap(
                    child,
                    #[cfg(test)]
                    None,
                );
                return false;
            }
        }
    }
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
                    // Reap a normally interruptible native child without ever
                    // turning Rosetta admission failure into an unbounded wait.
                    // If SIGKILL cannot complete within this small budget,
                    // transfer the handle to the background reaper and let the
                    // Unicorn candidate start at once.
                    let reaped = kill_and_reap_or_transfer(child, NATIVE_DEADLINE_REAP_BUDGET);
                    return Err(format!(
                        "native worker exceeded {} ms and was terminated{}",
                        deadline.as_millis(),
                        if reaped {
                            ""
                        } else {
                            " (child cleanup still pending)"
                        }
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

    fn fixture_resident_child() -> Child {
        let mut command = Command::new("/bin/sleep");
        command.arg("5").process_group(0);
        command.spawn().unwrap()
    }

    #[test]
    fn repository_root_is_found_from_worktree() {
        assert!(repository_root().is_some());
    }

    #[test]
    fn fixture_image_encoding_is_bit_exact_at_all_depths() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-macos-fixture-pixel-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        let path = root.join("pixel.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 128, 0, 64]))
            .save(&path)
            .unwrap();
        let (_, _, argb8) = encode_resident_image(&path, MacRenderFormat::PngArgb8).unwrap();
        assert_eq!(argb8, [64, 255, 128, 0]);
        let (_, _, argb16) = encode_resident_image(&path, MacRenderFormat::RawArgb16).unwrap();
        let words = argb16
            .chunks_exact(2)
            .map(|word| u16::from_le_bytes([word[0], word[1]]))
            .collect::<Vec<_>>();
        assert_eq!(words, [8224, 32768, 16448, 0]);
        let (_, _, argb32f) = encode_resident_image(&path, MacRenderFormat::ExrArgb32f).unwrap();
        let floats = argb32f
            .chunks_exact(4)
            .map(|word| f32::from_le_bytes(word.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(floats, [64.0 / 255.0, 1.0, 128.0 / 255.0, 0.0]);
        assert_eq!(
            argb_to_rgba_words(&argb8, 1),
            [255, 128, 0, 64],
            "artifact conversion must preserve component words"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    fn fixture_parameter(slot: u32, kind: &str, value: f64) -> InteractiveParameter {
        InteractiveParameter {
            slot,
            name: format!("parameter-{slot}"),
            kind: kind.into(),
            minimum: 0.0,
            maximum: 255.0,
            value,
            choices: Vec::new(),
            color: [255, 1, 2, 3],
            components: [1.5, 2.5, 3.5],
            component_count: match kind {
                "angle" => 1,
                "point" => 2,
                "point3d" => 3,
                _ => 0,
            },
            layer_path: (kind == "layer").then(|| PathBuf::from("secondary.png")),
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0; 2],
        }
    }

    #[test]
    fn fixture_payload_preserves_scalar_types_and_excludes_layers() {
        let payload = fixture_parameter_payload(&[
            fixture_parameter(1, "layer", 0.0),
            fixture_parameter(2, "integer", 12.0),
            fixture_parameter(3, "float", 2.5),
            fixture_parameter(4, "color", 0.0),
        ])
        .unwrap();
        assert_eq!(
            payload.transport,
            "v2|param_2@2:i32=12;param_3@3:f64=2.5;param_4@4:argb8=255,1,2,3"
        );
        assert_eq!(
            payload.identity,
            json!([
                {"id":"param_2","slot":2,"kind":"integer","value":12},
                {"id":"param_3","slot":3,"kind":"float","value":2.5},
                {"id":"param_4","slot":4,"kind":"color","value":{
                    "alpha":255,"red":1,"green":2,"blue":3
                }}
            ])
        );
    }

    #[test]
    fn fixture_payload_preserves_angle_point_and_point3d_components() {
        let payload = fixture_parameter_payload(&[
            fixture_parameter(1, "angle", 0.0),
            fixture_parameter(2, "point", 0.0),
            fixture_parameter(3, "point3d", 0.0),
        ])
        .unwrap();
        assert_eq!(
            payload.transport,
            "v4|param_1@1:angle=1.5;param_2@2:point=1.5,2.5;param_3@3:point3d=1.5,2.5,3.5"
        );
        assert_eq!(
            payload.identity,
            json!([
                {"id":"param_1","slot":1,"kind":"angle","value":[1.5]},
                {"id":"param_2","slot":2,"kind":"point","value":[1.5,2.5]},
                {"id":"param_3","slot":3,"kind":"point3d","value":[1.5,2.5,3.5]}
            ])
        );
    }

    #[test]
    fn fixture_render_request_carries_complete_timing() {
        let request = fixture_render_request(
            &FixtureTiming {
                current_time: 42,
                time_step: 7,
                total_time: 210,
                time_scale: 30,
            },
            "v2|",
        );
        assert_eq!(request["v"], 4);
        assert_eq!(
            request["current_time"],
            json!({"value":42,"step":7,"total":210,"scale":30})
        );
        assert_eq!(request["parameters"], "v2|");
    }

    #[test]
    fn native_carrier_requires_explicit_trusted_plugin_acknowledgement() {
        let one = std::ffi::OsStr::new("1");
        assert_eq!(native_carrier_opted_in_values(None, None), Ok(false));
        assert!(native_carrier_opted_in_values(Some(one), None).is_err());
        assert!(native_carrier_opted_in_values(None, Some(one)).is_err());
        assert_eq!(
            native_carrier_opted_in_values(Some(one), Some(one)),
            Ok(true)
        );
    }

    #[test]
    fn explicit_worker_architecture_detects_thin_x86_64() {
        let session = WorkerSession::create().unwrap();
        let x86 = session.root().join("x86-worker");
        let arm = session.root().join("arm-worker");
        std::fs::write(&x86, [0xcf, 0xfa, 0xed, 0xfe, 0x07, 0x00, 0x00, 0x01]).unwrap();
        std::fs::write(&arm, [0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0x00, 0x00, 0x01]).unwrap();
        assert!(thin_macho_is_x86_64(&x86).unwrap());
        assert!(!thin_macho_is_x86_64(&arm).unwrap());
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
    fn native_deadline_cleanup_reaps_an_interruptible_child_within_a_bounded_budget() {
        let child = Command::new("/bin/sleep").arg("5").spawn().unwrap();
        let started = Instant::now();

        assert!(kill_and_reap_or_transfer(
            child,
            NATIVE_DEADLINE_REAP_BUDGET
        ));
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn native_deadline_cleanup_transfers_slow_reap_ownership() {
        let child = Command::new("/bin/sleep").arg("0.05").spawn().unwrap();
        let pid = child.id();
        let (completion, completed) = mpsc::channel();

        enqueue_background_reap(child, Some(completion));

        assert_eq!(completed.recv_timeout(Duration::from_secs(1)).unwrap(), pid);
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
                color: None,
                default_color: None,
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
                color: None,
                default_color: None,
                minimum: 0.0,
                maximum: 1.0,
                precision: 0,
            },
        ];
        assert_eq!(
            parameter_payload(&parameters).unwrap(),
            "v2|param_1@1:f64=50.25;param_2@2:i32=1"
        );
    }

    #[test]
    fn resident_parameter_payload_encodes_slot_qualified_argb8() {
        let parameters = [GuiParameter {
            slot: 2,
            name: "Color".into(),
            param_type: 5,
            value: 0.0,
            default_value: 0.0,
            color: Some([255, 64, 128, 192]),
            default_color: Some([255, 0, 0, 0]),
            minimum: 0.0,
            maximum: 255.0,
            precision: 0,
        }];
        assert_eq!(
            parameter_payload(&parameters).unwrap(),
            "v2|param_2@2:argb8=255,64,128,192"
        );
    }

    #[test]
    fn translucent_color_editor_transport_preserves_unmultiplied_rgb() {
        let argb = [64, 200, 100, 50];
        assert_eq!(rgba8_to_argb8(argb8_to_rgba8(argb)), argb);
    }

    #[test]
    fn argb8_setup_fields_are_exact_and_bounded() {
        let parameter = json!({
            "current_color": [255, 64, 128, 192],
            "default_color": [255, 0, 0, 0]
        });
        assert_eq!(
            parse_argb8_parameter(&parameter, "current_color", "Color").unwrap(),
            [255, 64, 128, 192]
        );
        assert!(
            parse_argb8_parameter(
                &json!({"current_color": [256, 0, 0, 0]}),
                "current_color",
                "Color"
            )
            .is_err()
        );
    }

    #[test]
    fn setup_discovery_exposes_color_parameter_to_gui() {
        let parameters = gui_parameters_from_setup(&json!({
            "parameters": [{
                "slot": 2,
                "param_type": 5,
                "name": "Color",
                "current_color": [255, 64, 128, 192],
                "default_color": [255, 0, 0, 0]
            }]
        }))
        .unwrap();
        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0].slot, 2);
        assert_eq!(parameters[0].param_type, 5);
        assert_eq!(parameters[0].color, Some([255, 64, 128, 192]));
        assert_eq!(parameters[0].default_color, Some([255, 0, 0, 0]));
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
                "render_path": "classic",
                "checksum": "0".repeat(64),
                "guards_intact": true
            },
            "render_error": 0,
            "generation": 1
        });
        assert!(validate_resident_frame(&valid, 0, 2, 1, MacRenderFormat::PngArgb8).is_ok());
        let mut stale = valid.clone();
        stale["generation"] = json!(0);
        assert!(validate_resident_frame(&stale, 0, 2, 1, MacRenderFormat::PngArgb8).is_err());
        let mut unknown = valid;
        unknown["unexpected"] = json!(true);
        assert!(validate_resident_frame(&unknown, 0, 2, 1, MacRenderFormat::PngArgb8).is_err());
    }

    #[test]
    fn resident_argb32f_response_and_exr_artifact_preserve_word_identity() {
        let response = json!({
            "v": 1,
            "type": "frame_done",
            "frame_index": 3,
            "status": "ok",
            "output": {
                "width": 1,
                "height": 1,
                "rowbytes": 16,
                "pixel_format": "argb32f",
                "render_path": "smartfx",
                "checksum": "a".repeat(64),
                "guards_intact": true
            },
            "render_error": 0,
            "generation": 4
        });
        assert!(validate_resident_frame(&response, 3, 1, 1, MacRenderFormat::ExrArgb32f).is_ok());
        assert!(validate_resident_frame(&response, 3, 1, 1, MacRenderFormat::PngArgb8).is_err());

        let root = std::env::temp_dir().join(format!(
            "aexcompat-macos-exr-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let slot = root.join("output.argb32f");
        let artifact = root.join("artifact");
        let words = [0x8000_0000u32, 0x7fc1_2345, 0x0000_0001, 0x3f80_0000];
        let bytes = words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>();
        std::fs::write(&slot, &bytes).unwrap();
        let expected_checksum = format!("{:x}", Sha256::digest(&bytes));
        let rejected = root.join("rejected");
        assert!(
            save_argb32f_exr_slot(
                &slot,
                &rejected,
                1,
                1,
                &"11".repeat(32),
                &"22".repeat(32),
                "v2",
                "smartfx",
                &"00".repeat(32),
            )
            .is_err()
        );
        assert!(!rejected.exists());
        let checksum = save_argb32f_exr_slot(
            &slot,
            &artifact,
            1,
            1,
            &"11".repeat(32),
            &"22".repeat(32),
            "v2",
            "smartfx",
            &expected_checksum,
        )
        .unwrap();
        assert_eq!(checksum, expected_checksum);
        assert!(artifact.join("output.exr").is_file());
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(artifact.join("output.json")).unwrap()).unwrap();
        assert_eq!(metadata["pixel_format"], "float32");
        assert_eq!(metadata["comparison_identity"]["world_sha256"], checksum);
        let _ = std::fs::remove_dir_all(root);
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
        let child = fixture_resident_child();
        let started = Instant::now();
        assert!(wait_or_kill_resident_child(child, Duration::from_millis(20)).is_err());
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn resident_shutdown_preserves_normal_clean_close() {
        let (command_sender, command_receiver) = mpsc::channel();
        let (_result_sender, result_receiver) = mpsc::channel();
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let child = Arc::new(Mutex::new(None));
        let join = thread::spawn(move || match command_receiver.recv().unwrap() {
            ResidentCommand::Close(reply) => reply.send(Ok(())).unwrap(),
            ResidentCommand::Render { .. } => panic!("unexpected render"),
        });
        let mut session = ResidentSessionHandle {
            plugin_path: PathBuf::new(),
            input: PathBuf::new(),
            format: MacRenderFormat::PngArgb8,
            sender: command_sender,
            receiver: result_receiver,
            next_frame: 0,
            child,
            shutdown_requested,
            join: Some(join),
        };

        session
            .shutdown_with_deadline(Duration::from_millis(100))
            .unwrap();
        assert!(session.join.is_none());
    }

    #[test]
    fn resident_shutdown_cancels_an_active_render_before_close() {
        let (command_sender, command_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let worker_shutdown_requested = Arc::clone(&shutdown_requested);
        let child = Arc::new(Mutex::new(Some(fixture_resident_child())));
        let worker_child = Arc::clone(&child);
        let join = thread::spawn(move || {
            let (_response_sender, response_receiver) = mpsc::channel();
            match command_receiver.recv().unwrap() {
                ResidentCommand::Render { .. } => {
                    let result = recv_resident_response(
                        &response_receiver,
                        &worker_child,
                        &worker_shutdown_requested,
                        Duration::from_secs(5),
                    );
                    result_sender.send(result.map(|_| unreachable!())).unwrap();
                }
                ResidentCommand::Close(_) => panic!("render must be queued first"),
            }
            match command_receiver.recv().unwrap() {
                ResidentCommand::Close(reply) => {
                    assert!(worker_child.lock().unwrap().is_none());
                    reply.send(Ok(())).unwrap();
                }
                ResidentCommand::Render { .. } => panic!("close must follow render"),
            }
        });
        let mut session = ResidentSessionHandle {
            plugin_path: PathBuf::new(),
            input: PathBuf::new(),
            format: MacRenderFormat::PngArgb8,
            sender: command_sender,
            receiver: result_receiver,
            next_frame: 0,
            child,
            shutdown_requested,
            join: Some(join),
        };
        session
            .sender
            .send(ResidentCommand::Render {
                frame_index: 0,
                parameters: Vec::new(),
                output: PathBuf::new(),
            })
            .unwrap();

        let started = Instant::now();
        session
            .shutdown_with_deadline(Duration::from_millis(250))
            .unwrap();
        assert!(started.elapsed() < Duration::from_millis(250));
        assert!(
            session
                .receiver
                .recv_timeout(Duration::from_millis(50))
                .is_ok()
        );
    }

    #[test]
    fn resident_shutdown_terminates_a_hung_worker_without_joining_on_gui_thread() {
        let (command_sender, _command_receiver) = mpsc::channel();
        let (_result_sender, result_receiver) = mpsc::channel();
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let child = Arc::new(Mutex::new(Some(fixture_resident_child())));
        let observer_child = Arc::clone(&child);
        let (finished_sender, finished_receiver) = mpsc::channel();
        let join = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            assert!(observer_child.lock().unwrap().is_none());
            finished_sender.send(()).unwrap();
        });
        let mut session = ResidentSessionHandle {
            plugin_path: PathBuf::new(),
            input: PathBuf::new(),
            format: MacRenderFormat::PngArgb8,
            sender: command_sender,
            receiver: result_receiver,
            next_frame: 0,
            child,
            shutdown_requested,
            join: Some(join),
        };

        let started = Instant::now();
        let error = session
            .shutdown_with_deadline(Duration::from_millis(20))
            .unwrap_err();
        assert!(started.elapsed() < Duration::from_millis(100));
        assert!(error.contains("cleanup continues in background"));
        finished_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
    }

    #[test]
    fn resident_admission_returns_promptly_and_delivers_readiness_later() {
        let (release, blocked) = mpsc::channel();
        let started = Instant::now();
        let admission = spawn_resident_admission(move || {
            blocked.recv().unwrap();
            Ok::<_, String>(42)
        });

        assert!(started.elapsed() < Duration::from_millis(250));
        assert!(matches!(
            admission.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        release.send(()).unwrap();
        assert_eq!(
            admission.recv_timeout(Duration::from_secs(1)).unwrap(),
            Ok(42)
        );
    }

    #[test]
    fn resident_admission_delivers_failure_after_a_blocked_probe() {
        let (release, blocked) = mpsc::channel();
        let admission = spawn_resident_admission(move || {
            blocked.recv().unwrap();
            Err::<(), _>("probe rejected worker".to_string())
        });

        assert!(matches!(
            admission.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        release.send(()).unwrap();
        assert_eq!(
            admission.recv_timeout(Duration::from_secs(1)).unwrap(),
            Err("probe rejected worker".to_string())
        );
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
        let mut session = start_resident_session(
            &workers,
            &aex,
            &input,
            &output_directory,
            MacRenderFormat::PngArgb8,
        )
        .unwrap();
        let parameter = |value| GuiParameter {
            slot: 5,
            name: "Amount".into(),
            param_type: 1,
            value,
            default_value: 0.0,
            color: None,
            default_color: None,
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

    #[test]
    #[ignore = "requires AEXCOMPAT_TEST_AEX and AEXCOMPAT_TEST_INPUT_PNG"]
    fn real_resident_argb32f_render_commits_exr_artifact() {
        let aex = PathBuf::from(std::env::var_os("AEXCOMPAT_TEST_AEX").unwrap());
        let input = PathBuf::from(std::env::var_os("AEXCOMPAT_TEST_INPUT_PNG").unwrap());
        let repository = repository_root().unwrap();
        let output_directory = std::env::temp_dir().join(format!(
            "aexcompat-resident-exr-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&output_directory);
        std::fs::create_dir_all(&output_directory).unwrap();
        let workers = guest_worker_candidates(&repository).unwrap();
        let mut session = start_resident_session(
            &workers,
            &aex,
            &input,
            &output_directory,
            MacRenderFormat::ExrArgb32f,
        )
        .unwrap();
        let artifact = output_directory.join("frame-0.exr-artifact");
        session
            .sender
            .send(ResidentCommand::Render {
                frame_index: 0,
                parameters: Vec::new(),
                output: artifact.clone(),
            })
            .unwrap();
        let result = session
            .receiver
            .recv_timeout(RESIDENT_RENDER_DEADLINE)
            .unwrap()
            .unwrap();
        assert_eq!(result.output, artifact.join("output.exr"));
        assert!(result.output.is_file());
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(artifact.join("output.json")).unwrap()).unwrap();
        assert_eq!(metadata["pixel_format"], "float32");
        assert_eq!(metadata["comparison_identity"]["pixel_format"], "argb32f");
        session.shutdown().unwrap();
        let _ = std::fs::remove_dir_all(output_directory);
    }
}
