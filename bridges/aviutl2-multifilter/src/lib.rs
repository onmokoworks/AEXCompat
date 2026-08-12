//! AviUtl2 generic (`.aux2`) plugin: register each AEX in a folder as its own
//! keyframeable filter (issue #295).
//!
//! At load, scan `AEXCOMPAT_MULTIFILTER_DIR` for `*.aex`, discover each plug-in's
//! parameters via the broker, and register one AviUtl2 filter per AEX. Each
//! filter's config items (trackbar / checkbox / dropdown / color) are built from
//! the discovered parameters, so they are real, keyframeable AviUtl2 parameters
//! (AviUtl2 freezes a filter's config set at load — the per-AEX-filter design is
//! the only way to get keyframeable params for a runtime-chosen AEX).
//!
//! AviUtl2 gives `func_proc_video` no per-filter context and no `effect_id ->
//! filter` mapping, so N filters need N distinct C function pointers; libffi
//! mints one C callback per AEX at runtime (unbounded N), each capturing that
//! AEX's identity + its config-item pointers + its render session.
//!
//! Deployed as `.aux2` (generic plugin extension); `.auf2` would make AviUtl2
//! look for the single-filter `GetFilterPluginTable` export and fail.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use aexcompat_broker::companion_manifest::{ApprovedCompanion, CompanionSuiteIdentity};
use aexcompat_broker::image_render::{
    InteractiveParameter, RenderGpuBackend, RenderPixelFormat,
    initialize_experimental_aegp_in_place, inspect_experimental_cleanup_contained_in_place,
    inspect_experimental_in_place,
};
use aexcompat_broker::plugin_dependency_closure::{
    DependencyProvenance, survey_dependency_closure,
};
use aexcompat_broker::render_session::{
    ClusterRenderPlugins, DiscoverySession, FrameStatus, InPlaceDiscoverySessionOpenRequest,
    InspectOutcome, RenderSession, SessionLayer, SessionOpenRequest, SwapOutcome,
    validate_abandoned_smart_heap_corruption_close, validate_abandoned_smart_untouched_close,
    validate_completed_session_close,
};
use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
use aviutl2_sys::filter2::{
    FILTER_ITEM_CHECKBOX, FILTER_ITEM_COLOR, FILTER_ITEM_COLOR_VALUE, FILTER_ITEM_SELECT,
    FILTER_ITEM_SELECT_ITEM, FILTER_ITEM_TRACK, FILTER_PLUGIN_TABLE, FILTER_PROC_VIDEO,
    OBJECT_INFO, PIXEL_RGBA, SCENE_INFO,
};
use aviutl2_sys::plugin2::{COMMON_PLUGIN_TABLE, HOST_APP_TABLE};
use libffi::low;
use libffi::middle::{Cif, Closure, Type};
use sha2::{Digest, Sha256};

/// AviUtl2 minimum supported version (matches the aviutl2 crate constant).
const REQUIRED_VERSION: u32 = 2010100;
/// Folder scanned at load for `*.aex`; each becomes its own filter. Overrides
/// the TOML `dir` when set.
const ENV_DIR: &str = "AEXCOMPAT_MULTIFILTER_DIR";
/// Repo root holding the built workers (`target/minihost-build/`). Overrides the
/// TOML `repository` when set.
const ENV_REPOSITORY: &str = "AEXCOMPAT_MULTIFILTER_REPOSITORY";
/// Explicit TOML config path; overrides the default location when set.
const ENV_CONFIG: &str = "AEXCOMPAT_MULTIFILTER_CONFIG";
/// `;`-separated folders searched for an AEX's dependency DLLs, overriding the
/// TOML `dependency_dirs` when set (issue #304).
const ENV_DEPENDENCY_DIRS: &str = "AEXCOMPAT_MULTIFILTER_DEPENDENCY_DIRS";
/// Session dimension bounds (mirror the broker's `image_render` limits).
const MAX_DIMENSION: u32 = 4096;
const MAX_PIXELS: u64 = 16_777_216;
/// Per-frame watchdog deadline handed to the session (protocol §7).
const FRAME_DEADLINE_MS: u64 = 30_000;
/// Idle timeout after which an abandoned effect's session (worker + thread +
/// shared memory) is reaped. AviUtl2 has no per-effect teardown callback and
/// `effect_id` is unique per launch, so a deleted/abandoned effect is never
/// revisited; without reaping its session would leak until DLL unload. Well
/// above the frame deadline so an in-flight frame is never reaped.
const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// Monotonic instance counter, so a lost session is removed by the exact
/// instance that failed rather than by `effect_id` alone (a concurrent reopen
/// may have installed a healthy session at the same id).
static SESSION_SERIAL: AtomicU64 = AtomicU64::new(0);

