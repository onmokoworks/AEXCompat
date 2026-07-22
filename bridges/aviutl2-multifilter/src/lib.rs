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
    InteractiveParameter, RenderGpuBackend, RenderPixelFormat,
    inspect_experimental_with_approved_dependencies_and_diagnostics,
};
use aexcompat_broker::plugin_dependency_closure::{
    DependencyClosureRequest, DependencyProvenance, ResolvedDependencyClosure,
    resolve_dependency_closure, survey_dependency_closure,
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
    /// Folders searched for an AEX's dependency DLLs (issue #304). The AEX's own
    /// folder is always searched first; these are the extra runtime folders an
    /// installed host would have provided (for an AE effect, the AE
    /// `Support Files\` folder). Empty means the default AE runtime folders.
    #[serde(default)]
    dependency_dirs: Vec<PathBuf>,
    /// Optional ceiling on how many DLLs may be sealed with one AEX. Absent means
    /// no ceiling: the closure is whatever the plug-in imports out of the folders
    /// above, and a plug-in is not skipped for needing a large runtime. Set it to
    /// trade coverage for a shorter discovery pass — an AEX over the ceiling then
    /// fails discovery instead of copying its closure.
    dependency_module_limit: Option<usize>,
    /// Optional ceiling on the total bytes sealed with one AEX, same trade-off as
    /// `dependency_module_limit`. The heaviest AE plug-ins pull about 1 GB.
    dependency_byte_limit: Option<u64>,
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

    // Recursively collect the *.aex to expose (minus ignored), deduped + sorted.
    // `scan_complete` is false when a default folder went missing or one could not
    // be read, which makes the background pass keep (rather than prune) the
    // entries it did not see this launch.
    let scan = collect_aex(&dirs, &config.ignore);
    let scan_complete = dirs_complete && scan.complete;

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
    let dependency = resolve_dependency_config(&config);
    let build = build_fingerprint(&repository, &dependency);
    let mut cache = load_cache();
    let mut plugins = scan.plugins;
    // A scan that cannot be trusted must not make a cached effect disappear
    // for this launch. Registration is intentionally conservative in the
    // other direction: a temporarily missing AEX may be shown and fail closed
    // at render time, but AviUtl2 will not discard objects from a saved project
    // merely because this launch could not see the file (#321).
    plugins.extend(cached_fallback_plugins(
        &cache,
        &scan.seen,
        &dirs,
        scan_complete,
        !dirs_complete,
        &config.ignore,
    ));
    plugins.sort();
    plugins.dedup();
    if plugins.is_empty() {
        return;
    }

    // Register (host callback, main thread only) each AEX whose discovery already
    // succeeded. A changed AEX keeps its last known-good registration for this
    // launch while the replacement is discovered in the background; an
    // unregistered filter would let AviUtl2 discard objects from saved projects.
    // Unknown entries and old-host entries also go to the background pass; its
    // updated result is picked up on the next launch.
    let mut pending: Vec<PathBuf> = Vec::new();
    let mut aliases: Option<HashMap<PathBuf, Vec<String>>> = None;
    let mut rekey: Vec<(String, String)> = Vec::new();
    // Whether any cached key under the scan roots is a spelling this scan did not
    // walk. If none is, no other spelling exists and the alias lookup — which
    // touches the filesystem, on the thread AviUtl2 is loading from — is skipped.
    let walked: std::collections::HashSet<String> = scan
        .seen
        .iter()
        .map(|plugin| plugin.to_string_lossy().into_owned())
        .collect();
    let alias_possible = alias_possible(&cache, &walked, &dirs);

    for plugin in &plugins {
        let key = plugin.to_string_lossy().into_owned();
        let meta = file_meta(plugin);
        let (cached, alias) = resolve_cached(
            &cache,
            &key,
            plugin,
            meta,
            build,
            &dirs,
            alias_possible,
            &mut aliases,
        );
        if let Some(alias) = alias {
            rekey.push((alias, key));
        }
        let mut decision = classify(cached, meta, build);
        // A closure that would now resolve differently (issue #304) joins the same
        // queue rather than unregistering the filter: the dependency DLLs decide
        // the result as much as the host build does, and #307's rule is that
        // nothing is unregistered for a launch.
        //
        // Bounded by the same [`RETRY_BUDGET`] a host change is. A re-discovery
        // that keeps failing does not update the recorded closure (`keep_best`
        // refuses to demote a working entry, and keeps its record with it), so the
        // trigger would otherwise fire on every launch forever — an AE update that
        // rewrites one runtime DLL puts every effect in that state at once.
        // Checked only when the entry would otherwise be left alone, so an
        // already-queued one pays nothing.
        if !decision.discover
            && let Some(entry) = cached
            && needs_closure_recheck(entry, build, &search_roots_for(plugin, &dependency.dirs))
        {
            decision.discover = true;
        }
        if decision.register
            && let Some(entry) = cached
        {
            register_discovered(host, &repository, plugin, &dependency, entry);
        }
        if decision.discover {
            pending.push(plugin.clone());
        }
    }

    let rekeyed = !rekey.is_empty();
    apply_rekey(&mut cache, rekey);

    if pending.is_empty() {
        // Nothing to discover, so the background pass (the only other writer)
        // will not run. Persist the re-key here or it is recomputed every launch.
        if rekeyed {
            save_cache(&cache);
        }
        return;
    }

    spawn_background_discovery(
        repository,
        dependency,
        scan.seen,
        dirs,
        cache,
        pending,
        build,
        scan_complete,
    );
}

/// Drops cache entries for AEX that are no longer present.
///
/// An entry may only be judged gone if this launch actually looked where it
/// lives, and looked completely. Two guards, because "not in this scan" is not
/// "deleted", and dropping a live entry leaves that effect unregistered on the
/// next launch, deleting objects from saved projects that use it (issue #307):
///
/// - `scan_complete` is false when a folder went missing or could not be read.
/// - `roots` bounds the prune to the folders scanned. The cache file is shared
///   across configurations, so pointing `AEXCOMPAT_MULTIFILTER_DIR` at one folder
///   for a launch would otherwise delete every entry from the default AE and
///   MediaCore folders, and the next unset launch would register none of them.
/// - `seen` is every AEX found, `ignore`d ones included, since those exist on
///   disk; judging from the registered subset would drop an ignored effect's
///   entry and leave it unregistered the launch after it is un-ignored.
/// - Anything still missing is confirmed against the filesystem before it goes,
///   so a path the scan reached under a different spelling (through a junction)
///   is not mistaken for a deleted one.
///
/// Keeping a stale entry costs only cache bytes. An incomplete scan also uses
/// the cache as a registration fallback, so an effect can remain visible while
/// its folder is temporarily unavailable.
fn prune_cache(
    cache: &mut HashMap<String, CacheEntry>,
    seen: &[PathBuf],
    roots: &[PathBuf],
    scan_complete: bool,
) {
    if !scan_complete {
        return;
    }
    // Compare presence on the same lossy string the cache is keyed by, so a path
    // that does not round-trip through UTF-8 still matches itself; `starts_with`
    // needs a Path, but only decides whether this launch looked there at all.
    let present: std::collections::HashSet<String> = seen
        .iter()
        .map(|plugin| plugin.to_string_lossy().into_owned())
        .collect();
    cache.retain(|key, _| {
        let path = Path::new(key);
        if present.contains(key) || !roots.iter().any(|root| path.starts_with(root)) {
            return true;
        }
        // Missing from this launch's listing is not the same as gone: a junction
        // can make one AEX reachable under several paths and the scan keeps only
        // the spelling it walked. Ask the filesystem instead, and keep the entry
        // unless it answers a definite "no" — an error is "could not tell". (A
        // path behind an unresolvable link answers `Ok(false)`, not an error; that
        // case is held off by `scan_complete`, which is false for such a link.)
        !matches!(path.try_exists(), Ok(false))
    });
}

/// Returns usable cached AEX paths that a non-authoritative scan did not see.
///
/// A complete scan is authoritative: resurrecting an entry absent from it
/// would keep filters for files that really disappeared. An incomplete scan is
/// the opposite: absence is not evidence of deletion, so keeping a last-known-
/// good registration is safer than letting AviUtl2 remove project objects
/// before the file becomes visible again (#321).
fn cached_fallback_plugins(
    cache: &HashMap<String, CacheEntry>,
    seen: &[PathBuf],
    roots: &[PathBuf],
    scan_complete: bool,
    roots_incomplete: bool,
    ignore: &[String],
) -> Vec<PathBuf> {
    if scan_complete {
        return Vec::new();
    }
    let seen_keys: HashSet<String> = seen
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    let seen_real: HashSet<PathBuf> = seen
        .iter()
        .filter_map(|path| path.canonicalize().ok())
        .collect();

    cache
        .iter()
        .filter_map(|(key, entry)| {
            if !entry.ok || seen_keys.contains(key) {
                return None;
            }
            let path = PathBuf::from(key);
            if !roots_incomplete
                && !roots.is_empty()
                && !roots.iter().any(|root| path.starts_with(root))
            {
                return None;
            }
            if is_ignored(&path, ignore)
                || path
                    .canonicalize()
                    .is_ok_and(|real| seen_real.contains(&real))
            {
                return None;
            }
            Some(path)
        })
        .collect()
}

/// Indexes the cache by each entry's real (link-resolved) path, so an AEX whose
/// walked spelling changed between launches is still found. Keys that no longer
/// resolve are skipped; they are handled by [`prune_cache`].
///
/// Restricted to `roots` as a trade-off, not because keys outside them cannot
/// match: resolving one could match too, but a leftover key on a disconnected
/// drive would stall startup. So a scan root whose own spelling changed between
/// launches (its path is now written a different way) is not resolved, and its
/// effects go unregistered for that launch — the case tracked as #321.
fn index_by_real_path(
    cache: &HashMap<String, CacheEntry>,
    roots: &[PathBuf],
    build: BuildFingerprint,
) -> HashMap<PathBuf, Vec<String>> {
    let mut index: HashMap<PathBuf, Vec<String>> = HashMap::new();
    for key in cache
        .keys()
        .filter(|key| roots.iter().any(|root| Path::new(key.as_str()).starts_with(root)))
    {
        let Ok(real) = Path::new(key).canonicalize() else {
            continue;
        };
        index.entry(real).or_default().push(key.clone());
    }
    // Best first, and every candidate kept: ranking cannot tell whether an entry
    // still describes the file on disk, so the caller has to be able to fall
    // through to the next spelling rather than be handed one unusable pick and
    // leave the effect unregistered (issue #307). The key breaks the remaining
    // tie, so the order never depends on hash iteration order — which would make
    // the effect's parameters, or whether it registers at all, differ between
    // launches.
    for keys in index.values_mut() {
        keys.sort_by(|left, right| {
            alias_rank(cache.get(right), build)
                .cmp(&alias_rank(cache.get(left), build))
                .then_with(|| left.cmp(right))
        });
    }
    index
}

/// Whether any cached key under `roots` is a spelling this scan did not walk
/// and is still worth resolving.
///
/// If none is, every cached entry in scope is already keyed by the path the scan
/// produced, so no other spelling exists to look for and the alias lookup — which
/// canonicalizes paths on the thread AviUtl2 is loading from — can be skipped.
fn alias_possible(
    cache: &HashMap<String, CacheEntry>,
    walked: &std::collections::HashSet<String>,
    roots: &[PathBuf],
) -> bool {
    cache.iter().any(|(key, entry)| {
        !walked.contains(key)
            && roots
                .iter()
                .any(|root| Path::new(key.as_str()).starts_with(root))
            && (!entry.alias_fallback
                || entry
                    .alias_target
                    .as_deref()
                    .is_none_or(|target| !cache.contains_key(target)))
    })
}

/// Picks the cache entry to use for one AEX, and the alias key it came from when
/// that was not the spelling this scan walked.
///
/// Entries are keyed by the path string the scan walked, and a junction added,
/// renamed, or reached from another root changes that spelling without changing
/// the file. The file is looked for under another spelling whenever what is held
/// under this one would not register, or the effect goes unregistered for the
/// launch — which deletes it out of saved projects that use it (issue #307). Not
/// only on an outright miss: only the walked spelling is refreshed by discovery,
/// so a copy left under another one can be the newer of the two. Also when this
/// spelling holds a `stale` entry, which registers but on a payload that may
/// describe older bytes, so a sound copy elsewhere is worth preferring.
#[allow(clippy::too_many_arguments)]
fn resolve_cached<'a>(
    cache: &'a HashMap<String, CacheEntry>,
    key: &str,
    plugin: &Path,
    meta: Option<((u64, u32), u64)>,
    build: BuildFingerprint,
    roots: &[PathBuf],
    alias_possible: bool,
    aliases: &mut Option<HashMap<PathBuf, Vec<String>>>,
) -> (Option<&'a CacheEntry>, Option<String>) {
    let direct = cache.get(key);
    let direct_registers = classify(direct, meta, build).register;
    let direct_matches = direct.is_some_and(|entry| {
        meta.is_some_and(|(mtime, len)| entry.mtime == mtime && entry.len == len)
    });
    // Look further when this spelling holds nothing usable, and also when what it
    // holds is only usable in the weaker sense of `alias_rank` — a stale entry
    // registers, but on a payload that may describe older bytes, so its sessions
    // fail to open and its frames pass through unrendered. Another spelling can
    // hold a sound entry for the same file.
    let direct_is_sound = direct_registers
        && direct_matches
        && direct.is_some_and(|entry| !entry.stale);
    if !alias_possible || direct_is_sound {
        return (direct, None);
    }
    // Built lazily, so a launch where every spelling matches never pays for it.
    let index = aliases.get_or_insert_with(|| index_by_real_path(cache, roots, build));
    let Some(candidates) = plugin.canonicalize().ok().and_then(|real| index.get(&real)) else {
        return (direct, None);
    };
    // Best-ranked first, but try each: the rank cannot tell whether an entry still
    // describes the file, so a better-ranked but outdated one must not shadow a
    // usable one and leave the effect unregistered.
    for alias in candidates {
        let candidate = cache.get(alias);
        let candidate_matches = candidate.is_some_and(|entry| {
            meta.is_some_and(|(mtime, len)| entry.mtime == mtime && entry.len == len)
        });
        // Take it if this spelling had nothing usable, or if the candidate is
        // strictly sounder — never a lateral move, which would just churn.
        let improves = !direct_registers
            || !direct_matches
            || alias_rank(candidate, build) > alias_rank(direct, build);
        // A changed direct hit is intentionally retained for this launch, but
        // an outdated alias must not shadow a spelling whose metadata matches
        // the file currently being loaded.
        if improves && candidate_matches && classify(candidate, meta, build).register {
            return (candidate, Some(alias.clone()));
        }
    }
    (direct, None)
}

