use super::app::HarnessApp;
use super::render_contract::{ParameterInspectionState, apply_dynamic_ui_report};
use eframe::egui;
use std::path::PathBuf;

use std::time::{Duration, Instant};

fn pump(app: &mut HarnessApp, context: &egui::Context, seconds: u64) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while (app.busy || app.receiver.is_some()) && Instant::now() < deadline {
        app.poll(context);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!app.busy, "task timeout: {}", app.report);
}

#[test]
#[ignore = "requires a built x64 guest worker and AEXCOMPAT_SMOKE_AEX"]
fn unified_gui_inspects_and_renders_through_the_x64_guest() {
    let plugin =
        PathBuf::from(std::env::var_os("AEXCOMPAT_SMOKE_AEX").expect("set AEXCOMPAT_SMOKE_AEX"));
    let repository = crate::macos::gui_repository_root();
    let scratch = std::env::temp_dir().join(format!("aexcompat-unified-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let input = scratch.join("input.png");
    image::RgbaImage::from_fn(32, 24, |x, y| {
        image::Rgba([(x * 8) as u8, (y * 10) as u8, 90, 255])
    })
    .save(&input)
    .unwrap();
    let mut app = HarnessApp::new(repository);
    app.live_render = false;
    let context = egui::Context::default();
    app.choose_aex_path(plugin);
    pump(&mut app, &context, 120);
    assert!(app.selection.is_some(), "{}", app.report);
    assert!(
        matches!(
            app.parameter_inspection_state,
            ParameterInspectionState::Ready | ParameterInspectionState::ZeroParameters
        ),
        "{:?} / {}: {}",
        app.parameter_inspection_state,
        app.status,
        app.report
    );
    app.input_image = Some(input);
    for format in [
        aexcompat_broker::image_render::RenderPixelFormat::Argb8,
        aexcompat_broker::image_render::RenderPixelFormat::Argb16,
        aexcompat_broker::image_render::RenderPixelFormat::Argb32f,
    ] {
        let output = scratch.join(format!("output-{}.png", format.report_name()));
        app.pixel_format = format;
        app.preview = None;
        app.render_to(output.clone());
        pump(&mut app, &context, 120);
        assert!(output.is_file(), "{}: {}", format.report_name(), app.report);
        assert!(
            app.preview.is_some(),
            "{}: {}",
            format.report_name(),
            app.report
        );
        println!("{}", app.report);
    }
    crate::macos::backend::close_resident_session();
    std::fs::remove_dir_all(scratch).unwrap();
}

fn parameter(slot: u32) -> aexcompat_broker::image_render::InteractiveParameter {
    aexcompat_broker::image_render::InteractiveParameter {
        slot,
        name: format!("parameter-{slot}"),
        kind: "float".into(),
        minimum: 0.0,
        maximum: 1.0,
        value: 0.0,
        choices: Vec::new(),
        use_declared_default: false,
        color_float: None,
        color: [255, 0, 0, 0],
        components: [0.0; 3],
        component_count: 0,
        layer_path: None,
        enabled: true,
        visible: true,
        supervised: slot == 1,
        debug_summary: None,
        custom_ui_events: 0,
        control_size: [0; 2],
    }
}

#[test]
fn guest_user_changed_report_updates_effect_controls() {
    let mut parameters = vec![parameter(1), parameter(2), parameter(3)];
    // Slot 2 becomes disabled (bit 5), slot 3 hidden (bit 9).
    let report = crate::macos::backend::user_changed_report_from_rows(
        &parameters,
        1,
        &[(1, 0, 1 << 6), (2, 1 << 5, 0), (3, 1 << 9, 0)],
    )
    .unwrap();
    assert!(apply_dynamic_ui_report(&mut parameters, &report));
    assert!(parameters[0].enabled && parameters[0].visible);
    assert!(!parameters[1].enabled && parameters[1].visible);
    assert!(parameters[2].enabled && !parameters[2].visible);
}

#[test]
fn guest_user_changed_report_rejects_partial_or_reordered_tables() {
    let parameters = vec![parameter(1), parameter(2)];
    assert!(
        crate::macos::backend::user_changed_report_from_rows(&parameters, 1, &[(1, 0, 0)]).is_err()
    );
    assert!(
        crate::macos::backend::user_changed_report_from_rows(
            &parameters,
            1,
            &[(2, 0, 0), (1, 0, 0)]
        )
        .is_err()
    );
}