/// Cluster session bounds (issue #405, docs/CLOSURE_SESSION_PROTOCOL_2026-07-23
/// §2.1): a cluster that cannot fit these is structurally infeasible and its
/// members fall back to the per-plugin path. The headroom covers the modules
/// outside the declared set (worker image, System32/WinSxS) that every audit
/// snapshot also counts; the audit's observed union accumulates every visited
/// plug-in plus the whole system tail of a multimedia runtime (measured ~65
/// modules for built-in AE effects, more with GPU stacks), so undersizing it
/// fails an honest session's close-time audit (design §5) for no safety gain —
/// the plugin-class narrowing to the declared set is the actual bound.
const MAX_CLUSTER_PLUGINS: usize = 256;
const MAX_CLUSTER_MODULE_BOUND: usize = 4096;
/// Mirrors `cluster_manifest::MAX_CLUSTER_ADMITTED_DIRS`: an in-place cluster
/// whose search dirs plus member parent directories exceed this cannot launch,
/// so the pool degrades it to per-effect sessions instead of failing every
/// open (issue #751).
const MAX_CLUSTER_ADMITTED_DIRS: usize =
    aexcompat_broker::cluster_manifest::MAX_CLUSTER_ADMITTED_DIRS;
/// Per-inspect watchdog deadline for a cluster discovery session (design §7).
/// Every validated identity now takes this session route, including singleton
/// and sharded-tail members. The bound stays generous: its job is to catch a
/// hung worker, not to classify an otherwise slow plug-in as incompatible.
const CLUSTER_INSPECT_DEADLINE: Duration = Duration::from_secs(300);

/// References to every registered filter's session map, so `UninitializePlugin`
/// can drain them on plugin unload/reload (each `FilterCtx` is leaked `'static`,
/// so nothing else would close its worker threads if AviUtl2 unloads the plugin
/// without exiting the process).
static SESSION_MAPS: Mutex<Vec<&'static SessionMap>> = Mutex::new(Vec::new());

/// One registered AEX in a closure-identity cluster (issue #405): what the
/// render pool needs to put the member into a cluster manifest — its path,
/// its discovered SHA-256, and its supported render route.
#[derive(Clone)]
struct ClusterMember {
    plugin: PathBuf,
    sha: String,
    smart: bool,
    companions: Vec<ApprovedCompanion>,
}

/// Registered AEXes grouped by dependency-closure identity (issue #405),
/// populated at filter registration. The render pool opens one cluster
/// session per (identity, geometry, smart) covering exactly these members.
static CLUSTER_REGISTRY: Mutex<Option<HashMap<String, Vec<ClusterMember>>>> = Mutex::new(None);

/// Pooled cluster render sessions (issue #405, design §8): one session per
/// key, shared by every registered AEX with the same closure identity and
/// render configuration; switching between those effects is a `swap_plugin`
/// inside the session instead of a new worker process. Entries live until
/// `UninitializePlugin` drains them (no idle reaping — the per-effect path
/// keeps its own, and a pooled session serves every object of the cluster).
static SESSION_POOL: Mutex<Option<HashMap<PoolKey, PoolEntry>>> = Mutex::new(None);

#[derive(Clone, PartialEq, Eq, Hash)]
struct PoolKey {
    closure_identity: String,
    geom: GeomIdentity,
    smart: bool,
}

struct PoolEntry {
    session: MfSession,
    /// The manifest order the session was opened with; a member's position
    /// is its `plugin_index` for swaps.
    plugins: Vec<PathBuf>,
}

fn cluster_registry_members(key: &PoolKey, requester: &Path) -> Vec<ClusterMember> {
    let members = CLUSTER_REGISTRY
        .lock()
        .ok()
        .and_then(|registry| {
            registry
                .as_ref()
                .and_then(|registry| registry.get(&key.closure_identity).cloned())
        })
        .unwrap_or_default();
    let mut ordered: Vec<ClusterMember> = members
        .into_iter()
        .filter(|member| member.smart == key.smart)
        .collect();
    // The requester leads the manifest: it is the launch plugin (design
    // §2.2), so the session opens already swapped to whoever asked first.
    ordered.sort_by_key(|member| usize::from(member.plugin != requester));
    ordered
}

/// Returns the sender + serial + this plugin's manifest index of a live
/// pooled session, or `None` to open one.
fn pool_sender(key: &PoolKey, plugin: &Path) -> Option<(Sender<RenderReq>, u64, u32)> {
    let mut guard = SESSION_POOL.lock().ok()?;
    let entry = guard.as_mut()?.get_mut(key)?;
    let index = entry.plugins.iter().position(|path| path == plugin)? as u32;
    entry.session.last_used = Instant::now();
    entry
        .session
        .sender()
        .map(|tx| (tx, entry.session.serial, index))
}

