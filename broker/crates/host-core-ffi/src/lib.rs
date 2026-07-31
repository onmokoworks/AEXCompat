//! Thin value-only C ABI for the staged Rust host core.
//!
//! Adobe SDK-shaped data, pointer validity, and Windows SEH containment remain
//! C++ responsibilities. This layer validates nulls and value contracts, owns
//! Rust session handles, and prevents Rust unwinding from crossing `extern "C"`.

use aexcompat_host_core::boundary::{
    HOST_CORE_ABI_VERSION, HostCallContext, HostCallStatus, HostCoreAbiDescriptorV1,
    HostOpaqueHandle, contain_panic,
};
use aexcompat_host_core::error::{HostError, HostErrorCode};
use aexcompat_host_core::handle::{HandleKind, HandleRegistry, OwnerId};
use aexcompat_host_core::report::{HostReport, HostReportSnapshot, ReportCounters, ReportPhase};
use aexcompat_host_core::scene::{
    HostSceneIdentity, HostSceneIdentityAbiDescriptorV1, HostSceneOwnerRelation,
    HostSceneOwnerRelationAbiDescriptorV1,
};
use aexcompat_host_core::session::{HostSession, SessionState};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

#[unsafe(export_name = "aex_host_core_abi_descriptor_v1")]
pub static AEX_HOST_CORE_ABI_DESCRIPTOR_V1: HostCoreAbiDescriptorV1 =
    HostCoreAbiDescriptorV1::current();

#[unsafe(export_name = "aex_host_core_scene_identity_abi_descriptor_v1")]
pub static AEX_HOST_CORE_SCENE_IDENTITY_ABI_DESCRIPTOR_V1: HostSceneIdentityAbiDescriptorV1 =
    HostSceneIdentityAbiDescriptorV1::current();

#[unsafe(export_name = "aex_host_core_scene_owner_relation_abi_descriptor_v1")]
pub static AEX_HOST_CORE_SCENE_OWNER_RELATION_ABI_DESCRIPTOR_V1:
    HostSceneOwnerRelationAbiDescriptorV1 = HostSceneOwnerRelationAbiDescriptorV1::current();

struct SessionRecord {
    session: HostSession,
    caller_thread_token: u64,
}

#[derive(Default)]
struct AdapterState {
    sessions: HandleRegistry<SessionRecord>,
}

#[derive(Clone, Copy)]
struct CallObservation {
    session_state: Option<SessionState>,
    handle_kind: Option<HandleKind>,
    created_handle: Option<HostOpaqueHandle>,
}

enum CallOutcome {
    Passed(CallObservation),
    Failed {
        error: HostError,
        session_state: Option<SessionState>,
        handle_kind: Option<HandleKind>,
    },
}

#[derive(Clone, Copy)]
enum SessionOperation {
    Open,
    BeginCallback,
    EndCallback,
    Close,
}

static ADAPTER_STATE: OnceLock<Mutex<AdapterState>> = OnceLock::new();
static NEXT_REPORT_ID: AtomicU64 = AtomicU64::new(1);
static HANDLES_CREATED: AtomicU64 = AtomicU64::new(0);
static HANDLES_DISPOSED: AtomicU64 = AtomicU64::new(0);
static CALLBACKS_ATTEMPTED: AtomicU64 = AtomicU64::new(0);
static CALLBACKS_COMPLETED: AtomicU64 = AtomicU64::new(0);

fn adapter_state() -> &'static Mutex<AdapterState> {
    ADAPTER_STATE.get_or_init(|| Mutex::new(AdapterState::default()))
}

fn lock_state(operation: &'static str) -> Result<MutexGuard<'static, AdapterState>, HostError> {
    adapter_state()
        .lock()
        .map_err(|_| HostError::new(HostErrorCode::InvalidState, operation))
}

fn next_report_id() -> Result<u64, HostError> {
    NEXT_REPORT_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            next.checked_add(1)
        })
        .map_err(|_| HostError::new(HostErrorCode::CapacityExceeded, "allocate_report_id"))
}

fn counters() -> ReportCounters {
    ReportCounters {
        handles_created: HANDLES_CREATED.load(Ordering::Relaxed),
        handles_disposed: HANDLES_DISPOSED.load(Ordering::Relaxed),
        callbacks_attempted: CALLBACKS_ATTEMPTED.load(Ordering::Relaxed),
        callbacks_completed: CALLBACKS_COMPLETED.load(Ordering::Relaxed),
    }
}

