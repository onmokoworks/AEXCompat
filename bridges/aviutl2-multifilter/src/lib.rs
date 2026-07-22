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

use std::collections::HashMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use aexcompat_broker::image_render::{
    InteractiveParameter, RenderGpuBackend, RenderPixelFormat,
    inspect_experimental_with_diagnostics,
};
use aexcompat_broker::render_session::{FrameStatus, RenderSession, SessionOpenRequest};
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

/// References to every registered filter's session map, so `UninitializePlugin`
/// can drain them on plugin unload/reload (each `FilterCtx` is leaked `'static`,
/// so nothing else would close its worker threads if AviUtl2 unloads the plugin
/// without exiting the process).
static SESSION_MAPS: Mutex<Vec<&'static SessionMap>> = Mutex::new(Vec::new());

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
    let discovery = DISCOVERY_THREAD.lock().ok().and_then(|mut slot| slot.take());
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

/// Plugin configuration, from the TOML config file (see [`config_path`]). Env
/// vars override `dir`/`repository`. Absent fields fall back to those env vars.
#[derive(Default, serde::Deserialize)]
struct Config {
    /// A single folder scanned for `*.aex` (backward-compatible; merged with
    /// `dirs`). Kept so existing single-folder configs keep working.
    dir: Option<PathBuf>,
    /// Folders scanned recursively for `*.aex`. When neither this, `dir`, nor the
    /// env override is set, the default After Effects / MediaCore plug-in folders
    /// are used (see [`default_dirs`]).
    #[serde(default)]
    dirs: Vec<PathBuf>,
    /// Repo root holding the built workers.
    repository: Option<PathBuf>,
    /// Effect names to skip (matched against each AEX's file stem, case- and
    /// `.aex`-extension-insensitive).
    #[serde(default)]
    ignore: Vec<String>,
}

/// The TOML config path: `AEXCOMPAT_MULTIFILTER_CONFIG` if set, else the Windows
/// per-user default `%APPDATA%\aexcompat-multifilter\config.toml`.
fn config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(ENV_CONFIG) {
        return Some(PathBuf::from(path));
    }
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("aexcompat-multifilter")
            .join("config.toml"),
    )
}

/// Loads the config file, or a default (all-absent) config when it is missing or
/// malformed — the plug-in then relies on the env vars, or registers nothing.
fn load_config() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Config::default();
    };
    toml::from_str(&text).unwrap_or_default()
}

/// Drops a trailing `.aex` extension (any case) from an ignore entry so it may
/// be written with or without it. Checks the last four bytes case-insensitively,
/// then slices the `str` only on that ASCII-`.aex` match — where the byte at
/// `len - 4` is `.` and therefore a char boundary — so a multi-byte entry never
/// slices mid-character (that would panic across the `extern "C"` boundary).
fn strip_aex_ext(entry: &str) -> &str {
    let bytes = entry.as_bytes();
    if bytes.len() >= 4 && bytes[bytes.len() - 4..].eq_ignore_ascii_case(b".aex") {
        &entry[..entry.len() - 4]
    } else {
        entry
    }
}

/// Whether `path`'s file stem matches an ignore entry (case-insensitive; an
/// entry may be written with or without the `.aex` extension).
fn is_ignored(path: &Path, ignore: &[String]) -> bool {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    ignore
        .iter()
        .any(|entry| strip_aex_ext(entry).eq_ignore_ascii_case(stem))
}

/// Cap on concurrent discovery workers. Each spawns an L2 worker subprocess that
/// loads the AEX + the compat runtime (memory-heavy). Kept low: too much
/// concurrency causes resource contention that pushes a plain ~2 s discovery past
/// the worker's 5 s deadline, so a discoverable effect times out and is wrongly
/// cached as a non-effect. Discovery runs on a background thread, so a low cap is
/// cheap. (Measured: 8-way ≈ 68% false timeouts, serial ≈ 3%.)
const MAX_DISCOVERY_PARALLELISM: usize = 3;
/// Recursion depth cap for the folder scan (guards symlink loops / pathological
/// trees); the AE plug-in tree is only a few levels deep.
const MAX_SCAN_DEPTH: usize = 8;

/// The background discovery saves the cache after each chunk of this many AEX, so
/// progress survives a restart/shutdown mid-scan (rather than only at the end of a
/// multi-minute scan).
const DISCOVERY_SAVE_CHUNK: usize = 24;

/// Set on plugin unload so the background discovery thread stops promptly.
static DISCOVERY_SHUTDOWN: AtomicBool = AtomicBool::new(false);
/// The background discovery thread's join handle, so `UninitializePlugin` can
/// stop and join it (bounded by one in-flight worker deadline).
static DISCOVERY_THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

#[unsafe(no_mangle)]
pub extern "C" fn RegisterPlugin(host: *mut HOST_APP_TABLE) {
    if host.is_null() {
        return;
    }
    let config = load_config();
    let repository = std::env::var_os(ENV_REPOSITORY)
        .map(PathBuf::from)
        .or_else(|| config.repository.clone());
    let Some(repository) = repository else {
        return;
    };
    let (dirs, dirs_complete) = resolve_scan_dirs(&config);
    if dirs.is_empty() {
        return;
    }

    // Recursively collect the *.aex to expose (minus ignored), deduped + sorted.
    // `scan_complete` is false when a default folder went missing or one could not
    // be read, which makes the background pass keep (rather than prune) the
    // entries it did not see this launch.
    let (plugins, read_complete) = collect_aex(&dirs, &config.ignore);
    let scan_complete = dirs_complete && read_complete;
    if plugins.is_empty() {
        return;
    }

    // Discovery (an L2 worker per AEX) is slow — hundreds of AE effects take
    // minutes — and can only ever populate the cache, since AviUtl2 freezes a
    // filter's config at load and cannot register a filter discovered later. So
    // register from the cache immediately (never blocking startup) and discover
    // the rest on a background thread whose results appear on the NEXT launch.
    // The cache is keyed by AEX (path, mtime, len), but a discovery *result* also
    // depends on the compat host that produced it (the L2 worker and this DLL's
    // in-process broker), so an entry made by an older host is re-verified in the
    // background (issue #304). It is NOT dropped: an unregistered filter makes
    // AviUtl2 drop every object referencing it when a saved project is opened, and
    // saving then deletes those objects for good, so a host rebuild must never
    // empty the filter list for a launch (issue #307).
    let build = build_fingerprint(&repository);
    let cache = load_cache();

    // Register (host callback, main thread only) each AEX whose discovery already
    // succeeded and still matches the file on disk. Anything unknown, changed, or
    // discovered by an older host goes to the background pass; its (updated)
    // result is picked up on the next launch.
    let mut pending: Vec<PathBuf> = Vec::new();
    for plugin in &plugins {
        let cached = cache.get(&plugin.to_string_lossy().into_owned());
        let decision = classify(cached, file_meta(plugin), build);
        if decision.register
            && let Some(entry) = cached
        {
            register_discovered(host, &repository, plugin, entry);
        }
        if decision.discover {
            pending.push(plugin.clone());
        }
    }

    if !pending.is_empty() {
        spawn_background_discovery(repository, plugins, cache, pending, build, scan_complete);
    }
}

/// Drops cache entries for AEX that are no longer present.
///
/// Only when the scan saw every folder: after a folder went missing or could not
/// be read, "not scanned" does not mean "gone", and dropping a live entry would
/// leave that effect unregistered on the next launch, deleting objects from saved
/// projects that use it (issue #307). Keeping a stale entry costs only cache
/// bytes, since registration iterates the scan result, not the cache.
fn prune_cache(cache: &mut HashMap<String, CacheEntry>, plugins: &[PathBuf], scan_complete: bool) {
    if !scan_complete {
        return;
    }
    let present: std::collections::HashSet<String> = plugins
        .iter()
        .map(|plugin| plugin.to_string_lossy().into_owned())
        .collect();
    cache.retain(|key, _| present.contains(key));
}