/// Removes and drops the pooled session at `key` matching `serial` (dropped
/// off-lock), so the next frame reopens — matching on serial avoids dropping
/// a healthy session a concurrent reopen installed.
fn pool_remove(key: &PoolKey, serial: u64) {
    let removed = {
        let Ok(mut guard) = SESSION_POOL.lock() else {
            return;
        };
        let map = match guard.as_mut() {
            Some(map) => map,
            None => return,
        };
        if map
            .get(key)
            .is_some_and(|entry| entry.session.serial == serial)
        {
            map.remove(key)
        } else {
            None
        }
    };
    drop(removed);
}

/// A null-terminated UTF-16 string leaked for AviUtl2's lifetime (LPCWSTR).
fn wide_leak(text: &str) -> *const u16 {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.push(0);
    Box::leak(units.into_boxed_slice()).as_ptr()
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

// --- Required generic-plugin exports -------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn RequiredVersion() -> u32 {
    REQUIRED_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn InitializeLogger(logger: *mut aviutl2_sys::logger2::LOG_HANDLE) {
    set_logger(logger);
}

#[unsafe(no_mangle)]
pub extern "C" fn InitializeConfig(_config: *mut aviutl2_sys::config2::CONFIG_HANDLE) {}

#[unsafe(no_mangle)]
pub extern "C" fn InitializeCache(_cache: *mut aviutl2_sys::cache2::CACHE_HANDLE) {}

#[unsafe(no_mangle)]
pub extern "C" fn InitializePlugin(version: u32) -> bool {
    version >= REQUIRED_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn UninitializePlugin() {
    // Stop the background discovery thread (if still warming the cache) and join
    // it, so it does not outlive the plugin or leave an L2 worker orphaned. The
    // flag makes its work-steal loop exit after the current in-flight worker, so
    // the join is bounded by one worker deadline.
    DISCOVERY_SHUTDOWN.store(true, Ordering::Relaxed);
    let discovery = DISCOVERY_THREAD
        .lock()
        .ok()
        .and_then(|mut slot| slot.take());
    if let Some(handle) = discovery {
        let _ = handle.join();
    }

    // Drain every registered filter's sessions so their MfSessions drop: each
    // disconnects its channel, lets the session thread run RenderSession::close,
    // and joins it. Copy the map references out first, then drain each map and
    // drop the sessions after releasing that map's lock (joins run off-lock).
    let maps: Vec<&'static SessionMap> = SESSION_MAPS
        .lock()
        .map(|maps| maps.clone())
        .unwrap_or_default();
    for map in maps {
        let drained: Vec<MfSession> = match map.lock() {
            Ok(mut sessions) => sessions.drain().map(|(_, session)| session).collect(),
            Err(_) => continue,
        };
        drop(drained);
    }

    // Drain the pooled cluster sessions (issue #405) the same way: each
    // MfSession drop disconnects its channel, lets the session thread run
    // RenderSession::close, and joins it.
    let drained: Vec<MfSession> = SESSION_POOL
        .lock()
        .ok()
        .and_then(|mut pool| pool.take())
        .map(|pool| pool.into_values().map(|entry| entry.session).collect())
        .unwrap_or_default();
    drop(drained);
}

#[unsafe(no_mangle)]
pub extern "C" fn GetCommonPluginTable() -> *mut COMMON_PLUGIN_TABLE {
    Box::leak(Box::new(COMMON_PLUGIN_TABLE {
        name: wide_leak("AEXCompat multi-filter"),
        information: wide_leak(
            "Registers each AEX in the configured folder as its own keyframeable filter (issue #295)",
        ),
    }))
}

// --- Host logging --------------------------------------------------------

/// AviUtl2's log sink, handed to the plugin at load. Held so the plugin can say
/// what it resolved and what discovery produced: without it every failure mode
/// (a worker root that no longer exists, an unbuilt worker, a worker regression
/// that fails every plug-in) looks identical from the outside — AviUtl2 starts
/// and the filter list is simply empty (issue #655).
///
/// `AtomicPtr` because the background discovery thread logs too, while the host
/// hands the handle over on the load thread.
static LOGGER: std::sync::atomic::AtomicPtr<aviutl2_sys::logger2::LOG_HANDLE> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// Severity of a line sent to the host log.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LogLevel {
    Info,
    Warn,
}

/// Lines produced before the host handed over its log sink. The SDK does not fix
/// the order of `InitializeLogger` against `RegisterPlugin`, and registration is
/// where the lines that matter most are written ("no worker", "0 registered"), so
/// they are held rather than dropped on the guess that the sink arrives first.
static PENDING_LOG: Mutex<Vec<(LogLevel, String)>> = Mutex::new(Vec::new());

/// Cap on the buffer above. The load thread and the background discovery thread
/// together emit a handful of lines, so the bound is a backstop against retaining
/// unboundedly if a host never calls `InitializeLogger` at all.
const PENDING_LOG_LIMIT: usize = 64;

/// Serializes this plugin's own calls into the host sink. The SDK states no
/// thread affinity for `LOG_HANDLE`, and this plugin has two callers (the load
/// thread and the background discovery thread), so at minimum do not hand the
/// host two concurrent calls of our own making. This does not make the host's
/// implementation re-entrant; it only removes the concurrency we introduce.
///
/// Also held across [`set_logger`]'s flush, so a line produced on another thread
/// during the flush queues behind the held ones instead of overtaking them.
static LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A poisoned log lock means an earlier logger call panicked; the guarded state
/// is the host call itself, so keep logging rather than lose the diagnostic.
fn lock_log() -> std::sync::MutexGuard<'static, ()> {
    LOG_LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn set_logger(handle: *mut aviutl2_sys::logger2::LOG_HANDLE) {
    // Held for the whole publish-and-flush so a concurrent `log_line` that sees
    // the new handle blocks here rather than emitting ahead of the held lines.
    let guard = lock_log();
    // A null sink is nothing to publish and nothing to flush through: `write_line`
    // would dereference it. Leave `LOGGER` alone rather than store the null, so a
    // host that calls this twice (valid, then null) does not un-publish a working
    // sink and strand every later line in the buffer.
    //
    // This does assume a null is a non-answer rather than a revocation. The SDK
    // documents no teardown convention for `LOG_HANDLE` and offers no
    // `UninitializeLogger`, so there is nothing to honour; if a host ever meant it
    // as "stop using the sink", this keeps writing through the old pointer.
    if handle.is_null() {
        return;
    }
    let held: Vec<(LogLevel, String)> = match PENDING_LOG.lock() {
        // The store happens under `PENDING_LOG` because `log_line` reads the
        // handle under it too: without that, a thread can read "null", lose the
        // race to the drain below, and then push into a buffer nothing will ever
        // flush again (`InitializeLogger` runs once).
        Ok(mut pending) => {
            LOGGER.store(handle, Ordering::Release);
            std::mem::take(&mut pending)
        }
        // The buffer is unusable, but publishing the sink still gets every later
        // line through, which beats logging nothing at all.
        Err(_) => {
            LOGGER.store(handle, Ordering::Release);
            Vec::new()
        }
    };
    for (level, message) in held {
        write_line(handle, level, &message);
    }
    drop(guard);
}

/// Writes one line to AviUtl2's log, or holds it until the host hands over a
/// sink (see [`PENDING_LOG`]). On a host that never calls `InitializeLogger` the
/// held lines are simply never emitted.
fn log_line(level: LogLevel, message: &str) {
    // Decide under the buffer lock, so this cannot interleave with the drain in
    // `set_logger` (see there).
    let handle = match PENDING_LOG.lock() {
        Ok(mut pending) => {
            let handle = LOGGER.load(Ordering::Acquire);
            if handle.is_null() {
                if pending.len() < PENDING_LOG_LIMIT {
                    pending.push((level, message.to_owned()));
                }
                return;
            }
            handle
        }
        // Without the buffer there is nowhere to hold a pre-sink line; emit if a
        // sink exists, otherwise drop.
        Err(_) => LOGGER.load(Ordering::Acquire),
    };
    if handle.is_null() {
        return;
    }
    let _guard = lock_log();
    write_line(handle, level, message);
}

/// Hands one line to the host sink. Callers hold [`LOG_LOCK`]; `handle` is
/// non-null.
fn write_line(handle: *mut aviutl2_sys::logger2::LOG_HANDLE, level: LogLevel, message: &str) {
    let mut wide: Vec<u16> = format!("[AEXCompat] {message}").encode_utf16().collect();
    wide.push(0);
    // SAFETY: `handle` is the host's own log handle. The SDK documents no
    // lifetime for it and offers no teardown callback, so this assumes it stays
    // valid until the plugin is unloaded — including during `UninitializePlugin`,
    // where the joined discovery thread writes its last line. `wide` is a
    // null-terminated UTF-16 buffer alive for the call.
    unsafe {
        let sink = match level {
            LogLevel::Info => (*handle).info,
            LogLevel::Warn => (*handle).warn,
        };
        sink(handle, wide.as_ptr());
    }
}

fn log_info(message: &str) {
    log_line(LogLevel::Info, message);
}

fn log_warn(message: &str) {
    log_line(LogLevel::Warn, message);
}

include!("config_ui.rs");
include!("pipl_category.rs");
include!("discovery_cache.rs");
include!("discovery_inspection.rs");
include!("runtime.rs");
include!("tests.rs");
