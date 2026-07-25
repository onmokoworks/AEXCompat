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

use crate::pe::PeImage;
pub use crate::x64::{
    ExecutionTrace, GuestCensus, GuestParam, TraceStateValue, TraceWatchSpec, UnsupportedSuiteCall,
};
use crate::x64::{record_unsupported_suite_call, utility_suite_layout};

const ARENA_SIZE: usize = 256 * 1024 * 1024;
const MAX_HANDLE_SIZE: u64 = 128 * 1024 * 1024;
const PROT_READ: c_int = 0x1;
const PROT_WRITE: c_int = 0x2;
const PROT_EXEC: c_int = 0x4;
const MAP_PRIVATE: c_int = 0x0002;
const MAP_ANON: c_int = 0x1000;

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

#[derive(Clone, Debug)]
struct NativeHandle {
    data: u64,
    size: u64,
    locks: u32,
}

#[derive(Clone, Debug)]
struct NativeWorld {
    pixel_format: i32,
}

#[derive(Default)]
struct NativeState {
    params: Vec<GuestParam>,
    callback_error: Option<String>,
    smart_input_world: u64,
    smart_output_world: u64,
    smart_width: u32,
    smart_height: u32,
    suite_requests: Vec<String>,
    unsupported_suite_calls: Vec<UnsupportedSuiteCall>,
    dropped_unsupported_suite_calls: u64,
    utility_suites: HashMap<u32, u64>,
    pre_checkout_calls: u32,
    pre_checkout_requests: Vec<[i32; 4]>,
    checkout_pixels_calls: u32,
    checkout_output_calls: u32,
    parameter_definitions: Vec<u64>,
    handles: HashMap<u64, NativeHandle>,
    aegp_memory_handles: HashMap<u64, NativeHandle>,
    worlds: HashMap<u64, NativeWorld>,
    handle_allocations: Vec<u64>,
    arena_next: u64,
    arena_end: u64,
    handle_suite: u64,
    aegp_memory_suite: u64,
    world_suite: u64,
    point_param_suite: u64,
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
    lifetime: PhantomData<&'a ()>,
}

