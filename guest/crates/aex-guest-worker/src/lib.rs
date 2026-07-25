//! Out-of-process Windows x64 AEX guest worker for Apple Silicon.
//!
//! The worker is a separate GPL-licensed process because its execution backend
//! will link Unicorn. The broker-facing protocol and generated ABI crate remain
//! independent of that backend.

#![cfg_attr(not(feature = "native-carrier"), forbid(unsafe_code))]

pub mod backend {
    #[cfg(feature = "native-carrier")]
    pub use crate::native_x64::*;
    #[cfg(not(feature = "native-carrier"))]
    pub use crate::x64::*;
}
pub mod classic;
#[cfg(feature = "native-carrier")]
pub mod native_x64;
pub mod pe;
pub mod resident;
pub mod x64;
