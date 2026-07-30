//! Bounded safe access to the Apple OpenCL 1.2 framework.
//!
//! AEXCompat's workers forbid unsafe code. This crate is the intentionally
//! small exception: all OpenCL FFI and handle ownership live here, while the
//! public API exposes checked ranges, bounded inputs, and RAII objects.

#![deny(unsafe_op_in_unsafe_fn)]

mod common;

#[cfg(target_os = "macos")]
mod ffi;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod unsupported;

pub use common::{
    BufferAccess, Error, GpuDevice, KernelScalar, MAX_BUFFER_BYTES, MAX_BUILD_LOG_BYTES,
    MAX_BUILD_OPTIONS_BYTES, MAX_GLOBAL_WORK_ITEMS, MAX_GPU_DEVICE_COUNT, MAX_KERNEL_NAME_BYTES,
    MAX_PLATFORM_COUNT, MAX_PROGRAM_SOURCE_BYTES, ObjectCounts, ObjectTracker,
};
#[cfg(target_os = "macos")]
pub use macos::{Buffer, Kernel, Program, Session, enumerate_gpu_devices};
#[cfg(not(target_os = "macos"))]
pub use unsupported::{Buffer, Kernel, Program, Session, enumerate_gpu_devices};