type Win64Function = unsafe extern "win64" fn(u64, u64, u64, u64, u64, u64) -> u64;

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
                ..NativeState::default()
            },
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
        let memory_callbacks = [
            callback_address!(new_aegp_mem_handle),
            callback_address!(free_aegp_mem_handle),
            callback_address!(lock_aegp_mem_handle),
            callback_address!(unlock_aegp_mem_handle),
            callback_address!(get_aegp_mem_handle_size),
            callback_address!(resize_aegp_mem_handle),
            callback_address!(unsupported_aegp_memory_slot),
            callback_address!(unsupported_aegp_memory_slot),
        ];
        for (index, callback) in memory_callbacks.into_iter().enumerate() {
            engine.write_u64(aegp_memory_suite + (index * 8) as u64, callback)?;
        }
        engine.state.aegp_memory_suite = aegp_memory_suite;
        let world_suite = engine.allocate(24, 8)?;
        for (index, callback) in [
            callback_address!(new_world),
            callback_address!(dispose_world),
            callback_address!(get_world_pixel_format),
        ]
        .into_iter()
        .enumerate()
        {
            engine.write_u64(world_suite + (index * 8) as u64, callback)?;
        }
        engine.state.world_suite = world_suite;
        let point_param_suite = engine.allocate(8, 8)?;
        engine.write_u64(point_param_suite, callback_address!(point_param_value))?;
        engine.state.point_param_suite = point_param_suite;
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
        }
        Ok(engine)
    }

    fn install_imports(&mut self, image: &PeImage) -> Result<(), GuestError> {
        for library in image.imports() {
            for symbol in &library.symbols {
                let callback = match symbol.name.as_str() {
                    "strncpy" => callback_address!(native_strncpy),
                    "memset" => callback_address!(native_memset),
                    "expf" => callback_address!(native_expf),
                    "floorf" => callback_address!(native_floorf),
                    "powf" => callback_address!(native_powf),
                    "pow" => callback_address!(native_pow),
                    _ => callback_address!(noop_import),
                };
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
        let result = unsafe { function(args[0], args[1], args[2], args[3], args[4], args[5]) };
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

    pub fn configure_parameter_definitions(&mut self, definitions: Vec<u64>) {
        self.state.parameter_definitions = definitions;
    }

    pub fn configure_smart_render(
        &mut self,
        input_world: u64,
        output_world: u64,
        width: u32,
        height: u32,
    ) {
        self.state.pre_checkout_requests.clear();
        self.state.smart_input_world = input_world;
        self.state.smart_output_world = output_world;
        self.state.smart_width = width;
        self.state.smart_height = height;
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
        state.suite_requests.push(format!("{name} v{version}"));
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
        0x6267_7261 => Some(4),
        0x3631_6561 => Some(8),
        0x3233_6561 => Some(16),
        _ => None,
    }
}

unsafe extern "win64" fn new_world(
    _: u64,
    width: u64,
    height: u64,
    clear: u64,
    pixel_format: u64,
    world: u64,
) -> u64 {
    let width = width as i32;
    let height = height as i32;
    let pixel_format = pixel_format as i32;
    with_state(|state| {
        let Some(pixel_bytes) = native_world_pixel_bytes(pixel_format) else {
            return 4;
        };
        if width <= 0 || height <= 0 || world == 0 || state.worlds.contains_key(&world) {
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
        if rowbytes > i32::MAX as u64 || size > 128 * 1024 * 1024 {
            return 4;
        }
        let data = (state.arena_next + 63) & !63;
        let Some(end) = data.checked_add(size.max(1)) else {
            return 4;
        };
        if end > state.arena_end {
            return 4;
        }
        state.arena_next = end;
        unsafe {
            ptr::write_bytes(
                data as *mut u8,
                if clear != 0 { 0 } else { 0xcd },
                size as usize,
            );
        }
        let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        let flags = 2 | i32::from(pixel_format as u32 != 0x6267_7261);
        definition[abi::LAYER_WORLD_FLAGS_OFFSET..abi::LAYER_WORLD_FLAGS_OFFSET + 4]
            .copy_from_slice(&flags.to_le_bytes());
        definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
            .copy_from_slice(&data.to_le_bytes());
        definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
            .copy_from_slice(&(rowbytes as i32).to_le_bytes());
        definition[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
            .copy_from_slice(&width.to_le_bytes());
        definition[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
            .copy_from_slice(&height.to_le_bytes());
        for (offset, value) in [0, 0, width, height].into_iter().enumerate() {
            let start = abi::LAYER_EXTENT_HINT_OFFSET + offset * 4;
            definition[start..start + 4].copy_from_slice(&value.to_le_bytes());
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
        state.worlds.insert(world, NativeWorld { pixel_format });
        0
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn dispose_world(_: u64, world: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    with_state(|state| {
        let Some(_record) = state.worlds.remove(&world) else {
            return 4;
        };
        unsafe {
            ptr::write_bytes(world as *mut u8, 0, abi::PF_LAYER_DEF_SIZE);
        }
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
        if output == 0 {
            return 4;
        }
        let pixel_format = state
            .worlds
            .get(&world)
            .map(|record| record.pixel_format)
            .or_else(|| {
                (world == state.smart_input_world || world == state.smart_output_world)
                    .then_some(0x6267_7261u32 as i32)
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
        if size > MAX_HANDLE_SIZE {
            return 0;
        }
        let handle = (state.arena_next + 7) & !7;
        let data = (handle + 8 + 15) & !15;
        let Some(end) = data.checked_add(size.max(1)) else {
            return 0;
        };
        if end > state.arena_end {
            return 0;
        }
        state.arena_next = end;
        unsafe {
            *(handle as *mut u64) = data;
            ptr::write_bytes(data as *mut u8, 0, size as usize);
        }
        state.handles.insert(
            handle,
            NativeHandle {
                data,
                size,
                locks: 0,
            },
        );
        handle
    })
    .unwrap_or(0)
}

unsafe extern "win64" fn lock_handle(handle: u64, _: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    with_state(|state| {
        state.handles.get_mut(&handle).map_or(0, |record| {
            record.locks = record.locks.saturating_add(1);
            record.data
        })
    })
    .unwrap_or(0)
}

unsafe extern "win64" fn unlock_handle(handle: u64, _: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    with_state(|state| {
        if let Some(record) = state.handles.get_mut(&handle) {
            record.locks = record.locks.saturating_sub(1);
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
        state.handles.remove(&handle);
    });
    0
}

unsafe extern "win64" fn handle_size(handle: u64, _: u64, _: u64, _: u64, _: u64, _: u64) -> u64 {
    with_state(|state| state.handles.get(&handle).map_or(0, |record| record.size)).unwrap_or(0)
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
        if size > MAX_HANDLE_SIZE || handle_pointer == 0 {
            return 4;
        }
        let handle = unsafe { *(handle_pointer as *const u64) };
        let Some(old) = state.handles.get(&handle).cloned() else {
            return 4;
        };
        if old.locks != 0 {
            return 4;
        }
        let data = (state.arena_next + 15) & !15;
        let Some(end) = data.checked_add(size.max(1)) else {
            return 4;
        };
        if end > state.arena_end {
            return 4;
        }
        state.arena_next = end;
        unsafe {
            ptr::write_bytes(data as *mut u8, 0, size as usize);
            ptr::copy_nonoverlapping(
                old.data as *const u8,
                data as *mut u8,
                old.size.min(size) as usize,
            );
            *(handle as *mut u64) = data;
        }
        if let Some(record) = state.handles.get_mut(&handle) {
            record.data = data;
            record.size = size;
        }
        0
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn new_aegp_mem_handle(
    plugin_id: u64,
    what: u64,
    size: u64,
    flags: u64,
    output: u64,
    _: u64,
) -> u64 {
    with_state(|state| {
        if plugin_id != 1 || what == 0 || output == 0 || flags & !3 != 0 || size > 16 * 1024 * 1024
        {
            return 4;
        }
        let handle = (state.arena_next + 7) & !7;
        let data = (handle + 8 + 15) & !15;
        let Some(end) = data.checked_add(size.max(1)) else {
            return 4;
        };
        if end > state.arena_end {
            return 4;
        }
        state.arena_next = end;
        unsafe {
            *(handle as *mut u64) = data;
            ptr::write_bytes(data as *mut u8, 0, size as usize);
            *(output as *mut u64) = handle;
        }
        state.aegp_memory_handles.insert(
            handle,
            NativeHandle {
                data,
                size,
                locks: 0,
            },
        );
        0
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn free_aegp_mem_handle(
    handle: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_state(|state| {
        if state
            .aegp_memory_handles
            .get(&handle)
            .is_some_and(|record| record.locks == 0)
        {
            state.aegp_memory_handles.remove(&handle);
            0
        } else {
            4
        }
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn lock_aegp_mem_handle(
    handle: u64,
    output: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_state(|state| {
        if output == 0 {
            return 4;
        }
        if let Some(record) = state.aegp_memory_handles.get_mut(&handle) {
            record.locks += 1;
            unsafe {
                *(output as *mut u64) = record.data;
            }
            0
        } else {
            4
        }
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn unlock_aegp_mem_handle(
    handle: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_state(|state| {
        state
            .aegp_memory_handles
            .get_mut(&handle)
            .map_or(4, |record| {
                if record.locks == 0 {
                    4
                } else {
                    record.locks -= 1;
                    0
                }
            })
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn get_aegp_mem_handle_size(
    handle: u64,
    output: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_state(|state| {
        if output == 0 {
            return 4;
        }
        if let Some(record) = state.aegp_memory_handles.get(&handle) {
            unsafe {
                *(output as *mut u32) = record.size as u32;
            }
            0
        } else {
            4
        }
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn resize_aegp_mem_handle(
    what: u64,
    size: u64,
    handle: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_state(|state| {
        if what == 0 || size > 16 * 1024 * 1024 {
            return 4;
        }
        let Some(old) = state.aegp_memory_handles.get(&handle).cloned() else {
            return 4;
        };
        if old.locks != 0 {
            return 4;
        }
        let data = (state.arena_next + 15) & !15;
        let Some(end) = data.checked_add(size.max(1)) else {
            return 4;
        };
        if end > state.arena_end {
            return 4;
        }
        state.arena_next = end;
        unsafe {
            ptr::write_bytes(data as *mut u8, 0, size as usize);
            ptr::copy_nonoverlapping(
                old.data as *const u8,
                data as *mut u8,
                old.size.min(size) as usize,
            );
            *(handle as *mut u64) = data;
        }
        if let Some(record) = state.aegp_memory_handles.get_mut(&handle) {
            record.data = data;
            record.size = size;
        }
        0
    })
    .unwrap_or(4)
}

unsafe extern "win64" fn unsupported_aegp_memory_slot(
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    4
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn aegp_memory_v1_slots_zero_through_five_roundtrip() {
        let mut arena = vec![0u8; 0x10000];
        let mut state = NativeState {
            arena_next: arena.as_mut_ptr() as u64,
            arena_end: arena.as_ptr() as u64 + arena.len() as u64,
            ..NativeState::default()
        };
        ACTIVE_STATE.with(|slot| slot.set(&mut state));

        let label = std::ffi::CString::new("olm_test").unwrap();
        let mut handle = 0u64;
        assert_eq!(
            unsafe {
                new_aegp_mem_handle(
                    1,
                    label.as_ptr() as u64,
                    4,
                    0,
                    (&mut handle as *mut u64) as u64,
                    0,
                )
            },
            0
        );
        assert_ne!(handle, 0);

        let mut data = 0u64;
        assert_eq!(
            unsafe { lock_aegp_mem_handle(handle, (&mut data as *mut u64) as u64, 0, 0, 0, 0) },
            0
        );
        assert_ne!(data, 0);
        unsafe {
            *(data as *mut u32) = 0x1122_3344;
        }
        assert_eq!(unsafe { free_aegp_mem_handle(handle, 0, 0, 0, 0, 0) }, 4);
        assert_eq!(unsafe { unlock_aegp_mem_handle(handle, 0, 0, 0, 0, 0) }, 0);

        let mut size = 0u32;
        assert_eq!(
            unsafe { get_aegp_mem_handle_size(handle, (&mut size as *mut u32) as u64, 0, 0, 0, 0) },
            0
        );
        assert_eq!(size, 4);
        assert_eq!(
            unsafe { resize_aegp_mem_handle(label.as_ptr() as u64, 8, handle, 0, 0, 0) },
            0
        );
        let resized_data = unsafe { *(handle as *const u64) };
        assert_eq!(unsafe { *(resized_data as *const u32) }, 0x1122_3344);
        assert_eq!(unsafe { free_aegp_mem_handle(handle, 0, 0, 0, 0, 0) }, 0);
        assert_eq!(
            unsafe { get_aegp_mem_handle_size(handle, (&mut size as *mut u32) as u64, 0, 0, 0, 0) },
            4
        );

        ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
    }
}
