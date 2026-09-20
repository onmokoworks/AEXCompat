use std::path::{Path, PathBuf};

use aexcompat_broker::render_fixture::InteractiveParameter;
use eframe::egui::{self, Color32, RichText};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkspaceRegion {
    EffectControls,
    Viewer,
}

pub(crate) fn show_workspace_body(
    ctx: &egui::Context,
    mut show: impl FnMut(WorkspaceRegion, &mut egui::Ui),
) {
    egui::SidePanel::left("effect_controls")
        .default_width(340.0)
        .min_width(260.0)
        .max_width(460.0)
        .resizable(true)
        .show(ctx, |ui| show(WorkspaceRegion::EffectControls, ui));
    egui::CentralPanel::default().show(ctx, |ui| show(WorkspaceRegion::Viewer, ui));
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AnalysisSection {
    ProjectSession,
    Advanced,
    RenderSettings,
    Output,
}

impl AnalysisSection {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 4] = [
        Self::ProjectSession,
        Self::Advanced,
        Self::RenderSettings,
        Self::Output,
    ];

    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::ProjectSession => "PROJECT / SESSION SETTINGS",
            Self::Advanced => "ADVANCED",
            Self::RenderSettings => "RENDER SETTINGS",
            Self::Output => "ANALYSIS / LOG OUTPUT",
        }
    }
}

pub(crate) fn analysis_section_heading(ui: &mut egui::Ui, title: &str, color: Color32) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).small().strong().color(color));
    ui.separator();
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EffectControlIntent {
    Changed { slot: u32, supervised: bool },
    Reset { slot: u32, supervised: bool },
    TriggerButton { slot: u32 },
    ChooseLayer { slot: u32 },
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EffectControlRole {
    GroupStart,
    GroupEnd,
    Editor,
    Reset,
    Button,
    ChooseLayer,
}

pub(crate) struct EffectControlsOutput {
    pub(crate) intents: Vec<EffectControlIntent>,
    #[cfg(test)]
    pub(crate) widgets: Vec<(u32, EffectControlRole, egui::Response)>,
}

#[derive(Clone, Copy)]
pub(crate) struct EffectControlsText<'a> {
    pub(crate) reset: &'a str,
    pub(crate) enabled: &'a str,
    pub(crate) choose_image: &'a str,
    pub(crate) not_connected: &'a str,
    pub(crate) read_only: &'a str,
    pub(crate) layer_unavailable: &'a str,
    pub(crate) button_unavailable: &'a str,
}

