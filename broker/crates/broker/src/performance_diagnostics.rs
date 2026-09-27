//! Bounded advisory summaries for a repeated native render request family.

use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_SAMPLES: usize = 64;

/// One completed frame. All timings use a monotonic clock in nanoseconds;
/// missing phase values are observations, never zeros. A session index denotes
/// one fresh worker, and frame zero is the cold frame in that worker.
#[derive(Clone, Debug, Serialize)]
pub struct Sample {
    pub width: u32,
    pub height: u32,
    pub session_index: u32,
    pub frame_index: u32,
    pub cold: bool,
    pub session_open_ns: Option<u64>,
    pub wall_ns: u64,
    pub input_write_ns: Option<u64>,
    pub worker_setup_ns: Option<u64>,
    pub worker_render_ns: Option<u64>,
    pub selector_dispatch_ns: Option<u64>,
    pub worker_finalize_ns: Option<u64>,
    pub output_verify_ns: Option<u64>,
    pub output_bytes: Option<u64>,
    pub worker_live_commit_bytes: Option<u64>,
    pub worker_peak_commit_bytes: Option<u64>,
    pub worker_job_peak_commit_bytes: Option<u64>,
}

fn median(values: &mut [u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        Some(
            values[middle - 1] / 2
                + values[middle] / 2
                + (values[middle - 1] % 2 + values[middle] % 2) / 2,
        )
    } else {
        Some(values[middle])
    }
}

fn measure(samples: &[&Sample], value: impl Fn(&Sample) -> Option<u64>, unit: &str) -> Value {
    let median_key = if unit == "bytes" {
        "median_bytes"
    } else {
        "median_ns"
    };
    if samples.is_empty() {
        return json!({"status": "unavailable", "reason": "no_samples", median_key: null,
            "sample_count": 0, "unit": unit});
    }
    let mut values = Vec::with_capacity(samples.len());
    for sample in samples {
        match value(sample) {
            Some(nanos) => values.push(nanos),
            None => {
                return json!({"status": "unavailable", "reason": "not_measured",
                median_key: null, "sample_count": samples.len(), "unit": unit});
            }
        }
    }
    json!({"status": "available", "reason": null, median_key: median(&mut values),
        "sample_count": samples.len(), "unit": unit})
}

fn phase(samples: &[&Sample], value: impl Fn(&Sample) -> Option<u64>) -> Value {
    measure(samples, value, "ns")
}

