//! Native-depth comparison of one full-frame render with transformed worlds.
//!
//! This is deliberately independent of PNG/EXR encoding and of the AEX name.
//! The caller must report a transform it could not request as unsupported;
//! it must never replace an absent tile with the full-frame result.

#[cfg(windows)]
use crate::image_render::{
    InteractiveParameter, RenderGpuBackend, RenderTiming, decode_bounded_image,
};
use crate::render_pixel_format::RenderPixelFormat;
#[cfg(windows)]
use crate::render_session::{
    DiagnosticWorldLayout, FrameStatus, RenderSession, SessionOpenRequest,
    validate_completed_session_close,
};
#[cfg(windows)]
use crate::secure_launch::LaunchEnvironment;
use serde::Serialize;
#[cfg(windows)]
use serde_json::{Value, json};
#[cfg(windows)]
use sha2::{Digest, Sha256};
#[cfg(windows)]
use std::fs::File;
#[cfg(windows)]
use std::io::{self, Read};
#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use std::time::Duration;

#[derive(Clone, Copy)]
pub struct NativeWorld<'a> {
    pub pixels: &'a [u8],
    pub format: RenderPixelFormat,
    pub width: u32,
    pub height: u32,
    pub rowbytes: usize,
    pub origin_x: i32,
    pub origin_y: i32,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct DifferenceReport {
    pub comparison_rect: [i32; 4],
    pub compared_pixels: u64,
    pub differing_pixels: u64,
    pub nonfinite_pixels: u64,
    pub max_abs_difference: f64,
    pub difference_bbox: Option<[i32; 4]>,
}

#[derive(Debug, PartialEq)]
pub enum ComparisonError {
    InvalidWorld,
    IncompatibleFormat,
    IncompleteCoverage,
    OverlappingCoverage,
}

