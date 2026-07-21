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

/// The outcome of one frame, distinguishing a still-usable session from a lost
/// one so the caller reopens only when necessary.
enum FrameReply {
    /// The frame rendered; write these pixels back.
    Rendered(RenderedFrame),
    /// A frame-local compatibility diagnostic (selector error, time mismatch).
    /// The session stays usable; leave the object's pixels for this frame.
    FrameLocal(i64),
    /// The session/worker is gone (crash, timeout, host-protection
    /// invalidation, broken pipe). The caller drops it so the next frame
    /// reopens.
    SessionLost(String),
}

/// A render request sent to a session's owning thread.
struct RenderReq {
    current_time: i32,
    rgba: Vec<u8>,
    reply: Sender<FrameReply>,
}

/// Handle to a resident session. The `RenderSession` itself is `!Send` (it
/// holds the shared-memory view pointer), so it stays pinned to `join`'s thread
/// and is reached only through `tx`. This handle is `Send`, so the plugin's map
/// can live behind a `Mutex` reached from any AviUtl2 callback thread
/// (watch-item #2, see the design note).
struct BridgeSession {
    tx: Option<Sender<RenderReq>>,
    /// Launch identity, frozen at open. A frame whose object geometry or timing
    /// differs needs a fresh session (the slot dimensions, total_time and
    /// time_scale are fixed at launch), so this is compared per frame.
    identity: SessionIdentity,
    join: Option<JoinHandle<()>>,
}

/// The launch-fixed identity of a session, used to detect when a live session
/// no longer matches the object and must be reopened.
#[derive(Clone, Copy, PartialEq, Eq)]
struct SessionIdentity {
    width: u32,
    height: u32,
    time_step: i32,
    total_time: i32,
    time_scale: u32,
}

impl BridgeSession {
    fn open(config: SessionConfig) -> Result<BridgeSession, String> {
        let identity = SessionIdentity {
            width: config.width,
            height: config.height,
            time_step: config.time_step,
            total_time: config.total_time,
            time_scale: config.time_scale,
        };
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
                    let reply = match outcome {
                        Ok(outcome) => match outcome.status {
                            FrameStatus::Rendered {
                                pixels,
                                width: frame_width,
                                height: frame_height,
                                ..
                            } => FrameReply::Rendered(RenderedFrame {
                                pixels,
                                width: frame_width,
                                height: frame_height,
                            }),
                            FrameStatus::FrameError { render_error } => {
                                FrameReply::FrameLocal(render_error)
                            }
                        },
                        // An io error means the transport is broken; the session
                        // cannot be trusted for further frames.
                        Err(error) => FrameReply::SessionLost(format!("render_frame failed: {error}")),
                    };
                    // A host-protection invariant failure invalidates the whole
                    // session; the next frame must reopen, so report it lost even
                    // if this frame came back as a frame-local diagnostic.
                    let reply = if session.invalidation().is_some() {
                        let detail = match reply {
                            FrameReply::SessionLost(message) => message,
                            FrameReply::FrameLocal(code) => {
                                format!("session invalidated (render_error {code})")
                            }
                            FrameReply::Rendered(_) => "session invalidated".to_string(),
                        };
                        FrameReply::SessionLost(detail)
                    } else {
                        reply
                    };
                    let lost = matches!(reply, FrameReply::SessionLost(_));
                    // A dropped receiver means the callback stopped waiting; keep
                    // serving so the session stays valid for later frames.
                    let _ = req.reply.send(reply);
                    if lost {
                        break;
                    }
                }
                let _ = session.close();
            })
            .map_err(|error| format!("failed to spawn session thread: {error}"))?;

        match open_rx.recv() {
            Ok(Ok(())) => Ok(BridgeSession {
                tx: Some(tx),
                identity,
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
    /// and `Clone`, so the caller can drive a render after releasing the session
    /// map lock (the blocking worker round-trip must not hold that lock).
    fn sender(&self) -> Option<Sender<RenderReq>> {
        self.tx.clone()
    }
}

/// Renders one frame by round-tripping through a session's owning thread. Runs
/// with no lock held so concurrent effect instances render in parallel and a
/// slow worker never stalls other AviUtl2 threads on the session map. A gone
/// thread is reported as `SessionLost` so the caller reopens.
fn render_on(tx: &Sender<RenderReq>, current_time: i32, rgba: Vec<u8>) -> FrameReply {
    let (reply_tx, reply_rx) = channel();
    if tx
        .send(RenderReq {
            current_time,
            rgba,
            reply: reply_tx,
        })
        .is_err()
    {
        return FrameReply::SessionLost("session thread is gone".to_string());
    }
    match reply_rx.recv() {
        Ok(reply) => reply,
        Err(_) => FrameReply::SessionLost("session thread dropped the reply".to_string()),
    }
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

impl AexBridgeFilter {
    /// Returns the request channel of a live session already matching `identity`,
    /// or `None` when the caller must open one. A session whose identity no
    /// longer matches (object resized, retimed) is evicted here; the eviction is
    /// dropped after the lock is released so its thread `join()` never blocks the
    /// map.
    fn existing_sender(
        &self,
        effect_id: i64,
        identity: SessionIdentity,
    ) -> Result<Option<Sender<RenderReq>>, String> {
        let mut evicted: Option<BridgeSession> = None;
        let sender;
        {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| "session map poisoned".to_string())?;
            match sessions.get(&effect_id) {
                Some(session) if session.identity == identity => sender = session.sender(),
                Some(_) => {
                    evicted = sessions.remove(&effect_id);
                    sender = None;
                }
                None => sender = None,
            }
        }
        drop(evicted);
        Ok(sender)
    }

    /// Opens a session outside the map lock (open blocks for seconds spawning the
    /// worker), then inserts it under a brief lock. If another thread won the
    /// race and already installed a matching session, that one is kept and the
    /// freshly opened session is dropped after the lock is released.
    fn open_and_get_sender(
        &self,
        effect_id: i64,
        identity: SessionIdentity,
        config: SessionConfig,
    ) -> Result<Sender<RenderReq>, String> {
        let mut opened = Some(BridgeSession::open(config)?);
        let my_sender = opened
            .as_ref()
            .expect("just opened")
            .sender()
            .ok_or_else(|| "session is closing".to_string())?;
        let discard: Option<BridgeSession>;
        let sender;
        {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| "session map poisoned".to_string())?;
            match sessions
                .get(&effect_id)
                .filter(|session| session.identity == identity)
                .and_then(|session| session.sender())
            {
                // Lost the open race; keep the installed session, discard ours.
                Some(existing) => {
                    discard = opened.take();
                    sender = existing;
                }
                // Install ours, evicting any stale/dead entry (dropped off-lock).
                None => {
                    discard = sessions.insert(effect_id, opened.take().expect("just opened"));
                    sender = my_sender;
                }
            }
        }
        drop(discard);
        Ok(sender)
    }

    /// Removes and drops a session, e.g. after its worker died so the next frame
    /// reopens. The drop (thread `join()` + `RenderSession::close`) runs after
    /// the lock is released.
    fn remove_session(&self, effect_id: i64) {
        let removed = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(&effect_id));
        drop(removed);
    }
}

