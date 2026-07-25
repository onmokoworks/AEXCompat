//! Generated x86_64 Windows After Effects ABI constants.
//!
//! This crate deliberately contains no Rust layout recreation. The guest uses
//! byte buffers plus offsets from the same compiled SDK observation as the
//! native minihost.

#![forbid(unsafe_code)]

pub mod x86_64_windows {
    include!("generated.rs");
}
