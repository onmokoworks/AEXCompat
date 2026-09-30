//! Compare two real effects in separate Classic sessions and one in-place
//! cluster session. This is a focused diagnostic, not a corpus sweep.
//!
//! Usage: cluster_pair_diag <repository> <first.aex> <second.aex> <runtime-root>
//! The runtime root is an explicit diagnostic input. Its use here does not
//! change shipping dependency discovery or imply that users should configure it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use aexcompat_broker::image_render::{RenderGpuBackend, RenderPixelFormat};
use aexcompat_broker::render_session::{
    ClusterRenderPlugins, FrameStatus, RenderSession, SessionOpenRequest, SwapOutcome,
};
use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const WIDTH: u32 = 256;
const HEIGHT: u32 = 144;

fn artifact(path: &Path) -> std::io::Result<(ApprovedImageArtifact, String)> {
    let bytes = std::fs::read(path)?;
    let digest = Sha256::digest(&bytes);
    Ok((
        ApprovedImageArtifact {
            path: path.to_path_buf(),
            expected_sha256: digest.into(),
            expected_size: bytes.len() as u64,
        },
        format!("{digest:x}"),
    ))
}

fn request<'a>(
    repository: &'a Path,
    path: &'a Path,
    sha: &'a str,
    runtime_root: &Path,
) -> SessionOpenRequest<'a> {
    SessionOpenRequest {
        repository,
        plugin_path: path,
        plugin_sha256: sha,
        parameters: None,
        payload_override: None,
        parameter_animation: None,
        aux_manifest: None,
        world_dump_dir: None,
        output_checksum_detail: false,
        mask_trailer: None,
        spatial_trailer: None,
        camera_trailer: None,
        render_environment_trailer: None,
        audio_trailer: None,
        alpha_as_coverage_params: &[],
        conformance_render_settings: None,
        layers: &[],
        dependencies: Vec::new(),
        companions: Vec::new(),
        dependency_search_dirs: vec![
            path.parent().expect("plugin has parent").to_path_buf(),
            runtime_root.to_path_buf(),
        ],
        width: WIDTH,
        height: HEIGHT,
        pixel_format: RenderPixelFormat::Argb8,
        time_step: 1,
        total_time: 300,
        time_scale: 30,
        frame_deadline: Duration::from_secs(60),
        smart: false,
        gpu_backend: RenderGpuBackend::Cpu,
        gpu_runtime_policy: None,
        launch_environment: Default::default(),
    }
}

fn render(session: &mut RenderSession, frame_index: u32, input: &[u8]) -> Result<Vec<u8>, String> {
    let frame = session
        .render_frame_with_parameters(frame_index, 0, input, None)
        .map_err(|error| error.to_string())?;
    match frame.status {
        FrameStatus::Rendered {
            pixels,
            width,
            height,
            origin_x,
            origin_y,
        } if width == WIDTH
            && height == HEIGHT
            && origin_x == 0
            && origin_y == 0
            && pixels.len() == (WIDTH * HEIGHT * 4) as usize =>
        {
            Ok(pixels)
        }
        other => Err(format!("invalid or failed frame: {other:?}")),
    }
}

fn clean_close(close: &Value) -> bool {
    close["session_clean"] == true && close["invalidated"] == false
}

fn close_summary(close: &Value) -> Value {
    json!({
        "session_clean": close["session_clean"],
        "invalidated": close["invalidated"],
        "invalidated_reason": close.pointer("/invalidated_reason/reason"),
        "frames_ok": close["frames_ok"],
        "frames_errored": close["frames_errored"],
        "worker_classification": close.pointer("/worker/classification"),
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let repository = args.next().expect("repository argument");
    let first = args.next().expect("first AEX argument");
    let second = args.next().expect("second AEX argument");
    let runtime_root = args.next().expect("runtime root argument");
    assert!(args.next().is_none(), "unexpected extra argument");
    assert!(repository.is_dir() && runtime_root.is_dir());
    let (first_artifact, first_sha) = artifact(&first)?;
    let (second_artifact, second_sha) = artifact(&second)?;
    let input: Vec<u8> = (0..WIDTH * HEIGHT)
        .flat_map(|_| [32u8, 64, 128, 255])
        .collect();

    let mut first_alone =
        RenderSession::open(request(&repository, &first, &first_sha, &runtime_root))?;
    let first_control = render(&mut first_alone, 0, &input)?;
    let first_close = first_alone.close();
    let mut second_alone =
        RenderSession::open(request(&repository, &second, &second_sha, &runtime_root))?;
    let second_control = render(&mut second_alone, 0, &input)?;
    let second_close = second_alone.close();

    let mut pooled = RenderSession::open_cluster(
        request(&repository, &first, &first_sha, &runtime_root),
        ClusterRenderPlugins {
            plugins: vec![first_artifact, second_artifact],
            swap_payloads: vec![None, None],
            module_bound: 4096,
        },
    )?;
    let first_pooled = render(&mut pooled, 0, &input)?;
    assert!(matches!(pooled.swap_plugin(1)?, SwapOutcome::Swapped));
    let second_pooled = render(&mut pooled, 1, &input)?;
    assert!(matches!(pooled.swap_plugin(0)?, SwapOutcome::Swapped));
    let first_returned = render(&mut pooled, 2, &input)?;
    let pooled_close = pooled.close();

    let matched = first_control == first_pooled
        && second_control == second_pooled
        && first_control == first_returned;
    let cleanup =
        clean_close(&first_close) && clean_close(&second_close) && clean_close(&pooled_close);
    let sha = |pixels: &[u8]| format!("{:x}", Sha256::digest(pixels));
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "first_aex_sha256": first_sha,
            "second_aex_sha256": second_sha,
            "render": {"width": WIDTH, "height": HEIGHT, "depth": "argb8", "time": 0},
            "first_control": sha(&first_control),
            "first_pooled": sha(&first_pooled),
            "second_control": sha(&second_control),
            "second_pooled": sha(&second_pooled),
            "first_returned": sha(&first_returned),
            "pixels_match": matched,
            "clean_close": cleanup,
            "close": {
                "first": close_summary(&first_close),
                "second": close_summary(&second_close),
                "pooled": close_summary(&pooled_close),
            }
        }))?
    );
    if !matched || !cleanup {
        return Err("cluster pixels or cleanup differ from separate sessions".into());
    }
    Ok(())
}