/// Ranks one spelling of a file against another as the entry to reuse. Two
/// spellings can disagree because only the walked one is refreshed by discovery:
/// prefer the one that registers, then one whose parameters are not known to be
/// out of date, then the one the current host produced.
///
/// An unknown current build matches nothing rather than everything: it equals
/// `BuildFingerprint::default()`, which is also what an entry written before the
/// field existed carries, so comparing would rank a legacy entry above a freshly
/// discovered one. Same reasoning as `classify`'s `is_known` guard.
fn alias_rank(entry: Option<&CacheEntry>, build: BuildFingerprint) -> (bool, bool, bool) {
    match entry {
        Some(entry) => (
            entry.ok,
            !entry.stale,
            build.is_known() && entry.build == build,
        ),
        None => (false, false, false),
    }
}

/// Copies each aliased entry onto the spelling the scan actually walked.
///
/// The background pass keys by that spelling, so without this it would find no
/// cached entry and `keep_best`'s refusal to demote would never apply — a
/// transient discovery failure could then write a negative and unregister the
/// effect on the next launch (issue #307).
///
/// The alias is copied, not moved: the walked spelling may be the temporary one.
/// If the scan reached the AEX through a junction that is gone next launch, the
/// walked key no longer resolves, and having deleted the original would leave
/// nothing to find. Keeping both costs one entry until [`prune_cache`] sees a
/// spelling genuinely stop existing, which is the safe direction here.
fn apply_rekey(cache: &mut HashMap<String, CacheEntry>, rekey: Vec<(String, String)>) {
    for (alias, walked) in rekey {
        if let Some(entry) = cache.get(&alias).cloned() {
            let mut walked_entry = entry.clone();
            walked_entry.alias_fallback = false;
            walked_entry.alias_target = None;
            cache.insert(walked.clone(), walked_entry);
            if alias != walked {
                if let Some(alias_entry) = cache.get_mut(&alias) {
                    alias_entry.alias_fallback = true;
                    alias_entry.alias_target = Some(walked);
                }
            }
        }
    }
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
///
/// `meta` is `None` when the AEX could not be stat'd even though the scan just
/// found the path (a sharing violation, a deploy race between `read_dir` and
/// `metadata`). As in [`keep_best`], that is not evidence the file changed, so the
/// cached result keeps being registered — a failed stat must not be able to
/// unregister a filter for a launch. Re-discovery is queued either way, since
/// freshness could not be confirmed.
fn classify(
    cached: Option<&CacheEntry>,
    meta: Option<((u64, u32), u64)>,
    build: BuildFingerprint,
) -> LoadDecision {
    let Some(entry) = cached else {
        return LoadDecision {
            register: false,
            discover: true,
        };
    };
    match meta {
        // Confirmed unchanged: re-verify when the entry is marked stale, or when
        // the host build moved (but an unknown build is not a moved one — see
        // `BuildFingerprint::is_known`, or one failed stat costs two full passes)
        // and this host has not already spent its [`RETRY_BUDGET`] on it.
        Some((mtime, len)) if entry.mtime == mtime && entry.len == len => LoadDecision {
            register: entry.ok,
            discover: entry.stale
                || (build.is_known()
                    && entry.build != build
                    && (entry.checked != build || entry.attempts < RETRY_BUDGET)),
        },
        // Confirmed changed: keep a last-known-good effect registered for this
        // launch and discover the replacement in the background. The cached
        // sha/params may be incompatible with the new bytes, but RenderSession
        // then fails closed on the SHA instead of AviUtl2 dropping the object
        // before the replacement result is available (issue #309).
        Some(_) => LoadDecision {
            register: entry.ok,
            discover: true,
        },
        // Unknown: keep what we have and re-check in the background.
        None => LoadDecision {
            register: entry.ok,
            discover: true,
        },
    }
}

/// Discovers the pending AEX on a background thread and rewrites the cache, so
/// startup is never blocked. Newly-discovered effects appear on the next launch.
fn spawn_background_discovery(
    repository: PathBuf,
    dependency: DependencyConfig,
    // Every AEX the scan saw, ignored ones included: the prune judges existence
    // from this, not from the registered subset (issue #307).
    seen: Vec<PathBuf>,
    roots: Vec<PathBuf>,
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
            prune_cache(&mut cache, &seen, &roots, scan_complete);

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
                let results = discover_all(&repository, chunk, &dependency, build);
                let discovered = results.len();
                for (plugin, entry) in results {
                    let key = plugin.to_string_lossy().into_owned();
                    // `None` means there was nothing trustworthy to write; the
                    // existing entry keeps registering and is retried next launch.
                    if let Some(merged) = keep_best(cache.get(&key), entry, file_meta(&plugin)) {
                        cache.insert(key, merged);
                    }
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

/// How dependency closures are resolved for this launch (issue #304): the extra
/// folders to search — the env override wins, else the config, else the default
/// After Effects runtime folder — plus the operator's optional ceilings. An AEX's
/// own folder is not listed; it is always searched first, per plug-in.
fn resolve_dependency_config(config: &Config) -> DependencyConfig {
    let dirs = if let Some(dirs) = std::env::var_os(ENV_DEPENDENCY_DIRS) {
        dirs.to_string_lossy()
            .split(';')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(PathBuf::from)
            .collect()
    } else if !config.dependency_dirs.is_empty() {
        config.dependency_dirs.clone()
    } else {
        default_dependency_dirs()
    };
    DependencyConfig {
        dirs,
        module_limit: config.dependency_module_limit,
        byte_limit: config.dependency_byte_limit,
    }
}

/// Where an AEX's dependency DLLs are looked for, and the optional ceilings on
/// what may be sealed with it. Defaults to "the installed AE runtime folder, no
/// ceiling" (issue #304).
#[derive(Clone, Default)]
struct DependencyConfig {
    dirs: Vec<PathBuf>,
    module_limit: Option<usize>,
    byte_limit: Option<u64>,
}

/// The default dependency folders: the newest installed After Effects
/// `Support Files\`, which is where an AE effect's Adobe runtime DLLs
/// (`dvacore.dll` and friends) live, one level above the `Plug-ins\` tree that
/// is scanned for effects.
fn default_dependency_dirs() -> Vec<PathBuf> {
    latest_after_effects_plugins()
        .0
        .and_then(|plugins| plugins.parent().map(Path::to_path_buf))
        .filter(|support_files| support_files.is_dir())
        .into_iter()
        .collect()
}

/// The search roots for one AEX: its own folder first (an AEX that ships its
/// helper DLLs beside itself resolves them the way the installed host would),
/// then the configured runtime folders.
///
/// Each root is canonicalized here, because the resolver requires absolute roots
/// — a root whose meaning depends on the process working directory is exactly
/// what it should refuse — while the config may legitimately be written relative.
/// A folder that cannot be canonicalized (missing, or not a directory) is dropped
/// rather than failing the resolution: it can never provide a DLL, so keeping it
/// would only turn a stale config line into "nothing discovers at all". Too
/// *many* folders is not softened — the resolver rejects that, so a config over
/// the root limit fails loudly instead of silently ignoring the tail.
fn search_roots_for(plugin: &Path, dependency_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let canonical_dir = |dir: &Path| {
        std::fs::canonicalize(dir)
            .ok()
            .filter(|canonical| canonical.is_dir())
    };
    let mut roots: Vec<PathBuf> = plugin
        .parent()
        .and_then(canonical_dir)
        .into_iter()
        .collect();
    for dir in dependency_dirs {
        if let Some(dir) = canonical_dir(dir)
            && !roots.iter().any(|root| root == &dir)
        {
            roots.push(dir);
        }
    }
    roots
}

/// The dependency closure sealed with `plugin`, or an error string.
///
/// Failing here is not softened into "no dependencies": a plug-in whose closure
/// cannot be resolved would only fail again inside the worker as an opaque
/// `LoadLibraryExW` failure, so the reason is kept and surfaced by the caller.
fn dependency_closure_for(
    plugin: &Path,
    dependency: &DependencyConfig,
    roots: &[PathBuf],
) -> Result<ResolvedDependencyClosure, String> {
    resolve_dependency_closure(DependencyClosureRequest {
        max_dependencies: dependency.module_limit,
        max_total_bytes: dependency.byte_limit,
        ..DependencyClosureRequest::new(plugin, roots)
    })
    .map_err(|error| format!("dependency closure resolution failed: {error}"))
}

/// Whether a name is a Windows API set, which the loader resolves on its own.
fn is_api_set(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("api-ms-") || name.starts_with("ext-ms-")
}

/// `(path, mtime, len)` for each dependency file, plus the basenames of any that
/// vanished between the walk and here.
///
/// A file that is already gone cannot be compared against later, but its
/// disappearance is exactly the kind of change that should re-verify the entry —
/// so it is handed back as a name nothing provides. If it stays gone the entry
/// converges (no root offers it); if it comes back, the missing-name check fires.
fn cached_dependencies(paths: &[PathBuf]) -> (Vec<CachedDependency>, Vec<String>) {
    let mut dependencies = Vec::with_capacity(paths.len());
    let mut vanished = Vec::new();
    for path in paths {
        match file_meta(path) {
            Some((mtime, len)) => dependencies.push(CachedDependency {
                path: path.to_string_lossy().into_owned(),
                mtime,
                len,
            }),
            None => vanished.extend(
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_lowercase),
            ),
        }
    }
    (dependencies, vanished)
}

/// The imported names worth re-checking at startup: everything no search root
/// provided, minus the Windows API sets the loader owns.
fn cached_missing(unresolved: &[String]) -> Vec<String> {
    unresolved
        .iter()
        .filter(|name| !is_api_set(name))
        .cloned()
        .collect()
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
/// the folder could not be enumerated, an entry could not be read, or a version
/// *newer than the pick* was present without its `leaf`. That last case is what
/// an install being updated looks like, and silently falling back to an older
/// version while reporting a complete scan would make the newer version's
/// plug-ins look deleted — which prunes their cache entries and unregisters them
/// on the next launch, deleting objects from saved projects (issue #307). A
/// leafless *older* version is just an uninstall leftover and means nothing.
fn newest_versioned(root: &Path, prefix: &str, leaf: &[&str]) -> (Option<PathBuf>, bool) {
    let Ok(read) = std::fs::read_dir(root) else {
        return (None, false);
    };
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    let mut leafless: Vec<Vec<u64>> = Vec::new();
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
        // A numbered name is what an install in progress looks like. Unnumbered
        // ones (`... (Beta)`) are still picked when nothing numbered exists, but a
        // missing leaf under them is not evidence of an incomplete install.
        let numbered = version.split(['.', ' ']).any(|part| part.parse::<u64>().is_ok());
        let key = version_key(version);
        let mut candidate = entry.path();
        // Tested through the path, not `DirEntry::file_type`, which reports a
        // directory junction as a symlink rather than a directory — Adobe installs
        // are routinely junctioned to another drive.
        if !candidate.is_dir() {
            // A plain file is clutter. A reparse point that will not resolve is an
            // install we simply could not see this launch, which must not read as
            // "its plug-ins are gone".
            let unresolved = candidate
                .symlink_metadata()
                .is_ok_and(|meta| meta.file_type().is_symlink());
            if numbered && unresolved {
                leafless.push(key);
            }
            continue;
        }
        candidate.extend(leaf);
        if !candidate.is_dir() {
            if numbered {
                leafless.push(key);
            }
            continue;
        }
        if best.as_ref().is_none_or(|(best_key, _)| key > *best_key) {
            best = Some((key, candidate));
        }
    }
    // A leafless version above the pick means the newest install is not fully
    // visible this launch, so its absence is not evidence its plug-ins are gone.
    let best_key = best.as_ref().map(|(key, _)| key);
    complete &= !leafless
        .iter()
        .any(|key| best_key.is_none_or(|best_key| key > best_key));
    (best.map(|(_, path)| path), complete)
}

/// What one launch's folder scan saw.
struct Scan {
    /// The AEX to expose as filters (ignored ones removed).
    plugins: Vec<PathBuf>,
    /// Every `*.aex` seen, ignored ones included. This, not `plugins`, is what
    /// the prune may judge existence from: an ignored AEX is present on disk, and
    /// dropping its cache entry would leave it unregistered on the launch after
    /// it is taken back out of `ignore` (issue #307).
    seen: Vec<PathBuf>,
    /// False if any folder could not be fully enumerated, in which case nothing
    /// may be concluded to be gone at all.
    complete: bool,
}

/// Recursively scans `dirs`. An incomplete scan (a folder that could not be read,
/// a tree deeper than [`MAX_SCAN_DEPTH`]) must not be used to conclude an AEX is
/// gone: pruning its cache entry would leave the effect unregistered on the next
/// launch, which deletes objects from saved projects that use it (issue #307).
fn collect_aex(dirs: &[PathBuf], ignore: &[String]) -> Scan {
    let mut seen = Vec::new();
    let mut complete = true;
    // Shared across roots: junctions can make one folder reachable from several
    // of them, and descending twice would expose the same AEX as several filters.
    let mut visited = std::collections::HashSet::new();
    for dir in dirs {
        complete &= collect_aex_into(dir, 0, &mut seen, &mut visited);
    }
    seen.sort();
    seen.dedup();
    let plugins = seen
        .iter()
        .filter(|path| !is_ignored(path, ignore))
        .cloned()
        .collect();
    Scan {
        plugins,
        seen,
        complete,
    }
}

/// Returns false if any part of this subtree could not be enumerated.
fn collect_aex_into(
    dir: &Path,
    depth: usize,
    out: &mut Vec<PathBuf>,
    visited: &mut std::collections::HashSet<PathBuf>,
) -> bool {
    if depth > MAX_SCAN_DEPTH {
        return false;
    }
    // A junction can point back up the tree or make one folder reachable twice.
    // Visiting the real folder once keeps an AEX from being registered as several
    // filters (each with its own discovery worker). Already visited means "seen",
    // not "not looked at", so it does not make the scan incomplete.
    if let Ok(real) = dir.canonicalize()
        && !visited.insert(real)
    {
        return true;
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
        // A directory junction reports as a symlink, not a directory, so testing
        // only `is_dir()` would silently skip a junctioned subfolder while still
        // calling the scan complete — and the prune would then delete the cache
        // entries of every AEX under it, unregistering them (issue #307).
        // `MAX_SCAN_DEPTH` bounds any link cycle.
        let resolved = file_type.is_symlink().then(|| std::fs::metadata(&path));
        if matches!(resolved, Some(Err(_))) {
            // A link whose target cannot be resolved (its drive is not mounted
            // this launch) says nothing about what is behind it. Treating that as
            // "no AEX here" would prune everything under it.
            complete = false;
            continue;
        }
        if file_type.is_dir() || matches!(&resolved, Some(Ok(meta)) if meta.is_dir()) {
            complete &= collect_aex_into(&path, depth + 1, out, visited);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("aex"))
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
    /// Where this AEX's dependency DLLs are looked for when its session opens,
    /// and any ceilings on them; the same configuration discovery used (#304).
    dependency: DependencyConfig,
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
    /// Set when the AEX changed while it was being discovered, so `sha`/`params`
    /// may describe the previous bytes. The entry is still registered (better
    /// than unregistering it — issue #307) but is always re-discovered, so it
    /// cannot stay permanently wrong.
    #[serde(default)]
    stale: bool,
    /// The host that last *attempted* to re-verify this entry, which is not the
    /// one that produced it when the attempt failed. Kept apart from `build` so a
    /// failed attempt cannot pass the payload off as the current host's work:
    /// doing that both hides its real provenance and silently ends re-verification
    /// for that host, leaving an effect on an older host's parameters.
    #[serde(default)]
    checked: BuildFingerprint,
    /// Failed re-verification attempts under `checked`, so retries are bounded
    /// (see [`RETRY_BUDGET`]) instead of running on every launch forever.
    #[serde(default)]
    attempts: u8,
    /// What this entry's dependency resolution saw (issue #304), so the entry can
    /// be re-verified when that changes. Added additively: an entry written
    /// before this field simply has no roots recorded, which reads as "resolved
    /// differently" and queues it for the background pass — it is never dropped,
    /// and it stays registered meanwhile (issue #307).
    #[serde(default)]
    closure: CachedClosure,
    /// Classification of the most recent failed discovery attempt. Timeout
    /// classes are deliberately retained so transient runner pressure cannot
    /// demote a valid stale entry; deterministic worker failures may converge
    /// it to a negative cache entry (#328).
    #[serde(default)]
    failure_classification: Option<String>,
    /// This spelling is retained only as a fallback after an alias re-key. It
    /// must not keep the alias lookup hot while its walked spelling is present.
    #[serde(default)]
    alias_fallback: bool,
    /// The walked spelling copied from this fallback, if it is still cached.
    #[serde(default)]
    alias_target: Option<String>,
}

/// The resolution behind one cache entry: where it looked, what it sealed, and
/// what it could not find.
///
/// A discovery result depends on all three. Re-checking them costs a handful of
/// `stat` calls per entry, which is what lets the plug-in scan stay cheap at
/// startup while still re-verifying an entry whose closure would now differ.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct CachedClosure {
    /// Search roots in resolution order (first match wins, like the loader).
    #[serde(default)]
    roots: Vec<String>,
    /// The dependency files the resolution reached, as `(path, mtime, len)`:
    /// what was sealed when it succeeded, what it would have sealed when it
    /// failed. Recording them either way is what lets a failure caused by the
    /// dependencies themselves — an operator's ceiling exceeded, say — be redone
    /// once those files change.
    #[serde(default)]
    sealed: Vec<CachedDependency>,
    /// Imported names no search root provided, whether the loader then found them
    /// in System32 or not at all. Both matter the same way: if a root starts
    /// providing one, the closure changes.
    ///
    /// Windows API sets (`api-ms-*`, `ext-ms-*`) are left out. The loader owns
    /// those names and a plug-in folder cannot take them over, so tracking them
    /// would only cost startup `stat` calls.
    #[serde(default)]
    missing: Vec<String>,
    /// Resolver provenance for every sealed basename. This is diagnostic-only:
    /// worker module audit still classifies each authenticated file through the
    /// existing plug-in-tree policy. Additive/defaulted so pre-#360 caches remain
    /// readable and are naturally refreshed by the rebuilt host fingerprint.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    provenance: Vec<CachedDependencyProvenance>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct CachedDependencyProvenance {
    basename: String,
    import_derived: bool,
    string_derived: bool,
}

/// One sealed dependency's identity, as cheap to re-check as a `stat`.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct CachedDependency {
    path: String,
    mtime: (u64, u32),
    len: u64,
}

fn cached_provenance(sources: &[DependencyProvenance]) -> Vec<CachedDependencyProvenance> {
    sources
        .iter()
        .map(|source| CachedDependencyProvenance {
            basename: source.basename.clone(),
            import_derived: source.import_derived,
            string_derived: source.string_derived,
        })
        .collect()
}

/// Whether this entry should be re-discovered because its dependency closure
/// would now resolve differently (issue #304).
///
/// Bounded by the same [`RETRY_BUDGET`] a host change is: a re-discovery that
/// keeps failing does not update the recorded closure (`keep_best` refuses to
/// demote a working entry and keeps its record with it), so without the budget an
/// AE update that rewrites one runtime DLL would re-run a worker for every effect
/// on every launch, forever.
fn needs_closure_recheck(entry: &CacheEntry, build: BuildFingerprint, roots: &[PathBuf]) -> bool {
    (entry.checked != build || entry.attempts < RETRY_BUDGET)
        && !closure_still_resolves_the_same(entry, roots)
}

/// Whether re-resolving this entry's closure today would still reach the same
/// files, judged with `stat` only. `roots` is what the resolution would search
/// now, in order.
///
/// Four things can change the answer without touching the AEX itself, and each is
/// checked here:
///
/// 1. the search roots themselves differ from the ones the entry was resolved
///    against — a configured folder that moved, or a relative one now resolving
///    elsewhere because the host started from a different working directory,
/// 2. a sealed dependency was rewritten or removed (an AE update rewriting
///    `dvacore.dll` in place, a helper `*.aex` replaced),
/// 3. a file appeared in an earlier search root and now wins a name that used to
///    resolve further down the order,
/// 4. a search root now provides a name that no root provided at discovery time —
///    which turns a cached failure into a plug-in that would load, and equally
///    turns a System32 fallback into an app-local DLL that would be sealed.
///
/// A false "changed" only re-verifies the plug-in in the background; the entry
/// stays registered either way (issue #307).
fn closure_still_resolves_the_same(entry: &CacheEntry, roots: &[PathBuf]) -> bool {
    if entry.closure.roots.len() != roots.len()
        || !entry
            .closure
            .roots
            .iter()
            .zip(roots)
            .all(|(recorded, current)| Path::new(recorded) == current.as_path())
    {
        return false;
    }
    for dependency in &entry.closure.sealed {
        let path = Path::new(&dependency.path);
        if !file_meta(path)
            .is_some_and(|(mtime, len)| mtime == dependency.mtime && len == dependency.len)
        {
            return false;
        }
        let Some(name) = path.file_name() else {
            return false;
        };
        for root in roots {
            if path.parent() == Some(root.as_path()) {
                break;
            }
            if root.join(name).is_file() {
                return false;
            }
        }
    }
    !entry
        .closure
        .missing
        .iter()
        .any(|name| roots.iter().any(|root| root.join(name).is_file()))
}

/// How many times a re-verification may fail for one host build before the entry
/// is left alone until the host changes again.
///
/// Above one so a single transient failure — a worker timeout under load — does
/// not strand an entry on an older host's parameters. Small, because a host that
/// genuinely cannot discover an effect any more would otherwise re-run a worker
/// for it on every launch, and a regression can put hundreds of entries in that
/// state at once.
const RETRY_BUDGET: u8 = 3;

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
/// A save is short, but another AviUtl2 process may be between its read and
/// atomic replace.  Serialize the read/merge/write critical section with a
/// Windows handle lock so every writer observes the previous writer's result.
const CACHE_LOCK_RETRIES: usize = 200;
const CACHE_LOCK_RETRY: Duration = Duration::from_millis(10);

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
    /// Digest of the closure ceilings (issue #304). They decide whether a closure
    /// is sealed at all, so an entry produced under different ones is re-verified
    /// like one produced by an older host. The search folders and what they
    /// contain are tracked per entry instead, in [`CachedClosure`].
    #[serde(default)]
    dependency_inputs: u64,
}

