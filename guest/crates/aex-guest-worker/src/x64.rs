use aex_abi::x86_64_windows as abi;
use iced_x86::{
    Decoder, DecoderOptions, EncodingKind, InstructionInfoFactory, Mnemonic, OpAccess, OpKind,
    Register,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};
use thiserror::Error;
use unicorn_engine::unicorn_const::{Arch, Mode, Prot};
use unicorn_engine::{Context, RegisterX86, UcHookId, Unicorn};

use crate::crt_heap::{CrtHeap, CrtHeapError, MAX_CRT_HEAP_BYTES};
use crate::pe::{PeImage, StaticTlsImage};
use crate::plugin_data::{
    CALLBACK_REJECTED, EffectRegistry, RegistrationPointers, decode_registration,
};

#[cfg(feature = "wgpu-metal-experimental")]
mod wgpu_runtime;
#[cfg(feature = "wgpu-metal-experimental")]
use wgpu_runtime::{
    WgpuArgumentExpectation, WgpuCompilerConfig, WgpuExecutor, WgpuKernel, add_artifact_evidence,
    add_dispatch_evidence, add_resource_counts, replace_resource_counts,
};

const PAGE_SIZE: u64 = 0x1000;
const STACK_BASE: u64 = 0x0000_0000_7000_0000;
const STACK_SIZE: u64 = 0x20_0000;
const MAX_WINDOWS_THREADS: usize = 32;
const STUB_BASE: u64 = 0x0000_0000_6000_0000;
const STUB_SIZE: u64 = 0x10_0000;
const STUB_STRIDE: u64 = 16;
const RETURN_ADDRESS: u64 = STUB_BASE + STUB_SIZE - PAGE_SIZE;
const HOST_ADD_PARAM: u64 = STUB_BASE + 0x80000;
const HOST_POISON: u64 = STUB_BASE + 0x80010;
const HOST_ANSI_STRCPY: u64 = STUB_BASE + 0x80020;
const HOST_COPY: u64 = STUB_BASE + 0x80030;
const HOST_NOOP: u64 = STUB_BASE + 0x80040;
const HOST_PRE_CHECKOUT_LAYER: u64 = STUB_BASE + 0x80050;
const HOST_CHECKOUT_LAYER_PIXELS: u64 = STUB_BASE + 0x80060;
const HOST_CHECKIN_LAYER_PIXELS: u64 = STUB_BASE + 0x80070;
const HOST_CHECKOUT_OUTPUT: u64 = STUB_BASE + 0x80080;
const HOST_ACQUIRE_SUITE: u64 = STUB_BASE + 0x80090;
const HOST_CHECKOUT_PARAM: u64 = STUB_BASE + 0x800a0;
const HOST_CHECKIN_PARAM: u64 = STUB_BASE + 0x800b0;
const HOST_NEW_HANDLE: u64 = STUB_BASE + 0x800c0;
const HOST_LOCK_HANDLE: u64 = STUB_BASE + 0x800d0;
const HOST_UNLOCK_HANDLE: u64 = STUB_BASE + 0x800e0;
const HOST_DISPOSE_HANDLE: u64 = STUB_BASE + 0x800f0;
const HOST_HANDLE_SIZE: u64 = STUB_BASE + 0x80100;
const HOST_RESIZE_HANDLE: u64 = STUB_BASE + 0x80110;
const HOST_AEGP_REGISTER: u64 = STUB_BASE + 0x80120;
const HOST_AEGP_GET_MAIN_WINDOW: u64 = STUB_BASE + 0x80130;
const HOST_ITERATE8: u64 = STUB_BASE + 0x80140;
const HOST_ITERATE8_CONTINUE: u64 = STUB_BASE + 0x80150;
const HOST_COLOR_PARAM_VALUE: u64 = STUB_BASE + 0x80160;
const HOST_POINT_PARAM_VALUE: u64 = STUB_BASE + 0x80170;
const HOST_AEGP_NEW_MEM_HANDLE: u64 = STUB_BASE + 0x80180;
const HOST_AEGP_FREE_MEM_HANDLE: u64 = STUB_BASE + 0x80190;
const HOST_AEGP_LOCK_MEM_HANDLE: u64 = STUB_BASE + 0x801a0;
const HOST_AEGP_UNLOCK_MEM_HANDLE: u64 = STUB_BASE + 0x801b0;
const HOST_AEGP_MEM_HANDLE_SIZE: u64 = STUB_BASE + 0x801c0;
const HOST_AEGP_RESIZE_MEM_HANDLE: u64 = STUB_BASE + 0x801d0;
const HOST_AEGP_MEMORY_UNSUPPORTED: u64 = STUB_BASE + 0x801e0;
const HOST_PLUGIN_DATA_V2: u64 = STUB_BASE + 0x801f0;
const HOST_PLUGIN_DATA_V1: u64 = STUB_BASE + 0x80200;
const HOST_NEW_WORLD: u64 = STUB_BASE + 0x80210;
const HOST_DISPOSE_WORLD: u64 = STUB_BASE + 0x80220;
const HOST_GET_WORLD_PIXEL_FORMAT: u64 = STUB_BASE + 0x80230;
const HOST_PF_ANSI_ATAN: u64 = STUB_BASE + 0x80240;
const HOST_PF_ANSI_ATAN2: u64 = STUB_BASE + 0x80250;
const HOST_PF_ANSI_CEIL: u64 = STUB_BASE + 0x80260;
const HOST_PF_ANSI_COS: u64 = STUB_BASE + 0x80270;
const HOST_PF_ANSI_EXP: u64 = STUB_BASE + 0x80280;
const HOST_PF_ANSI_FABS: u64 = STUB_BASE + 0x80290;
const HOST_PF_ANSI_FLOOR: u64 = STUB_BASE + 0x802a0;
const HOST_PF_ANSI_FMOD: u64 = STUB_BASE + 0x802b0;
const HOST_PF_ANSI_HYPOT: u64 = STUB_BASE + 0x802c0;
const HOST_PF_ANSI_LOG: u64 = STUB_BASE + 0x802d0;
const HOST_PF_ANSI_LOG10: u64 = STUB_BASE + 0x802e0;
const HOST_PF_ANSI_POW: u64 = STUB_BASE + 0x802f0;
const HOST_PF_ANSI_SIN: u64 = STUB_BASE + 0x80300;
const HOST_PF_ANSI_SQRT: u64 = STUB_BASE + 0x80310;
const HOST_PF_ANSI_TAN: u64 = STUB_BASE + 0x80320;
const HOST_PF_ANSI_SPRINTF: u64 = STUB_BASE + 0x80330;
const HOST_PF_ANSI_STRCPY: u64 = STUB_BASE + 0x80340;
const HOST_PF_ANSI_ASIN: u64 = STUB_BASE + 0x80350;
const HOST_PF_ANSI_ACOS: u64 = STUB_BASE + 0x80360;
const HOST_PF_ANSI_STRCPY_BOUNDED: u64 = STUB_BASE + 0x80370;
const HOST_EXTENDED_ALLOC: u64 = STUB_BASE + 0x80380;
const HOST_EXTENDED_FREE: u64 = STUB_BASE + 0x80390;
const HOST_EXTENDED_LOOKUP: u64 = STUB_BASE + 0x803a0;
const HOST_ITERATE8_ORIGIN: u64 = STUB_BASE + 0x803b0;
const HOST_FILL8: u64 = STUB_BASE + 0x803c0;
const HOST_NEW_WORLD8: u64 = STUB_BASE + 0x803d0;
const HOST_GET_CALLBACK_ADDR: u64 = STUB_BASE + 0x803e0;
const HOST_ZERO_PIXEL: u64 = STUB_BASE + 0x803f0;
const HOST_SUBPIXEL_SAMPLE8: u64 = STUB_BASE + 0x80400;
const HOST_AREA_SAMPLE8: u64 = STUB_BASE + 0x80410;
const HOST_TRANSFER_RECT8: u64 = STUB_BASE + 0x80420;
const HOST_ITERATE16: u64 = STUB_BASE + 0x80430;
const HOST_ITERATE16_CONTINUE: u64 = STUB_BASE + 0x80440;
const HOST_ITERATE_FLOAT: u64 = STUB_BASE + 0x804d0;
const HOST_ITERATE_FLOAT_CONTINUE: u64 = STUB_BASE + 0x804e0;
const HOST_BLEND: u64 = STUB_BASE + 0x80450;
const HOST_CRT_INITTERM_CONTINUE: u64 = STUB_BASE + 0x80470;
const HOST_INITIALIZE_CONDITION_VARIABLE: u64 = STUB_BASE + 0x80480;
const HOST_SLEEP_CONDITION_VARIABLE_CS: u64 = STUB_BASE + 0x80490;
const HOST_WAKE_CONDITION_VARIABLE: u64 = STUB_BASE + 0x804a0;
const HOST_WAKE_ALL_CONDITION_VARIABLE: u64 = STUB_BASE + 0x804b0;
const HOST_FLS_FREE_CONTINUE: u64 = STUB_BASE + 0x804c0;
const HOST_ENVIRONMENT_VALUE: u64 = STUB_BASE + 0x80500;
const HOST_AEGP_COMPUTE_CACHE_CALLBACKS: [u64; 6] = [
    STUB_BASE + 0x80510,
    STUB_BASE + 0x80520,
    STUB_BASE + 0x80530,
    STUB_BASE + 0x80540,
    STUB_BASE + 0x80550,
    STUB_BASE + 0x80560,
];
const HOST_DYNAMIC_FLS_ALLOC: u64 = STUB_BASE + 0x80570;
const HOST_REGISTER_UI: u64 = STUB_BASE + 0x80590;
const HOST_CREATE_THREAD_CONTINUE: u64 = STUB_BASE + 0x80580;
const WINDOWS_KERNEL32_MODULE_TOKEN: u64 = STUB_BASE + 0x8f000;
const WINDOWS_NTDLL_MODULE_TOKEN: u64 = STUB_BASE + 0x8f180;
const WINDOWS_STANDARD_INPUT_TOKEN: u64 = STUB_BASE + 0x8f100;
const WINDOWS_STANDARD_OUTPUT_TOKEN: u64 = STUB_BASE + 0x8f110;
const WINDOWS_STANDARD_ERROR_TOKEN: u64 = STUB_BASE + 0x8f120;
const WINDOWS_THREAD_HANDLE_BASE: u64 = STUB_BASE + 0x8f200;
const WINDOWS_THREAD_STACK_BASE: u64 = 0x0000_0000_6800_0000;
const WINDOWS_THREAD_STACK_STRIDE: u64 = STACK_SIZE + PAGE_SIZE;
const HOST_GPU_GET_DEVICE_COUNT: u64 = STUB_BASE + 0x80600;
const HOST_GPU_GET_DEVICE_INFO: u64 = STUB_BASE + 0x80610;
const HOST_GPU_ACQUIRE_EXCLUSIVE: u64 = STUB_BASE + 0x80620;
const HOST_GPU_RELEASE_EXCLUSIVE: u64 = STUB_BASE + 0x80630;
const HOST_GPU_ALLOCATE_DEVICE: u64 = STUB_BASE + 0x80640;
const HOST_GPU_FREE_DEVICE: u64 = STUB_BASE + 0x80650;
const HOST_GPU_PURGE_DEVICE: u64 = STUB_BASE + 0x80660;
const HOST_GPU_ALLOCATE_HOST: u64 = STUB_BASE + 0x80670;
const HOST_GPU_FREE_HOST: u64 = STUB_BASE + 0x80680;
const HOST_GPU_PURGE_HOST: u64 = STUB_BASE + 0x80690;
const HOST_GPU_CREATE_WORLD: u64 = STUB_BASE + 0x806a0;
const HOST_GPU_DISPOSE_WORLD: u64 = STUB_BASE + 0x806b0;
const HOST_GPU_GET_WORLD_DATA: u64 = STUB_BASE + 0x806c0;
const HOST_GPU_GET_WORLD_SIZE: u64 = STUB_BASE + 0x806d0;
const HOST_GPU_GET_WORLD_DEVICE_INDEX: u64 = STUB_BASE + 0x806e0;
const HOST_GPU_SUITE_CALLBACKS: [u64; 15] = [
    HOST_GPU_GET_DEVICE_COUNT,
    HOST_GPU_GET_DEVICE_INFO,
    HOST_GPU_ACQUIRE_EXCLUSIVE,
    HOST_GPU_RELEASE_EXCLUSIVE,
    HOST_GPU_ALLOCATE_DEVICE,
    HOST_GPU_FREE_DEVICE,
    HOST_GPU_PURGE_DEVICE,
    HOST_GPU_ALLOCATE_HOST,
    HOST_GPU_FREE_HOST,
    HOST_GPU_PURGE_HOST,
    HOST_GPU_CREATE_WORLD,
    HOST_GPU_DISPOSE_WORLD,
    HOST_GPU_GET_WORLD_DATA,
    HOST_GPU_GET_WORLD_SIZE,
    HOST_GPU_GET_WORLD_DEVICE_INDEX,
];
const MAX_SMART_CHECKOUT_IDS: usize = 64;
const HOST_HANDLE_SUITE: u64 = STUB_BASE + 0x81000;
const HOST_ITERATE8_SUITE: u64 = STUB_BASE + 0x81100;
const HOST_ITERATE16_SUITE: u64 = STUB_BASE + 0x81180;
const HOST_ITERATE_FLOAT_SUITE: u64 = STUB_BASE + 0x81188;
const HOST_COLOR_PARAM_SUITE: u64 = STUB_BASE + 0x81200;
const HOST_POINT_PARAM_SUITE: u64 = STUB_BASE + 0x81300;
const HOST_AEGP_MEMORY_SUITE: u64 = STUB_BASE + 0x81400;
const HOST_WORLD_SUITE: u64 = STUB_BASE + 0x81500;
const HOST_PF_ANSI_SUITE_V2: u64 = STUB_BASE + 0x81600;
const HOST_GPU_DEVICE_SUITE_V1: u64 = STUB_BASE + 0x81700;
const HOST_AEGP_COMPUTE_CACHE_SUITE_V1: u64 = STUB_BASE + 0x81800;
const HOST_AEGP_UTILITY_TABLES: u64 = STUB_BASE + 0x82000;
const HOST_AEGP_UNSUPPORTED_STUBS: u64 = STUB_BASE + 0x83000;
const HOST_ITERATE8_UNSUPPORTED_STUBS: u64 = STUB_BASE + 0x88000;
const DATA_BASE: u64 = 0x0000_0000_4000_0000;
const DATA_SIZE: u64 = 0x1000_0000;
const HANDLE_DATA_BASE: u64 = DATA_BASE + 0x400_0000;
const HANDLE_DATA_END: u64 = DATA_BASE + DATA_SIZE;
const AEGP_MEMORY_HANDLE_BASE: u64 = STUB_BASE + 0x90000;
const MAX_AEGP_MEMORY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_AEGP_MEMORY_HANDLES: usize = 256;
const PF_HANDLE_DATA_BASE: u64 = 0x0000_0001_0000_0000;
const PF_HANDLE_DATA_END: u64 = PF_HANDLE_DATA_BASE + 0x2_0000_0000;
const MAX_PF_HANDLE_SIZE: u64 = 0x8000_0000;
const MAX_PF_HANDLE_COUNT: usize = 16_384;
const WORLD_DATA_BASE: u64 = PF_HANDLE_DATA_END;
const WORLD_DATA_END: u64 = WORLD_DATA_BASE + 0x2_0000_0000;
const CRT_HEAP_BASE: u64 = 0x0000_0010_0000_0000;
const CRT_HEAP_END: u64 = CRT_HEAP_BASE + MAX_CRT_HEAP_BYTES;
const ENVIRONMENT_STRINGS_BASE: u64 = 0x0000_0020_0000_0000;
const ENVIRONMENT_STRINGS_NAMESPACE_SIZE: u64 = MAX_CRT_HEAP_BYTES;
const ENVIRONMENT_STRINGS_END: u64 = 0x0000_7fff_0000_0000;
static NEXT_ENVIRONMENT_STRINGS_NAMESPACE: AtomicU64 = AtomicU64::new(0);
static NEXT_PRIVATE_HEAP_TOKEN: AtomicU64 = AtomicU64::new(1);
const PRIVATE_HEAP_TOKEN_BASE: u64 = 0x0000_7ffe_0000_0000;
const MAX_WORLD_SIZE: u64 = 128 * 1024 * 1024;
const MAX_WORLD_COUNT: usize = 256;
const MAX_WORLD_DIMENSION: i32 = 32_768;
const MAX_ITERATE_PIXELS: i64 = 16_777_216;
// A nonzero Unicorn instruction limit enables instruction counting across the
// whole run, which is prohibitively expensive for image kernels. The wall-clock
// timeout and return-sentinel check still bound and validate guest execution.
const MAX_INSTRUCTIONS: usize = 0;
const TIMEOUT_MICROSECONDS: u64 = 600_000_000;
const MAX_TRACE_EVENTS: usize = 50_000;
const MAX_TRACE_BASIC_BLOCKS: usize = 50_000;
const MAX_TRACE_BRANCH_EDGES: usize = 100_000;
const TRACE_STACK_ARGUMENTS: usize = MAX_WIN64_IMPORT_ARGUMENTS - 4;
const TRACE_FIRST_SAMPLES: usize = 3;
const TRACE_LAST_SAMPLES: usize = 3;
const TRACE_DISTINCT_SAMPLES: usize = 16;
const TRACE_DISTINCT_FINGERPRINTS: usize = 4096;
const MAX_TRACE_WATCH_BYTES: usize = 4096;
const MAX_TRACE_WITNESSES: usize = 256;
const MAX_UNSUPPORTED_SUITE_CALLS: usize = 256;
const MAX_SUITE_REQUESTS: usize = 256;
const MAX_AVX_FALLBACK_INSTRUCTIONS: u64 = 1_000_000;
// Small images keep precise one-address hooks. Large executable sections use
// one range hook plus a bounded address map, avoiding hundreds of thousands of
// Unicorn hook objects. The independent hard point budget is 1/512 of the
// maximum accepted 256 MiB PE image size and bounds retained map memory even
// when a smaller executable section contains dense or false-positive decodes.
const MAX_SPARSE_AVX_STATE_SYNC_HOOKS: usize = 4_096;
const MAX_AVX_STATE_SYNC_POINTS: usize = 512 * 1_024;
const MAX_VCOMP_REQUESTED_THREADS: i32 = 1_024;
const MAX_CRT_MEMORY_COPY_BYTES: u64 = 128 * 1024 * 1024;
const CRT_MEMORY_COPY_CHUNK: usize = 64 * 1024;
const MAX_CRT_STRING_BYTES: u64 = 1024 * 1024;
const MAX_CRT_STDIO_BUFFER_BYTES: u64 = 1024 * 1024;
const MAX_CRT_STDIO_FORMAT_BYTES: u64 = 4 * 1024;
const MAX_CRT_STDIO_ARGUMENTS: usize = 32;
const MAX_CRT_INITIALIZERS: usize = 4096;
const MAX_CRT_ONEXIT_TABLES: usize = 64;
const WINDOWS_CRITICAL_SECTION_BYTES: usize = 40;
// Shared across the primary image and every loaded runtime DLL. Real DLL
// sets keep hundreds of startup locks alive; retain a finite resource bound.
const MAX_WINDOWS_CRITICAL_SECTIONS: usize = 4096;
const MAX_WINDOWS_CRITICAL_SECTION_RECURSION: u32 = 1024;
const MAX_WINDOWS_SRW_LOCKS: usize = 256;
const MAX_WINDOWS_SRW_WAITERS: usize = MAX_WINDOWS_THREADS;
const MAX_WINDOWS_CONDITION_VARIABLES: usize = 256;
const MAX_WINDOWS_ADDRESS_WAIT_LOCATIONS: usize = 256;
const MAX_WINDOWS_ADDRESS_WAITERS_PER_LOCATION: usize = 32;
const MAX_WINDOWS_FLS_SLOTS: u32 = 128;
const MAX_WINDOWS_TLS_SLOTS: u32 = 128;
const MAX_WINDOWS_ENVIRONMENT_NAME_BYTES: usize = 255;
const MAX_PROCESS_PRNG_BYTES: u64 = 1024 * 1024;
const PROCESS_HEAP_HANDLE: u64 = 0x0000_0000_AE70_0001;
const HEAP_NO_SERIALIZE: u32 = 0x0000_0001;
const HEAP_GENERATE_EXCEPTIONS: u32 = 0x0000_0004;
const HEAP_ZERO_MEMORY: u32 = 0x0000_0008;
const HEAP_REALLOC_IN_PLACE_ONLY: u32 = 0x0000_0010;
const HEAP_ALLOC_ALLOWED_FLAGS: u32 = HEAP_NO_SERIALIZE | HEAP_ZERO_MEMORY;
const HEAP_REALLOC_ALLOWED_FLAGS: u32 =
    HEAP_NO_SERIALIZE | HEAP_ZERO_MEMORY | HEAP_REALLOC_IN_PLACE_ONLY;