pub fn compare_native_worlds(
    full: NativeWorld<'_>,
    parts: &[NativeWorld<'_>],
) -> Result<DifferenceReport, ComparisonError> {
    const MAX_DIMENSION: u32 = 4096;
    const MAX_PIXELS: u64 = 16_777_216;
    fn validated_rect(world: NativeWorld<'_>) -> Result<[i32; 4], ComparisonError> {
        let pixel_bytes = usize::try_from(world.format.bytes_per_pixel()).unwrap();
        let tight = usize::try_from(world.width)
            .ok()
            .and_then(|width| width.checked_mul(pixel_bytes))
            .ok_or(ComparisonError::InvalidWorld)?;
        let extent = world
            .rowbytes
            .checked_mul(usize::try_from(world.height).unwrap_or(usize::MAX))
            .ok_or(ComparisonError::InvalidWorld)?;
        let area = u64::from(world.width) * u64::from(world.height);
        if world.width == 0
            || world.height == 0
            || world.width > MAX_DIMENSION
            || world.height > MAX_DIMENSION
            || area > MAX_PIXELS
            || world.rowbytes < tight
            || world.pixels.len() < extent
        {
            return Err(ComparisonError::InvalidWorld);
        }
        let right = world
            .origin_x
            .checked_add(i32::try_from(world.width).unwrap())
            .ok_or(ComparisonError::InvalidWorld)?;
        let bottom = world
            .origin_y
            .checked_add(i32::try_from(world.height).unwrap())
            .ok_or(ComparisonError::InvalidWorld)?;
        Ok([world.origin_x, world.origin_y, right, bottom])
    }

    let rect = validated_rect(full)?;
    if parts.is_empty() || parts.len() > 16 {
        return Err(ComparisonError::IncompleteCoverage);
    }
    let width = usize::try_from(full.width).unwrap();
    let height = usize::try_from(full.height).unwrap();
    let pixel_bytes = usize::try_from(full.format.bytes_per_pixel()).unwrap();
    let mut covered = vec![false; width * height];
    let mut report = DifferenceReport {
        comparison_rect: rect,
        compared_pixels: 0,
        differing_pixels: 0,
        nonfinite_pixels: 0,
        max_abs_difference: 0.0,
        difference_bbox: None,
    };
    for part in parts {
        if part.format != full.format {
            return Err(ComparisonError::IncompatibleFormat);
        }
        let part_rect = validated_rect(*part)?;
        if part_rect[0] < rect[0]
            || part_rect[1] < rect[1]
            || part_rect[2] > rect[2]
            || part_rect[3] > rect[3]
        {
            return Err(ComparisonError::InvalidWorld);
        }
        for y in 0..usize::try_from(part.height).unwrap() {
            let layer_y = part.origin_y + i32::try_from(y).unwrap();
            let full_y = usize::try_from(layer_y - rect[1]).unwrap();
            for x in 0..usize::try_from(part.width).unwrap() {
                let layer_x = part.origin_x + i32::try_from(x).unwrap();
                let full_x = usize::try_from(layer_x - rect[0]).unwrap();
                let index = full_y * width + full_x;
                if covered[index] {
                    return Err(ComparisonError::OverlappingCoverage);
                }
                covered[index] = true;
                report.compared_pixels += 1;
                let reference_offset = full_y * full.rowbytes + full_x * pixel_bytes;
                let actual_offset = y * part.rowbytes + x * pixel_bytes;
                let reference = &full.pixels[reference_offset..reference_offset + pixel_bytes];
                let actual = &part.pixels[actual_offset..actual_offset + pixel_bytes];
                let mut different = false;
                let mut nonfinite = false;
                for channel in 0..4 {
                    let (expected, observed) = match full.format {
                        RenderPixelFormat::Argb8 => {
                            (f64::from(reference[channel]), f64::from(actual[channel]))
                        }
                        RenderPixelFormat::Argb16 => {
                            let offset = channel * 2;
                            (
                                f64::from(u16::from_le_bytes(
                                    reference[offset..offset + 2].try_into().unwrap(),
                                )),
                                f64::from(u16::from_le_bytes(
                                    actual[offset..offset + 2].try_into().unwrap(),
                                )),
                            )
                        }
                        RenderPixelFormat::Argb32f => {
                            let offset = channel * 4;
                            (
                                f64::from(f32::from_le_bytes(
                                    reference[offset..offset + 4].try_into().unwrap(),
                                )),
                                f64::from(f32::from_le_bytes(
                                    actual[offset..offset + 4].try_into().unwrap(),
                                )),
                            )
                        }
                    };
                    if !expected.is_finite() || !observed.is_finite() {
                        nonfinite = true;
                        different = true;
                        continue;
                    }
                    let delta = (expected - observed).abs();
                    report.max_abs_difference = report.max_abs_difference.max(delta);
                    let tolerance = if full.format == RenderPixelFormat::Argb32f {
                        1e-6 + 1e-5 * expected.abs()
                    } else {
                        0.0
                    };
                    different |= delta > tolerance;
                }
                report.nonfinite_pixels += u64::from(nonfinite);
                if different {
                    report.differing_pixels += 1;
                    let bbox = report.difference_bbox.get_or_insert([
                        layer_x,
                        layer_y,
                        layer_x + 1,
                        layer_y + 1,
                    ]);
                    bbox[0] = bbox[0].min(layer_x);
                    bbox[1] = bbox[1].min(layer_y);
                    bbox[2] = bbox[2].max(layer_x + 1);
                    bbox[3] = bbox[3].max(layer_y + 1);
                }
            }
        }
    }
    if covered.iter().any(|covered| !covered) {
        return Err(ComparisonError::IncompleteCoverage);
    }
    Ok(report)
}

#[cfg(windows)]
struct OwnedWorld {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    origin_x: i32,
    origin_y: i32,
    session_receipt: Value,
}

#[cfg(windows)]
impl OwnedWorld {
    fn view(&self, format: RenderPixelFormat) -> NativeWorld<'_> {
        NativeWorld {
            pixels: &self.pixels,
            format,
            width: self.width,
            height: self.height,
            rowbytes: self.width as usize * format.bytes_per_pixel() as usize,
            origin_x: self.origin_x,
            origin_y: self.origin_y,
        }
    }

    fn rect(&self) -> [i32; 4] {
        [
            self.origin_x,
            self.origin_y,
            self.origin_x + self.width as i32,
            self.origin_y + self.height as i32,
        ]
    }

    fn crop(&self, rect: [i32; 4], format: RenderPixelFormat) -> Result<Self, ComparisonError> {
        let own = self.rect();
        if rect[0] < own[0]
            || rect[1] < own[1]
            || rect[2] > own[2]
            || rect[3] > own[3]
            || rect[0] >= rect[2]
            || rect[1] >= rect[3]
        {
            return Err(ComparisonError::InvalidWorld);
        }
        let bytes = format.bytes_per_pixel() as usize;
        let width = (rect[2] - rect[0]) as usize;
        let height = (rect[3] - rect[1]) as usize;
        let source_rowbytes = self.width as usize * bytes;
        let rowbytes = width * bytes;
        let mut pixels = vec![0; rowbytes * height];
        for y in 0..height {
            let source_y = (rect[1] - own[1]) as usize + y;
            let source_x = (rect[0] - own[0]) as usize;
            let start = source_y * source_rowbytes + source_x * bytes;
            pixels[y * rowbytes..(y + 1) * rowbytes]
                .copy_from_slice(&self.pixels[start..start + rowbytes]);
        }
        Ok(Self {
            pixels,
            width: width as u32,
            height: height as u32,
            origin_x: rect[0],
            origin_y: rect[1],
            session_receipt: self.session_receipt.clone(),
        })
    }
}

