use aexcompat_broker::host_core::{boundary, error, handle, report, session};

#[test]
fn broker_paths_reexport_the_dependency_minimal_core_types() {
    let context: aexcompat_host_core::boundary::HostCallContext =
        boundary::HostCallContext::new(41, 73);
    let handle: aexcompat_host_core::boundary::HostOpaqueHandle =
        boundary::HostOpaqueHandle(19);
    let code: aexcompat_host_core::error::HostErrorCode = error::HostErrorCode::WrongOwner;
    let kind: aexcompat_host_core::handle::HandleKind = handle::HandleKind::Session;
    let phase: aexcompat_host_core::report::ReportPhase = report::ReportPhase::Session;
    let state: aexcompat_host_core::session::SessionState = session::SessionState::Created;

    assert_eq!(context.abi_version, boundary::HOST_CORE_ABI_VERSION);
    assert_eq!(handle.0, 19);
    assert_eq!(code as i32, 5);
    assert_eq!(kind as u8, 4);
    assert_eq!(phase as u32, 3);
    assert_eq!(state as u32, 1);
}
