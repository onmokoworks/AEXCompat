//! Experimental native x86_64 carrier.
//!
//! This module is compiled only for the separate x86_64 macOS worker. It maps
//! the unchanged PE image and calls it with the Win64 ABI, while retaining the
//! same Classic host boundary used by the Unicorn worker. Plug-ins enter
//! through the shared PE/ABI path: no plug-in identity, RVA, parameter name,
//! or effect algorithm appears here.

#![cfg(all(
    feature = "native-carrier",
    target_arch = "x86_64",
    target_os = "macos"
))]
#![allow(unsafe_code)]

use aex_abi::x86_64_windows as abi;
use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::{c_int, c_void};
use std::marker::PhantomData;
use std::ptr;
use thiserror::Error;

use crate::crt_heap::CrtHeap;
#[cfg(test)]
use crate::native_aegp_memory::active_arena_next;
use crate::native_aegp_memory::{
    NativeAegpMemory, native_aegp_memory_callbacks, with_native_aegp_memory_context,
};
#[cfg(test)]
use crate::native_aegp_memory::{
    free_aegp_mem_handle, get_aegp_mem_handle_size, lock_aegp_mem_handle, new_aegp_mem_handle,
    resize_aegp_mem_handle, unlock_aegp_mem_handle, unsupported_aegp_memory_slot,
};
use crate::pe::PeImage;
use crate::plugin_data::{
    CALLBACK_REJECTED, EffectRegistry, RegistrationPointers, decode_registration,
};
pub use crate::x64::{
    ExecutionTrace, GuestCensus, GuestParam, TraceStateValue, TraceWatchSpec, UnsupportedSuiteCall,
};
use crate::x64::{record_suite_request, record_unsupported_suite_call, utility_suite_layout};

const ARENA_SIZE: usize = 256 * 1024 * 1024;
#[cfg(test)]
const MAX_AEGP_MEMORY_HANDLES: usize = 256;
const PAGE_SIZE: usize = 4096;
const MAX_PF_HANDLE_SIZE: u64 = 2 * 1024 * 1024 * 1024;
const MAX_PF_HANDLE_COUNT: usize = 16_384;
const MAX_WORLD_SIZE: u64 = 128 * 1024 * 1024;
const MAX_WORLD_COUNT: usize = 256;
const PROT_READ: c_int = 0x1;
const PROT_WRITE: c_int = 0x2;
const PROT_EXEC: c_int = 0x4;
const MAP_PRIVATE: c_int = 0x0002;
const MAP_ANON: c_int = 0x1000;
const PF_INVALID_INDEX: u64 = 513;
const PF_UNRECOGNIZED_PARAM_TYPE: u64 = 514;
const PF_BAD_CALLBACK_PARAM: u64 = 516;
const PARAM_TYPE_COLOR: i32 = 5;
#[cfg(test)]
const PARAM_TYPE_POINT: i32 = 6;
const HOST_EFFECT_REF: u64 = 1;

macro_rules! callback_address {
    ($callback:expr) => {
        $callback as *const () as usize as u64
    };
}

unsafe extern "C" {
    fn mmap(
        address: *mut c_void,
        length: usize,
        protection: c_int,
        flags: c_int,
        fd: c_int,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(address: *mut c_void, length: usize) -> c_int;
    fn mprotect(address: *mut c_void, length: usize, protection: c_int) -> c_int;
    fn pthread_self() -> *mut c_void;
    fn pthread_get_stackaddr_np(thread: *mut c_void) -> *mut c_void;
    fn pthread_get_stacksize_np(thread: *mut c_void) -> usize;
}

#[derive(Debug, Error)]
pub enum GuestError {
    #[error("native carrier mapping failed: {0}")]
    Mapping(String),
    #[error("native carrier callback failed: {0}")]
    Callback(String),
    #[error("native carrier data arena exhausted")]
    DataCapacity,
    #[error("DLL process attach returned FALSE")]
    DllProcessAttach,
    #[error("guest census is unavailable in the native-speed oracle")]
    CensusUnavailable,
    #[error("detailed execution tracing is available only in the Unicorn worker")]
    TraceUnavailable,
}

impl GuestError {
    pub fn diagnostic_category(&self) -> &'static str {
        match self {
            Self::Mapping(_) => "mapping",
            Self::Callback(_) => "callback",
            Self::DataCapacity => "memory",
            Self::DllProcessAttach => "dllmain",
            Self::CensusUnavailable | Self::TraceUnavailable => "capability",
        }
    }

    pub fn diagnostic_message(&self) -> String {
        self.to_string()
    }

    pub fn crash_reason(&self) -> Option<&str> {
        None
    }
}

#[derive(Clone, Debug)]
struct NativeHandle {
    data: u64,
    size: u64,
    locks: u32,
    data_mapping_size: usize,
}

#[derive(Clone, Debug)]
struct NativeWorld {
    pixel_format: i32,
    size: u64,
    data: u64,
    mapping_size: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct ColorParamPixelFloat {
    alpha: f32,
    red: f32,
    green: f32,
    blue: f32,
}

#[derive(Default)]
struct NativeState {
    params: Vec<GuestParam>,
    callback_error: Option<String>,
    smart_input_world: u64,
    smart_output_world: u64,
    smart_width: u32,
    smart_height: u32,
    smart_pixel_format: i32,
    suite_requests: Vec<String>,
    unsupported_suite_calls: Vec<UnsupportedSuiteCall>,
    dropped_unsupported_suite_calls: u64,
    utility_suites: HashMap<u32, u64>,
    iterate8_suite: u64,
    pre_checkout_calls: u32,
    pre_checkout_requests: Vec<[i32; 4]>,
    checkout_pixels_calls: u32,
    checkout_output_calls: u32,
    parameter_definitions: Vec<u64>,
    handles: HashMap<u64, NativeHandle>,
    worlds: HashMap<u64, NativeWorld>,
    aegp_memory: NativeAegpMemory,
    handle_allocations: Vec<u64>,
    arena_next: u64,
    arena_end: u64,
    handle_suite: u64,
    aegp_memory_suite: u64,
    color_param_suite: u64,
    point_param_suite: u64,
    world_suite: u64,
    image_start: u64,
    image_end: u64,
    plugin_data_registry: EffectRegistry,
    crt_heap: CrtHeap,
}

thread_local! {
    static ACTIVE_STATE: Cell<*mut NativeState> = const { Cell::new(ptr::null_mut()) };
}

fn with_state<T>(operation: impl FnOnce(&mut NativeState) -> T) -> Option<T> {
    ACTIVE_STATE.with(|slot| {
        let pointer = slot.get();
        if pointer.is_null() {
            None
        } else {
            // The pointer is installed only for the synchronous duration of
            // GuestEngine::call_win64 on this worker thread.
            Some(unsafe { operation(&mut *pointer) })
        }
    })
}

struct Mapping {
    pointer: *mut u8,
    size: usize,
}

impl Mapping {
    fn anonymous(
        preferred: *mut c_void,
        size: usize,
        executable: bool,
    ) -> Result<Self, GuestError> {
        let protection = PROT_READ | PROT_WRITE | if executable { PROT_EXEC } else { 0 };
        let pointer = unsafe { mmap(preferred, size, protection, MAP_PRIVATE | MAP_ANON, -1, 0) };
        if pointer as isize == -1 {
            return Err(GuestError::Mapping("mmap returned MAP_FAILED".into()));
        }
        if !preferred.is_null() && pointer != preferred {
            unsafe {
                munmap(pointer, size);
            }
            return Err(GuestError::Mapping(format!(
                "preferred PE base {preferred:p} was unavailable (mapped {pointer:p})"
            )));
        }
        Ok(Self {
            pointer: pointer.cast(),
            size,
        })
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            munmap(self.pointer.cast(), self.size);
        }
    }
}

pub struct GuestEngine<'a> {
    image: Mapping,
    arena: Mapping,
    state: NativeState,
    dllmain_attached: bool,
    lifetime: PhantomData<&'a ()>,
}

impl Drop for GuestEngine<'_> {
    fn drop(&mut self) {
        for (handle, record) in self.state.handles.drain() {
            unsafe {
                munmap(record.data as *mut c_void, record.data_mapping_size);
                munmap(handle as *mut c_void, PAGE_SIZE);
            }
        }
        for record in self.state.worlds.drain().map(|(_, record)| record) {
            unsafe {
                munmap(record.data as *mut c_void, record.mapping_size);
            }
        }
        for (pointer, allocation) in self.state.crt_heap.allocations() {
            unsafe {
                munmap(pointer as *mut c_void, allocation.backing_size as usize);
            }
        }
    }
}

type Win64Function = unsafe extern "win64" fn(u64, u64, u64, u64, u64, u64) -> u64;

fn native_world_callbacks() -> [u64; 3] {
    [
        callback_address!(new_world),
        callback_address!(dispose_world),
        callback_address!(get_world_pixel_format),
    ]
}

