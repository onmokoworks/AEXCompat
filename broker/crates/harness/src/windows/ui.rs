use egui_shadcn::{
    CardProps, CardSize, CardVariant, ColorPalette, ControlSize, ControlVariant, ShadcnBaseColor,
    Theme, button, card,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum UiLanguage {
    #[default]
    English,
    Japanese,
}

#[derive(Clone, Debug)]
struct AexUiKit {
    theme: Theme,
    dark_mode: bool,
    language: UiLanguage,
    fonts_installed: bool,
}

impl Default for AexUiKit {
    fn default() -> Self {
        Self {
            theme: Theme::new(ColorPalette::shadcn_dark(ShadcnBaseColor::Neutral)),
            dark_mode: true,
            language: UiLanguage::English,
            fonts_installed: false,
        }
    }
}

impl AexUiKit {
    fn install(&mut self, ctx: &egui::Context) {
        if !self.fonts_installed {
            self.install_japanese_font(ctx);
            self.fonts_installed = true;
        }
        let palette = &self.theme.palette;
        let mut visuals = if self.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
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

    fn install_japanese_font(&self, ctx: &egui::Context) {
        let candidates = [
            r"C:\Windows\Fonts\YuGothM.ttc",
            r"C:\Windows\Fonts\meiryo.ttc",
            r"C:\Windows\Fonts\msgothic.ttc",
        ];
        let Some(font_bytes) = candidates
            .iter()
            .find_map(|path| std::fs::read(path).ok())
        else {
            return;
        };
        ctx.add_font(egui::epaint::text::FontInsert::new(
            "aexcompat-japanese",
            egui::FontData::from_owned(font_bytes),
            vec![
                egui::epaint::text::InsertFontFamily {
                    family: egui::FontFamily::Proportional,
                    priority: egui::epaint::text::FontPriority::Lowest,
                },
                egui::epaint::text::InsertFontFamily {
                    family: egui::FontFamily::Monospace,
                    priority: egui::epaint::text::FontPriority::Lowest,
                },
            ],
        ));
    }

    fn toggle_color_mode(&mut self) {
        self.dark_mode = !self.dark_mode;
        let palette = if self.dark_mode {
            ColorPalette::shadcn_dark(ShadcnBaseColor::Neutral)
        } else {
            ColorPalette::shadcn_light(ShadcnBaseColor::Neutral)
        };
        self.theme = Theme::new(palette);
    }

    fn toggle_language(&mut self) {
        self.language = match self.language {
            UiLanguage::English => UiLanguage::Japanese,
            UiLanguage::Japanese => UiLanguage::English,
        };
    }

    fn text(&self, english: &'static str, japanese: &'static str) -> &'static str {
        match self.language {
            UiLanguage::English => english,
            UiLanguage::Japanese => japanese,
        }
    }

    fn color_mode_action_label(&self) -> &'static str {
        if self.dark_mode {
            self.text("Light", "ライト")
        } else {
            self.text("Dark", "ダーク")
        }
    }

    fn language_action_label(&self) -> &'static str {
        match self.language {
            UiLanguage::English => "JA",
            UiLanguage::Japanese => "EN",
        }
    }

    fn muted_foreground(&self) -> egui::Color32 {
        self.theme.palette.muted_foreground
    }

    fn success_foreground(&self) -> egui::Color32 {
        if self.dark_mode {
            egui::Color32::from_rgb(100, 205, 150)
        } else {
            egui::Color32::from_rgb(24, 105, 65)
        }
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