/// What to do with one AEX at load.
#[derive(PartialEq, Eq, Debug)]
struct LoadDecision {
    /// Register it now from the cached parameters.
    register: bool,
    /// (Re-)discover it on the background thread.
    discover: bool,
}

/// Decides both from the cached entry, the AEX's current `(mtime, len)`, and the
/// current host build.
///
/// The two are independent on purpose: an entry discovered by an older host is
/// still registered while it is re-verified, because not registering it would let
/// AviUtl2 delete every object that uses it out of a saved project (issue #307).
fn classify(
    cached: Option<&CacheEntry>,
    meta: Option<((u64, u32), u64)>,
    build: BuildFingerprint,
) -> LoadDecision {
    let current = cached
        .filter(|entry| meta.is_some_and(|(mtime, len)| entry.mtime == mtime && entry.len == len));
    LoadDecision {
        register: current.is_some_and(|entry| entry.ok),
        discover: current.is_none_or(|entry| entry.build != build),
    }
}

/// Discovers the pending AEX on a background thread and rewrites the cache, so
/// startup is never blocked. Newly-discovered effects appear on the next launch.
fn spawn_background_discovery(
    repository: PathBuf,
    plugins: Vec<PathBuf>,
    mut cache: HashMap<String, CacheEntry>,
    pending: Vec<PathBuf>,
    build: BuildFingerprint,
    scan_complete: bool,
) {
    let handle = std::thread::Builder::new()
        .name("aex-multifilter-discovery".into())
        .spawn(move || {
            // Prune stale entries (removed/renamed AEX) up front so an early
            // shutdown still leaves a pruned cache.
            prune_cache(&mut cache, &plugins, scan_complete);

            // Discover in chunks and save the cache after each, so a restart or
            // shutdown mid-scan keeps the progress so far (effects appear across
            // successive launches) instead of discarding a multi-minute scan. The
            // full scan of hundreds of AE effects can only ever populate the cache
            // — AviUtl2 freezes a filter's config at load — so the results show on
            // the next launch.
            // Each entry carries the build that produced it, so an interrupted pass
            // leaves the not-yet-redone entries on the old build and they are
            // queued again next launch (issue #307).
            for chunk in pending.chunks(DISCOVERY_SAVE_CHUNK) {
                if DISCOVERY_SHUTDOWN.load(Ordering::Relaxed) {
                    break;
                }
                let results = discover_all(&repository, chunk, build);
                let discovered = results.len();
                for (plugin, entry) in results {
                    let key = plugin.to_string_lossy().into_owned();
                    let merged = keep_best(cache.get(&key), entry, file_meta(&plugin));
                    cache.insert(key, merged);
                }
                // discover_all returns fewer than the chunk only if it was cut
                // short by the shutdown flag; save what we have and stop.
                save_cache(&cache);
                if discovered < chunk.len() {
                    break;
                }
            }
        });
    if let Ok(handle) = handle
        && let Ok(mut slot) = DISCOVERY_THREAD.lock()
    {
        // A previous launch's thread cannot exist (RegisterPlugin runs once), so
        // just store this one for UninitializePlugin to join.
        *slot = Some(handle);
    }
}

/// The folders to scan: the env override wins, else `dir` + `dirs` from the
/// config, else the default After Effects / MediaCore plug-in folders.
///
/// The second value is false when a *default* folder could not be resolved this
/// launch (an AE update in progress, a drive not yet mounted). Explicitly
/// configured folders are always "complete": the user named them, so a missing
/// one is their intent, not a probe that failed. See [`collect_aex`] — an
/// incomplete resolution must not let the background pass prune that folder's
/// cache entries, which would unregister hundreds of effects (issue #307).
fn resolve_scan_dirs(config: &Config) -> (Vec<PathBuf>, bool) {
    if let Some(dir) = std::env::var_os(ENV_DIR) {
        return (vec![PathBuf::from(dir)], true);
    }
    let mut dirs: Vec<PathBuf> = config.dir.clone().into_iter().collect();
    dirs.extend(config.dirs.iter().cloned());
    if dirs.is_empty() {
        return default_dirs();
    }
    (dirs, true)
}

/// The default scan folders: the latest installed After Effects `Plug-ins`
/// folder and the shared Adobe MediaCore folder. Only existing paths are kept,
/// and the second value is false if either one could not be resolved, so a
/// transiently invisible AE install is not mistaken for "these effects are gone".
fn default_dirs() -> (Vec<PathBuf>, bool) {
    let (after_effects, ae_complete) = latest_after_effects_plugins();
    let (mediacore, mediacore_complete) = mediacore_dir();
    let complete =
        ae_complete && mediacore_complete && after_effects.is_some() && mediacore.is_some();
    (
        after_effects.into_iter().chain(mediacore).collect(),
        complete,
    )
}

/// `%ProgramFiles%\Adobe`, the root of Adobe app installs.
fn adobe_root() -> Option<PathBuf> {
    let program_files = std::env::var_os("ProgramFiles")?;
    Some(PathBuf::from(program_files).join("Adobe"))
}

/// The newest `Adobe After Effects <year>\Support Files\Plug-ins`, or `None`.
fn latest_after_effects_plugins() -> (Option<PathBuf>, bool) {
    let Some(adobe) = adobe_root() else {
        return (None, false);
    };
    newest_versioned(&adobe, "Adobe After Effects ", &["Support Files", "Plug-ins"])
}

/// The newest `Adobe\Common\Plug-ins\<version>\MediaCore`, or `None`.
fn mediacore_dir() -> (Option<PathBuf>, bool) {
    let Some(adobe) = adobe_root() else {
        return (None, false);
    };
    let root = adobe.join("Common").join("Plug-ins");
    newest_versioned(&root, "", &["MediaCore"])
}

