use eframe::egui;

pub(crate) const AEXCOMPAT_LICENSE: &str = include_str!("../../../../LICENSE");
pub(crate) const THIRD_PARTY_LICENSES: &str = include_str!("../../../../THIRD_PARTY_LICENSES.txt");

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum LicensePage {
    #[default]
    AexCompat,
    ThirdParty,
}

#[derive(Debug, Default)]
pub(crate) struct LicenseWindow {
    open: bool,
    page: LicensePage,
}

impl LicenseWindow {
    pub(crate) fn about_button(&mut self, ui: &mut egui::Ui, label: &str) -> egui::Response {
        let response = ui.button(label);
        if response.clicked() {
            self.open = true;
        }
        response
    }

    fn document(&self) -> &'static str {
        match self.page {
            LicensePage::AexCompat => AEXCOMPAT_LICENSE,
            LicensePage::ThirdParty => THIRD_PARTY_LICENSES,
        }
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context) -> Option<&'static str> {
        if !self.open {
            return None;
        }
        let mut open = self.open;
        egui::Window::new("About AEXCompat")
            .open(&mut open)
            .default_size(egui::vec2(760.0, 620.0))
            .min_size(egui::vec2(480.0, 320.0))
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("AEXCompat");
                ui.label("Desktop AEX compatibility harness");
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.page,
                        LicensePage::AexCompat,
                        "AEXCompat license",
                    );
                    ui.selectable_value(
                        &mut self.page,
                        LicensePage::ThirdParty,
                        "Third-party licenses",
                    );
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(self.document()).monospace())
                                .selectable(true)
                                .wrap(),
                        );
                    });
            });
        self.open = open;
        Some(self.document())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_about_button_opens_the_window_and_each_page_renders_its_document() {
        let ctx = egui::Context::default();
        let mut window = LicenseWindow::default();
        let mut button_rect = egui::Rect::NOTHING;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                button_rect = window.about_button(ui, "About").rect;
            });
        });
        assert!(!window.open);
        let input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(button_rect.center()),
                egui::Event::PointerButton {
                    pos: button_rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: button_rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                window.about_button(ui, "About");
            });
        });
        assert!(window.open);
        assert_eq!(window.show(&ctx), Some(AEXCOMPAT_LICENSE));
        window.page = LicensePage::ThirdParty;
        assert_eq!(window.show(&ctx), Some(THIRD_PARTY_LICENSES));
    }
}
