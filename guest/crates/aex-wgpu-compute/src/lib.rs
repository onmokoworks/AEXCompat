//! Bounded, backend-specific compute access through wgpu.
//!
//! The public surface deliberately accepts generic WGSL and storage-buffer
//! bindings. OpenCL-C translation and AEX-specific kernel semantics belong in
//! the compatibility layer above this crate.

#![deny(unsafe_code)]

mod common;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod unsupported;

pub use common::{
    AdapterReport, BindingAccess, BufferBinding, BufferOutput, DispatchDescriptor, DispatchReport,
    Error, MAX_BINDING_COUNT, MAX_BUFFER_BYTES, MAX_ENTRY_POINT_BYTES, MAX_LABEL_BYTES,
    MAX_SHADER_SOURCE_BYTES, MAX_TOTAL_BUFFER_BYTES, MAX_TOTAL_WORKGROUPS, MAX_WORKGROUPS_PER_AXIS,
    ObjectCounts,
};
#[cfg(target_os = "macos")]
pub use macos::Session;
#[cfg(not(target_os = "macos"))]
pub use unsupported::Session;
