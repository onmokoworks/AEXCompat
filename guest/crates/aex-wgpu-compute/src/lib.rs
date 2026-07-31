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
    Error, MAX_BINDING_COUNT, MAX_BINDING_INDEX_EXCLUSIVE, MAX_BUFFER_BYTES, MAX_ENTRY_POINT_BYTES,
    MAX_LABEL_BYTES, MAX_METAL_ADAPTER_COUNT, MAX_PIPELINE_CONSTANT_KEY_BYTES,
    MAX_PIPELINE_CONSTANTS, MAX_SHADER_SOURCE_BYTES, MAX_STORAGE_BINDING_COUNT,
    MAX_TOTAL_BUFFER_BYTES, MAX_TOTAL_WORKGROUPS, MAX_UNIFORM_BINDING_COUNT,
    MAX_UNIFORM_BUFFER_BYTES, MAX_WORKGROUPS_PER_AXIS, NagaDispatchDescriptor, ObjectCounts,
    PipelineConstant, ValidatedNagaModule,
};
#[cfg(target_os = "macos")]
pub use macos::Session;
pub use naga;
#[cfg(not(target_os = "macos"))]
pub use unsupported::Session;
