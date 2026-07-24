//! Out-of-process Windows x64 AEX guest worker for Apple Silicon.
//!
//! The worker is a separate GPL-licensed process because its execution backend
//! will link Unicorn. The broker-facing protocol and generated ABI crate remain
//! independent of that backend.

#![forbid(unsafe_code)]

pub mod classic;
pub mod pe;
pub mod x64;