#[cfg(windows)]
pub struct DifferentialRequest<'a> {
    pub repository: &'a Path,
    pub plugin_path: &'a Path,
    pub plugin_sha256: &'a str,
    pub input_path: &'a Path,
    pub parameters: &'a [InteractiveParameter],
    pub timing: RenderTiming,
    pub smart: bool,
    pub format: RenderPixelFormat,
}

#[cfg(windows)]
fn digest_file(path: &Path) -> io::Result<String> {
    let mut input = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let size = input.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(windows)]
fn shift_rgba_source_to_origin(input: &[u8], width: u32, height: u32, dx: u32, dy: u32) -> Vec<u8> {
    let mut shifted = vec![0; input.len()];
    for y in 0..height.saturating_sub(dy) as usize {
        let source_row = (y + dy as usize) * width as usize * 4;
        let target_row = y * width as usize * 4;
        let size = (width - dx) as usize * 4;
        let source_start = source_row + dx as usize * 4;
        shifted[target_row..target_row + size]
            .copy_from_slice(&input[source_start..source_start + size]);
    }
    shifted
}

#[cfg(windows)]
fn render_variant(
    request: &DifferentialRequest<'_>,
    input_rgba: &[u8],
    width: u32,
    height: u32,
    layout: Option<DiagnosticWorldLayout>,
) -> Result<OwnedWorld, Value> {
    let worker_path = request
        .repository
        .join(crate::secure_image_dispatch::WORKER_RELATIVE_PROGRAM);
    let worker_before = digest_file(&worker_path).ok();
    let plugin_before = digest_file(request.plugin_path).ok();
    let host_path = std::env::current_exe().ok();
    let host_before = host_path.as_ref().and_then(|path| digest_file(path).ok());
    let parent = request
        .plugin_path
        .parent()
        .ok_or_else(|| json!({"stage":"session_open", "reason":"plugin_has_no_parent"}))?;
    let open = SessionOpenRequest {
        repository: request.repository,
        plugin_path: request.plugin_path,
        plugin_sha256: request.plugin_sha256,
        parameters: (!request.parameters.is_empty()).then_some(request.parameters),
        payload_override: None,
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
        layers: &[],
        dependencies: Vec::new(),
        companions: Vec::new(),
        dependency_search_dirs: vec![parent.to_path_buf()],
        width,
        height,
        pixel_format: request.format,
        time_step: request.timing.time_step,
        total_time: request.timing.total_time,
        time_scale: request.timing.time_scale,
        frame_deadline: Duration::from_secs(30),
        smart: request.smart,
        gpu_backend: RenderGpuBackend::Cpu,
        gpu_runtime_policy: None,
        launch_environment: LaunchEnvironment::default(),
    };
    let mut session = match layout {
        Some(layout) => RenderSession::open_diagnostic(open, layout),
        None => RenderSession::open(open),
    }
    .map_err(|_| json!({"stage":"session_open", "reason":"open_failed"}))?;
    let frame = session.render_frame(0, request.timing.current_time, input_rgba);
    let close = session.close();
    let mut image = match frame {
        Ok(outcome) => match outcome.status {
            FrameStatus::Rendered {
                pixels,
                width,
                height,
                origin_x,
                origin_y,
            } if width > 0 && height > 0 && !pixels.is_empty() => Ok(OwnedWorld {
                pixels,
                width,
                height,
                origin_x,
                origin_y,
                session_receipt: Value::Null,
            }),
            FrameStatus::FrameError { render_error, .. } => Err(json!({
                "stage":"frame", "reason":"explicit_frame_error", "render_error":render_error,
            })),
            FrameStatus::SmartOutputUntouched => {
                Err(json!({"stage":"frame", "reason":"smart_output_untouched"}))
            }
            FrameStatus::Rendered { .. } => {
                Err(json!({"stage":"frame", "reason":"invalid_or_empty_image"}))
            }
        },
        Err(_) => Err(json!({"stage":"frame", "reason":"transport_or_invariant_failure"})),
    }?;
    if let Err(error) = validate_completed_session_close(&close, request.smart) {
        return Err(json!({"stage":"session_close", "reason":error}));
    }
    let final_report = &close["final_report"];
    let admitted = final_report["worker_admitted_plugin_sha256"].as_str();
    let worker_after = digest_file(&worker_path).ok();
    let plugin_after = digest_file(request.plugin_path).ok();
    let host_after = host_path.as_ref().and_then(|path| digest_file(path).ok());
    let report_sha = serde_json::to_vec(final_report)
        .ok()
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)));
    image.session_receipt = json!({
        "input_rgba_sha256":format!("{:x}", Sha256::digest(input_rgba)),
        "worker_admitted_plugin_sha256":admitted,
        "plugin_file_sha256_before":plugin_before,
        "plugin_file_sha256_after":plugin_after,
        "worker_file_sha256_before":worker_before,
        "worker_file_sha256_after":worker_after,
        "host_file_sha256_before":host_before,
        "host_file_sha256_after":host_after,
        "admitted_matches_requested":admitted.is_some_and(|sha|
            sha.eq_ignore_ascii_case(request.plugin_sha256)),
        "plugin_file_stable":plugin_before.is_some() && plugin_before == plugin_after,
        "worker_file_stable":worker_before.is_some() && worker_before == worker_after,
        "host_file_stable":host_before.is_some() && host_before == host_after,
        "close_plugin_sha256":close["plugin_sha256"],
        "worker_classification":close["worker"]["classification"],
        "final_report_sha256":report_sha,
        "module_audit":final_report["module_audit"],
        "module_audit_warning":close["module_audit_warning"],
    });
    Ok(image)
}