fn growth_per_frame(samples: &[&Sample]) -> Option<(f64, bool)> {
    if samples.len() < 4 {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by_key(|sample| sample.frame_index);
    if sorted
        .windows(2)
        .any(|pair| pair[0].frame_index + 1 != pair[1].frame_index)
    {
        return None;
    }
    let values: Vec<u64> = sorted
        .iter()
        .map(|sample| sample.worker_live_commit_bytes)
        .collect::<Option<_>>()?;
    // Preserve the signed observation even when commit falls between frames.
    // A peak is monotone by definition and cannot establish a leak; only a
    // separate monotone-live flag can make a positive slope a candidate.
    let slope = (values[values.len() - 1] as f64 - values[0] as f64) / (values.len() - 1) as f64;
    let monotone = values.windows(2).all(|pair| pair[1] >= pair[0]);
    Some((slope, monotone))
}

/// Summarize a single request family. The caller must separately prove common
/// plug-in bytes, host build, parameters, time, depth, path, and input pattern.
/// No success verdict is changed by this advisory report.
pub fn summarize(samples: &[Sample]) -> Value {
    if samples.is_empty() || samples.len() > MAX_SAMPLES {
        return json!({"advisory": true, "status": "unavailable",
            "reason": if samples.is_empty() { "no_samples" } else { "sample_limit" },
            "sample_limit": MAX_SAMPLES});
    }
    let mut seen = BTreeSet::new();
    let mut groups: BTreeMap<(u32, u32), Vec<&Sample>> = BTreeMap::new();
    for sample in samples {
        if sample.width == 0
            || sample.height == 0
            || sample.wall_ns == 0
            || sample.cold != (sample.frame_index == 0)
            || !seen.insert((sample.session_index, sample.frame_index))
        {
            return json!({"advisory": true, "status": "unavailable",
                "reason": "invalid_sample", "sample_limit": MAX_SAMPLES});
        }
        groups
            .entry((sample.width, sample.height))
            .or_default()
            .push(sample);
    }
    let mut resolutions = Vec::with_capacity(groups.len());
    let mut memory_growth_resolutions = 0;
    let mut adjacent_inputs = Vec::new();
    for ((width, height), group) in groups {
        let cold: Vec<&Sample> = group.iter().copied().filter(|s| s.cold).collect();
        let warm: Vec<&Sample> = group.iter().copied().filter(|s| !s.cold).collect();
        let pixel_count = width as u64 * height as u64;
        let warm_dispatch = phase(&warm, |s| s.selector_dispatch_ns);
        let dispatch_median = warm_dispatch["median_ns"].as_u64();
        let process_peak = group
            .iter()
            .filter_map(|s| s.worker_peak_commit_bytes)
            .max();
        let job_peak = group
            .iter()
            .filter_map(|s| s.worker_job_peak_commit_bytes)
            .max();
        adjacent_inputs.push((pixel_count, dispatch_median));
        let mut sessions: BTreeMap<u32, Vec<&Sample>> = BTreeMap::new();
        for sample in &group {
            sessions
                .entry(sample.session_index)
                .or_default()
                .push(sample);
        }
        let slopes: Vec<(f64, bool)> = sessions
            .values()
            .filter_map(|frames| growth_per_frame(frames))
            .collect();
        if slopes
            .iter()
            .any(|(slope, monotone)| *monotone && *slope >= 131_072.0)
        {
            memory_growth_resolutions += 1;
        }
        let live_slope = if slopes.is_empty() {
            None
        } else {
            Some(slopes.iter().map(|(slope, _)| slope).sum::<f64>() / slopes.len() as f64)
        };
        let live_monotone = (!slopes.is_empty()).then(|| slopes.iter().all(|(_, flag)| *flag));
        resolutions.push(json!({
            "width": width, "height": height, "pixels": pixel_count,
            "cold_session_open": phase(&cold, |s| s.session_open_ns),
            "cold_wall": phase(&cold, |s| Some(s.wall_ns)),
            "warm_wall": phase(&warm, |s| Some(s.wall_ns)),
            "warm_input_write": phase(&warm, |s| s.input_write_ns),
            "warm_worker_setup": phase(&warm, |s| s.worker_setup_ns),
            "warm_worker_render": phase(&warm, |s| s.worker_render_ns),
            "warm_selector_dispatch": warm_dispatch,
            "warm_worker_nonselector": phase(&warm, |s| {
                s.worker_render_ns?.checked_sub(s.selector_dispatch_ns?)
            }),
            "warm_nonselector_wall": phase(&warm, |s| {
                s.wall_ns.checked_sub(s.selector_dispatch_ns?)
            }),
            "warm_broker_ipc_schedule_estimate": phase(&warm, |s| {
                s.wall_ns.checked_sub(s.worker_setup_ns?)?
                    .checked_sub(s.worker_render_ns?)?
                    .checked_sub(s.worker_finalize_ns?)
            }),
            "warm_worker_finalize": phase(&warm, |s| s.worker_finalize_ns),
            "warm_output_verify": phase(&warm, |s| s.output_verify_ns),
            "warm_ns_per_megapixel": dispatch_median.map(|ns| ns as f64 * 1_000_000.0 / pixel_count as f64),
            "warm_ms_per_megapixel": dispatch_median.map(|ns| ns as f64 / pixel_count as f64),
            "warm_ms_per_megapixel_status": if dispatch_median.is_some() { "available" } else { "unavailable" },
            "warm_ms_per_megapixel_unavailable_reason": if dispatch_median.is_some() { None } else { Some("missing_dispatch_measurement") },
            "output_bytes": measure(&warm, |s| s.output_bytes, "bytes"),
            "worker_live_commit_bytes": measure(&warm, |s| s.worker_live_commit_bytes, "bytes"),
            "worker_peak_commit_bytes": process_peak,
            "worker_peak_commit_status": if process_peak.is_some() { "available" } else { "unavailable" },
            "worker_peak_commit_unavailable_reason": if process_peak.is_some() { None } else { Some("process_query_failed") },
            "worker_job_peak_commit_bytes": job_peak,
            "worker_job_peak_commit_status": if job_peak.is_some() { "available" } else { "unavailable" },
            "worker_job_peak_commit_unavailable_reason": if job_peak.is_some() { None } else { Some("job_query_failed") },
            "worker_peak_commit_bytes_per_pixel": process_peak.map(|bytes| bytes as f64 / pixel_count as f64),
            "worker_live_commit_slope_bytes_per_frame": live_slope,
            "worker_live_commit_slope_status": if live_slope.is_some() { "available" } else { "unavailable" },
            "worker_live_commit_slope_unavailable_reason": if live_slope.is_some() { None } else { Some("insufficient_contiguous_frames_or_query_failed") },
            "worker_live_commit_monotone_non_decreasing": live_monotone,
            "sample_count": group.len(),
        }));
    }
    let mut adjacent_ratios = Vec::new();
    adjacent_inputs.sort_by_key(|(pixels, _)| *pixels);
    for pair in adjacent_inputs.windows(2) {
        let (small_pixels, small_ns) = pair[0];
        let (large_pixels, large_ns) = pair[1];
        let pixel_ratio = large_pixels as f64 / small_pixels as f64;
        let normalized = match (small_ns, large_ns) {
            (Some(a), Some(b)) if a > 0 => Some((b as f64 / a as f64) / pixel_ratio),
            _ => None,
        };
        adjacent_ratios.push(
            json!({"from_pixels": small_pixels, "to_pixels": large_pixels,
            "pixel_ratio": pixel_ratio, "normalized_dispatch_ratio": normalized,
            "status": if normalized.is_some() { "available" } else { "unavailable" },
            "unavailable_reason": if normalized.is_some() { None } else { Some("missing_dispatch_measurement") }}),
        );
    }
    // A single adjacent step is sensitive to scheduler and clock noise,
    // especially at the smallest resolution. Compare the endpoints of the
    // three largest resolutions while retaining every adjacent observation.
    let high_end_normalized_ratio = adjacent_inputs
        .get(adjacent_inputs.len().saturating_sub(3)..)
        .filter(|points| points.len() == 3)
        .and_then(|points| match (points[0].1, points[2].1) {
            (Some(small_ns), Some(large_ns)) if small_ns > 0 => Some(
                (large_ns as f64 / small_ns as f64) / (points[2].0 as f64 / points[0].0 as f64),
            ),
            _ => None,
        });
    let mut reasons = Vec::new();
    if high_end_normalized_ratio.is_some_and(|ratio| ratio >= 2.25) {
        reasons.push("superlinear_candidate");
    }
    if memory_growth_resolutions >= 2 {
        reasons.push("memory_growth_candidate");
    }
    json!({"advisory": true, "status": "available", "sample_limit": MAX_SAMPLES,
        "sample_count": samples.len(), "timer_unit": "monotonic_nanoseconds",
        "clocks": "independent_broker_and_worker",
        "timer_resolution": {"status": "unavailable", "reason": "not_calibrated"},
        "aggregation": "median", "outlier_policy": "none_removed",
        "classification_policy": {
            "superlinear_high_end_normalized_ratio_min": 2.25,
            "superlinear_high_end_resolution_count": 3,
            "memory_growth_live_slope_min_bytes_per_frame": 131_072,
            "memory_growth_required_resolutions": 2,
        },
        "resolutions": resolutions, "adjacent_ratios": adjacent_ratios,
        "high_end_normalized_dispatch_ratio": high_end_normalized_ratio,
        "high_end_normalized_dispatch_ratio_status": if high_end_normalized_ratio.is_some() { "available" } else { "unavailable" },
        "high_end_normalized_dispatch_ratio_unavailable_reason": if high_end_normalized_ratio.is_some() { None } else { Some("insufficient_resolutions_or_missing_dispatch_measurement") },
        "reasons": reasons})
}

#[cfg(windows)]
fn incomplete(
    samples: &[Sample],
    identity: &Value,
    reason: &str,
    session_index: Option<usize>,
    frame_index: Option<u32>,
) -> Value {
    json!({
        "advisory": true,
        "status": "unavailable",
        "reason": reason,
        "identity": identity,
        "sample_count": samples.len(),
        "samples": samples,
        "session_index": session_index,
        "frame_index": frame_index,
    })
}

/// Run one bounded ladder through the ordinary isolated resident session.
/// This route does not save image pixels, change a render verdict, or add a
/// performance threshold to admission. Its returned document has identities
/// but no private paths, input bytes, or authorization material.
#[cfg(windows)]
pub fn run(request: RunRequest<'_>) -> std::io::Result<Value> {
    use crate::image_render::{MAX_DIMENSION, MAX_PIXELS};
    use crate::render_session::{FrameStatus, RenderSession, SessionOpenRequest};
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::io::{self, ErrorKind};
    use std::time::{Duration, Instant};

    fn invalid(message: &str) -> io::Error {
        io::Error::new(ErrorKind::InvalidInput, message)
    }
    fn digest_file(path: &std::path::Path) -> io::Result<String> {
        Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
    }
    if request.resolutions.is_empty()
        || request.frames_per_resolution < 4
        || request.frames_per_resolution > 16
        || request.resolutions.len() * request.frames_per_resolution as usize > MAX_SAMPLES
        || request.timeout_ms == 0
    {
        return Err(invalid(
            "performance ladder exceeds the bounded sample contract",
        ));
    }
    let mut distinct = BTreeSet::new();
    for &(width, height) in request.resolutions {
        if width == 0
            || height == 0
            || width > MAX_DIMENSION
            || height > MAX_DIMENSION
            || u64::from(width) * u64::from(height) > MAX_PIXELS
            || !distinct.insert((width, height))
        {
            return Err(invalid(
                "performance ladder has invalid or duplicate dimensions",
            ));
        }
    }
    let host_path = std::env::current_exe()?;
    let worker_path = request
        .repository
        .join(crate::secure_image_dispatch::WORKER_RELATIVE_PROGRAM);
    let host_sha256 = digest_file(&host_path)?;
    let worker_sha256 = digest_file(&worker_path)?;
    let plugin_sha256_before = digest_file(request.plugin_path)?;
    let params_sha256 = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(request.parameters)?)
    );
    let mut identity = json!({
        "plugin_sha256_at_start": plugin_sha256_before,
        "host_executable_sha256": host_sha256,
        "worker_executable_sha256": worker_sha256,
        "parameter_sha256": params_sha256,
        "render_path": if request.smart { "smart" } else { "classic" },
        "pixel_format": request.pixel_format.report_name(),
        "current_time": 0,
        "time_scale": 1,
        "input_pattern": "normalized_xy_xor_v1",
        "worker_admitted_plugin_sha256_per_session": [],
    });
    let parent = request
        .plugin_path
        .parent()
        .ok_or_else(|| invalid("performance plug-in has no parent directory"))?;
    let mut samples = Vec::new();
    let mut session_open_ns = Vec::new();
    let mut worker_admitted_hashes = Vec::new();
    for (session_index, &(width, height)) in request.resolutions.iter().enumerate() {
        let pixel_count = width as usize * height as usize;
        let mut rgba = vec![0u8; pixel_count * 4];
        for y in 0..height as usize {
            for x in 0..width as usize {
                let offset = (y * width as usize + x) * 4;
                let normalized_x = ((x * 255) / width as usize) as u8;
                let normalized_y = ((y * 255) / height as usize) as u8;
                rgba[offset] = normalized_x;
                rgba[offset + 1] = normalized_y;
                rgba[offset + 2] = normalized_x ^ normalized_y;
                rgba[offset + 3] = 255;
            }
        }
        let opened_at = Instant::now();
        let opened = RenderSession::open(SessionOpenRequest {
            repository: request.repository,
            plugin_path: request.plugin_path,
            plugin_sha256: request.plugin_sha256,
            parameters: Some(request.parameters),
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
            smart: request.smart,
            gpu_backend: crate::image_render::RenderGpuBackend::Auto,
            gpu_runtime_policy: None,
            dependencies: Vec::new(),
            companions: Vec::new(),
            dependency_search_dirs: vec![parent.to_path_buf()],
            width,
            height,
            pixel_format: request.pixel_format,
            time_step: 1,
            total_time: 0,
            time_scale: 1,
            frame_deadline: Duration::from_millis(request.timeout_ms),
            launch_environment: Default::default(),
        });
        let mut session = match opened {
            Ok(session) => session,
            Err(_) => {
                return Ok(incomplete(
                    &samples,
                    &identity,
                    "session_open_failed",
                    Some(session_index),
                    None,
                ));
            }
        };
        let open_ns = opened_at.elapsed().as_nanos() as u64;
        session_open_ns.push(open_ns);
        for frame_index in 0..request.frames_per_resolution {
            let outcome = match session.render_frame_with_parameters(frame_index, 0, &rgba, None) {
                Ok(outcome) => outcome,
                Err(_) => {
                    let reason = session
                        .invalidation()
                        .map(|invalidation| invalidation.reason)
                        .unwrap_or("frame_dispatch_failed");
                    let _ = session.close();
                    return Ok(incomplete(
                        &samples,
                        &identity,
                        reason,
                        Some(session_index),
                        Some(frame_index),
                    ));
                }
            };
            let pixels = match outcome.status {
                FrameStatus::Rendered { pixels, .. } if !pixels.is_empty() => pixels,
                FrameStatus::FrameError { render_error, .. } => {
                    let _ = session.close();
                    let mut report = incomplete(
                        &samples,
                        &identity,
                        "frame_error",
                        Some(session_index),
                        Some(frame_index),
                    );
                    report["render_error"] = json!(render_error);
                    return Ok(report);
                }
                FrameStatus::SmartOutputUntouched => {
                    let _ = session.close();
                    return Ok(incomplete(
                        &samples,
                        &identity,
                        "smart_output_untouched",
                        Some(session_index),
                        Some(frame_index),
                    ));
                }
                FrameStatus::Rendered { .. } => {
                    let _ = session.close();
                    return Ok(incomplete(
                        &samples,
                        &identity,
                        "empty_output",
                        Some(session_index),
                        Some(frame_index),
                    ));
                }
            };
            let timing = outcome.performance;
            samples.push(Sample {
                width,
                height,
                session_index: session_index as u32,
                frame_index,
                cold: frame_index == 0,
                session_open_ns: (frame_index == 0).then_some(open_ns),
                wall_ns: timing.broker_frame_wall_ns,
                input_write_ns: Some(timing.broker_input_write_ns),
                worker_setup_ns: timing.worker_setup_ns,
                worker_render_ns: timing.worker_render_ns,
                selector_dispatch_ns: timing.render_selector_ns,
                worker_finalize_ns: timing.worker_finalize_ns,
                output_verify_ns: timing.broker_output_verify_ns,
                output_bytes: Some(pixels.len() as u64),
                worker_live_commit_bytes: timing.worker_live_commit_bytes,
                worker_peak_commit_bytes: timing.worker_peak_commit_bytes,
                worker_job_peak_commit_bytes: timing.worker_job_peak_commit_bytes,
            });
        }
        let close = session.close();
        if crate::render_session::validate_close_report(&close, request.smart).is_err() {
            return Ok(incomplete(
                &samples,
                &identity,
                "session_close_failed",
                Some(session_index),
                None,
            ));
        }
        let Some(worker_sha) = close
            .pointer("/final_report/worker_admitted_plugin_sha256")
            .and_then(Value::as_str)
            .filter(|sha| sha.len() == 64 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        else {
            return Ok(incomplete(
                &samples,
                &identity,
                "worker_admission_identity_unavailable",
                Some(session_index),
                None,
            ));
        };
        worker_admitted_hashes.push(worker_sha.to_ascii_lowercase());
        identity["worker_admitted_plugin_sha256_per_session"] = json!(worker_admitted_hashes);
        if !worker_sha.eq_ignore_ascii_case(&plugin_sha256_before) {
            return Ok(incomplete(
                &samples,
                &identity,
                "worker_admission_identity_mismatch",
                Some(session_index),
                None,
            ));
        }
    }
    let plugin_sha256_after = digest_file(request.plugin_path)?;
    if plugin_sha256_before != plugin_sha256_after
        || host_sha256 != digest_file(&host_path)?
        || worker_sha256 != digest_file(&worker_path)?
    {
        return Ok(incomplete(
            &samples,
            &identity,
            "identity_changed_during_run",
            None,
            None,
        ));
    }
    let mut report = summarize(&samples);
    identity["plugin_sha256_before_and_after"] = json!(plugin_sha256_before);
    identity["plugin_sha256_at_end"] = json!(plugin_sha256_after);
    report["identity"] = identity;
    report["samples"] = json!(samples);
    report["frames_per_resolution"] = json!(request.frames_per_resolution);
    report["session_open_ns"] = json!(session_open_ns);
    Ok(report)
}

