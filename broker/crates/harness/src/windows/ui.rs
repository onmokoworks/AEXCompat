use egui_shadcn::{
    CardProps, CardSize, CardVariant, ControlSize, ControlVariant, Theme, button, card,
};

#[derive(Clone, Debug, Default)]
struct AexUiKit {
    theme: Theme,
}

impl AexUiKit {
    fn install(&self, ctx: &egui::Context) {
        let palette = &self.theme.palette;
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = palette.background;
        visuals.window_fill = palette.background;
        visuals.faint_bg_color = palette.muted;
        visuals.selection.bg_fill = palette.primary;
        visuals.selection.stroke.color = palette.primary_foreground;
        visuals.widgets.noninteractive.fg_stroke.color = palette.foreground;
        ctx.set_visuals(visuals);
        ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 6.0);
        });
    }

    fn primary_button(
        &self,
        ui: &mut egui::Ui,
        label: impl Into<egui::WidgetText>,
        enabled: bool,
    ) -> egui::Response {
        let label = label.into();
        let accessible_label = label.text().to_owned();
        let response = ui
            .add_enabled_ui(enabled, |ui| {
                button(
                    ui,
                    &self.theme,
                    label,
                    ControlVariant::Primary,
                    ControlSize::Md,
                    enabled,
                )
            })
            .inner;
        let response_enabled = response.enabled();
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                response_enabled,
                accessible_label.clone(),
            )
        });
        response
    }

    fn secondary_button(
        &self,
        ui: &mut egui::Ui,
        label: impl Into<egui::WidgetText>,
        enabled: bool,
    ) -> egui::Response {
        let label = label.into();
        let accessible_label = label.text().to_owned();
        let response = ui
            .add_enabled_ui(enabled, |ui| {
                button(
                    ui,
                    &self.theme,
                    label,
                    ControlVariant::Secondary,
                    ControlSize::Md,
                    enabled,
                )
            })
            .inner;
        let response_enabled = response.enabled();
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                response_enabled,
                accessible_label.clone(),
            )
        });
        response
    }

    fn onboarding_card(&self, ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
        let props = CardProps::default()
            .with_id(ui.make_persistent_id("aexcompat-onboarding-card"))
            .with_variant(CardVariant::Surface)
            .with_size(CardSize::Size5)
            .with_shadow(false);
        card(ui, &self.theme, props, add_contents);
    }
}

#[cfg(test)]
mod ui_kit_tests {
    use super::*;

    #[test]
    fn disabled_shadcn_buttons_are_non_interactive() {
        egui::__run_test_ui(|ui| {
            let kit = AexUiKit::default();
            let primary = kit.primary_button(ui, "Primary", false);
            let secondary = kit.secondary_button(ui, "Secondary", false);

            assert!(!primary.enabled());
            assert!(!primary.clicked());
            assert!(!secondary.enabled());
            assert!(!secondary.clicked());
        });
    }
}