fn passed(
    session_state: Option<SessionState>,
    created_handle: Option<HostOpaqueHandle>,
) -> CallOutcome {
    CallOutcome::Passed(CallObservation {
        session_state,
        handle_kind: Some(HandleKind::Session),
        created_handle,
    })
}

fn failed(error: HostError, session_state: Option<SessionState>) -> CallOutcome {
    CallOutcome::Failed {
        error,
        session_state,
        handle_kind: Some(HandleKind::Session),
    }
}

unsafe fn read_context(
    context: *const HostCallContext,
    operation: &'static str,
) -> Result<HostCallContext, HostError> {
    if context.is_null() {
        return Err(HostError::new(HostErrorCode::InvalidArgument, operation));
    }
    // SAFETY: The thin C++ adapter owns pointer validity and places this read
    // inside its SEH frame. Rust validates null and the copied value contract.
    let context = unsafe { context.read() };
    context.validate()?;
    Ok(context)
}

unsafe fn read_scene_identity(
    identity: *const HostSceneIdentity,
    operation: &'static str,
) -> Result<HostSceneIdentity, HostError> {
    if identity.is_null() {
        return Err(HostError::new(HostErrorCode::InvalidArgument, operation));
    }
    // SAFETY: The thin C++ adapter owns pointer validity and places this read
    // inside its SEH frame. Rust validates the copied integer-only value.
    let identity = unsafe { identity.read() };
    identity.validate()?;
    Ok(identity)
}

unsafe fn read_scene_owner_relation(
    relation: *const HostSceneOwnerRelation,
    operation: &'static str,
) -> Result<HostSceneOwnerRelation, HostError> {
    if relation.is_null() {
        return Err(HostError::new(HostErrorCode::InvalidArgument, operation));
    }
    // SAFETY: The thin C++ adapter owns pointer validity and places this read
    // inside its SEH frame. Rust validates the copied pointer-free relation.
    let relation = unsafe { relation.read() };
    relation.validate()?;
    Ok(relation)
}

fn create_session(context: HostCallContext) -> CallOutcome {
    let owner = match OwnerId::new(context.session_id) {
        Ok(owner) => owner,
        Err(error) => return failed(error, None),
    };
    let mut state = match lock_state("create_ffi_session") {
        Ok(state) => state,
        Err(error) => return failed(error, None),
    };
    let record = SessionRecord {
        session: HostSession::new(),
        caller_thread_token: context.caller_thread_token,
    };
    match state.sessions.insert(owner, HandleKind::Session, record) {
        Ok(handle) => {
            HANDLES_CREATED.fetch_add(1, Ordering::Relaxed);
            passed(Some(SessionState::Created), Some(handle))
        }
        Err(error) => failed(error, None),
    }
}

fn call_session(
    context: HostCallContext,
    handle: HostOpaqueHandle,
    operation: SessionOperation,
) -> CallOutcome {
    let owner = match OwnerId::new(context.session_id) {
        Ok(owner) => owner,
        Err(error) => return failed(error, None),
    };
    let mut state = match lock_state("call_ffi_session") {
        Ok(state) => state,
        Err(error) => return failed(error, None),
    };
    let record = match state.sessions.get_mut(handle, owner, HandleKind::Session) {
        Ok(record) => record,
        Err(error) => return failed(error, None),
    };
    let before = record.session.state();
    if record.caller_thread_token != context.caller_thread_token {
        return failed(
            HostError::new(HostErrorCode::WrongThread, "validate_caller_thread_token"),
            Some(before),
        );
    }
    if let Err(error) = record.session.require_thread("validate_caller_thread") {
        return failed(error, Some(before));
    }
    let result = match operation {
        SessionOperation::Open => record.session.open(),
        SessionOperation::BeginCallback => record.session.begin_callback(),
        SessionOperation::EndCallback => record.session.end_callback(),
        SessionOperation::Close => record.session.close(),
    };
    match result {
        Ok(()) => {
            if matches!(operation, SessionOperation::BeginCallback) {
                CALLBACKS_ATTEMPTED.fetch_add(1, Ordering::Relaxed);
            } else if matches!(operation, SessionOperation::EndCallback) {
                CALLBACKS_COMPLETED.fetch_add(1, Ordering::Relaxed);
            }
            passed(Some(record.session.state()), None)
        }
        Err(error) => failed(error, Some(record.session.state())),
    }
}

