//! Value-only C/Rust migration boundary.
//!
//! Adobe SDK-shaped tables, raw SDK pointers, and Windows SEH remain owned by
//! the C++ adapter. Rust entry points must call [`contain_panic`] before
//! returning through that adapter so Rust unwinding never crosses a C ABI.

use crate::host_core::error::{HostError, HostErrorCode};
use std::mem::{align_of, offset_of, size_of};
use std::panic::{AssertUnwindSafe, catch_unwind};

pub const HOST_CORE_ABI_VERSION: u32 = 1;

/// Adapter-supplied value context. No SDK pointer or host-owned address is
/// admitted into the Rust core.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostCallContext {
    pub abi_version: u32,
    pub struct_size: u32,
    pub session_id: u64,
    pub caller_thread_token: u64,
}

impl HostCallContext {
    pub const fn new(session_id: u64, caller_thread_token: u64) -> Self {
        Self {
            abi_version: HOST_CORE_ABI_VERSION,
            struct_size: size_of::<Self>() as u32,
            session_id,
            caller_thread_token,
        }
    }

    pub fn validate(&self) -> Result<(), HostError> {
        if self.abi_version != HOST_CORE_ABI_VERSION
            || self.struct_size as usize != size_of::<Self>()
            || self.session_id == 0
            || self.caller_thread_token == 0
        {
            return Err(HostError::new(
                HostErrorCode::InvalidArgument,
                "validate_host_call_context",
            ));
        }
        Ok(())
    }
}

/// Stable value returned to the C++ adapter. Detailed diagnostics remain in a
/// value-only Rust report and are referred to by `report_id`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostCallStatus {
    pub abi_version: u32,
    pub struct_size: u32,
    pub code: i32,
    pub reserved: u32,
    pub report_id: u64,
}

impl HostCallStatus {
    pub const fn success(report_id: u64) -> Self {
        Self {
            abi_version: HOST_CORE_ABI_VERSION,
            struct_size: size_of::<Self>() as u32,
            code: HostErrorCode::Ok as i32,
            reserved: 0,
            report_id,
        }
    }

    pub const fn failure(code: HostErrorCode, report_id: u64) -> Self {
        Self {
            abi_version: HOST_CORE_ABI_VERSION,
            struct_size: size_of::<Self>() as u32,
            code: code.fail_closed() as i32,
            reserved: 0,
            report_id,
        }
    }
}

/// C-compatible opaque token. The token is never a pointer and may only be
/// resolved by the Rust registry that created it.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HostOpaqueHandle(pub u64);

pub fn contain_panic<T>(
    operation: &'static str,
    callback: impl FnOnce() -> Result<T, HostError>,
) -> Result<T, HostError> {
    match catch_unwind(AssertUnwindSafe(callback)) {
        Ok(result) => result,
        Err(_) => Err(HostError::new(HostErrorCode::Panic, operation)),
    }
}

const _: () = {
    assert!(size_of::<HostCallContext>() == 24);
    assert!(align_of::<HostCallContext>() == 8);
    assert!(offset_of!(HostCallContext, abi_version) == 0);
    assert!(offset_of!(HostCallContext, struct_size) == 4);
    assert!(offset_of!(HostCallContext, session_id) == 8);
    assert!(offset_of!(HostCallContext, caller_thread_token) == 16);

    assert!(size_of::<HostCallStatus>() == 24);
    assert!(align_of::<HostCallStatus>() == 8);
    assert!(offset_of!(HostCallStatus, abi_version) == 0);
    assert!(offset_of!(HostCallStatus, struct_size) == 4);
    assert!(offset_of!(HostCallStatus, code) == 8);
    assert!(offset_of!(HostCallStatus, reserved) == 12);
    assert!(offset_of!(HostCallStatus, report_id) == 16);

    assert!(size_of::<HostOpaqueHandle>() == 8);
    assert!(align_of::<HostOpaqueHandle>() == 8);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_core_boundary_layout_is_frozen_for_x86_64_windows() {
        assert_eq!(size_of::<HostCallContext>(), 24);
        assert_eq!(align_of::<HostCallContext>(), 8);
        assert_eq!(offset_of!(HostCallContext, session_id), 8);
        assert_eq!(offset_of!(HostCallContext, caller_thread_token), 16);
        assert_eq!(size_of::<HostCallStatus>(), 24);
        assert_eq!(offset_of!(HostCallStatus, code), 8);
        assert_eq!(offset_of!(HostCallStatus, report_id), 16);
        assert_eq!(size_of::<HostOpaqueHandle>(), 8);
    }

    #[test]
    fn invalid_context_fails_closed() {
        let mut context = HostCallContext::new(7, 9);
        assert!(context.validate().is_ok());
        context.struct_size = 0;
        assert_eq!(
            context.validate().unwrap_err().code(),
            HostErrorCode::InvalidArgument
        );
    }

    #[test]
    fn panic_is_converted_before_reaching_the_adapter() {
        let result: Result<(), HostError> =
            contain_panic("test_callback", || panic!("must not cross the boundary"));
        assert_eq!(result.unwrap_err().code(), HostErrorCode::Panic);
    }

    #[test]
    fn failure_status_rejects_success_code() {
        let status = HostCallStatus::failure(HostErrorCode::Ok, 17);
        assert_eq!(status.code, HostErrorCode::InvalidState as i32);
        assert_ne!(status.code, HostErrorCode::Ok as i32);
    }
}
