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
use aexcompat_host_core::parameter::{
    HOST_PARAMETER_ANIMATION_MAX_KEYS, HostAnimationKey, HostAnimationValue,
    HostParameterAnimationAbiDescriptorV1, HostRationalTime,
    evaluate as evaluate_parameter_animation,
};
use aexcompat_host_core::report::{HostReport, HostReportSnapshot, ReportCounters, ReportPhase};
use aexcompat_host_core::scene::{
    HostOwnedSceneTopologySnapshot, HostSceneIdentity, HostSceneIdentityAbiDescriptorV1,
    HostSceneOwnerRelation, HostSceneOwnerRelationAbiDescriptorV1,
    HostSceneTopologyAbiDescriptorV1, HostSceneTopologyEntry, HostSceneTopologySnapshot,
    HostSceneTopologySummary,
};
use aexcompat_host_core::session::{HostSession, SessionState};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread::{self, ThreadId};

#[unsafe(export_name = "aex_host_core_abi_descriptor_v1")]
pub static AEX_HOST_CORE_ABI_DESCRIPTOR_V1: HostCoreAbiDescriptorV1 =
    HostCoreAbiDescriptorV1::current();

#[unsafe(export_name = "aex_host_core_scene_identity_abi_descriptor_v1")]
pub static AEX_HOST_CORE_SCENE_IDENTITY_ABI_DESCRIPTOR_V1: HostSceneIdentityAbiDescriptorV1 =
    HostSceneIdentityAbiDescriptorV1::current();

#[unsafe(export_name = "aex_host_core_scene_owner_relation_abi_descriptor_v1")]
pub static AEX_HOST_CORE_SCENE_OWNER_RELATION_ABI_DESCRIPTOR_V1:
    HostSceneOwnerRelationAbiDescriptorV1 = HostSceneOwnerRelationAbiDescriptorV1::current();

#[unsafe(export_name = "aex_host_core_scene_topology_abi_descriptor_v1")]
pub static AEX_HOST_CORE_SCENE_TOPOLOGY_ABI_DESCRIPTOR_V1: HostSceneTopologyAbiDescriptorV1 =
    HostSceneTopologyAbiDescriptorV1::current();

#[unsafe(export_name = "aex_host_core_parameter_animation_abi_descriptor_v1")]
pub static AEX_HOST_CORE_PARAMETER_ANIMATION_ABI_DESCRIPTOR_V1:
    HostParameterAnimationAbiDescriptorV1 = HostParameterAnimationAbiDescriptorV1::current();

fn call_parameter_evaluator(
    now: HostRationalTime,
    keys: &[HostAnimationKey],
    output: &mut HostAnimationValue,
    evaluator: impl FnOnce(
        HostRationalTime,
        &[HostAnimationKey],
    ) -> Result<HostAnimationValue, HostError>,
) -> i32 {
    *output = HostAnimationValue::default();
    match contain_panic("ffi_parameter_animation_evaluate", || evaluator(now, keys)) {
        Ok(value) => {
            *output = value;
            HostErrorCode::Ok as i32
        }
        Err(error) => error.code() as i32,
    }
}

/// Evaluates one bounded, pointer-free typed timeline without mutating any
/// production worker state.
///
/// # Safety
///
/// All non-null pointers must be aligned and valid for the full input/output
/// extent. The native adapter must contain pointer faults with Windows SEH.
/// Valid inputs are copied before `output` is written, so their storage may
/// overlap. Rejected calls clear a valid output pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aex_host_core_parameter_animation_evaluate_v1(
    now: *const HostRationalTime,
    keys: *const HostAnimationKey,
    key_count: u32,
    output: *mut HostAnimationValue,
) -> i32 {
    if output.is_null()
        || !(output as usize).is_multiple_of(std::mem::align_of::<HostAnimationValue>())
    {
        return HostErrorCode::InvalidArgument as i32;
    }
    if now.is_null()
        || !(now as usize).is_multiple_of(std::mem::align_of::<HostRationalTime>())
        || keys.is_null()
        || !(keys as usize).is_multiple_of(std::mem::align_of::<HostAnimationKey>())
        || key_count == 0
        || key_count as usize > HOST_PARAMETER_ANIMATION_MAX_KEYS
    {
        // SAFETY: The non-null, aligned output is writable by contract.
        unsafe { output.write(HostAnimationValue::default()) };
        return HostErrorCode::InvalidArgument as i32;
    }
    // Copy both inputs before writing output. The C ABI permits an output
    // address inside either input, so creating shared and mutable references
    // to the caller's storage would violate Rust's aliasing rules.
    // SAFETY: The caller promises readable input for the validated extent;
    // the native adapter contains pointer faults in its SEH frame.
    let now = unsafe { now.read() };
    let mut owned_keys = [HostAnimationKey::default(); HOST_PARAMETER_ANIMATION_MAX_KEYS];
    // SAFETY: The bounded source extent is valid by contract; the private
    // stack buffer cannot overlap caller-owned input.
    unsafe { std::ptr::copy_nonoverlapping(keys, owned_keys.as_mut_ptr(), key_count as usize) };
    // SAFETY: All input reads are complete, so even an aliased output can be
    // borrowed mutably for the remainder of this synchronous call.
    let output = unsafe { &mut *output };
    call_parameter_evaluator(
        now,
        &owned_keys[..key_count as usize],
        output,
        evaluate_parameter_animation,
    )
}

