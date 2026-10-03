//! Bounded, advisory worker-commit observations. Never a render verdict.

use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeSet, VecDeque};

pub const WINDOW_LIMIT: usize = 32;
const WARMUP: usize = 4;
const TAIL: usize = 8;
const GROWTH_PER_FRAME: f64 = 131_072.0;
const EARLY_SAMPLES: usize = 4;
const EARLY_MIN_STEP: u64 = 4 * 1024 * 1024;
const EARLY_BUDGET_DIVISOR: u64 = 64;

#[derive(Clone, Debug, Serialize)]
pub struct MemorySample {
    pub frame_index: u32,
    pub current_time: i32,
    pub width: u32,
    pub height: u32,
    pub live_commit_bytes: Option<u64>,
    pub process_peak_commit_bytes: Option<u64>,
    pub job_peak_commit_bytes: Option<u64>,
    pub process_limit_bytes: Option<u64>,
}

#[derive(Default)]
pub struct MemoryMonitor {
    samples: VecDeque<MemorySample>,
    count: u64,
    baseline: Vec<u64>,
    active: u8,
    notification_count: u64,
}

fn median(values: &[u64]) -> u64 {
    let mut sorted = [0; TAIL];
    sorted[..values.len()].copy_from_slice(values);
    sorted[..values.len()].sort_unstable();
    // Observation windows have an even length; use the lower midpoint. No
    // samples are discarded, and this definition is published in the report.
    sorted[(values.len() - 1) / 2]
}

const NEAR_LIMIT: u8 = 1;
const GROWTH: u8 = 2;
const RETENTION: u8 = 4;
const EARLY_HEADROOM: u8 = 8;

struct EarlyGrowth {
    sample_count: usize,
    horizon: usize,
    minimum_step: Option<u64>,
    projected_frames: Option<f64>,
    candidate: bool,
    unavailable_reason: Option<&'static str>,
}

struct Assessment {
    warnings: u8,
    live: Option<u64>,
    limit: Option<u64>,
    ratio: Option<f64>,
    baseline: Option<u64>,
    slope: Option<f64>,
    retained: Option<i128>,
    trend: &'static str,
    unavailable_reason: Option<&'static str>,
    tail_range: Option<u64>,
    tail_count: usize,
    recovered_peak: bool,
    early: EarlyGrowth,
}

