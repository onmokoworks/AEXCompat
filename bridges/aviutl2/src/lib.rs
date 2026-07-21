//! AviUtl2 filter plugin (.auf2) bridging into an AEXCompat resident
//! `RenderSession` (issue #269, stage 1).
//!
//! This crate is loaded in-process by AviUtl2, but the untrusted AEX never runs
//! here: `RenderSession::open` spawns the isolated worker subprocess, so the
//! plug-in executes out-of-process under the crash-containment floor (process
//! isolation + Job Object + per-frame watchdog). This plugin only marshals
//! pixels and drives the session.
//!
//! Stage 1 scope: a fixed AEX (from environment), 8-bit RGBA only, no
//! parameters. See `docs/AVIUTL2_BRIDGE_2026-07-21.md`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use aviutl2::{
    AnyResult, AviUtl2Info,
    filter::{FilterPlugin, FilterPluginFlags, FilterPluginTable, FilterProcVideo, RgbaPixel},
    tracing,
};
use zerocopy::IntoBytes;

use aexcompat_broker::image_render::RenderPixelFormat;
use aexcompat_broker::render_session::{FrameStatus, RenderSession, SessionOpenRequest};

/// Per-frame watchdog deadline handed to the session (protocol §7). A frame the
/// worker cannot finish in time invalidates the session fail-closed.
const FRAME_DEADLINE_MS: u64 = 30_000;

/// Environment variable naming the AEX to load (stage 1 fixed plug-in).
const ENV_PLUGIN: &str = "AEXCOMPAT_AVIUTL2_PLUGIN";
/// Environment variable naming the repository root holding the built worker at
/// `target/minihost-build/aex_render_worker.exe`.
const ENV_REPOSITORY: &str = "AEXCOMPAT_AVIUTL2_REPOSITORY";

/// The immutable launch configuration for one resident session, resolved once
/// from the first `proc_video` call for an effect instance.
struct SessionConfig {
    repository: PathBuf,
    plugin: PathBuf,
    plugin_sha256: String,
    width: u32,
    height: u32,
    time_step: i32,
    total_time: i32,
    time_scale: u32,
}

/// A single validated frame handed back to the AviUtl2 callback thread.
struct RenderedFrame {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

/// A render request sent to a session's owning thread. `reply` carries either
/// the validated frame or a human-readable diagnostic.
struct RenderReq {
    current_time: i32,
    rgba: Vec<u8>,
    reply: Sender<Result<RenderedFrame, String>>,
}

/// Handle to a resident session. The `RenderSession` itself is `!Send` (it
/// holds the shared-memory view pointer), so it stays pinned to `join`'s thread
/// and is reached only through `tx`. This handle is `Send`, so the plugin's map
/// can live behind a `Mutex` reached from any AviUtl2 callback thread
/// (watch-item #2, see the design note).
struct BridgeSession {
    tx: Option<Sender<RenderReq>>,
    width: u32,
    height: u32,
    join: Option<JoinHandle<()>>,
}

impl BridgeSession {
    fn open(config: SessionConfig) -> Result<BridgeSession, String> {
        let width = config.width;
        let height = config.height;
        let (tx, rx) = channel::<RenderReq>();
        let (open_tx, open_rx) = channel::<Result<(), String>>();

        let join = std::thread::Builder::new()
            .name("aex-aviutl2-session".into())
            .spawn(move || {
                let mut session = match RenderSession::open(SessionOpenRequest {
                    repository: &config.repository,
                    plugin_path: &config.plugin,
                    plugin_sha256: &config.plugin_sha256,
                    parameters: None,
                    parameter_animation: None,
                    aux_manifest: None,
                    world_dump_dir: None,
                    output_checksum_detail: false,
                    mask_trailer: None,
                    spatial_trailer: None,
                    render_environment_trailer: None,
                    alpha_as_coverage_params: &[],
                    layers: &[],
                    dependencies: Vec::new(),
                    width: config.width,
                    height: config.height,
                    pixel_format: RenderPixelFormat::Argb8,
                    time_step: config.time_step,
                    total_time: config.total_time,
                    time_scale: config.time_scale,
                    frame_deadline: Duration::from_millis(FRAME_DEADLINE_MS),
                    smart: false,
                    gpu_backend: aexcompat_broker::image_render::RenderGpuBackend::Auto,
                    gpu_runtime_policy: None,
                }) {
                    Ok(session) => session,
                    Err(error) => {
                        let _ = open_tx.send(Err(format!("RenderSession::open failed: {error}")));
                        return;
                    }
                };
                if open_tx.send(Ok(())).is_err() {
                    // The opener gave up; tear the session down cleanly.
                    let _ = session.close();
                    return;
                }

                // `frame_index` is a transport serial for the generation check
                // (protocol §6), decoupled from AviUtl2's `object.frame`: the
                // host renders frames out of order and re-requests the same
                // frame, so the AE time rides `current_time` while the serial
                // only ever increments.
                let mut frame_index: u32 = 0;
                while let Ok(req) = rx.recv() {
                    let outcome = session.render_frame_with_parameters(
                        frame_index,
                        req.current_time,
                        &req.rgba,
                        None,
                    );
                    frame_index = frame_index.wrapping_add(1);
                    let result = match outcome {
                        Ok(outcome) => match outcome.status {
                            FrameStatus::Rendered {
                                pixels,
                                width: frame_width,
                                height: frame_height,
                                ..
                            } => Ok(RenderedFrame {
                                pixels,
                                width: frame_width,
                                height: frame_height,
                            }),
                            FrameStatus::FrameError { render_error } => {
                                Err(format!("frame-local render error {render_error}"))
                            }
                        },
                        Err(error) => Err(format!("render_frame failed: {error}")),
                    };
                    // A dropped receiver means the callback stopped waiting; keep
                    // serving so the session stays valid for later frames.
                    let _ = req.reply.send(result);
                    if session.invalidation().is_some() {
                        break;
                    }
                }
                let _ = session.close();
            })
            .map_err(|error| format!("failed to spawn session thread: {error}"))?;

        match open_rx.recv() {
            Ok(Ok(())) => Ok(BridgeSession {
                tx: Some(tx),
                width,
                height,
                join: Some(join),
            }),
            Ok(Err(message)) => {
                let _ = join.join();
                Err(message)
            }
            Err(_) => {
                let _ = join.join();
                Err("session thread exited before reporting open result".into())
            }
        }
    }