impl GuestEngine<'static> {
    pub fn backend_name(&self) -> &'static str {
        "native-x86_64-carrier"
    }

    pub fn load(image: &PeImage) -> Result<Self, GuestError> {
        let image_size = image.mapped_bytes().len();
        let image_mapping =
            Mapping::anonymous(image.image_base() as *mut c_void, image_size, false)?;
        unsafe {
            ptr::copy_nonoverlapping(
                image.mapped_bytes().as_ptr(),
                image_mapping.pointer,
                image_size,
            );
        }
        let arena = Mapping::anonymous(ptr::null_mut(), ARENA_SIZE, false)?;
        let arena_base = arena.pointer as u64;
        let mut engine = Self {
            image: image_mapping,
            arena,
            state: NativeState {
                arena_next: arena_base,
                arena_end: arena_base + ARENA_SIZE as u64,
                image_start: image.image_base(),
                image_end: image.image_base() + image_size as u64,
                ..NativeState::default()
            },
            dllmain_attached: image.dll_entry_address().is_none(),
            lifetime: PhantomData,
        };
        engine.install_imports(image)?;
        engine.seal_image(image)?;
        let handle_suite = engine.allocate(48, 8)?;
        let callbacks = [
            callback_address!(new_handle),
            callback_address!(lock_handle),
            callback_address!(unlock_handle),
            callback_address!(dispose_handle),
            callback_address!(handle_size),
            callback_address!(resize_handle),
        ];
        for (index, callback) in callbacks.into_iter().enumerate() {
            engine.write_u64(handle_suite + (index * 8) as u64, callback)?;
        }
        engine.state.handle_suite = handle_suite;
        let aegp_memory_suite = engine.allocate(64, 8)?;
        for (index, callback) in native_aegp_memory_callbacks().into_iter().enumerate() {
            engine.write_u64(aegp_memory_suite + (index * 8) as u64, callback)?;
        }
        engine.state.aegp_memory_suite = aegp_memory_suite;
        let iterate8_suite = engine.allocate(5 * 8, 8)?;
        for (slot, callback) in [
            callback_address!(iterate_world8),
            callback_address!(iterate_origin8),
            callback_address!(iterate_lut8),
            callback_address!(iterate_origin_non_clip8),
            callback_address!(iterate_generic),
        ]
        .into_iter()
        .enumerate()
        {
            engine.write_u64(iterate8_suite + (slot * 8) as u64, callback)?;
        }
        engine.state.iterate8_suite = iterate8_suite;
        let color_param_suite = engine.allocate(8, 8)?;
        engine.write_u64(color_param_suite, callback_address!(color_param_value))?;
        engine.state.color_param_suite = color_param_suite;
        let point_param_suite = engine.allocate(8, 8)?;
        engine.write_u64(point_param_suite, callback_address!(point_param_value))?;
        engine.state.point_param_suite = point_param_suite;
        let world_suite = engine.allocate(24, 8)?;
        for (slot, callback) in native_world_callbacks().into_iter().enumerate() {
            engine.write_u64(world_suite + (slot * 8) as u64, callback)?;
        }
        engine.state.world_suite = world_suite;
        for version in [3u32, 7, 11, 13] {
            let callbacks =
                native_utility_callbacks(version).expect("known AEGP Utility Suite version");
            let table = engine.allocate(callbacks.len() * 8, 8)?;
            for (slot, callback) in callbacks.into_iter().enumerate() {
                engine.write_u64(table + (slot * 8) as u64, callback)?;
            }
            engine.state.utility_suites.insert(version, table);
        }
        // Windows CRT process attach is not safe to enter natively until its
        // OS/SEH imports have typed implementations. The native carrier keeps
        // it opt-in; the Unicorn backend remains the exact lifecycle fallback.
        let run_dllmain = std::env::var_os("AEXCOMPAT_NATIVE_RUN_DLLMAIN").is_some();
        if run_dllmain && let Some(entry) = image.dll_entry_address() {
            let attached = engine.call_win64(entry, [image.image_base(), 1, 0, 0, 0, 0])?;
            if attached == 0 {
                return Err(GuestError::DllProcessAttach);
            }
            engine.dllmain_attached = true;
        }
        Ok(engine)
    }

    pub fn resolve_effect_entry(
        &mut self,
        image: &PeImage,
        selector: Option<&str>,
        basic_suite: u64,
    ) -> Result<u64, GuestError> {
        if let Some(entry) = image.entry_address() {
            if selector.is_some() {
                return Err(GuestError::Callback(
                    "effect selection is unavailable when a direct effect entrypoint exists".into(),
                ));
            }
            return Ok(entry);
        }
        if !self.dllmain_attached {
            return Err(GuestError::Callback(
                "PluginData registration requires DLL_PROCESS_ATTACH in the native carrier".into(),
            ));
        }
        let (registration_entry, callback) = if let Some(entry) =
            image.export_address("PluginDataEntryFunction2")
        {
            (entry, callback_address!(plugin_data_callback_v2))
        } else if let Some(entry) = image.export_address("PluginDataEntryFunction") {
            (entry, callback_address!(plugin_data_callback_v1))
        } else {
            return Err(GuestError::Callback(
                    "effect selector requires PluginData registration, but no registration export exists"
                        .into(),
                ));
        };
        self.state.plugin_data_registry = EffectRegistry::default();
        let host_name = self.allocate(10, 1)?;
        self.write(host_name, b"AEXCompat\0")?;
        let host_version = self.allocate(5, 1)?;
        self.write(host_version, b"2025\0")?;
        let returned = self.call_win64(
            registration_entry,
            [1, callback, basic_suite, host_name, host_version, 0],
        )? as i32;
        if returned != 0 {
            return Err(GuestError::Callback(format!(
                "PluginData entrypoint returned {returned}"
            )));
        }
        let registration = self
            .state
            .plugin_data_registry
            .select(selector)
            .map_err(|error| GuestError::Callback(error.to_string()))?
            .clone();
        image
            .export_address(&registration.entrypoint)
            .ok_or_else(|| {
                GuestError::Callback(format!(
                    "registered effect entrypoint {} is not an executable export",
                    registration.entrypoint
                ))
            })
    }

    fn install_imports(&mut self, image: &PeImage) -> Result<(), GuestError> {
        for library in image.imports() {
            for symbol in &library.symbols {
                let callback = native_import_callback(&symbol.name);
                self.write_u64(image.image_base() + symbol.iat_rva as u64, callback)?;
            }
        }
        Ok(())
    }

    fn seal_image(&mut self, image: &PeImage) -> Result<(), GuestError> {
        let page_size = 0x1000usize;
        for section in image.section_protections() {
            let start = section.virtual_address & !(page_size - 1);
            let end = section
                .virtual_address
                .checked_add(section.virtual_size)
                .ok_or_else(|| GuestError::Mapping("section protection overflow".into()))?;
            let end = (end + page_size - 1) & !(page_size - 1);
            let mut protection = PROT_READ;
            if section.writable {
                protection |= PROT_WRITE;
            }
            if section.executable {
                protection |= PROT_EXEC;
            }
            let result = unsafe {
                mprotect(
                    self.image.pointer.add(start).cast(),
                    end - start,
                    protection,
                )
            };
            if result != 0 {
                return Err(GuestError::Mapping(format!(
                    "mprotect failed for PE section at RVA {start:#x}"
                )));
            }
        }
        Ok(())
    }

    pub fn call_win64(&mut self, address: u64, args: [u64; 6]) -> Result<u64, GuestError> {
        self.state.callback_error = None;
        let previous = ACTIVE_STATE.with(|slot| slot.replace(&mut self.state));
        let function: Win64Function = unsafe { std::mem::transmute(address as usize) };
        let result = with_native_aegp_memory_context(
            &mut self.state.aegp_memory,
            &mut self.state.arena_next,
            self.state.arena_end,
            || unsafe { function(args[0], args[1], args[2], args[3], args[4], args[5]) },
        );
        ACTIVE_STATE.with(|slot| slot.set(previous));
        if let Some(error) = self.state.callback_error.take() {
            Err(GuestError::Callback(error))
        } else {
            Ok(result)
        }
    }

    pub fn allocate(&mut self, size: usize, alignment: u64) -> Result<u64, GuestError> {
        let alignment = alignment.max(1).next_power_of_two();
        let start = self
            .state
            .arena_next
            .checked_add(alignment - 1)
            .map(|value| value & !(alignment - 1))
            .ok_or(GuestError::DataCapacity)?;
        let end = start
            .checked_add(size.max(1) as u64)
            .ok_or(GuestError::DataCapacity)?;
        if end > self.state.arena_end {
            return Err(GuestError::DataCapacity);
        }
        self.state.arena_next = end;
        unsafe {
            ptr::write_bytes(start as *mut u8, 0, size);
        }
        Ok(start)
    }

    pub fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), GuestError> {
        if !self.contains_writable(address, bytes.len()) {
            return Err(GuestError::Mapping(format!(
                "write outside mapped image/arena: {address:#x}+{}",
                bytes.len()
            )));
        }
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
        }
        Ok(())
    }

    pub fn read(&self, address: u64, bytes: &mut [u8]) -> Result<(), GuestError> {
        if !self.contains_readable(address, bytes.len()) {
            return Err(GuestError::Mapping(format!(
                "read outside mapped image/arena: {address:#x}+{}",
                bytes.len()
            )));
        }
        unsafe {
            ptr::copy_nonoverlapping(address as *const u8, bytes.as_mut_ptr(), bytes.len());
        }
        Ok(())
    }

    fn contains_readable(&self, address: u64, size: usize) -> bool {
        contains(&self.image, address, size) || contains(&self.arena, address, size)
    }

    fn contains_writable(&self, address: u64, size: usize) -> bool {
        self.contains_readable(address, size)
    }

    pub fn write_u64(&mut self, address: u64, value: u64) -> Result<(), GuestError> {
        self.write(address, &value.to_le_bytes())
    }

    pub fn begin_block_census(&mut self) -> Result<(), GuestError> {
        Err(GuestError::CensusUnavailable)
    }

    pub fn finish_block_census(&mut self, _: u64) -> Result<GuestCensus, GuestError> {
        Err(GuestError::CensusUnavailable)
    }

    pub fn begin_execution_trace(&mut self, _: &str, _: u64) -> Result<(), GuestError> {
        Err(GuestError::TraceUnavailable)
    }

    pub fn finish_execution_trace(&mut self, _: u64) -> Result<ExecutionTrace, GuestError> {
        Err(GuestError::TraceUnavailable)
    }

    pub fn configure_trace_watches(&mut self, _: Vec<TraceWatchSpec>) {}

    pub fn add_trace_watch(&mut self, _: TraceWatchSpec) {}

    pub fn configure_parameter_definitions(
        &mut self,
        definitions: Vec<u64>,
    ) -> Result<(), GuestError> {
        if definitions.len() != self.state.params.len() {
            return Err(GuestError::Callback(
                "active parameter definition count differs from setup".into(),
            ));
        }
        for (definition, parameter) in definitions
            .iter()
            .copied()
            .zip(self.state.params.iter_mut())
        {
            if parameter.param_type == PARAM_TYPE_COLOR {
                let color =
                    unsafe { copy_from_pointer(definition + abi::PARAM_U_OFFSET as u64, 4) };
                parameter.bytes[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_PIXEL_SIZE]
                    .copy_from_slice(&color);
            }
        }
        self.state.parameter_definitions = definitions;
        Ok(())
    }

    pub fn configure_smart_render(
        &mut self,
        input_world: u64,
        output_world: u64,
        width: u32,
        height: u32,
        pixel_format: i32,
    ) {
        self.state.pre_checkout_requests.clear();
        self.state.smart_input_world = input_world;
        self.state.smart_output_world = output_world;
        self.state.smart_width = width;
        self.state.smart_height = height;
        self.state.smart_pixel_format = pixel_format;
    }

    pub fn parameters(&self) -> &[GuestParam] {
        &self.state.params
    }

    pub fn suite_requests(&self) -> &[String] {
        &self.state.suite_requests
    }

    pub fn unsupported_suite_calls(&self) -> &[UnsupportedSuiteCall] {
        &self.state.unsupported_suite_calls
    }

    pub fn dropped_unsupported_suite_calls(&self) -> u64 {
        self.state.dropped_unsupported_suite_calls
    }

    pub fn pre_checkout_requests(&self) -> &[[i32; 4]] {
        &self.state.pre_checkout_requests
    }

    pub fn handle_allocations(&self) -> &[u64] {
        &self.state.handle_allocations
    }

    pub fn smart_callback_counts(&self) -> (u32, u32, u32) {
        (
            self.state.pre_checkout_calls,
            self.state.checkout_pixels_calls,
            self.state.checkout_output_calls,
        )
    }

    pub fn add_param_callback_address(&self) -> u64 {
        callback_address!(add_param)
    }
    pub fn poison_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn ansi_strcpy_callback_address(&self) -> u64 {
        callback_address!(native_strcpy)
    }
    pub fn copy_callback_address(&self) -> u64 {
        callback_address!(copy_world)
    }
    pub fn noop_callback_address(&self) -> u64 {
        callback_address!(noop_import)
    }
    pub fn pre_checkout_layer_callback_address(&self) -> u64 {
        callback_address!(pre_checkout_layer)
    }
    pub fn checkout_layer_pixels_callback_address(&self) -> u64 {
        callback_address!(checkout_layer_pixels)
    }
    pub fn checkin_layer_pixels_callback_address(&self) -> u64 {
        callback_address!(noop_import)
    }
    pub fn checkout_output_callback_address(&self) -> u64 {
        callback_address!(checkout_output)
    }
    pub fn acquire_suite_callback_address(&self) -> u64 {
        callback_address!(acquire_suite)
    }
    pub fn checkout_param_callback_address(&self) -> u64 {
        callback_address!(checkout_param)
    }
    pub fn checkin_param_callback_address(&self) -> u64 {
        callback_address!(noop_import)
    }
    // The extended Inter callbacks are implemented only by the Unicorn
    // backend in this issue. Keep the native carrier buildable without
    // advertising silent success at an unimplemented callback boundary.
    pub fn extended_alloc_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn extended_lookup_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn extended_free_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn iterate8_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn iterate8_origin_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn fill8_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn new_world8_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn dispose_world_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn get_callback_addr_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn ansi_ceil_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn ansi_cos_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn ansi_fabs_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn ansi_pow_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn ansi_sin_callback_address(&self) -> u64 {
        callback_address!(poison_callback)
    }
    pub fn new_handle_callback_address(&self) -> u64 {
        callback_address!(new_handle)
    }
    pub fn lock_handle_callback_address(&self) -> u64 {
        callback_address!(lock_handle)
    }
    pub fn unlock_handle_callback_address(&self) -> u64 {
        callback_address!(unlock_handle)
    }
    pub fn dispose_handle_callback_address(&self) -> u64 {
        callback_address!(dispose_handle)
    }
    pub fn handle_size_callback_address(&self) -> u64 {
        callback_address!(handle_size)
    }
    pub fn resize_handle_callback_address(&self) -> u64 {
        callback_address!(resize_handle)
    }
}