struct SessionRecord {
    session: HostSession,
    caller_thread_token: u64,
}

struct SceneSnapshotRecord {
    snapshot: HostOwnedSceneTopologySnapshot,
    caller_thread_token: u64,
    origin_thread: ThreadId,
}

#[derive(Default)]
struct AdapterState {
    sessions: HandleRegistry<SessionRecord>,
    scene_snapshots: HandleRegistry<SceneSnapshotRecord>,
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

unsafe fn read_scene_topology_snapshot(
    snapshot: *const HostSceneTopologySnapshot,
    operation: &'static str,
) -> Result<HostSceneTopologySnapshot, HostError> {
    if snapshot.is_null() {
        return Err(HostError::new(HostErrorCode::InvalidArgument, operation));
    }
    // SAFETY: The thin C++ adapter owns pointer validity and places this read
    // inside its SEH frame. The copied value contains no pointer.
    Ok(unsafe { snapshot.read() })
}

fn validate_scene_snapshot_thread(
    record: &SceneSnapshotRecord,
    context: HostCallContext,
    operation: &'static str,
) -> Result<(), HostError> {
    if record.caller_thread_token != context.caller_thread_token
        || record.origin_thread != thread::current().id()
    {
        return Err(HostError::new(HostErrorCode::WrongThread, operation));
    }
    Ok(())
}

fn create_owned_scene_snapshot(
    context: HostCallContext,
    snapshot: HostSceneTopologySnapshot,
) -> Result<HostOpaqueHandle, HostError> {
    let owner = OwnerId::new(context.session_id)?;
    let snapshot = HostOwnedSceneTopologySnapshot::from_snapshot(&snapshot)?;
    let record = SceneSnapshotRecord {
        snapshot,
        caller_thread_token: context.caller_thread_token,
        origin_thread: thread::current().id(),
    };
    lock_state("create_owned_scene_snapshot")?
        .scene_snapshots
        .insert(owner, HandleKind::Scene, record)
}

fn query_owned_scene_snapshot(
    context: HostCallContext,
    handle: HostOpaqueHandle,
    index: u32,
) -> Result<HostSceneTopologyEntry, HostError> {
    let owner = OwnerId::new(context.session_id)?;
    let state = lock_state("query_owned_scene_snapshot")?;
    let record = state
        .scene_snapshots
        .get(handle, owner, HandleKind::Scene)?;
    validate_scene_snapshot_thread(record, context, "query_owned_scene_snapshot_thread")?;
    record.snapshot.entry(index)
}

fn summarize_owned_scene_snapshot(
    context: HostCallContext,
    handle: HostOpaqueHandle,
) -> Result<HostSceneTopologySummary, HostError> {
    let owner = OwnerId::new(context.session_id)?;
    let state = lock_state("summarize_owned_scene_snapshot")?;
    let record = state
        .scene_snapshots
        .get(handle, owner, HandleKind::Scene)?;
    validate_scene_snapshot_thread(record, context, "summarize_owned_scene_snapshot_thread")?;
    Ok(record.snapshot.summary())
}

fn destroy_owned_scene_snapshot(
    context: HostCallContext,
    handle: HostOpaqueHandle,
) -> Result<(), HostError> {
    let owner = OwnerId::new(context.session_id)?;
    let mut state = lock_state("destroy_owned_scene_snapshot")?;
    {
        let record = state
            .scene_snapshots
            .get(handle, owner, HandleKind::Scene)?;
        validate_scene_snapshot_thread(record, context, "destroy_owned_scene_snapshot_thread")?;
    }
    state
        .scene_snapshots
        .remove(handle, owner, HandleKind::Scene)?;
    Ok(())
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
    if let (Some(handle_output), CallOutcome::Passed(observation)) = (created_handle, &outcome)
        && let Some(handle) = observation.created_handle
    {
        // SAFETY: The pointer was checked above; native validity is the
        // C++ adapter's responsibility.
        unsafe { handle_output.write(handle) };
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

/// Validates and summarizes one bounded C++-owned topology snapshot.
///
/// # Safety
///
/// `snapshot` and `summary` must be aligned and valid for one complete read or
/// write. The native adapter must place the call inside its Windows SEH
/// boundary. `summary` is cleared before any input validation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aex_host_core_scene_topology_summarize_v1(
    snapshot: *const HostSceneTopologySnapshot,
    summary: *mut HostSceneTopologySummary,
) -> i32 {
    if summary.is_null() {
        return HostErrorCode::InvalidArgument as i32;
    }
    // SAFETY: The caller provides one writable summary and the native adapter
    // contains pointer faults with SEH.
    unsafe { summary.write(HostSceneTopologySummary::zeroed()) };
    match contain_panic("ffi_scene_topology_summarize", || {
        let snapshot =
            unsafe { read_scene_topology_snapshot(snapshot, "read_scene_topology_snapshot") }?;
        snapshot.calculate_summary()
    }) {
        Ok(calculated) => {
            // SAFETY: The output pointer was checked above and remains valid
            // for the duration of this synchronous call.
            unsafe { summary.write(calculated) };
            HostErrorCode::Ok as i32
        }
        Err(error) => error.code() as i32,
    }
}

/// Copies one validated topology value into Rust-owned immutable state.
///
/// # Safety
///
/// All pointers must be aligned and valid for their complete value. The native
/// adapter must contain pointer faults with SEH. `handle` is cleared before
/// any input is inspected.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aex_host_core_scene_topology_snapshot_create_v1(
    context: *const HostCallContext,
    snapshot: *const HostSceneTopologySnapshot,
    handle: *mut HostOpaqueHandle,
) -> i32 {
    if handle.is_null() {
        return HostErrorCode::InvalidArgument as i32;
    }
    unsafe { handle.write(HostOpaqueHandle(0)) };
    match contain_panic("ffi_scene_topology_snapshot_create", || {
        let context = unsafe { read_context(context, "read_scene_snapshot_create_context") }?;
        let snapshot =
            unsafe { read_scene_topology_snapshot(snapshot, "read_scene_snapshot_create_value") }?;
        create_owned_scene_snapshot(context, snapshot)
    }) {
        Ok(created) => {
            unsafe { handle.write(created) };
            HostErrorCode::Ok as i32
        }
        Err(error) => error.code() as i32,
    }
}

/// Returns one canonical entry from a Rust-owned topology snapshot.
///
/// # Safety
///
/// `context` and `entry` must be valid for one complete read or write and the
/// native adapter must contain pointer faults with SEH. `entry` is always
/// cleared before validation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aex_host_core_scene_topology_snapshot_query_v1(
    context: *const HostCallContext,
    handle: HostOpaqueHandle,
    index: u32,
    entry: *mut HostSceneTopologyEntry,
) -> i32 {
    if entry.is_null() {
        return HostErrorCode::InvalidArgument as i32;
    }
    unsafe { entry.write(HostSceneTopologyEntry::zeroed()) };
    match contain_panic("ffi_scene_topology_snapshot_query", || {
        let context = unsafe { read_context(context, "read_scene_snapshot_query_context") }?;
        query_owned_scene_snapshot(context, handle, index)
    }) {
        Ok(found) => {
            unsafe { entry.write(found) };
            HostErrorCode::Ok as i32
        }
        Err(error) => error.code() as i32,
    }
}