const ERROR_ENVVAR_NOT_FOUND: u32 = 203;
const ERROR_FILENAME_EXCED_RANGE: u32 = 206;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_INVALID_HANDLE: u32 = 6;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_PATH_NOT_FOUND: u32 = 3;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_NOT_SUPPORTED: u32 = 50;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const ERROR_MOD_NOT_FOUND: u32 = 126;
const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
const MAX_WINDOWS_MODULE_REFERENCES: u32 = 1_000_000;
const ERROR_PROC_NOT_FOUND: u32 = 127;
const HRESULT_E_INVALIDARG: u32 = 0x8007_0057;
const WINDOWS_MAX_PATH_BYTES: u64 = 260;
const MSVCP_MUTEX_TRY: u32 = 0x02;
const MSVCP_MUTEX_RECURSIVE: u32 = 0x100;
const OBSERVED_MSVCP_MUTEX_TYPE: u32 = MSVCP_MUTEX_TRY | MSVCP_MUTEX_RECURSIVE;
const MSVCP_MUTEX_BYTES: usize = 80;
const MAX_MSVCP_MUTEXES: usize = 256;
const MAX_MSVCP_MUTEX_RECURSION: u32 = 1024;
const MSVCP_THRD_BUSY: u32 = 3;
const MSVCP_EXCEPTION_PTR_BYTES: usize = 16;
const VCRUNTIME_EXCEPTION_DATA_BYTES: usize = 16;

