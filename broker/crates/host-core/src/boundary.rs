//! Value-only C/Rust migration boundary.
//!
//! Adobe SDK-shaped tables, raw SDK pointers, and Windows SEH remain owned by
//! the C++ adapter. Rust entry points must call [`contain_panic`] before
//! returning through that adapter so Rust unwinding never crosses a C ABI.

use crate::error::{HostError, HostErrorCode};
use crate::report::HostReportSnapshot;
use std::mem::{align_of, offset_of, size_of};
use std::panic::{AssertUnwindSafe, catch_unwind};

pub const HOST_CORE_ABI_VERSION: u32 = 1;
pub const HOST_CORE_ABI_DESCRIPTOR_MAGIC: u64 = 0x4145_5848_434f_5245;
pub const HOST_CORE_CAPABILITY_SESSION_LIFECYCLE_V1: u64 = 1;

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

/// Pointer-free identity for the exact value ABI exported by the host-core
/// DLL. Native code copies and validates this value before casting any
/// function export.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostCoreAbiDescriptorV1 {
    pub magic: u64,
    pub abi_version: u32,
    pub struct_size: u32,
    pub call_context_size: u32,
    pub call_context_alignment: u32,
    pub call_status_size: u32,
    pub call_status_alignment: u32,
    pub opaque_handle_size: u32,
    pub opaque_handle_alignment: u32,
    pub report_snapshot_size: u32,
    pub report_snapshot_alignment: u32,
    pub capabilities: u64,
}

impl HostCoreAbiDescriptorV1 {
    pub const fn current() -> Self {
        Self {
            magic: HOST_CORE_ABI_DESCRIPTOR_MAGIC,
            abi_version: HOST_CORE_ABI_VERSION,
            struct_size: size_of::<Self>() as u32,
            call_context_size: size_of::<HostCallContext>() as u32,
            call_context_alignment: align_of::<HostCallContext>() as u32,
            call_status_size: size_of::<HostCallStatus>() as u32,
            call_status_alignment: align_of::<HostCallStatus>() as u32,
            opaque_handle_size: size_of::<HostOpaqueHandle>() as u32,
            opaque_handle_alignment: align_of::<HostOpaqueHandle>() as u32,
            report_snapshot_size: size_of::<HostReportSnapshot>() as u32,
            report_snapshot_alignment: align_of::<HostReportSnapshot>() as u32,
            capabilities: HOST_CORE_CAPABILITY_SESSION_LIFECYCLE_V1,
        }
    }
}

pub fn contain_panic<T>(
    operation: &'static str,
    callback: impl FnOnce() -> Result<T, HostError>,
) -> Result<T, HostError> {
    match catch_unwind(AssertUnwindSafe(callback)) {
        Ok(result) => result,
        Err(payload) => {
            dispose_caught_panic(payload);
            Err(HostError::new(HostErrorCode::Panic, operation))
        }
    }
}

fn dispose_caught_panic(payload: Box<dyn std::any::Any + Send>) {
    if let Err(drop_panic) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
        // A panic payload may itself panic from Drop. The secondary payload
        // must not be dropped on this boundary because that can unwind again.
        // Leaking this exceptional payload is the only fail-closed option.
        std::mem::forget(drop_panic);
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

    assert!(size_of::<HostCoreAbiDescriptorV1>() == 56);
    assert!(align_of::<HostCoreAbiDescriptorV1>() == 8);
    assert!(offset_of!(HostCoreAbiDescriptorV1, magic) == 0);
    assert!(offset_of!(HostCoreAbiDescriptorV1, abi_version) == 8);
    assert!(offset_of!(HostCoreAbiDescriptorV1, struct_size) == 12);
    assert!(offset_of!(HostCoreAbiDescriptorV1, call_context_size) == 16);
    assert!(offset_of!(HostCoreAbiDescriptorV1, call_context_alignment) == 20);
    assert!(offset_of!(HostCoreAbiDescriptorV1, call_status_size) == 24);
    assert!(offset_of!(HostCoreAbiDescriptorV1, call_status_alignment) == 28);
    assert!(offset_of!(HostCoreAbiDescriptorV1, opaque_handle_size) == 32);
    assert!(offset_of!(HostCoreAbiDescriptorV1, opaque_handle_alignment) == 36);
    assert!(offset_of!(HostCoreAbiDescriptorV1, report_snapshot_size) == 40);
    assert!(offset_of!(HostCoreAbiDescriptorV1, report_snapshot_alignment) == 44);
    assert!(offset_of!(HostCoreAbiDescriptorV1, capabilities) == 48);
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
    fn abi_descriptor_identifies_the_exact_shared_value_layouts() {
        let descriptor = HostCoreAbiDescriptorV1::current();
        assert_eq!(size_of::<HostCoreAbiDescriptorV1>(), 56);
        assert_eq!(align_of::<HostCoreAbiDescriptorV1>(), 8);
        assert_eq!(offset_of!(HostCoreAbiDescriptorV1, capabilities), 48);
        assert_eq!(descriptor.magic, HOST_CORE_ABI_DESCRIPTOR_MAGIC);
        assert_eq!(descriptor.abi_version, HOST_CORE_ABI_VERSION);
        assert_eq!(descriptor.struct_size, 56);
        assert_eq!(descriptor.call_context_size, 24);
        assert_eq!(descriptor.call_context_alignment, 8);
        assert_eq!(descriptor.call_status_size, 24);
        assert_eq!(descriptor.call_status_alignment, 8);
        assert_eq!(descriptor.opaque_handle_size, 8);
        assert_eq!(descriptor.opaque_handle_alignment, 8);
        assert_eq!(descriptor.report_snapshot_size, 72);
        assert_eq!(descriptor.report_snapshot_alignment, 8);
        assert_eq!(
            descriptor.capabilities,
            HOST_CORE_CAPABILITY_SESSION_LIFECYCLE_V1
        );
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
    fn panic_payload_drop_cannot_escape_the_boundary() {
        struct PanicOnDrop;

        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                panic!("panic payload drop must also be contained");
            }
        }

        let outer = catch_unwind(AssertUnwindSafe(|| {
            contain_panic::<()>("test_drop_panic", || std::panic::panic_any(PanicOnDrop))
        }));
        let result = outer.expect("contain_panic must absorb payload Drop panics");
        assert_eq!(result.unwrap_err().code(), HostErrorCode::Panic);
    }

    #[test]
    fn failure_status_rejects_success_code() {
        let status = HostCallStatus::failure(HostErrorCode::Ok, 17);
        assert_eq!(status.code, HostErrorCode::InvalidState as i32);
        assert_ne!(status.code, HostErrorCode::Ok as i32);
    }
}