/// Returns the summary stored when the Rust-owned snapshot was created.
///
/// # Safety
///
/// `context` and `summary` must be valid for one complete read or write and
/// the native adapter must contain pointer faults with SEH. `summary` is
/// always cleared before validation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aex_host_core_scene_topology_snapshot_summary_v1(
    context: *const HostCallContext,
    handle: HostOpaqueHandle,
    summary: *mut HostSceneTopologySummary,
) -> i32 {
    if summary.is_null() {
        return HostErrorCode::InvalidArgument as i32;
    }
    unsafe { summary.write(HostSceneTopologySummary::zeroed()) };
    match contain_panic("ffi_scene_topology_snapshot_summary", || {
        let context = unsafe { read_context(context, "read_scene_snapshot_summary_context") }?;
        summarize_owned_scene_snapshot(context, handle)
    }) {
        Ok(stored) => {
            unsafe { summary.write(stored) };
            HostErrorCode::Ok as i32
        }
        Err(error) => error.code() as i32,
    }
}

/// Destroys one Rust-owned topology snapshot.
///
/// # Safety
///
/// `context` must be valid for one complete read and the native adapter must
/// contain pointer faults with SEH.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aex_host_core_scene_topology_snapshot_destroy_v1(
    context: *const HostCallContext,
    handle: HostOpaqueHandle,
) -> i32 {
    match contain_panic("ffi_scene_topology_snapshot_destroy", || {
        let context = unsafe { read_context(context, "read_scene_snapshot_destroy_context") }?;
        destroy_owned_scene_snapshot(context, handle)
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
        assert_eq!(
            AEX_HOST_CORE_SCENE_TOPOLOGY_ABI_DESCRIPTOR_V1,
            HostSceneTopologyAbiDescriptorV1::current()
        );
        assert_eq!(
            AEX_HOST_CORE_PARAMETER_ANIMATION_ABI_DESCRIPTOR_V1,
            HostParameterAnimationAbiDescriptorV1::current()
        );
    }

    #[test]
    fn parameter_animation_ffi_clears_output_on_rejection_and_panic() {
        let now = HostRationalTime { value: 1, scale: 2 };
        let key = HostAnimationKey {
            time: HostRationalTime { value: 0, scale: 1 },
            kind: aexcompat_host_core::parameter::value_kind::SCALAR,
            interpolation: aexcompat_host_core::parameter::interpolation::LINEAR,
            scalar: 7.0,
            ..HostAnimationKey::default()
        };
        let mut output = HostAnimationValue {
            scalar: 123.0,
            ..HostAnimationValue::default()
        };
        assert_eq!(
            unsafe { aex_host_core_parameter_animation_evaluate_v1(&now, &key, 1, &mut output) },
            HostErrorCode::Ok as i32
        );
        assert_eq!(output.scalar, 7.0);

        let mut alias_key = key;
        let alias_input = &raw mut alias_key;
        let alias_output = alias_input.cast::<HostAnimationValue>();
        assert_eq!(
            unsafe {
                aex_host_core_parameter_animation_evaluate_v1(&now, alias_input, 1, alias_output)
            },
            HostErrorCode::Ok as i32
        );
        // SAFETY: The completed FFI call wrote a value inside alias_key's
        // still-live, aligned storage.
        assert_eq!(unsafe { (*alias_output).scalar }, 7.0);

        output.scalar = 123.0;
        assert_eq!(
            unsafe {
                aex_host_core_parameter_animation_evaluate_v1(
                    std::ptr::null(),
                    &key,
                    1,
                    &mut output,
                )
            },
            HostErrorCode::InvalidArgument as i32
        );
        assert_eq!(output, HostAnimationValue::default());
        output.scalar = 123.0;
        assert_eq!(
            unsafe { aex_host_core_parameter_animation_evaluate_v1(&now, &key, 257, &mut output) },
            HostErrorCode::InvalidArgument as i32
        );
        assert_eq!(output, HostAnimationValue::default());

        output.scalar = 123.0;
        assert_eq!(
            call_parameter_evaluator(now, &[key], &mut output, |_, _| panic!("synthetic panic")),
            HostErrorCode::Panic as i32
        );
        assert_eq!(output, HostAnimationValue::default());
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
    fn exported_scene_topology_summary_is_value_only_and_fail_closed() {
        use aexcompat_host_core::scene::{HostSceneTopologyEntry, object_kind};

        let zero = HostSceneIdentity::new(0, 0, 0, object_kind::NONE);
        let project = HostSceneIdentity::new(17, 17, 1, object_kind::PROJECT);
        let item = HostSceneIdentity::new(17, 101, 1, object_kind::ITEM);
        let mut snapshot = HostSceneTopologySnapshot::empty(17);
        snapshot.entry_count = 2;
        snapshot.entries[0] =
            HostSceneTopologyEntry::new(HostSceneOwnerRelation::new(project, zero), -1);
        snapshot.entries[1] =
            HostSceneTopologyEntry::new(HostSceneOwnerRelation::new(item, project), 0);
        let mut summary = HostSceneTopologySummary::zeroed();

        unsafe {
            assert_eq!(
                aex_host_core_scene_topology_summarize_v1(&snapshot, &mut summary),
                HostErrorCode::Ok as i32
            );
            assert_eq!(summary.object_count, 2);
            assert_eq!(summary.edge_count, 1);

            snapshot.entry_count =
                aexcompat_host_core::scene::HOST_SCENE_TOPOLOGY_CAPACITY as u32 + 1;
            summary.fingerprint = 99;
            assert_eq!(
                aex_host_core_scene_topology_summarize_v1(&snapshot, &mut summary),
                HostErrorCode::CapacityExceeded as i32
            );
            assert_eq!(summary, HostSceneTopologySummary::zeroed());
            assert_eq!(
                aex_host_core_scene_topology_summarize_v1(std::ptr::null(), &mut summary,),
                HostErrorCode::InvalidArgument as i32
            );
            assert_eq!(
                aex_host_core_scene_topology_summarize_v1(&snapshot, std::ptr::null_mut(),),
                HostErrorCode::InvalidArgument as i32
            );
        }
    }

    #[test]
    fn rust_owned_scene_snapshot_lifecycle_is_detached_typed_and_thread_bound() {
        use aexcompat_host_core::scene::{HostSceneTopologyEntry, object_kind};

        let zero = HostSceneIdentity::new(0, 0, 0, object_kind::NONE);
        let project = HostSceneIdentity::new(81, 81, 1, object_kind::PROJECT);
        let composition = HostSceneIdentity::new(81, 900, 2, object_kind::COMPOSITION);
        let layer = HostSceneIdentity::new(81, 700, 4, object_kind::LAYER);
        let mut input = HostSceneTopologySnapshot::empty(81);
        input.entry_count = 3;
        input.entries[0] =
            HostSceneTopologyEntry::new(HostSceneOwnerRelation::new(layer, composition), 0);
        input.entries[1] =
            HostSceneTopologyEntry::new(HostSceneOwnerRelation::new(project, zero), -1);
        input.entries[2] =
            HostSceneTopologyEntry::new(HostSceneOwnerRelation::new(composition, project), 0);
        let expected_summary = input.calculate_summary().unwrap();
        let context = HostCallContext::new(81_001, 81_101);
        let mut scene_handle = HostOpaqueHandle(u64::MAX);

        unsafe {
            assert_eq!(
                aex_host_core_scene_topology_snapshot_create_v1(
                    &context,
                    &input,
                    &mut scene_handle,
                ),
                HostErrorCode::Ok as i32
            );
        }
        assert_ne!(scene_handle, HostOpaqueHandle(0));

        input = HostSceneTopologySnapshot::empty(999);
        assert_eq!(input.entry_count, 0);
        let mut entry =
            HostSceneTopologyEntry::new(HostSceneOwnerRelation::new(layer, composition), i32::MAX);
        let mut summary = HostSceneTopologySummary::zeroed();
        unsafe {
            assert_eq!(
                aex_host_core_scene_topology_snapshot_query_v1(
                    &context,
                    scene_handle,
                    0,
                    &mut entry,
                ),
                HostErrorCode::Ok as i32
            );
            assert_eq!(entry.relation.object, project);
            assert_eq!(
                aex_host_core_scene_topology_snapshot_summary_v1(
                    &context,
                    scene_handle,
                    &mut summary,
                ),
                HostErrorCode::Ok as i32
            );
        }
        assert_eq!(summary, expected_summary);

        entry.local_index = i32::MAX;
        unsafe {
            assert_eq!(
                aex_host_core_scene_topology_snapshot_query_v1(
                    &context,
                    scene_handle,
                    3,
                    &mut entry,
                ),
                HostErrorCode::InvalidArgument as i32
            );
        }
        assert_eq!(entry, HostSceneTopologyEntry::zeroed());

        let wrong_logical_thread = HostCallContext::new(81_001, 81_102);
        summary = expected_summary;
        unsafe {
            assert_eq!(
                aex_host_core_scene_topology_snapshot_summary_v1(
                    &wrong_logical_thread,
                    scene_handle,
                    &mut summary,
                ),
                HostErrorCode::WrongThread as i32
            );
        }
        assert_eq!(summary, HostSceneTopologySummary::zeroed());

        let foreign_owner = HostCallContext::new(81_002, 81_101);
        unsafe {
            assert_eq!(
                aex_host_core_scene_topology_snapshot_destroy_v1(&foreign_owner, scene_handle),
                HostErrorCode::WrongOwner as i32
            );
        }

        let foreign_thread_code = std::thread::spawn(move || {
            let mut foreign_entry = HostSceneTopologyEntry::new(
                HostSceneOwnerRelation::new(layer, composition),
                i32::MAX,
            );
            let code = unsafe {
                aex_host_core_scene_topology_snapshot_query_v1(
                    &context,
                    scene_handle,
                    0,
                    &mut foreign_entry,
                )
            };
            (code, foreign_entry)
        })
        .join()
        .unwrap();
        assert_eq!(foreign_thread_code.0, HostErrorCode::WrongThread as i32);
        assert_eq!(foreign_thread_code.1, HostSceneTopologyEntry::zeroed());

        let mut session_handle = HostOpaqueHandle(0);
        let mut status = HostCallStatus::success(0);
        let mut report = HostReport::passed(0, ReportPhase::Session, ReportCounters::default())
            .snapshot(HOST_CORE_ABI_VERSION);
        unsafe {
            assert_eq!(
                aex_host_core_session_create_v1(
                    &context,
                    &mut session_handle,
                    &mut status,
                    &mut report,
                ),
                HostErrorCode::Ok as i32
            );
            assert_eq!(
                aex_host_core_scene_topology_snapshot_destroy_v1(&context, session_handle),
                HostErrorCode::InvalidHandle as i32
            );
            assert_eq!(
                aex_host_core_session_open_v1(&context, session_handle, &mut status, &mut report,),
                HostErrorCode::Ok as i32
            );
            assert_eq!(
                aex_host_core_session_close_v1(&context, scene_handle, &mut status, &mut report,),
                HostErrorCode::InvalidHandle as i32
            );
            assert_eq!(
                aex_host_core_session_close_v1(&context, session_handle, &mut status, &mut report,),
                HostErrorCode::Ok as i32
            );
            assert_eq!(
                aex_host_core_session_dispose_v1(
                    &context,
                    session_handle,
                    &mut status,
                    &mut report,
                ),
                HostErrorCode::Ok as i32
            );
            assert_eq!(
                aex_host_core_scene_topology_snapshot_summary_v1(
                    &context,
                    scene_handle,
                    &mut summary,
                ),
                HostErrorCode::Ok as i32
            );
            assert_eq!(
                aex_host_core_scene_topology_snapshot_destroy_v1(&context, scene_handle),
                HostErrorCode::Ok as i32
            );
            assert_eq!(
                aex_host_core_scene_topology_snapshot_query_v1(
                    &context,
                    scene_handle,
                    0,
                    &mut entry,
                ),
                HostErrorCode::StaleHandle as i32
            );
            assert_eq!(entry, HostSceneTopologyEntry::zeroed());
            assert_eq!(
                aex_host_core_scene_topology_snapshot_destroy_v1(&context, scene_handle),
                HostErrorCode::StaleHandle as i32
            );
        }

        let mut overflow = HostSceneTopologySnapshot::empty(81);
        overflow.entry_count = aexcompat_host_core::scene::HOST_SCENE_TOPOLOGY_CAPACITY as u32 + 1;
        let mut rejected = HostOpaqueHandle(u64::MAX);
        unsafe {
            assert_eq!(
                aex_host_core_scene_topology_snapshot_create_v1(&context, &overflow, &mut rejected,),
                HostErrorCode::CapacityExceeded as i32
            );
        }
        assert_eq!(rejected, HostOpaqueHandle(0));
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