impl BuildFingerprint {
    /// Whether both halves were actually stat'd. An unknown fingerprint must not
    /// count as "a different host", or one transient stat failure re-discovers
    /// every AEX twice: once under the unknown build, once when it resolves again.
    fn is_known(&self) -> bool {
        self.worker.is_some() && self.host.is_some()
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct CacheFile {
    version: u32,
    /// Held as raw JSON, and converted per entry on load, so one entry that no
    /// longer deserializes drops only itself instead of emptying the cache and
    /// unregistering every filter for a launch (issue #307).
    ///
    /// This contains *isolated* damage only. [`CacheEntry::params`] embeds
    /// `InteractiveParameter` from the broker crate, whose fields are not all
    /// `#[serde(default)]`, so a field added there fails every entry that has
    /// parameters — i.e. every registerable filter — at once. That shared
    /// dependency is pinned by `the_cached_parameter_schema_is_stable`, which
    /// fails in `cargo test` rather than letting the change reach users' caches.
    entries: HashMap<String, serde_json::Value>,
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
fn build_fingerprint(repository: &Path, dependency: &DependencyConfig) -> BuildFingerprint {
    let worker = repository
        .join("target")
        .join("minihost-build")
        .join("aex_l2_worker.exe");
    let flatten = |m: ((u64, u32), u64)| (m.0.0, m.0.1, m.1);
    BuildFingerprint {
        worker: file_meta(&worker).map(flatten),
        host: self_module_path().as_deref().and_then(file_meta).map(flatten),
        dependency_inputs: dependency_inputs_fingerprint(dependency),
    }
}

/// A digest of the ceilings, which decide whether a closure is sealed at all and
/// are not recorded per entry. Only equality matters, so the leading 8 bytes of
/// the SHA-256 are enough and keep the fingerprint `Copy`.
///
/// The search folders are deliberately **not** hashed here, even though they
/// decide the outcome too. Each entry records the canonical roots it actually
/// resolved against and is compared against today's, which is both exact (a
/// relative config string can mean different folders on different launches) and
/// per-entry. Hashing the folders instead would make one global value out of a
/// resolution that is not global — and `default_dependency_dirs` can legitimately
/// return the previous AE version's folder while an update is in flight, which
/// would then queue every entry for re-verification against the wrong runtime.
fn dependency_inputs_fingerprint(dependency: &DependencyConfig) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(b"limits\0");
    hasher.update(dependency.module_limit.unwrap_or(usize::MAX).to_le_bytes());
    hasher.update(dependency.byte_limit.unwrap_or(u64::MAX).to_le_bytes());
    let digest = hasher.finalize();
    u64::from_le_bytes(digest[..8].try_into().unwrap_or_default())
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
    load_cache_at(&path)
}

fn load_cache_at(path: &Path) -> HashMap<String, CacheEntry> {
    let Ok(text) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    accept_cache_file(serde_json::from_str(&text).unwrap_or_default())
}

/// Only a schema-version mismatch discards entries: the older shape cannot be
/// trusted field-for-field. A host-build change does NOT discard them — each entry
/// carries its own build ([`CacheEntry::build`]) and is re-verified in the
/// background while still being registered, so no filter disappears for a launch
/// (issue #307). An entry that no longer deserializes drops only itself, for the
/// same reason.
fn accept_cache_file(file: CacheFile) -> HashMap<String, CacheEntry> {
    if file.version != CACHE_VERSION {
        return HashMap::new();
    }
    file.entries
        .into_iter()
        .filter_map(|(key, value)| Some((key, serde_json::from_value(value).ok()?)))
        .collect()
}

fn save_cache(entries: &HashMap<String, CacheEntry>) {
    let Some(path) = cache_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // The lock is held across the disk read and the atomic replacement.  A
    // lock around only the final rename would still allow two launches to read
    // the same old cache and lose one another's newly discovered entries.
    let Some(_lock) = acquire_cache_lock(&path) else {
        return;
    };
    let mut merged_entries = load_cache_at(&path);
    merge_cache_entries(&mut merged_entries, entries);
    let file = CacheFile {
        version: CACHE_VERSION,
        // An entry that cannot be serialized is dropped rather than failing the
        // whole write, so the rest of the cache still survives the launch.
        entries: merged_entries
            .iter()
            .filter_map(|(key, entry)| Some((key.clone(), serde_json::to_value(entry).ok()?)))
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
/// A negative only wins when the AEX's current `(mtime, len)` *proves* it changed.
///
/// Returns `None` to mean "leave the cache alone". `meta` is the file's current
/// `(mtime, len)`, or `None` when it could not be stat'd — a transient condition
/// (an AV scanner's sharing violation, a plug-in being replaced). Discovery's own
/// stat can fail the same way, and [`negative_entry`] then falls back to
/// `(0, 0), 0`; writing that out would store an entry under a `(mtime, len)` that
/// never matches the file again, so every later launch would treat the effect as
/// replaced and stop registering it. With no trustworthy meta there is nothing
/// safe to write, so the existing entry (which still registers) is kept and the
/// AEX is re-checked on the next launch.
fn keep_best(
    cached: Option<&CacheEntry>,
    discovered: CacheEntry,
    meta: Option<((u64, u32), u64)>,
) -> Option<CacheEntry> {
    let (mtime, len) = meta?;
    // The AEX differs from what discovery stat'd, so `sha`/`params` may describe
    // the previous bytes. Take the meta just read (so the entry keeps matching the
    // file and stays registered) but mark it for one more pass.
    let stale = discovered.mtime != mtime || discovered.len != len;
    let discovered = CacheEntry {
        mtime,
        len,
        stale,
        ..discovered
    };
    match cached {
        Some(old)
            if old.ok
                && old.stale
                && !discovered.ok
                && old.mtime == mtime
                && old.len == len
                && deterministic_failure(discovered.failure_classification.as_deref()) =>
        {
            // The current bytes were rechecked and failed deterministically.
            // Unlike a timeout, this is safe to converge: keeping the stale
            // payload would register an effect whose SHA no longer opens.
            Some(CacheEntry {
                stale: false,
                ..discovered
            })
        }
        Some(old) if old.ok && !discovered.ok && old.mtime == mtime && old.len == len => {
            Some(CacheEntry {
                // Provenance stays with the host that actually produced the
                // payload; only the attempt is recorded, and it is counted so
                // retries are bounded rather than endless.
                checked: discovered.build,
                attempts: if old.checked == discovered.build {
                    old.attempts.saturating_add(1)
                } else {
                    1
                },
                // Keep any existing stale mark. Clearing it because *this* pass
                // failed would strand an entry whose sha/params describe older
                // bytes: it would never be re-discovered again, so every frame
                // would fail the sha check with no way back except deleting the
                // cache — the very operation that risks issue #307.
                //
                // A stale entry carries the meta just read from disk, so this
                // guard also holds when what is there now genuinely does not
                // discover. Such an entry does not converge on its own while the
                // bytes stay identical: it stays registered on the older bytes'
                // sha, whose session then fails to open, so its frames pass
                // through unrendered — and it is re-discovered every launch.
                // Excluding stale entries here would converge, but at the cost of
                // unregistering one whose re-check merely timed out, trading a
                // fault that leaves the objects in place for the irreversible
                // deletion this path exists to avoid. It also gives up the
                // self-healing: today one later success is enough. Converging
                // safely needs the failure's classification, which is #328.
                stale: old.stale,
                ..old.clone()
            })
        }
        _ => Some(discovered),
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
        stale: false,
        checked: build,
        attempts: 0,
        closure: CachedClosure::default(),
        failure_classification: None,
        alias_fallback: false,
        alias_target: None,
    }
}

/// Acquires an OS-level exclusive handle on the cache lock file.  The file is
/// intentionally retained after release: unlike a create-new sentinel, a
/// handle lock is released by Windows when the process exits, so a crash cannot
/// strand future saves behind a stale marker.
fn acquire_cache_lock(path: &Path) -> Option<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;

    let lock_path = path.with_file_name("discovery-cache.lock");
    for _ in 0..CACHE_LOCK_RETRIES {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .share_mode(0)
            .open(&lock_path)
        {
            Ok(lock) => return Some(lock),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                std::thread::sleep(CACHE_LOCK_RETRY);
            }
            Err(_) => return None,
        }
    }
    None
}

/// Unions another launch's cache into this launch's snapshot before replacing
/// the file.  Disjoint AEX results must survive regardless of which launch
/// saves last.  For the same path and file metadata, a known-good entry wins
/// over a negative one so a transient failure cannot erase a usable filter;
/// otherwise the local snapshot remains authoritative for that key.
fn merge_cache_entries(
    local: &mut HashMap<String, CacheEntry>,
    on_disk: &HashMap<String, CacheEntry>,
) {
    for (key, disk_entry) in on_disk {
        match local.get(key) {
            None => {
                local.insert(key.clone(), disk_entry.clone());
            }
            Some(local_entry)
                if disk_entry.ok
                    && !local_entry.ok
                    && disk_entry.mtime == local_entry.mtime
                    && disk_entry.len == local_entry.len =>
            {
                local.insert(key.clone(), disk_entry.clone());
            }
            Some(_) => {}
        }
    }
}

/// Extract the broker's already-normalized worker classification from the
/// diagnostic JSON embedded in an inspection error. Missing or malformed
/// diagnostics stay unknown and therefore retain the old safe behavior.
fn inspection_failure_classification(error: &std::io::Error) -> Option<String> {
    let message = error.to_string();
    let payload = message.split_once("diagnostics=")?.1;
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()?
        .get("classification")?
        .as_str()
        .map(str::to_owned)
}

fn deterministic_failure(classification: Option<&str>) -> bool {
    matches!(classification, Some("nonzero_exit" | "crashed"))
}

/// Discovers one AEX, always returning a cache entry (cache-all): `ok = true` for
/// a discoverable effect, `ok = false` for any failure (a genuine non-effect, or
/// an AEX the compat host cannot load, or a timeout). Discovery runs on the
/// background thread, so caching every outcome — even a timeout — means it is not
/// re-probed on later launches; a spurious negative is cleared by re-touching the
/// AEX or deleting the cache file (documented in the README).
fn discover_one(
    repository: &Path,
    plugin: &Path,
    dependency: &DependencyConfig,
    build: BuildFingerprint,
) -> CacheEntry {
    let mut entry = negative_entry(plugin, build);
    let Ok(bytes) = std::fs::read(plugin) else {
        return entry;
    };
    entry.sha = hex_lower(&Sha256::digest(&bytes));
    // Seal the plug-in's dependency DLLs with it, so an effect whose imports live
    // in its installed runtime folder can load inside the isolated sealed root at
    // all (issue #304). A closure that cannot be resolved is a failed discovery,
    // not a dependency-free retry.
    let roots = search_roots_for(plugin, &dependency.dirs);
    let recorded_roots: Vec<String> = roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect();
    let Ok(closure) = dependency_closure_for(plugin, dependency, &roots) else {
        // A resolution that failed outright — over an operator's ceiling, an
        // unreadable image — still records what it looked at, so the negative both
        // converges (no re-walk every launch) and is redone once the reason it
        // failed could have gone away. Surveying costs a walk without the hashing
        // or copying, which is what the failure saved in the first place.
        entry.closure = match survey_dependency_closure(plugin, &roots) {
            Ok(survey) => {
                let (sealed, vanished) = cached_dependencies(&survey.modules);
                let mut missing = cached_missing(&survey.unresolved);
                missing.extend(vanished);
                missing.sort();
                missing.dedup();
                CachedClosure {
                    roots: recorded_roots,
                    sealed,
                    missing,
                    provenance: cached_provenance(&survey.provenance),
                }
            }
            Err(_) => CachedClosure {
                roots: recorded_roots,
                ..CachedClosure::default()
            },
        };
        return entry;
    };
    let sealed_paths: Vec<PathBuf> = closure
        .dependencies()
        .iter()
        .map(|sealed| sealed.path.clone())
        .collect();
    let (sealed, vanished) = cached_dependencies(&sealed_paths);
    let provenance = cached_provenance(closure.provenance());
    let mut missing = cached_missing(closure.unresolved());
    missing.extend(vanished);
    missing.sort();
    missing.dedup();
    entry.closure = CachedClosure {
        roots: recorded_roots,
        sealed,
        missing,
        provenance,
    };
    match inspect_experimental_with_approved_dependencies_and_diagnostics(
        repository,
        plugin,
        &entry.sha,
        closure.into_dependencies(),
    ) {
        Ok((params, diagnostics)) => {
            // PF_OutFlag2_SUPPORTS_SMART_RENDER = bit 10.
            entry.smart = diagnostics
                .get("advertised_out_flags2")
                .and_then(|value| value.as_u64())
                .unwrap_or(0)
                & (1 << 10)
                != 0;
            entry.params = params;
            normalize_parameters_for_cache(&mut entry.params);
            entry.ok = true;
        }
        Err(error) => {
            entry.failure_classification = inspection_failure_classification(&error);
        }
    }
    entry
}

/// Keep broker parameters JSON-round-trippable before they enter the persistent
/// discovery cache (#322). `serde_json` writes non-finite `f64` values as `null`,
/// which the typed `InteractiveParameter` loader rejects on the next launch and
/// sends the same effect through discovery again forever.
fn normalize_parameters_for_cache(parameters: &mut [InteractiveParameter]) {
    for parameter in parameters {
        let minimum = parameter.minimum.is_finite().then_some(parameter.minimum);
        let maximum = parameter.maximum.is_finite().then_some(parameter.maximum);
        if let (Some(minimum), Some(maximum)) = (minimum, maximum)
            && minimum < maximum
        {
            parameter.minimum = minimum;
            parameter.maximum = maximum;
        } else {
            parameter.minimum = 0.0;
            parameter.maximum = 1.0;
        }
        if !parameter.value.is_finite() {
            parameter.value = parameter.minimum;
        }
        for component in &mut parameter.components {
            if !component.is_finite() {
                *component = 0.0;
            }
        }
    }
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
    dependency: &DependencyConfig,
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
                        discover_one(repository, plugin, dependency, build)
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
    dependency: &DependencyConfig,
    entry: &CacheEntry,
) {
    // Build config items + readers + normalized defaults from the exposed params.
    let mut items: Vec<*const c_void> = Vec::new();
    let mut readers: Vec<ItemReader> = Vec::new();
    let mut defaults: Vec<InteractiveParameter> = Vec::new();
    let item_names = unique_item_names(&entry.params);
    for (parameter, item_name) in entry.params.iter().zip(item_names) {
        let Some(item_name) = item_name else {
            continue;
        };
        if let Some((item_ptr, reader, sent)) = build_item(parameter, &item_name) {
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
        dependency: dependency.clone(),
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
fn build_item(
    parameter: &InteractiveParameter,
    item_name: &str,
) -> Option<(*const c_void, ItemReader, InteractiveParameter)> {
    match parameter.kind.as_str() {
        "float" => {
            let (min, max) = bounded_range(parameter)?;
            let ptr = leak_track(item_name, parameter.value, min, max, track_step(max - min));
            Some((ptr as *const c_void, ItemReader::Track { ptr, slot: parameter.slot, integer: false }, parameter.clone()))
        }
        "integer" => {
            if !parameter.choices.is_empty() {
                // Popup -> dropdown (AE popups are 1-based).
                let count = parameter.choices.len() as i32;
                let ptr = leak_select(item_name, (parameter.value as i32).clamp(1, count), &parameter.choices);
                let mut sent = parameter.clone();
                sent.minimum = 1.0;
                sent.maximum = count as f64;
                sent.value = sent.value.clamp(1.0, count as f64);
                return Some((ptr as *const c_void, ItemReader::Select { ptr, slot: parameter.slot }, sent));
            }
            let (min, max) = bounded_range(parameter)?;
            if min == 0.0 && max == 1.0 {
                let ptr = leak_checkbox(item_name, parameter.value != 0.0);
                Some((ptr as *const c_void, ItemReader::Checkbox { ptr, slot: parameter.slot }, parameter.clone()))
            } else {
                let ptr = leak_track(item_name, parameter.value.round(), min, max, 1.0);
                Some((ptr as *const c_void, ItemReader::Track { ptr, slot: parameter.slot, integer: true }, parameter.clone()))
            }
        }
        "color" => {
            // InteractiveParameter.color is ARGB; AviUtl2 color code is 0x00RRGGBB.
            let (r, g, b) = (parameter.color[1], parameter.color[2], parameter.color[3]);
            let ptr = leak_color(item_name, r, g, b);
            Some((ptr as *const c_void, ItemReader::Color { ptr, slot: parameter.slot }, parameter.clone()))
        }
        // "angle" and others are not exposed (stay at the AEX default).
        _ => None,
    }
}

/// Produces the names used by AviUtl2's config items.
///
/// AviUtl2 persists config values by item name, while AEX parameter names are
/// only human-facing labels and are not required to be unique. Keep the old
/// name for a unique visible parameter, but add its stable AEX slot to every
/// duplicate. Empty labels get the same slot-based fallback. The final set is
/// checked again so a user-supplied label cannot collide with a generated one.
fn unique_item_names(parameters: &[InteractiveParameter]) -> Vec<Option<String>> {
    let mut counts = HashMap::<String, usize>::new();
    for parameter in parameters.iter().filter(|parameter| parameter.visible) {
        let name = parameter.name.trim();
        if !name.is_empty() {
            *counts.entry(name.to_owned()).or_default() += 1;
        }
    }

    let mut used = HashSet::<String>::new();
    parameters
        .iter()
        .map(|parameter| {
            if !parameter.visible {
                return None;
            }
            let trimmed = parameter.name.trim();
            let base = if trimmed.is_empty() {
                format!("Parameter {}", parameter.slot)
            } else {
                parameter.name.clone()
            };
            let duplicate =
                !trimmed.is_empty() && counts.get(trimmed).copied().unwrap_or_default() > 1;
            let stem = if duplicate || trimmed.is_empty() {
                format!("{base} [slot {}]", parameter.slot)
            } else {
                base
            };
            let mut candidate = stem.clone();
            let mut disambiguator = 2u32;
            while !used.insert(candidate.clone()) {
                candidate = format!("{stem} [{disambiguator}]");
                disambiguator = disambiguator.saturating_add(1);
            }
            Some(candidate)
        })
        .collect()
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
    dependency: DependencyConfig,
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
            // Re-resolve the closure the discovery pass sealed, so the render
            // session's sealed root carries the same dependency DLLs the
            // parameter inspection loaded with (issue #304). Resolving here (on
            // the session thread, once per session) keeps the hashing off both
            // the AviUtl2 callback thread and plugin startup.
            let roots = search_roots_for(&config.plugin, &config.dependency.dirs);
            let dependencies =
                match dependency_closure_for(&config.plugin, &config.dependency, &roots) {
                    Ok(closure) => closure.into_dependencies(),
                    Err(error) => {
                        let _ = open_tx.send(Err(error));
                        return;
                    }
                };
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
                // The multifilter bridge renders video frames only; an audio
                // source would come from the host's audio graph, which it
                // does not read (issue #339).
                audio_trailer: None,
                alpha_as_coverage_params: &[],
                conformance_render_settings: None,
                layers: &[],
                dependencies,
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
                        FrameStatus::FrameError { render_error, .. } => {
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
        dependency: ctx.dependency.clone(),
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
            dependency_inputs: 0,
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
            stale: false,
            checked: build,
            attempts: 0,
            closure: CachedClosure::default(),
            failure_classification: None,
            alias_fallback: false,
            alias_target: None,
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
        let merged = keep_best(Some(&old), failed(5, 64, build(2)), META).unwrap();
        assert!(merged.ok, "an unchanged, previously working AEX stayed ok");
        assert_eq!(merged.sha, "aa", "the old payload was kept");
        assert!(merged.smart);
    }

    /// A failure to stat the AEX is not evidence that it changed, so it must not
    /// open the demotion path either.
    /// With no trustworthy meta there is nothing safe to write: discovery's own
    /// stat may have failed too, and storing its `(0, 0), 0` fallback would make
    /// every later launch see a mismatch and unregister the effect (#307).
    #[test]
    fn an_unreadable_aex_leaves_the_cache_alone() {
        let old = discovered(5, 64, build(1));
        assert!(keep_best(Some(&old), failed(0, 0, build(2)), NO_META).is_none());
        assert!(
            keep_best(None, discovered(0, 0, build(2)), NO_META).is_none(),
            "a successful discovery with no meta is not written either"
        );
    }

    /// A failed re-verification must not pass the payload off as the current
    /// host's work: that hides which host produced it and ends re-verification
    /// for that host, stranding the effect on older parameters after a single
    /// transient failure. The attempt is recorded separately and retried.
    #[test]
    fn a_failed_recheck_does_not_claim_the_new_build() {
        let old = discovered(5, 64, build(1));
        let merged = keep_best(Some(&old), failed(5, 64, build(2)), META).unwrap();
        assert_eq!(merged.build, build(1), "provenance is unchanged");
        assert_eq!(merged.checked, build(2), "but the attempt is recorded");
        assert_eq!(merged.attempts, 1);
        assert_eq!(
            classify(Some(&merged), META, build(2)),
            LoadDecision { register: true, discover: true },
            "still registered, and tried again"
        );
    }

    /// Retries are bounded, so a host that genuinely cannot discover an effect
    /// any more does not re-run a worker for it on every launch forever.
    #[test]
    fn re_verification_gives_up_after_the_retry_budget() {
        let mut entry = discovered(5, 64, build(1));
        for attempt in 1..=RETRY_BUDGET {
            entry = keep_best(Some(&entry), failed(5, 64, build(2)), META).unwrap();
            assert_eq!(entry.attempts, attempt);
            assert!(entry.ok, "registered throughout");
        }
        assert_eq!(
            classify(Some(&entry), META, build(2)),
            LoadDecision { register: true, discover: false },
            "converged: still registered, no longer queued"
        );
        // A different host starts the budget over, since it may well succeed.
        assert_eq!(
            classify(Some(&entry), META, build(3)),
            LoadDecision { register: true, discover: true }
        );
    }

    /// A replaced AEX is a different plug-in, so its old parameters are
    /// meaningless and the negative result must win.
    #[test]
    fn a_replaced_aex_may_become_negative() {
        let old = discovered(5, 64, build(1));
        let newer = Some(((9, 0), 64));
        let resized = Some(((5, 0), 99));
        assert!(!keep_best(Some(&old), failed(9, 64, build(1)), newer).unwrap().ok);
        assert!(!keep_best(Some(&old), failed(5, 99, build(1)), resized).unwrap().ok);
    }

    /// The point of re-verifying at all (issue #304): a host that gained support
    /// for an effect promotes the old negative.
    #[test]
    fn a_new_host_promotes_a_previously_failing_effect() {
        let old = failed(5, 64, build(1));
        let merged = keep_best(Some(&old), discovered(5, 64, build(2)), META).unwrap();
        assert!(merged.ok);
        assert_eq!(merged.build, build(2));
    }

    #[test]
    fn a_first_discovery_is_taken_as_is() {
        assert!(!keep_best(None, failed(5, 64, build(1)), META).unwrap().ok);
        assert!(keep_best(None, discovered(5, 64, build(1)), META).unwrap().ok);
    }

    /// Discovery records the AEX's meta itself, and the file can change between
    /// that stat and the read (or that stat can fail, falling back to `(0, 0), 0`).
    /// Storing the entry under a `(mtime, len)` that never matches again would
    /// make every later launch unregister it, so the merge stamps the meta it read
    /// — and marks the entry stale, because `sha`/`params` may describe the older
    /// bytes and would otherwise stay wrong forever without being re-discovered.
    #[test]
    fn a_discovery_that_raced_the_file_is_stamped_and_marked_stale() {
        let fresh = discovered(9, 99, build(1)); // discovery saw different bytes
        let merged = keep_best(None, fresh, META).unwrap();
        assert_eq!(merged.mtime, (5, 0), "matches the file, so it registers");
        assert_eq!(merged.len, 64);
        assert!(merged.stale);
        assert_eq!(
            classify(Some(&merged), META, build(1)),
            LoadDecision { register: true, discover: true },
            "registered (no object loss) and re-discovered (self-heals)"
        );
    }

    /// The ordinary case must not be marked stale, or every entry re-discovers
    /// on every launch.
    #[test]
    fn an_undisturbed_discovery_is_not_stale() {
        let merged = keep_best(None, discovered(5, 64, build(1)), META).unwrap();
        assert!(!merged.stale);
        assert_eq!(
            classify(Some(&merged), META, build(1)),
            LoadDecision { register: true, discover: false }
        );
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

    /// A stat failure on a path the scan just found is not evidence the AEX
    /// changed, so the cached result keeps being registered. Unregistering it for
    /// this launch would delete objects from saved projects that use it (#307).
    #[test]
    fn an_unstattable_aex_stays_registered() {
        let entry = discovered(5, 64, build(1));
        assert_eq!(
            classify(Some(&entry), NO_META, build(1)),
            LoadDecision { register: true, discover: true },
            "registered from cache, and re-checked in the background"
        );
    }

    /// ...but an unknown AEX with no cached entry still has nothing to register.
    #[test]
    fn an_unstattable_aex_without_a_cache_entry_is_only_discovered() {
        assert_eq!(
            classify(None, NO_META, build(1)),
            LoadDecision { register: false, discover: true }
        );
    }

    /// A cached negative is not resurrected by a stat failure.
    #[test]
    fn an_unstattable_negative_is_still_not_registered() {
        let entry = failed(5, 64, build(1));
        assert_eq!(
            classify(Some(&entry), NO_META, build(1)),
            LoadDecision { register: false, discover: true }
        );
    }

    #[test]
    fn an_unknown_aex_is_only_discovered() {
        assert_eq!(
            classify(None, META, build(1)),
            LoadDecision { register: false, discover: true },
            "never seen"
        );
    }

    /// A replacement must remain visible for this launch. Its cached payload
    /// may fail the SHA check, but keeping the filter registered prevents
    /// AviUtl2 from deleting objects before background discovery replaces the
    /// entry (#309).
    #[test]
    fn a_replaced_aex_keeps_a_known_good_registration_until_rediscovered() {
        let entry = discovered(5, 64, build(1));
        assert_eq!(
            classify(Some(&entry), Some(((9, 0), 64)), build(1)),
            LoadDecision {
                register: true,
                discover: true
            },
            "keep the last known-good filter registered while the replacement is discovered"
        );
    }

    // --- prune: never conclude "gone" from an incomplete scan ---------------

    fn json_entries(keys: &[&str]) -> HashMap<String, serde_json::Value> {
        cache_of(keys)
            .iter()
            .map(|(key, entry)| (key.clone(), serde_json::to_value(entry).unwrap()))
            .collect()
    }

    fn cache_of(keys: &[&str]) -> HashMap<String, CacheEntry> {
        keys.iter()
            .map(|key| ((*key).to_string(), discovered(5, 64, build(1))))
            .collect()
    }

    #[test]
    fn a_complete_scan_prunes_entries_whose_aex_is_gone() {
        let mut cache = cache_of(&["a.aex", "gone.aex"]);
        prune_cache(&mut cache, &[PathBuf::from("a.aex")], &[PathBuf::from("")], true);
        assert_eq!(cache.len(), 1);
        assert!(cache.contains_key("a.aex"));
    }

    /// A folder that could not be read (or a default folder that went missing)
    /// must not make its effects look deleted: pruning them would leave them
    /// unregistered next launch and delete objects from saved projects (#307).
    #[test]
    fn an_incomplete_scan_prunes_nothing() {
        let mut cache = cache_of(&["a.aex", "unscanned.aex"]);
        prune_cache(&mut cache, &[PathBuf::from("a.aex")], &[PathBuf::from("")], false);
        assert_eq!(cache.len(), 2, "the unscanned entry survived");
    }

    #[test]
    fn an_incomplete_scan_registers_cached_entries_under_its_roots() {
        let root = PathBuf::from("scan-root");
        let cached = root.join("temporarily-hidden.aex");
        let outside = PathBuf::from("other-root").join("outside.aex");
        let cache = cache_of(&[
            &cached.to_string_lossy(),
            &outside.to_string_lossy(),
        ]);
        let fallback = cached_fallback_plugins(
            &cache,
            &[root.join("visible.aex")],
            std::slice::from_ref(&root),
            false,
            false,
            &[],
        );
        assert_eq!(fallback, vec![cached]);
    }

    #[test]
    fn missing_scan_roots_keep_all_registerable_cached_entries() {
        let first_root = PathBuf::from("first-root");
        let first = first_root.join("first.aex");
        let second = PathBuf::from("second-root").join("second.aex");
        let cache = cache_of(&[
            &first.to_string_lossy(),
            &second.to_string_lossy(),
        ]);
        let fallback = cached_fallback_plugins(
            &cache,
            &[],
            std::slice::from_ref(&first_root),
            false,
            true,
            &[],
        );
        assert_eq!(fallback.len(), 2);
        assert!(fallback.contains(&first));
        assert!(fallback.contains(&second));
    }

    #[test]
    fn complete_scan_does_not_resurrect_missing_cached_entries() {
        let root = PathBuf::from("scan-root");
        let cached = root.join("gone.aex");
        let cache = cache_of(&[&cached.to_string_lossy()]);
        assert!(cached_fallback_plugins(
            &cache,
            &[root.join("visible.aex")],
            std::slice::from_ref(&root),
            true,
            false,
            &[],
        )
        .is_empty());
    }

    // --- cache file acceptance ----------------------------------------------

    /// Entries must survive being read back; only a schema-version change
    /// discards them (a host-build change is handled per entry).
    #[test]
    fn a_current_cache_file_keeps_its_entries() {
        let file = CacheFile {
            version: CACHE_VERSION,
            entries: json_entries(&["a.aex"]),
        };
        assert_eq!(accept_cache_file(file).len(), 1);
    }

    #[test]
    fn a_future_or_older_schema_is_discarded() {
        let file = CacheFile {
            version: CACHE_VERSION + 1,
            entries: json_entries(&["a.aex"]),
        };
        assert!(accept_cache_file(file).is_empty());
    }

    #[test]
    fn a_concurrent_cache_save_preserves_disjoint_discoveries() {
        let mut local = cache_of(&["local.aex"]);
        let on_disk = cache_of(&["other-process.aex"]);

        merge_cache_entries(&mut local, &on_disk);

        assert!(local.contains_key("local.aex"));
        assert!(local.contains_key("other-process.aex"));
    }

    #[test]
    fn a_concurrent_cache_save_keeps_known_good_for_an_unchanged_aex() {
        let key = "same.aex";
        let mut local = HashMap::from([(key.to_string(), failed(5, 64, build(1)))]);
        let on_disk = HashMap::from([(key.to_string(), discovered(5, 64, build(1)))]);

        merge_cache_entries(&mut local, &on_disk);

        assert!(
            local[key].ok,
            "a transient negative must not erase a good entry"
        );
    }

    #[test]
    fn a_concurrent_cache_save_keeps_current_negative_for_changed_bytes() {
        let key = "changed.aex";
        let mut local = HashMap::from([(key.to_string(), failed(9, 64, build(1)))]);
        let on_disk = HashMap::from([(key.to_string(), discovered(5, 64, build(1)))]);

        merge_cache_entries(&mut local, &on_disk);

        assert!(
            !local[key].ok,
            "a result for older bytes must not be resurrected"
        );
    }

    // --- scan completeness ---------------------------------------------------

    #[test]
    fn a_readable_folder_scans_completely() {
        let dir = std::env::temp_dir().join(format!("aexcompat-mf-{}-scan", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let complete = collect_aex(&[dir], &[]).complete;
        assert!(complete);
    }

    #[test]
    fn an_unreadable_folder_marks_the_scan_incomplete() {
        let missing = std::env::temp_dir().join(format!("aexcompat-mf-{}-missing", std::process::id()));
        let scan = collect_aex(&[missing], &[]);
        assert!(scan.plugins.is_empty());
        assert!(!scan.complete, "a folder that could not be read is not a complete scan");
    }

    // --- default folder resolution ------------------------------------------

    /// A temp dir unique to this test and this process, so concurrent `cargo test`
    /// runs do not delete each other's fixtures.
    /// An entry whose closure was resolved against `roots` and sealed `sealed`.
    fn with_closure(roots: &[&Path], sealed: &[&Path], missing: &[&str]) -> CacheEntry {
        let mut entry = discovered(5, 64, build(1));
        entry.closure = CachedClosure {
            roots: roots
                .iter()
                .map(|root| root.to_string_lossy().into_owned())
                .collect(),
            sealed: sealed
                .iter()
                .map(|path| {
                    let (mtime, len) = file_meta(path).expect("sealed dependency");
                    CachedDependency {
                        path: path.to_string_lossy().into_owned(),
                        mtime,
                        len,
                    }
                })
                .collect(),
            missing: missing.iter().map(|name| name.to_string()).collect(),
            provenance: Vec::new(),
        };
        entry
    }

    #[test]
    fn an_unchanged_closure_is_not_re_discovered() {
        let root = temp_root("closure-stable");
        let dependency = root.join("dvacore.dll");
        std::fs::write(&dependency, b"runtime").unwrap();
        let root = std::fs::canonicalize(&root).unwrap();
        let dependency = std::fs::canonicalize(&dependency).unwrap();

        let entry = with_closure(&[&root], &[&dependency], &["kernel32.dll"]);
        assert!(!needs_closure_recheck(&entry, build(1), &[root.clone()]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_rewritten_or_removed_dependency_re_discovers_that_effect() {
        let root = temp_root("closure-rewritten");
        let dependency = root.join("dvacore.dll");
        std::fs::write(&dependency, b"runtime").unwrap();
        let root = std::fs::canonicalize(&root).unwrap();
        let dependency = std::fs::canonicalize(&dependency).unwrap();
        let entry = with_closure(&[&root], &[&dependency], &[]);

        // An AE update rewriting the DLL in place changes neither the AEX nor the
        // host build, so nothing else would notice it.
        std::fs::write(&dependency, b"a different runtime").unwrap();
        assert!(needs_closure_recheck(&entry, build(1), &[root.clone()]));

        std::fs::remove_file(&dependency).unwrap();
        assert!(needs_closure_recheck(&entry, build(1), &[root.clone()]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_import_that_appears_re_discovers_the_effect_that_wanted_it() {
        let root = temp_root("closure-appeared");
        let root = std::fs::canonicalize(&root).unwrap();
        let entry = with_closure(&[&root], &[], &["helper.dll"]);
        assert!(!needs_closure_recheck(&entry, build(1), &[root.clone()]));

        // The missing dependency turns up: the plug-in that failed for want of it
        // is exactly the one to try again.
        std::fs::write(root.join("helper.dll"), b"helper").unwrap();
        assert!(needs_closure_recheck(&entry, build(1), &[root.clone()]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_entry_with_no_recorded_closure_is_re_verified_once() {
        // Written before #304: nothing recorded, so it cannot be judged unchanged.
        // It is re-verified — and, per issue #307, stays registered meanwhile.
        let root = temp_root("closure-legacy");
        let root = std::fs::canonicalize(&root).unwrap();
        let entry = discovered(5, 64, build(1));
        assert!(needs_closure_recheck(&entry, build(1), &[root.clone()]));
        assert!(classify(Some(&entry), META, build(1)).register);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_closure_recheck_gives_up_after_the_retry_budget() {
        // Otherwise one rewritten runtime DLL re-runs a worker for every effect on
        // every launch: the re-discovery fails, `keep_best` keeps the old entry
        // and its old record, and the trigger fires again unchanged.
        let root = temp_root("closure-budget");
        let root = std::fs::canonicalize(&root).unwrap();
        let mut entry = with_closure(&[&root], &[], &["helper.dll"]);
        std::fs::write(root.join("helper.dll"), b"helper").unwrap();
        assert!(needs_closure_recheck(&entry, build(1), &[root.clone()]));

        entry.checked = build(1);
        entry.attempts = RETRY_BUDGET;
        assert!(!needs_closure_recheck(&entry, build(1), &[root.clone()]));
        // A different host gets its own budget.
        assert!(needs_closure_recheck(&entry, build(2), &[root.clone()]));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aexcompat-mf-{}-{tag}", std::process::id()));
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
        let root = std::env::temp_dir().join(format!("aexcompat-mf-{}-no-root", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert!(picked.is_none());
        assert!(!complete);
    }

    /// The cache file is shared across scan configurations, so a launch that
    /// scanned only one folder must not conclude the other folders' effects are
    /// gone — that would unregister them all on the next normal launch (#307).
    #[test]
    fn a_prune_only_judges_the_folders_it_scanned() {
        let scanned = PathBuf::from("scan-root");
        let inside = scanned.join("gone.aex").to_string_lossy().into_owned();
        let outside = PathBuf::from("other-root")
            .join("kept.aex")
            .to_string_lossy()
            .into_owned();
        let mut cache = cache_of(&[&inside, &outside]);
        prune_cache(&mut cache, &[], &[scanned], true);
        assert!(!cache.contains_key(&inside), "scanned and absent: pruned");
        assert!(
            cache.contains_key(&outside),
            "outside the scanned roots: untouched"
        );
    }

    /// `CacheEntry::params` embeds a broker type whose fields are not all
    /// defaulted. One entry that no longer parses must not empty the cache and
    /// unregister every filter for a launch (#307).
    #[test]
    fn one_unparseable_entry_does_not_discard_the_rest() {
        let mut entries = json_entries(&["good.aex"]);
        entries.insert("broken.aex".into(), serde_json::json!({"mtime": "not-a-tuple"}));
        let accepted = accept_cache_file(CacheFile { version: CACHE_VERSION, entries });
        assert_eq!(accepted.len(), 1);
        assert!(accepted.contains_key("good.aex"));
    }

    /// A host fingerprint that could not be read is not "a different host":
    /// treating it as one re-discovers everything twice for one failed stat.
    #[test]
    fn an_unknown_host_build_does_not_force_rediscovery() {
        let entry = discovered(5, 64, build(1));
        let unknown = BuildFingerprint::default();
        assert!(!unknown.is_known());
        assert_eq!(
            classify(Some(&entry), META, unknown),
            LoadDecision { register: true, discover: false }
        );
    }

    /// An uninstall leaves empty version folders behind; one older than the pick
    /// says nothing about the install being incomplete.
    #[test]
    fn a_leafless_older_version_does_not_mark_the_resolution_incomplete() {
        let root = temp_root("leftover");
        std::fs::create_dir_all(root.join("App 2025").join("Plug-ins")).unwrap();
        std::fs::create_dir_all(root.join("App 2019")).unwrap(); // uninstall leftover
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert_eq!(picked.unwrap(), root.join("App 2025").join("Plug-ins"));
        assert!(complete);
    }

    /// The MediaCore shape: no prefix, so every entry under the root is a
    /// candidate.
    #[test]
    fn an_empty_prefix_picks_the_newest_bare_version() {
        let root = temp_root("mediacore");
        for version in ["7.0", "10.0", "CS6"] {
            std::fs::create_dir_all(root.join(version).join("MediaCore")).unwrap();
        }
        let (picked, complete) = newest_versioned(&root, "", &["MediaCore"]);
        assert_eq!(picked.unwrap(), root.join("10.0").join("MediaCore"));
        assert!(complete);
    }

    /// A re-verification that failed must not clear the stale mark: the entry's
    /// sha/params would stay wrong forever with nothing left to re-discover it,
    /// so every frame would fail the sha check (issue #307's recovery step is the
    /// cache deletion that itself risks the data loss).
    #[test]
    fn a_failed_recheck_keeps_an_existing_stale_mark() {
        let mut old = discovered(5, 64, build(1));
        old.stale = true;
        let merged = keep_best(Some(&old), failed(5, 64, build(2)), META).unwrap();
        assert!(merged.ok, "still registered");
        assert!(merged.stale, "still queued for another attempt");
        assert_eq!(
            classify(Some(&merged), META, build(2)),
            LoadDecision { register: true, discover: true }
        );
    }

    /// An ignored AEX is still on disk. Pruning its entry would leave it
    /// unregistered on the launch after it is taken back out of `ignore` (#307).
    #[test]
    fn an_ignored_aex_keeps_its_cache_entry() {
        let root = temp_root("ignored");
        std::fs::write(root.join("keep.aex"), b"x").unwrap();
        std::fs::write(root.join("skip.aex"), b"x").unwrap();
        let scan = collect_aex(std::slice::from_ref(&root), &["skip".into()]);
        assert_eq!(scan.plugins.len(), 1, "the ignored one is not registered");
        assert_eq!(scan.seen.len(), 2, "but it was seen");

        let key = root.join("skip.aex").to_string_lossy().into_owned();
        let mut cache = cache_of(&[&key]);
        prune_cache(&mut cache, &scan.seen, &[root], true);
        assert!(cache.contains_key(&key), "an ignored AEX is not gone");
    }

    /// A stray file whose name carries digits is not an install, and must not
    /// look like one missing its leaf — that would disable the prune forever.
    #[test]
    fn a_stray_file_is_not_a_version() {
        let root = temp_root("stray-file");
        std::fs::create_dir_all(root.join("App 2025").join("Plug-ins")).unwrap();
        std::fs::write(root.join("App 2026.log"), b"x").unwrap();
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert_eq!(picked.unwrap(), root.join("App 2025").join("Plug-ins"));
        assert!(complete);
    }

    #[test]
    fn dependency_provenance_is_additive_and_round_trips() {
        let closure = CachedClosure {
            provenance: vec![CachedDependencyProvenance {
                basename: "runtime.dll".into(),
                import_derived: false,
                string_derived: true,
            }],
            ..CachedClosure::default()
        };
        let encoded = serde_json::to_value(&closure).unwrap();
        assert_eq!(encoded["provenance"][0]["basename"], "runtime.dll");
        assert_eq!(encoded["provenance"][0]["string_derived"], true);
        let decoded: CachedClosure = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.provenance.len(), 1);
        assert!(!decoded.provenance[0].import_derived);
        assert!(decoded.provenance[0].string_derived);
    }

    /// The cache embeds the broker's `InteractiveParameter`, whose fields are not
    /// all defaulted, so a field added there stops every entry that has
    /// parameters from deserializing at once — every registerable filter, on
    /// users' machines, with the object deletion of issue #307 behind it. Pin the
    /// shape here so that change fails in `cargo test` instead.
    ///
    /// If this fails because the broker type gained a field: give the new field
    /// `#[serde(default)]` there (so old caches still read), then add it here.
    #[test]
    fn the_cached_parameter_schema_is_stable() {
        let frozen = serde_json::json!({
            "slot": 0,
            "name": "Intensity",
            "kind": "float",
            "minimum": 0.0,
            "maximum": 100.0,
            "value": 50.0,
            "choices": [],
            "color": [0, 0, 0, 255],
            "components": [0.0, 0.0, 0.0],
            "component_count": 0,
            "layer_path": null,
            "enabled": true,
            "visible": true,
            "supervised": false,
        });
        let parsed = serde_json::from_value::<InteractiveParameter>(frozen.clone());
        assert!(
            parsed.is_ok(),
            "a cache written by an older build no longer deserializes: {:?}",
            parsed.err()
        );
        // And the fields we persist still round-trip.
        let value = serde_json::to_value(parsed.unwrap()).unwrap();
        for key in frozen.as_object().unwrap().keys() {
            assert!(value.get(key).is_some(), "field `{key}` disappeared from the schema");
        }
    }

    #[test]
    fn non_finite_parameters_are_normalized_before_cache_round_trip() {
        let mut parameters = vec![InteractiveParameter {
            slot: 1,
            name: "Broken range".into(),
            kind: "float".into(),
            minimum: f64::NAN,
            maximum: f64::INFINITY,
            value: f64::NEG_INFINITY,
            choices: Vec::new(),
            color: [0, 0, 0, 255],
            components: [f64::NAN, 0.5, f64::INFINITY],
            component_count: 3,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }];

        normalize_parameters_for_cache(&mut parameters);
        let parameter = &parameters[0];
        assert_eq!(
            (parameter.minimum, parameter.maximum, parameter.value),
            (0.0, 1.0, 0.0)
        );
        assert_eq!(parameter.components, [0.0, 0.5, 0.0]);

        let encoded = serde_json::to_value(&parameters).expect("finite parameters serialize");
        let decoded: Vec<InteractiveParameter> =
            serde_json::from_value(encoded).expect("normalized parameters deserialize");
        assert_eq!(decoded[0].value, 0.0);
        assert!(decoded[0].components.iter().all(|value| value.is_finite()));
    }

    /// The same hazard for this crate's own entry shape: adding a field without
    /// `#[serde(default)]` stops every existing cache entry from deserializing.
    #[test]
    fn an_older_cache_entry_shape_still_reads() {
        // What an entry written before `params`/`build`/`stale` existed looks like.
        let oldest = serde_json::json!({
            "mtime": [5, 0],
            "len": 64,
            "ok": true,
            "sha": "aa",
            "smart": true,
        });
        let entry: CacheEntry = serde_json::from_value(oldest)
            .expect("an entry from an older build must still read, or every filter unregisters");
        assert!(entry.ok);
        assert!(entry.params.is_empty());
        assert!(!entry.stale);
        assert_eq!(entry.build, BuildFingerprint::default());
    }

    /// A junctioned install (Adobe moved to another drive) must still be found:
    /// `DirEntry::file_type` calls a junction a symlink, not a directory.
    #[cfg(windows)]
    #[test]
    fn a_junctioned_install_is_still_found() {
        let root = temp_root("junction");
        let real = root.join("real");
        std::fs::create_dir_all(real.join("Plug-ins")).unwrap();
        let link = root.join("App 2025");
        junction(&link, &real);
        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert_eq!(picked.unwrap(), link.join("Plug-ins"));
        assert!(complete);
    }

    /// The same for a junctioned subfolder of a scan root: missing it would leave
    /// its AEX out of `seen`, and the prune would then delete their entries.
    #[cfg(windows)]
    #[test]
    fn a_junctioned_subfolder_is_scanned() {
        let root = temp_root("junction-scan");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("deep.aex"), b"x").unwrap();
        let scanned = root.join("scanned");
        std::fs::create_dir_all(&scanned).unwrap();
        junction(&scanned.join("linked"), &real);
        let scan = collect_aex(std::slice::from_ref(&scanned), &[]);
        assert_eq!(scan.seen.len(), 1, "the AEX behind the junction was seen");
        assert!(scan.complete);
    }

    /// Creates a directory junction, failing loudly rather than letting the test
    /// pass without exercising anything. Junctions need no elevation on NTFS.
    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) {
        let out = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("run mklink");
        assert!(
            out.status.success(),
            "could not create a junction, so this test proves nothing: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A junction whose target is not mounted this launch hides whatever is
    /// behind it. Reporting the scan as complete would let the prune delete those
    /// AEX's cache entries, unregistering them once the drive is back (#307).
    #[cfg(windows)]
    #[test]
    fn an_unresolvable_junction_marks_the_scan_incomplete() {
        let root = temp_root("dangling");
        let target = root.join("target");
        std::fs::create_dir_all(target.join("sub")).unwrap();
        let scanned = root.join("scanned");
        std::fs::create_dir_all(&scanned).unwrap();
        junction(&scanned.join("linked"), &target);
        std::fs::remove_dir_all(&target).unwrap(); // the drive went away

        let scan = collect_aex(std::slice::from_ref(&scanned), &[]);
        assert!(
            !scan.complete,
            "an unresolvable link is 'not looked at', not 'nothing there'"
        );

        let hidden = scanned
            .join("linked")
            .join("deep.aex")
            .to_string_lossy()
            .into_owned();
        let mut cache = cache_of(&[&hidden]);
        prune_cache(&mut cache, &scan.seen, &[scanned], scan.complete);
        assert!(cache.contains_key(&hidden), "its entry survived");
    }

    /// The same for a version folder that is an unresolvable junction: falling
    /// back to an older version must not also claim the resolution was complete.
    #[cfg(windows)]
    #[test]
    fn an_unresolvable_version_junction_marks_the_resolution_incomplete() {
        let root = temp_root("dangling-version");
        std::fs::create_dir_all(root.join("App 2024").join("Plug-ins")).unwrap();
        let target = root.join("target");
        std::fs::create_dir_all(&target).unwrap();
        junction(&root.join("App 2025"), &target);
        std::fs::remove_dir_all(&target).unwrap();

        let (picked, complete) = newest_versioned(&root, "App ", &["Plug-ins"]);
        assert_eq!(picked.unwrap(), root.join("App 2024").join("Plug-ins"));
        assert!(!complete, "the newer install was not visible, not absent");
    }

    /// A junction pointing back up the tree must not expose the same AEX as a
    /// pile of duplicate filters (each with its own discovery worker).
    #[cfg(windows)]
    #[test]
    fn a_junction_loop_does_not_duplicate_an_aex() {
        let root = temp_root("junction-loop");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("only.aex"), b"x").unwrap();
        junction(&root.join("loop"), &root);
        let scan = collect_aex(std::slice::from_ref(&root), &[]);
        assert_eq!(scan.seen.len(), 1, "one AEX, seen once: {:?}", scan.seen);
    }

    /// Two scan roots that reach the same folder through a junction likewise
    /// must not register everything under it twice.
    #[cfg(windows)]
    #[test]
    fn two_roots_crossing_through_a_junction_do_not_duplicate() {
        let root = temp_root("junction-cross");
        let shared = root.join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("one.aex"), b"x").unwrap();
        let other = root.join("other");
        std::fs::create_dir_all(&other).unwrap();
        junction(&other.join("link"), &shared);
        let scan = collect_aex(&[shared.clone(), other], &[]);
        assert_eq!(scan.seen.len(), 1, "one AEX, seen once: {:?}", scan.seen);
        assert!(scan.complete, "reaching it twice is not an incomplete scan");
    }

    /// The scan lists one spelling per AEX, so an entry keyed by another path to
    /// the same file (reached through a junction) is missing from the listing but
    /// is not gone. Pruning it would unregister that filter next launch (#307).
    #[cfg(windows)]
    #[test]
    fn an_entry_reachable_under_another_name_is_not_pruned() {
        let root = temp_root("alias");
        let real = root.join("Effects");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("AAA-link"), &real);

        let scan = collect_aex(std::slice::from_ref(&root), &[]);
        assert_eq!(scan.seen.len(), 1, "walked once: {:?}", scan.seen);

        // Key the cache by the spelling the scan did NOT keep.
        let other = if scan.seen[0].starts_with(&real) {
            root.join("AAA-link").join("foo.aex")
        } else {
            real.join("foo.aex")
        };
        let key = other.to_string_lossy().into_owned();
        let mut cache = cache_of(&[&key]);
        prune_cache(&mut cache, &scan.seen, &[root], scan.complete);
        assert!(cache.contains_key(&key), "the file is still there, so is its entry");
    }

    /// An AEX that really is gone still goes, or the cache never shrinks.
    #[test]
    fn a_deleted_aex_is_still_pruned() {
        let root = temp_root("deleted");
        let key = root.join("gone.aex").to_string_lossy().into_owned();
        let mut cache = cache_of(&[&key]);
        prune_cache(&mut cache, &[], &[root], true);
        assert!(cache.is_empty(), "the file does not exist, so the entry goes");
    }

    /// The spelling the scan walks can change between launches (a junction added
    /// or renamed, a different scan-root order) without the file changing. The
    /// entry must still be found, or that effect is unregistered for a launch and
    /// saved projects lose the objects using it (#307).
    #[cfg(windows)]
    #[test]
    fn an_entry_keyed_under_another_name_is_still_found() {
        let root = temp_root("alias-lookup");
        let real = root.join("Effects");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("AAA-link"), &real);

        let scan = collect_aex(std::slice::from_ref(&root), &[]);
        assert_eq!(scan.seen.len(), 1);
        let walked = &scan.seen[0];
        // Key the cache by the other spelling, as an earlier launch would have.
        let other = if walked.starts_with(&real) {
            root.join("AAA-link").join("foo.aex")
        } else {
            real.join("foo.aex")
        };
        let cache = cache_of(&[&other.to_string_lossy()]);

        assert!(
            !cache.contains_key(&walked.to_string_lossy().into_owned()),
            "the exact key really does miss"
        );
        let index = index_by_real_path(&cache, std::slice::from_ref(&root), build(1));
        let found = walked
            .canonicalize()
            .ok()
            .and_then(|real| index.get(&real))
            .and_then(|candidates| cache.get(&candidates[0]));
        assert!(found.is_some(), "but the real path finds it");
    }

    /// After an aliased hit the entry must also be reachable under the spelling
    /// the scan walked: the background pass keys by that, and without it
    /// `keep_best` sees no cached entry, so a transient discovery failure would
    /// write a negative and unregister the effect next launch (#307).
    #[test]
    fn an_aliased_entry_becomes_reachable_under_the_walked_key() {
        let mut cache = cache_of(&["old-spelling.aex"]);
        apply_rekey(
            &mut cache,
            vec![("old-spelling.aex".into(), "walked.aex".into())],
        );
        let moved = cache.get("walked.aex").expect("found under the walked key");
        assert!(moved.ok);

        // And now the background merge sees it, so a failed recheck cannot demote.
        let merged = keep_best(Some(moved), failed(5, 64, build(2)), META).unwrap();
        assert!(merged.ok, "the demotion guard applies again");
    }

    /// The alias index only covers the folders this launch scanned, so a leftover
    /// key elsewhere (a disconnected drive) is never resolved at startup.
    #[test]
    fn the_alias_index_only_covers_the_scanned_roots() {
        let root = temp_root("alias-scope");
        std::fs::write(root.join("here.aex"), b"x").unwrap();
        let inside = root.join("here.aex").to_string_lossy().into_owned();
        let outside = PathBuf::from("elsewhere")
            .join("far.aex")
            .to_string_lossy()
            .into_owned();
        let cache = cache_of(&[&inside, &outside]);
        let index = index_by_real_path(&cache, std::slice::from_ref(&root), build(1));
        assert_eq!(index.len(), 1, "only the key under the scanned root");
        assert!(index.values().any(|keys| keys.contains(&inside)));
    }

    /// The walked spelling can be the temporary one. If the scan reached the AEX
    /// through a junction that is gone next launch, deleting the original would
    /// leave nothing that resolves, and the effect would go unregistered (#307).
    #[test]
    fn re_keying_keeps_the_original_spelling() {
        let mut cache = cache_of(&["stable.aex"]);
        apply_rekey(&mut cache, vec![("stable.aex".into(), "via-junction.aex".into())]);
        assert!(cache.contains_key("stable.aex"));
        assert!(cache.contains_key("via-junction.aex"));
    }

    #[test]
    fn a_live_alias_copy_does_not_keep_alias_lookup_hot() {
        let root = PathBuf::from("root");
        let alias = root.join("stable.aex").to_string_lossy().into_owned();
        let walked = root.join("walked.aex").to_string_lossy().into_owned();
        let mut cache = cache_of(&[&alias]);
        apply_rekey(&mut cache, vec![(alias.clone(), walked.clone())]);

        assert!(!alias_possible(
            &cache,
            &walked_set(&[&walked]),
            std::slice::from_ref(&root),
        ));
        cache.remove(&walked);
        assert!(
            alias_possible(&cache, &walked_set(&[&walked]), std::slice::from_ref(&root)),
            "the fallback becomes eligible again if its walked copy disappears"
        );
    }

    /// When both spellings of one file are cached and they disagree (only the
    /// walked one is refreshed by discovery), the alias lookup must land on the
    /// one that registers, and must do so every launch rather than by iteration
    /// order — otherwise the effect flickers in and out (#307).
    #[cfg(windows)]
    #[test]
    fn the_alias_index_prefers_an_entry_that_registers() {
        let root = temp_root("index-preference");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);

        let direct = real.join("foo.aex").to_string_lossy().into_owned();
        let via_link = root.join("link").join("foo.aex").to_string_lossy().into_owned();

        // Both spellings resolve to one file, so they collide in the index.
        // Whichever spelling holds the negative, the registering entry must win.
        for (negative, positive) in [(&direct, &via_link), (&via_link, &direct)] {
            let mut cache = HashMap::new();
            cache.insert(negative.clone(), failed(5, 64, build(1)));
            cache.insert(positive.clone(), discovered(5, 64, build(1)));
            let index = index_by_real_path(&cache, std::slice::from_ref(&root), build(1));
            assert_eq!(index.len(), 1, "both spellings resolved to one file");
            let winner = &index.values().next().unwrap()[0];
            assert!(cache[winner].ok, "the registering entry won");
        }
    }

    /// Copying an aliased entry makes "one file, two cached spellings, both
    /// registerable" the normal case, so the pick has to stay put across launches
    /// — otherwise the effect's parameters (frozen by AviUtl2 at load) change
    /// depending on hash order.
    #[cfg(windows)]
    #[test]
    fn the_alias_index_picks_the_same_spelling_every_time() {
        let root = temp_root("index-stable");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);

        let direct = real.join("foo.aex").to_string_lossy().into_owned();
        let via_link = root.join("link").join("foo.aex").to_string_lossy().into_owned();

        let mut winners = std::collections::HashSet::new();
        for _ in 0..64 {
            let mut cache = HashMap::new();
            // Both registerable and both on the current build: only the tie-break
            // decides. Different sha so the winner is identifiable.
            let mut a = discovered(5, 64, build(1));
            a.sha = "aaa".into();
            let mut b = discovered(5, 64, build(1));
            b.sha = "bbb".into();
            cache.insert(direct.clone(), a);
            cache.insert(via_link.clone(), b);
            let index = index_by_real_path(&cache, std::slice::from_ref(&root), build(1));
            assert_eq!(index.len(), 1);
            winners.insert(cache[&index.values().next().unwrap()[0]].sha.clone());
        }
        assert_eq!(winners.len(), 1, "one winner across runs, got {winners:?}");
    }

    /// An entry the current host produced beats a leftover from an older one.
    #[cfg(windows)]
    #[test]
    fn the_alias_index_prefers_the_current_host_build() {
        let root = temp_root("index-build");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);

        let direct = real.join("foo.aex").to_string_lossy().into_owned();
        let via_link = root.join("link").join("foo.aex").to_string_lossy().into_owned();

        for (stale_key, fresh_key) in [(&direct, &via_link), (&via_link, &direct)] {
            let mut cache = HashMap::new();
            let mut stale = discovered(5, 64, build(1));
            stale.sha = "old".into();
            let mut fresh = discovered(5, 64, build(2));
            fresh.sha = "new".into();
            cache.insert(stale_key.clone(), stale);
            cache.insert(fresh_key.clone(), fresh);
            let index = index_by_real_path(&cache, std::slice::from_ref(&root), build(2));
            let winner = &index.values().next().unwrap()[0];
            assert_eq!(cache[winner].sha, "new", "the current build's entry won");
        }
    }

    /// Only the walked spelling is refreshed, so the copy left under another one
    /// can be the newer of the two. When the entry found directly would not
    /// register, the alias must still be consulted, or the effect is unregistered
    /// for that launch even though a usable result is cached (#307).
    #[cfg(windows)]
    #[test]
    fn a_negative_direct_hit_still_falls_back_to_a_usable_alias() {
        let root = temp_root("stale-direct");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);

        let walked = real.join("foo.aex").to_string_lossy().into_owned();
        let other = root.join("link").join("foo.aex").to_string_lossy().into_owned();

        // The spelling being walked holds an old negative; the other spelling
        // holds the result a later pass discovered.
        let mut cache = HashMap::new();
        cache.insert(walked.clone(), failed(5, 64, build(1)));
        cache.insert(other.clone(), discovered(5, 64, build(1)));

        let direct = cache.get(&walked);
        assert!(
            !classify(direct, META, build(1)).register,
            "the direct hit alone would not register"
        );
        let index = index_by_real_path(&cache, std::slice::from_ref(&root), build(1));
        let candidates = index
            .get(&real.join("foo.aex").canonicalize().unwrap())
            .expect("the file is in the index");
        let alias = candidates
            .iter()
            .find(|alias| classify(cache.get(*alias), META, build(1)).register)
            .expect("one of the spellings registers");
        assert_eq!(alias, &other);
    }

    // --- resolve_cached: which spelling's entry gets used --------------------

    /// Only the walked spelling is refreshed, so a copy under another one can be
    /// the newer of the two. When what is held under the walked spelling would
    /// not register, the alias must be adopted, or the effect goes unregistered
    /// for that launch and saved projects lose the objects using it (#307).
    #[cfg(windows)]
    #[test]
    fn a_negative_direct_hit_adopts_a_usable_alias() {
        let root = temp_root("resolve-adopt");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);
        let walked = real.join("foo.aex");
        let other = root.join("link").join("foo.aex").to_string_lossy().into_owned();

        let mut cache = HashMap::new();
        cache.insert(walked.to_string_lossy().into_owned(), failed(5, 64, build(1)));
        cache.insert(other.clone(), discovered(5, 64, build(1)));

        let mut aliases = None;
        let (entry, alias) = resolve_cached(
            &cache,
            &walked.to_string_lossy(),
            &walked,
            META,
            build(1),
            std::slice::from_ref(&root),
            true,
            &mut aliases,
        );
        assert!(entry.is_some_and(|entry| entry.ok), "adopted the usable entry");
        assert_eq!(alias.as_deref(), Some(other.as_str()), "and reports the re-key");
    }

    /// A spelling that already registers must not pay for the alias lookup.
    #[test]
    fn a_usable_direct_hit_never_consults_the_index() {
        let cache = cache_of(&["a.aex"]);
        let mut aliases = None;
        let (entry, alias) = resolve_cached(
            &cache,
            "a.aex",
            Path::new("a.aex"),
            META,
            build(1),
            &[PathBuf::from("")],
            true,
            &mut aliases,
        );
        assert!(entry.is_some_and(|entry| entry.ok));
        assert_eq!(alias, None);
        assert!(aliases.is_none(), "the index was never built");
    }

    /// And when no other spelling can exist, the lookup is skipped outright.
    #[test]
    fn nothing_is_resolved_when_no_alias_can_exist() {
        let cache = cache_of(&["gone.aex"]);
        let mut aliases = None;
        let (entry, alias) = resolve_cached(
            &cache,
            "missing.aex",
            Path::new("missing.aex"),
            META,
            build(1),
            &[PathBuf::from("")],
            false,
            &mut aliases,
        );
        assert!(entry.is_none());
        assert_eq!(alias, None);
        assert!(aliases.is_none(), "no filesystem work at all");
    }

    // --- alias_rank ----------------------------------------------------------

    /// An entry written before the `build` field existed carries the default,
    /// which is also what an unreadable current fingerprint is. Comparing them
    /// would rank the legacy entry above a freshly discovered one.
    #[test]
    fn an_unknown_build_does_not_favour_a_legacy_entry() {
        let unknown = BuildFingerprint::default();
        let legacy = discovered(5, 64, unknown);
        let fresh = discovered(5, 64, build(1));
        assert_eq!(
            alias_rank(Some(&legacy), unknown),
            alias_rank(Some(&fresh), unknown),
            "with no usable fingerprint, neither wins on build"
        );
    }

    #[test]
    fn a_current_build_entry_outranks_an_older_one() {
        let old = discovered(5, 64, build(1));
        let current = discovered(5, 64, build(2));
        assert!(alias_rank(Some(&current), build(2)) > alias_rank(Some(&old), build(2)));
    }

    /// A stale entry's sha/params may describe older bytes, so a sound entry
    /// wins even if it came from an older host.
    #[test]
    fn a_sound_entry_outranks_a_stale_one() {
        let mut stale = discovered(5, 64, build(2));
        stale.stale = true;
        let sound = discovered(5, 64, build(1));
        assert!(alias_rank(Some(&sound), build(2)) > alias_rank(Some(&stale), build(2)));
    }

    #[test]
    fn a_registerable_entry_outranks_a_negative_one() {
        let ok = discovered(5, 64, build(1));
        let negative = failed(5, 64, build(1));
        assert!(alias_rank(Some(&ok), build(1)) > alias_rank(Some(&negative), build(1)));
    }

    /// The rank cannot tell whether an entry still describes the file, so a
    /// better-ranked but outdated spelling must not shadow a usable one — that
    /// would leave the effect unregistered even though a usable result is cached
    /// (#307). Both entries here rank equally, so the tie-break orders them and
    /// the lookup has to fall through to the second.
    #[cfg(windows)]
    #[test]
    fn an_outdated_candidate_does_not_shadow_a_usable_one() {
        let root = temp_root("candidate-fallthrough");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);
        let walked = root.join("zzz-missing.aex"); // not cached at all

        for (outdated, usable) in [
            (real.join("foo.aex"), root.join("link").join("foo.aex")),
            (root.join("link").join("foo.aex"), real.join("foo.aex")),
        ] {
            let mut cache = HashMap::new();
            // Same rank (ok, not stale, same build); only the meta differs.
            cache.insert(
                outdated.to_string_lossy().into_owned(),
                discovered(9, 99, build(1)),
            );
            cache.insert(
                usable.to_string_lossy().into_owned(),
                discovered(5, 64, build(1)),
            );
            let mut aliases = None;
            let (entry, alias) = resolve_cached(
                &cache,
                &walked.to_string_lossy(),
                &real.join("foo.aex"),
                META,
                build(1),
                std::slice::from_ref(&root),
                true,
                &mut aliases,
            );
            assert_eq!(entry.map(|entry| entry.len), Some(64), "took the usable one");
            assert_eq!(alias.as_deref(), Some(&*usable.to_string_lossy()));
        }
    }

    /// When no spelling registers, nothing is adopted and nothing is re-keyed.
    #[cfg(windows)]
    #[test]
    fn an_unusable_alias_is_not_adopted() {
        let root = temp_root("alias-unusable");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);
        let walked = real.join("foo.aex");

        let mut cache = HashMap::new();
        cache.insert(walked.to_string_lossy().into_owned(), failed(5, 64, build(1)));
        cache.insert(
            root.join("link").join("foo.aex").to_string_lossy().into_owned(),
            failed(5, 64, build(1)),
        );
        let mut aliases = None;
        let (entry, alias) = resolve_cached(
            &cache,
            &walked.to_string_lossy(),
            &walked,
            META,
            build(1),
            std::slice::from_ref(&root),
            true,
            &mut aliases,
        );
        assert!(entry.is_some_and(|entry| !entry.ok), "kept what was there");
        assert_eq!(alias, None, "nothing worth re-keying");
    }

    // --- alias_possible ------------------------------------------------------

    fn walked_set(keys: &[&str]) -> std::collections::HashSet<String> {
        keys.iter().map(|key| (*key).to_string()).collect()
    }

    /// A cached key under a scan root that this scan did not walk is exactly the
    /// case the alias lookup exists for.
    #[test]
    fn an_unwalked_key_under_a_root_means_an_alias_may_exist() {
        let root = PathBuf::from("root");
        let cache = cache_of(&[&root.join("old-name.aex").to_string_lossy()]);
        assert!(alias_possible(
            &cache,
            &walked_set(&[&root.join("walked.aex").to_string_lossy()]),
            std::slice::from_ref(&root),
        ));
    }

    /// When every in-scope key is one the scan walked, there is no other spelling
    /// and the lookup is pure cost.
    #[test]
    fn all_keys_walked_means_no_alias_can_exist() {
        let root = PathBuf::from("root");
        let key = root.join("walked.aex").to_string_lossy().into_owned();
        let cache = cache_of(&[&key]);
        assert!(!alias_possible(
            &cache,
            &walked_set(&[&key]),
            std::slice::from_ref(&root)
        ));
    }

    /// Keys outside the scanned roots say nothing: they are never registered from
    /// and never pruned.
    #[test]
    fn keys_outside_the_roots_do_not_imply_an_alias() {
        let root = PathBuf::from("root");
        let cache = cache_of(&["elsewhere/other.aex"]);
        assert!(!alias_possible(
            &cache,
            &walked_set(&[]),
            std::slice::from_ref(&root)
        ));
    }

    /// A stale entry carries the meta just read from disk, so the no-demotion
    /// guard holds even when what is there now genuinely does not discover: the
    /// merge is a fixed point, so the entry stays registered on the older bytes'
    /// payload and is re-checked every launch instead of converging. That is
    /// deliberate — excluding stale entries here would unregister one whose
    /// re-check merely timed out, deleting objects out of saved projects (#307),
    /// and would give up the self-healing that one later success provides.
    /// Converging safely needs the failure's classification (#328). Pinned so the
    /// trade-off is not reversed by accident.
    #[test]
    fn a_stale_entry_does_not_converge_on_a_failed_recheck() {
        let mut stale = discovered(9, 128, build(1));
        stale.stale = true;
        stale.sha = "older-bytes".into();

        // What is on disk now is the replacement, and it fails to discover.
        let replacement = Some(((9, 0), 128));
        let merged = keep_best(Some(&stale), failed(9, 128, build(1)), replacement).unwrap();

        assert!(merged.ok, "still registered, so objects survive");
        assert_eq!(merged.sha, "older-bytes", "on the older bytes' payload");
        assert!(merged.stale, "and queued again");
        assert_eq!(
            classify(Some(&merged), replacement, build(1)),
            LoadDecision { register: true, discover: true }
        );
        // A fixed point: re-checking again cannot move it, which is what "does
        // not converge" means here.
        assert_eq!((merged.ok, merged.stale, &merged.sha), (stale.ok, stale.stale, &stale.sha));
        assert_eq!((merged.mtime, merged.len, merged.build), (stale.mtime, stale.len, stale.build));
    }

    #[test]
    fn a_stale_entry_converges_on_a_deterministic_failure() {
        let mut stale = discovered(9, 128, build(1));
        stale.stale = true;
        stale.sha = "older-bytes".into();
        let mut failed = failed(9, 128, build(1));
        failed.failure_classification = Some("nonzero_exit".into());

        let merged = keep_best(Some(&stale), failed, Some(((9, 0), 128))).unwrap();
        assert!(
            !merged.ok,
            "the deterministically rejected replacement is negative"
        );
        assert!(!merged.stale, "a permanent failure is no longer queued");
        assert_eq!(
            classify(Some(&merged), Some(((9, 0), 128)), build(1)),
            LoadDecision {
                register: false,
                discover: false
            }
        );
    }

    #[test]
    fn a_stale_entry_keeps_retrying_after_a_timeout_failure() {
        let mut stale = discovered(9, 128, build(1));
        stale.stale = true;
        let mut failed = failed(9, 128, build(1));
        failed.failure_classification = Some("timeout_killed".into());

        let merged = keep_best(Some(&stale), failed, Some(((9, 0), 128))).unwrap();
        assert!(merged.ok);
        assert!(merged.stale);
        assert_eq!(
            classify(Some(&merged), Some(((9, 0), 128)), build(1)),
            LoadDecision {
                register: true,
                discover: true
            }
        );
    }

    #[test]
    fn inspection_errors_preserve_the_broker_failure_classification() {
        let error = std::io::Error::other(
            r#"inspection failed: diagnostics={"classification":"crashed","exit_code":3221225477}"#,
        );
        assert_eq!(
            inspection_failure_classification(&error).as_deref(),
            Some("crashed")
        );
    }

    /// The same entry does converge as soon as a re-check succeeds.
    #[test]
    fn a_stale_entry_converges_on_a_successful_recheck() {
        let mut stale = discovered(9, 128, build(1));
        stale.stale = true;
        stale.sha = "older-bytes".into();

        let replacement = Some(((9, 0), 128));
        let mut fresh = discovered(9, 128, build(1));
        fresh.sha = "replacement".into();
        let merged = keep_best(Some(&stale), fresh, replacement).unwrap();
        assert_eq!(merged.sha, "replacement");
        assert!(!merged.stale, "no longer queued");
    }

    /// A stale entry registers, but on a payload that may describe older bytes,
    /// so its sessions fail to open and its frames pass through unrendered. When
    /// another spelling of the same file holds a sound entry, that one must be
    /// used instead of stopping at the stale direct hit.
    #[cfg(windows)]
    #[test]
    fn a_stale_direct_hit_still_looks_for_a_sound_alias() {
        let root = temp_root("stale-vs-sound");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);
        let walked = real.join("foo.aex");
        let other = root.join("link").join("foo.aex").to_string_lossy().into_owned();

        let mut stale = discovered(5, 64, build(1));
        stale.stale = true;
        stale.sha = "older-bytes".into();
        let mut sound = discovered(5, 64, build(1));
        sound.sha = "current".into();

        let mut cache = HashMap::new();
        cache.insert(walked.to_string_lossy().into_owned(), stale);
        cache.insert(other.clone(), sound);

        let mut aliases = None;
        let (entry, alias) = resolve_cached(
            &cache,
            &walked.to_string_lossy(),
            &walked,
            META,
            build(1),
            std::slice::from_ref(&root),
            true,
            &mut aliases,
        );
        assert_eq!(entry.map(|entry| entry.sha.as_str()), Some("current"));
        assert_eq!(alias.as_deref(), Some(other.as_str()));
    }

    /// But a stale direct hit is kept when no sounder spelling exists: dropping
    /// it would unregister the effect (#307).
    #[cfg(windows)]
    #[test]
    fn a_stale_direct_hit_is_kept_when_no_alias_is_sounder() {
        let root = temp_root("stale-only");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);
        let walked = real.join("foo.aex");

        let mut stale = discovered(5, 64, build(1));
        stale.stale = true;
        stale.sha = "older-bytes".into();
        let mut also_stale = discovered(5, 64, build(1));
        also_stale.stale = true;
        also_stale.sha = "other-older".into();

        let mut cache = HashMap::new();
        cache.insert(walked.to_string_lossy().into_owned(), stale);
        cache.insert(
            root.join("link").join("foo.aex").to_string_lossy().into_owned(),
            also_stale,
        );

        let mut aliases = None;
        let (entry, alias) = resolve_cached(
            &cache,
            &walked.to_string_lossy(),
            &walked,
            META,
            build(1),
            std::slice::from_ref(&root),
            true,
            &mut aliases,
        );
        assert_eq!(
            entry.map(|entry| entry.sha.as_str()),
            Some("older-bytes"),
            "kept the walked spelling, still registered"
        );
        assert_eq!(alias, None, "no lateral move");
    }

    /// `alias_rank` cannot see whether an entry still describes the file, so a
    /// top-ranked direct hit can still fail to register. Choosing only strictly
    /// sounder candidates would then skip an equally ranked but usable spelling
    /// and leave the effect unregistered (#307).
    #[cfg(windows)]
    #[test]
    fn an_unusable_top_ranked_direct_hit_adopts_an_equal_ranked_alias() {
        let root = temp_root("equal-rank-adopt");
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("foo.aex"), b"x").unwrap();
        junction(&root.join("link"), &real);
        let walked = real.join("foo.aex");
        let other = root.join("link").join("foo.aex").to_string_lossy().into_owned();

        let mut cache = HashMap::new();
        // Same rank as the alias (ok, not stale, current build) but its meta does
        // not match the file, so it cannot be registered.
        cache.insert(
            walked.to_string_lossy().into_owned(),
            discovered(9, 99, build(1)),
        );
        cache.insert(other.clone(), discovered(5, 64, build(1)));

        let mut aliases = None;
        let (entry, alias) = resolve_cached(
            &cache,
            &walked.to_string_lossy(),
            &walked,
            META,
            build(1),
            std::slice::from_ref(&root),
            true,
            &mut aliases,
        );
        assert_eq!(entry.map(|entry| entry.len), Some(64), "took the usable one");
        assert_eq!(alias.as_deref(), Some(other.as_str()));
    }

    fn parameter(slot: u32, name: &str, visible: bool) -> InteractiveParameter {
        InteractiveParameter {
            slot,
            name: name.into(),
            kind: "float".into(),
            minimum: 0.0,
            maximum: 1.0,
            value: 0.0,
            choices: Vec::new(),
            color: [255, 0, 0, 0],
            components: [0.0; 3],
            component_count: 1,
            layer_path: None,
            enabled: true,
            visible,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }
    }

    #[test]
    fn unique_item_names_preserve_unique_labels_and_bind_duplicates_to_slots() {
        let parameters = vec![
            parameter(1, "Intensity", true),
            parameter(2, "Intensity", true),
            parameter(3, "", true),
            parameter(4, " ", true),
            parameter(5, "Unique", true),
            parameter(6, "Hidden", false),
        ];
        let names = unique_item_names(&parameters);
        let visible: Vec<&str> = names.iter().filter_map(Option::as_deref).collect();

        assert_eq!(visible[0], "Intensity [slot 1]");
        assert_eq!(visible[1], "Intensity [slot 2]");
        assert_eq!(visible[2], "Parameter 3 [slot 3]");
        assert_eq!(visible[3], "Parameter 4 [slot 4]");
        assert_eq!(visible[4], "Unique");
        assert!(
            names[5].is_none(),
            "invisible parameters do not consume item names"
        );
        assert_eq!(visible.len(), visible.iter().collect::<HashSet<_>>().len());
    }

    #[test]
    fn generated_slot_name_collision_gets_a_second_stable_suffix() {
        let parameters = vec![
            parameter(1, "Intensity", true),
            parameter(2, "Intensity", true),
            parameter(3, "Intensity [slot 1]", true),
        ];
        let names = unique_item_names(&parameters);

        assert_eq!(names[0].as_deref(), Some("Intensity [slot 1]"));
        assert_eq!(names[1].as_deref(), Some("Intensity [slot 2]"));
        assert_eq!(names[2].as_deref(), Some("Intensity [slot 1] [2]"));
        assert_eq!(
            names
                .iter()
                .filter_map(Option::as_ref)
                .collect::<HashSet<_>>()
                .len(),
            3
        );
    }
}