impl FilterPlugin for AexBridgeFilter {
    fn new(_info: AviUtl2Info) -> AnyResult<Self> {
        // `try_init` instead of `init`: setting the global subscriber fails if
        // one is already installed (the plugin reloaded, or the host set one).
        // That is not fatal to the bridge, so ignore the error rather than panic
        // inside `new`.
        let _ = aviutl2::tracing_subscriber::fmt()
            .with_max_level(if cfg!(debug_assertions) {
                tracing::Level::DEBUG
            } else {
                tracing::Level::INFO
            })
            .event_format(aviutl2::logger::AviUtl2Formatter)
            .with_writer(aviutl2::logger::AviUtl2LogWriter)
            .try_init();
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

        let identity = SessionIdentity {
            width,
            height,
            time_step,
            total_time,
            time_scale,
        };

        // Reuse a live matching session; otherwise open one outside the map lock.
        // Neither the blocking worker round-trip below nor `open` holds the lock.
        let sender = match self
            .existing_sender(effect_id, identity)
            .map_err(|message| aviutl2::anyhow::anyhow!("{message}"))?
        {
            Some(sender) => sender,
            None => {
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
                self.open_and_get_sender(effect_id, identity, config)
                    .map_err(|message| aviutl2::anyhow::anyhow!("{message}"))?
            }
        };

        match render_on(&sender, current_time, rgba) {
            FrameReply::Rendered(frame) => {
                // A filter object cannot change the image size (AviUtl2 filter
                // contract). An expand/shrink-output effect (PF_OutFlag_I_*_BUFFER)
                // returns dimensions different from the object's; pushing those
                // through `set_image_data` in filter mode is undefined, so reject
                // the frame and leave the object's pixels instead of resizing.
                if frame.width != width || frame.height != height {
                    tracing::warn!(
                        "AEX resized effect {effect_id} from {width}x{height} to {}x{}; \
                         a filter object cannot change size, leaving pixels unchanged",
                        frame.width,
                        frame.height
                    );
                    return Ok(());
                }
                video.set_image_data(&frame.pixels, frame.width, frame.height);
                Ok(())
            }
            FrameReply::FrameLocal(code) => {
                // A frame-local diagnostic. Keep the session and leave this
                // frame's pixels untouched rather than aborting the whole
                // filter/output chain.
                tracing::warn!("AEX frame-local error on effect {effect_id}: render_error {code}");
                Ok(())
            }
            FrameReply::SessionLost(message) => {
                // Worker crash, timeout, or host-protection invalidation. Drop
                // the dead entry so the next frame reopens.
                self.remove_session(effect_id);
                tracing::error!("AEX session lost on effect {effect_id}: {message}");
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