impl Default for EffectControlsText<'static> {
    fn default() -> Self {
        Self {
            reset: "Reset",
            enabled: "Enabled",
            choose_image: "Choose image",
            not_connected: "Not connected",
            read_only: "read-only",
            layer_unavailable: "Layer selection is unavailable in this backend.",
            button_unavailable: "Button dispatch is unavailable in this backend.",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct EffectControlCapabilities {
    pub(crate) choose_layer: bool,
    pub(crate) trigger_button: bool,
}

pub(crate) fn show_effect_controls(
    ui: &mut egui::Ui,
    parameters: &mut [InteractiveParameter],
    defaults: &[InteractiveParameter],
    busy: bool,
    capabilities: EffectControlCapabilities,
    text: EffectControlsText<'_>,
) -> EffectControlsOutput {
    let mut intents = Vec::new();
    #[cfg(test)]
    let mut widgets = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for parameter in parameters {
                if !parameter.visible {
                    continue;
                }
                if parameter.kind == "group_start" {
                    ui.add_space(8.0);
                    let _response = ui.label(RichText::new(&parameter.name).strong());
                    #[cfg(test)]
                    widgets.push((parameter.slot, EffectControlRole::GroupStart, _response));
                    continue;
                }
                if parameter.kind == "group_end" {
                    let _response = ui.separator();
                    #[cfg(test)]
                    widgets.push((parameter.slot, EffectControlRole::GroupEnd, _response));
                    continue;
                }
                if matches!(parameter.kind.as_str(), "button" | "compatibility_action") {
                    let response = ui
                        .add_enabled(
                            parameter.enabled && !busy && capabilities.trigger_button,
                            egui::Button::new(&parameter.name),
                        )
                        .on_disabled_hover_text(text.button_unavailable);
                    if response.clicked() {
                        intents.push(EffectControlIntent::TriggerButton {
                            slot: parameter.slot,
                        });
                    }
                    #[cfg(test)]
                    widgets.push((parameter.slot, EffectControlRole::Button, response));
                    continue;
                }
                if matches!(parameter.kind.as_str(), "custom" | "no_data") {
                    ui.label(&parameter.name);
                    ui.small(format!("{} ({})", parameter.kind, text.read_only));
                    continue;
                }

                let previous_value = parameter.value;
                let previous_color = parameter.color;
                let previous_components = parameter.components;
                let previous_summary = parameter.debug_summary.clone();
                let _editor = ui.add_enabled_ui(parameter.enabled && !busy, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&parameter.name).small());
                        if let Some(default) = defaults
                            .iter()
                            .find(|default| default.slot == parameter.slot)
                        {
                            let reset = ui.add_enabled(
                                !crate::gui_state::parameter_is_default(parameter, default),
                                egui::Button::new(text.reset).small(),
                            );
                            if reset.clicked() {
                                intents.push(EffectControlIntent::Reset {
                                    slot: parameter.slot,
                                    supervised: parameter.supervised,
                                });
                            }
                            #[cfg(test)]
                            widgets.push((parameter.slot, EffectControlRole::Reset, reset));
                        }
                    });
                    match parameter.kind.as_str() {
                        "layer" => {
                            ui.horizontal(|ui| {
                                let response = ui
                                    .add_enabled(
                                        capabilities.choose_layer,
                                        egui::Button::new(text.choose_image).small(),
                                    )
                                    .on_disabled_hover_text(text.layer_unavailable);
                                if response.clicked() {
                                    intents.push(EffectControlIntent::ChooseLayer {
                                        slot: parameter.slot,
                                    });
                                }
                                #[cfg(test)]
                                widgets.push((
                                    parameter.slot,
                                    EffectControlRole::ChooseLayer,
                                    response,
                                ));
                                ui.label(
                                    parameter
                                        .layer_path
                                        .as_ref()
                                        .and_then(|path| path.file_name())
                                        .and_then(|name| name.to_str())
                                        .unwrap_or(text.not_connected),
                                );
                            });
                        }
                        "arbitrary_data" => {
                            ui.add(
                                egui::TextEdit::singleline(
                                    parameter.debug_summary.get_or_insert_with(String::new),
                                )
                                .desired_width(f32::INFINITY),
                            );
                        }
                        "path" => {
                            ui.add(
                                egui::DragValue::new(&mut parameter.value)
                                    .range(0.0..=parameter.maximum)
                                    .clamp_existing_to_range(false),
                            );
                        }
                        "angle" | "point" | "point3d" => {
                            ui.horizontal(|ui| {
                                for (index, label) in ["X", "Y", "Z"]
                                    .iter()
                                    .enumerate()
                                    .take(parameter.component_count)
                                {
                                    ui.label(*label);
                                    ui.add(
                                        egui::DragValue::new(&mut parameter.components[index])
                                            .speed(0.1),
                                    );
                                }
                            });
                        }
                        "color" => {
                            let mut color = Color32::from_rgba_unmultiplied(
                                parameter.color[1],
                                parameter.color[2],
                                parameter.color[3],
                                parameter.color[0],
                            );
                            if ui.color_edit_button_srgba(&mut color).changed() {
                                parameter.color = [color.a(), color.r(), color.g(), color.b()];
                            }
                        }
                        _ if !parameter.choices.is_empty() => {
                            let mut selected = parameter.value as usize;
                            egui::ComboBox::from_id_salt(("effect-control", parameter.slot))
                                .selected_text(
                                    parameter
                                        .choices
                                        .get(selected.saturating_sub(1))
                                        .map(String::as_str)
                                        .unwrap_or("Unknown"),
                                )
                                .show_ui(ui, |ui| {
                                    for (index, choice) in parameter.choices.iter().enumerate() {
                                        ui.selectable_value(&mut selected, index + 1, choice);
                                    }
                                });
                            parameter.value = selected as f64;
                        }
                        "integer" if parameter.minimum == 0.0 && parameter.maximum == 1.0 => {
                            let mut checked = parameter.value != 0.0;
                            if ui.checkbox(&mut checked, text.enabled).changed() {
                                parameter.value = f64::from(checked);
                            }
                        }
                        _ => {
                            modern_slider(
                                ui,
                                &mut parameter.value,
                                parameter.minimum..=parameter.maximum,
                                &parameter.name,
                            );
                        }
                    }
                });
                #[cfg(test)]
                widgets.push((parameter.slot, EffectControlRole::Editor, _editor.response));
                if parameter.value != previous_value
                    || parameter.color != previous_color
                    || parameter.components != previous_components
                    || parameter.debug_summary != previous_summary
                {
                    intents.push(EffectControlIntent::Changed {
                        slot: parameter.slot,
                        supervised: parameter.supervised,
                    });
                }
                ui.add_space(4.0);
            }
        });
    EffectControlsOutput {
        intents,
        #[cfg(test)]
        widgets,
    }
}

