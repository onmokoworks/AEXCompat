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

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

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
/// Folder scanned at load for `*.aex`; each becomes its own filter.
const ENV_DIR: &str = "AEXCOMPAT_MULTIFILTER_DIR";
/// Repo root holding the built workers (`target/minihost-build/`).
const ENV_REPOSITORY: &str = "AEXCOMPAT_MULTIFILTER_REPOSITORY";
/// Session dimension bounds (mirrors the broker's limits).
const MAX_DIMENSION: u32 = 16_384;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

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
pub extern "C" fn UninitializePlugin() {}

#[unsafe(no_mangle)]
pub extern "C" fn GetCommonPluginTable() -> *mut COMMON_PLUGIN_TABLE {
    Box::leak(Box::new(COMMON_PLUGIN_TABLE {
        name: wide_leak("AEXCompat multi-filter"),
        information: wide_leak(
            "Registers each AEX in AEXCOMPAT_MULTIFILTER_DIR as its own keyframeable filter (issue #295)",
        ),
    }))
}

#[unsafe(no_mangle)]
pub extern "C" fn RegisterPlugin(host: *mut HOST_APP_TABLE) {
    if host.is_null() {
        return;
    }
    let Some(dir) = std::env::var_os(ENV_DIR).map(PathBuf::from) else {
        return;
    };
    let Some(repository) = std::env::var_os(ENV_REPOSITORY).map(PathBuf::from) else {
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

/// A live session pinned to one geometry/time; reopened when the object changes.
struct SessionSlot {
    session: RenderSession,
    width: u32,
    height: u32,
    time_step: i32,
    total_time: i32,
    time_scale: u32,
    /// Monotonic transport serial. The worker rejects a non-advancing
    /// `frame_index`, so this must strictly increase for the session's life
    /// regardless of the AE frame being (re-)rendered (paused redraws, backward
    /// scrubs, parameter edits all re-render the same AE frame). AE time still
    /// rides `current_time`.
    frame_serial: u32,
}

/// Per-filter userdata carried by the libffi closure.
struct FilterCtx {
    repository: PathBuf,
    plugin: PathBuf,
    sha: String,
    smart: bool,
    /// Exposed parameter defaults (normalized), cloned per frame as the baseline.
    defaults: Vec<InteractiveParameter>,
    /// Readers pulling each frame's current config value into the parameters.
    readers: Vec<ItemReader>,
    /// The live session, reopened on geometry/time change. Mutex serializes the
    /// blocking worker round-trip across concurrent proc_video calls.
    session: Mutex<Option<SessionSlot>>,
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
        session: Mutex::new(None),
    }));

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

    let mut guard = match ctx.session.lock() {
        Ok(guard) => guard,
        Err(_) => return false,
    };
    // Reuse a session matching this geometry/time, else (re)open.
    let matches = guard.as_ref().is_some_and(|slot| {
        slot.width == width
            && slot.height == height
            && slot.time_step == time_step
            && slot.total_time == total_time
            && slot.time_scale == time_scale
    });
    if !matches {
        *guard = None; // drop the stale session before opening a new one
        match open_session(ctx, width, height, time_step, total_time, time_scale) {
            Ok(slot) => *guard = Some(slot),
            Err(_) => return false,
        }
    }
    let slot = guard.as_mut().expect("session set above");
    let serial = slot.frame_serial;

    match slot.session.render_frame_with_parameters(
        serial,
        current_time,
        &rgba,
        parameters.as_deref(),
    ) {
        Ok(outcome) => {
            // Advance the transport serial only on a delivered frame, so the next
            // render (even of the same AE frame) is accepted as advancing.
            slot.frame_serial = serial.wrapping_add(1);
            match outcome.status {
                FrameStatus::Rendered { pixels, width: out_w, height: out_h, .. } => {
                    // A filter object cannot change the image size.
                    if out_w != width || out_h != height {
                        return true; // leave pixels unchanged
                    }
                    let out = bytes_to_pixels(&pixels);
                    if out.len() == count {
                        unsafe {
                            ((*video).set_image_data)(out.as_ptr(), width as i32, height as i32)
                        };
                    }
                    true
                }
                FrameStatus::FrameError { .. } => true, // keep session, leave pixels
            }
        }
        Err(_) => {
            *guard = None; // invalidate on transport failure
            false
        }
    }
}

fn open_session(
    ctx: &FilterCtx,
    width: u32,
    height: u32,
    time_step: i32,
    total_time: i32,
    time_scale: u32,
) -> Result<SessionSlot, ()> {
    let session = RenderSession::open(SessionOpenRequest {
        repository: &ctx.repository,
        plugin_path: &ctx.plugin,
        plugin_sha256: &ctx.sha,
        parameters: (!ctx.defaults.is_empty()).then_some(&ctx.defaults[..]),
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
        width,
        height,
        pixel_format: RenderPixelFormat::Argb8,
        time_step,
        total_time,
        time_scale,
        frame_deadline: Duration::from_millis(30_000),
        smart: ctx.smart,
        gpu_backend: RenderGpuBackend::Auto,
        gpu_runtime_policy: None,
    })
    .map_err(|_| ())?;
    Ok(SessionSlot {
        session,
        width,
        height,
        time_step,
        total_time,
        time_scale,
        frame_serial: 0,
    })
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