impl MemoryMonitor {
    /// Called only after a validated frame reply. State changes are returned
    /// for notification; stable warning states do not produce repeated alerts.
    pub fn observe(&mut self, sample: MemorySample) -> bool {
        self.count = self.count.saturating_add(1);
        if self.count > WARMUP as u64 && self.count <= (WARMUP + 4) as u64 {
            if let Some(live) = sample.live_commit_bytes {
                self.baseline.push(live);
            }
        }
        if self.samples.len() == WINDOW_LIMIT {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
        let next = self.assess().warnings;
        let changed = next != self.active;
        if changed {
            self.notification_count = self.notification_count.saturating_add(1);
            self.active = next;
        }
        changed
    }

    fn early_growth(&self) -> EarlyGrowth {
        let mut result = EarlyGrowth {
            sample_count: self.samples.len().min(EARLY_SAMPLES),
            horizon: 0,
            minimum_step: None,
            projected_frames: None,
            candidate: false,
            unavailable_reason: Some("insufficient_compatible_samples"),
        };
        if self.count >= (WARMUP + TAIL) as u64 {
            result.unavailable_reason = Some("regular_trend_window_reached");
            return result;
        }
        result.horizon = WARMUP + TAIL - self.count as usize;
        if result.sample_count < EARLY_SAMPLES {
            return result;
        }
        let latest = self.samples.back().unwrap();
        if latest.width == 0 || latest.height == 0 {
            result.unavailable_reason = Some("invalid_geometry");
            return result;
        }
        let (Some(live), Some(limit)) = (latest.live_commit_bytes, latest.process_limit_bytes)
        else {
            result.unavailable_reason = Some("missing_live_or_process_budget");
            return result;
        };
        if limit == 0 || live >= limit {
            result.unavailable_reason = Some("no_positive_headroom");
            return result;
        }
        let minimum_growth = EARLY_MIN_STEP.max(limit / EARLY_BUDGET_DIVISOR);
        let mut previous: Option<&MemorySample> = None;
        let mut minimum_step = u64::MAX;
        for observation in self.samples.iter().skip(self.samples.len() - EARLY_SAMPLES) {
            if observation.live_commit_bytes.is_none()
                || observation.process_limit_bytes != Some(limit)
                || (observation.width, observation.height) != (latest.width, latest.height)
            {
                result.unavailable_reason = Some("missing_or_incompatible_sample");
                return result;
            }
            if let Some(previous) = previous {
                if previous.frame_index.checked_add(1) != Some(observation.frame_index) {
                    result.unavailable_reason = Some("nonconsecutive_frame_samples");
                    return result;
                }
                let step = observation
                    .live_commit_bytes
                    .unwrap()
                    .checked_sub(previous.live_commit_bytes.unwrap());
                if !step.is_some_and(|step| step >= minimum_growth) {
                    result.unavailable_reason = Some("insufficient_repeated_rapid_growth");
                    return result;
                }
                minimum_step = minimum_step.min(step.unwrap());
            }
            previous = Some(observation);
        }
        let remaining = limit - live;
        result.minimum_step = Some(minimum_step);
        result.projected_frames = Some(remaining as f64 / minimum_step as f64);
        // Use the slowest of all three increments and integer comparison. This
        // is conditional headroom, not a prediction that growth will continue.
        result.candidate = remaining as u128 <= minimum_step as u128 * result.horizon as u128;
        result.unavailable_reason = None;
        result
    }

    // Classification is fixed-size stack work: no JSON, strings, sorting
    // buffers or geometry sets are allocated on the ordinary frame hot path.
    fn assess(&self) -> Assessment {
        let latest = self.samples.back();
        let live = latest.and_then(|s| s.live_commit_bytes);
        let limit = latest
            .and_then(|s| s.process_limit_bytes)
            .filter(|n| *n > 0);
        let ratio = live.zip(limit).map(|(n, d)| n as f64 / d as f64);
        let mut warnings = 0;
        if ratio.is_some_and(|r| r >= 0.8) {
            warnings |= NEAR_LIMIT;
        }
        let early = self.early_growth();
        if early.candidate {
            warnings |= EARLY_HEADROOM;
        }
        let mut values = [0; TAIL];
        let mut tail_count = 0;
        let mut all_live = true;
        let mut peak = None;
        for sample in self.samples.iter().rev().take(TAIL).rev() {
            if let Some(live) = sample.live_commit_bytes {
                values[tail_count] = live;
            } else {
                all_live = false;
            }
            peak = peak.max(sample.process_peak_commit_bytes);
            tail_count += 1;
        }
        let baseline = (self.baseline.len() == 4).then(|| median(&self.baseline));
        let enough = self.count >= (WARMUP + TAIL) as u64 && tail_count == TAIL;
        let mut recovered_peak = false;
        let mut slope = None;
        let mut retained = None;
        let mut trend = "unavailable";
        let mut trend_reason = if enough {
            "missing_live_sample"
        } else {
            "insufficient_post_warmup_samples"
        };
        let mut tail_range = None;
        if all_live && enough {
            let low = *values.iter().min().unwrap();
            let high = *values.iter().max().unwrap();
            let middle = median(&values);
            let tolerance = 262_144u64.max(middle / 50);
            let delta = values[TAIL - 1] as i128 - values[0] as i128;
            let signed_slope = delta as f64 / (TAIL - 1) as f64;
            slope = Some(signed_slope);
            tail_range = Some(high - low);
            retained = baseline.map(|base| middle as i128 - base as i128);
            let nondecreasing = values.windows(2).filter(|p| p[1] >= p[0]).count();
            let halves_delta = median(&values[4..]) as i128 - median(&values[..4]) as i128;
            if signed_slope >= GROWTH_PER_FRAME && nondecreasing >= 6 && halves_delta >= 524_288 {
                trend = "sustained_growth_candidate";
                warnings |= GROWTH;
            } else if high - low <= tolerance {
                trend = "stable_plateau";
                if baseline
                    .zip(retained)
                    .is_some_and(|(base, retained)| retained >= 1_048_576u64.max(base / 10) as i128)
                {
                    warnings |= RETENTION;
                }
            } else if signed_slope <= -GROWTH_PER_FRAME {
                trend = "declining_commit";
            } else {
                trend = "variable_commit";
            }
            trend_reason = "";
            if peak.is_some_and(|peak| peak.saturating_sub(high) >= tolerance) {
                // Peak is historical. This observation is not a live-pressure
                // warning and cannot establish sustained growth or a leak.
                recovered_peak = true;
            }
        }
        Assessment {
            warnings,
            live,
            limit,
            ratio,
            baseline,
            slope,
            retained,
            trend,
            unavailable_reason: if trend_reason.is_empty() {
                None
            } else {
                Some(trend_reason)
            },
            tail_range,
            tail_count,
            recovered_peak,
            early,
        }
    }

    pub fn report(&self) -> Value {
        let a = self.assess();
        let latest = self.samples.back();
        let warnings: Vec<_> = [
            (NEAR_LIMIT, "process_limit_near"),
            (GROWTH, "sustained_live_growth_candidate"),
            (RETENTION, "post_frame_retention_candidate"),
            (EARLY_HEADROOM, "rapid_live_growth_headroom_candidate"),
        ]
        .into_iter()
        .filter_map(|(bit, reason)| (a.warnings & bit != 0).then_some(reason))
        .collect();
        let mut observations = Vec::new();
        if a.trend == "stable_plateau" {
            observations.push("stable_plateau");
        }
        if a.recovered_peak {
            observations.push("temporary_peak_or_recovered_commit");
        }
        let resolutions: BTreeSet<_> = self.samples.iter().map(|s| (s.width, s.height)).collect();
        let policy = json!({"warmup_samples": WARMUP, "tail_samples": TAIL,
            "minimum_samples": WARMUP + TAIL, "growth_min_bytes_per_frame": GROWTH_PER_FRAME,
            "growth_min_nondecreasing_steps": 6, "growth_min_half_median_delta_bytes": 524_288,
            "near_limit_ratio": 0.8, "plateau_range_min_bytes": 262_144,
            "plateau_range_fraction": 0.02, "retention_min_bytes": 1_048_576,
            "retention_baseline_fraction": 0.1, "median": "lower_midpoint", "outliers_removed": 0,
            "early_growth_samples": EARLY_SAMPLES, "early_growth_min_step_bytes": EARLY_MIN_STEP,
            "early_growth_min_step_budget_divisor": EARLY_BUDGET_DIVISOR,
            "early_growth_horizon": "samples_remaining_until_regular_trend_minimum",
            "early_growth_projection": "minimum_of_all_three_positive_increments"});
        json!({
            "advisory": true,
            "scope": "resident_worker_session",
            "observation_phase": "after_validated_frame_reply",
            "attribution": "host_plugin_allocator_or_runtime_not_disambiguated",
            "caveat": "Candidate only: host frame caches, plug-in state, allocator caches and runtime initialization can retain commit. A frame reply is not a full-session release checkpoint. No leak diagnosis or render verdict change.",
            "sample_count": self.count,
            "window_sample_count": self.samples.len(),
            "window_limit": WINDOW_LIMIT,
            "session_count": 1,
            "resolution_count": resolutions.len(),
            "trend": a.trend,
            "trend_status": if a.trend == "unavailable" { "unavailable" } else { "available" },
            "trend_unavailable_reason": a.unavailable_reason,
            "early_growth": {
                "provisional": true,
                "status": if a.early.minimum_step.is_some() { "available" } else { "unavailable" },
                "unavailable_reason": a.early.unavailable_reason,
                "candidate": a.early.candidate,
                "sample_count": a.early.sample_count,
                "regular_trend_samples_remaining": a.early.horizon,
                "minimum_observed_growth_bytes_per_frame": a.early.minimum_step,
                "projected_frames_to_limit": a.early.projected_frames,
                "caveat": "Conditional on the recent growth continuing; initialization and bounded caches may plateau. No leak diagnosis or future failure verdict."
            },
            "tail_sample_count": a.tail_count,
            "live_slope_bytes_per_frame": a.slope,
            "tail_live_range_bytes": a.tail_range,
            "baseline_after_warmup_bytes": a.baseline,
            "retained_commit_delta_bytes": a.retained.map(|n| n as f64),
            "live_commit_bytes": a.live,
            "process_limit_bytes": a.limit,
            "limit_scope": "job_object_per_process_limit_not_aggregate_job_peak",
            "live_limit_ratio": a.ratio,
            "limit_ratio_status": if a.ratio.is_some() { "available" } else { "unavailable" },
            "limit_ratio_unavailable_reason": if a.ratio.is_some() { None } else { Some("missing_live_or_process_budget") },
            "process_peak_commit_bytes": latest.and_then(|s| s.process_peak_commit_bytes),
            "job_peak_commit_bytes": latest.and_then(|s| s.job_peak_commit_bytes),
            "warnings": warnings,
            "observations": observations,
            "warning_state_change_count": self.notification_count,
            "policy": policy,
            "samples": self.samples,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const M: u64 = 1024 * 1024;
    fn sample(live: Option<u64>, peak: Option<u64>) -> MemorySample {
        MemorySample {
            frame_index: 0,
            current_time: 0,
            width: 1920,
            height: 1080,
            live_commit_bytes: live,
            process_peak_commit_bytes: peak,
            job_peak_commit_bytes: peak,
            process_limit_bytes: Some(1024 * M),
        }
    }
    fn series(values: &[u64]) -> MemoryMonitor {
        let mut monitor = MemoryMonitor::default();
        for (i, n) in values.iter().enumerate() {
            let mut s = sample(Some(*n), Some(*values[..=i].iter().max().unwrap()));
            s.frame_index = i as u32;
            monitor.observe(s);
        }
        monitor
    }
    #[test]
    fn short_rapid_growth_warns_before_the_regular_trend_window() {
        let mut monitor = MemoryMonitor::default();
        for (i, live) in [218_685_440, 418_160_640, 617_709_568, 817_242_112]
            .into_iter()
            .enumerate()
        {
            let mut observation = sample(Some(live), Some(live));
            observation.process_limit_bytes = Some(2048 * M);
            observation.frame_index = i as u32;
            let changed = monitor.observe(observation);
            assert_eq!(changed, i == 3);
        }
        let report = monitor.report();
        assert_eq!(report["trend_status"], "unavailable");
        assert_eq!(
            report["warnings"],
            json!(["rapid_live_growth_headroom_candidate"])
        );
        assert_eq!(report["early_growth"]["provisional"], true);
        assert_eq!(report["early_growth"]["sample_count"], 4);
        assert!(
            report["early_growth"]["projected_frames_to_limit"]
                .as_f64()
                .unwrap()
                < 8.0
        );
        assert_eq!(report["live_commit_bytes"], 817_242_112u64);
        assert_eq!(report["sample_count"], 4);
    }
    fn early_series(values: &[u64]) -> MemoryMonitor {
        let mut monitor = MemoryMonitor::default();
        for (i, live) in values.iter().enumerate() {
            let mut observation = sample(Some(*live), Some(*live));
            observation.frame_index = i as u32;
            monitor.observe(observation);
        }
        monitor
    }
    #[test]
    fn early_growth_is_provisional_and_clears_on_a_cache_plateau() {
        let mut monitor = early_series(&[32 * M, 132 * M, 232 * M, 332 * M]);
        assert_eq!(
            monitor.report()["warnings"],
            json!(["rapid_live_growth_headroom_candidate"])
        );
        let mut plateau = sample(Some(332 * M), Some(332 * M));
        plateau.frame_index = 4;
        assert!(monitor.observe(plateau.clone()));
        assert_eq!(monitor.report()["warnings"], json!([]));
        for i in 5..32 {
            plateau.frame_index = i;
            assert!(!monitor.observe(plateau.clone()));
        }
        let report = monitor.report();
        assert_eq!(report["trend"], "stable_plateau");
        assert_eq!(report["warning_state_change_count"], 2);
    }
    #[test]
    fn early_growth_ignores_cold_spikes_noise_slow_growth_and_distant_headroom() {
        for values in [
            [32 * M, 332 * M, 332 * M, 332 * M],
            [32 * M, 332 * M, 232 * M, 432 * M],
            [32 * M, 332 * M, 342 * M, 442 * M],
            [32 * M, 33 * M, 34 * M, 35 * M],
            [32 * M, 52 * M, 72 * M, 92 * M],
        ] {
            assert_eq!(
                early_series(&values).report()["warnings"],
                json!([]),
                "{values:?}"
            );
        }
        let mut monitor = early_series(&[32 * M, 132 * M, 232 * M, 332 * M]);
        let mut decline = sample(Some(232 * M), Some(332 * M));
        decline.frame_index = 4;
        assert!(monitor.observe(decline));
        assert_eq!(monitor.report()["warnings"], json!([]));
    }
    #[test]
    fn early_growth_requires_compatible_live_budget_geometry_and_frame_samples() {
        for variant in 0..9 {
            let mut monitor = MemoryMonitor::default();
            for i in 0..4 {
                let mut observation = sample(Some((32 + i * 100) * M), Some(332 * M));
                observation.frame_index = i as u32;
                if i == 1 {
                    match variant {
                        0 => observation.live_commit_bytes = None,
                        1 => observation.process_limit_bytes = None,
                        2 => observation.process_limit_bytes = Some(0),
                        3 => observation.process_limit_bytes = Some(2048 * M),
                        4 => observation.width = 640,
                        5 => observation.height = 360,
                        6 => observation.frame_index = 7,
                        7 => observation.width = 0,
                        8 => observation.height = 0,
                        _ => unreachable!(),
                    }
                }
                assert!(!monitor.observe(observation));
            }
            let report = monitor.report();
            assert_eq!(report["warnings"], json!([]));
            assert_eq!(report["early_growth"]["status"], "unavailable");
            assert!(report["early_growth"]["projected_frames_to_limit"].is_null());
        }
        for limit in [None, Some(0)] {
            let mut monitor = MemoryMonitor::default();
            for i in 0..4 {
                let mut observation = sample(Some((32 + i * 100) * M), Some(332 * M));
                observation.frame_index = i as u32;
                observation.process_limit_bytes = limit;
                assert!(!monitor.observe(observation));
            }
            assert!(monitor.report()["early_growth"]["projected_frames_to_limit"].is_null());
        }
    }
    #[test]
    fn early_growth_state_is_not_spammed_and_transfers_to_regular_trend() {
        let mut monitor = MemoryMonitor::default();
        for i in 0..12 {
            let live = (20 + i * 180) * M;
            let mut observation = sample(Some(live), Some(live));
            observation.process_limit_bytes = Some(2000 * M);
            observation.frame_index = i as u32;
            let changed = monitor.observe(observation);
            assert_eq!(changed, i == 3 || i == 9 || i == 11);
        }
        let report = monitor.report();
        assert_eq!(
            report["warnings"],
            json!(["process_limit_near", "sustained_live_growth_candidate"])
        );
        assert_eq!(report["early_growth"]["status"], "unavailable");
        assert_eq!(
            report["early_growth"]["unavailable_reason"],
            "regular_trend_window_reached"
        );
        assert_eq!(report["warning_state_change_count"], 3);
    }
    #[test]
    fn early_warning_clears_on_missing_data_and_requires_four_fresh_samples() {
        let mut monitor = early_series(&[32 * M, 132 * M, 232 * M, 332 * M]);
        let mut missing = sample(None, Some(332 * M));
        missing.frame_index = 4;
        assert!(monitor.observe(missing));
        let report = monitor.report();
        assert_eq!(report["warnings"], json!([]));
        assert_eq!(report["early_growth"]["status"], "unavailable");
        assert!(report["early_growth"]["projected_frames_to_limit"].is_null());
        for (i, live) in [332 * M, 482 * M, 632 * M, 782 * M].into_iter().enumerate() {
            let mut observation = sample(Some(live), Some(live));
            observation.frame_index = 5 + i as u32;
            assert_eq!(monitor.observe(observation), i == 3);
        }
        assert_eq!(
            monitor.report()["warnings"],
            json!(["rapid_live_growth_headroom_candidate"])
        );
        assert_eq!(monitor.report()["warning_state_change_count"], 3);
    }
    #[test]
    fn early_growth_uses_the_smallest_step_and_an_inclusive_integer_horizon() {
        let at_boundary = early_series(&[32 * M, 132 * M, 232 * M, 332 * M]);
        // A 1,024MiB budget leaves692MiB: under8 *100MiB.
        assert_eq!(
            at_boundary.report()["early_growth"]["minimum_observed_growth_bytes_per_frame"],
            100 * M
        );
        let mut monitor = MemoryMonitor::default();
        for (i, live) in [16 * M, 56 * M, 72 * M, 96 * M].into_iter().enumerate() {
            let mut observation = sample(Some(live), Some(live));
            observation.process_limit_bytes = Some(224 * M);
            observation.frame_index = i as u32;
            monitor.observe(observation);
        }
        let report = monitor.report();
        assert_eq!(
            report["early_growth"]["minimum_observed_growth_bytes_per_frame"],
            16 * M
        );
        assert_eq!(report["early_growth"]["projected_frames_to_limit"], 8.0);
        assert_eq!(
            report["warnings"],
            json!(["rapid_live_growth_headroom_candidate"])
        );
        // One byte more headroom must not round into an inclusive boundary.
        let mut outside = MemoryMonitor::default();
        for (i, live) in [16 * M, 56 * M, 72 * M, 96 * M].into_iter().enumerate() {
            let mut observation = sample(Some(live), Some(live));
            observation.process_limit_bytes = Some(224 * M + 1);
            observation.frame_index = i as u32;
            outside.observe(observation);
        }
        assert_eq!(outside.report()["warnings"], json!([]));
    }
    #[test]
    fn short_warmup_growth_is_not_sustained_after_plateau() {
        let values: Vec<_> = (0..16).map(|i| 32 * M + i.min(3) * M).collect();
        // Exercise the legacy production summary too: two short sessions
        // label this growth, although longer observations disprove continuation.
        let mut legacy = Vec::new();
        for (session_index, width) in [640, 1920].into_iter().enumerate() {
            for frame_index in 0..4 {
                legacy.push(crate::performance_diagnostics::Sample {
                    width,
                    height: width / 2,
                    session_index: session_index as u32,
                    frame_index,
                    cold: frame_index == 0,
                    session_open_ns: Some(1),
                    wall_ns: 1,
                    input_write_ns: Some(1),
                    worker_setup_ns: Some(1),
                    worker_render_ns: Some(1),
                    selector_dispatch_ns: Some(1),
                    worker_finalize_ns: Some(1),
                    output_verify_ns: Some(1),
                    output_bytes: Some(width as u64 * width as u64 * 2),
                    worker_live_commit_bytes: Some(values[frame_index as usize]),
                    worker_peak_commit_bytes: Some(values[frame_index as usize]),
                    worker_job_peak_commit_bytes: Some(values[frame_index as usize]),
                });
            }
        }
        assert!(
            crate::performance_diagnostics::summarize(&legacy)["reasons"]
                .as_array()
                .unwrap()
                .contains(&json!("memory_growth_candidate"))
        );
        let short = series(&values[..4]).report();
        assert_eq!(short["trend_status"], "unavailable");
        let longer = series(&values).report();
        assert_eq!(longer["trend"], "stable_plateau");
        assert_eq!(longer["warnings"], json!([]));
    }
    #[test]
    fn growth_retention_recovery_and_limit_pressure_are_distinct() {
        let growth: Vec<_> = (0..32).map(|i| 32 * M + i * M).collect();
        assert_eq!(
            series(&growth).report()["warnings"],
            json!(["sustained_live_growth_candidate"])
        );
        let retention: Vec<_> = (0..32).map(|i| 32 * M + i.min(15) * M).collect();
        let retained = series(&retention).report();
        assert_eq!(retained["trend"], "stable_plateau");
        assert_eq!(
            retained["warnings"],
            json!(["post_frame_retention_candidate"])
        );
        let recovery: Vec<_> = (0..32)
            .map(|i| if i == 8 { 128 * M } else { 32 * M })
            .collect();
        let recovered = series(&recovery).report();
        assert_eq!(recovered["warnings"], json!([]));
        assert!(
            recovered["observations"]
                .as_array()
                .unwrap()
                .contains(&json!("temporary_peak_or_recovered_commit"))
        );
        let near = series(&[900 * M; 16]).report();
        assert_eq!(near["warnings"], json!(["process_limit_near"]));
        assert!(near["live_limit_ratio"].as_f64().unwrap() > 0.8);
    }
    #[test]
    fn missing_short_and_zero_budget_never_become_zero_or_plateau() {
        let mut monitor = series(&[32 * M; 16]);
        let mut missing = sample(None, Some(512 * M));
        missing.process_limit_bytes = Some(0);
        monitor.observe(missing);
        let report = monitor.report();
        assert_eq!(report["trend_status"], "unavailable");
        assert!(report["live_slope_bytes_per_frame"].is_null());
        assert!(report["live_limit_ratio"].is_null());
        assert_eq!(report["warnings"], json!([]));
        assert_eq!(
            MemoryMonitor::default().report()["trend_status"],
            "unavailable"
        );
    }
    #[test]
    fn metadata_window_is_bounded_and_warning_changes_are_not_spammed() {
        let mut monitor = MemoryMonitor::default();
        let mut changes = 0;
        for i in 0..1000 {
            changes +=
                usize::from(monitor.observe(sample(Some(32 * M + i * M), Some(32 * M + i * M))));
        }
        let report = monitor.report();
        assert_eq!(report["sample_count"], 1000);
        assert_eq!(report["window_sample_count"], WINDOW_LIMIT);
        // Sustained growth first, then near-limit pressure added. The first
        // eight observations' baseline remains bounded at four integers.
        assert_eq!(changes, 2);
    }
    #[test]
    fn signed_decline_and_historical_peak_do_not_produce_leak_warnings() {
        let values: Vec<_> = (0..32).map(|i| 128 * M - i * M).collect();
        let report = series(&values).report();
        assert_eq!(report["trend"], "declining_commit");
        assert!(report["live_slope_bytes_per_frame"].as_f64().unwrap() < 0.0);
        assert_eq!(report["warnings"], json!([]));
    }

    #[test]
    fn notification_tracks_entry_and_missing_observation_without_spam() {
        let mut monitor = MemoryMonitor::default();
        assert!(monitor.observe(sample(Some(900 * M), Some(900 * M))));
        assert!(!monitor.observe(sample(Some(900 * M), Some(900 * M))));
        assert!(monitor.observe(sample(None, Some(900 * M))));
        assert!(!monitor.observe(sample(None, Some(900 * M))));
        assert_eq!(monitor.report()["warning_state_change_count"], 2);
        assert_eq!(monitor.report()["limit_ratio_status"], "unavailable");
    }

    #[test]
    fn inclusive_growth_and_pressure_boundaries_are_not_strict() {
        let mut values: Vec<_> = (0..12).map(|i| 32 * M + i * 131_072).collect();
        values.swap(6, 7); // Exactly six nondecreasing steps in the last eight.
        let report = series(&values).report();
        assert_eq!(report["live_slope_bytes_per_frame"], 131_072.0);
        // Half medians differ by exactly 4 * 128 KiB = 512 KiB.
        assert_eq!(
            report["warnings"],
            json!(["sustained_live_growth_candidate"])
        );
        let mut five_steps = values.clone();
        five_steps.swap(9, 10);
        assert_eq!(series(&five_steps).report()["warnings"], json!([]));
        let below: Vec<_> = (0..12).map(|i| 32 * M + i * 131_071).collect();
        assert_eq!(series(&below).report()["warnings"], json!([]));
        let mut pressure = MemoryMonitor::default();
        let mut s = sample(Some(800 * M), Some(800 * M));
        s.process_limit_bytes = Some(1000 * M);
        pressure.observe(s.clone());
        assert_eq!(pressure.report()["warnings"], json!(["process_limit_near"]));
        s.live_commit_bytes = Some(800 * M - 1);
        pressure.observe(s);
        assert_eq!(pressure.report()["warnings"], json!([]));
    }

    #[test]
    fn retention_and_plateau_boundaries_are_inclusive() {
        let mut retained = vec![8 * M; 8];
        retained.extend([9 * M; 8]);
        assert_eq!(
            series(&retained).report()["warnings"],
            json!(["post_frame_retention_candidate"])
        );
        let mut below = vec![8 * M; 8];
        below.extend([9 * M - 1; 8]);
        assert_eq!(series(&below).report()["warnings"], json!([]));
        let mut range = vec![8 * M; 8];
        range.extend([
            8 * M,
            8 * M + 262_144,
            8 * M,
            8 * M,
            8 * M,
            8 * M,
            8 * M,
            8 * M,
        ]);
        assert_eq!(series(&range).report()["trend"], "stable_plateau");
        range[9] += 1;
        assert_eq!(series(&range).report()["trend"], "variable_commit");
    }
}