/// The `leaf` folder under the newest versioned subfolder of `root` whose name
/// starts with `prefix` (e.g. `Adobe After Effects 2025/Support Files/Plug-ins`).
///
/// The second value is false when the pick cannot be trusted to be the newest:
/// the folder could not be enumerated, an entry could not be read, or a versioned
/// install was present but its `leaf` was not. That last case is what an install
/// being updated looks like, and silently falling back to an older version while
/// reporting a complete scan would make the newer version's plug-ins look
/// deleted — which prunes their cache entries and unregisters them on the next
/// launch, deleting objects from saved projects that use them (issue #307).
fn newest_versioned(root: &Path, prefix: &str, leaf: &[&str]) -> (Option<PathBuf>, bool) {
    let Ok(read) = std::fs::read_dir(root) else {
        return (None, false);
    };
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    let mut complete = true;
    for entry in read {
        let Ok(entry) = entry else {
            complete = false;
            continue;
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(version) = name.strip_prefix(prefix) else {
            continue;
        };
        // Ignore entries that are not versioned at all (a stray file, an
        // unrelated Adobe app); only a real version folder missing its leaf is
        // evidence that this launch is seeing an incomplete install.
        if !version.split(['.', ' ']).any(|part| part.parse::<u64>().is_ok()) {
            continue;
        }
        let mut candidate = entry.path();
        candidate.extend(leaf);
        if !candidate.is_dir() {
            complete = false;
            continue;
        }
        let key = version_key(version);
        if best.as_ref().is_none_or(|(best_key, _)| key > *best_key) {
            best = Some((key, candidate));
        }
    }
    (best.map(|(_, path)| path), complete)
}

/// Recursively collects `*.aex` under `dirs` (minus ignored), deduped + sorted.
/// Returns the AEX found, and whether the scan actually saw every folder. An
/// incomplete scan (a folder that could not be read, a tree deeper than
/// [`MAX_SCAN_DEPTH`]) must not be used to conclude an AEX is gone: pruning its
/// cache entry would leave the effect unregistered on the next launch, which
/// deletes objects from saved projects that use it (issue #307).
fn collect_aex(dirs: &[PathBuf], ignore: &[String]) -> (Vec<PathBuf>, bool) {
    let mut found = Vec::new();
    let mut complete = true;
    for dir in dirs {
        complete &= collect_aex_into(dir, ignore, 0, &mut found);
    }
    found.sort();
    found.dedup();
    (found, complete)
}

/// Returns false if any part of this subtree could not be enumerated.
fn collect_aex_into(dir: &Path, ignore: &[String], depth: usize, out: &mut Vec<PathBuf>) -> bool {
    if depth > MAX_SCAN_DEPTH {
        return false;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut complete = true;
    for entry in read {
        // An entry the iterator itself could not yield is a partially enumerated
        // folder; flattening it away would report the scan as complete and let
        // the prune drop that AEX's entry (issue #307).
        let Ok(entry) = entry else {
            complete = false;
            continue;
        };
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            complete = false;
            continue;
        };
        if file_type.is_dir() {
            complete &= collect_aex_into(&path, ignore, depth + 1, out);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("aex"))
            && !is_ignored(&path, ignore)
        {
            out.push(path);
        }
    }
    complete
}

// --- Per-AEX discovery + registration ------------------------------------

/// Reads one config item's current (keyframed) value back into the parameter it
/// drives. AviUtl2 updates each item struct's value right before proc_video.
enum ItemReader {
    Track { ptr: *const FILTER_ITEM_TRACK, slot: u32, integer: bool },
    Checkbox { ptr: *const FILTER_ITEM_CHECKBOX, slot: u32 },
    Select { ptr: *const FILTER_ITEM_SELECT, slot: u32 },
    Color { ptr: *const FILTER_ITEM_COLOR, slot: u32 },
}

/// The launch-fixed geometry/time of a session (the AEX identity is fixed per
/// FilterCtx). A frame whose object geometry or timing differs needs a fresh
/// session, so this is compared per frame.
#[derive(Clone, PartialEq, Eq)]
struct GeomIdentity {
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
    Rendered(RenderedFrame),
    /// A frame-local diagnostic; the session stays usable, leave pixels.
    FrameLocal(i64),
    /// The session/worker is gone; the caller drops it so the next frame reopens.
    SessionLost(String),
}

/// A render request sent to a session's owning thread.
struct RenderReq {
    current_time: i32,
    rgba: Vec<u8>,
    parameters: Option<Vec<InteractiveParameter>>,
    reply: Sender<FrameReply>,
}

/// Handle to a resident session. `RenderSession` is `!Send` (it holds the
/// shared-memory view pointer), so it stays pinned to `join`'s thread and is
/// reached only through `tx`. This handle is `Send`, so the per-AEX session map
/// can live behind a `Mutex` reached from any AviUtl2 callback thread.
struct MfSession {
    tx: Option<Sender<RenderReq>>,
    identity: GeomIdentity,
    serial: u64,
    last_used: Instant,
    join: Option<JoinHandle<()>>,
}

impl MfSession {
    fn sender(&self) -> Option<Sender<RenderReq>> {
        self.tx.clone()
    }
}

impl Drop for MfSession {
    fn drop(&mut self) {
        // Drop the sender first so the owning thread's `rx.recv()` returns and it
        // closes the session; joining before disconnecting would deadlock.
        self.tx = None;
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Per-filter userdata carried by the libffi closure. One per registered AEX,
/// captured by that AEX's single closure and reached as `&FilterCtx` through the
/// C callback boundary from any AviUtl2 callback thread.
///
/// `FilterCtx` is not auto-`Send + Sync` (the `ItemReader`s hold `*const
/// FILTER_ITEM_*` raw pointers, which are `!Sync`), and no `unsafe impl` claims
/// otherwise — the C boundary erases the check. Sharing it across threads is
/// nonetheless sound because: the only genuinely thread-unsafe state, the
/// `!Send` `RenderSession`, never leaves its owning session thread (only the
/// `Send` `Sender<RenderReq>` crosses threads); the session map is behind a
/// `Mutex`; and the raw item pointers address leaked `'static` (process-global)
/// memory that is only ever read in `apply_readers`, on the calling AviUtl2
/// thread. Do not move `readers`/`apply_readers` onto the session thread.
struct FilterCtx {
    repository: PathBuf,
    plugin: PathBuf,
    sha: String,
    smart: bool,
    /// Exposed parameter defaults (normalized), cloned per frame as the baseline.
    defaults: Vec<InteractiveParameter>,
    /// Readers pulling each frame's current config value into the parameters.
    readers: Vec<ItemReader>,
    /// Live sessions keyed by AviUtl2 `effect_id`, so two objects of the same
    /// AEX filter each get their own session/worker (no cross-object thrash).
    sessions: Mutex<HashMap<i64, MfSession>>,
}

/// The cached discovery result for one AEX. Keyed in the cache file by the AEX
/// path; `(mtime, len)` invalidates the entry when the file changes. `ok` records
/// a non-discoverable `.aex` (e.g. a format/codec plug-in, not an effect) so it
/// is skipped without being re-probed every launch.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct CacheEntry {
    mtime: (u64, u32),
    len: u64,
    ok: bool,
    sha: String,
    smart: bool,
    #[serde(default)]
    params: Vec<InteractiveParameter>,
    /// The host build that produced this entry. Held per entry, not per file, so
    /// an interrupted re-verification pass leaves the not-yet-redone entries
    /// carrying the old build and they are queued again on the next launch.
    #[serde(default)]
    build: BuildFingerprint,
}

/// Invalidates cache files whose [`CacheEntry`] shape can no longer be trusted
/// field-for-field.
///
/// **Avoid bumping this.** A bump discards every entry, so that launch registers
/// no filters at all, and opening a saved project that uses them makes AviUtl2
/// drop those objects — saving then deletes them for good (issue #307). Extend
/// the schema additively instead: a new field with `#[serde(default)]` reads old
/// cache files safely and needs no bump (this is how `CacheEntry::build` was
/// added). Bump only if an existing field's meaning changes, which is a real
/// data-loss risk that has to be weighed rather than done reflexively.
const CACHE_VERSION: u32 = 1;

/// Fingerprints the compat host that produces a discovery result, so an entry can
/// be re-verified when the host changes (e.g. it gains support for an effect that
/// previously failed to load — issue #304). A cached result depends on the host,
/// not just the AEX bytes: on the L2 worker exe that loads the AEX and runs
/// `EffectMain`, and on this multifilter DLL, whose in-process broker does the
/// sealed-load-tree staging and dispatch that decide whether a load even succeeds.
/// `(mtime_secs, mtime_nanos, len)` per file; `None` when a file cannot be stat'd.
#[derive(serde::Serialize, serde::Deserialize, Default, PartialEq, Eq, Clone, Copy, Debug)]
struct BuildFingerprint {
    #[serde(default)]
    worker: Option<(u64, u32, u64)>,
    #[serde(default)]
    host: Option<(u64, u32, u64)>,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct CacheFile {
    version: u32,
    entries: HashMap<String, CacheEntry>,
}

/// The discovery cache path, next to the config in `%APPDATA%`.
fn cache_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("aexcompat-multifilter")
            .join("discovery-cache.json"),
    )
}

/// Fingerprints the L2 discovery worker and this multifilter DLL. Either side can
/// change a discovery result: the worker exe loads the AEX and runs `EffectMain`,
/// while the in-DLL broker does the sealed-load-tree staging that decides whether
/// the load succeeds (issue #304's dep-sealing lives broker-side, in this DLL).
fn build_fingerprint(repository: &Path) -> BuildFingerprint {
    let worker = repository
        .join("target")
        .join("minihost-build")
        .join("aex_l2_worker.exe");
    let flatten = |m: ((u64, u32), u64)| (m.0.0, m.0.1, m.1);
    BuildFingerprint {
        worker: file_meta(&worker).map(flatten),
        host: self_module_path().as_deref().and_then(file_meta).map(flatten),
    }
}

/// The path of this running DLL, resolved from an address inside it. Used to
/// fingerprint the in-process broker (its bytes ship in this module, not the
/// worker exe), so a rebuilt-and-redeployed DLL re-verifies the discovery cache.
fn self_module_path() -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    // GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | _UNCHANGED_REFCOUNT: resolve the
    // module owning `addr` without touching its refcount (no matching FreeLibrary).
    const FROM_ADDRESS_UNCHANGED: u32 = 0x0000_0004 | 0x0000_0002;
    unsafe extern "system" {
        fn GetModuleHandleExW(flags: u32, addr: *const u16, module: *mut isize) -> i32;
        fn GetModuleFileNameW(module: isize, buf: *mut u16, size: u32) -> u32;
    }

    let anchor = self_module_path as *const () as *const u16;
    let mut module: isize = 0;
    // SAFETY: `anchor` points into this module's code; out-params are valid.
    if unsafe { GetModuleHandleExW(FROM_ADDRESS_UNCHANGED, anchor, &mut module) } == 0 {
        return None;
    }
    let mut buf = [0u16; 32768];
    // SAFETY: `module` is a valid HMODULE from the call above; `buf` is sized.
    let len = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    // 0 = failure; len == buf.len() means truncation (path longer than the buffer).
    if len == 0 || len >= buf.len() {
        return None;
    }
    Some(PathBuf::from(OsString::from_wide(&buf[..len])))
}