#[cfg(windows)]
fn compare_case(
    id: &str,
    expected_regions: &[[i32; 4]],
    baseline: &OwnedWorld,
    variants: Vec<Result<OwnedWorld, Value>>,
    format: RenderPixelFormat,
) -> Value {
    compare_case_in_region(id, expected_regions, baseline, variants, format, None)
}

#[cfg(windows)]
fn comparable_identity(receipt: &Value) -> Option<(&str, &str, &str, &str)> {
    if receipt["admitted_matches_requested"] != true
        || receipt["plugin_file_stable"] != true
        || receipt["worker_file_stable"] != true
        || receipt["host_file_stable"] != true
    {
        return None;
    }
    Some((
        receipt["worker_admitted_plugin_sha256"].as_str()?,
        receipt["plugin_file_sha256_before"].as_str()?,
        receipt["worker_file_sha256_before"].as_str()?,
        receipt["host_file_sha256_before"].as_str()?,
    ))
}

#[cfg(windows)]
fn compare_case_in_region(
    id: &str,
    expected_regions: &[[i32; 4]],
    baseline: &OwnedWorld,
    variants: Vec<Result<OwnedWorld, Value>>,
    format: RenderPixelFormat,
    comparison_rect: Option<[i32; 4]>,
) -> Value {
    let mut images = Vec::with_capacity(variants.len());
    for (index, variant) in variants.into_iter().enumerate() {
        let image = match variant {
            Ok(image) => image,
            Err(failure) => {
                return json!({
                    "id":id, "status":"failed", "region":expected_regions,
                    "failure":failure,
                });
            }
        };
        if image.rect() != expected_regions[index] {
            return json!({
                "id":id, "status":"unsupported", "region":expected_regions,
                "reason":"output_geometry_did_not_honor_request",
                "observed_region":image.rect(),
                "session_receipts":[image.session_receipt],
            });
        }
        images.push(image);
    }
    let cropped = comparison_rect.map(|rect| {
        let reference = baseline.crop(rect, format)?;
        let variants = images
            .iter()
            .map(|image| image.crop(rect, format))
            .collect::<Result<Vec<_>, _>>()?;
        Ok::<_, ComparisonError>((reference, variants))
    });
    let session_receipts: Vec<_> = images
        .iter()
        .map(|image| image.session_receipt.clone())
        .collect();
    let baseline_identity = comparable_identity(&baseline.session_receipt);
    if baseline_identity.is_none()
        || images
            .iter()
            .any(|image| comparable_identity(&image.session_receipt) != baseline_identity)
    {
        return json!({
            "id":id, "status":"unsupported", "region":expected_regions,
            "reason":"session_identity_not_comparable",
            "session_receipts":session_receipts,
        });
    }
    let (reference, images) = match cropped {
        Some(Ok((reference, images))) => (Some(reference), images),
        Some(Err(error)) => {
            return json!({
                "id":id, "status":"failed", "region":expected_regions,
                "failure":{"stage":"comparison", "reason":format!("{error:?}")},
            });
        }
        None => (None, images),
    };
    let views: Vec<_> = images.iter().map(|image| image.view(format)).collect();
    let reference_view = reference.as_ref().unwrap_or(baseline).view(format);
    match compare_native_worlds(reference_view, &views) {
        Ok(comparison) => json!({
            "id":id,
            "status":if comparison.differing_pixels == 0 {"matched"} else {"different"},
            "region":expected_regions,
            "comparison":comparison,
            "session_receipts":session_receipts,
        }),
        Err(error) => json!({
            "id":id, "status":"failed", "region":expected_regions,
            "failure":{"stage":"comparison", "reason":format!("{error:?}")},
            "session_receipts":session_receipts,
        }),
    }
}