pub(crate) fn modern_slider(
    ui: &mut egui::Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    accessible_label: &str,
) -> egui::Response {
    let minimum = *range.start();
    let maximum = *range.end();
    let previous = *value;
    let enabled = ui.is_enabled();
    let mut displayed_value = *value;
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
                    for request in input
                        .accesskit_action_requests(response.id, egui::accesskit::Action::SetValue)
                    {
                        if let Some(egui::accesskit::ActionData::NumericValue(new_value)) =
                            request.data
                        {
                            *value = new_value.clamp(minimum, maximum);
                        }
                    }
                });
            }
            if response.enabled() && *value != previous {
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
            let track_color = ui.visuals().widgets.inactive.bg_fill;
            let primary = if enabled {
                ui.visuals().selection.bg_fill
            } else {
                ui.visuals().weak_text_color().gamma_multiply(0.55)
            };
            let thumb_fill = if enabled {
                ui.visuals().panel_fill
            } else {
                ui.visuals().weak_text_color().gamma_multiply(0.55)
            };
            let track = egui::Rect::from_center_size(
                rect.center(),
                egui::vec2(rect.width(), if response.hovered() { 3.0 } else { 2.0 }),
            );
            ui.painter().rect_filled(track, 2.0, track_color);
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
                    .clamp_existing_to_range(false)
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AegpRoundtripAction {
    Keyframes,
    Seek,
    Trim,
    LayerSwitches,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Each platform constructs the opposite capability variant.
pub(crate) enum BackendCapability<'a> {
    Available,
    Unavailable { reason: &'a str },
}

impl AegpRoundtripAction {
    pub(crate) const ALL: [Self; 4] =
        [Self::Keyframes, Self::Seek, Self::Trim, Self::LayerSwitches];

    pub(crate) fn label(self, japanese: bool) -> &'static str {
        match (self, japanese) {
            (Self::Keyframes, false) => "Keyframes",
            (Self::Keyframes, true) => "キーフレーム",
            (Self::Seek, false) => "Seek",
            (Self::Seek, true) => "時間移動",
            (Self::Trim, false) => "Layer trim",
            (Self::Trim, true) => "レイヤートリム",
            (Self::LayerSwitches, false) => "Layer switches",
            (Self::LayerSwitches, true) => "レイヤースイッチ",
        }
    }

    pub(crate) fn help(self, japanese: bool) -> &'static str {
        match (self, japanese) {
            (Self::Keyframes, false) => {
                "Round-trip keyframe times, values, and interpolation through the AEGP stream suites."
            }
            (Self::Keyframes, true) => {
                "AEGPストリームSuite経由でキーフレームの時刻・値・補間を往復検査します。"
            }
            (Self::Seek, false) => {
                "Set and read back the current composition time through the AEGP item suite."
            }
            (Self::Seek, true) => {
                "AEGP Item Suite経由でコンポジションの現在時刻を設定し、読み戻します。"
            }
            (Self::Trim, false) => {
                "Set and read back a layer in-point and duration through the AEGP layer suite."
            }
            (Self::Trim, true) => {
                "AEGP Layer Suite経由でレイヤーのイン点とデュレーションを設定し、読み戻します。"
            }
            (Self::LayerSwitches, false) => {
                "Toggle and read back layer switches through the AEGP layer suite."
            }
            (Self::LayerSwitches, true) => {
                "AEGP Layer Suite経由でレイヤースイッチを切り替え、読み戻します。"
            }
        }
    }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn show_unavailable_aegp_actions(
    ui: &mut eframe::egui::Ui,
    explanation: &str,
) -> (
    Vec<(AegpRoundtripAction, eframe::egui::Response)>,
    Option<AegpRoundtripAction>,
) {
    show_aegp_actions(
        ui,
        false,
        false,
        BackendCapability::Unavailable {
            reason: explanation,
        },
    )
}

pub(crate) fn show_aegp_actions(
    ui: &mut eframe::egui::Ui,
    busy: bool,
    japanese: bool,
    capability: BackendCapability<'_>,
) -> (
    Vec<(AegpRoundtripAction, eframe::egui::Response)>,
    Option<AegpRoundtripAction>,
) {
    let enabled = !busy && matches!(capability, BackendCapability::Available);
    let reason = match capability {
        BackendCapability::Available => None,
        BackendCapability::Unavailable { reason } => Some(reason),
    };
    let mut intent = None;
    let responses = AegpRoundtripAction::ALL
        .into_iter()
        .map(|action| {
            let mut response = ui
                .add_enabled(enabled, eframe::egui::Button::new(action.label(japanese)))
                .on_hover_text(action.help(japanese));
            if let Some(reason) = reason {
                response = response.on_disabled_hover_text(reason);
            }
            if response.clicked() {
                intent = Some(action);
            }
            (action, response)
        })
        .collect();
    (responses, intent)
}

pub(crate) fn show_analysis_panel(
    ctx: &egui::Context,
    state: &mut crate::gui_state::AnalysisPaneState,
    title: &str,
    subtitle: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    state.set_width(state.width);
    let openness = ctx.animate_bool(egui::Id::new("analysis_and_logs_animation"), state.open);
    let width = egui::lerp(18.0..=state.width, openness.clamp(0.0, 1.0));
    let response = egui::SidePanel::left("analysis_and_logs")
        .exact_width(width)
        .resizable(false)
        .show_separator_line(false)
        .show(ctx, |ui| {
            if openness <= 0.12 {
                return;
            }
            ui.heading(title);
            ui.weak(subtitle);
            ui.separator();
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, add_contents);
        });
    let rect = response.response.rect;
    egui::Area::new(egui::Id::new("analysis_and_logs_rail"))
        .fixed_pos(egui::pos2(rect.right() - 8.0, rect.top()))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let (rail_rect, rail) = ui.allocate_exact_size(
                egui::vec2(16.0, rect.height()),
                egui::Sense::click_and_drag(),
            );
            let rail = rail.on_hover_text("Click to collapse or reopen. Drag to resize.");
            rail.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    true,
                    state.open,
                    "Show Analysis and Logs panel",
                )
            });
            ui.painter().vline(
                rail_rect.center().x,
                rail_rect.y_range(),
                egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
            );
            if state.open && rail.dragged() {
                state.set_width(state.width + ui.input(|input| input.pointer.delta().x));
                ctx.request_repaint();
            }
            if rail.clicked() {
                state.open = !state.open;
            }
            let center = egui::pos2(rail_rect.center().x, rail_rect.top() + 16.0);
            let points = if state.open {
                vec![
                    egui::pos2(center.x + 3.5, center.y - 5.5),
                    egui::pos2(center.x - 3.5, center.y),
                    egui::pos2(center.x + 3.5, center.y + 5.5),
                ]
            } else {
                vec![
                    egui::pos2(center.x - 3.5, center.y - 5.5),
                    egui::pos2(center.x + 3.5, center.y),
                    egui::pos2(center.x - 3.5, center.y + 5.5),
                ]
            };
            ui.painter().add(egui::Shape::convex_polygon(
                points,
                ui.visuals().text_color(),
                egui::Stroke::NONE,
            ));
        });
}

