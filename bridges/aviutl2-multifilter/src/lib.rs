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
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use aexcompat_broker::image_render::{
    InteractiveParameter, RenderGpuBackend, RenderPixelFormat, encode_interactive_payload,
    inspect_experimental_with_approved_dependencies_and_resources,
};
use aexcompat_broker::plugin_dependency_closure::{
    DependencyClosureRequest, DependencyProvenance, ResolvedDependencyClosure,
    resolve_dependency_closure, survey_dependency_closure,
};
use aexcompat_broker::render_session::{
    ClusterRenderPlugins, DiscoverySession, DiscoverySessionOpenRequest, FrameStatus,
    InspectOutcome, RenderSession, SessionOpenRequest, SwapOutcome,
};
use aexcompat_broker::sealed_load_tree::SealedResourceEntry;
use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
use aexcompat_broker::worker_module_audit::MAX_AUDITED_MODULES as ONESHOT_AUDIT_MODULE_LIMIT;
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
const CLUSTER_MODULE_HEADROOM: usize = 256;
/// The one-shot module-audit cap (the broker's `MAX_AUDITED_MODULES`): total
/// modules across every category in one snapshot. A singleton whose closure
/// cannot fit it is exactly the case the cluster session's declared-set
/// audit exists for (issue #362), so discovery routes it to a one-member
/// cluster session instead of the one-shot inspect.
/// Estimated non-declared modules in a one-shot audit snapshot (the worker
/// image plus the System32/WinSxS tail), measured ~65 for built-in AE
/// effects. A singleton with `deps + SYSTEM_TAIL_ESTIMATE` over the one-shot
/// cap would fail the audit there, so it goes to a one-member cluster
/// session whose module bound is declared instead (issue #362).
const SYSTEM_TAIL_ESTIMATE: usize = 66;
/// Per-inspect watchdog deadline for a cluster discovery session (design §7).
/// The one-shot inspect carries no deadline (#354: mapping a large closure
/// must not be decided by wall-clock), so this stays generous — its job is to
/// catch a hung resident worker, not to time a plugin.
const CLUSTER_INSPECT_DEADLINE: Duration = Duration::from_secs(300);

/// References to every registered filter's session map, so `UninitializePlugin`
/// can drain them on plugin unload/reload (each `FilterCtx` is leaked `'static`,
/// so nothing else would close its worker threads if AviUtl2 unloads the plugin
/// without exiting the process).
static SESSION_MAPS: Mutex<Vec<&'static SessionMap>> = Mutex::new(Vec::new());

/// One registered AEX in a closure-identity cluster (issue #405): what the
/// render pool needs to put the member into a cluster manifest — its path,
/// its discovered SHA-256, and its exposed defaults (the swap payload).
#[derive(Clone)]
struct ClusterMember {
    plugin: PathBuf,
    sha: String,
    smart: bool,
    defaults: Vec<InteractiveParameter>,
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
pub extern "C" fn InitializeLogger(_logger: *mut aviutl2_sys::logger2::LOG_HANDLE) {}

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

include!("discovery_cache.rs");
include!("discovery_inspection.rs");
include!("runtime.rs");
include!("tests.rs");
