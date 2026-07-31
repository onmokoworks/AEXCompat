use crate::host_core::error::{HostError, HostErrorCode};
use crate::host_core::handle::HandleKind;
use crate::host_core::session::SessionState;
use serde::Serialize;
use std::mem::{align_of, offset_of, size_of};

pub const HOST_REPORT_SCHEMA_VERSION: u32 = 1;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportPhase {
    Boundary = 1,
    Handle = 2,
    Session = 3,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportOutcome {
    Passed = 1,
    Rejected = 2,
    Faulted = 3,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReportCounters {
    pub handles_created: u64,
    pub handles_disposed: u64,
    pub callbacks_attempted: u64,
    pub callbacks_completed: u64,
}

/// Value-only diagnostic contract. It cannot carry host handles, pointers,
/// filesystem paths, pixels, plugin bytes, or free-form exception text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostReport {
    pub schema_version: u32,
    pub report_id: u64,
    pub phase: ReportPhase,
    pub outcome: ReportOutcome,
    pub error_code: Option<i32>,
    pub handle_kind: Option<HandleKindReport>,
    pub session_state: Option<SessionState>,
    pub counters: ReportCounters,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleKindReport {
    Scene = 1,
    World = 2,
    Parameter = 3,
    Session = 4,
    Report = 5,
}

/// Fixed C ABI projection of [`HostReport`]. Optional enum fields use zero as
/// `none`; error code zero means the call passed.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostReportSnapshot {
    pub abi_version: u32,
    pub struct_size: u32,
    pub schema_version: u32,
    pub phase: u32,
    pub outcome: u32,
    pub error_code: i32,
    pub handle_kind: u32,
    pub session_state: u32,
    pub report_id: u64,
    pub handles_created: u64,
    pub handles_disposed: u64,
    pub callbacks_attempted: u64,
    pub callbacks_completed: u64,
}

impl From<HandleKind> for HandleKindReport {
    fn from(value: HandleKind) -> Self {
        match value {
            HandleKind::Scene => Self::Scene,
            HandleKind::World => Self::World,
            HandleKind::Parameter => Self::Parameter,
            HandleKind::Session => Self::Session,
            HandleKind::Report => Self::Report,
        }
    }
}

impl HostReport {
    pub fn passed(report_id: u64, phase: ReportPhase, counters: ReportCounters) -> Self {
        Self {
            schema_version: HOST_REPORT_SCHEMA_VERSION,
            report_id,
            phase,
            outcome: ReportOutcome::Passed,
            error_code: None,
            handle_kind: None,
            session_state: None,
            counters,
        }
    }

    pub fn rejected(
        report_id: u64,
        phase: ReportPhase,
        error: &HostError,
        handle_kind: Option<HandleKind>,
        session_state: Option<SessionState>,
        counters: ReportCounters,
    ) -> Self {
        let outcome = if matches!(error.code(), HostErrorCode::Panic | HostErrorCode::SehFault) {
            ReportOutcome::Faulted
        } else {
            ReportOutcome::Rejected
        };
        Self {
            schema_version: HOST_REPORT_SCHEMA_VERSION,
            report_id,
            phase,
            outcome,
            error_code: Some(error.code() as i32),
            handle_kind: handle_kind.map(Into::into),
            session_state,
            counters,
        }
    }

    pub fn snapshot(&self, abi_version: u32) -> HostReportSnapshot {
        HostReportSnapshot {
            abi_version,
            struct_size: size_of::<HostReportSnapshot>() as u32,
            schema_version: self.schema_version,
            phase: self.phase as u32,
            outcome: self.outcome as u32,
            error_code: self.error_code.unwrap_or(HostErrorCode::Ok as i32),
            handle_kind: self.handle_kind.map_or(0, |kind| kind as u32),
            session_state: self.session_state.map_or(0, |state| state as u32),
            report_id: self.report_id,
            handles_created: self.counters.handles_created,
            handles_disposed: self.counters.handles_disposed,
            callbacks_attempted: self.counters.callbacks_attempted,
            callbacks_completed: self.counters.callbacks_completed,
        }
    }
}

const _: () = {
    assert!(size_of::<HostReportSnapshot>() == 72);
    assert!(align_of::<HostReportSnapshot>() == 8);
    assert!(offset_of!(HostReportSnapshot, error_code) == 20);
    assert!(offset_of!(HostReportSnapshot, report_id) == 32);
    assert!(offset_of!(HostReportSnapshot, callbacks_completed) == 64);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_is_structured_and_value_only() {
        let error = HostError::new(HostErrorCode::WrongOwner, "resolve_world");
        let report = HostReport::rejected(
            17,
            ReportPhase::Handle,
            &error,
            Some(HandleKind::World),
            None,
            ReportCounters {
                handles_created: 1,
                ..ReportCounters::default()
            },
        );
        let json = serde_json::to_string(&report).unwrap();
        assert_eq!(
            json,
            r#"{"schema_version":1,"report_id":17,"phase":"handle","outcome":"rejected","error_code":5,"handle_kind":"world","session_state":null,"counters":{"handles_created":1,"handles_disposed":0,"callbacks_attempted":0,"callbacks_completed":0}}"#
        );
        for forbidden in ["0x", ":\\", "\\\\", "\"path\"", "\"pointer\"", "\"bytes\""] {
            assert!(!json.contains(forbidden));
        }
    }

    #[test]
    fn panic_and_seh_codes_are_faults_not_successes() {
        for code in [HostErrorCode::Panic, HostErrorCode::SehFault] {
            let report = HostReport::rejected(
                1,
                ReportPhase::Boundary,
                &HostError::new(code, "adapter"),
                None,
                None,
                ReportCounters::default(),
            );
            assert_eq!(report.outcome, ReportOutcome::Faulted);
            assert_ne!(report.error_code, Some(HostErrorCode::Ok as i32));
        }
    }

    #[test]
    fn fixed_snapshot_layout_and_optional_sentinels_are_stable() {
        assert_eq!(size_of::<HostReportSnapshot>(), 72);
        assert_eq!(align_of::<HostReportSnapshot>(), 8);
        assert_eq!(offset_of!(HostReportSnapshot, error_code), 20);
        assert_eq!(offset_of!(HostReportSnapshot, report_id), 32);
        assert_eq!(offset_of!(HostReportSnapshot, callbacks_completed), 64);

        let snapshot =
            HostReport::passed(9, ReportPhase::Session, ReportCounters::default()).snapshot(1);
        assert_eq!(snapshot.struct_size, 72);
        assert_eq!(snapshot.error_code, HostErrorCode::Ok as i32);
        assert_eq!(snapshot.handle_kind, 0);
        assert_eq!(snapshot.session_state, 0);
    }
}
