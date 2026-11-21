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

pub(crate) fn parameter_is_default(
    parameter: &aexcompat_broker::render_fixture::InteractiveParameter,
    default: &aexcompat_broker::render_fixture::InteractiveParameter,
) -> bool {
    parameter.slot == default.slot
        && parameter.value == default.value
        && parameter.use_declared_default == default.use_declared_default
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
    parameter.use_declared_default = default.use_declared_default;
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
            use_declared_default: false,
            color_float: None,
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
}

/// Both backends park the complete editor for each stage.
pub(crate) const MAX_EFFECT_CHAIN_STAGES: usize = 8;

#[derive(Clone)]
pub(crate) struct EffectChainSlot<T> {
    pub(crate) enabled: bool,
    pub(crate) editor: T,
}
