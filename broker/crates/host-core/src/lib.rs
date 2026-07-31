//! Dependency-minimal AE-compatible host state for staged native migration.
//!
//! This crate owns only value ABI, errors, opaque handles, pointer-free scene
//! identity comparison, session lifecycle, and reports. Adobe SDK types,
//! Windows SEH, worker orchestration, rendering, mutable C++ scene registry
//! ownership, and parameter transport remain outside this boundary. Immutable
//! pointer-free scene topology snapshots may be copied into Rust-owned state.

pub mod boundary;
pub mod error;
pub mod handle;
pub mod report;
pub mod scene;
pub mod session;
