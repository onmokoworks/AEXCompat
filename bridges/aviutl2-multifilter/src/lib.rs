//! AviUtl2 generic (`.aux2`) plugin: register each AEX in a folder as its own
//! keyframeable filter (issue #295).
//!
//! Increment 1 (this file): validate that a **libffi closure** works as a
//! `FILTER_PLUGIN_TABLE::func_proc_video`. AviUtl2 passes no per-filter context
//! to that callback and exposes no `effect_id -> filter` mapping, so N distinct
//! filters need N distinct C function pointers. libffi mints one C callback per
//! filter at runtime (unbounded N), each carrying its own userdata. This
//! increment registers two filters (tint red / tint green), each driven by a
//! libffi closure capturing its channel, to prove the mechanism end to end
//! before wiring folder-scan + AEX discovery + RenderSession.
//!
//! Deployed as `.aux2` (generic plugin extension); `.auf2` would make AviUtl2
//! look for the single-filter `GetFilterPluginTable` export and fail.

use std::ffi::c_void;

use aviutl2_sys::filter2::{FILTER_PLUGIN_TABLE, FILTER_PROC_VIDEO, OBJECT_INFO, PIXEL_RGBA};
use aviutl2_sys::plugin2::{COMMON_PLUGIN_TABLE, HOST_APP_TABLE};
use libffi::low;
use libffi::middle::{Cif, Closure, Type};

/// AviUtl2 minimum supported version (matches the aviutl2 crate constant).
const REQUIRED_VERSION: u32 = 2010100;

/// A null-terminated UTF-16 string leaked for AviUtl2's lifetime (LPCWSTR).
fn wide_leak(text: &str) -> *const u16 {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.push(0);
    let boxed = units.into_boxed_slice();
    Box::leak(boxed).as_ptr()
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
    let table = Box::new(COMMON_PLUGIN_TABLE {
        name: wide_leak("AEX multi-filter (libffi spike)"),
        information: wide_leak(
            "Registers per-channel tint filters via libffi closures (issue #295)",
        ),
    });
    Box::leak(table)
}

#[unsafe(no_mangle)]
pub extern "C" fn RegisterPlugin(host: *mut HOST_APP_TABLE) {
    if host.is_null() {
        return;
    }
    register_filter(host, "Spike libffi Tint R", 0);
    register_filter(host, "Spike libffi Tint G", 1);
}

// --- Per-filter registration via a libffi closure ------------------------

/// Per-filter userdata carried by the libffi closure (its C callback receives
/// only the FILTER_PROC_VIDEO pointer; identity comes from here).
struct FilterCtx {
    channel: usize,
}

/// The shared libffi callback body. libffi passes the closure's userdata plus
/// the raw C args, so one Rust function serves every filter; the channel comes
/// from `userdata`. Signature is libffi's classic closure callback form.
unsafe extern "C" fn tint_callback(
    _cif: &low::ffi_cif,
    result: &mut u8,
    args: *const *const c_void,
    userdata: &FilterCtx,
) {
    // args[0] points to storage holding the single `*mut FILTER_PROC_VIDEO` arg.
    let video = unsafe { *(*args as *const *mut FILTER_PROC_VIDEO) };
    *result = tint(video, userdata.channel, 80) as u8;
}

fn register_filter(host: *mut HOST_APP_TABLE, name: &str, channel: usize) {
    // One CIF for the func_proc_video ABI: (pointer) -> u8 (C++ bool is 1 byte).
    let cif = Cif::new([Type::pointer()], Type::u8());
    let userdata = Box::leak(Box::new(FilterCtx { channel }));
    let closure = Box::leak(Box::new(Closure::new(cif, tint_callback, userdata)));
    // The code pointer is `unsafe extern "C" fn()`; reinterpret as the actual
    // func_proc_video ABI (matched by the CIF above).
    let code: unsafe extern "C" fn() = *closure.code_ptr();
    let func_proc_video: extern "C" fn(*mut FILTER_PROC_VIDEO) -> bool =
        unsafe { std::mem::transmute(code) };

    // A filter with no config items: a single NULL terminator for the item list.
    let items: &'static [*const c_void] = Box::leak(Box::new([std::ptr::null()]));

    let table = Box::leak(Box::new(FILTER_PLUGIN_TABLE {
        // FLAG_VIDEO (1) | FLAG_FILTER (8): a filter-object video filter.
        flag: 1 | 8,
        name: wide_leak(name),
        label: std::ptr::null(),
        information: wide_leak(name),
        items: items.as_ptr(),
        func_proc_video: Some(func_proc_video),
        func_proc_audio: None,
    }));

    unsafe { ((*host).register_filter_plugin)(table) };
}

/// Add `add` to one channel (0 = red, 1 = green) of every pixel, saturating.
fn tint(video: *mut FILTER_PROC_VIDEO, channel: usize, add: u8) -> bool {
    if video.is_null() {
        return false;
    }
    let object: *const OBJECT_INFO = unsafe { (*video).object };
    if object.is_null() {
        return false;
    }
    let width = unsafe { (*object).width };
    let height = unsafe { (*object).height };
    if width <= 0 || height <= 0 {
        return true;
    }
    let count = (width as usize) * (height as usize);
    let mut pixels: Vec<PIXEL_RGBA> = (0..count)
        .map(|_| PIXEL_RGBA {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        })
        .collect();
    unsafe { ((*video).get_image_data)(pixels.as_mut_ptr()) };
    for pixel in &mut pixels {
        let component = if channel == 0 {
            &mut pixel.r
        } else {
            &mut pixel.g
        };
        *component = component.saturating_add(add);
    }
    unsafe { ((*video).set_image_data)(pixels.as_ptr(), width, height) };
    true
}