pub(crate) fn utility_suite_layout(version: u32) -> Option<(usize, usize, usize)> {
    match version {
        3 => Some((9, 7, 8)),
        7 => Some((25, 7, 8)),
        11 => Some((31, 8, 9)),
        13 => Some((33, 9, 10)),
        _ => None,
    }
}

fn utility_suite_table_address(version: u32) -> Option<u64> {
    match version {
        3 => Some(HOST_AEGP_UTILITY_TABLES),
        7 => Some(HOST_AEGP_UTILITY_TABLES + 0x100),
        11 => Some(HOST_AEGP_UTILITY_TABLES + 0x200),
        13 => Some(HOST_AEGP_UTILITY_TABLES + 0x300),
        _ => None,
    }
}

fn unsupported_suite_stub_address(version: u32, slot: usize) -> Option<u64> {
    let version_index = match version {
        3 => 0,
        7 => 1,
        11 => 2,
        13 => 3,
        _ => return None,
    };
    Some(HOST_AEGP_UNSUPPORTED_STUBS + version_index * 0x1000 + slot as u64 * STUB_STRIDE)
}

fn iterate8_suite_table_address(version: u64) -> Option<u64> {
    match version {
        1 => Some(HOST_ITERATE8_SUITE),
        2 => Some(HOST_ITERATE8_SUITE + 0x40),
        _ => None,
    }
}

