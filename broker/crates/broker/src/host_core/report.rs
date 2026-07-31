use crate::host_core::error::{HostError, HostErrorCode};
use crate::host_core::handle::HandleKind;
use crate::host_core::session::SessionState;
use serde::Serialize;

pub const HOST_REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportPhase {
    Boundary,
    Handle,
    Session,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportOutcome {
    Passed,
    Rejected,
    Faulted,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleKindReport {
    Scene,
    World,
    Parameter,
    Session,
    Report,
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
}

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
}
