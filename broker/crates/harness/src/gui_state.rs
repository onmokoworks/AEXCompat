#[cfg(any(target_os = "macos", test))]
use std::time::{Duration, Instant};

#[cfg(any(target_os = "macos", test))]
pub(crate) const LIVE_RENDER_DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AnalysisPaneState {
    pub(crate) open: bool,
    pub(crate) width: f32,
}

impl Default for AnalysisPaneState {
    fn default() -> Self {
        Self {
            open: true,
            width: 360.0,
        }
    }
}

impl AnalysisPaneState {
    pub(crate) const MIN_WIDTH: f32 = 280.0;
    pub(crate) const MAX_WIDTH: f32 = 620.0;

    pub(crate) fn set_width(&mut self, width: f32) {
        self.width = width.clamp(Self::MIN_WIDTH, Self::MAX_WIDTH);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg(any(target_os = "macos", test))]
pub(crate) enum ViewerMode {
    #[default]
    Input,
    Output,
    Compare,
}

#[cfg(any(target_os = "macos", test))]
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
#[cfg(any(target_os = "macos", test))]
pub(crate) struct LiveRenderState {
    enabled: bool,
    due: Option<Instant>,
}

#[cfg(any(target_os = "macos", test))]
impl Default for LiveRenderState {
    fn default() -> Self {
        Self {
            enabled: true,
            due: None,
        }
    }
}

#[cfg(any(target_os = "macos", test))]
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

    #[cfg(target_os = "macos")]
    pub(crate) fn remaining(&self, now: Instant) -> Option<Duration> {
        self.due.map(|due| due.saturating_duration_since(now))
    }
}

pub(crate) fn parameter_is_default(
    parameter: &aexcompat_broker::render_fixture::InteractiveParameter,
    default: &aexcompat_broker::render_fixture::InteractiveParameter,
) -> bool {
    parameter.slot == default.slot
        && parameter.value == default.value
        && parameter.color == default.color
        && parameter.components == default.components
        && parameter.layer_path == default.layer_path
        && parameter.debug_summary == default.debug_summary
}

pub(crate) fn reset_parameter(
    parameter: &mut aexcompat_broker::render_fixture::InteractiveParameter,
    default: &aexcompat_broker::render_fixture::InteractiveParameter,
) -> bool {
    if parameter_is_default(parameter, default) {
        return false;
    }
    parameter.value = default.value;
    parameter.color = default.color;
    parameter.components = default.components;
    parameter.layer_path.clone_from(&default.layer_path);
    parameter.debug_summary.clone_from(&default.debug_summary);
    true
}

pub(crate) fn reset_all(
    parameters: &mut [aexcompat_broker::render_fixture::InteractiveParameter],
    defaults: &[aexcompat_broker::render_fixture::InteractiveParameter],
) -> bool {
    if parameters.len() != defaults.len()
        || parameters
            .iter()
            .zip(defaults)
            .any(|(parameter, default)| parameter.slot != default.slot)
    {
        return false;
    }
    parameters
        .iter_mut()
        .zip(defaults)
        .fold(false, |changed, (parameter, default)| {
            reset_parameter(parameter, default) || changed
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameter(value: f64) -> aexcompat_broker::render_fixture::InteractiveParameter {
        aexcompat_broker::render_fixture::InteractiveParameter {
            slot: 1,
            name: "Amount".into(),
            kind: "integer".into(),
            minimum: 0.0,
            maximum: 100.0,
            value,
            choices: Vec::new(),
            color: [255, 0, 0, 0],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0; 2],
        }
    }

    #[test]
    fn parameter_reset_reports_only_real_changes() {
        let default = parameter(0.0);
        let mut changed = parameter(50.0);
        assert!(reset_parameter(&mut changed, &default));
        assert_eq!(changed.value, 0.0);
        assert!(!reset_parameter(&mut changed, &default));
    }

    #[test]
    fn parameter_reset_preserves_dynamic_ui_and_descriptor_state() {
        let default = parameter(0.0);
        let mut changed = parameter(50.0);
        changed.enabled = false;
        changed.visible = false;
        changed.supervised = true;
        changed.choices = vec!["runtime choice".into()];
        changed.minimum = -50.0;
        changed.maximum = 250.0;
        changed.custom_ui_events = 7;
        changed.control_size = [320, 24];

        assert!(reset_parameter(&mut changed, &default));
        assert_eq!(changed.value, default.value);
        assert!(!changed.enabled);
        assert!(!changed.visible);
        assert!(changed.supervised);
        assert_eq!(changed.choices, ["runtime choice"]);
        assert_eq!((changed.minimum, changed.maximum), (-50.0, 250.0));
        assert_eq!(changed.custom_ui_events, 7);
        assert_eq!(changed.control_size, [320, 24]);
        assert!(parameter_is_default(&changed, &default));
    }

    #[test]
    fn reset_all_restores_every_parameter() {
        let defaults = [parameter(0.0), parameter(50.0)];
        let mut parameters = [parameter(25.0), parameter(70.0)];
        parameters[1].slot = 2;
        let mut defaults = defaults;
        defaults[1].slot = 2;
        assert!(reset_all(&mut parameters, &defaults));
        assert!(
            parameters
                .iter()
                .zip(&defaults)
                .all(|(a, b)| parameter_is_default(a, b))
        );
        assert!(!reset_all(&mut parameters, &defaults));
    }

    #[test]
    fn analysis_pane_width_is_bounded() {
        let mut state = AnalysisPaneState::default();
        state.set_width(10.0);
        assert_eq!(state.width, AnalysisPaneState::MIN_WIDTH);
        state.set_width(10_000.0);
        assert_eq!(state.width, AnalysisPaneState::MAX_WIDTH);
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