fn dispose_session(context: HostCallContext, handle: HostOpaqueHandle) -> CallOutcome {
    let owner = match OwnerId::new(context.session_id) {
        Ok(owner) => owner,
        Err(error) => return failed(error, None),
    };
    let mut state = match lock_state("dispose_ffi_session") {
        Ok(state) => state,
        Err(error) => return failed(error, None),
    };
    let terminal_state = {
        let record = match state.sessions.get_mut(handle, owner, HandleKind::Session) {
            Ok(record) => record,
            Err(error) => return failed(error, None),
        };
        let observed = record.session.state();
        if record.caller_thread_token != context.caller_thread_token {
            return failed(
                HostError::new(HostErrorCode::WrongThread, "validate_caller_thread_token"),
                Some(observed),
            );
        }
        if let Err(error) = record.session.require_thread("validate_caller_thread") {
            return failed(error, Some(observed));
        }
        if !matches!(observed, SessionState::Closed | SessionState::Faulted) {
            return failed(
                HostError::new(HostErrorCode::InvalidState, "dispose_ffi_session"),
                Some(observed),
            );
        }
        observed
    };
    match state.sessions.remove(handle, owner, HandleKind::Session) {
        Ok(_) => {
            HANDLES_DISPOSED.fetch_add(1, Ordering::Relaxed);
            passed(Some(terminal_state), None)
        }
        Err(error) => failed(error, None),
    }
}

unsafe fn write_outcome(
    status: *mut HostCallStatus,
    report: *mut HostReportSnapshot,
    report_id: u64,
    outcome: CallOutcome,
) -> i32 {
    let (status_value, report_value) = match outcome {
        CallOutcome::Passed(observation) => {
            let mut host_report = HostReport::passed(report_id, ReportPhase::Session, counters());
            host_report.handle_kind = observation.handle_kind.map(Into::into);
            host_report.session_state = observation.session_state;
            (
                HostCallStatus::success(report_id),
                host_report.snapshot(HOST_CORE_ABI_VERSION),
            )
        }
        CallOutcome::Failed {
            error,
            session_state,
            handle_kind,
        } => (
            HostCallStatus::failure(error.code(), report_id),
            HostReport::rejected(
                report_id,
                ReportPhase::Session,
                &error,
                handle_kind,
                session_state,
                counters(),
            )
            .snapshot(HOST_CORE_ABI_VERSION),
        ),
    };
    let code = status_value.code;
    // SAFETY: Nulls were rejected by `ffi_entry`; non-null pointer validity is
    // owned by the C++ adapter and protected by its native SEH boundary.
    unsafe {
        status.write(status_value);
        report.write(report_value);
    }
    code
}

unsafe fn ffi_entry(
    status: *mut HostCallStatus,
    report: *mut HostReportSnapshot,
    created_handle: Option<*mut HostOpaqueHandle>,
    operation: &'static str,
    call: impl FnOnce() -> CallOutcome,
) -> i32 {
    let result = contain_panic(operation, || {
        Ok(unsafe { ffi_entry_uncontained(status, report, created_handle, operation, call) })
    });
    match result {
        Ok(code) => code,
        Err(error) => {
            if status.is_null() || report.is_null() {
                return error.code() as i32;
            }
            if let Some(handle) = created_handle.filter(|handle| !handle.is_null()) {
                // SAFETY: Native pointer validity remains protected by the
                // calling C++ SEH frame. A caught Rust panic must not publish
                // a possibly-created token.
                unsafe { handle.write(HostOpaqueHandle(0)) };
            }
            let report_id = next_report_id().unwrap_or(0);
            unsafe { write_outcome(status, report, report_id, failed(error, None)) }
        }
    }
}

