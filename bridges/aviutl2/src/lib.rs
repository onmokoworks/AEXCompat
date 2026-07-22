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
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use aviutl2::{
    AnyResult, AviUtl2Info,
    common::FileFilter,
    filter::{
        FilterConfigCheckbox, FilterConfigColor, FilterConfigColorValue, FilterConfigFile,
        FilterConfigItem, FilterConfigSelect, FilterConfigSelectItem, FilterConfigTrack,
        FilterPlugin, FilterPluginFlags, FilterPluginTable, FilterProcVideo, RgbaPixel,
    },
    tracing,
};
use zerocopy::IntoBytes;

use aexcompat_broker::image_render::{
    InteractiveParameter, RenderPixelFormat, inspect_experimental_with_diagnostics,
};
use aexcompat_broker::render_session::{FrameStatus, RenderSession, SessionOpenRequest};

/// Per-frame watchdog deadline handed to the session (protocol §7). A frame the
/// worker cannot finish in time invalidates the session fail-closed.
const FRAME_DEADLINE_MS: u64 = 30_000;

/// Image dimension bounds, mirrored from the broker (`image_render.rs`
/// `MAX_DIMENSION` / `MAX_PIXELS`, which are `pub(crate)` and so cannot be
/// imported). Checked before allocating the transfer buffer and before opening
/// the session, which enforces the same limits.
const MAX_DIMENSION: u32 = 4096;
const MAX_PIXELS: u64 = 16_777_216;

/// Idle timeout after which a session (worker subprocess + thread + shared
/// memory) is reaped. AviUtl2's filter API has no per-effect teardown callback
/// and `effect_id` is unique per app launch, so a deleted or abandoned effect
/// is never revisited; without reaping its session would leak until DLL
/// unload. The timeout comfortably exceeds the frame deadline so an in-flight
/// frame is never reaped. Reaping runs opportunistically when a new session is
/// opened.
const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// Monotonic instance counter so a session can be removed by the exact instance
/// that failed rather than by `effect_id` alone (which a concurrent reopen may
/// have replaced with a healthy session).
static SESSION_SERIAL: AtomicU64 = AtomicU64::new(0);

/// Environment variable naming the AEX to load (stage 1 fixed plug-in).
const ENV_PLUGIN: &str = "AEXCOMPAT_AVIUTL2_PLUGIN";
/// Environment variable naming the repository root holding the built workers at
/// `target/minihost-build/` — `aex_render_worker.exe` for a classic AEX and
/// `aex_smart_worker.exe` for a SmartFX AEX.
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
    /// Open the SmartFX session instead of the classic one.
    smart: bool,
    /// The declared parameter set (discovered defaults) used as the session's
    /// launch baseline. Per-frame `render` messages override individual values.
    /// Empty means open with no declared parameters (the plug-in's own
    /// defaults), matching stage-1 behaviour.
    parameters: Vec<InteractiveParameter>,
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
    /// Per-frame parameter values (protocol §4.2.1). `None` renders with the
    /// session's launch baseline. When present it fully replaces the baseline,
    /// so it carries the whole declared set (unmapped slots keep their
    /// discovered defaults).
    parameters: Option<Vec<InteractiveParameter>>,
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
    /// Unique instance id (see [`SESSION_SERIAL`]).
    serial: u64,
    /// Last time a frame was routed to this session; drives idle reaping.
    last_used: Instant,
    join: Option<JoinHandle<()>>,
}

/// The launch-fixed identity of a session, used to detect when a live session
/// no longer matches the object and must be reopened.
#[derive(Clone, PartialEq, Eq)]
struct SessionIdentity {
    /// The selected AEX's path. Kept in the identity (separate from the
    /// content-based `plugin_sha256`) so switching to a byte-identical copy at a
    /// different directory still reopens: an AEX can load DLLs/resources adjacent
    /// to its own path, so same bytes at another location may render differently.
    plugin: PathBuf,
    /// The selected AEX's sha256; a different AEX (runtime file switch) reopens.
    plugin_sha256: String,
    /// Whether this identity renders on the smart path.
    smart: bool,
    width: u32,
    height: u32,
    time_step: i32,
    total_time: i32,
    time_scale: u32,
}