fn load_cache() -> HashMap<String, CacheEntry> {
    let Some(path) = cache_path() else {
        return HashMap::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    accept_cache_file(serde_json::from_str(&text).unwrap_or_default())
}

/// Only a schema-version mismatch discards entries: the older shape cannot be
/// trusted field-for-field. A host-build change does NOT discard them — each entry
/// carries its own build ([`CacheEntry::build`]) and is re-verified in the
/// background while still being registered, so no filter disappears for a launch
/// (issue #307).
fn accept_cache_file(file: CacheFile) -> HashMap<String, CacheEntry> {
    if file.version == CACHE_VERSION {
        file.entries
    } else {
        HashMap::new()
    }
}

fn save_cache(entries: &HashMap<String, CacheEntry>) {
    let Some(path) = cache_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = CacheFile {
        version: CACHE_VERSION,
        // Clone is unavoidable through the borrow; the cache is small vs the AEX
        // bytes and this runs once per launch.
        entries: entries
            .iter()
            .map(|(key, entry)| (key.clone(), entry.clone()))
            .collect(),
    };
    // Write atomically (temp + rename) so a crash or process exit mid-write (the
    // background thread can still be writing when AviUtl2 quits) never leaves a
    // truncated, unparseable cache file behind. The temp name carries the PID so
    // two AviUtl2 instances do not clobber each other's temp before the rename.
    if let Ok(text) = serde_json::to_string(&file) {
        let temp = path.with_file_name(format!("discovery-cache.{}.tmp", std::process::id()));
        if std::fs::write(&temp, text).is_ok() && std::fs::rename(&temp, &path).is_err() {
            let _ = std::fs::remove_file(&temp);
        }
    }
}

/// `(mtime, len)` for cache invalidation; `mtime` degrades to `(0, 0)` if the
/// platform cannot report it (then `len` alone guards, as on the aviutl2 bridge).
fn file_meta(path: &Path) -> Option<((u64, u32), u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| (d.as_secs(), d.subsec_nanos()))
        .unwrap_or((0, 0));
    Some((mtime, meta.len()))
}

/// Merges a freshly discovered entry over the cached one, refusing to demote an
/// unchanged AEX from `ok = true` to `ok = false`.
///
/// Re-verification (a host build change) re-runs discovery on AEX that already
/// discovered fine, and discovery has a fixed per-AEX worker deadline, so a
/// transient failure — a timeout under load, with the re-verification pass now
/// competing with the user's own editing and rendering — would otherwise rewrite a
/// working effect as a negative. It would then not be registered on the next
/// launch, and opening a saved project that uses it silently drops those objects
/// for good (issue #307). Keeping the old result instead is the safe direction:
/// if the host really did regress, the effect fails at render time with a
/// diagnostic, which is visible and recoverable, unlike a deleted object.
///
/// A negative only wins when `meta` *proves* the AEX changed. `meta` is the file's
/// current `(mtime, len)`, or `None` when it could not be stat'd — a transient
/// condition (an AV scanner's sharing violation, a plug-in being replaced) that
/// must not be read as "replaced", or the same irreversible deletion follows.
/// The new `build` is always taken, so re-verification still converges and the
/// entry is not queued again on the next launch.
fn keep_best(
    cached: Option<&CacheEntry>,
    discovered: CacheEntry,
    meta: Option<((u64, u32), u64)>,
) -> CacheEntry {
    let Some(old) = cached else {
        return discovered;
    };
    if !old.ok || discovered.ok {
        return discovered;
    }
    let replaced = meta.is_some_and(|(mtime, len)| old.mtime != mtime || old.len != len);
    if replaced {
        discovered
    } else {
        CacheEntry {
            build: discovered.build,
            ..old.clone()
        }
    }
}

/// A negative (`ok = false`) cache entry for a plug-in that failed discovery.
fn negative_entry(plugin: &Path, build: BuildFingerprint) -> CacheEntry {
    let (mtime, len) = file_meta(plugin).unwrap_or(((0, 0), 0));
    CacheEntry {
        mtime,
        len,
        ok: false,
        sha: String::new(),
        smart: false,
        params: Vec::new(),
        build,
    }
}

/// Discovers one AEX, always returning a cache entry (cache-all): `ok = true` for
/// a discoverable effect, `ok = false` for any failure (a genuine non-effect, or
/// an AEX the compat host cannot load, or a timeout). Discovery runs on the
/// background thread, so caching every outcome — even a timeout — means it is not
/// re-probed on later launches; a spurious negative is cleared by re-touching the
/// AEX or deleting the cache file (documented in the README).
fn discover_one(repository: &Path, plugin: &Path, build: BuildFingerprint) -> CacheEntry {
    let mut entry = negative_entry(plugin, build);
    let Ok(bytes) = std::fs::read(plugin) else {
        return entry;
    };
    entry.sha = hex_lower(&Sha256::digest(&bytes));
    if let Ok((params, diagnostics)) =
        inspect_experimental_with_diagnostics(repository, plugin, &entry.sha)
    {
        // PF_OutFlag2_SUPPORTS_SMART_RENDER = bit 10.
        entry.smart = diagnostics
            .get("advertised_out_flags2")
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
            & (1 << 10)
            != 0;
        entry.params = params;
        entry.ok = true;
    }
    entry
}