unsafe fn ffi_entry_uncontained(
    status: *mut HostCallStatus,
    report: *mut HostReportSnapshot,
    created_handle: Option<*mut HostOpaqueHandle>,
    operation: &'static str,
    call: impl FnOnce() -> CallOutcome,
) -> i32 {
    if status.is_null() || report.is_null() {
        return HostErrorCode::InvalidArgument as i32;
    }
    if created_handle.is_some_and(|handle| handle.is_null()) {
        let report_id = next_report_id().unwrap_or(0);
        return unsafe {
            write_outcome(
                status,
                report,
                report_id,
                failed(
                    HostError::new(HostErrorCode::InvalidArgument, operation),
                    None,
                ),
            )
        };
    }
    if let Some(handle) = created_handle {
        // SAFETY: The pointer was checked above; native validity is the C++
        // adapter's responsibility.
        unsafe { handle.write(HostOpaqueHandle(0)) };
    }
    let report_id = match next_report_id() {
        Ok(report_id) => report_id,
        Err(error) => {
            return unsafe { write_outcome(status, report, 0, failed(error, None)) };
        }
    };
    let outcome = call();
    if let (Some(handle_output), CallOutcome::Passed(observation)) = (created_handle, &outcome) {
        if let Some(handle) = observation.created_handle {
            // SAFETY: The pointer was checked above; native validity is the
            // C++ adapter's responsibility.
            unsafe { handle_output.write(handle) };
        }
    }
    unsafe { write_outcome(status, report, report_id, outcome) }
}

unsafe fn ffi_session_call(
    context: *const HostCallContext,
    handle: HostOpaqueHandle,
    status: *mut HostCallStatus,
    report: *mut HostReportSnapshot,
    operation_name: &'static str,
    operation: SessionOperation,
) -> i32 {
    unsafe {
        ffi_entry(status, report, None, operation_name, || {
            let context = match read_context(context, operation_name) {
                Ok(context) => context,
                Err(error) => return failed(error, None),
            };
            call_session(context, handle, operation)
        })
    }
}

/// Creates one registry-owned session token.
///
/// # Safety
///
/// Every non-null pointer must be aligned and valid for the corresponding
/// read or write for the duration of the call. The native adapter must place
/// the call inside its Windows SEH boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aex_host_core_session_create_v1(
    context: *const HostCallContext,
    session: *mut HostOpaqueHandle,
    status: *mut HostCallStatus,
    report: *mut HostReportSnapshot,
) -> i32 {
    unsafe {
        ffi_entry(status, report, Some(session), "ffi_session_create", || {
            let context = match read_context(context, "ffi_session_create") {
                Ok(context) => context,
                Err(error) => return failed(error, None),
            };
            create_session(context)
        })
    }
}

/// Opens a newly created session.
///
/// # Safety
///
/// Every non-null pointer must be aligned and valid for the corresponding
/// read or write for the duration of the call. The native adapter must place
/// the call inside its Windows SEH boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aex_host_core_session_open_v1(
    context: *const HostCallContext,
    session: HostOpaqueHandle,
    status: *mut HostCallStatus,
    report: *mut HostReportSnapshot,
) -> i32 {
    unsafe {
        ffi_session_call(
            context,
            session,
            status,
            report,
            "ffi_session_open",
            SessionOperation::Open,
        )
    }
}

/// Enters the session callback state.
///
/// # Safety
///
/// Every non-null pointer must be aligned and valid for the corresponding
/// read or write for the duration of the call. The native adapter must place
/// the call inside its Windows SEH boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aex_host_core_session_begin_callback_v1(
    context: *const HostCallContext,
    session: HostOpaqueHandle,
    status: *mut HostCallStatus,
    report: *mut HostReportSnapshot,
) -> i32 {
    unsafe {
        ffi_session_call(
            context,
            session,
            status,
            report,
            "ffi_session_begin_callback",
            SessionOperation::BeginCallback,
        )
    }
}

/// Leaves the session callback state.
///
/// # Safety
///
/// Every non-null pointer must be aligned and valid for the corresponding
/// read or write for the duration of the call. The native adapter must place
/// the call inside its Windows SEH boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aex_host_core_session_end_callback_v1(
    context: *const HostCallContext,
    session: HostOpaqueHandle,
    status: *mut HostCallStatus,
    report: *mut HostReportSnapshot,
) -> i32 {
    unsafe {
        ffi_session_call(
            context,
            session,
            status,
            report,
            "ffi_session_end_callback",
            SessionOperation::EndCallback,
        )
    }
}

