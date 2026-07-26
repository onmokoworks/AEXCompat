//! Out-of-process Windows x64 AEX guest worker for Apple Silicon.
//!
//! The worker is a separate GPL-licensed process because its execution backend
//! will link Unicorn. The broker-facing protocol and generated ABI crate remain
//! independent of that backend.

#![cfg_attr(not(feature = "native-carrier"), forbid(unsafe_code))]

pub mod backend {
    #[cfg(all(feature = "native-carrier", target_os = "macos"))]
    pub use crate::native_x64::*;
    #[cfg(any(not(feature = "native-carrier"), not(target_os = "macos")))]
    pub use crate::x64::*;
}
pub mod classic;
mod crt_heap;
#[cfg(all(
    feature = "native-carrier",
    target_arch = "x86_64",
    any(target_os = "macos", all(test, target_os = "windows"))
))]
mod native_aegp_memory;
#[cfg(all(feature = "native-carrier", target_os = "macos"))]
pub mod native_x64;
pub mod pe;
pub mod pixel;
pub mod plugin_data;
pub mod resident;
pub mod x64;