fn contains(mapping: &Mapping, address: u64, size: usize) -> bool {
    let start = mapping.pointer as u64;
    address >= start
        && address
            .checked_add(size as u64)
            .is_some_and(|end| end <= start + mapping.size as u64)
}

unsafe fn copy_from_pointer(pointer: u64, size: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; size];
    unsafe {
        ptr::copy_nonoverlapping(pointer as *const u8, bytes.as_mut_ptr(), size);
    }
    bytes
}

unsafe fn write_pointer(pointer: u64, bytes: &[u8]) {
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), pointer as *mut u8, bytes.len());
    }
}

unsafe extern "win64" fn noop_import(_: u64, _: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    0
}

fn capture_native_plugin_data(context: u64, pointers: RegistrationPointers) -> u64 {
    if context != 1 {
        with_state(|state| {
            state.callback_error = Some(format!(
                "PluginData callback context {context:#x} is invalid"
            ))
        });
        return CALLBACK_REJECTED as u32 as u64;
    }
    with_state(|state| {
        let decoded = decode_registration(pointers, |address| {
            native_guest_range_valid(state, address, 1).then(|| unsafe { *(address as *const u8) })
        });
        match decoded.and_then(|registration| state.plugin_data_registry.push(registration)) {
            Ok(()) => 0,
            Err(error) => {
                state.callback_error = Some(format!("PluginData registration rejected: {error}"));
                CALLBACK_REJECTED as u32 as u64
            }
        }
    })
    .unwrap_or(CALLBACK_REJECTED as u32 as u64)
}

#[allow(clippy::too_many_arguments)]
unsafe extern "win64" fn plugin_data_callback_v2(
    context: u64,
    name: u64,
    match_name: u64,
    category: u64,
    entrypoint: u64,
    kind: u64,
    api_major: u64,
    api_minor: u64,
    reserved_info: u64,
    support_url: u64,
) -> u64 {
    capture_native_plugin_data(
        context,
        RegistrationPointers {
            name,
            match_name,
            category,
            entrypoint,
            kind: kind as u32 as i32,
            api_major: api_major as u32 as i32,
            api_minor: api_minor as u32 as i32,
            reserved_info: reserved_info as u32 as i32,
            support_url: Some(support_url),
        },
    )
}

#[allow(clippy::too_many_arguments)]
unsafe extern "win64" fn plugin_data_callback_v1(
    context: u64,
    name: u64,
    match_name: u64,
    category: u64,
    entrypoint: u64,
    kind: u64,
    api_major: u64,
    api_minor: u64,
    reserved_info: u64,
) -> u64 {
    capture_native_plugin_data(
        context,
        RegistrationPointers {
            name,
            match_name,
            category,
            entrypoint,
            kind: kind as u32 as i32,
            api_major: api_major as u32 as i32,
            api_minor: api_minor as u32 as i32,
            reserved_info: reserved_info as u32 as i32,
            support_url: None,
        },
    )
}

unsafe extern "win64" fn native_omp_get_max_threads(
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    1
}

fn native_import_callback(name: &str) -> u64 {
    match name {
        "malloc" => callback_address!(native_crt_malloc),
        "calloc" => callback_address!(native_crt_calloc),
        "free" => callback_address!(native_crt_free),
        "_callnewh" => callback_address!(noop_import),
        "strncpy" => callback_address!(native_strncpy),
        "memset" => callback_address!(native_memset),
        "expf" => callback_address!(native_expf),
        "floorf" => callback_address!(native_floorf),
        "powf" => callback_address!(native_powf),
        "pow" => callback_address!(native_pow),
        "omp_get_max_threads" => callback_address!(native_omp_get_max_threads),
        _ => callback_address!(noop_import),
    }
}

unsafe extern "win64" fn native_crt_malloc(
    size: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    native_crt_allocate(size)
}

unsafe extern "win64" fn native_crt_calloc(
    count: u64,
    element_size: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    CrtHeap::checked_calloc_size(count, element_size)
        .map(native_crt_allocate)
        .unwrap_or(0)
}

fn native_crt_allocate(requested_size: u64) -> u64 {
    with_state(|state| {
        let allocation = match state.crt_heap.prepare_allocation(requested_size) {
            Ok(allocation) => allocation,
            Err(_) => return 0,
        };
        let pointer = unsafe {
            mmap(
                ptr::null_mut(),
                allocation.backing_size as usize,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANON,
                -1,
                0,
            )
        };
        if pointer as usize == usize::MAX {
            return 0;
        }
        let address = pointer as u64;
        if let Err(error) = state.crt_heap.insert(address, allocation) {
            unsafe {
                munmap(pointer, allocation.backing_size as usize);
            }
            state.callback_error = Some(error.to_string());
            return 0;
        }
        address
    })
    .unwrap_or(0)
}

unsafe extern "win64" fn native_crt_free(
    pointer: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    if pointer == 0 {
        return 0;
    }
    with_state(|state| match state.crt_heap.remove(pointer) {
        Ok(allocation) => {
            if unsafe { munmap(pointer as *mut c_void, allocation.backing_size as usize) } != 0 {
                state.callback_error = Some(format!("unmap CRT allocation {pointer:#x} failed"));
            }
        }
        Err(error) => state.callback_error = Some(error.to_string()),
    });
    0
}

unsafe extern "win64" fn poison_callback(_: u64, _: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    with_state(|state| state.callback_error = Some("unsupported host callback".into()));
    u32::MAX as u64
}

unsafe extern "win64" fn native_memset(
    destination: u64,
    value: u64,
    length: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    if length <= ARENA_SIZE as u64 {
        unsafe {
            ptr::write_bytes(destination as *mut u8, value as u8, length as usize);
        }
        destination
    } else {
        0
    }
}

unsafe extern "win64" fn native_strncpy(
    destination: u64,
    source: u64,
    count: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    if count > 4096 {
        return 0;
    }
    let mut terminated = false;
    for index in 0..count {
        let byte = if terminated {
            0
        } else {
            let value = unsafe { *((source + index) as *const u8) };
            terminated = value == 0;
            value
        };
        unsafe {
            *((destination + index) as *mut u8) = byte;
        }
    }
    destination
}

unsafe extern "win64" fn native_strcpy(
    destination: u64,
    source: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    for index in 0..4096u64 {
        let byte = unsafe { *((source + index) as *const u8) };
        unsafe {
            *((destination + index) as *mut u8) = byte;
        }
        if byte == 0 {
            return destination;
        }
    }
    0
}

unsafe extern "win64" fn native_expf(value: f32) -> f32 {
    value.exp()
}
unsafe extern "win64" fn native_floorf(value: f32) -> f32 {
    value.floor()
}
unsafe extern "win64" fn native_powf(left: f32, right: f32) -> f32 {
    left.powf(right)
}
unsafe extern "win64" fn native_pow(left: f64, right: f64) -> f64 {
    left.powf(right)
}