/// Closes an open session.
///
/// # Safety
///
/// Every non-null pointer must be aligned and valid for the corresponding
/// read or write for the duration of the call. The native adapter must place
/// the call inside its Windows SEH boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aex_host_core_session_close_v1(
    context: *const HostCallContext,
    session: HostOpaqueHandle,
    status: *mut HostCallStatus,
    report: *mut HostReportSnapshot,
) -> i32 {
    unsafe {
        ffi_session_call(
            context,
            session,
            status,
            report,
            "ffi_session_close",
            SessionOperation::Close,
        )
    }
}

/// Disposes a closed or faulted session token.
///
/// # Safety
///
/// Every non-null pointer must be aligned and valid for the corresponding
/// read or write for the duration of the call. The native adapter must place
/// the call inside its Windows SEH boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aex_host_core_session_dispose_v1(
    context: *const HostCallContext,
    session: HostOpaqueHandle,
    status: *mut HostCallStatus,
    report: *mut HostReportSnapshot,
) -> i32 {
    unsafe {
        ffi_entry(status, report, None, "ffi_session_dispose", || {
            let context = match read_context(context, "ffi_session_dispose") {
                Ok(context) => context,
                Err(error) => return failed(error, None),
            };
            dispose_session(context, session)
        })
    }
}

/// Matches a caller-held identity against the current C++ registry identity.
///
/// # Safety
///
/// Both pointers must be aligned and valid for one `HostSceneIdentity` read.
/// The native adapter must place the call inside its Windows SEH boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aex_host_core_scene_identity_match_v1(
    current: *const HostSceneIdentity,
    candidate: *const HostSceneIdentity,
) -> i32 {
    match contain_panic("ffi_scene_identity_match", || {
        let current = unsafe { read_scene_identity(current, "read_current_scene_identity") }?;
        let candidate = unsafe { read_scene_identity(candidate, "read_candidate_scene_identity") }?;
        current.match_candidate(&candidate)
    }) {
        Ok(()) => HostErrorCode::Ok as i32,
        Err(error) => error.code() as i32,
    }
}