pub(crate) fn single_supported_dropped_path(
    dropped: &[eframe::egui::DroppedFile],
    is_supported: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if dropped.len() != 1 {
        return None;
    }
    dropped[0]
        .path
        .as_deref()
        .filter(|path| is_supported(path))
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameter(slot: u32, kind: &str) -> InteractiveParameter {
        InteractiveParameter {
            slot,
            name: format!("Control {slot}"),
            kind: kind.into(),
            minimum: 0.0,
            maximum: 100.0,
            value: 50.0,
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
    fn image_drop_requires_one_supported_filesystem_path() {
        let png = eframe::egui::DroppedFile {
            path: Some(PathBuf::from("input.png")),
            ..Default::default()
        };
        let memory = eframe::egui::DroppedFile {
            name: "memory.png".into(),
            bytes: Some(vec![1, 2, 3].into()),
            ..Default::default()
        };
        let supported = |path: &Path| path.extension().is_some_and(|ext| ext == "png");

        assert_eq!(
            single_supported_dropped_path(std::slice::from_ref(&png), supported),
            png.path
        );
        assert!(single_supported_dropped_path(std::slice::from_ref(&memory), supported).is_none());
        assert!(single_supported_dropped_path(&[png, memory], supported).is_none());
    }

    #[test]
    fn analysis_sections_have_one_shared_cross_platform_order() {
        assert_eq!(
            AnalysisSection::ALL.map(AnalysisSection::title),
            [
                "PROJECT / SESSION SETTINGS",
                "ADVANCED",
                "RENDER SETTINGS",
                "ANALYSIS / LOG OUTPUT",
            ]
        );
    }

    #[test]
    fn unavailable_aegp_actions_are_visible_but_cannot_dispatch() {
        eframe::egui::__run_test_ui(|ui| {
            let (buttons, intent) =
                show_unavailable_aegp_actions(ui, "guest transport unavailable");
            assert_eq!(buttons.len(), AegpRoundtripAction::ALL.len());
            assert_eq!(
                buttons
                    .iter()
                    .map(|(action, _)| *action)
                    .collect::<Vec<_>>(),
                AegpRoundtripAction::ALL
            );
            for (_, response) in buttons {
                assert!(!response.enabled());
                assert!(!response.clicked());
            }
            assert_eq!(
                intent, None,
                "an unavailable backend must not emit a command"
            );
        });
    }

    #[test]
    fn shared_effect_controls_keep_unavailable_backend_actions_inert() {
        let parameters = std::cell::RefCell::new(vec![
            parameter(1, "group_start"),
            parameter(2, "layer"),
            parameter(3, "button"),
            parameter(4, "group_end"),
            {
                let mut hidden = parameter(5, "float");
                hidden.visible = false;
                hidden
            },
        ]);
        let defaults = parameters.borrow().clone();
        eframe::egui::__run_test_ui(|ui| {
            let output = show_effect_controls(
                ui,
                &mut parameters.borrow_mut(),
                &defaults,
                false,
                EffectControlCapabilities {
                    choose_layer: false,
                    trigger_button: false,
                },
                EffectControlsText::default(),
            );
            assert!(output.intents.is_empty());
            assert_eq!(
                output
                    .widgets
                    .iter()
                    .map(|(slot, role, _)| (*slot, *role))
                    .collect::<Vec<_>>(),
                vec![
                    (1, EffectControlRole::GroupStart),
                    (2, EffectControlRole::Reset),
                    (2, EffectControlRole::ChooseLayer),
                    (2, EffectControlRole::Editor),
                    (3, EffectControlRole::Button),
                    (4, EffectControlRole::GroupEnd),
                ]
            );
            assert!(output.widgets.iter().all(|(slot, _, _)| *slot != 5));
            assert!(
                !output
                    .widgets
                    .iter()
                    .find(|(slot, role, _)| {
                        *slot == 2 && *role == EffectControlRole::ChooseLayer
                    })
                    .unwrap()
                    .2
                    .enabled()
            );
        });
        assert!(
            parameters
                .borrow()
                .iter()
                .zip(defaults)
                .all(|(actual, expected)| {
                    crate::gui_state::parameter_is_default(actual, &expected)
                })
        );
    }

    fn click_input(rect: egui::Rect) -> egui::RawInput {
        egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(rect.center()),
                egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn available_backend_maps_layer_and_button_clicks_to_exact_slots() {
        for (kind, role, expected) in [
            (
                "layer",
                EffectControlRole::ChooseLayer,
                EffectControlIntent::ChooseLayer { slot: 7 },
            ),
            (
                "button",
                EffectControlRole::Button,
                EffectControlIntent::TriggerButton { slot: 7 },
            ),
            (
                "compatibility_action",
                EffectControlRole::Button,
                EffectControlIntent::TriggerButton { slot: 7 },
            ),
        ] {
            let ctx = egui::Context::default();
            let parameters = std::cell::RefCell::new(vec![parameter(7, kind)]);
            let defaults = parameters.borrow().clone();
            let rect = std::cell::Cell::new(egui::Rect::NOTHING);
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let output = show_effect_controls(
                        ui,
                        &mut parameters.borrow_mut(),
                        &defaults,
                        false,
                        EffectControlCapabilities {
                            choose_layer: true,
                            trigger_button: true,
                        },
                        EffectControlsText::default(),
                    );
                    rect.set(
                        output
                            .widgets
                            .iter()
                            .find(|(_, observed, _)| *observed == role)
                            .unwrap()
                            .2
                            .rect,
                    );
                });
            });
            let intents = std::cell::RefCell::new(Vec::new());
            let _ = ctx.run(click_input(rect.get()), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    intents.borrow_mut().extend(
                        show_effect_controls(
                            ui,
                            &mut parameters.borrow_mut(),
                            &defaults,
                            false,
                            EffectControlCapabilities {
                                choose_layer: true,
                                trigger_button: true,
                            },
                            EffectControlsText::default(),
                        )
                        .intents,
                    );
                });
            });
            assert_eq!(intents.into_inner(), vec![expected]);
        }
    }

    #[test]
    fn reset_and_supervised_edits_emit_exact_backend_intents() {
        let ctx = egui::Context::default();
        let mut current = parameter(9, "float");
        current.value = 75.0;
        current.supervised = true;
        let parameters = std::cell::RefCell::new(vec![current]);
        let mut default = parameter(9, "float");
        default.value = 25.0;
        let defaults = vec![default];
        let reset_rect = std::cell::Cell::new(egui::Rect::NOTHING);
        let editor_rect = std::cell::Cell::new(egui::Rect::NOTHING);
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let output = show_effect_controls(
                    ui,
                    &mut parameters.borrow_mut(),
                    &defaults,
                    false,
                    EffectControlCapabilities {
                        choose_layer: true,
                        trigger_button: true,
                    },
                    EffectControlsText::default(),
                );
                reset_rect.set(
                    output
                        .widgets
                        .iter()
                        .find(|(_, role, _)| *role == EffectControlRole::Reset)
                        .unwrap()
                        .2
                        .rect,
                );
                editor_rect.set(
                    output
                        .widgets
                        .iter()
                        .find(|(_, role, _)| *role == EffectControlRole::Editor)
                        .unwrap()
                        .2
                        .rect,
                );
            });
        });

        let intents = std::cell::RefCell::new(Vec::new());
        let _ = ctx.run(click_input(reset_rect.get()), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                intents.borrow_mut().extend(
                    show_effect_controls(
                        ui,
                        &mut parameters.borrow_mut(),
                        &defaults,
                        false,
                        EffectControlCapabilities {
                            choose_layer: true,
                            trigger_button: true,
                        },
                        EffectControlsText::default(),
                    )
                    .intents,
                );
            });
        });
        assert_eq!(
            intents.borrow().as_slice(),
            [EffectControlIntent::Reset {
                slot: 9,
                supervised: true,
            }]
        );

        intents.borrow_mut().clear();
        let rect = editor_rect.get();
        let slider_position = egui::pos2(rect.left() + 20.0, rect.bottom() - 14.0);
        let input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(slider_position),
                egui::Event::PointerButton {
                    pos: slider_position,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: slider_position,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                intents.borrow_mut().extend(
                    show_effect_controls(
                        ui,
                        &mut parameters.borrow_mut(),
                        &defaults,
                        false,
                        EffectControlCapabilities {
                            choose_layer: true,
                            trigger_button: true,
                        },
                        EffectControlsText::default(),
                    )
                    .intents,
                );
            });
        });
        assert_eq!(
            intents.into_inner(),
            vec![EffectControlIntent::Changed {
                slot: 9,
                supervised: true,
            }]
        );
    }

    #[test]
    fn complete_control_kinds_expose_distinct_accessible_widgets() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut scalar = parameter(1, "float");
        scalar.name = "Scalar".into();
        let mut choice = parameter(2, "integer");
        choice.name = "Choice".into();
        choice.value = 1.0;
        choice.choices = vec!["One".into(), "Two".into()];
        let mut color = parameter(3, "color");
        color.name = "Color".into();
        let mut point = parameter(4, "point");
        point.name = "Point".into();
        point.component_count = 2;
        let mut arbitrary = parameter(5, "arbitrary_data");
        arbitrary.name = "Arbitrary".into();
        arbitrary.debug_summary = Some("value".into());
        let parameters = std::cell::RefCell::new(vec![scalar, choice, color, point, arbitrary]);
        let defaults = parameters.borrow().clone();
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                show_effect_controls(
                    ui,
                    &mut parameters.borrow_mut(),
                    &defaults,
                    false,
                    EffectControlCapabilities {
                        choose_layer: true,
                        trigger_button: true,
                    },
                    EffectControlsText::default(),
                );
            });
        });
        let roles = output
            .platform_output
            .accesskit_update
            .expect("shared controls must expose an accessibility tree")
            .nodes
            .into_iter()
            .map(|(_, node)| node.role())
            .collect::<Vec<_>>();
        use egui::accesskit::Role;
        assert!(roles.contains(&Role::Slider), "scalar must expose a slider");
        assert!(
            roles.contains(&Role::ComboBox),
            "choices must expose a combo box"
        );
        assert!(
            roles.contains(&Role::ColorWell),
            "color must expose a color well"
        );
        assert!(
            roles.contains(&Role::TextInput),
            "arbitrary data must expose text input"
        );
        assert!(
            roles
                .iter()
                .filter(|role| **role == Role::SpinButton)
                .count()
                >= 2,
            "point controls must expose each numeric component"
        );
    }

    #[test]
    fn path_control_preserves_native_values_without_user_input() {
        for value in [-1.0, 150.0] {
            let mut path = parameter(1, "path");
            path.value = value;
            path.maximum = 100.0;
            let parameters = std::cell::RefCell::new(vec![path]);
            let defaults = parameters.borrow().clone();
            eframe::egui::__run_test_ui(|ui| {
                let output = show_effect_controls(
                    ui,
                    &mut parameters.borrow_mut(),
                    &defaults,
                    false,
                    EffectControlCapabilities {
                        choose_layer: false,
                        trigger_button: false,
                    },
                    EffectControlsText::default(),
                );
                assert_eq!(parameters.borrow()[0].value, value);
                assert!(output.intents.is_empty());
            });
        }
    }

    #[test]
    fn modern_slider_preserves_native_values_without_user_input() {
        eframe::egui::__run_test_ui(|ui| {
            let mut value = 150.0;
            let response = modern_slider(ui, &mut value, 0.0..=100.0, "Amount");
            assert!(response.enabled());
            assert_eq!(value, 150.0);
            assert!(!response.changed());

            let mut disabled = 150.0;
            ui.add_enabled_ui(false, |ui| {
                let response = modern_slider(ui, &mut disabled, 0.0..=100.0, "Disabled");
                assert!(!response.enabled());
            });
            assert_eq!(disabled, 150.0);
        });
    }
}