/// Discovers the given AEX with low bounded parallelism (to keep each discovery
/// under the worker deadline — high concurrency causes contention false-timeouts),
/// work-stealing over the slice and caching every result. Stops promptly when
/// `DISCOVERY_SHUTDOWN` is set (plugin unload); unprocessed paths stay misses and
/// are retried next launch. A panic in `discover_one` (arbitrary third-party AEX)
/// is caught and turned into a negative entry, so one bad plug-in cannot abort the
/// process by unwinding out of the scoped thread.
fn discover_all(
    repository: &Path,
    paths: &[PathBuf],
    build: BuildFingerprint,
) -> Vec<(PathBuf, CacheEntry)> {
    let parallelism = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(MAX_DISCOVERY_PARALLELISM)
        .min(paths.len().max(1));
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(PathBuf, CacheEntry)>> = Mutex::new(Vec::with_capacity(paths.len()));
    std::thread::scope(|scope| {
        for _ in 0..parallelism {
            scope.spawn(|| {
                loop {
                    if DISCOVERY_SHUTDOWN.load(Ordering::Relaxed) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= paths.len() {
                        break;
                    }
                    let plugin = &paths[index];
                    let entry = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        discover_one(repository, plugin, build)
                    }))
                    .unwrap_or_else(|_| negative_entry(plugin, build));
                    if let Ok(mut results) = results.lock() {
                        results.push((plugin.clone(), entry));
                    }
                }
            });
        }
    });
    results.into_inner().unwrap_or_else(|poison| poison.into_inner())
}

/// A numerically-comparable key for a version token ("25.0" > "7.0", unlike a
/// lexical compare), falling back to 0 for non-numeric components.
fn version_key(version: &str) -> Vec<u64> {
    version
        .split(['.', ' '])
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

/// Registers one discovered AEX as an AviUtl2 filter. Runs on the RegisterPlugin
/// (host callback) thread only.
fn register_discovered(
    host: *mut HOST_APP_TABLE,
    repository: &Path,
    plugin: &Path,
    entry: &CacheEntry,
) {
    // Build config items + readers + normalized defaults from the exposed params.
    let mut items: Vec<*const c_void> = Vec::new();
    let mut readers: Vec<ItemReader> = Vec::new();
    let mut defaults: Vec<InteractiveParameter> = Vec::new();
    for parameter in &entry.params {
        if !parameter.visible {
            continue;
        }
        if let Some((item_ptr, reader, sent)) = build_item(parameter) {
            items.push(item_ptr);
            readers.push(reader);
            defaults.push(sent);
        }
    }
    items.push(std::ptr::null());
    let items: &'static [*const c_void] = Box::leak(items.into_boxed_slice());

    let userdata = Box::leak(Box::new(FilterCtx {
        repository: repository.to_path_buf(),
        plugin: plugin.to_path_buf(),
        sha: entry.sha.clone(),
        smart: entry.smart,
        defaults,
        readers,
        sessions: Mutex::new(HashMap::new()),
    }));
    // Register this filter's session map so UninitializePlugin can drain it.
    if let Ok(mut maps) = SESSION_MAPS.lock() {
        maps.push(&userdata.sessions);
    }

    let cif = Cif::new([Type::pointer()], Type::u8());
    let closure = Box::leak(Box::new(Closure::new(cif, render_callback, userdata)));
    let code: unsafe extern "C" fn() = *closure.code_ptr();
    let func_proc_video: extern "C" fn(*mut FILTER_PROC_VIDEO) -> bool =
        unsafe { std::mem::transmute(code) };

    let name = plugin
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("AEX");
    let table = Box::leak(Box::new(FILTER_PLUGIN_TABLE {
        flag: 1 | 8, // FLAG_VIDEO | FLAG_FILTER
        name: wide_leak(&format!("AEX: {name}")),
        label: std::ptr::null(),
        information: wide_leak(&format!("AEXCompat multi-filter: {name}")),
        items: items.as_ptr(),
        func_proc_video: Some(func_proc_video),
        func_proc_audio: None,
    }));

    unsafe { ((*host).register_filter_plugin)(table) };
}

/// Build one leaked FILTER_ITEM for an exposed parameter, plus a reader and the
/// (range-normalized) parameter to send. Mirrors the aviutl2 bridge's mapping.
fn build_item(parameter: &InteractiveParameter) -> Option<(*const c_void, ItemReader, InteractiveParameter)> {
    let name = parameter.name.clone();
    match parameter.kind.as_str() {
        "float" => {
            let (min, max) = bounded_range(parameter)?;
            let ptr = leak_track(&name, parameter.value, min, max, track_step(max - min));
            Some((ptr as *const c_void, ItemReader::Track { ptr, slot: parameter.slot, integer: false }, parameter.clone()))
        }
        "integer" => {
            if !parameter.choices.is_empty() {
                // Popup -> dropdown (AE popups are 1-based).
                let count = parameter.choices.len() as i32;
                let ptr = leak_select(&name, (parameter.value as i32).clamp(1, count), &parameter.choices);
                let mut sent = parameter.clone();
                sent.minimum = 1.0;
                sent.maximum = count as f64;
                sent.value = sent.value.clamp(1.0, count as f64);
                return Some((ptr as *const c_void, ItemReader::Select { ptr, slot: parameter.slot }, sent));
            }
            let (min, max) = bounded_range(parameter)?;
            if min == 0.0 && max == 1.0 {
                let ptr = leak_checkbox(&name, parameter.value != 0.0);
                Some((ptr as *const c_void, ItemReader::Checkbox { ptr, slot: parameter.slot }, parameter.clone()))
            } else {
                let ptr = leak_track(&name, parameter.value.round(), min, max, 1.0);
                Some((ptr as *const c_void, ItemReader::Track { ptr, slot: parameter.slot, integer: true }, parameter.clone()))
            }
        }
        "color" => {
            // InteractiveParameter.color is ARGB; AviUtl2 color code is 0x00RRGGBB.
            let (r, g, b) = (parameter.color[1], parameter.color[2], parameter.color[3]);
            let ptr = leak_color(&name, r, g, b);
            Some((ptr as *const c_void, ItemReader::Color { ptr, slot: parameter.slot }, parameter.clone()))
        }
        // "angle" and others are not exposed (stay at the AEX default).
        _ => None,
    }
}

fn leak_track(name: &str, value: f64, min: f64, max: f64, step: f64) -> *const FILTER_ITEM_TRACK {
    Box::leak(Box::new(FILTER_ITEM_TRACK {
        r#type: wide_leak("track2"),
        name: wide_leak(name),
        value: value.clamp(min, max),
        s: min,
        e: max,
        step,
        zero_display: std::ptr::null(),
        slider_ratio: 1.0,
    }))
}

fn leak_checkbox(name: &str, value: bool) -> *const FILTER_ITEM_CHECKBOX {
    Box::leak(Box::new(FILTER_ITEM_CHECKBOX {
        r#type: wide_leak("check"),
        name: wide_leak(name),
        value,
    }))
}

fn leak_select(name: &str, value: i32, choices: &[String]) -> *const FILTER_ITEM_SELECT {
    let mut list: Vec<FILTER_ITEM_SELECT_ITEM> = choices
        .iter()
        .enumerate()
        .map(|(index, label)| FILTER_ITEM_SELECT_ITEM {
            name: wide_leak(label),
            value: index as i32 + 1,
        })
        .collect();
    // Null-name terminator.
    list.push(FILTER_ITEM_SELECT_ITEM { name: std::ptr::null(), value: 0 });
    let items = Box::leak(list.into_boxed_slice()).as_ptr();
    Box::leak(Box::new(FILTER_ITEM_SELECT {
        r#type: wide_leak("select"),
        name: wide_leak(name),
        value,
        items,
    }))
}

fn leak_color(name: &str, r: u8, g: u8, b: u8) -> *const FILTER_ITEM_COLOR {
    Box::leak(Box::new(FILTER_ITEM_COLOR {
        r#type: wide_leak("color"),
        name: wide_leak(name),
        value: FILTER_ITEM_COLOR_VALUE { bgrx: [b, g, r, 0] },
    }))
}

fn bounded_range(parameter: &InteractiveParameter) -> Option<(f64, f64)> {
    let (min, max) = (parameter.minimum, parameter.maximum);
    (min.is_finite() && max.is_finite() && min < max).then_some((min, max))
}

fn track_step(span: f64) -> f64 {
    for step in [1.0, 0.1, 0.01] {
        if span / step >= 100.0 {
            return step;
        }
    }
    0.001
}

// --- Per-frame render ----------------------------------------------------

unsafe extern "C" fn render_callback(
    _cif: &low::ffi_cif,
    result: &mut u8,
    args: *const *const c_void,
    userdata: &FilterCtx,
) {
    let video = unsafe { *(*args as *const *mut FILTER_PROC_VIDEO) };
    // Never let a panic unwind across the C boundary (that aborts AviUtl2). On
    // panic report failure and leave the frame's pixels unchanged.
    let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render_frame(userdata, video)))
        .unwrap_or(false);
    *result = ok as u8;
}

