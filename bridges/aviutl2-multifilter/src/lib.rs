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
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// Folder scanned for `*.aex`.
    dir: Option<PathBuf>,
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

#[unsafe(no_mangle)]
pub extern "C" fn RegisterPlugin(host: *mut HOST_APP_TABLE) {
    if host.is_null() {
        return;
    }
    let config = load_config();
    // Env vars override the TOML values (backward compatible; useful for tests).
    let dir = std::env::var_os(ENV_DIR)
        .map(PathBuf::from)
        .or(config.dir);
    let repository = std::env::var_os(ENV_REPOSITORY)
        .map(PathBuf::from)
        .or(config.repository);
    let (Some(dir), Some(repository)) = (dir, repository) else {
        return;
    };

    let mut entries: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(read) => read
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("aex"))
            })
            .filter(|path| !is_ignored(path, &config.ignore))
            .collect(),
        Err(_) => return,
    };
    // Deterministic registration order (read_dir order is unspecified).
    entries.sort();
    for plugin in entries {
        register_aex(host, &repository, &plugin);
    }
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

fn register_aex(host: *mut HOST_APP_TABLE, repository: &Path, plugin: &Path) {
    let Ok(bytes) = std::fs::read(plugin) else {
        return;
    };
    let sha = hex_lower(&Sha256::digest(&bytes));
    let Ok((params, diagnostics)) =
        inspect_experimental_with_diagnostics(repository, plugin, &sha)
    else {
        return;
    };
    // PF_OutFlag2_SUPPORTS_SMART_RENDER = bit 10.
    let smart = diagnostics
        .get("advertised_out_flags2")
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
        & (1 << 10)
        != 0;

    // Build config items + readers + normalized defaults from the exposed params.
    let mut items: Vec<*const c_void> = Vec::new();
    let mut readers: Vec<ItemReader> = Vec::new();
    let mut defaults: Vec<InteractiveParameter> = Vec::new();
    for parameter in &params {
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
        sha,
        smart,
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
