use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use thiserror::Error;

pub const MAX_PLATFORM_COUNT: usize = 16;
pub const MAX_GPU_DEVICE_COUNT: usize = 32;
pub const MAX_BUFFER_BYTES: usize = 2 * 1024 * 1024 * 1024;
pub const MAX_PROGRAM_SOURCE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_BUILD_OPTIONS_BYTES: usize = 64 * 1024;
pub const MAX_BUILD_LOG_BYTES: usize = 1024 * 1024;
pub const MAX_KERNEL_NAME_BYTES: usize = 1024;
pub const MAX_GLOBAL_WORK_ITEMS: usize = 1 << 34;
#[cfg(target_os = "macos")]
pub(crate) const MAX_INFO_BYTES: usize = 64 * 1024;
#[cfg(target_os = "macos")]
pub(crate) const MAX_WORK_DIMENSIONS: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BufferAccess {
    ReadWrite,
    ReadOnly,
    WriteOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuDevice {
    ordinal: usize,
    platform_name: String,
    name: String,
    vendor: String,
    compute_units: u32,
}

impl GpuDevice {
    #[cfg(target_os = "macos")]
    pub(crate) fn new(
        ordinal: usize,
        platform_name: String,
        name: String,
        vendor: String,
        compute_units: u32,
    ) -> Self {
        Self {
            ordinal,
            platform_name,
            name,
            vendor,
            compute_units,
        }
    }

    #[must_use]
    pub const fn ordinal(&self) -> usize {
        self.ordinal
    }

    #[must_use]
    pub fn platform_name(&self) -> &str {
        &self.platform_name
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn vendor(&self) -> &str {
        &self.vendor
    }

    #[must_use]
    pub const fn compute_units(&self) -> u32 {
        self.compute_units
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum Error {
    #[error("Apple OpenCL is unsupported on this platform")]
    UnsupportedPlatform,

    #[error("no OpenCL GPU device is available")]
    NoGpuDevice,

    #[error("GPU index {requested} is out of range; {available} device(s) are available")]
    DeviceIndexOutOfRange { requested: usize, available: usize },

    #[error("{resource} exceeds its bound: {actual} > {maximum}")]
    LimitExceeded {
        resource: &'static str,
        actual: usize,
        maximum: usize,
    },

    #[error("OpenCL buffers must contain at least one byte")]
    ZeroBufferSize,

    #[error("{operation} range {offset}..{end} exceeds buffer length {buffer_len}")]
    BufferRange {
        operation: &'static str,
        offset: usize,
        end: usize,
        buffer_len: usize,
    },

    #[error("{operation} range overflowed")]
    RangeOverflow { operation: &'static str },

    #[error("{field} contains an interior NUL byte")]
    InteriorNul { field: &'static str },

    #[error("program source must not be empty")]
    EmptyProgramSource,

    #[error("work dimensions must contain between 1 and 3 entries")]
    InvalidWorkDimensions,

    #[error("global work size at dimension {dimension} must be non-zero")]
    ZeroGlobalWorkSize { dimension: usize },

    #[error("local work dimensions must match global work dimensions")]
    LocalWorkDimensionMismatch,

    #[error("local work size at dimension {dimension} must be non-zero")]
    ZeroLocalWorkSize { dimension: usize },

    #[error(
        "global work size {global} is not divisible by local work size {local} at dimension {dimension}"
    )]
    NonDivisibleLocalWorkSize {
        dimension: usize,
        global: usize,
        local: usize,
    },

    #[error("{object} belongs to another OpenCL session")]
    SessionMismatch { object: &'static str },

    #[error("OpenCL {operation} failed with status {code}")]
    Api { operation: &'static str, code: i32 },

    #[error("OpenCL {operation} failed with status {code}: {detail}")]
    ApiWithDetail {
        operation: &'static str,
        code: i32,
        detail: String,
    },

    #[error("OpenCL program build failed with status {code}: {log}")]
    ProgramBuild { code: i32, log: String },
}

impl Error {
    #[cfg(target_os = "macos")]
    pub(crate) fn api(operation: &'static str, code: i32) -> Self {
        Self::Api { operation, code }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn api_with_detail(operation: &'static str, code: i32, detail: String) -> Self {
        Self::ApiWithDetail {
            operation,
            code,
            detail,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ObjectCounts {
    pub contexts: usize,
    pub command_queues: usize,
    pub buffers: usize,
    pub programs: usize,
    pub kernels: usize,
    pub release_errors: usize,
}

impl ObjectCounts {
    #[must_use]
    pub const fn live_total(self) -> usize {
        self.contexts + self.command_queues + self.buffers + self.programs + self.kernels
    }
}

#[derive(Default)]
pub(crate) struct ObjectCounters {
    pub contexts: AtomicUsize,
    pub command_queues: AtomicUsize,
    pub buffers: AtomicUsize,
    pub programs: AtomicUsize,
    pub kernels: AtomicUsize,
    pub release_errors: AtomicUsize,
}

#[derive(Clone, Default)]
pub struct ObjectTracker {
    pub(crate) counters: Arc<ObjectCounters>,
}

impl ObjectTracker {
    #[cfg(target_os = "macos")]
    pub(crate) fn new(counters: Arc<ObjectCounters>) -> Self {
        Self { counters }
    }

    #[must_use]
    pub fn snapshot(&self) -> ObjectCounts {
        ObjectCounts {
            contexts: self.counters.contexts.load(Ordering::Acquire),
            command_queues: self.counters.command_queues.load(Ordering::Acquire),
            buffers: self.counters.buffers.load(Ordering::Acquire),
            programs: self.counters.programs.load(Ordering::Acquire),
            kernels: self.counters.kernels.load(Ordering::Acquire),
            release_errors: self.counters.release_errors.load(Ordering::Acquire),
        }
    }
}

mod scalar_sealed {
    pub trait Sealed {}
}

/// Scalar types with a stable, padding-free representation accepted by
/// `clSetKernelArg`.
pub trait KernelScalar: scalar_sealed::Sealed + Copy + 'static {}

macro_rules! kernel_scalars {
    ($($type:ty),+ $(,)?) => {
        $(
            impl scalar_sealed::Sealed for $type {}
            impl KernelScalar for $type {}
        )+
    };
}

kernel_scalars!(i8, u8, i16, u16, i32, u32, i64, u64, f32, f64, isize, usize);
