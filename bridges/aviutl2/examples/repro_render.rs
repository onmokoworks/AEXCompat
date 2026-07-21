//! Standalone reproduction of the AviUtl2 bridge's discovery + render path,
//! outside AviUtl2, to isolate where a failure occurs. Reads the same env as
//! the bridge (AEXCOMPAT_AVIUTL2_PLUGIN / AEXCOMPAT_AVIUTL2_REPOSITORY).
//!
//! Run: cargo run --example repro_render

use std::path::PathBuf;
use std::time::Duration;

use aexcompat_broker::image_render::{
    InteractiveParameter, RenderGpuBackend, RenderPixelFormat,
    inspect_experimental_with_diagnostics,
};
use aexcompat_broker::render_session::{FrameStatus, RenderSession, SessionOpenRequest};
use sha2::{Digest, Sha256};

fn main() {
    let plugin =
        PathBuf::from(std::env::var_os("AEXCOMPAT_AVIUTL2_PLUGIN").expect("AEXCOMPAT_AVIUTL2_PLUGIN"));
    let repository = PathBuf::from(
        std::env::var_os("AEXCOMPAT_AVIUTL2_REPOSITORY").expect("AEXCOMPAT_AVIUTL2_REPOSITORY"),
    );
    let bytes = std::fs::read(&plugin).expect("read AEX");
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();

    eprintln!("plugin = {plugin:?}");
    eprintln!("repository = {repository:?}");

    // 1. Discovery.
    let (params, _diag) = match inspect_experimental_with_diagnostics(&repository, &plugin, &sha) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("DISCOVERY FAILED: {error}");
            return;
        }
    };
    eprintln!("DISCOVERY OK: {} parameters", params.len());
    for p in &params {
        eprintln!(
            "  slot={} kind={:?} name={:?} min={} max={} value={} choices={:?}",
            p.slot, p.kind, p.name, p.minimum, p.maximum, p.value, p.choices
        );
    }

    // 2. Build a scalar subset (float/checkbox/color/valid-integer) to open and
    //    render with. This is a diagnostic stand-in, not the bridge's exact
    //    `config_item_for` predicate (which also surfaces popups as Select).
    let exposed: Vec<InteractiveParameter> = params
        .iter()
        .filter(|p| {
            matches!(p.kind.as_str(), "float" | "slider" | "angle" | "integer")
                && p.minimum.is_finite()
                && p.maximum.is_finite()
                && p.minimum < p.maximum
                || p.kind == "checkbox"
                || p.kind == "color"
        })
        .cloned()
        .collect();
    eprintln!("exposed subset = {} params", exposed.len());

    let width = 16u32;
    let height = 16u32;
    let rgba = vec![128u8; (width * height * 4) as usize];

    // Prove per-frame parameter application end to end: open once with the
    // exposed baseline, then render two frames overriding the first exposed
    // param's value. For the echo probe (writes its float value into the red
    // channel) the two frames must differ in red.
    let mut session = match RenderSession::open(SessionOpenRequest {
        repository: &repository,
        plugin_path: &plugin,
        plugin_sha256: &sha,
        parameters: (!exposed.is_empty()).then_some(&exposed[..]),
        parameter_animation: None,
        aux_manifest: None,
        world_dump_dir: None,
        output_checksum_detail: false,
        mask_trailer: None,
        spatial_trailer: None,
        render_environment_trailer: None,
        alpha_as_coverage_params: &[],
        layers: &[],
        dependencies: Vec::new(),
        width,
        height,
        pixel_format: RenderPixelFormat::Argb8,
        time_step: 1,
        total_time: 100,
        time_scale: 30,
        frame_deadline: Duration::from_millis(30_000),
        smart: false,
        gpu_backend: RenderGpuBackend::Auto,
        gpu_runtime_policy: None,
    }) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("OPEN FAILED: {error}");
            return;
        }
    };
    eprintln!("open OK");

    let render_red = |session: &mut RenderSession, frame: u32, value: Option<f64>| {
        let params = value.map(|v| {
            let mut p = exposed.clone();
            if let Some(first) = p.first_mut() {
                first.value = v;
            }
            p
        });
        match session.render_frame_with_parameters(frame, frame as i32, &rgba, params.as_deref()) {
            Ok(outcome) => match outcome.status {
                FrameStatus::Rendered { pixels, .. } => {
                    eprintln!(
                        "  frame {frame} value={value:?} -> RENDERED, first pixel RGBA={:?}",
                        pixels.get(0..4).unwrap_or(&pixels)
                    );
                }
                FrameStatus::FrameError { render_error } => {
                    eprintln!("  frame {frame} FRAME ERROR render_error={render_error}")
                }
            },
            Err(error) => eprintln!("  frame {frame} RENDER FAILED: {error}"),
        }
    };

    render_red(&mut session, 0, Some(0.0));
    render_red(&mut session, 1, Some(200.0));
    let _ = session.close();
}