fn typed_iterate_suite_table_address(name: &str, version: u64) -> Option<u64> {
    match (name, version) {
        ("PF iterate16 Suite", 1) => Some(HOST_ITERATE16_SUITE),
        ("PF iterateFloat Suite", 1) => Some(HOST_ITERATE_FLOAT_SUITE),
        _ => None,
    }
}

#[derive(Debug, Error)]
pub enum GuestError {
    #[error("unicorn error during {operation}: {detail}")]
    Unicorn {
        operation: &'static str,
        detail: String,
    },
    #[error("mapped PE image is not page aligned")]
    ImageAlignment,
    #[error("invalid PE memory protection policy: {0}")]
    ImageProtection(String),
    #[error("import stub capacity exceeded")]
    StubCapacity,
    #[error("IAT entry is outside the mapped image")]
    IatRange,
    #[error("unsupported Win64 import: {library}!{symbol}")]
    UnsupportedImport { library: String, symbol: String },
    #[error("guest data arena exhausted")]
    DataCapacity,
    #[error("native AVX state sync point capacity exceeded ({observed} > {limit})")]
    AvxStateCapacity { observed: usize, limit: usize },
    #[error("guest callback failed: {0}")]
    Callback(String),
    #[error(
        "guest selector aborted with {error} after unsupported suite {suite_name} v{suite_version} (acquire error {acquire_error})"
    )]
    SelectorAbort {
        error: i32,
        suite_name: String,
        suite_version: u64,
        acquire_error: i32,
    },
    #[error("DLL process attach returned FALSE")]
    DllProcessAttach,
    #[error("TLS process attach callback {index} at {address:#x} failed: {detail}")]
    TlsProcessAttach {
        index: usize,
        address: u64,
        detail: String,
    },
    #[error("guest execution failed: {reason}; crash_snapshot={snapshot_json}")]
    ExecutionCrash {
        reason: String,
        snapshot_json: String,
        snapshot: Box<TraceCrashSnapshot>,
    },
}