/// Runs one bounded full-frame/variant family through the same resident worker
/// as the shipping session renderer. A transformation the plug-in did not
/// honor is explicit `unsupported`, never replaced with baseline pixels.
#[cfg(windows)]
pub fn run_native_differential(request: DifferentialRequest<'_>) -> io::Result<Value> {
    let plugin_path = request.plugin_path.canonicalize()?;
    let request = DifferentialRequest {
        plugin_path: &plugin_path,
        ..request
    };
    let timing = request.timing;
    if timing.current_time < 0
        || timing.time_step <= 0
        || timing.total_time < timing.current_time
        || timing.time_scale == 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid render time",
        ));
    }
    let image = decode_bounded_image(request.input_path, "input")?.into_rgba8();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty input image",
        ));
    }
    let input_rgba = image.into_raw();
    let host_hash = std::env::current_exe()
        .ok()
        .and_then(|path| digest_file(&path).ok());
    let worker_hash = digest_file(
        &request
            .repository
            .join(crate::secure_image_dispatch::WORKER_RELATIVE_PROGRAM),
    )
    .ok();
    let plugin_path_hash = format!(
        "{:x}",
        Sha256::digest(
            request
                .plugin_path
                .canonicalize()?
                .to_string_lossy()
                .as_bytes()
        )
    );
    let parameters_hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(request.parameters)?)
    );
    let provenance = json!({
        "plugin_sha256":request.plugin_sha256,
        "plugin_path_sha256":plugin_path_hash,
        "host_executable_sha256":host_hash,
        "worker_executable_sha256":worker_hash,
        "input_rgba_sha256":format!("{:x}", Sha256::digest(&input_rgba)),
        "parameters_sha256":parameters_hash,
    });
    let conditions = json!({
        "width":width, "height":height,
        "pixel_format":request.format.report_name(),
        "render_path":if request.smart {"smart"} else {"classic"},
        "current_time":timing.current_time,
        "time_step":timing.time_step,
        "total_time":timing.total_time,
        "time_scale":timing.time_scale,
        "gpu_backend":"cpu",
    });
    let baseline = match render_variant(&request, &input_rgba, width, height, None) {
        Ok(baseline) => baseline,
        Err(failure) => {
            return Ok(json!({
                "schema_version":1, "stage":"render_differential", "passed":false,
                "provenance":provenance, "conditions":conditions,
                "cases":[{"id":"full", "status":"failed", "failure":failure,
                    "pixel_format":request.format.report_name(),
                    "render_path":if request.smart {"smart"} else {"classic"}}],
            }));
        }
    };
    let full_rect = baseline.rect();
    let mut cases = vec![json!({
        "id":"full", "status":"rendered", "region":full_rect,
        "session_receipts":[baseline.session_receipt],
    })];
    let pixel_bytes = request.format.bytes_per_pixel() as u32;
    cases.push(compare_case(
        "padded_stride",
        &[full_rect],
        &baseline,
        vec![render_variant(
            &request,
            &input_rgba,
            width,
            height,
            Some(DiagnosticWorldLayout {
                input_row_padding: pixel_bytes * 3,
                output_row_padding: pixel_bytes * 5,
                ..DiagnosticWorldLayout::default()
            }),
        )],
        request.format,
    ));
    // Repack source pixels so their layer-space content is unchanged where
    // the baseline and shifted input worlds overlap. Outside that support,
    // a sampling effect may legitimately disagree and is not certified.
    if width > 3 && height > 2 {
        let common = [3, 2, width as i32, height as i32];
        let comparison_rect = [
            common[0].max(full_rect[0]),
            common[1].max(full_rect[1]),
            common[2].min(full_rect[2]),
            common[3].min(full_rect[3]),
        ];
        if comparison_rect[0] < comparison_rect[2] && comparison_rect[1] < comparison_rect[3] {
            let translated = shift_rgba_source_to_origin(&input_rgba, width, height, 3, 2);
            let mut origin_case = compare_case_in_region(
                "shifted_input_origin",
                &[full_rect],
                &baseline,
                vec![render_variant(
                    &request,
                    &translated,
                    width,
                    height,
                    Some(DiagnosticWorldLayout {
                        input_origin_x: 3,
                        input_origin_y: 2,
                        ..DiagnosticWorldLayout::default()
                    }),
                )],
                request.format,
                Some(comparison_rect),
            );
            if origin_case["status"] == "different" {
                origin_case["status"] = json!("unsupported");
                origin_case["reason"] = json!("shifted_source_support_not_equivalent");
            }
            origin_case["input_origin"] = json!([3, 2]);
            cases.push(origin_case);
        } else {
            cases.push(json!({"id":"shifted_input_origin", "status":"unsupported",
                "reason":"no_common_layer_region"}));
        }
    } else {
        cases.push(json!({"id":"shifted_input_origin", "status":"unsupported",
            "reason":"image_too_small"}));
    }
    if width > 2 && height > 2 {
        let hint = [1, 1, width as i32 - 1, height as i32 - 1];
        let comparison_rect = [
            hint[0].max(full_rect[0]),
            hint[1].max(full_rect[1]),
            hint[2].min(full_rect[2]),
            hint[3].min(full_rect[3]),
        ];
        let layout = DiagnosticWorldLayout {
            extent_hint: Some(hint),
            ..DiagnosticWorldLayout::default()
        };
        if comparison_rect[0] < comparison_rect[2] && comparison_rect[1] < comparison_rect[3] {
            cases.push(compare_case_in_region(
                "extent_hint",
                &[full_rect],
                &baseline,
                vec![render_variant(
                    &request,
                    &input_rgba,
                    width,
                    height,
                    Some(layout),
                )],
                request.format,
                Some(comparison_rect),
            ));
        } else {
            cases.push(json!({"id":"extent_hint", "status":"unsupported",
                "reason":"no_common_effective_region"}));
        }
    } else {
        cases.push(json!({"id":"extent_hint", "status":"unsupported", "reason":"image_too_small"}));
    }
    if !request.smart {
        if full_rect != [0, 0, width as i32, height as i32] {
            cases.push(compare_case(
                "classic_output_geometry",
                &[full_rect],
                &baseline,
                vec![render_variant(
                    &request,
                    &input_rgba,
                    width,
                    height,
                    Some(DiagnosticWorldLayout {
                        output_row_padding: pixel_bytes * 3,
                        ..DiagnosticWorldLayout::default()
                    }),
                )],
                request.format,
            ));
        } else {
            cases.push(json!({
                "id":"classic_output_geometry", "status":"unsupported",
                "reason":"plugin_did_not_resize_or_shift_output",
            }));
        }
        for id in ["horizontal_tiles", "vertical_tiles"] {
            cases.push(
                json!({"id":id, "status":"unsupported", "reason":"classic_has_no_request_rect"}),
            );
        }
    } else if full_rect != [0, 0, width as i32, height as i32] || width < 2 || height < 2 {
        for id in ["horizontal_tiles", "vertical_tiles"] {
            cases.push(
                json!({"id":id, "status":"unsupported", "reason":"baseline_geometry_not_tileable"}),
            );
        }
    } else {
        for (id, rects) in [
            (
                "horizontal_tiles",
                [
                    [0, 0, (width / 2) as i32, height as i32],
                    [(width / 2) as i32, 0, width as i32, height as i32],
                ],
            ),
            (
                "vertical_tiles",
                [
                    [0, 0, width as i32, (height / 2) as i32],
                    [0, (height / 2) as i32, width as i32, height as i32],
                ],
            ),
        ] {
            let outputs = rects
                .iter()
                .map(|rect| {
                    render_variant(
                        &request,
                        &input_rgba,
                        width,
                        height,
                        Some(DiagnosticWorldLayout {
                            request_rect: Some(*rect),
                            output_row_padding: pixel_bytes * 3,
                            ..DiagnosticWorldLayout::default()
                        }),
                    )
                })
                .collect();
            cases.push(compare_case(id, &rects, &baseline, outputs, request.format));
        }
    }
    for case in &mut cases {
        case["pixel_format"] = json!(request.format.report_name());
        case["render_path"] = json!(if request.smart { "smart" } else { "classic" });
    }
    let passed = cases.iter().skip(1).all(|case| case["status"] == "matched");
    Ok(json!({
        "schema_version":1, "stage":"render_differential", "passed":passed,
        "provenance":provenance, "conditions":conditions, "cases":cases,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn shifted_source_preserves_only_shared_layer_coordinates() {
        let source: Vec<u8> = (0..5 * 4).flat_map(|index| [index as u8; 4]).collect();
        let shifted = shift_rgba_source_to_origin(&source, 5, 4, 2, 1);
        for y in 0..3 {
            for x in 0..3 {
                let target = ((y * 5 + x) * 4) as usize;
                let original = (((y + 1) * 5 + x + 2) * 4) as usize;
                assert_eq!(
                    &shifted[target..target + 4],
                    &source[original..original + 4]
                );
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn identity_discrepancy_disqualifies_comparison_not_render_launch() {
        let mut receipt = json!({
            "admitted_matches_requested":true,
            "plugin_file_stable":true,
            "worker_file_stable":true,
            "host_file_stable":true,
            "worker_admitted_plugin_sha256":"a",
            "plugin_file_sha256_before":"a",
            "worker_file_sha256_before":"b",
            "host_file_sha256_before":"c",
        });
        assert!(comparable_identity(&receipt).is_some());
        receipt["worker_file_stable"] = json!(false);
        assert!(comparable_identity(&receipt).is_none());
        receipt["worker_file_stable"] = json!(true);
        receipt["worker_file_sha256_before"] = json!("changed");
        assert_ne!(
            comparable_identity(&receipt),
            comparable_identity(&json!({
                "admitted_matches_requested":true,
                "plugin_file_stable":true,
                "worker_file_stable":true,
                "host_file_stable":true,
                "worker_admitted_plugin_sha256":"a",
                "plugin_file_sha256_before":"a",
                "worker_file_sha256_before":"b",
                "host_file_sha256_before":"c",
            }))
        );
    }

    fn world<'a>(
        pixels: &'a [u8],
        width: u32,
        height: u32,
        rowbytes: usize,
        x: i32,
        y: i32,
    ) -> NativeWorld<'a> {
        NativeWorld {
            pixels,
            format: RenderPixelFormat::Argb8,
            width,
            height,
            rowbytes,
            origin_x: x,
            origin_y: y,
        }
    }

    #[test]
    fn split_tiles_match_and_pixel_mutation_has_layer_space_bbox() {
        let full = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        ];
        let left = [1, 2, 3, 4, 13, 14, 15, 16];
        let right = [5, 6, 7, 8, 9, 10, 11, 12, 17, 18, 19, 20, 21, 22, 23, 24];
        let reference = world(&full, 3, 2, 12, 0, 0);
        let matched = compare_native_worlds(
            reference,
            &[world(&left, 1, 2, 4, 0, 0), world(&right, 2, 2, 8, 1, 0)],
        )
        .unwrap();
        assert_eq!(matched.compared_pixels, 6);
        assert_eq!(matched.differing_pixels, 0);
        assert_eq!(matched.difference_bbox, None);

        let mut wrong = right;
        wrong[8] ^= 1;
        let changed = compare_native_worlds(
            reference,
            &[world(&left, 1, 2, 4, 0, 0), world(&wrong, 2, 2, 8, 1, 0)],
        )
        .unwrap();
        assert_eq!(changed.differing_pixels, 1);
        assert_eq!(changed.difference_bbox, Some([1, 1, 2, 2]));
    }

    #[test]
    fn padded_rows_and_shifted_origin_compare_logical_pixels_only() {
        let full = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let padded = [
            1, 2, 3, 4, 5, 6, 7, 8, 99, 99, 99, 99, 9, 10, 11, 12, 13, 14, 15, 16, 99, 99, 99, 99,
        ];
        let report = compare_native_worlds(
            world(&full, 2, 2, 8, -3, 4),
            &[world(&padded, 2, 2, 12, -3, 4)],
        )
        .unwrap();
        assert_eq!(report.comparison_rect, [-3, 4, -1, 6]);
        assert_eq!(report.differing_pixels, 0);
    }

    #[test]
    fn missing_or_duplicate_tile_cannot_pass() {
        let full = [1u8; 16];
        let reference = world(&full, 2, 2, 8, 0, 0);
        let half = [1u8; 8];
        assert_eq!(
            compare_native_worlds(reference, &[world(&half, 1, 2, 4, 0, 0)]),
            Err(ComparisonError::IncompleteCoverage)
        );
        assert_eq!(
            compare_native_worlds(
                reference,
                &[world(&full, 2, 2, 8, 0, 0), world(&half, 1, 2, 4, 0, 0)]
            ),
            Err(ComparisonError::OverlappingCoverage)
        );
    }

    #[test]
    fn argb16_is_exact_and_float_tolerance_does_not_hide_nan() {
        let reference16 = [0u8, 0, 1, 0, 2, 0, 3, 0];
        let changed16 = [0u8, 0, 1, 0, 3, 0, 3, 0];
        fn make_world<'a>(pixels: &'a [u8], format: RenderPixelFormat) -> NativeWorld<'a> {
            NativeWorld {
                pixels,
                format,
                width: 1,
                height: 1,
                rowbytes: usize::try_from(format.bytes_per_pixel()).unwrap(),
                origin_x: 0,
                origin_y: 0,
            }
        }
        let report16 = compare_native_worlds(
            make_world(&reference16, RenderPixelFormat::Argb16),
            &[make_world(&changed16, RenderPixelFormat::Argb16)],
        )
        .unwrap();
        assert_eq!(report16.differing_pixels, 1);
        assert_eq!(report16.max_abs_difference, 1.0);

        let reference32 = [0.0f32, 0.5, 1.0, 2.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let near32 = [0.0f32, 0.500001, 1.0, 2.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let nan32 = [0.0f32, f32::NAN, 1.0, 2.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let near = compare_native_worlds(
            make_world(&reference32, RenderPixelFormat::Argb32f),
            &[make_world(&near32, RenderPixelFormat::Argb32f)],
        )
        .unwrap();
        assert_eq!(near.differing_pixels, 0);
        let nan = compare_native_worlds(
            make_world(&reference32, RenderPixelFormat::Argb32f),
            &[make_world(&nan32, RenderPixelFormat::Argb32f)],
        )
        .unwrap();
        assert_eq!(nan.differing_pixels, 1);
        assert_eq!(nan.nonfinite_pixels, 1);
    }
}
