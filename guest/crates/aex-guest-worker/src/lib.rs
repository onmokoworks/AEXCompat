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
pub mod gpu_lifecycle;
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

fn compose_iterate_progress(
    progress_base: i32,
    progress_final: i32,
    completed_rows: i32,
    rows: i32,
) -> Result<Option<(i32, i32)>, ()> {
    let reverse_progress = progress_final < progress_base;
    let progress_span = if reverse_progress {
        i64::from(progress_base) - i64::from(progress_final)
    } else {
        i64::from(progress_final) - i64::from(progress_base)
    };
    if progress_span > i64::from(i32::MAX) {
        return Err(());
    }
    let current = if reverse_progress {
        progress_span * i64::from(completed_rows) / i64::from(rows)
    } else {
        i64::from(progress_base) + progress_span * i64::from(completed_rows) / i64::from(rows)
    };
    let total = if reverse_progress {
        progress_span as i32
    } else {
        progress_final
    };
    Ok((total > 0).then_some((current as i32, total)))
}