impl GuestError {
    pub fn diagnostic_category(&self) -> &'static str {
        match self {
            Self::Unicorn { .. } => "emulation",
            Self::ImageAlignment | Self::ImageProtection(_) | Self::AvxStateCapacity { .. } => {
                "image"
            }
            Self::StubCapacity | Self::IatRange | Self::UnsupportedImport { .. } => "import",
            Self::DataCapacity => "memory",
            Self::Callback(_) => "callback",
            Self::SelectorAbort { .. } => "selector",
            Self::DllProcessAttach | Self::TlsProcessAttach { .. } => "dllmain",
            Self::ExecutionCrash { .. } => "crash",
        }
    }

    pub fn diagnostic_message(&self) -> String {
        match self {
            Self::ExecutionCrash { reason, .. } => reason.clone(),
            _ => self.to_string(),
        }
    }

    pub fn crash_reason(&self) -> Option<&str> {
        match self {
            Self::ExecutionCrash { reason, .. } => Some(reason),
            _ => None,
        }
    }

    pub fn crash_snapshot(&self) -> Option<serde_json::Value> {
        match self {
            Self::ExecutionCrash { snapshot, .. } => serde_json::to_value(snapshot).ok(),
            _ => None,
        }
    }
}

fn uc<T>(
    operation: &'static str,
    result: Result<T, unicorn_engine::unicorn_const::uc_error>,
) -> Result<T, GuestError> {
    result.map_err(|error| GuestError::Unicorn {
        operation,
        detail: error.to_string(),
    })
}

include!("x64/trace.rs");
include!("x64/gpu_runtime.rs");
include!("x64/gpu_suite.rs");
include!("x64/types.rs");
include!("x64/imports.rs");
include!("x64/opencl_imports.rs");
include!("x64/engine.rs");
include!("x64/libraries.rs");
include!("x64/callbacks.rs");
include!("x64/iterate_and_suites.rs");
include!("x64/tail.rs");

#[cfg(test)]
mod tests {
    include!("x64/tests_support.rs");
    include!("x64/tests_area_sample.rs");
    include!("x64/tests_ansi_callbacks.rs");
    include!("x64/tests_transfer_rect.rs");
    include!("x64/tests_register_ui.rs");
    include!("x64/tests_cases.rs");
    include!("x64/tests_issue1077.rs");
    include!("x64/tests_gpu.rs");
}

include!("x64/lockit.rs");

include!("x64/import_data.rs");

include!("x64/environment.rs");

include!("x64/files.rs");

include!("x64/mutex.rs");

include!("x64/sid.rs");

include!("x64/acl.rs");
include!("x64/network.rs");

include!("x64/printf.rs");
include!("x64/windows_files.rs");
