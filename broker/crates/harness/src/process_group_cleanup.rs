//! Platform-independent classification of owned leader/group exit observations.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GroupState {
    Absent,
    Present,
    PermissionDenied,
}

impl GroupState {
    pub(crate) fn exists(self) -> bool {
        self != Self::Absent
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExitObservation {
    pub(crate) initial_reaped: bool,
    pub(crate) final_reaped: bool,
    pub(crate) group: GroupState,
}

impl ExitObservation {
    pub(crate) fn fully_exited(self) -> bool {
        self.final_reaped && self.group == GroupState::Absent
    }
}

pub(crate) fn observe_exit<E>(
    initial_reaped: bool,
    wait_once: impl FnOnce() -> Result<bool, E>,
    probe_group: impl FnOnce() -> Result<GroupState, E>,
) -> Result<ExitObservation, E> {
    let final_reaped = if initial_reaped { true } else { wait_once()? };
    Ok(ExitObservation {
        initial_reaped,
        final_reaped,
        group: probe_group()?,
    })
}

#[derive(Debug)]
pub(crate) enum CleanupError {
    // Only an actual group-signal EPERM may remain pending until the existing
    // cleanup boundary. Identity signals and all other errors stay fatal.
    PendingGroupPermissionDenied(String),
    Other(String),
}

impl From<String> for CleanupError {
    fn from(error: String) -> Self {
        Self::Other(error)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CleanupObservation {
    pub(crate) leader_reaped: bool,
    pub(crate) group: GroupState,
    pub(crate) descendants_absent: bool,
}

impl CleanupObservation {
    pub(crate) fn fully_exited(self) -> bool {
        self.leader_reaped && self.group == GroupState::Absent && self.descendants_absent
    }
}

pub(crate) fn finish_cleanup(
    errors: Vec<CleanupError>,
    final_observation: CleanupObservation,
) -> Result<(), String> {
    // A past group EPERM does not invalidate a later, complete exit proof.
    // Never discard accounting, overflow, reap, probe or other signal errors,
    // even when every observed process has subsequently disappeared.
    if final_observation.fully_exited()
        && errors
            .iter()
            .all(|error| matches!(error, CleanupError::PendingGroupPermissionDenied(_)))
    {
        return Ok(());
    }
    if errors.is_empty() {
        return Err("macos_worker_residual_process: cleanup exit is unconfirmed".into());
    }
    Err(errors
        .into_iter()
        .map(|error| match error {
            CleanupError::PendingGroupPermissionDenied(message) | CleanupError::Other(message) => {
                message
            }
        })
        .collect::<Vec<_>>()
        .join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending_permission_denied() -> CleanupError {
        CleanupError::PendingGroupPermissionDenied("group SIGKILL EPERM".into())
    }

    #[test]
    fn delayed_group_disappearance_releases_only_the_pending_group_error() {
        // Model the observed failure path: owned leader already reaped,
        // group EPERM, and only later a fresh group ESRCH. This uses the same
        // exit observation and final decision as signal_group/termination.
        let mut groups = [GroupState::PermissionDenied, GroupState::Absent].into_iter();
        let initial = observe_exit::<&str>(
            true,
            || panic!("leader is already reaped"),
            || Ok(groups.next().unwrap()),
        )
        .unwrap();
        assert!(!initial.fully_exited());
        assert_eq!(
            finish_cleanup(
                vec![pending_permission_denied()],
                CleanupObservation {
                    leader_reaped: initial.final_reaped,
                    group: initial.group,
                    descendants_absent: true,
                },
            ),
            Err("group SIGKILL EPERM".into()),
        );
        let final_observation = CleanupObservation {
            leader_reaped: initial.final_reaped,
            group: groups.next().unwrap(),
            descendants_absent: true,
        };
        assert_eq!(
            finish_cleanup(vec![pending_permission_denied()], final_observation),
            Ok(()),
        );
        assert!(groups.next().is_none());
    }

    #[test]
    fn pending_group_error_requires_every_exit_condition() {
        for leader_reaped in [false, true] {
            for group in [
                GroupState::Absent,
                GroupState::Present,
                GroupState::PermissionDenied,
            ] {
                for descendants_absent in [false, true] {
                    let observation = CleanupObservation {
                        leader_reaped,
                        group,
                        descendants_absent,
                    };
                    let expected =
                        leader_reaped && group == GroupState::Absent && descendants_absent;
                    assert_eq!(
                        finish_cleanup(vec![pending_permission_denied()], observation).is_ok(),
                        expected,
                        "{observation:?}",
                    );
                    // No earlier error is not itself proof of cleanup either.
                    assert_eq!(finish_cleanup(vec![], observation).is_ok(), expected);
                }
            }
        }
    }

    #[test]
    fn final_disappearance_keeps_all_hard_errors_and_pending_context() {
        let exited = CleanupObservation {
            leader_reaped: true,
            group: GroupState::Absent,
            descendants_absent: true,
        };
        for message in [
            "descendant signal EPERM",
            "group signal EINVAL",
            "group probe error",
            "identity accounting error",
            "descendant tracking overflow",
            "leader reap error",
        ] {
            assert_eq!(
                finish_cleanup(vec![message.to_string().into()], exited),
                Err(message.into()),
            );
            assert_eq!(
                finish_cleanup(
                    vec![pending_permission_denied(), message.to_string().into()],
                    exited,
                ),
                Err(format!("group SIGKILL EPERM; {message}")),
            );
        }
        // Both TERM and KILL may be denied before the final exit boundary.
        assert_eq!(
            finish_cleanup(
                vec![pending_permission_denied(), pending_permission_denied()],
                exited,
            ),
            Ok(()),
        );
    }

    #[test]
    fn delayed_reap_requires_a_fresh_absent_group() {
        let mut waits = 0;
        let observed = observe_exit::<&str>(
            false,
            || {
                waits += 1;
                Ok(true)
            },
            || Ok(GroupState::Absent),
        )
        .unwrap();
        assert_eq!(waits, 1);
        assert!(!observed.initial_reaped);
        assert!(observed.final_reaped);
        assert!(observed.fully_exited());
    }

    #[test]
    fn already_reaped_leader_does_not_wait_again() {
        let observed = observe_exit::<&str>(
            true,
            || panic!("unnecessary wait"),
            || Ok(GroupState::Absent),
        )
        .unwrap();
        assert!(observed.fully_exited());
    }

    #[test]
    fn live_or_denied_group_remains_a_failure_even_after_reap() {
        for group in [GroupState::Present, GroupState::PermissionDenied] {
            let observed = observe_exit::<&str>(false, || Ok(true), || Ok(group)).unwrap();
            assert!(observed.group.exists());
            assert!(!observed.fully_exited());
        }
    }

    #[test]
    fn absent_group_does_not_substitute_for_leader_reap() {
        let observed =
            observe_exit::<&str>(false, || Ok(false), || Ok(GroupState::Absent)).unwrap();
        assert!(!observed.fully_exited());
    }

    #[test]
    fn wait_and_probe_errors_are_not_success() {
        assert_eq!(
            observe_exit(false, || Err("reap failure"), || Ok(GroupState::Absent)),
            Err("reap failure")
        );
        assert_eq!(
            observe_exit(true, || Ok(true), || Err("probe failure")),
            Err("probe failure")
        );
    }
}