#[cfg(windows)]
pub struct RunRequest<'a> {
    pub repository: &'a std::path::Path,
    pub plugin_path: &'a std::path::Path,
    pub plugin_sha256: &'a str,
    pub parameters: &'a [crate::image_render::InteractiveParameter],
    pub smart: bool,
    pub pixel_format: crate::image_render::RenderPixelFormat,
    pub resolutions: &'a [(u32, u32)],
    pub frames_per_resolution: u32,
    pub timeout_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn family(pixel_cost: u64, fixed_cost: u64, quadratic: bool, leak_bytes: u64) -> Vec<Sample> {
        let mut samples = Vec::new();
        for (session, edge) in [64u32, 128, 256].into_iter().enumerate() {
            let pixels = edge as u64 * edge as u64;
            for frame in 0..4 {
                let dispatch_ns =
                    fixed_cost + pixel_cost * if quadratic { pixels * pixels } else { pixels };
                samples.push(Sample {
                    width: edge,
                    height: edge,
                    session_index: session as u32,
                    frame_index: frame,
                    cold: frame == 0,
                    session_open_ns: (frame == 0).then_some(500_000),
                    wall_ns: dispatch_ns + 1_000_000,
                    input_write_ns: Some(100_000),
                    worker_setup_ns: Some(200_000),
                    worker_render_ns: Some(dispatch_ns + 100_000),
                    selector_dispatch_ns: Some(dispatch_ns),
                    worker_finalize_ns: Some(50_000),
                    output_verify_ns: Some(50_000),
                    output_bytes: Some(pixels * 4),
                    worker_live_commit_bytes: Some(8_000_000 + frame as u64 * leak_bytes),
                    worker_peak_commit_bytes: Some(9_000_000 + frame as u64 * leak_bytes),
                    worker_job_peak_commit_bytes: Some(9_500_000 + frame as u64 * leak_bytes),
                });
            }
        }
        samples
    }

    #[test]
    fn detects_superlinear_dispatch_without_labelling_linear_or_fixed_cost_as_bug() {
        let linear = summarize(&family(100, 0, false, 0));
        let fixed = summarize(&family(100, 10_000_000, false, 0));
        let quadratic = summarize(&family(1, 0, true, 0));
        assert!(
            !linear["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "superlinear_candidate")
        );
        assert!(
            !fixed["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "superlinear_candidate")
        );
        assert!(
            quadratic["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "superlinear_candidate")
        );
        assert_eq!(quadratic["resolutions"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn superlinear_span_survives_one_noisy_middle_resolution() {
        let mut samples = family(1, 0, true, 0);
        for sample in &mut samples {
            if sample.width == 128 {
                sample.selector_dispatch_ns = Some(1_500_000_000);
            }
        }
        let report = summarize(&samples);
        let adjacent = report["adjacent_ratios"].as_array().unwrap();
        assert_eq!(
            adjacent
                .iter()
                .filter(|step| step["normalized_dispatch_ratio"].as_f64().unwrap() >= 1.5)
                .count(),
            1
        );
        assert!(
            report["high_end_normalized_dispatch_ratio"]
                .as_f64()
                .unwrap()
                >= 2.25
        );
        assert!(
            report["reasons"]
                .as_array()
                .unwrap()
                .contains(&json!("superlinear_candidate"))
        );
    }

    #[test]
    fn live_memory_growth_is_distinct_from_peak_and_missing_phase_is_not_zero() {
        let stable = summarize(&family(100, 0, false, 0));
        let leaking = summarize(&family(100, 0, false, 256_000));
        assert!(
            !stable["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "memory_growth_candidate")
        );
        assert!(
            leaking["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "memory_growth_candidate")
        );
        let mut missing = family(100, 0, false, 0);
        for sample in &mut missing {
            sample.selector_dispatch_ns = None;
            sample.worker_peak_commit_bytes = None;
            sample.worker_job_peak_commit_bytes = None;
        }
        let report = summarize(&missing);
        assert_eq!(
            report["resolutions"][0]["warm_selector_dispatch"]["status"],
            "unavailable"
        );
        assert!(report["resolutions"][0]["warm_selector_dispatch"]["median_ns"].is_null());
        assert_eq!(
            report["resolutions"][0]["worker_peak_commit_status"],
            "unavailable"
        );
        assert_eq!(
            report["resolutions"][0]["worker_peak_commit_unavailable_reason"],
            "process_query_failed"
        );
    }

    #[test]
    fn declining_and_nonmonotone_live_commit_keep_their_signed_slope() {
        let mut declining = family(100, 0, false, 0);
        for sample in &mut declining {
            sample.worker_live_commit_bytes = Some(8_000_000 - sample.frame_index as u64 * 256_000);
        }
        let report = summarize(&declining);
        assert_eq!(
            report["resolutions"][0]["worker_live_commit_slope_bytes_per_frame"],
            -256_000.0
        );
        assert_eq!(
            report["resolutions"][0]["worker_live_commit_monotone_non_decreasing"],
            false
        );
        assert!(
            !report["reasons"]
                .as_array()
                .unwrap()
                .contains(&json!("memory_growth_candidate"))
        );

        let mut nonmonotone = family(100, 0, false, 0);
        for sample in &mut nonmonotone {
            sample.worker_live_commit_bytes =
                Some(8_000_000 + [0, 512_000, 256_000, 768_000][sample.frame_index as usize]);
        }
        let report = summarize(&nonmonotone);
        assert_eq!(
            report["resolutions"][0]["worker_live_commit_slope_bytes_per_frame"],
            256_000.0
        );
        assert_eq!(
            report["resolutions"][0]["worker_live_commit_monotone_non_decreasing"],
            false
        );
        assert!(
            !report["reasons"]
                .as_array()
                .unwrap()
                .contains(&json!("memory_growth_candidate"))
        );
    }

    #[test]
    fn one_noisy_resolution_does_not_imply_a_persistent_memory_growth_family() {
        let mut samples = family(100, 0, false, 0);
        for sample in &mut samples {
            if sample.width == 64 {
                sample.worker_live_commit_bytes =
                    Some(8_000_000 + u64::from(sample.frame_index) * 256_000);
            }
        }
        let report = summarize(&samples);
        assert!(
            !report["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason == "memory_growth_candidate")
        );
    }
}