unsafe extern "win64" fn add_param(
    _: u64,
    index: u64,
    definition: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    let bytes = unsafe { copy_from_pointer(definition, abi::PF_PARAM_DEF_SIZE) };
    let param_type = i32::from_le_bytes(
        bytes[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
            .try_into()
            .expect("generated field is four bytes"),
    );
    let name_bytes = &bytes[abi::PARAM_NAME_OFFSET..abi::PARAM_NAME_OFFSET + abi::PARAM_NAME_SIZE];
    let name_end = name_bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(name_bytes.len());
    let name = String::from_utf8_lossy(&name_bytes[..name_end]).into_owned();
    with_state(|state| {
        state.params.push(GuestParam {
            index: index as i32,
            param_type,
            name,
            bytes,
        });
    });
    0
}

unsafe extern "win64" fn copy_world(
    _: u64,
    source: u64,
    destination: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    let read_u64 = |base: u64, offset: usize| unsafe { *((base + offset as u64) as *const u64) };
    let read_i32 = |base: u64, offset: usize| unsafe { *((base + offset as u64) as *const i32) };
    let source_data = read_u64(source, abi::LAYER_DATA_OFFSET);
    let destination_data = read_u64(destination, abi::LAYER_DATA_OFFSET);
    let source_rowbytes = read_i32(source, abi::LAYER_ROWBYTES_OFFSET).max(0) as usize;
    let destination_rowbytes = read_i32(destination, abi::LAYER_ROWBYTES_OFFSET).max(0) as usize;
    let height = read_i32(source, abi::LAYER_HEIGHT_OFFSET)
        .min(read_i32(destination, abi::LAYER_HEIGHT_OFFSET))
        .max(0) as usize;
    let row_size = source_rowbytes.min(destination_rowbytes);
    for row in 0..height {
        unsafe {
            ptr::copy_nonoverlapping(
                (source_data as *const u8).add(row * source_rowbytes),
                (destination_data as *mut u8).add(row * destination_rowbytes),
                row_size,
            );
        }
    }
    0
}

#[derive(Clone, Copy)]
struct NativeWorld8 {
    data: u64,
    rowbytes: usize,
    width: i32,
    height: i32,
}

fn native_world8(state: &NativeState, world: u64) -> Option<NativeWorld8> {
    let arena_base = state.arena_end.saturating_sub(ARENA_SIZE as u64);
    if world < arena_base || world.checked_add(abi::PF_LAYER_DEF_SIZE as u64)? > state.arena_end {
        return None;
    }
    let read_u64 =
        |offset: usize| unsafe { ptr::read_unaligned((world + offset as u64) as *const u64) };
    let read_i32 =
        |offset: usize| unsafe { ptr::read_unaligned((world + offset as u64) as *const i32) };
    let data = read_u64(abi::LAYER_DATA_OFFSET);
    let rowbytes = usize::try_from(read_i32(abi::LAYER_ROWBYTES_OFFSET)).ok()?;
    let width = read_i32(abi::LAYER_WIDTH_OFFSET);
    let height = read_i32(abi::LAYER_HEIGHT_OFFSET);
    if data == 0
        || width <= 0
        || height <= 0
        || rowbytes < usize::try_from(width).ok()?.checked_mul(4)?
        || height > 16_777_216
    {
        return None;
    }
    let bytes = rowbytes.checked_mul(usize::try_from(height).ok()?)?;
    let end = data.checked_add(bytes as u64)?;
    let in_arena = data >= arena_base && end <= state.arena_end;
    let in_owned_world = state
        .worlds
        .values()
        .any(|record| data >= record.data && end <= record.data + record.size);
    if !in_arena && !in_owned_world {
        return None;
    }
    Some(NativeWorld8 {
        data,
        rowbytes,
        width,
        height,
    })
}

fn native_bounds(area: u64, width: i32, height: i32) -> Option<[i32; 4]> {
    let mut bounds = [0, 0, width, height];
    if area != 0 {
        unsafe {
            ptr::copy_nonoverlapping(area as *const i32, bounds.as_mut_ptr(), 4);
        }
        bounds[0] = bounds[0].clamp(0, width);
        bounds[1] = bounds[1].clamp(0, height);
        bounds[2] = bounds[2].clamp(bounds[0], width);
        bounds[3] = bounds[3].clamp(bounds[1], height);
    }
    (bounds[0] < bounds[2] && bounds[1] < bounds[3]).then_some(bounds)
}

type IteratePixel8 = unsafe extern "win64" fn(u64, i32, i32, u64, u64) -> i32;
type IterateProgress = unsafe extern "win64" fn(u64, i32, i32) -> i32;
type IterateAbort = unsafe extern "win64" fn(u64) -> i32;
type IterateGeneric = unsafe extern "win64" fn(u64, i32, i32, i32) -> i32;

unsafe extern "win64" fn iterate_world8(
    in_data: u64,
    progress_base: i32,
    progress_final: i32,
    source_world: u64,
    area: u64,
    refcon: u64,
    pixel_function: u64,
    destination_world: u64,
) -> u64 {
    if pixel_function == 0 {
        return 4;
    }
    let Some(Some((destination, source, bounds, effect_ref, abort, progress))) =
        with_state(|state| {
            let Some(destination) = native_world8(state, destination_world) else {
                return None;
            };
            let source = if source_world == 0 {
                None
            } else {
                let Some(source) = native_world8(state, source_world) else {
                    return None;
                };
                Some(source)
            };
            let width = source.map_or(destination.width, |source| {
                source.width.min(destination.width)
            });
            let height = source.map_or(destination.height, |source| {
                source.height.min(destination.height)
            });
            let bounds = native_bounds(area, width, height)?;
            let (effect_ref, abort, progress) = if in_data == 0 {
                (0, 0, 0)
            } else {
                unsafe {
                    (
                        ptr::read_unaligned(
                            (in_data + abi::IN_EFFECT_REF_OFFSET as u64) as *const u64,
                        ),
                        ptr::read_unaligned(
                            (in_data + abi::INTER_ABORT_OFFSET as u64) as *const u64,
                        ),
                        ptr::read_unaligned(
                            (in_data + abi::INTER_PROGRESS_OFFSET as u64) as *const u64,
                        ),
                    )
                }
            };
            Some((destination, source, bounds, effect_ref, abort, progress))
        })
    else {
        return 4;
    };
    let [left, top, right, bottom] = bounds;
    let pixel: IteratePixel8 = unsafe { std::mem::transmute(pixel_function as usize) };
    let rows = bottom - top;
    for y in top..bottom {
        for x in left..right {
            let output = destination.data + y as u64 * destination.rowbytes as u64 + x as u64 * 4;
            let input = source.map_or(0, |source| {
                source.data + y as u64 * source.rowbytes as u64 + x as u64 * 4
            });
            let error = unsafe { pixel(refcon, x, y, input, output) };
            if error != 0 || with_state(|state| state.callback_error.is_some()).unwrap_or(true) {
                if error != 0 {
                    return error as u32 as u64;
                }
                return 4;
            }
        }
        let completed = y - top + 1;
        if progress != 0 {
            let callback: IterateProgress = unsafe { std::mem::transmute(progress as usize) };
            let current = progress_base as i64
                + (progress_final as i64 - progress_base as i64) * completed as i64 / rows as i64;
            let error = unsafe { callback(effect_ref, current as i32, progress_final) };
            if error != 0 || with_state(|state| state.callback_error.is_some()).unwrap_or(true) {
                if error != 0 {
                    return error as u32 as u64;
                }
                return 4;
            }
        }
        if completed < rows && abort != 0 {
            let callback: IterateAbort = unsafe { std::mem::transmute(abort as usize) };
            let error = unsafe { callback(effect_ref) };
            if error != 0 || with_state(|state| state.callback_error.is_some()).unwrap_or(true) {
                if error != 0 {
                    return error as u32 as u64;
                }
                return 4;
            }
        }
    }
    0
}

unsafe extern "win64" fn iterate_origin8(
    in_data: u64,
    progress_base: i32,
    progress_final: i32,
    source_world: u64,
    area: u64,
    origin: u64,
    refcon: u64,
    pixel_function: u64,
    destination_world: u64,
) -> u64 {
    // The ordinary iterate contract is identical when origin is zero, which is
    // the overwhelmingly common AE call. Non-zero origin remains explicit
    // fail-closed until the shared callback runner carries coordinate offsets.
    if origin != 0 {
        let origin_x = unsafe { ptr::read_unaligned(origin as *const i16) };
        let origin_y = unsafe { ptr::read_unaligned((origin + 2) as *const i16) };
        if origin_x != 0 || origin_y != 0 {
            return 4;
        }
    }
    unsafe {
        iterate_world8(
            in_data,
            progress_base,
            progress_final,
            source_world,
            area,
            refcon,
            pixel_function,
            destination_world,
        )
    }
}

unsafe extern "win64" fn iterate_origin_non_clip8(
    in_data: u64,
    progress_base: i32,
    progress_final: i32,
    source_world: u64,
    area: u64,
    origin: u64,
    refcon: u64,
    pixel_function: u64,
    destination_world: u64,
) -> u64 {
    unsafe {
        iterate_origin8(
            in_data,
            progress_base,
            progress_final,
            source_world,
            area,
            origin,
            refcon,
            pixel_function,
            destination_world,
        )
    }
}

unsafe extern "win64" fn iterate_lut8(
    _: u64,
    _: i32,
    _: i32,
    source_world: u64,
    area: u64,
    alpha_lut: u64,
    red_lut: u64,
    green_lut: u64,
    blue_lut: u64,
    destination_world: u64,
) -> u64 {
    with_state(|state| {
        let Some(source) = native_world8(state, source_world) else {
            return 4;
        };
        let Some(destination) = native_world8(state, destination_world) else {
            return 4;
        };
        let Some([left, top, right, bottom]) = native_bounds(
            area,
            source.width.min(destination.width),
            source.height.min(destination.height),
        ) else {
            return 4;
        };
        let tables = [alpha_lut, red_lut, green_lut, blue_lut];
        for y in top..bottom {
            for x in left..right {
                let input = source.data + y as u64 * source.rowbytes as u64 + x as u64 * 4;
                let output =
                    destination.data + y as u64 * destination.rowbytes as u64 + x as u64 * 4;
                for (channel, table) in tables.into_iter().enumerate() {
                    let value = unsafe { *((input + channel as u64) as *const u8) };
                    let mapped = if table == 0 {
                        value
                    } else {
                        unsafe { *((table + value as u64) as *const u8) }
                    };
                    unsafe {
                        *((output + channel as u64) as *mut u8) = mapped;
                    }
                }
            }
        }
        0
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn iterate_generic(iterations: i32, refcon: u64, callback: u64) -> u64 {
    if callback == 0 || (iterations != -1 && !(1..=16_777_216).contains(&iterations)) {
        return 4;
    }
    let callback: IterateGeneric = unsafe { std::mem::transmute(callback as usize) };
    let actual = if iterations == -1 { 1 } else { iterations };
    for index in 0..actual {
        let error = unsafe { callback(refcon, 0, index, actual) };
        if error != 0 {
            return error as u32 as u64;
        }
    }
    0
}

unsafe extern "win64" fn pre_checkout_layer(
    _: u64,
    index: u64,
    checkout_id: u64,
    request: u64,
    _: u64,
    _: u64,
    _: u64,
    result: u64,
) -> u64 {
    with_state(|state| {
        state.pre_checkout_calls += 1;
        if index != 0 || checkout_id != 0 || request == 0 || result == 0 {
            state.callback_error = Some(format!(
                "invalid pre-checkout index={index} id={checkout_id} request={request:#x} result={result:#x}"
            ));
            return 4;
        }
        let mut rectangle = [0i32; 4];
        unsafe {
            ptr::copy_nonoverlapping(request as *const i32, rectangle.as_mut_ptr(), 4);
        }
        state.pre_checkout_requests.push(rectangle);
        let width = state.smart_width as i32;
        let height = state.smart_height as i32;
        let mut bytes = [0u8; 76];
        for (offset, value) in [
            (0, 0),
            (4, 0),
            (8, width),
            (12, height),
            (16, 0),
            (20, 0),
            (24, width),
            (28, height),
            (32, 1),
            (36, 1),
            (44, width),
            (48, height),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        unsafe {
            write_pointer(result, &bytes);
        }
        0
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn checkout_layer_pixels(
    _: u64,
    checkout_id: u64,
    output: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_state(|state| {
        state.checkout_pixels_calls += 1;
        if checkout_id != 0 || output == 0 || state.smart_input_world == 0 {
            state.callback_error = Some("invalid checkout-layer-pixels request".into());
            4
        } else {
            unsafe {
                *(output as *mut u64) = state.smart_input_world;
            }
            0
        }
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn checkout_output(
    _: u64,
    output: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_state(|state| {
        state.checkout_output_calls += 1;
        if output == 0 || state.smart_output_world == 0 {
            state.callback_error = Some("invalid checkout-output request".into());
            4
        } else {
            unsafe {
                *(output as *mut u64) = state.smart_output_world;
            }
            0
        }
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn unsupported_utility_slot<const VERSION: u32, const SLOT: usize>(
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_state(|state| {
        record_unsupported_suite_call(
            &mut state.unsupported_suite_calls,
            &mut state.dropped_unsupported_suite_calls,
            VERSION,
            SLOT,
        );
        4
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn register_with_aegp(
    _: u64,
    _: u64,
    plugin_id: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    if plugin_id == 0 {
        return 4;
    }
    unsafe {
        *(plugin_id as *mut i32) = 1;
    }
    0
}

unsafe extern "win64" fn get_main_window(
    main_window: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    if main_window == 0 {
        return 4;
    }
    unsafe {
        *(main_window as *mut u64) = 0;
    }
    0
}

macro_rules! utility_callbacks {
    ($version:literal; $($slot:literal),+ $(,)?) => {{
        vec![$(
            callback_address!(unsupported_utility_slot::<$version, $slot>)
        ),+]
    }};
}

fn native_utility_callbacks(version: u32) -> Option<Vec<u64>> {
    let mut callbacks = match version {
        3 => utility_callbacks!(3; 0, 1, 2, 3, 4, 5, 6, 7, 8),
        7 => utility_callbacks!(
            7;
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
            20, 21, 22, 23, 24
        ),
        11 => utility_callbacks!(
            11;
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
            20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30
        ),
        13 => utility_callbacks!(
            13;
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
            20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32
        ),
        _ => return None,
    };
    let (_, register_slot, window_slot) = utility_suite_layout(version)?;
    callbacks[register_slot] = callback_address!(register_with_aegp);
    callbacks[window_slot] = callback_address!(get_main_window);
    Some(callbacks)
}

unsafe extern "win64" fn acquire_suite(
    name: u64,
    version: u64,
    output: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    let mut bytes = Vec::new();
    if name != 0 {
        for offset in 0..256u64 {
            let byte = unsafe { *((name + offset) as *const u8) };
            if byte == 0 {
                break;
            }
            bytes.push(byte);
        }
    }
    let name = String::from_utf8_lossy(&bytes).into_owned();
    with_state(|state| {
        record_suite_request(&mut state.suite_requests, format!("{name} v{version}"));
        if output != 0 {
            unsafe {
                *(output as *mut u64) = 0;
            }
        }
        if name == "PF Handle Suite" && version == 2 && output != 0 {
            unsafe {
                *(output as *mut u64) = state.handle_suite;
            }
            0
        } else if name == "AEGP Memory Suite" && version == 1 && output != 0 {
            unsafe {
                *(output as *mut u64) = state.aegp_memory_suite;
            }
            0
        } else if name == "PF World Suite" && version == 2 && output != 0 {
            unsafe {
                *(output as *mut u64) = state.world_suite;
            }
            0
        } else if name == "PF Iterate8 Suite" && matches!(version, 1 | 2) && output != 0 {
            unsafe {
                *(output as *mut u64) = state.iterate8_suite;
            }
            0
        } else if name == "PF ColorParamSuite" && version == 1 && output != 0 {
            unsafe {
                *(output as *mut u64) = state.color_param_suite;
            }
            0
        } else if name == "PF PointParamSuite" && version == 1 && output != 0 {
            unsafe {
                *(output as *mut u64) = state.point_param_suite;
            }
            0
        } else if name == "AEGP Utility Suite" && output != 0 {
            let Some(table) = u32::try_from(version)
                .ok()
                .and_then(|version| state.utility_suites.get(&version))
                .copied()
            else {
                return u32::MAX as u64;
            };
            unsafe {
                *(output as *mut u64) = table;
            }
            0
        } else {
            u32::MAX as u64
        }
    })
    .unwrap_or(u32::MAX as u64)
}

fn native_world_pixel_bytes(pixel_format: i32) -> Option<u64> {
    match pixel_format as u32 {
        0x6267_7261 => Some(abi::PF_PIXEL_SIZE as u64),
        0x3631_6561 => Some(abi::PF_PIXEL16_SIZE as u64),
        0x3233_6561 => Some(abi::PF_PIXEL_FLOAT_SIZE as u64),
        _ => None,
    }
}

fn native_world_descriptor_valid(state: &NativeState, world: u64) -> bool {
    native_guest_range_valid(state, world, abi::PF_LAYER_DEF_SIZE as u64)
}

fn native_guest_range_valid(state: &NativeState, address: u64, size: u64) -> bool {
    let Some(end) = address.checked_add(size) else {
        return false;
    };
    let arena_base = state.arena_end.saturating_sub(ARENA_SIZE as u64);
    let thread = unsafe { pthread_self() };
    let stack_top = unsafe { pthread_get_stackaddr_np(thread) } as u64;
    let stack_size = unsafe { pthread_get_stacksize_np(thread) } as u64;
    let stack_base = stack_top.saturating_sub(stack_size);
    (address >= arena_base && end <= state.arena_end)
        || (address >= state.image_start && end <= state.image_end)
        || (address >= stack_base && end <= stack_top)
        || state.crt_heap.allocations().any(|(pointer, allocation)| {
            address >= pointer && end <= pointer + allocation.requested_size
        })
        || state
            .worlds
            .values()
            .any(|record| address >= record.data && end <= record.data + record.size)
}

unsafe extern "win64" fn new_world(
    _: u64,
    width: u64,
    height: u64,
    clear: u64,
    pixel_format: u64,
    world: u64,
) -> u64 {
    let width = width as u32 as i32;
    let height = height as u32 as i32;
    let pixel_format = pixel_format as u32 as i32;
    with_state(|state| {
        let Some(pixel_bytes) = native_world_pixel_bytes(pixel_format) else {
            return 4;
        };
        if width <= 0
            || height <= 0
            || !native_world_descriptor_valid(state, world)
            || state.worlds.contains_key(&world)
        {
            return 4;
        }
        let Some(rowbytes) = u64::try_from(width)
            .ok()
            .and_then(|value| value.checked_mul(pixel_bytes))
        else {
            return 4;
        };
        let Some(size) = rowbytes.checked_mul(height as u64) else {
            return 4;
        };
        if rowbytes > i32::MAX as u64
            || size > MAX_WORLD_SIZE
            || state.worlds.len() >= MAX_WORLD_COUNT
            || state.worlds.values().map(|record| record.size).sum::<u64>() > MAX_WORLD_SIZE - size
        {
            return 4;
        }
        let Some(mapping_size) = usize::try_from(size)
            .ok()
            .and_then(|size| size.checked_add(PAGE_SIZE - 1))
            .map(|size| size & !(PAGE_SIZE - 1))
        else {
            return 4;
        };
        let data = unsafe {
            mmap(
                ptr::null_mut(),
                mapping_size,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANON,
                -1,
                0,
            )
        };
        if data as isize == -1 {
            return 4;
        }
        unsafe {
            ptr::write_bytes(
                data.cast::<u8>(),
                if clear as u8 != 0 { 0 } else { 0xcd },
                size as usize,
            );
        }
        let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        let flags = 2 | i32::from(pixel_format as u32 != 0x6267_7261);
        definition[abi::LAYER_WORLD_FLAGS_OFFSET..abi::LAYER_WORLD_FLAGS_OFFSET + 4]
            .copy_from_slice(&flags.to_le_bytes());
        definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
            .copy_from_slice(&(data as u64).to_le_bytes());
        definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
            .copy_from_slice(&(rowbytes as i32).to_le_bytes());
        definition[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
            .copy_from_slice(&width.to_le_bytes());
        definition[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
            .copy_from_slice(&height.to_le_bytes());
        for (index, value) in [0, 0, width, height].into_iter().enumerate() {
            let offset = abi::LAYER_EXTENT_HINT_OFFSET + index * 4;
            definition[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        definition[88..92].copy_from_slice(&1i32.to_le_bytes());
        definition[92..96].copy_from_slice(&1u32.to_le_bytes());
        unsafe {
            ptr::copy_nonoverlapping(
                definition.as_ptr(),
                world as *mut u8,
                abi::PF_LAYER_DEF_SIZE,
            );
        }
        state.worlds.insert(
            world,
            NativeWorld {
                pixel_format,
                size,
                data: data as u64,
                mapping_size,
            },
        );
        0
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn dispose_world(_: u64, world: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    with_state(|state| {
        if !native_world_descriptor_valid(state, world) {
            return 4;
        }
        let Some(record) = state.worlds.get(&world).cloned() else {
            return 4;
        };
        if unsafe { munmap(record.data as *mut c_void, record.mapping_size) } != 0 {
            return 4;
        }
        unsafe {
            ptr::write_bytes(world as *mut u8, 0, abi::PF_LAYER_DEF_SIZE);
        }
        state.worlds.remove(&world);
        0
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn get_world_pixel_format(
    world: u64,
    output: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_state(|state| {
        if !native_guest_range_valid(state, output, std::mem::size_of::<i32>() as u64) {
            return 4;
        }
        let pixel_format = state
            .worlds
            .get(&world)
            .map(|record| record.pixel_format)
            .or_else(|| {
                (world != 0
                    && (world == state.smart_input_world || world == state.smart_output_world))
                    .then_some(state.smart_pixel_format)
            })
            .or_else(|| {
                if !native_world_descriptor_valid(state, world) {
                    return None;
                }
                let width = unsafe {
                    ptr::read_unaligned((world + abi::LAYER_WIDTH_OFFSET as u64) as *const i32)
                };
                let rowbytes = unsafe {
                    ptr::read_unaligned((world + abi::LAYER_ROWBYTES_OFFSET as u64) as *const i32)
                };
                if width <= 0 || rowbytes <= 0 || rowbytes % width != 0 {
                    return None;
                }
                match rowbytes / width {
                    4 => Some(0x6267_7261u32 as i32),
                    8 => Some(0x3631_6561u32 as i32),
                    16 => Some(0x3233_6561u32 as i32),
                    _ => None,
                }
            });
        let Some(pixel_format) = pixel_format else {
            return 4;
        };
        unsafe {
            ptr::write_unaligned(output as *mut i32, pixel_format);
        }
        0
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn color_param_value(
    effect_ref: u64,
    definition: u64,
    output: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    if effect_ref != HOST_EFFECT_REF || definition == 0 || output == 0 {
        return PF_BAD_CALLBACK_PARAM;
    }
    with_state(|state| {
        let disk_id = unsafe { ptr::read_unaligned(definition as *const i32) };
        let param_type = unsafe {
            ptr::read_unaligned((definition + abi::PARAM_PARAM_TYPE_OFFSET as u64) as *const i32)
        };
        let Some(source) = state.params.iter().find(|parameter| {
            parameter
                .bytes
                .get(..4)
                .and_then(|bytes| bytes.try_into().ok())
                .map(i32::from_le_bytes)
                == Some(disk_id)
        }) else {
            return PF_INVALID_INDEX;
        };
        if param_type != PARAM_TYPE_COLOR || source.param_type != PARAM_TYPE_COLOR {
            return PF_UNRECOGNIZED_PARAM_TYPE;
        }
        let argb: [u8; 4] = unsafe {
            copy_from_pointer(definition + abi::PARAM_U_OFFSET as u64, abi::PF_PIXEL_SIZE)
        }
        .try_into()
        .expect("PF_Pixel is four bytes");
        let current: [u8; 4] = source.bytes
            [abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_PIXEL_SIZE]
            .try_into()
            .expect("PF_Pixel is four bytes");
        let default: [u8; 4] = source.bytes[abi::PARAM_U_OFFSET + abi::PF_PIXEL_SIZE
            ..abi::PARAM_U_OFFSET + abi::PF_PIXEL_SIZE * 2]
            .try_into()
            .expect("PF color default is four bytes");
        if argb != current && argb != default {
            return PF_BAD_CALLBACK_PARAM;
        }
        let pixel = ColorParamPixelFloat {
            alpha: f32::from(argb[0]) / 255.0,
            red: f32::from(argb[1]) / 255.0,
            green: f32::from(argb[2]) / 255.0,
            blue: f32::from(argb[3]) / 255.0,
        };
        unsafe {
            ptr::write_unaligned(output as *mut ColorParamPixelFloat, pixel);
        }
        0
    })
    .unwrap_or(PF_BAD_CALLBACK_PARAM)
}

unsafe extern "win64" fn point_param_value(
    _: u64,
    definition: u64,
    output: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    if definition == 0 || output == 0 {
        return 4;
    }
    let x = unsafe { ptr::read_unaligned((definition + abi::PARAM_U_OFFSET as u64) as *const i32) }
        as f64
        / 65536.0;
    let y =
        unsafe { ptr::read_unaligned((definition + abi::PARAM_U_OFFSET as u64 + 4) as *const i32) }
            as f64
            / 65536.0;
    unsafe {
        ptr::write_unaligned(output as *mut f64, x);
        ptr::write_unaligned((output + 8) as *mut f64, y);
    }
    0
}

unsafe extern "win64" fn checkout_param(
    _: u64,
    index: u64,
    _: u64,
    _: u64,
    _: u64,
    destination: u64,
) -> u64 {
    with_state(|state| {
        let Some(source) = (index as usize)
            .checked_sub(1)
            .and_then(|offset| state.parameter_definitions.get(offset))
            .copied()
        else {
            state.callback_error =
                Some(format!("checkout-param index outside definitions: {index}"));
            return 4;
        };
        if destination == 0 {
            state.callback_error = Some("checkout-param destination is null".into());
            return 4;
        }
        unsafe {
            ptr::copy_nonoverlapping(
                source as *const u8,
                destination as *mut u8,
                abi::PF_PARAM_DEF_SIZE,
            );
        }
        0
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn new_handle(size: u64, _: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    with_state(|state| {
        state.handle_allocations.push(size);
        let live_bytes = state.handles.values().map(|record| record.size).sum::<u64>();
        if size > MAX_PF_HANDLE_SIZE
            || state.handles.len() >= MAX_PF_HANDLE_COUNT
            || live_bytes > MAX_PF_HANDLE_SIZE - size
        {
            state.callback_error = Some(format!(
                "PF Handle allocation exceeds live budget: size={size}, live_bytes={live_bytes}, live_count={}",
                state.handles.len()
            ));
            return 0;
        }
        let Some(data_mapping_size) = usize::try_from(size.max(1))
            .ok()
            .and_then(|size| size.checked_add(PAGE_SIZE - 1))
            .map(|size| size & !(PAGE_SIZE - 1))
        else {
            state.callback_error = Some(format!("PF Handle allocation size overflow: {size}"));
            return 0;
        };
        let handle = unsafe {
            mmap(
                ptr::null_mut(),
                PAGE_SIZE,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANON,
                -1,
                0,
            )
        };
        if handle as isize == -1 {
            state.callback_error = Some(format!(
                "PF Handle header allocation failed for {size} bytes"
            ));
            return 0;
        }
        let data = unsafe {
            mmap(
                ptr::null_mut(),
                data_mapping_size,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANON,
                -1,
                0,
            )
        };
        if data as isize == -1 {
            unsafe {
                munmap(handle, PAGE_SIZE);
            }
            state.callback_error =
                Some(format!("PF Handle data allocation failed for {size} bytes"));
            return 0;
        }
        let handle = handle as u64;
        let data = data as u64;
        unsafe {
            *(handle as *mut u64) = data;
        }
        state.handles.insert(
            handle,
            NativeHandle {
                data,
                size,
                locks: 0,
                data_mapping_size,
            },
        );
        handle
    })
    .unwrap_or(0)
}

unsafe extern "win64" fn lock_handle(handle: u64, _: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    with_state(|state| {
        if let Some(record) = state.handles.get_mut(&handle) {
            record.locks = record.locks.saturating_add(1);
            record.data
        } else {
            state.callback_error = Some(format!(
                "PF Handle lock received unknown handle {handle:#x}"
            ));
            0
        }
    })
    .unwrap_or(0)
}

unsafe extern "win64" fn unlock_handle(handle: u64, _: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    with_state(|state| match state.handles.get_mut(&handle) {
        Some(record) if record.locks != 0 => record.locks -= 1,
        _ => {
            state.callback_error = Some(format!(
                "PF Handle unlock received stale or unlocked handle {handle:#x}"
            ));
        }
    });
    0
}

unsafe extern "win64" fn dispose_handle(
    handle: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_state(|state| {
        let Some(record) = state.handles.get(&handle).cloned() else {
            state.callback_error = Some(format!(
                "PF Handle dispose received stale handle {handle:#x}"
            ));
            return;
        };
        if record.locks != 0 {
            state.callback_error = Some(format!(
                "PF Handle dispose received locked handle {handle:#x}"
            ));
            return;
        }
        state.handles.remove(&handle);
        unsafe {
            munmap(record.data as *mut c_void, record.data_mapping_size);
            munmap(handle as *mut c_void, PAGE_SIZE);
        }
    });
    0
}

unsafe extern "win64" fn handle_size(handle: u64, _: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    with_state(|state| {
        if let Some(record) = state.handles.get(&handle) {
            record.size
        } else {
            state.callback_error = Some(format!(
                "PF Handle size received unknown handle {handle:#x}"
            ));
            0
        }
    })
    .unwrap_or(0)
}

unsafe extern "win64" fn resize_handle(
    size: u64,
    handle_pointer: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_state(|state| {
        if size > MAX_PF_HANDLE_SIZE || handle_pointer == 0 {
            return 4;
        }
        let handle = unsafe { *(handle_pointer as *const u64) };
        let Some(old) = state.handles.get(&handle).cloned() else {
            return 4;
        };
        if old.locks != 0 {
            return 4;
        }
        let live_bytes = state
            .handles
            .values()
            .map(|record| record.size)
            .sum::<u64>();
        if live_bytes - old.size > MAX_PF_HANDLE_SIZE - size {
            state.callback_error = Some(format!(
                "PF Handle resize exceeds live budget: size={size}, live_bytes={live_bytes}"
            ));
            return 4;
        }
        let Some(data_mapping_size) = usize::try_from(size.max(1))
            .ok()
            .and_then(|size| size.checked_add(PAGE_SIZE - 1))
            .map(|size| size & !(PAGE_SIZE - 1))
        else {
            return 4;
        };
        let data = unsafe {
            mmap(
                ptr::null_mut(),
                data_mapping_size,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANON,
                -1,
                0,
            )
        };
        if data as isize == -1 {
            state.callback_error = Some(format!(
                "PF Handle resize allocation failed for {size} bytes"
            ));
            return 1;
        }
        let data = data as u64;
        unsafe {
            ptr::copy_nonoverlapping(
                old.data as *const u8,
                data as *mut u8,
                old.size.min(size) as usize,
            );
            *(handle as *mut u64) = data;
            munmap(old.data as *mut c_void, old.data_mapping_size);
        }
        if let Some(record) = state.handles.get_mut(&handle) {
            record.data = data;
            record.size = size;
            record.data_mapping_size = data_mapping_size;
        }
        0
    })
    .unwrap_or(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crt_heap_imports_allocate_zero_and_reject_invalid_free() {
        let mut state = NativeState::default();
        ACTIVE_STATE.with(|slot| slot.set(&mut state));

        let zero = unsafe { native_crt_malloc(0, 0, 0, 0, 0, 0) };
        assert_ne!(zero, 0);
        assert_eq!(zero % crate::crt_heap::CRT_HEAP_ALIGNMENT, 0);
        let calloc_pointer = unsafe { native_crt_calloc(8, 4, 0, 0, 0, 0) };
        assert_ne!(calloc_pointer, 0);
        let bytes = unsafe { std::slice::from_raw_parts(calloc_pointer as *const u8, 32) };
        assert_eq!(bytes, &[0; 32]);

        assert_eq!(unsafe { native_crt_free(0, 0, 0, 0, 0, 0) }, 0);
        assert!(state.callback_error.is_none());
        assert_eq!(unsafe { native_crt_free(zero, 0, 0, 0, 0, 0) }, 0);
        assert_eq!(unsafe { native_crt_free(zero, 0, 0, 0, 0, 0) }, 0);
        assert!(
            state
                .callback_error
                .as_deref()
                .is_some_and(|message| message.contains("foreign or already-freed"))
        );
        state.callback_error = None;
        assert_eq!(unsafe { native_crt_free(calloc_pointer, 0, 0, 0, 0, 0) }, 0);
        assert_eq!(state.crt_heap.allocations().count(), 0);
        ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
    }

    #[test]
    fn crt_heap_imports_return_null_for_overflow_and_budget_failure() {
        let mut state = NativeState::default();
        ACTIVE_STATE.with(|slot| slot.set(&mut state));
        assert_eq!(unsafe { native_crt_calloc(u64::MAX, 2, 0, 0, 0, 0) }, 0);
        assert_eq!(
            unsafe {
                native_crt_malloc(crate::crt_heap::MAX_CRT_ALLOCATION_BYTES + 1, 0, 0, 0, 0, 0)
            },
            0
        );
        assert!(state.callback_error.is_none());
        ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
    }

    #[test]
    fn color_param_suite_v1_is_stateful_and_fails_closed() {
        let mut definition = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        definition[..4].copy_from_slice(&101i32.to_le_bytes());
        definition[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
            .copy_from_slice(&PARAM_TYPE_COLOR.to_le_bytes());
        definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 8]
            .copy_from_slice(&[255, 64, 128, 192, 255, 1, 2, 3]);
        let mut state = NativeState::default();
        state.color_param_suite = 0x1234;
        state.params.push(GuestParam {
            index: 1,
            param_type: PARAM_TYPE_COLOR,
            name: "Key Color".into(),
            bytes: definition.clone(),
        });
        state.parameter_definitions.push(definition.as_ptr() as u64);
        ACTIVE_STATE.with(|slot| slot.set(&mut state));
        let name = b"PF ColorParamSuite\0";
        let mut suite = 0u64;
        assert_eq!(
            unsafe {
                acquire_suite(
                    name.as_ptr() as u64,
                    1,
                    (&mut suite as *mut u64) as u64,
                    0,
                    0,
                    0,
                )
            },
            0
        );
        assert_eq!(suite, state.color_param_suite);

        let mut output = ColorParamPixelFloat {
            alpha: -1.0,
            red: -1.0,
            green: -1.0,
            blue: -1.0,
        };
        assert_eq!(
            unsafe {
                color_param_value(
                    HOST_EFFECT_REF,
                    definition.as_ptr() as u64,
                    (&mut output as *mut ColorParamPixelFloat) as u64,
                    0,
                    0,
                    0,
                )
            },
            0
        );
        assert_eq!(
            output,
            ColorParamPixelFloat {
                alpha: 1.0,
                red: 64.0 / 255.0,
                green: 128.0 / 255.0,
                blue: 192.0 / 255.0,
            }
        );

        let sentinel = output;
        definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 4].copy_from_slice(&[9, 9, 9, 9]);
        assert_eq!(
            unsafe {
                color_param_value(
                    HOST_EFFECT_REF,
                    definition.as_ptr() as u64,
                    (&mut output as *mut ColorParamPixelFloat) as u64,
                    0,
                    0,
                    0,
                )
            },
            PF_BAD_CALLBACK_PARAM
        );
        assert_eq!(output, sentinel);
        definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 4].copy_from_slice(&[255, 1, 2, 3]);
        assert_eq!(
            unsafe {
                color_param_value(
                    HOST_EFFECT_REF,
                    definition.as_ptr() as u64,
                    (&mut output as *mut ColorParamPixelFloat) as u64,
                    0,
                    0,
                    0,
                )
            },
            0
        );
        definition[..4].copy_from_slice(&999i32.to_le_bytes());
        assert_eq!(
            unsafe {
                color_param_value(
                    HOST_EFFECT_REF,
                    definition.as_ptr() as u64,
                    (&mut output as *mut ColorParamPixelFloat) as u64,
                    0,
                    0,
                    0,
                )
            },
            PF_INVALID_INDEX
        );
        definition[..4].copy_from_slice(&101i32.to_le_bytes());
        definition[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
            .copy_from_slice(&6i32.to_le_bytes());
        assert_eq!(
            unsafe {
                color_param_value(
                    HOST_EFFECT_REF,
                    definition.as_ptr() as u64,
                    (&mut output as *mut ColorParamPixelFloat) as u64,
                    0,
                    0,
                    0,
                )
            },
            PF_UNRECOGNIZED_PARAM_TYPE
        );
        assert_eq!(
            unsafe {
                color_param_value(
                    0,
                    definition.as_ptr() as u64,
                    (&mut output as *mut ColorParamPixelFloat) as u64,
                    0,
                    0,
                    0,
                )
            },
            PF_BAD_CALLBACK_PARAM
        );
        ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
    }

    #[test]
    fn point_param_suite_v1_returns_signed_fixed_values_and_rejects_nulls() {
        let mut state = NativeState {
            point_param_suite: 0x5678,
            ..NativeState::default()
        };
        ACTIVE_STATE.with(|slot| slot.set(&mut state));
        let name = b"PF PointParamSuite\0";
        let mut suite = 0u64;
        assert_eq!(
            unsafe {
                acquire_suite(
                    name.as_ptr() as u64,
                    1,
                    (&mut suite as *mut u64) as u64,
                    0,
                    0,
                    0,
                )
            },
            0
        );
        assert_eq!(suite, state.point_param_suite);

        let mut definition = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        definition[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
            .copy_from_slice(&PARAM_TYPE_POINT.to_le_bytes());
        definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 4]
            .copy_from_slice(&98304i32.to_le_bytes());
        definition[abi::PARAM_U_OFFSET + 4..abi::PARAM_U_OFFSET + 8]
            .copy_from_slice(&(-147456i32).to_le_bytes());
        let mut output = [0.0f64; 2];
        assert_eq!(
            unsafe {
                point_param_value(
                    HOST_EFFECT_REF,
                    definition.as_ptr() as u64,
                    output.as_mut_ptr() as u64,
                    0,
                    0,
                    0,
                )
            },
            0
        );
        assert_eq!(output, [1.5, -2.25]);
        assert_eq!(
            unsafe { point_param_value(HOST_EFFECT_REF, 0, output.as_mut_ptr() as u64, 0, 0, 0) },
            4
        );
        assert_eq!(
            unsafe { point_param_value(HOST_EFFECT_REF, definition.as_ptr() as u64, 0, 0, 0, 0,) },
            4
        );
        ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
    }

    #[test]
    fn pf_handles_map_large_storage_and_preserve_resize_bytes() {
        let mut state = NativeState::default();
        ACTIVE_STATE.with(|slot| slot.set(&mut state));
        let size = 333_294_848;
        let handle = unsafe { new_handle(size, 0, 0, 0, 0, 0) };
        assert_ne!(handle, 0);
        let data = unsafe { lock_handle(handle, 0, 0, 0, 0, 0) };
        assert_ne!(data, 0);
        unsafe {
            *((data + size - 1) as *mut u8) = 0x5a;
        }
        assert_eq!(unsafe { handle_size(handle, 0, 0, 0, 0, 0) }, size);
        unsafe {
            unlock_handle(handle, 0, 0, 0, 0, 0);
            dispose_handle(handle, 0, 0, 0, 0, 0);
        }

        let handle = unsafe { new_handle(16, 0, 0, 0, 0, 0) };
        let data = unsafe { lock_handle(handle, 0, 0, 0, 0, 0) };
        unsafe {
            *(data as *mut u32) = 0x0403_0201;
            unlock_handle(handle, 0, 0, 0, 0, 0);
        }
        let mut handle_pointer = handle;
        assert_eq!(
            unsafe { resize_handle(8192, (&mut handle_pointer as *mut u64) as u64, 0, 0, 0, 0,) },
            0
        );
        assert_eq!(handle_pointer, handle);
        let resized = unsafe { lock_handle(handle, 0, 0, 0, 0, 0) };
        assert_eq!(unsafe { *(resized as *const u32) }, 0x0403_0201);
        unsafe {
            unlock_handle(handle, 0, 0, 0, 0, 0);
            dispose_handle(handle, 0, 0, 0, 0, 0);
        }
        assert!(state.handles.is_empty());
        assert!(state.callback_error.is_none());

        let budget_handle = unsafe { new_handle(MAX_PF_HANDLE_SIZE, 0, 0, 0, 0, 0) };
        assert_ne!(budget_handle, 0);
        assert_eq!(unsafe { new_handle(1, 0, 0, 0, 0, 0) }, 0);
        assert!(
            state
                .callback_error
                .as_deref()
                .is_some_and(|message| message.contains("live budget"))
        );
        state.callback_error = None;
        unsafe {
            dispose_handle(budget_handle, 0, 0, 0, 0, 0);
        }
        let released_budget_handle = unsafe { new_handle(1, 0, 0, 0, 0, 0) };
        assert_ne!(released_budget_handle, 0);
        unsafe {
            dispose_handle(released_budget_handle, 0, 0, 0, 0, 0);
        }

        let mut regression_handles = Vec::with_capacity(1025);
        for _ in 0..1025 {
            let handle = unsafe { new_handle(0, 0, 0, 0, 0, 0) };
            assert_ne!(
                handle, 0,
                "real AEX workloads must be allowed to exceed the old 1024-handle cap"
            );
            regression_handles.push(handle);
        }
        for handle in regression_handles {
            unsafe {
                dispose_handle(handle, 0, 0, 0, 0, 0);
            }
        }
        assert!(state.handles.is_empty());
        assert!(state.callback_error.is_none());

        assert_eq!(unsafe { lock_handle(0xdead_beef, 0, 0, 0, 0, 0) }, 0);
        assert!(
            state
                .callback_error
                .as_deref()
                .is_some_and(|message| message.contains("unknown handle"))
        );
        ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
    }

    #[test]
    fn openmp_thread_count_is_positive_and_deterministic() {
        assert_eq!(unsafe { native_omp_get_max_threads(0, 0, 0, 0, 0, 0) }, 1);
        let omp_callback: unsafe extern "win64" fn(u64, u64, u64, u64, u64, u64) -> u64 =
            unsafe { std::mem::transmute(native_import_callback("omp_get_max_threads")) };
        assert_eq!(unsafe { omp_callback(0, 0, 0, 0, 0, 0) }, 1);
        let unknown_callback: unsafe extern "win64" fn(u64, u64, u64, u64, u64, u64) -> u64 =
            unsafe { std::mem::transmute(native_import_callback("unknown_import")) };
        assert_eq!(unsafe { unknown_callback(0, 0, 0, 0, 0, 0) }, 0);
    }

    #[test]
    fn utility_v7_v13_callbacks_match_unicorn_contract() {
        let mut state = NativeState::default();
        ACTIVE_STATE.with(|slot| slot.set(&mut state));

        for version in [7u32, 13] {
            let callbacks = native_utility_callbacks(version).unwrap();
            let (slot_count, register_slot, window_slot) = utility_suite_layout(version).unwrap();
            assert_eq!(callbacks.len(), slot_count);

            let register: Win64Function =
                unsafe { std::mem::transmute(callbacks[register_slot] as usize) };
            let mut plugin_id = 0i32;
            assert_eq!(
                unsafe { register(0, 0, (&mut plugin_id as *mut i32) as u64, 0, 0, 0,) },
                0
            );
            assert_eq!(plugin_id, 1);

            let get_window: Win64Function =
                unsafe { std::mem::transmute(callbacks[window_slot] as usize) };
            let mut window = u64::MAX;
            assert_eq!(
                unsafe { get_window((&mut window as *mut u64) as u64, 0, 0, 0, 0, 0) },
                0
            );
            assert_eq!(window, 0);

            let unsupported: Win64Function = unsafe { std::mem::transmute(callbacks[0] as usize) };
            assert_eq!(unsafe { unsupported(0, 0, 0, 0, 0, 0) }, 4);
            assert_eq!(unsafe { unsupported(0, 0, 0, 0, 0, 0) }, 4);
        }

        ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
        assert_eq!(
            state.unsupported_suite_calls,
            [
                UnsupportedSuiteCall {
                    name: "AEGP Utility Suite",
                    version: 7,
                    slot: 0,
                    call_count: 2,
                },
                UnsupportedSuiteCall {
                    name: "AEGP Utility Suite",
                    version: 13,
                    slot: 0,
                    call_count: 2,
                },
            ]
        );
        assert_eq!(state.dropped_unsupported_suite_calls, 0);
    }

    #[test]
    fn pf_world_suite_v2_native_lifecycle_matches_unicorn() {
        let mut arena = vec![0u8; 0x10000];
        let arena_base = arena.as_mut_ptr() as u64;
        let mut state = NativeState {
            arena_next: arena_base,
            arena_end: arena_base + arena.len() as u64,
            world_suite: 0x1234,
            ..NativeState::default()
        };
        ACTIVE_STATE.with(|slot| slot.set(&mut state));
        let mut world_storage = [0u8; abi::PF_LAYER_DEF_SIZE];
        let world = world_storage.as_mut_ptr() as u64;
        let suite_name = std::ffi::CString::new("PF World Suite").unwrap();
        let mut suite = 0u64;
        assert_eq!(
            unsafe {
                acquire_suite(
                    suite_name.as_ptr() as u64,
                    2,
                    (&mut suite as *mut u64) as u64,
                    0,
                    0,
                    0,
                )
            },
            0
        );
        assert_eq!(suite, state.world_suite);

        assert_eq!(unsafe { new_world(1, 3, 2, 1, 0xdead_beef, world) }, 4);
        assert!(state.worlds.is_empty());
        assert_eq!(unsafe { new_world(1, 3, 2, 1, 0x3631_6561, world) }, 0);
        let data =
            unsafe { ptr::read_unaligned((world + abi::LAYER_DATA_OFFSET as u64) as *const u64) };
        assert_ne!(data, 0);
        assert_eq!(
            unsafe {
                ptr::read_unaligned((world + abi::LAYER_ROWBYTES_OFFSET as u64) as *const i32)
            },
            24
        );
        assert_eq!(
            unsafe { std::slice::from_raw_parts(data as *const u8, 48) },
            &[0; 48]
        );
        assert_eq!(unsafe { new_world(1, 3, 2, 1, 0x3631_6561, world) }, 4);
        assert_eq!(state.worlds.len(), 1);
        let mut format = 0u32;
        let format_output = (&mut format as *mut u32) as u64;
        format = 0xfeed_beef;
        assert_eq!(
            unsafe { get_world_pixel_format(0, format_output, 0, 0, 0, 0) },
            4
        );
        assert_eq!(format, 0xfeed_beef);
        assert_eq!(
            unsafe { get_world_pixel_format(world, format_output, 0, 0, 0, 0) },
            0
        );
        assert_eq!(format, 0x3631_6561);
        let mut smart_input_storage = [0u8; abi::PF_LAYER_DEF_SIZE];
        let mut smart_output_storage = [0u8; abi::PF_LAYER_DEF_SIZE];
        state.smart_input_world = smart_input_storage.as_mut_ptr() as u64;
        state.smart_output_world = smart_output_storage.as_mut_ptr() as u64;
        state.smart_pixel_format = crate::pixel::PF_PIXEL_FORMAT_ARGB128;
        for smart_world in [state.smart_input_world, state.smart_output_world] {
            assert_eq!(
                unsafe { get_world_pixel_format(smart_world, format_output, 0, 0, 0, 0) },
                0
            );
            assert_eq!(format as i32, crate::pixel::PF_PIXEL_FORMAT_ARGB128);
        }
        assert_eq!(unsafe { get_world_pixel_format(world, 0, 0, 0, 0, 0) }, 4);
        assert_eq!(unsafe { dispose_world(1, world, 0, 0, 0, 0) }, 0);
        assert!(state.worlds.is_empty());
        assert_eq!(
            unsafe { std::slice::from_raw_parts(world as *const u8, abi::PF_LAYER_DEF_SIZE) },
            vec![0; abi::PF_LAYER_DEF_SIZE]
        );
        assert_eq!(unsafe { dispose_world(1, world, 0, 0, 0, 0) }, 4);
        world_storage[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
            .copy_from_slice(&2i32.to_le_bytes());
        world_storage[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
            .copy_from_slice(&8i32.to_le_bytes());
        assert_eq!(
            unsafe { get_world_pixel_format(world, format_output, 0, 0, 0, 0) },
            0
        );
        assert_eq!(format, 0x6267_7261);
        assert_eq!(unsafe { new_world(1, 1, 1, 0x100, 0x3233_6561, world) }, 0);
        let float_data =
            unsafe { ptr::read_unaligned((world + abi::LAYER_DATA_OFFSET as u64) as *const u64) };
        assert_eq!(
            unsafe { std::slice::from_raw_parts(float_data as *const u8, 16) },
            &[0xcd; 16]
        );
        assert_eq!(unsafe { dispose_world(1, world, 0, 0, 0, 0) }, 0);
        assert_eq!(
            unsafe { new_world(1, i32::MAX as u64, i32::MAX as u64, 1, 0x6267_7261, world,) },
            4
        );
        assert!(state.worlds.is_empty());
        ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
    }

    #[test]
    fn aegp_memory_v1_slots_zero_through_five_match_unicorn_lifecycle() {
        let mut arena = vec![0u8; 0x10000];
        let arena_end = arena.as_ptr() as u64 + arena.len() as u64;
        let mut state = NativeState {
            arena_next: arena.as_mut_ptr() as u64,
            arena_end,
            ..NativeState::default()
        };
        with_native_aegp_memory_context(
            &mut state.aegp_memory,
            &mut state.arena_next,
            arena_end,
            || {
                let label = std::ffi::CString::new("olm_memory").unwrap();
                let mut handle = u64::MAX;
                assert_eq!(
                    unsafe {
                        new_aegp_mem_handle(
                            1,
                            label.as_ptr() as u64,
                            i32::MAX as u64 + 1,
                            0,
                            (&mut handle as *mut u64) as u64,
                            0,
                        )
                    },
                    4
                );
                assert_eq!(handle, 0);
                assert_eq!(
                    unsafe {
                        new_aegp_mem_handle(
                            1,
                            label.as_ptr() as u64,
                            4,
                            1,
                            (&mut handle as *mut u64) as u64,
                            0,
                        )
                    },
                    0
                );
                assert_ne!(handle, 0);
                assert_eq!(handle % 8, 0);

                let mut data = 0u64;
                assert_eq!(
                    unsafe {
                        lock_aegp_mem_handle(handle, (&mut data as *mut u64) as u64, 0, 0, 0, 0)
                    },
                    0
                );
                assert_eq!(data % 16, 0);
                assert_eq!(unsafe { *(data as *const u32) }, 0);
                unsafe {
                    *(data as *mut u32) = 0x1122_3344;
                }
                assert_eq!(unsafe { free_aegp_mem_handle(handle, 0, 0, 0, 0, 0) }, 4);
                assert_eq!(
                    unsafe { resize_aegp_mem_handle(label.as_ptr() as u64, 8, handle, 0, 0, 0) },
                    4
                );
                assert_eq!(unsafe { unlock_aegp_mem_handle(handle, 0, 0, 0, 0, 0) }, 0);

                let mut size = 0u32;
                assert_eq!(
                    unsafe {
                        get_aegp_mem_handle_size(handle, (&mut size as *mut u32) as u64, 0, 0, 0, 0)
                    },
                    0
                );
                assert_eq!(size, 4);
                assert_eq!(
                    unsafe { resize_aegp_mem_handle(label.as_ptr() as u64, 8, handle, 0, 0, 0) },
                    0
                );
                let mut resized_data = 0u64;
                assert_eq!(
                    unsafe {
                        lock_aegp_mem_handle(
                            handle,
                            (&mut resized_data as *mut u64) as u64,
                            0,
                            0,
                            0,
                            0,
                        )
                    },
                    0
                );
                assert_eq!(unsafe { *(resized_data as *const u32) }, 0x1122_3344);
                assert_eq!(unsafe { *((resized_data + 4) as *const u32) }, 0);
                assert_eq!(unsafe { unlock_aegp_mem_handle(handle, 0, 0, 0, 0, 0) }, 0);
                assert_eq!(unsafe { free_aegp_mem_handle(handle, 0, 0, 0, 0, 0) }, 0);
                assert_eq!(unsafe { free_aegp_mem_handle(handle, 0, 0, 0, 0, 0) }, 4);
                assert_eq!(
                    unsafe {
                        get_aegp_mem_handle_size(handle, (&mut size as *mut u32) as u64, 0, 0, 0, 0)
                    },
                    4
                );
                let callbacks = native_aegp_memory_callbacks();
                assert_eq!(
                    callbacks[6],
                    callback_address!(unsupported_aegp_memory_slot)
                );
                assert_eq!(
                    callbacks[7],
                    callback_address!(unsupported_aegp_memory_slot)
                );
                assert_eq!(unsafe { unsupported_aegp_memory_slot(0, 0, 0, 0, 0, 0) }, 4);

                let mut reuse_high_water = 0u64;
                for cycle in 0..(MAX_AEGP_MEMORY_HANDLES * 2) {
                    let mut recycled_handle = 0u64;
                    assert_eq!(
                        unsafe {
                            new_aegp_mem_handle(
                                1,
                                label.as_ptr() as u64,
                                32,
                                0,
                                (&mut recycled_handle as *mut u64) as u64,
                                0,
                            )
                        },
                        0
                    );
                    let mut recycled_data = 0u64;
                    assert_eq!(
                        unsafe {
                            lock_aegp_mem_handle(
                                recycled_handle,
                                (&mut recycled_data as *mut u64) as u64,
                                0,
                                0,
                                0,
                                0,
                            )
                        },
                        0
                    );
                    assert!(recycled_data >= arena.as_ptr() as u64);
                    assert!(recycled_data + 32 <= arena_end);
                    assert_eq!(
                        unsafe { unlock_aegp_mem_handle(recycled_handle, 0, 0, 0, 0, 0) },
                        0
                    );
                    assert_eq!(
                        unsafe { free_aegp_mem_handle(recycled_handle, 0, 0, 0, 0, 0) },
                        0
                    );
                    if cycle == 0 {
                        reuse_high_water = active_arena_next().unwrap();
                    } else {
                        assert_eq!(active_arena_next().unwrap(), reuse_high_water);
                    }
                }
                let mut resize_handle = 0u64;
                assert_eq!(
                    unsafe {
                        new_aegp_mem_handle(
                            1,
                            label.as_ptr() as u64,
                            32,
                            0,
                            (&mut resize_handle as *mut u64) as u64,
                            0,
                        )
                    },
                    0
                );
                for _ in 0..(MAX_AEGP_MEMORY_HANDLES * 2) {
                    assert_eq!(
                        unsafe {
                            resize_aegp_mem_handle(
                                label.as_ptr() as u64,
                                128,
                                resize_handle,
                                0,
                                0,
                                0,
                            )
                        },
                        0
                    );
                    assert_eq!(
                        unsafe {
                            resize_aegp_mem_handle(
                                label.as_ptr() as u64,
                                32,
                                resize_handle,
                                0,
                                0,
                                0,
                            )
                        },
                        0
                    );
                }
                assert_eq!(
                    unsafe { free_aegp_mem_handle(resize_handle, 0, 0, 0, 0, 0) },
                    0
                );
            },
        );
    }
}
