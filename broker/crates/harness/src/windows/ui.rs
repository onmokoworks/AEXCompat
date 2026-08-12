use egui_shadcn::{
    Button as ShadcnButton, ButtonSize, ButtonStyle, ButtonVariant, CardProps, CardSize,
    CardVariant, ColorPalette, ControlSize, ControlVariant, ShadcnBaseColor, Theme, card, checkbox,
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
            r"C:\Windows\Fonts\meiryo.ttc",
            r"C:\Windows\Fonts\YuGothM.ttc",
            r"C:\Windows\Fonts\msgothic.ttc",
        ];
        let Some(font_bytes) = candidates.iter().find_map(|path| std::fs::read(path).ok()) else {
            return;
        };
        let font_data = egui::FontData::from_owned(font_bytes).tweak(egui::FontTweak {
            scale: 1.0,
            y_offset_factor: 0.12,
            y_offset: 0.0,
        });
        ctx.add_font(egui::epaint::text::FontInsert::new(
            "aexcompat-japanese",
            font_data,
            vec![
                egui::epaint::text::InsertFontFamily {
                    family: egui::FontFamily::Proportional,
                    priority: egui::epaint::text::FontPriority::Highest,
                },
                egui::epaint::text::InsertFontFamily {
                    family: egui::FontFamily::Monospace,
                    priority: egui::epaint::text::FontPriority::Highest,
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

    fn set_language(&mut self, language: UiLanguage) {
        self.language = language;
    }

    fn language_is(&self, language: UiLanguage) -> bool {
        self.language == language
    }

    fn text(&self, english: &'static str, japanese: &'static str) -> &'static str {
        match self.language {
            UiLanguage::English => english,
            UiLanguage::Japanese => japanese,
        }
    }

    fn status_text<'a>(&self, status: &'a str) -> std::borrow::Cow<'a, str> {
        if self.language == UiLanguage::English {
            return status.into();
        }
        let translated = match status {
            "Select an AEX file. Effect Controls inspection runs in an isolated worker." => {
                "AEXファイルを選択してください。エフェクトコントロールは隔離ワーカーで検査されます。"
            }
            "Computing AEX identity..." => "AEXの識別情報を確認しています...",
            "Loading Effect Controls..." => "エフェクトコントロールを読み込んでいます...",
            "Input image loaded. Ready to render." => {
                "入力画像を読み込みました。レンダーできます。"
            }
            "Input image could not be decoded." => "入力画像をデコードできませんでした。",
            "Image drop ignored while a native task is running." => {
                "処理中のため画像のドロップを受け付けませんでした。"
            }
            "Drop exactly one supported image file." => {
                "対応している画像ファイルを1枚だけドロップしてください。"
            }
            "Audio input selected. Ready to render." => {
                "音声入力を選択しました。レンダーできます。"
            }
            "AE reference image loaded." => "AE参照画像を読み込みました。",
            "AE reference image could not be decoded." => "AE参照画像をデコードできませんでした。",
            "AEX identity is unchanged." => "AEXの識別情報に変更はありません。",
            "Could not reload the selected AEX." => "選択したAEXを再読み込みできませんでした。",
            "Rendering through the resident session..." => "常駐セッションでレンダーしています...",
            "Rendering in an isolated worker..." => "隔離ワーカーでレンダーしています...",
            "Rendering audio in an isolated worker..." => {
                "隔離ワーカーで音声をレンダーしています..."
            }
            "Completed" => "完了しました",
            "Failed safely" => "安全に停止しました",
            "AEX output ready." => "AEX出力を表示しました。",
            "AEX output could not be loaded." => "AEX出力を読み込めませんでした。",
            "Required render worker is missing or unreadable." => {
                "必要なレンダーワーカーが見つからないか、読み込めません。"
            }
            "Dependency list cleared." => "依存DLL一覧を消去しました。",
            "Effect Controls inspection has invalid dependency folders." => {
                "依存フォルダーが無効なため、エフェクトコントロールを検査できません。"
            }
            "Render blocked: dependency folders are invalid." => {
                "依存フォルダーが無効なため、レンダーを停止しました。"
            }
            "Effect Controls capability inspection failed safely; rendering is blocked." => {
                "エフェクトコントロールの機能検査が安全に停止しました。レンダーは無効です。"
            }
            _ => return status.into(),
        };
        translated.into()
    }

    fn muted_foreground(&self) -> egui::Color32 {
        self.theme.palette.muted_foreground
    }

    fn separator_color(&self) -> egui::Color32 {
        self.theme.palette.border
    }

    fn success_foreground(&self) -> egui::Color32 {
        if self.dark_mode {
            egui::Color32::from_rgb(100, 205, 150)
        } else {
            egui::Color32::from_rgb(24, 105, 65)
        }
    }

    fn workflow_label(
        &self,
        ui: &mut egui::Ui,
        text: &str,
        color: egui::Color32,
    ) -> egui::Response {
        let font = egui::FontId::proportional(11.0);
        let galley = ui
            .painter()
            .layout_no_wrap(text.to_owned(), font.clone(), color);
        let height = ButtonSize::Sm.height();
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(galley.size().x, height), egui::Sense::hover());
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            font,
            color,
        );
        response
    }

    fn primary_button(
        &self,
        ui: &mut egui::Ui,
        label: impl Into<egui::WidgetText>,
        enabled: bool,
    ) -> egui::Response {
        self.button_with_variant(ui, label, enabled, ControlVariant::Primary, ControlSize::Md)
    }

    fn compact_button(
        &self,
        ui: &mut egui::Ui,
        label: impl Into<egui::WidgetText>,
        enabled: bool,
    ) -> egui::Response {
        self.button_with_variant(
            ui,
            label,
            enabled,
            ControlVariant::Secondary,
            ControlSize::Sm,
        )
    }

    fn compact_primary_button(
        &self,
        ui: &mut egui::Ui,
        label: impl Into<egui::WidgetText>,
        enabled: bool,
    ) -> egui::Response {
        self.button_with_variant(ui, label, enabled, ControlVariant::Primary, ControlSize::Sm)
    }

    fn eye_button(
        &self,
        ui: &mut egui::Ui,
        effect_visible: bool,
        enabled: bool,
        accessible_label: &str,
    ) -> egui::Response {
        let desired_size = egui::vec2(34.0, 30.0);
        let response = ui
            .add_enabled_ui(enabled, |ui| {
                ui.allocate_response(desired_size, egui::Sense::click())
            })
            .inner;
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Checkbox,
                enabled,
                effect_visible,
                accessible_label,
            )
        });
        let visuals = ui.style().interact(&response);
        ui.painter().rect(
            response.rect,
            6.0,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );

        let center = response.rect.center();
        let half_width = 9.5;
        let half_height = 5.5;
        let mut outline = Vec::with_capacity(18);
        for index in 0..=8 {
            let t = index as f32 / 8.0;
            outline.push(egui::pos2(
                center.x - half_width + half_width * 2.0 * t,
                center.y - half_height * (std::f32::consts::PI * t).sin(),
            ));
        }
        for index in 0..=8 {
            let t = index as f32 / 8.0;
            outline.push(egui::pos2(
                center.x + half_width - half_width * 2.0 * t,
                center.y + half_height * (std::f32::consts::PI * t).sin(),
            ));
        }
        ui.painter().add(egui::Shape::closed_line(
            outline,
            egui::Stroke::new(1.6, visuals.fg_stroke.color),
        ));
        ui.painter()
            .circle_filled(center, 3.6, visuals.fg_stroke.color);
        if !effect_visible {
            ui.painter().line_segment(
                [
                    egui::pos2(center.x - 10.5, center.y - 8.0),
                    egui::pos2(center.x + 10.5, center.y + 8.0),
                ],
                egui::Stroke::new(2.2, visuals.fg_stroke.color),
            );
        }
        response
    }

    fn segmented_toggle(
        &self,
        ui: &mut egui::Ui,
        id_source: impl std::hash::Hash,
        checked: bool,
        off_label: &str,
        on_label: &str,
    ) -> egui::Response {
        let desired_size = egui::vec2(92.0, 32.0);
        let rect = ui.allocate_space(desired_size).1;
        let id = ui.make_persistent_id(id_source);
        let response = ui.interact(rect, id, egui::Sense::click());
        let accessible_label = format!("{off_label}/{on_label}");
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Checkbox,
                true,
                checked,
                accessible_label.clone(),
            )
        });
        let progress = ui.ctx().animate_bool_with_time(response.id, checked, 0.18);
        let visuals = ui.style().interact(&response);
        ui.painter().rect(
            response.rect,
            8.0,
            self.theme.palette.muted,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );

        let inset = 3.0;
        let segment_width = (response.rect.width() - inset * 2.0) * 0.5;
        let left = response.rect.left() + inset + segment_width * progress;
        let thumb = egui::Rect::from_min_size(
            egui::pos2(left, response.rect.top() + inset),
            egui::vec2(segment_width, response.rect.height() - inset * 2.0),
        );
        ui.painter()
            .rect_filled(thumb, 6.0, self.theme.palette.background);

        let font = egui::FontId::proportional(12.0);
        let active = self.theme.palette.foreground;
        let inactive = self.theme.palette.muted_foreground;
        let left_center = egui::pos2(
            response.rect.left() + inset + segment_width * 0.5,
            response.rect.center().y,
        );
        let right_center = egui::pos2(left_center.x + segment_width, left_center.y);
        ui.painter().text(
            left_center,
            egui::Align2::CENTER_CENTER,
            off_label,
            font.clone(),
            if checked { inactive } else { active },
        );
        ui.painter().text(
            right_center,
            egui::Align2::CENTER_CENTER,
            on_label,
            font,
            if checked { active } else { inactive },
        );
        response
    }

    fn modern_slider(
        &self,
        ui: &mut egui::Ui,
        value: &mut f64,
        range: std::ops::RangeInclusive<f64>,
        accessible_label: &str,
    ) -> egui::Response {
        let minimum = *range.start();
        let maximum = *range.end();
        let previous = *value;
        let enabled = ui.is_enabled();
        let mut displayed_value = value.clamp(minimum, maximum);
        let (track_response, numeric_response) = ui
            .horizontal(|ui| {
                let desired = egui::vec2(ui.available_width().min(180.0).max(96.0), 28.0);
                let (rect, mut response) =
                    ui.allocate_exact_size(desired, egui::Sense::click_and_drag());
                if response.clicked() {
                    response.request_focus();
                }
                if response.dragged() || response.clicked() {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        let fraction = ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                        *value = minimum + (maximum - minimum) * f64::from(fraction);
                    }
                }
                if response.enabled() && response.has_focus() && maximum > minimum {
                    ui.memory_mut(|memory| {
                        memory.set_focus_lock_filter(
                            response.id,
                            egui::EventFilter {
                                horizontal_arrows: true,
                                vertical_arrows: true,
                                ..Default::default()
                            },
                        );
                    });
                    let step = (maximum - minimum) / 100.0;
                    let (delta, home, end) = ui.input(|input| {
                        let increment = input.key_pressed(egui::Key::ArrowRight)
                            || input.key_pressed(egui::Key::ArrowUp);
                        let decrement = input.key_pressed(egui::Key::ArrowLeft)
                            || input.key_pressed(egui::Key::ArrowDown);
                        (
                            f64::from(increment) - f64::from(decrement),
                            input.key_pressed(egui::Key::Home),
                            input.key_pressed(egui::Key::End),
                        )
                    });
                    if home {
                        *value = minimum;
                    } else if end {
                        *value = maximum;
                    } else {
                        *value += delta * step;
                    }
                }
                if response.enabled() && maximum > minimum {
                    let step = (maximum - minimum) / 100.0;
                    let access_delta = ui.input(|input| {
                        input.num_accesskit_action_requests(
                            response.id,
                            egui::accesskit::Action::Increment,
                        ) as f64
                            - input.num_accesskit_action_requests(
                                response.id,
                                egui::accesskit::Action::Decrement,
                            ) as f64
                    });
                    *value += access_delta * step;
                    ui.input(|input| {
                        for request in input.accesskit_action_requests(
                            response.id,
                            egui::accesskit::Action::SetValue,
                        ) {
                            if let Some(egui::accesskit::ActionData::NumericValue(new_value)) =
                                request.data
                            {
                                *value = new_value.clamp(minimum, maximum);
                            }
                        }
                    });
                }
                if response.enabled() {
                    *value = value.clamp(minimum, maximum);
                }
                if *value != previous {
                    response.mark_changed();
                }
                response.widget_info(|| {
                    egui::WidgetInfo::slider(ui.is_enabled(), *value, accessible_label)
                });
                ui.ctx().accesskit_node_builder(response.id, |builder| {
                    use egui::accesskit::Action;
                    builder.set_min_numeric_value(minimum);
                    builder.set_max_numeric_value(maximum);
                    builder.set_numeric_value_step((maximum - minimum) / 100.0);
                    if enabled {
                        builder.add_action(Action::SetValue);
                    }
                    if enabled && *value < maximum {
                        builder.add_action(Action::Increment);
                    }
                    if enabled && *value > minimum {
                        builder.add_action(Action::Decrement);
                    }
                });
                let visuals = ui.style().interact(&response);
                let primary = if enabled {
                    self.theme.palette.primary
                } else {
                    self.theme.palette.muted.gamma_multiply(0.55)
                };
                let thumb_fill = if enabled {
                    self.theme.palette.background
                } else {
                    self.theme.palette.muted.gamma_multiply(0.55)
                };
                let track = egui::Rect::from_center_size(
                    rect.center(),
                    egui::vec2(rect.width(), if response.hovered() { 3.0 } else { 2.0 }),
                );
                ui.painter()
                    .rect_filled(track, 2.0, self.theme.palette.muted);
                let fraction = if maximum > minimum {
                    ((*value - minimum) / (maximum - minimum)).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                };
                let thumb = egui::pos2(
                    egui::lerp(rect.left()..=rect.right(), fraction),
                    rect.center().y,
                );
                ui.painter().line_segment(
                    [track.left_center(), thumb],
                    egui::Stroke::new(2.0, primary),
                );
                ui.painter().circle_filled(
                    thumb,
                    if response.hovered() || response.dragged() {
                        7.0
                    } else {
                        6.0
                    },
                    thumb_fill,
                );
                ui.painter().circle_stroke(
                    thumb,
                    if response.has_focus() { 7.0 } else { 6.0 },
                    egui::Stroke::new(2.0, visuals.fg_stroke.color),
                );
                let numeric_value = if enabled {
                    &mut *value
                } else {
                    &mut displayed_value
                };
                let numeric = ui.add(
                    egui::DragValue::new(numeric_value)
                        .range(range)
                        .speed(((maximum - minimum).abs() / 200.0).max(0.01)),
                );
                (response, numeric)
            })
            .inner;
        let mut response = track_response.union(numeric_response.clone());
        if numeric_response.changed() {
            response.mark_changed();
        }
        response
    }

    fn tab_button(
        &self,
        ui: &mut egui::Ui,
        label: impl Into<egui::WidgetText>,
        selected: bool,
    ) -> egui::Response {
        let variant = if selected {
            ControlVariant::Primary
        } else {
            ControlVariant::Ghost
        };
        self.button_with_variant(ui, label, true, variant, ControlSize::Sm)
    }

    fn checkbox(
        &self,
        ui: &mut egui::Ui,
        checked: &mut bool,
        label: impl Into<egui::WidgetText>,
        enabled: bool,
    ) -> egui::Response {
        ui.add_enabled_ui(enabled, |ui| {
            checkbox(
                ui,
                &self.theme,
                checked,
                label,
                ControlVariant::Secondary,
                ControlSize::Sm,
                enabled,
            )
        })
        .inner
    }

    fn button_with_variant(
        &self,
        ui: &mut egui::Ui,
        label: impl Into<egui::WidgetText>,
        enabled: bool,
        variant: ControlVariant,
        size: ControlSize,
    ) -> egui::Response {
        let label = label.into();
        let accessible_label = label.text().to_owned();
        let button_size = ButtonSize::from(size);
        let text_width = ui
            .painter()
            .layout_no_wrap(
                accessible_label.clone(),
                button_size.font(),
                self.theme.palette.foreground,
            )
            .rect
            .width();
        let min_width = (text_width + button_size.padding_x() * 2.0).max(40.0);
        let disabled_style = (!enabled).then(|| self.disabled_button_style(variant));
        let render = |ui: &mut egui::Ui| {
            let mut button = ShadcnButton::new(label)
                .variant(ButtonVariant::from(variant))
                .size(button_size)
                .min_width(min_width)
                .enabled(enabled);
            if let Some(style) = disabled_style {
                button = button.style(style);
            }
            button.show(ui, &self.theme)
        };
        let response = if enabled {
            render(ui)
        } else {
            ui.add_enabled_ui(false, render).inner
        };
        let response = if enabled {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            response
        };
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

    fn disabled_button_style(&self, variant: ControlVariant) -> ButtonStyle {
        let mut style =
            ButtonStyle::from_variant(&self.theme.palette, ButtonVariant::from(variant));
        let (background, foreground, border) = if self.dark_mode {
            (
                egui::Color32::from_rgb(50, 50, 55),
                egui::Color32::from_rgb(158, 158, 166),
                egui::Color32::from_rgb(76, 76, 84),
            )
        } else {
            (
                egui::Color32::from_rgb(226, 226, 230),
                egui::Color32::from_rgb(104, 104, 112),
                egui::Color32::from_rgb(194, 194, 201),
            )
        };
        style.bg = background;
        style.bg_hover = background;
        style.bg_active = background;
        style.text = foreground;
        style.text_hover = foreground;
        style.text_active = foreground;
        style.border = border;
        style.border_hover = border;
        style.disabled_opacity = 1.0;
        style
    }

    fn secondary_button(
        &self,
        ui: &mut egui::Ui,
        label: impl Into<egui::WidgetText>,
        enabled: bool,
    ) -> egui::Response {
        self.button_with_variant(
            ui,
            label,
            enabled,
            ControlVariant::Secondary,
            ControlSize::Md,
        )
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

    #[test]
    fn modern_slider_is_shared_bounded_and_disabled_with_its_ui() {
        egui::__run_test_ui(|ui| {
            let kit = AexUiKit::default();
            let mut value = 25.0;
            let response = kit.modern_slider(ui, &mut value, 0.0..=100.0, "Amount");
            assert!(response.enabled());
            assert_eq!(value, 25.0);
            assert!(response.rect.height() >= 28.0);

            value = 150.0;
            let disabled = ui
                .add_enabled_ui(false, |ui| {
                    kit.modern_slider(ui, &mut value, 0.0..=100.0, "Amount")
                })
                .inner;
            assert!(!disabled.enabled());
            assert_eq!(value, 150.0);
            assert!(!disabled.changed());
        });
    }

    #[test]
    fn focused_modern_slider_ignores_arrow_keys_after_becoming_disabled() {
        let ctx = egui::Context::default();
        let mut value = 25.0;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response =
                    AexUiKit::default().modern_slider(ui, &mut value, 0.0..=100.0, "Amount");
                response.request_focus();
            });
        });

        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut changed = true;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                changed = ui
                    .add_enabled_ui(false, |ui| {
                        AexUiKit::default().modern_slider(ui, &mut value, 0.0..=100.0, "Amount")
                    })
                    .inner
                    .changed();
            });
        });

        assert_eq!(value, 25.0);
        assert!(!changed);
    }

    #[test]
    fn focused_modern_slider_supports_vertical_arrows_and_range_endpoints() {
        for (key, expected) in [
            (egui::Key::ArrowUp, 26.0),
            (egui::Key::ArrowDown, 24.0),
            (egui::Key::Home, 0.0),
            (egui::Key::End, 100.0),
        ] {
            let ctx = egui::Context::default();
            let mut value = 25.0;
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    AexUiKit::default()
                        .modern_slider(ui, &mut value, 0.0..=100.0, "Amount")
                        .request_focus();
                });
            });
            let input = egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            };
            let mut changed = false;
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    changed = AexUiKit::default()
                        .modern_slider(ui, &mut value, 0.0..=100.0, "Amount")
                        .changed();
                });
            });
            assert_eq!(value, expected);
            assert!(changed);
        }
    }

    #[test]
    fn modern_slider_accepts_accesskit_set_value_and_keeps_focus_for_repeated_arrows() {
        let ctx = egui::Context::default();
        let mut value = 25.0;
        let mut slider_id = egui::Id::NULL;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response =
                    AexUiKit::default().modern_slider(ui, &mut value, 0.0..=100.0, "Amount");
                slider_id = response.id;
                response.request_focus();
            });
        });

        for expected in [26.0, 27.0] {
            let input = egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::ArrowUp,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    assert!(
                        AexUiKit::default()
                            .modern_slider(ui, &mut value, 0.0..=100.0, "Amount")
                            .changed()
                    );
                });
            });
            assert_eq!(value, expected);
            assert!(ctx.memory(|memory| memory.has_focus(slider_id)));
        }

        ctx.memory_mut(|memory| memory.surrender_focus(slider_id));
        assert!(!ctx.memory(|memory| memory.has_focus(slider_id)));

        let input = egui::RawInput {
            events: vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::SetValue,
                    target: slider_id.value().into(),
                    data: Some(egui::accesskit::ActionData::NumericValue(75.0)),
                },
            )],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert!(
                    AexUiKit::default()
                        .modern_slider(ui, &mut value, 0.0..=100.0, "Amount")
                        .changed()
                );
            });
        });
        assert_eq!(value, 75.0);
    }
}
