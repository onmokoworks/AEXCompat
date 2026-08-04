//! Diagnostic: render one frame of an AEX that takes a secondary layer, with a
//! synthetic layer supplied, and print everything the worker reports about it
//! (issue #695: `Displacement` answers every frame with
//! `PF_Err_OUT_OF_MEMORY`, while `Blend` renders on the same route).
//!
//! Reproduces without AviUtl2, so the failure can be attributed to the host's
//! own checkout bookkeeping rather than guessed at from a preview.
//!
//! Run:
//!   set AEXCOMPAT_MULTIFILTER_REPOSITORY=C:\path\to\AEXCompat
//!   cargo run --release --example layer_render_diag -- "<plugin.aex>" [deps-dir]
//!
//! Switches, all optional, all read from the environment so one build can
//! bisect a failure:
//!
//! * `AEXCOMPAT_DIAG_FORCE_CLASSIC=1` - dispatch `PF_Cmd_RENDER` even when the
//!   plug-in advertises SmartFX, which is how "Displacement renders fine on the
//!   classic route" was established.
//! * `AEXCOMPAT_DIAG_NO_LAYER=1` - open the session with no secondary layer.
//! * `AEXCOMPAT_DIAG_SET=slot=value,slot=value` - override discovered parameter
//!   defaults, so a value-dependent refusal can be ruled in or out.
//!
//! `AEXCOMPAT_EXTENDED_DIAG=1` additionally turns on the worker's host-callback
//! trace; it reaches stderr, which the session collects but does not report.

use std::path::PathBuf;
use std::time::Duration;

use aexcompat_broker::image_render::inspect_experimental_with_approved_dependencies_and_resources;
use aexcompat_broker::image_render::{RenderGpuBackend, RenderPixelFormat};
use aexcompat_broker::plugin_dependency_closure::{
    DependencyClosureRequest, resolve_dependency_closure,
};
use aexcompat_broker::render_session::{
    FrameStatus, RenderSession, SessionLayer, SessionOpenRequest,
};
use sha2::{Digest, Sha256};

const WIDTH: u32 = 256;
const HEIGHT: u32 = 144;

fn main() {
    let mut args = std::env::args().skip(1);
    let plugin = PathBuf::from(
        args.next()
            .expect("usage: layer_render_diag <plugin.aex> [deps-dir]"),
    );
    let repository = PathBuf::from(
        std::env::var_os("AEXCOMPAT_MULTIFILTER_REPOSITORY")
            .expect("set AEXCOMPAT_MULTIFILTER_REPOSITORY"),
    );
    let mut roots: Vec<PathBuf> = plugin
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
        .into_iter()
        .collect();
    roots
        .extend(args.map(|dir| std::fs::canonicalize(&dir).unwrap_or_else(|_| PathBuf::from(dir))));

    let bytes = std::fs::read(&plugin).expect("read plugin");
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    let closure = resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots))
        .expect("resolve the plug-in's dependency closure");
    let dependencies = closure.into_dependencies();
    eprintln!("closure: {} dependencies", dependencies.len());

    // Discovery's own inspect, so the session opens on the parameters the
    // plug-in actually declares - the layer slot among them.
    let (parameters, diagnostics) = inspect_experimental_with_approved_dependencies_and_resources(
        &repository,
        &plugin,
        &sha,
        dependencies.clone(),
        Vec::new(),
    )
    .expect("inspect the plug-in");
    // `AEXCOMPAT_DIAG_SET=slot=value,slot=value` overrides discovered defaults,
    // so a value-dependent refusal can be bisected without AviUtl2.
    let mut parameters = parameters;
    if let Ok(overrides) = std::env::var("AEXCOMPAT_DIAG_SET") {
        for entry in overrides.split(',').filter(|entry| !entry.is_empty()) {
            let (slot, value) = entry.split_once('=').expect("slot=value");
            let slot: u32 = slot.parse().expect("slot");
            let value: f64 = value.parse().expect("value");
            let parameter = parameters
                .iter_mut()
                .find(|parameter| parameter.slot == slot)
                .expect("a parameter in that slot");
            eprintln!(
                "override slot {slot} ({}) {} -> {value}",
                parameter.name, parameter.value
            );
            parameter.value = value;
        }
    }
    for parameter in &parameters {
        eprintln!(
            "param slot={} kind={} value={} name={:?}",
            parameter.slot, parameter.kind, parameter.value, parameter.name
        );
    }

    let smart = diagnostics
        .get("advertised_out_flags2")
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
        & (1 << 10)
        != 0
        && std::env::var("AEXCOMPAT_DIAG_FORCE_CLASSIC").is_err();
    let layer_slots: Vec<u32> = parameters
        .iter()
        .filter(|parameter| parameter.kind == "layer")
        .map(|parameter| parameter.slot)
        .collect();
    eprintln!(
        "{} parameter(s), smart={smart}, layer slot(s)={layer_slots:?}",
        parameters.len()
    );
    // A plug-in without a layer parameter still renders here, with no layer
    // supplied: that is the control for "is this about the secondary layer, or
    // about this render path at all".

    // A map with structure, so an effect that samples it cannot answer
    // identically for every pixel by accident.
    let layer_pixels: Vec<u8> = (0..WIDTH * HEIGHT)
        .flat_map(|index| {
            let x = (index % WIDTH) as u8;
            let y = (index / WIDTH) as u8;
            [x, y, x ^ y, 255]
        })
        .collect();
    let layers: Vec<SessionLayer> = layer_slots
        .first()
        .filter(|_| std::env::var("AEXCOMPAT_DIAG_NO_LAYER").is_err())
        .map(|&slot| SessionLayer {
            slot,
            width: WIDTH,
            height: HEIGHT,
            rgba: layer_pixels,
            timed: None,
            dynamic: false,
        })
        .into_iter()
        .collect();

    let mut session = RenderSession::open(SessionOpenRequest {
        repository: &repository,
        plugin_path: &plugin,
        plugin_sha256: &sha,
        parameters: Some(&parameters),
        parameter_animation: None,
        aux_manifest: None,
        world_dump_dir: None,
        output_checksum_detail: false,
        mask_trailer: None,
        spatial_trailer: None,
        render_environment_trailer: None,
        audio_trailer: None,
        alpha_as_coverage_params: &[],
        conformance_render_settings: None,
        layers: &layers,
        dependencies,
        width: WIDTH,
        height: HEIGHT,
        pixel_format: RenderPixelFormat::Argb8,
        time_step: 1,
        total_time: 300,
        time_scale: 30,
        frame_deadline: Duration::from_secs(60),
        smart,
        gpu_backend: RenderGpuBackend::Auto,
        gpu_runtime_policy: None,
        payload_override: None,
    })
    .expect("open a render session with the secondary layer");

    let input: Vec<u8> = (0..WIDTH * HEIGHT)
        .flat_map(|_| [32u8, 64, 128, 255])
        .collect();
    match session.render_frame(0, 0, &input) {
        Ok(outcome) => match outcome.status {
            FrameStatus::Rendered { width, height, .. } => {
                println!("frame rendered {width}x{height}")
            }
            FrameStatus::FrameError {
                render_error,
                missing_dependency,
            } => {
                println!("frame error {render_error} (missing dependency: {missing_dependency:?})")
            }
        },
        Err(error) => println!("render_frame failed: {error}"),
    }

    // The worker's own account of the session: the stage errors, the smart
    // telemetry (rejected temporal checkouts, malformed requests, checkout
    // balance) and the final report.
    let close = session.close();
    println!(
        "{}",
        serde_json::to_string_pretty(&close).unwrap_or_else(|_| close.to_string())
    );
}