/// Matches a caller-held object-to-owner edge against the current C++ snapshot.
///
/// # Safety
///
/// Both pointers must be aligned and valid for one `HostSceneOwnerRelation`
/// read. The native adapter must place the call inside its Windows SEH boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aex_host_core_scene_owner_relation_match_v1(
    current: *const HostSceneOwnerRelation,
    candidate: *const HostSceneOwnerRelation,
) -> i32 {
    match contain_panic("ffi_scene_owner_relation_match", || {
        let current =
            unsafe { read_scene_owner_relation(current, "read_current_scene_owner_relation") }?;
        let candidate =
            unsafe { read_scene_owner_relation(candidate, "read_candidate_scene_owner_relation") }?;
        current.match_candidate(&candidate)
    }) {
        Ok(()) => HostErrorCode::Ok as i32,
        Err(error) => error.code() as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aexcompat_host_core::report::ReportOutcome;

    #[test]
    fn exported_descriptor_matches_the_compiled_value_abi() {
        assert_eq!(
            AEX_HOST_CORE_ABI_DESCRIPTOR_V1,
            HostCoreAbiDescriptorV1::current()
        );
        assert_eq!(
            AEX_HOST_CORE_SCENE_IDENTITY_ABI_DESCRIPTOR_V1,
            HostSceneIdentityAbiDescriptorV1::current()
        );
        assert_eq!(
            AEX_HOST_CORE_SCENE_OWNER_RELATION_ABI_DESCRIPTOR_V1,
            HostSceneOwnerRelationAbiDescriptorV1::current()
        );
    }

    #[test]
    fn exported_scene_identity_matcher_is_value_only_and_fail_closed() {
        let current =
            HostSceneIdentity::new(17, 103, 4, aexcompat_host_core::scene::object_kind::LAYER);
        let mut candidate = current;
        unsafe {
            assert_eq!(
                aex_host_core_scene_identity_match_v1(&current, &candidate),
                HostErrorCode::Ok as i32
            );
            candidate.generation -= 1;
            assert_eq!(
                aex_host_core_scene_identity_match_v1(&current, &candidate),
                HostErrorCode::StaleHandle as i32
            );
            assert_eq!(
                aex_host_core_scene_identity_match_v1(std::ptr::null(), &candidate),
                HostErrorCode::InvalidArgument as i32
            );
        }
    }

    #[test]
    fn exported_scene_owner_relation_matcher_is_value_only_and_fail_closed() {
        let owner = HostSceneIdentity::new(
            17,
            101,
            4,
            aexcompat_host_core::scene::object_kind::COMPOSITION,
        );
        let object =
            HostSceneIdentity::new(17, 103, 2, aexcompat_host_core::scene::object_kind::LAYER);
        let current = HostSceneOwnerRelation::new(object, owner);
        let mut candidate = current;
        unsafe {
            assert_eq!(
                aex_host_core_scene_owner_relation_match_v1(&current, &candidate),
                HostErrorCode::Ok as i32
            );
            candidate.owner.generation -= 1;
            assert_eq!(
                aex_host_core_scene_owner_relation_match_v1(&current, &candidate),
                HostErrorCode::StaleHandle as i32
            );
            assert_eq!(
                aex_host_core_scene_owner_relation_match_v1(std::ptr::null(), &candidate),
                HostErrorCode::InvalidArgument as i32
            );
        }
    }

    #[test]
    fn exported_lifecycle_is_fail_closed() {
        let context = HostCallContext::new(1001, 7001);
        let mut handle = HostOpaqueHandle(0);
        let mut status = HostCallStatus::success(0);
        let mut report = HostReport::passed(0, ReportPhase::Session, ReportCounters::default())
            .snapshot(HOST_CORE_ABI_VERSION);

        unsafe {
            assert_eq!(
                aex_host_core_session_create_v1(&context, &mut handle, &mut status, &mut report,),
                HostErrorCode::Ok as i32
            );
            assert_ne!(handle.0, 0);
            assert_eq!(report.session_state, SessionState::Created as u32);
            assert_eq!(
                aex_host_core_session_open_v1(&context, handle, &mut status, &mut report,),
                HostErrorCode::Ok as i32
            );
            assert_eq!(
                aex_host_core_session_begin_callback_v1(&context, handle, &mut status, &mut report,),
                HostErrorCode::Ok as i32
            );
            assert_eq!(
                aex_host_core_session_end_callback_v1(&context, handle, &mut status, &mut report,),
                HostErrorCode::Ok as i32
            );
            assert_eq!(
                aex_host_core_session_close_v1(&context, handle, &mut status, &mut report,),
                HostErrorCode::Ok as i32
            );
            assert_eq!(
                aex_host_core_session_dispose_v1(&context, handle, &mut status, &mut report,),
                HostErrorCode::Ok as i32
            );
            assert_eq!(
                aex_host_core_session_open_v1(&context, handle, &mut status, &mut report,),
                HostErrorCode::StaleHandle as i32
            );
            assert_eq!(
                aex_host_core_session_create_v1(
                    &context,
                    std::ptr::null_mut(),
                    &mut status,
                    &mut report,
                ),
                HostErrorCode::InvalidArgument as i32
            );
            assert_eq!(
                aex_host_core_session_open_v1(std::ptr::null(), handle, &mut status, &mut report,),
                HostErrorCode::InvalidArgument as i32
            );
        }
    }

    #[test]
    fn whole_ffi_entry_contains_panics_and_clears_created_tokens() {
        let mut handle = HostOpaqueHandle(u64::MAX);
        let mut status = HostCallStatus::success(0);
        let mut report = HostReport::passed(0, ReportPhase::Session, ReportCounters::default())
            .snapshot(HOST_CORE_ABI_VERSION);

        let code = unsafe {
            ffi_entry(
                &mut status,
                &mut report,
                Some(&mut handle),
                "test_whole_ffi_entry",
                || panic!("must not cross extern C"),
            )
        };

        assert_eq!(code, HostErrorCode::Panic as i32);
        assert_eq!(handle, HostOpaqueHandle(0));
        assert_eq!(status.code, HostErrorCode::Panic as i32);
        assert_ne!(status.report_id, 0);
        assert_eq!(status.report_id, report.report_id);
        assert_eq!(report.outcome, ReportOutcome::Faulted as u32);
        assert_eq!(report.error_code, HostErrorCode::Panic as i32);
    }
}