fn render_frame(ctx: &FilterCtx, video: *mut FILTER_PROC_VIDEO) -> bool {
    if video.is_null() {
        return false;
    }
    let object: *const OBJECT_INFO = unsafe { (*video).object };
    let scene: *const SCENE_INFO = unsafe { (*video).scene };
    if object.is_null() || scene.is_null() {
        return false;
    }
    let width = unsafe { (*object).width };
    let height = unsafe { (*object).height };
    if width <= 0 || height <= 0 {
        return true;
    }
    let (width, height) = (width as u32, height as u32);
    if width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return true; // leave pixels unchanged
    }

    let rate = unsafe { (*scene).rate };
    let scale = unsafe { (*scene).scale };
    if rate <= 0 || scale <= 0 {
        return false;
    }
    let frame = unsafe { (*object).frame };
    let frame_total = unsafe { (*object).frame_total };
    let current_time = (frame as i64 * scale as i64).clamp(0, i32::MAX as i64) as i32;
    let total_time = ((frame_total.max(1)) as i64 * scale as i64).clamp(1, i32::MAX as i64) as i32;
    let time_scale = rate as u32;
    let time_step = scale;

    // Current pixels (RGBA8, packed).
    let count = (width as usize) * (height as usize);
    let mut pixels: Vec<PIXEL_RGBA> =
        (0..count).map(|_| PIXEL_RGBA { r: 0, g: 0, b: 0, a: 0 }).collect();
    unsafe { ((*video).get_image_data)(pixels.as_mut_ptr()) };
    let rgba = pixels_to_bytes(&pixels);

    // Overlay this frame's current config values onto the exposed defaults.
    let parameters = if ctx.defaults.is_empty() {
        None
    } else {
        let mut values = ctx.defaults.clone();
        apply_readers(&mut values, &ctx.readers);
        Some(values)
    };

    let effect_id = unsafe { (*object).effect_id };
    let identity = GeomIdentity { width, height, time_step, total_time, time_scale };

    // Reuse a live matching session, else open one outside the map lock (open
    // blocks for seconds spawning the worker). The blocking render round-trip
    // below runs off-lock too, so concurrent objects/threads never serialize on
    // the map.
    let (tx, serial) = match existing_sender(&ctx.sessions, effect_id, &identity) {
        Some(pair) => pair,
        None => match open_and_get_sender(ctx, effect_id, &identity) {
            Ok(pair) => pair,
            Err(_) => return false,
        },
    };

    match render_on(&tx, current_time, rgba, parameters) {
        FrameReply::Rendered(frame) => {
            // A filter object cannot change the image size; reject a resized frame.
            if frame.width != width || frame.height != height {
                return true;
            }
            let out = bytes_to_pixels(&frame.pixels);
            if out.len() == count {
                unsafe { ((*video).set_image_data)(out.as_ptr(), width as i32, height as i32) };
            }
            true
        }
        // Keep the session; leave this frame's pixels.
        FrameReply::FrameLocal(_) => true,
        FrameReply::SessionLost(_) => {
            // Drop this exact instance so the next frame reopens, without
            // disturbing a healthy session a concurrent reopen may have installed.
            remove_session(&ctx.sessions, effect_id, serial);
            true
        }
    }
}

/// The owned launch config moved into a session's thread.
struct MfSessionConfig {
    repository: PathBuf,
    plugin: PathBuf,
    sha: String,
    smart: bool,
    defaults: Vec<InteractiveParameter>,
    identity: GeomIdentity,
}

