use std::time::{Duration, Instant};

pub(crate) const LIVE_RENDER_DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GuiParameter {
    pub(crate) name: String,
    pub(crate) param_type: i64,
    pub(crate) value: f64,
    pub(crate) default_value: f64,
    pub(crate) minimum: f64,
    pub(crate) maximum: f64,
    pub(crate) precision: usize,
}

impl GuiParameter {
    pub(crate) fn is_default(&self) -> bool {
        self.value == self.default_value
    }

    pub(crate) fn reset(&mut self) -> bool {
        if self.is_default() {
            return false;
        }
        self.value = self.default_value;
        true
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ViewerMode {
    #[default]
    Input,
    Output,
    Compare,
}

impl ViewerMode {
    pub(crate) fn after_successful_render(self, had_output: bool) -> Self {
        if !had_output && self == Self::Input {
            Self::Output
        } else {
            self
        }
    }
}

#[derive(Debug)]
pub(crate) struct LiveRenderState {
    enabled: bool,
    due: Option<Instant>,
}

impl Default for LiveRenderState {
    fn default() -> Self {
        Self {
            enabled: true,
            due: None,
        }
    }
}

impl LiveRenderState {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.due = None;
        }
    }

    pub(crate) fn parameter_changed(&mut self, now: Instant) {
        if self.enabled {
            self.due = Some(now + LIVE_RENDER_DEBOUNCE);
        }
    }

    pub(crate) fn take_due(&mut self, now: Instant, busy: bool, render_ready: bool) -> bool {
        let Some(due) = self.due else {
            return false;
        };
        if busy || !render_ready || now < due {
            return false;
        }
        self.due = None;
        true
    }

    pub(crate) fn remaining(&self, now: Instant) -> Option<Duration> {
        self.due.map(|due| due.saturating_duration_since(now))
    }
}

pub(crate) fn reset_all(parameters: &mut [GuiParameter]) -> bool {
    parameters
        .iter_mut()
        .fold(false, |changed, parameter| parameter.reset() || changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameter(value: f64, default_value: f64) -> GuiParameter {
        GuiParameter {
            name: "Amount".into(),
            param_type: 1,
            value,
            default_value,
            minimum: 0.0,
            maximum: 100.0,
            precision: 0,
        }
    }

    #[test]
    fn parameter_reset_reports_only_real_changes() {
        let mut changed = parameter(50.0, 0.0);
        assert!(changed.reset());
        assert_eq!(changed.value, 0.0);
        assert!(!changed.reset());
    }

    #[test]
    fn reset_all_restores_every_parameter() {
        let mut parameters = [parameter(25.0, 0.0), parameter(70.0, 50.0)];
        assert!(reset_all(&mut parameters));
        assert!(parameters.iter().all(GuiParameter::is_default));
        assert!(!reset_all(&mut parameters));
    }

    #[test]
    fn live_render_debounces_parameter_changes() {
        let started = Instant::now();
        let mut state = LiveRenderState::default();
        state.parameter_changed(started);
        state.parameter_changed(started + Duration::from_millis(300));
        assert!(!state.take_due(started + Duration::from_millis(799), false, true));
        assert!(state.take_due(started + Duration::from_millis(800), false, true));
        assert!(!state.take_due(started + Duration::from_secs(2), false, true));
    }

    #[test]
    fn live_render_waits_for_readiness_and_disables_cleanly() {
        let started = Instant::now();
        let mut state = LiveRenderState::default();
        state.parameter_changed(started);
        let due = started + LIVE_RENDER_DEBOUNCE;
        assert!(!state.take_due(due, true, true));
        assert!(!state.take_due(due, false, false));
        assert!(state.take_due(due, false, true));

        state.parameter_changed(started);
        state.set_enabled(false);
        assert!(!state.enabled());
        assert!(!state.take_due(started + Duration::from_secs(1), false, true));
    }

    #[test]
    fn first_render_reveals_output_without_overriding_later_viewer_choices() {
        assert_eq!(
            ViewerMode::Input.after_successful_render(false),
            ViewerMode::Output
        );
        assert_eq!(
            ViewerMode::Compare.after_successful_render(false),
            ViewerMode::Compare
        );
        assert_eq!(
            ViewerMode::Input.after_successful_render(true),
            ViewerMode::Input
        );
        assert_eq!(
            ViewerMode::Compare.after_successful_render(true),
            ViewerMode::Compare
        );
    }
}
