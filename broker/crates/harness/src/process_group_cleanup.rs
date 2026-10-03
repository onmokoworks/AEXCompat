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

// Diagnostic-only interpretation of the PID/PPID/PGID/status observation.
// Unavailable/moved samples are not proof of absence or permission to signal.
#[cfg(test)]
pub(crate) fn member_state(pid: u32, group: u32, sample: Option<[u32; 4]>) -> &'static str {
    match sample {
        None => "unavailable",
        Some([observed, _, _, _]) if observed != pid => "pid-mismatch",
        Some([_, _, observed, _]) if observed != group => "group-changed",
        Some([_, _, _, 5]) => "zombie",
        Some(_) => "live-or-other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_members_keep_unknown_and_live_distinct_from_zombies() {
        assert_eq!(member_state(7, 3, None), "unavailable");
        assert_eq!(member_state(7, 3, Some([8, 1, 3, 5])), "pid-mismatch");
        assert_eq!(member_state(7, 3, Some([7, 1, 4, 5])), "group-changed");
        assert_eq!(member_state(7, 3, Some([7, 1, 3, 5])), "zombie");
        for status in [0, 1, 2, 3, 4, 6] {
            assert_eq!(member_state(7, 3, Some([7, 1, 3, status])), "live-or-other");
        }
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