/// Opens a session on its own thread, which owns the `!Send` `RenderSession` and
/// serves render requests until the channel closes or the session is lost.
fn open_mf_session(config: MfSessionConfig) -> Result<MfSession, String> {
    let identity = config.identity.clone();
    let (tx, rx) = channel::<RenderReq>();
    let (open_tx, open_rx) = channel::<Result<(), String>>();

    let join = std::thread::Builder::new()
        .name("aex-multifilter-session".into())
        .spawn(move || {
            let baseline = (!config.defaults.is_empty()).then_some(&config.defaults[..]);
            let mut session = match RenderSession::open(SessionOpenRequest {
                repository: &config.repository,
                plugin_path: &config.plugin,
                plugin_sha256: &config.sha,
                parameters: baseline,
                parameter_animation: None,
                aux_manifest: None,
                world_dump_dir: None,
                output_checksum_detail: false,
                mask_trailer: None,
                spatial_trailer: None,
                render_environment_trailer: None,
                alpha_as_coverage_params: &[],
                conformance_render_settings: None,
                layers: &[],
                dependencies: Vec::new(),
                width: config.identity.width,
                height: config.identity.height,
                pixel_format: RenderPixelFormat::Argb8,
                time_step: config.identity.time_step,
                total_time: config.identity.total_time,
                time_scale: config.identity.time_scale,
                frame_deadline: Duration::from_millis(FRAME_DEADLINE_MS),
                smart: config.smart,
                gpu_backend: RenderGpuBackend::Auto,
                gpu_runtime_policy: None,
            }) {
                Ok(session) => session,
                Err(error) => {
                    let _ = open_tx.send(Err(format!("RenderSession::open failed: {error}")));
                    return;
                }
            };
            if open_tx.send(Ok(())).is_err() {
                let _ = session.close();
                return;
            }

            // `frame_index` is a transport serial for the worker's non-advancing
            // check, decoupled from AviUtl2's `object.frame` (the host re-renders
            // and scrubs the same frame); AE time rides `current_time`.
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
                        FrameStatus::Rendered { pixels, width, height, .. } => {
                            FrameReply::Rendered(RenderedFrame { pixels, width, height })
                        }
                        FrameStatus::FrameError { render_error } => {
                            FrameReply::FrameLocal(render_error)
                        }
                    },
                    Err(error) => FrameReply::SessionLost(format!("render_frame failed: {error}")),
                };
                // A host-protection invariant failure invalidates the whole
                // session; report it lost so the next frame reopens.
                let reply = if session.invalidation().is_some() {
                    FrameReply::SessionLost(match reply {
                        FrameReply::SessionLost(message) => message,
                        FrameReply::FrameLocal(code) => {
                            format!("session invalidated (render_error {code})")
                        }
                        FrameReply::Rendered(_) => "session invalidated".to_string(),
                    })
                } else {
                    reply
                };
                let lost = matches!(reply, FrameReply::SessionLost(_));
                let _ = req.reply.send(reply);
                if lost {
                    break;
                }
            }
            let _ = session.close();
        })
        .map_err(|error| format!("failed to spawn session thread: {error}"))?;

    match open_rx.recv() {
        Ok(Ok(())) => Ok(MfSession {
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

/// Renders one frame by round-tripping through a session's owning thread, with
/// no map lock held.
fn render_on(
    tx: &Sender<RenderReq>,
    current_time: i32,
    rgba: Vec<u8>,
    parameters: Option<Vec<InteractiveParameter>>,
) -> FrameReply {
    let (reply_tx, reply_rx) = channel();
    if tx
        .send(RenderReq { current_time, rgba, parameters, reply: reply_tx })
        .is_err()
    {
        return FrameReply::SessionLost("session thread is gone".to_string());
    }
    match reply_rx.recv() {
        Ok(reply) => reply,
        Err(_) => FrameReply::SessionLost("session thread dropped the reply".to_string()),
    }
}

type SessionMap = Mutex<HashMap<i64, MfSession>>;

/// Returns the sender + serial of a live session matching `identity`, refreshing
/// its `last_used`, or `None` to open one. A mismatched session (object resized/
/// retimed) is evicted; the eviction is dropped after the lock is released.
fn existing_sender(
    sessions: &SessionMap,
    effect_id: i64,
    identity: &GeomIdentity,
) -> Option<(Sender<RenderReq>, u64)> {
    let mut evicted: Option<MfSession> = None;
    let result;
    {
        let Ok(mut map) = sessions.lock() else {
            return None;
        };
        match map.get_mut(&effect_id) {
            Some(session) if &session.identity == identity => {
                session.last_used = Instant::now();
                result = session.sender().map(|tx| (tx, session.serial));
            }
            Some(_) => {
                evicted = map.remove(&effect_id);
                result = None;
            }
            None => result = None,
        }
    }
    drop(evicted);
    result
}

/// Opens a session off-lock, then installs it under a brief lock. If another
/// thread won the race, keeps that one and drops ours off-lock. Reaps idle
/// sessions opportunistically.
fn open_and_get_sender(
    ctx: &FilterCtx,
    effect_id: i64,
    identity: &GeomIdentity,
) -> Result<(Sender<RenderReq>, u64), String> {
    let opened = open_mf_session(MfSessionConfig {
        repository: ctx.repository.clone(),
        plugin: ctx.plugin.clone(),
        sha: ctx.sha.clone(),
        smart: ctx.smart,
        defaults: ctx.defaults.clone(),
        identity: identity.clone(),
    })?;
    let serial = opened.serial;
    let mut discard: Vec<MfSession> = Vec::new();
    let sender;
    {
        let mut map = ctx
            .sessions
            .lock()
            .map_err(|_| "session map poisoned".to_string())?;

        let now = Instant::now();
        let expired: Vec<i64> = map
            .iter()
            .filter(|(_, session)| now.duration_since(session.last_used) > SESSION_IDLE_TIMEOUT)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            if let Some(session) = map.remove(&id) {
                discard.push(session);
            }
        }

        match map
            .get(&effect_id)
            .filter(|session| &session.identity == identity)
            .and_then(|session| session.sender().map(|tx| (tx, session.serial)))
        {
            // Lost the race; keep the installed session, discard ours. No clone of
            // `opened`'s channel is taken here, so the off-lock drop/join cannot
            // wait on a stray sender that outlives it.
            Some(existing) => {
                discard.push(opened);
                sender = existing;
            }
            None => {
                let tx = opened
                    .sender()
                    .expect("a freshly opened session has a live sender");
                if let Some(old) = map.insert(effect_id, opened) {
                    discard.push(old);
                }
                sender = (tx, serial);
            }
        }
    }
    drop(discard);
    Ok(sender)
}

/// Removes and drops the session at `effect_id` matching `serial` (dropped
/// off-lock). Matching on serial avoids dropping a healthy session a concurrent
/// reopen installed at the same id.
fn remove_session(sessions: &SessionMap, effect_id: i64, serial: u64) {
    let removed = {
        let Ok(mut map) = sessions.lock() else {
            return;
        };
        if map
            .get(&effect_id)
            .is_some_and(|session| session.serial == serial)
        {
            map.remove(&effect_id)
        } else {
            None
        }
    };
    drop(removed);
}

/// Reads each item's current (keyframed) value into the matching parameter.
fn apply_readers(parameters: &mut [InteractiveParameter], readers: &[ItemReader]) {
    for reader in readers {
        match reader {
            ItemReader::Track { ptr, slot, integer } => {
                let value = unsafe { (**ptr).value };
                if let Some(p) = parameters.iter_mut().find(|p| p.slot == *slot) {
                    p.value = if *integer { value.round() } else { value };
                }
            }
            ItemReader::Checkbox { ptr, slot } => {
                let value = unsafe { (**ptr).value };
                if let Some(p) = parameters.iter_mut().find(|p| p.slot == *slot) {
                    p.value = if value { 1.0 } else { 0.0 };
                }
            }
            ItemReader::Select { ptr, slot } => {
                let value = unsafe { (**ptr).value };
                if let Some(p) = parameters.iter_mut().find(|p| p.slot == *slot) {
                    p.value = f64::from(value);
                }
            }
            ItemReader::Color { ptr, slot } => {
                let bgrx = unsafe { (**ptr).value.bgrx };
                if let Some(p) = parameters.iter_mut().find(|p| p.slot == *slot) {
                    // color is ARGB [a, r, g, b]; keep alpha, update rgb.
                    p.color[1] = bgrx[2];
                    p.color[2] = bgrx[1];
                    p.color[3] = bgrx[0];
                }
            }
        }
    }
}

fn pixels_to_bytes(pixels: &[PIXEL_RGBA]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() * 4);
    for p in pixels {
        out.extend_from_slice(&[p.r, p.g, p.b, p.a]);
    }
    out
}

fn bytes_to_pixels(bytes: &[u8]) -> Vec<PIXEL_RGBA> {
    bytes
        .chunks_exact(4)
        .map(|c| PIXEL_RGBA { r: c[0], g: c[1], b: c[2], a: c[3] })
        .collect()
}


#[cfg(test)]
mod tests {
    use super::*;

    const META: Option<((u64, u32), u64)> = Some(((5, 0), 64));
    /// The AEX could not be stat'd this pass (transient: AV scanner, replacement).
    const NO_META: Option<((u64, u32), u64)> = None;

    fn build(worker_mtime: u64) -> BuildFingerprint {
        BuildFingerprint {
            worker: Some((worker_mtime, 0, 4096)),
            host: Some((100, 0, 8192)),
        }
    }

    fn discovered(mtime_secs: u64, len: u64, build: BuildFingerprint) -> CacheEntry {
        CacheEntry {
            mtime: (mtime_secs, 0),
            len,
            ok: true,
            sha: "aa".into(),
            smart: true,
            params: Vec::new(),
            build,
        }
    }

    fn failed(mtime_secs: u64, len: u64, build: BuildFingerprint) -> CacheEntry {
        CacheEntry {
            ok: false,
            sha: String::new(),
            smart: false,
            ..discovered(mtime_secs, len, build)
        }
    }

    // --- keep_best: never lose a working effect to a transient failure -------

    /// The core of issue #307: a transient re-verification failure (a worker
    /// timeout under load) must not turn a working effect into a negative, or the
    /// next launch stops registering it and a saved project silently loses every
    /// object that used it.
    #[test]
    fn a_failed_reverification_does_not_demote_an_unchanged_effect() {
        let old = discovered(5, 64, build(1));
        let merged = keep_best(Some(&old), failed(5, 64, build(2)), META);
        assert!(merged.ok, "an unchanged, previously working AEX stayed ok");
        assert_eq!(merged.sha, "aa", "the old payload was kept");
        assert!(merged.smart);
    }

    /// A failure to stat the AEX is not evidence that it changed, so it must not
    /// open the demotion path either.
    #[test]
    fn an_unreadable_aex_is_not_treated_as_replaced() {
        let old = discovered(5, 64, build(1));
        let merged = keep_best(Some(&old), failed(0, 0, build(2)), NO_META);
        assert!(merged.ok, "a transient stat failure did not demote");
        assert_eq!(merged.mtime, (5, 0), "the old meta was kept, not zeroed");
        assert_eq!(merged.len, 64);
    }

    /// Keeping the old result must still take the new build, otherwise the entry
    /// is queued for re-verification again on every launch and never converges.
    #[test]
    fn a_kept_entry_still_takes_the_new_build() {
        let old = discovered(5, 64, build(1));
        let merged = keep_best(Some(&old), failed(5, 64, build(2)), META);
        assert_eq!(merged.build, build(2));
        assert_eq!(
            classify(Some(&merged), META, build(2)),
            LoadDecision { register: true, discover: false },
            "converged: registered, not queued again"
        );
    }

    /// A replaced AEX is a different plug-in, so its old parameters are
    /// meaningless and the negative result must win.
    #[test]
    fn a_replaced_aex_may_become_negative() {
        let old = discovered(5, 64, build(1));
        let newer = Some(((9, 0), 64));
        let resized = Some(((5, 0), 99));
        assert!(!keep_best(Some(&old), failed(9, 64, build(1)), newer).ok);
        assert!(!keep_best(Some(&old), failed(5, 99, build(1)), resized).ok);
    }

    /// The point of re-verifying at all (issue #304): a host that gained support
    /// for an effect promotes the old negative.
    #[test]
    fn a_new_host_promotes_a_previously_failing_effect() {
        let old = failed(5, 64, build(1));
        let merged = keep_best(Some(&old), discovered(5, 64, build(2)), META);
        assert!(merged.ok);
        assert_eq!(merged.build, build(2));
    }

    #[test]
    fn a_first_discovery_is_taken_as_is() {
        assert!(!keep_best(None, failed(5, 64, build(1)), META).ok);
        assert!(keep_best(None, discovered(5, 64, build(1)), META).ok);
    }

    // --- classify: an older host must not unregister a filter ---------------

    /// The other half of issue #307: an entry from an older host keeps being
    /// registered while it is re-verified, instead of vanishing for a launch.
    #[test]
    fn an_entry_from_an_older_host_is_registered_and_reverified() {
        let old = discovered(5, 64, build(1));
        assert_eq!(
            classify(Some(&old), META, build(2)),
            LoadDecision { register: true, discover: true }
        );
    }

    #[test]
    fn a_current_entry_is_registered_without_rediscovery() {
        let entry = discovered(5, 64, build(1));
        assert_eq!(
            classify(Some(&entry), META, build(1)),
            LoadDecision { register: true, discover: false }
        );
    }

    /// A cached non-effect (a format/codec `.aex`) is not registered, and is only
    /// re-probed when the host changed.
    #[test]
    fn a_cached_negative_is_not_registered() {
        let entry = failed(5, 64, build(1));
        assert_eq!(
            classify(Some(&entry), META, build(1)),
            LoadDecision { register: false, discover: false }
        );
        assert_eq!(
            classify(Some(&entry), META, build(2)),
            LoadDecision { register: false, discover: true }
        );
    }

    #[test]
    fn an_unknown_or_changed_aex_is_only_discovered() {
        let entry = discovered(5, 64, build(1));
        assert_eq!(
            classify(None, META, build(1)),
            LoadDecision { register: false, discover: true },
            "never seen"
        );
        assert_eq!(
            classify(Some(&entry), Some(((9, 0), 64)), build(1)),
            LoadDecision { register: false, discover: true },
            "the AEX itself changed"
        );
    }

    // --- prune: never conclude "gone" from an incomplete scan ---------------

    fn cache_of(keys: &[&str]) -> HashMap<String, CacheEntry> {
        keys.iter()
            .map(|key| ((*key).to_string(), discovered(5, 64, build(1))))
            .collect()
    }

    #[test]
    fn a_complete_scan_prunes_entries_whose_aex_is_gone() {
        let mut cache = cache_of(&["a.aex", "gone.aex"]);
        prune_cache(&mut cache, &[PathBuf::from("a.aex")], true);
        assert_eq!(cache.len(), 1);
        assert!(cache.contains_key("a.aex"));
    }

    /// A folder that could not be read (or a default folder that went missing)
    /// must not make its effects look deleted: pruning them would leave them
    /// unregistered next launch and delete objects from saved projects (#307).
    #[test]
    fn an_incomplete_scan_prunes_nothing() {
        let mut cache = cache_of(&["a.aex", "unscanned.aex"]);
        prune_cache(&mut cache, &[PathBuf::from("a.aex")], false);
        assert_eq!(cache.len(), 2, "the unscanned entry survived");
    }

    // --- cache file acceptance ----------------------------------------------

    /// Entries must survive being read back; only a schema-version change
    /// discards them (a host-build change is handled per entry).
    #[test]
    fn a_current_cache_file_keeps_its_entries() {
        let file = CacheFile {
            version: CACHE_VERSION,
            entries: cache_of(&["a.aex"]),
        };
        assert_eq!(accept_cache_file(file).len(), 1);
    }

    #[test]
    fn a_future_or_older_schema_is_discarded() {
        let file = CacheFile {
            version: CACHE_VERSION + 1,
            entries: cache_of(&["a.aex"]),
        };
        assert!(accept_cache_file(file).is_empty());
    }

    // --- scan completeness ---------------------------------------------------

    #[test]
    fn a_readable_folder_scans_completely() {
        let dir = std::env::temp_dir().join("aexcompat-multifilter-scan-test");
        let _ = std::fs::create_dir_all(&dir);
        let (_, complete) = collect_aex(&[dir], &[]);
        assert!(complete);
    }

    #[test]
    fn an_unreadable_folder_marks_the_scan_incomplete() {
        let missing = std::env::temp_dir().join("aexcompat-multifilter-does-not-exist");
        let (plugins, complete) = collect_aex(&[missing], &[]);
        assert!(plugins.is_empty());
        assert!(!complete, "a folder that could not be read is not a complete scan");
    }

    // --- default folder resolution ------------------------------------------

    /// A unique temp dir per test (no `Date::now`/rand available in-process here,
    /// so the test name provides uniqueness).
    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aexcompat-mf-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp root");
        dir
    }

    #[test]
    fn the_newest_versioned_install_wins() {
        let root = temp_root("newest");
        for version in ["2024", "2025"] {
            std::fs::create_dir_all(root.join(format!("App {version}")).join("Plug-ins")).unwrap();
        }
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert!(complete);
        assert_eq!(picked.unwrap(), root.join("App 2025").join("Plug-ins"));
    }

    /// An install being updated has its version folder but not yet its leaf.
    /// Falling back to the older version must not also claim the scan was
    /// complete, or the newer version's effects get pruned and unregistered (#307).
    #[test]
    fn a_version_missing_its_leaf_marks_the_resolution_incomplete() {
        let root = temp_root("updating");
        std::fs::create_dir_all(root.join("App 2024").join("Plug-ins")).unwrap();
        std::fs::create_dir_all(root.join("App 2025")).unwrap(); // mid-update
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert_eq!(picked.unwrap(), root.join("App 2024").join("Plug-ins"));
        assert!(!complete, "fell back to an older version, so not complete");
    }

    /// Unversioned clutter next to the installs is not evidence of anything.
    #[test]
    fn unversioned_entries_do_not_mark_the_resolution_incomplete() {
        let root = temp_root("clutter");
        std::fs::create_dir_all(root.join("App 2025").join("Plug-ins")).unwrap();
        std::fs::write(root.join("App readme.txt"), b"x").unwrap();
        std::fs::create_dir_all(root.join("App Common")).unwrap();
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert_eq!(picked.unwrap(), root.join("App 2025").join("Plug-ins"));
        assert!(complete);
    }

    #[test]
    fn a_missing_root_is_incomplete() {
        let root = std::env::temp_dir().join("aexcompat-mf-no-such-root");
        let _ = std::fs::remove_dir_all(&root);
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert!(picked.is_none());
        assert!(!complete);
    }
}
