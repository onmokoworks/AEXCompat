//! Dependency-minimal AE-compatible host state for staged native migration.
//!
//! This crate owns only value ABI, errors, opaque identity, session lifecycle,
//! and reports. Adobe SDK types, Windows SEH, worker orchestration, rendering,
//! scene semantics, and parameter transport remain outside this boundary.

pub mod boundary;
pub mod error;
pub mod handle;
pub mod report;
pub mod session;