    /// A clone of the request channel to the owning thread. `Sender` is `Send`
    /// + `Clone`, so the caller can drive a render after releasing the session
    /// map lock (the blocking worker round-trip must not hold that lock).
    fn sender(&self) -> Option<Sender<RenderReq>> {
        self.tx.clone()
    }
}

/// Renders one frame by round-tripping through a session's owning thread. Runs
/// with no lock held so concurrent effect instances render in parallel and a
/// slow worker never stalls other AviUtl2 threads on the session map.
fn render_on(tx: &Sender<RenderReq>, current_time: i32, rgba: Vec<u8>) -> Result<RenderedFrame, String> {
    let (reply_tx, reply_rx) = channel();
    tx.send(RenderReq {
        current_time,
        rgba,
        reply: reply_tx,
    })
    .map_err(|_| "session thread is gone".to_string())?;
    reply_rx
        .recv()
        .map_err(|_| "session thread dropped the reply".to_string())?
}

impl Drop for BridgeSession {
    fn drop(&mut self) {
        // Drop the sender first so the owning thread's `rx.recv()` returns; it
        // then closes the session (SEQUENCE_SETDOWN → GLOBAL_SETDOWN) and exits.
        // Joining before disconnecting would deadlock.
        self.tx = None;
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[aviutl2::plugin(FilterPlugin)]
struct AexBridgeFilter {
    sessions: Mutex<HashMap<i64, BridgeSession>>,
}

impl FilterPlugin for AexBridgeFilter {
    fn new(_info: AviUtl2Info) -> AnyResult<Self> {
        aviutl2::tracing_subscriber::fmt()
            .with_max_level(if cfg!(debug_assertions) {
                tracing::Level::DEBUG
            } else {
                tracing::Level::INFO
            })
            .event_format(aviutl2::logger::AviUtl2Formatter)
            .with_writer(aviutl2::logger::AviUtl2LogWriter)
            .init();
        Ok(Self {
            sessions: Mutex::new(HashMap::new()),
        })
    }

    fn plugin_info(&self) -> FilterPluginTable {
        FilterPluginTable {
            name: "AEXCompat (AEX bridge)".to_string(),
            label: None,
            information: format!(
                "Run After Effects AEX plug-ins out-of-process via AEXCompat / v{version} (stage 1)",
                version = env!("CARGO_PKG_VERSION")
            ),
            flags: aviutl2::bitflag!(FilterPluginFlags {
                video: true,
                filter: true,
            }),
            // Stage 1 carries no parameters; the AEX runs with its launch
            // defaults. Parameter mapping is stage 2 (issue #269).
            config_items: Vec::new(),
        }
    }

    fn proc_video(
        &self,
        _config: &[aviutl2::filter::FilterConfigItem],
        video: &mut FilterProcVideo,
    ) -> AnyResult<()> {
        let width = video.video_object.width;
        let height = video.video_object.height;
        if width == 0 || height == 0 {
            return Ok(());
        }
        let effect_id = video.object.effect_id;

        // Map AviUtl2's frame/rate to AE rational time: with frame_rate =
        // rate/scale, current_time = frame*scale over time_scale = rate yields
        // frame/fps seconds. time_step = scale (one frame).
        let rate = *video.scene.frame_rate.numer();
        let scale = *video.scene.frame_rate.denom();
        if rate <= 0 || scale <= 0 {
            return Err(aviutl2::anyhow::anyhow!(
                "AviUtl2 reported a non-positive frame rate {rate}/{scale}"
            ));
        }
        let current_time =
            (video.object.frame as i64 * scale as i64).clamp(0, i32::MAX as i64) as i32;
        let total_time = ((video.object.frame_total.max(1)) as i64 * scale as i64)
            .clamp(1, i32::MAX as i64) as i32;
        // time_scale is the rate numerator; open rejects values above i32::MAX.
        if rate as i64 > i32::MAX as i64 {
            return Err(aviutl2::anyhow::anyhow!(
                "AviUtl2 frame rate numerator {rate} exceeds the session time-scale limit"
            ));
        }
        let time_scale = rate as u32;
        let time_step = scale;

        // Pull the object's current pixels (RGBA8, packed). RgbaPixel is
        // byte-identical to the worker's RGBA8 transport.
        let mut pixels = vec![RgbaPixel::default(); (width as usize) * (height as usize)];
        video.get_image_data(&mut pixels);
        let rgba = pixels.as_bytes().to_vec();

        // Hold the map lock only long enough to get (or open) the session and
        // clone its request channel. The blocking worker round-trip then runs
        // lock-free below.
        let sender = {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| aviutl2::anyhow::anyhow!("session map poisoned"))?;

            // Re-open when the object geometry changes: the session's slot
            // dimensions are fixed at launch.
            let stale = sessions
                .get(&effect_id)
                .is_some_and(|session| session.width != width || session.height != height);
            if stale {
                sessions.remove(&effect_id);
            }
            if !sessions.contains_key(&effect_id) {
                let (repository, plugin, plugin_sha256) = resolve_launch_env()?;
                let config = SessionConfig {
                    repository,
                    plugin,
                    plugin_sha256,
                    width,
                    height,
                    time_step,
                    total_time,
                    time_scale,
                };
                let session = BridgeSession::open(config)
                    .map_err(|message| aviutl2::anyhow::anyhow!("{message}"))?;
                sessions.insert(effect_id, session);
            }
            sessions
                .get(&effect_id)
                .expect("session was just inserted")
                .sender()
                .ok_or_else(|| aviutl2::anyhow::anyhow!("session is closing"))?
        };

        let rendered = render_on(&sender, current_time, rgba);

        match rendered {
            Ok(frame) => {
                video.set_image_data(&frame.pixels, frame.width, frame.height);
                Ok(())
            }
            Err(message) => {
                // A frame-local failure leaves the object's pixels untouched
                // rather than aborting the whole filter/output chain.
                tracing::error!("AEX render failed on effect {effect_id}: {message}");
                Ok(())
            }
        }
    }
}

/// Resolves the fixed stage-1 plug-in identity from the environment: the repo
/// root holding the built worker, the AEX path, and the AEX's sha256.
fn resolve_launch_env() -> AnyResult<(PathBuf, PathBuf, String)> {
    use sha2::{Digest, Sha256};

    let plugin = PathBuf::from(std::env::var_os(ENV_PLUGIN).ok_or_else(|| {
        aviutl2::anyhow::anyhow!("set {ENV_PLUGIN} to the AEX path (stage 1 fixed plug-in)")
    })?);
    let repository = PathBuf::from(std::env::var_os(ENV_REPOSITORY).ok_or_else(|| {
        aviutl2::anyhow::anyhow!(
            "set {ENV_REPOSITORY} to the repo root holding target/minihost-build/aex_render_worker.exe"
        )
    })?);

    let bytes = std::fs::read(&plugin)
        .map_err(|error| aviutl2::anyhow::anyhow!("cannot read AEX {plugin:?}: {error}"))?;
    let plugin_sha256 = hex_lower(&Sha256::digest(&bytes));

    Ok((repository, plugin, plugin_sha256))
}

/// Lowercase hex encoding matching the worker's sha256 form.
fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

aviutl2::register_filter_plugin!(AexBridgeFilter);