impl BridgeSession {
    fn open(config: SessionConfig) -> Result<BridgeSession, String> {
        let identity = SessionIdentity {
            plugin: config.plugin.clone(),
            plugin_sha256: config.plugin_sha256.clone(),
            smart: config.smart,
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
                let baseline = (!config.parameters.is_empty()).then_some(&config.parameters[..]);
                let mut session = match RenderSession::open(SessionOpenRequest {
                    repository: &config.repository,
                    plugin_path: &config.plugin,
                    plugin_sha256: &config.plugin_sha256,
                    parameters: baseline,
                    parameter_animation: None,
                    aux_manifest: None,
                    world_dump_dir: None,
                    output_checksum_detail: false,
                    mask_trailer: None,
                    spatial_trailer: None,
                    render_environment_trailer: None,
                    // The bridge renders video frames only; an audio source
                    // would come from the host's audio graph, which it does not
                    // read (issue #339).
                    audio_trailer: None,
                    alpha_as_coverage_params: &[],
                    // Conformance render settings feed the worker's report, not
                    // the render (#275). The interactive bridge does not produce
                    // conformance evidence, so leave it unset.
                    conformance_render_settings: None,
                    layers: &[],
                    dependencies: Vec::new(),
                    width: config.width,
                    height: config.height,
                    pixel_format: RenderPixelFormat::Argb8,
                    time_step: config.time_step,
                    total_time: config.total_time,
                    time_scale: config.time_scale,
                    frame_deadline: Duration::from_millis(FRAME_DEADLINE_MS),
                    // SmartFX at 8-bit is CPU (RenderSession only treats
                    // smart+ARGB32f as GPU-capable), so Auto needs no runtime policy.
                    smart: config.smart,
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
                        req.parameters.as_deref(),
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
                            FrameStatus::FrameError { render_error, .. } => {
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
                serial: SESSION_SERIAL.fetch_add(1, Ordering::Relaxed),
                last_used: Instant::now(),
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
fn render_on(
    tx: &Sender<RenderReq>,
    current_time: i32,
    rgba: Vec<u8>,
    parameters: Option<Vec<InteractiveParameter>>,
) -> FrameReply {
    let (reply_tx, reply_rx) = channel();
    if tx
        .send(RenderReq {
            current_time,
            rgba,
            parameters,
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

/// AEX path → (mtime, len, sha256): the [`AexBridgeFilter::sha_for`] cache.
type ShaCache = HashMap<PathBuf, (Option<std::time::SystemTime>, u64, String)>;

#[aviutl2::plugin(FilterPlugin)]
struct AexBridgeFilter {
    sessions: Mutex<HashMap<i64, BridgeSession>>,
    /// The fixed AEX's discovered parameter set (defaults), resolved once at
    /// load. Cloned as each session's launch baseline and overlaid with the
    /// object's config values per frame. Empty if discovery failed (the bridge
    /// then renders with the plug-in's own defaults, as in stage 1). The exposed
    /// config items are rebuilt from this on demand (`FilterConfigItem` is not
    /// `Sync`, so it cannot be stored in this `Send + Sync` plug-in).
    param_template: Vec<InteractiveParameter>,
    /// The load-time env AEX path, if set. When the "AEX" file control selects a
    /// different path the bridge switches to it at runtime; its parameters are
    /// not exposed as controls (AviUtl2 config is static), so it renders at the
    /// AEX defaults, and the env AEX's controls apply only when the control is
    /// left empty / equal to it.
    env_plugin: Option<PathBuf>,
    /// The load-time env AEX's canonical path, or `None` when no env AEX is set
    /// or it could not be canonicalized. `is_default` compares the selected AEX's
    /// canonical path against this: canonicalization normalizes case/separators so
    /// re-selecting the same file through AviUtl2's dialog still exposes the env
    /// AEX's parameter controls, while a byte-identical copy in another directory
    /// (which can load different adjacent resources) is correctly not treated as
    /// the default and renders at its own defaults.
    env_canonical: Option<PathBuf>,
    /// The load-time env AEX's sha256, or `None` when no env AEX is set or
    /// discovery failed. `is_default` requires this in addition to the canonical
    /// path: a same-path rebuild keeps the canonical path but changes the sha and
    /// may change the parameter set, so the load-time `param_template` (whose
    /// controls AviUtl2 froze at load) must not be applied to the rebuilt bytes.
    env_sha: Option<String>,
    /// Cache of AEX sha256 (lowercase hex) → advertises-SmartFX, so switching to
    /// a runtime-selected AEX detects its render path once, not per reopen.
    smart_cache: Mutex<HashMap<String, bool>>,
    /// Cache of AEX path → (mtime, len, sha256), so `resolve_aex` does not re-read
    /// and re-hash a multi-MB AEX on every frame. A rebuild (mtime/len change)
    /// invalidates the entry, so a switched-in or rebuilt AEX is still detected.
    sha_cache: Mutex<ShaCache>,
}

impl AexBridgeFilter {
    /// Returns the channel and instance serial of a live session already
    /// matching `identity`, or `None` when the caller must open one. A session
    /// whose identity no longer matches (object resized, retimed) is evicted
    /// here. The matched session's `last_used` is refreshed so it is not reaped
    /// while in use. Any eviction is dropped after the lock is released so its
    /// thread `join()` never blocks the map.
    fn existing_sender(
        &self,
        effect_id: i64,
        identity: &SessionIdentity,
    ) -> Result<Option<(Sender<RenderReq>, u64)>, String> {
        let mut evicted: Option<BridgeSession> = None;
        let result;
        {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| "session map poisoned".to_string())?;
            match sessions.get_mut(&effect_id) {
                Some(session) if &session.identity == identity => {
                    session.last_used = Instant::now();
                    result = session.sender().map(|tx| (tx, session.serial));
                }
                Some(_) => {
                    evicted = sessions.remove(&effect_id);
                    result = None;
                }
                None => result = None,
            }
        }
        drop(evicted);
        Ok(result)
    }

    /// Opens a session outside the map lock (open blocks for seconds spawning the
    /// worker), then inserts it under a brief lock. If another thread won the
    /// race and already installed a matching session, that one is kept and the
    /// freshly opened session is dropped after the lock is released. Idle
    /// sessions are reaped here (see [`SESSION_IDLE_TIMEOUT`]). Every removed
    /// session is dropped off-lock.
    fn open_and_get_sender(
        &self,
        effect_id: i64,
        identity: &SessionIdentity,
        config: SessionConfig,
    ) -> Result<(Sender<RenderReq>, u64), String> {
        let opened = BridgeSession::open(config)?;
        let serial = opened.serial;
        let mut discard: Vec<BridgeSession> = Vec::new();
        let sender;
        {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| "session map poisoned".to_string())?;

            // Reap sessions of abandoned effects (deleted, or scrubbed past and
            // not re-rendered). last_used exceeds the frame deadline by far, so
            // an in-flight frame is never reaped.
            let now = Instant::now();
            let expired: Vec<i64> = sessions
                .iter()
                .filter(|(_, session)| now.duration_since(session.last_used) > SESSION_IDLE_TIMEOUT)
                .map(|(id, _)| *id)
                .collect();
            for id in expired {
                if let Some(session) = sessions.remove(&id) {
                    discard.push(session);
                }
            }

            match sessions
                .get(&effect_id)
                .filter(|session| &session.identity == identity)
                .and_then(|session| session.sender().map(|tx| (tx, session.serial)))
            {
                // Lost the open race; keep the installed session, discard ours.
                // No clone of `opened`'s channel is taken on this path, so the
                // off-lock drop/join below cannot wait on a stray sender that
                // outlives it (would deadlock).
                Some(existing) => {
                    discard.push(opened);
                    sender = existing;
                }
                // Install ours, evicting any stale/dead entry (dropped off-lock).
                // Clone the sender before the move; a freshly opened session
                // always has a live channel.
                None => {
                    let tx = opened
                        .sender()
                        .expect("a freshly opened session has a live sender");
                    if let Some(old) = sessions.insert(effect_id, opened) {
                        discard.push(old);
                    }
                    sender = (tx, serial);
                }
            }
        }
        drop(discard);
        Ok(sender)
    }

    /// Removes and drops the session identified by `effect_id` **and** `serial`,
    /// e.g. after its worker died so the next frame reopens. Matching on serial
    /// avoids dropping a healthy session that a concurrent reopen installed at
    /// the same `effect_id`. The drop (thread `join()` + `RenderSession::close`)
    /// runs after the lock is released.
    fn remove_session(&self, effect_id: i64, serial: u64) {
        let removed = {
            let Ok(mut sessions) = self.sessions.lock() else {
                return;
            };
            if sessions
                .get(&effect_id)
                .is_some_and(|session| session.serial == serial)
            {
                sessions.remove(&effect_id)
            } else {
                None
            }
        };
        drop(removed);
    }

    /// Resolves which AEX to render: the "AEX" file control (config item 0)
    /// overrides the env default, letting the operator switch AEX at runtime.
    fn resolve_aex(&self, config: &[FilterConfigItem]) -> Result<ResolvedAex, String> {
        let repository = PathBuf::from(std::env::var_os(ENV_REPOSITORY).ok_or_else(|| {
            format!("set {ENV_REPOSITORY} to the repo root holding the built workers")
        })?);
        let override_path = config.iter().find_map(|item| match item {
            FilterConfigItem::File(file) => {
                let trimmed = file.value.trim();
                (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
            }
            _ => None,
        });
        let plugin = match override_path {
            Some(path) => path,
            None => self.env_plugin.clone().ok_or_else(|| {
                format!("select an AEX (the \"AEX\" control) or set {ENV_PLUGIN}")
            })?,
        };
        let sha = self.sha_for(&plugin)?;
        // The selection is the env default (whose frozen load-time controls apply)
        // only when it is BOTH the same file AND the same bytes as at load:
        // - canonical path match: normalizes case/separators so a re-selection of
        //   the env AEX still matches, while a byte-identical copy in another
        //   directory does not (it can load different adjacent resources).
        // - sha match: a same-path rebuild keeps the canonical path but changes the
        //   bytes and may change the parameter set, so the load-time param_template
        //   must not be applied to the rebuilt AEX (it renders at its own defaults).
        // Canonicalize failure (file vanished) conservatively falls to "not default".
        let same_path = match (std::fs::canonicalize(&plugin).ok(), &self.env_canonical) {
            (Some(selected), Some(env)) => &selected == env,
            _ => false,
        };
        let is_default = same_path && self.env_sha.as_deref() == Some(sha.as_str());
        // Always resolve smart by the current sha (cached), never by is_default:
        // a same-path rebuild of the env AEX keeps is_default true but changes the
        // sha and may flip its SmartFX advertisement, so the load-time flag can be
        // stale. The env AEX's load-time (sha -> smart) is primed into the cache in
        // `new`, so this is a cache hit until the bytes actually change.
        let smart = self.smart_for(&repository, &plugin, &sha);
        Ok(ResolvedAex {
            repository,
            plugin,
            sha,
            smart,
            is_default,
        })
    }

    /// The AEX's sha256 (lowercase hex), cached by path + (mtime, len) so a frame
    /// does not re-read and re-hash a multi-MB AEX every time. A rebuild changes
    /// mtime/len and invalidates the entry, so a rebuilt AEX is still detected.
    fn sha_for(&self, plugin: &Path) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        let metadata = std::fs::metadata(plugin)
            .map_err(|error| format!("cannot stat AEX {plugin:?}: {error}"))?;
        let len = metadata.len();
        // On Windows (the only `.auf2` target) NTFS always reports mtime, so
        // (mtime, len) catches every rebuild. If mtime is unavailable the entry
        // degrades to len-only keying, which a same-length rebuild could evade;
        // that path does not occur on the supported platform.
        let mtime = metadata.modified().ok();
        if let Ok(cache) = self.sha_cache.lock()
            && let Some((cached_mtime, cached_len, cached_sha)) = cache.get(plugin)
            && *cached_mtime == mtime
            && *cached_len == len
        {
            return Ok(cached_sha.clone());
        }
        let bytes = std::fs::read(plugin)
            .map_err(|error| format!("cannot read AEX {plugin:?}: {error}"))?;
        let sha = hex_lower(&Sha256::digest(&bytes));
        if let Ok(mut cache) = self.sha_cache.lock() {
            cache.insert(plugin.to_path_buf(), (mtime, len, sha.clone()));
        }
        Ok(sha)
    }

    /// Whether an AEX advertises SmartFX, discovered once per distinct AEX and
    /// cached by sha so a runtime switch does not re-inspect on every reopen.
    fn smart_for(&self, repository: &Path, plugin: &Path, sha: &str) -> bool {
        if let Ok(cache) = self.smart_cache.lock()
            && let Some(&cached) = cache.get(sha)
        {
            return cached;
        }
        // PF_OutFlag2_SUPPORTS_SMART_RENDER = bit 10. Default classic on any
        // discovery failure; the render then surfaces the real error per frame.
        let smart = inspect_experimental_with_diagnostics(repository, plugin, sha)
            .ok()
            .map(|(_, diagnostics)| {
                diagnostics
                    .get("advertised_out_flags2")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0)
                    & (1 << 10)
                    != 0
            })
            .unwrap_or(false);
        if let Ok(mut cache) = self.smart_cache.lock() {
            cache.insert(sha.to_string(), smart);
        }
        smart
    }
}

/// The AEX to render for a frame, resolved from the file control or env default.
struct ResolvedAex {
    repository: PathBuf,
    plugin: PathBuf,
    sha: String,
    smart: bool,
    /// True when this is the load-time env AEX, whose discovered parameters are
    /// exposed as controls (a runtime-selected AEX renders at its own defaults).
    is_default: bool,
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

        // Discover the fixed AEX's parameters once. Failure is non-fatal: the
        // plug-in still loads and renders with launch defaults (the render path
        // surfaces the real error).
        let env_plugin = std::env::var_os(ENV_PLUGIN).map(PathBuf::from);
        // Canonicalize once at load for the is_default comparison in `resolve_aex`.
        let env_canonical = env_plugin
            .as_ref()
            .and_then(|path| std::fs::canonicalize(path).ok());
        let (param_template, env_smart, env_sha) = match discover_config() {
            Ok((parameters, smart, sha)) => (parameters, smart, Some(sha)),
            Err(message) => {
                tracing::warn!(
                    "AEX parameter discovery failed ({message}); rendering with launch defaults"
                );
                (Vec::new(), false, None)
            }
        };

        // Prime the SmartFX cache with the env AEX's load-time (sha -> smart), so
        // resolve_aex's per-frame `smart_for(sha)` is a cache hit until the bytes
        // change (a same-path rebuild changes the sha and re-inspects).
        let mut smart_cache = HashMap::new();
        if let Some(sha) = &env_sha {
            smart_cache.insert(sha.clone(), env_smart);
        }

        Ok(Self {
            sessions: Mutex::new(HashMap::new()),
            param_template,
            env_plugin,
            env_canonical,
            env_sha,
            smart_cache: Mutex::new(smart_cache),
            sha_cache: Mutex::new(HashMap::new()),
        })
    }

    fn plugin_info(&self) -> FilterPluginTable {
        // The env AEX's discovered parameter controls come first, so their config
        // indices line up 1:1 with `exposed_config`'s slots (proc_video zips them
        // directly). The "AEX" selector is appended last: prepending it would
        // shift every parameter's positional index, so a later layout change (or a
        // saved project) would misalign the saved values. Empty selector means
        // "use the env AEX"; selecting a different AEX switches to it live and
        // renders it at its own defaults.
        let mut config_items = exposed_config(&self.param_template).items;
        config_items.push(FilterConfigItem::File(FilterConfigFile {
            name: "AEX".to_string(),
            value: self
                .env_plugin
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            filters: vec![
                FileFilter {
                    name: "After Effects plug-in".to_string(),
                    extensions: vec!["aex".to_string()],
                },
                FileFilter {
                    name: "All files".to_string(),
                    extensions: vec![],
                },
            ],
        }));
        FilterPluginTable {
            name: "AEXCompat (AEX bridge)".to_string(),
            label: None,
            information: format!(
                "Run After Effects AEX plug-ins out-of-process via AEXCompat / v{version}",
                version = env!("CARGO_PKG_VERSION")
            ),
            flags: aviutl2::bitflag!(FilterPluginFlags {
                video: true,
                filter: true,
            }),
            config_items,
        }
    }

    fn proc_video(
        &self,
        config: &[FilterConfigItem],
        video: &mut FilterProcVideo,
    ) -> AnyResult<()> {
        let width = video.video_object.width;
        let height = video.video_object.height;
        if width == 0 || height == 0 {
            return Ok(());
        }
        // Bound the dimensions before allocating the transfer buffer (and before
        // opening the session, which enforces the same limits). A pathological
        // object size would otherwise allocate a huge Vec below.
        if width > MAX_DIMENSION
            || height > MAX_DIMENSION
            || u64::from(width) * u64::from(height) > MAX_PIXELS
        {
            tracing::warn!(
                "AEX object {width}x{height} exceeds the session limits \
                 ({MAX_DIMENSION} per side, {MAX_PIXELS} px); leaving pixels unchanged"
            );
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

        // Resolve the AEX: the "AEX" file control (config[0]) overrides the env
        // default at runtime; a different AEX reopens (identity includes its sha).
        let aex = self
            .resolve_aex(config)
            .map_err(|message| aviutl2::anyhow::anyhow!("{message}"))?;

        let identity = SessionIdentity {
            plugin: aex.plugin.clone(),
            plugin_sha256: aex.sha.clone(),
            smart: aex.smart,
            width,
            height,
            time_step,
            total_time,
            time_scale,
        };

        // The env AEX's discovered controls apply only when it is the selected
        // AEX; a runtime-selected AEX renders at its own defaults (AviUtl2 config
        // is static, so its parameters cannot be exposed as controls). Only the
        // exposed (UI-controllable) parameters are ever sent.
        let exposed = if aex.is_default {
            exposed_config(&self.param_template)
        } else {
            ExposedParams {
                items: Vec::new(),
                slots: Vec::new(),
                defaults: Vec::new(),
            }
        };

        // Reuse a live matching session; otherwise open one outside the map lock.
        // Neither the blocking worker round-trip below nor `open` holds the lock.
        // `serial` identifies this exact instance for a precise removal on loss.
        let (sender, serial) = match self
            .existing_sender(effect_id, &identity)
            .map_err(|message| aviutl2::anyhow::anyhow!("{message}"))?
        {
            Some(pair) => pair,
            None => {
                let session_config = SessionConfig {
                    repository: aex.repository.clone(),
                    plugin: aex.plugin.clone(),
                    plugin_sha256: aex.sha.clone(),
                    width,
                    height,
                    time_step,
                    total_time,
                    time_scale,
                    smart: aex.smart,
                    parameters: exposed.defaults.clone(),
                };
                self.open_and_get_sender(effect_id, &identity, session_config)
                    .map_err(|message| aviutl2::anyhow::anyhow!("{message}"))?
            }
        };

        // Overlay this object's config values onto the exposed defaults and send
        // them per frame. The parameter controls occupy config[0..slots.len()] and
        // the appended "AEX" selector sits after them, so `apply_config_values`
        // (which zips slots against config) consumes only the parameter items and
        // never reaches the trailing selector. No exposed params => None (launch
        // baseline).
        let parameters = if exposed.defaults.is_empty() {
            None
        } else {
            let mut values = exposed.defaults;
            apply_config_values(&mut values, &exposed.slots, config);
            Some(values)
        };

        match render_on(&sender, current_time, rgba, parameters) {
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
                // this exact instance so the next frame reopens, without
                // disturbing a healthy session a concurrent reopen may have
                // installed at the same effect_id.
                self.remove_session(effect_id, serial);
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

/// Discovers the fixed AEX's parameter set (spawns a `--l2-params-only` worker
/// via the broker). The result is the launch baseline; exposed config items are
/// derived from it by [`exposed_config`].
fn discover_config() -> AnyResult<(Vec<InteractiveParameter>, bool, String)> {
    let (repository, plugin, plugin_sha256) = resolve_launch_env()?;
    let (parameters, diagnostics) =
        inspect_experimental_with_diagnostics(&repository, &plugin, &plugin_sha256)
            .map_err(|error| aviutl2::anyhow::anyhow!("parameter discovery failed: {error}"))?;
    // A SmartFX effect (PF_OutFlag2_SUPPORTS_SMART_RENDER = bit 10) must run on
    // the smart session; the classic session rejects it with a callback error.
    let smart = diagnostics
        .get("advertised_out_flags2")
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
        & (1 << 10)
        != 0;
    Ok((parameters, smart, plugin_sha256))
}

/// The mappable parameters exposed as AviUtl2 controls: the config items, the
/// AEX slot each drives, and the parameters themselves (a subset of the template).
/// The three vectors are parallel and deterministic, so `plugin_info` (items),
/// session open (defaults), and `proc_video` (slots + defaults) all agree.
///
/// Only exposed parameters are ever sent to the worker. The full discovered set
/// includes kinds the interactive payload cannot carry (a popup, for instance,
/// is reported as an `integer` with a degenerate range and is dropped here);
/// feeding those back to the worker breaks the render, whereas an AEX renders
/// fine with them left at its own defaults (stage-1 behaviour).
struct ExposedParams {
    items: Vec<FilterConfigItem>,
    slots: Vec<u32>,
    defaults: Vec<InteractiveParameter>,
}

fn exposed_config(template: &[InteractiveParameter]) -> ExposedParams {
    let mut exposed = ExposedParams {
        items: Vec::new(),
        slots: Vec::new(),
        defaults: Vec::new(),
    };
    for parameter in template {
        // Skip parameters AE keeps hidden (PF_PUI_INVISIBLE / conditionally
        // hidden), matching the harness (main.rs:4181): exposing a control for
        // one, and sending edited values, would override a parameter the effect
        // intends to keep private. It stays at the AEX default.
        if !parameter.visible {
            continue;
        }
        if let Some(item) = config_item_for(parameter) {
            let mut sent = parameter.clone();
            // A popup's valid range is 1..=choices.len() (AE popups are 1-based).
            // Discovery falls the range back to the default when the plug-in sets
            // no valid_min/max, so without this a selection above that degenerate
            // maximum would be rejected as out-of-range by the payload encoder,
            // invalidating the session. Normalize the range to cover every choice.
            if sent.kind == "integer" && !sent.choices.is_empty() {
                let count = sent.choices.len() as f64;
                sent.minimum = 1.0;
                sent.maximum = count;
                // Guard a malformed/absent popup default (discovery reports 0 when
                // the "default" field is missing) so the baseline stays in range.
                sent.value = sent.value.clamp(1.0, count);
            }
            exposed.items.push(item);
            exposed.slots.push(parameter.slot);
            exposed.defaults.push(sent);
        }
    }
    exposed
}

/// Maps one discovered AEX parameter to an AviUtl2 config item, or `None` for
/// kinds not yet exposed (angle, point, layer, comp, button, custom, group
/// markers, …), which keep their discovered default.
///
/// Discovery reports a parameter's *runtime* kind (`image_render.rs`
/// `runtime_kind`), which collapses AE's slider/checkbox/popup (param types
/// 1/4/7) all into `"integer"`. So a checkbox is recognised here as an integer
/// spanning exactly 0..1 with no choices, not by a dedicated kind string.
fn config_item_for(parameter: &InteractiveParameter) -> Option<FilterConfigItem> {
    let name = parameter.name.clone();
    match parameter.kind.as_str() {
        "float" => {
            let (min, max) = bounded_range(parameter)?;
            Some(track(name, parameter.value, min, max, track_step(max - min)))
        }
        // "angle" is deferred to stage 2b: its value lives in `components[0]`
        // (not `value`), it usually reports no numeric bounds (so a slider needs
        // a fallback degree range), and multi-turn angles do not fit a clamped
        // slider. Exposing it needs a fixture to verify; for now it stays at the
        // AEX default.
        "integer" => {
            // A popup arrives here as an integer carrying its choice labels. AE
            // popup values are 1-based (1..=N), matching the harness's ComboBox
            // (main.rs:4298-4313). Expose it as a dropdown rather than a slider.
            if !parameter.choices.is_empty() {
                let items = parameter
                    .choices
                    .iter()
                    .enumerate()
                    .map(|(index, label)| FilterConfigSelectItem {
                        name: label.clone(),
                        value: index as i32 + 1,
                    })
                    .collect();
                // Clamp the default to a real item value (1..=N) so a malformed
                // popup default — discovery reports 0 when the "default" field is
                // absent — still selects a valid choice, consistent with the
                // range normalized onto the sent parameter in `exposed_config`.
                let count = parameter.choices.len() as i32;
                return Some(FilterConfigItem::Select(FilterConfigSelect {
                    name,
                    value: (parameter.value as i32).clamp(1, count),
                    items,
                }));
            }
            let (min, max) = bounded_range(parameter)?;
            if min == 0.0 && max == 1.0 {
                // An AE checkbox arrives here as an integer 0..1; expose it as a
                // checkbox rather than a two-tick slider.
                Some(FilterConfigItem::Checkbox(FilterConfigCheckbox {
                    name,
                    value: parameter.value != 0.0,
                }))
            } else {
                Some(track(name, parameter.value.round(), min, max, 1.0))
            }
        }
        "color" => Some(FilterConfigItem::Color(FilterConfigColor {
            name,
            // InteractiveParameter.color is ARGB ([alpha, red, green, blue]);
            // AviUtl2 wants 0x00RRGGBB.
            value: FilterConfigColorValue(
                ((parameter.color[1] as u32) << 16)
                    | ((parameter.color[2] as u32) << 8)
                    | (parameter.color[3] as u32),
            ),
        })),
        _ => None,
    }
}

/// A clamped AviUtl2 track (slider) config item.
fn track(name: String, value: f64, min: f64, max: f64, step: f64) -> FilterConfigItem {
    FilterConfigItem::Track(FilterConfigTrack {
        name,
        value: value.clamp(min, max),
        range: min..=max,
        step,
        zero_display: None,
        slider_ratio: 1.0,
    })
}

/// A finite, strictly-increasing range for a numeric parameter, or `None` when
/// the reported bounds are degenerate (the parameter then keeps its default,
/// unexposed).
fn bounded_range(parameter: &InteractiveParameter) -> Option<(f64, f64)> {
    let (min, max) = (parameter.minimum, parameter.maximum);
    (min.is_finite() && max.is_finite() && min < max).then_some((min, max))
}

/// AviUtl2's track `step` must be a power-of-ten unit (1.0 / 0.1 / 0.01 / 0.001
/// per `filter2.h`); an arbitrary fraction is snapped by the host, distorting
/// the slider (a 0.255 step over 0..255 rendered as a 0..1000 slider). Pick the
/// largest such unit that still gives at least ~100 divisions over `span`.
fn track_step(span: f64) -> f64 {
    for step in [1.0, 0.1, 0.01] {
        if span / step >= 100.0 {
            return step;
        }
    }
    0.001
}

/// Overlays the object's current config values onto `parameters`. `config` is
/// the slice AviUtl2 hands `proc_video`, parallel to the declared config items
/// and to `slots`, so each entry updates the parameter at the matching slot.
fn apply_config_values(
    parameters: &mut [InteractiveParameter],
    slots: &[u32],
    config: &[FilterConfigItem],
) {
    for (item, &slot) in config.iter().zip(slots) {
        let Some(parameter) = parameters.iter_mut().find(|p| p.slot == slot) else {
            continue;
        };
        match item {
            // An integer track is rounded (the encoder rejects the whole payload
            // on a fractional integer value); every other track drives `value`.
            // (Angle is not exposed in stage 2a, so no components handling here.)
            FilterConfigItem::Track(track) => {
                if parameter.kind == "integer" {
                    parameter.value = track.value.round();
                } else {
                    parameter.value = track.value;
                }
            }
            FilterConfigItem::Checkbox(check) => {
                parameter.value = if check.value { 1.0 } else { 0.0 }
            }
            // A popup's selected value is its (1-based) choice index.
            FilterConfigItem::Select(select) => parameter.value = f64::from(select.value),
            FilterConfigItem::Color(color) => {
                // color is ARGB ([alpha, red, green, blue]); update RGB, keep alpha.
                let (r, g, b) = color.value.to_rgb();
                parameter.color[1] = r;
                parameter.color[2] = g;
                parameter.color[3] = b;
            }
            // config_item_for emits only Track/Checkbox/Color, so no other
            // variant is ever handed back.
            _ => {}
        }
    }
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
