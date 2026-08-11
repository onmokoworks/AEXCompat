//! Sweep: discover every AEX the AviUtl2 registration would register, render
//! one frame of each without AviUtl2, and collect why the ones that failed
//! failed (issue #957).
//!
//! Successor to `layer_render_diag.rs` and `discover_sweep.rs`, both deleted by
//! #870 as collateral of the sealed-staging removal. #704's table (AE 2026's
//! 224 Effects, 41 of them answering error 512) came from the former, and until
//! this example there was no way to re-measure it after a fix.
//!
//! Enumeration and discovery are the shipping code — `scan_for_diagnostics`
//! resolves the same folders, ignore list and dependency folders
//! `RegisterPlugin` does, and `discover_records_for_diagnostics` runs the same
//! `discover_all`. A sweep that reimplemented either would measure the
//! reimplementation. Neither reads nor writes the discovery cache file, so a
//! sweep cannot demote what a running AviUtl2 depends on.
//!
//! Run:
//!   set AEXCOMPAT_MULTIFILTER_REPOSITORY=C:\path\to\AEXCompat
//!   cargo run --release --example render_sweep -- --json sweep.json
//!
//! With no folder argument the configured/default scan folders are swept, which
//! is the set AviUtl2 would see. Naming folders replaces those and nothing else.
//!
//! Options:
//!   --json <path>        write the report here at the end, and one record per
//!                        line to <path>.partial.jsonl while the sweep runs, so
//!                        a killed sweep still leaves what it had
//!   --discovery-only     stop after shipping discovery. With --json, completed
//!                        discovery tasks are appended to the partial sidecar,
//!                        so a long or interrupted pass retains exact progress
//!   --limit <n>          sweep at most n plug-ins
//!   --skip <n>           start at the n-th, to sweep the corpus in slices.
//!                        Each run reports exactly what it swept and overwrites
//!                        whatever is at its own --json, so give each slice its
//!                        own path; merging them is not something this does
//!   --filter <substr>    only plug-ins whose file name contains it (no case)
//!   --depth 8|16|32      session bit depth (a plug-in can fail at one and not
//!                        another: #777 access-violated at 16 while answering 4
//!                        at 8 and 32)
//!   --size <W>x<H>       session dimensions
//!   --time <n>           timeline position to render at, not the frame index.
//!                        Every SmartFX plug-in rendered at 0 and failed
//!                        elsewhere, which a fixed-0 sweep could not see (#828)
//!   --no-layer           do not supply a secondary layer to plug-ins that take
//!                        one, as the control for "is this about the layer"
//!   --force-classic      dispatch PF_Cmd_RENDER even where SmartFX is advertised
//!   --frames <n>         render n frames into one session instead of one. The
//!                        bridge renders continuously, so an effect whose frame
//!                        0 fails and whose frame 1 renders is a working effect
//!                        there and a failing one to a single-frame sweep
//!   --plugin-defaults    send no parameters at all, leaving the plug-in on the
//!                        values its own PARAMS_SETUP installed. The control for
//!                        "is this about the host's parameter transport"
//!   --close-report       carry each session's whole close report, not just the
//!                        pruned failure fields. For drilling into one bucket;
//!                        too large to hold for a whole sweep, and not
//!                        shareable - under AEXCOMPAT_EXTENDED_DIAG the close
//!                        report carries the worker's raw stderr, which is
//!                        unbounded in shape and can hold absolute paths
//!   --include-scan-paths record absolute folder paths in the report. Off by
//!                        default: the report is meant to be shareable, and
//!                        private absolute paths are not (EVIDENCE_POLICY §)
//!
//! `AEXCOMPAT_EXTENDED_DIAG=1` additionally turns on the worker's host-callback
//! trace on stderr, which is worth having on a re-run of one bucket, not on a
//! whole sweep.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use aexcompat_aviutl2_multifilter::{
    DiagnosticDiscovery, DiagnosticScan, PluginName,
    discover_records_for_diagnostics_with_progress, layer_slots_of, pf_error_name, plugin_name,
    scan_for_diagnostics, smart_render_route_supported,
};
use aexcompat_broker::image_render::{RenderGpuBackend, RenderPixelFormat};
use aexcompat_broker::render_session::{
    FrameOutcome, FrameStatus, RenderSession, SessionLayer, SessionOpenRequest,
    validate_abandoned_smart_heap_corruption_close, validate_abandoned_smart_untouched_close,
};
use serde_json::{Map, Value, json};

/// Per-frame deadline. The sweep's own bound on a plug-in that never answers;
/// discovery above it has none, by policy — a wrong verdict there is worse than
/// a slow one (#354) — but a frame the caller is waiting on is exactly where a
/// deadline belongs.
const FRAME_DEADLINE: Duration = Duration::from_secs(60);

/// The session's timing: one tick of a 30-per-second scale, over a ten-second
/// span. `--frames` advances the timeline by one step per frame.
const TIME_STEP: i32 = 1;
const TIME_SCALE: u32 = 30;
const TOTAL_TIME: i32 = 300;

#[derive(Clone)]
struct Options {
    json: Option<PathBuf>,
    limit: Option<usize>,
    skip: usize,
    filter: Option<String>,
    pixel_format: RenderPixelFormat,
    width: u32,
    height: u32,
    current_time: i32,
    no_layer: bool,
    force_classic: bool,
    include_scan_paths: bool,
    close_report: bool,
    plugin_defaults: bool,
    frames: u32,
    discovery_only: bool,
    dirs: Vec<PathBuf>,
}

fn parse_options() -> Options {
    let mut options = Options {
        json: None,
        limit: None,
        skip: 0,
        filter: None,
        pixel_format: RenderPixelFormat::Argb8,
        width: 256,
        height: 144,
        current_time: 0,
        no_layer: false,
        force_classic: false,
        include_scan_paths: false,
        close_report: false,
        plugin_defaults: false,
        frames: 1,
        discovery_only: false,
        dirs: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        let mut value = || {
            args.next()
                .unwrap_or_else(|| panic!("{argument} takes a value"))
        };
        match argument.as_str() {
            "--json" => options.json = Some(PathBuf::from(value())),
            "--limit" => options.limit = Some(value().parse().expect("--limit takes a count")),
            "--skip" => options.skip = value().parse().expect("--skip takes a count"),
            "--filter" => options.filter = Some(value().to_lowercase()),
            "--depth" => {
                options.pixel_format = match value().as_str() {
                    "8" => RenderPixelFormat::Argb8,
                    "16" => RenderPixelFormat::Argb16,
                    "32" => RenderPixelFormat::Argb32f,
                    other => panic!("--depth takes 8, 16 or 32, not {other}"),
                }
            }
            "--size" => {
                let size = value();
                let (width, height) = size.split_once('x').expect("--size takes <W>x<H>");
                options.width = width.trim().parse().expect("--size width");
                options.height = height.trim().parse().expect("--size height");
            }
            "--time" => {
                options.current_time = value().parse().expect("--time takes a position");
                // Outside the session's span the broker refuses every frame as a
                // caller error, which the sweep would otherwise bucket as a
                // plug-in failure for the whole corpus.
                assert!(
                    (0..=TOTAL_TIME).contains(&options.current_time),
                    "--time is outside the session's total time (0..={TOTAL_TIME})",
                );
            }
            "--no-layer" => options.no_layer = true,
            "--force-classic" => options.force_classic = true,
            "--include-scan-paths" => options.include_scan_paths = true,
            "--close-report" => options.close_report = true,
            "--plugin-defaults" => options.plugin_defaults = true,
            "--frames" => {
                options.frames = value().parse().expect("--frames takes a count");
                assert!(options.frames >= 1, "--frames takes at least 1");
            }
            "--discovery-only" => options.discovery_only = true,
            other if other.starts_with("--") => panic!("unknown option {other}"),
            other => options.dirs.push(PathBuf::from(other)),
        }
    }
    // `--frames` advances the timeline one step per frame, so the last frame's
    // position has to fit too. Past it the broker refuses the frame as a caller
    // error, which the sweep would bucket as a plug-in failure for the whole
    // corpus. Checked after parsing because either option may come first, and
    // in checked arithmetic because a count large enough to wrap would
    // otherwise compute its way past the check it is here to fail.
    let span = u64::from(options.frames - 1) * TIME_STEP.unsigned_abs() as u64;
    let last = u64::from(options.current_time.unsigned_abs()) + span;
    assert!(
        last <= TOTAL_TIME.unsigned_abs() as u64,
        "--time plus --frames runs to {last}, past the session's total time of {TOTAL_TIME}",
    );
    options
}

fn main() {
    let options = parse_options();
    let repository = PathBuf::from(
        std::env::var_os("AEXCOMPAT_MULTIFILTER_REPOSITORY")
            .expect("set AEXCOMPAT_MULTIFILTER_REPOSITORY to the repository root"),
    );

    let scan = scan_for_diagnostics((!options.dirs.is_empty()).then(|| options.dirs.clone()));
    // Not exhaustive means the denominator is short by an unknown amount, which
    // is the one thing a sweep's headline number must not hide (#660).
    if let Some(reason) = &scan.incomplete_reason {
        eprintln!("warning: the scan was not exhaustive: {reason}");
    }
    let mut matched: Vec<PathBuf> = scan
        .plugins
        .iter()
        .filter(|path| match &options.filter {
            Some(filter) => path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.to_lowercase().contains(filter)),
            None => true,
        })
        .cloned()
        .collect();
    let selected = matched.len();
    let targets: Vec<PathBuf> = matched
        .drain(options.skip.min(selected)..)
        .take(options.limit.unwrap_or(usize::MAX))
        .collect();
    eprintln!(
        "scan: {} folder(s), {} .aex seen, {} after ignore, {} matched, {} swept",
        scan.dirs.len(),
        scan.seen,
        scan.plugins.len(),
        selected,
        targets.len(),
    );
    if targets.is_empty() {
        eprintln!("nothing to sweep");
        return;
    }

    let started = Instant::now();
    eprintln!("discovering {} plug-in(s)...", targets.len());
    let discovery_started = Instant::now();
    let partial = options
        .discovery_only
        .then(|| options.json.as_ref().map(|path| partial_path(path)))
        .flatten();
    if let Some(path) = &partial {
        let _ = std::fs::remove_file(path);
    }
    let partial = Mutex::new(partial);
    let completed = AtomicUsize::new(0);
    let mut records = discover_records_for_diagnostics_with_progress(
        &repository,
        &targets,
        scan.dependency_dirs.clone(),
        |batch| {
            record_discovery_progress(&batch, &scan, &partial, &completed, targets.len());
        },
    );
    records.sort_by(|left, right| left.path.cmp(&right.path));
    let discovery_elapsed = discovery_started.elapsed();
    eprintln!(
        "discovery: {} of {} inspected in {:.1}s",
        records.iter().filter(|record| record.ok).count(),
        records.len(),
        discovery_elapsed.as_secs_f64(),
    );

    if options.discovery_only {
        let report = discovery_only_report(
            &options,
            &scan,
            &records,
            discovery_elapsed,
            started.elapsed(),
        );
        finish_report(&options, &report);
        return;
    }

    // Built once: they depend only on the session geometry, and rebuilding them
    // per plug-in would memcpy the same few hundred KB a few hundred times.
    let input: Vec<u8> = (0..options.width * options.height)
        .flat_map(|_| [32u8, 64, 128, 255])
        .collect();
    // A map with structure, so an effect that samples it cannot answer
    // identically for every pixel by accident.
    let layer_pixels: Vec<u8> = (0..options.width * options.height)
        .flat_map(|index| {
            let x = (index % options.width) as u8;
            let y = (index / options.width) as u8;
            [x, y, x ^ y, 255]
        })
        .collect();

    // Appended, not rewritten: the report has to survive a killed sweep, and
    // rewriting the whole thing after every plug-in makes that cost quadratic
    // (a 300-plug-in run with --close-report writes over a gigabyte to leave a
    // nine-megabyte file). One line per plug-in costs what the plug-in's own
    // record costs, and the aggregated report is written once at the end.
    // One run, one record file. A slice never reads or merges another slice's
    // output: a merge has to reconcile which plug-ins each covered, which
    // render settings each measured under, and which folder list each resolved
    // its indices against, and getting any of that wrong turns a narrower
    // answer into what reads as a whole-corpus one. Slices go to separate
    // `--json` paths and are compared by whoever asked for them.
    let mut sidecar = options.json.as_ref().map(|path| {
        let path = partial_path(path);
        let _ = std::fs::remove_file(&path);
        (path, Vec::<u8>::new())
    });

    let mut plugins: Vec<Value> = Vec::with_capacity(records.len());
    let mut buckets: BTreeMap<String, usize> = BTreeMap::new();
    for (index, record) in records.iter().enumerate() {
        let name = plugin_name(&record.path, &scan.dirs);
        let plugin_started = Instant::now();
        // Third-party AEX in-process code paths (the PE read, the parameter
        // translation) can panic; one plug-in must not end the sweep.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sweep_one(&repository, record, &options, &input, &layer_pixels)
        }))
        .unwrap_or_else(|_| Outcome {
            bucket: "sweep_panicked".to_owned(),
            detail: Map::new(),
        });
        let elapsed_ms = plugin_started.elapsed().as_millis();
        // Numbered from the corpus, not from this slice: the number an operator
        // reads off the log is the one they would pass back as `--skip`.
        eprintln!(
            "[{}/{}] {}	{}	{elapsed_ms}ms",
            options.skip + index + 1,
            options.skip + records.len(),
            name.relative,
            outcome.bucket,
        );
        *buckets.entry(outcome.bucket.clone()).or_default() += 1;
        let record = plugin_record(record, &name, outcome, elapsed_ms);
        if let Some((path, line)) = &mut sidecar {
            line.clear();
            if serde_json::to_writer(&mut *line, &record).is_ok() {
                line.push(b'\n');
                append_line(path, line);
            }
        }
        plugins.push(record);
    }

    let report = report(
        &options,
        &scan,
        discovery_elapsed,
        started.elapsed(),
        buckets,
        plugins,
    );
    finish_report(&options, &report);
}

fn record_discovery_progress(
    batch: &[DiagnosticDiscovery],
    scan: &DiagnosticScan,
    partial: &Mutex<Option<PathBuf>>,
    completed: &AtomicUsize,
    total: usize,
) {
    let done = completed.fetch_add(batch.len(), Ordering::Relaxed) + batch.len();
    eprintln!("discovery progress: {done}/{total}");
    let Ok(partial) = partial.lock() else {
        return;
    };
    let Some(path) = partial.as_ref() else {
        return;
    };
    for record in batch {
        let name = plugin_name(&record.path, &scan.dirs);
        let value = plugin_record(record, &name, discovery_outcome(record), 0);
        if let Ok(mut line) = serde_json::to_vec(&value) {
            line.push(b'\n');
            append_line(path, &line);
        }
    }
}

fn discovery_only_report(
    options: &Options,
    scan: &DiagnosticScan,
    records: &[DiagnosticDiscovery],
    discovery_elapsed: Duration,
    elapsed: Duration,
) -> Value {
    let mut buckets = BTreeMap::new();
    let plugins = records
        .iter()
        .map(|record| {
            let name = plugin_name(&record.path, &scan.dirs);
            let outcome = discovery_outcome(record);
            *buckets.entry(outcome.bucket.clone()).or_default() += 1;
            plugin_record(record, &name, outcome, 0)
        })
        .collect();
    report(options, scan, discovery_elapsed, elapsed, buckets, plugins)
}

fn finish_report(options: &Options, report: &Value) {
    if let Some(path) = &options.json {
        if write_report(path, report) {
            let _ = std::fs::remove_file(partial_path(path));
            eprintln!("wrote {}", path.display());
        } else {
            eprintln!(
                "the per-plug-in records are in {}",
                partial_path(path).display()
            );
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&report["buckets"]).unwrap_or_default()
    );
    eprintln!(
        "total={} elapsed={:.1}s",
        report["plugins"].as_array().map_or(0, Vec::len),
        report["elapsed_ms"].as_u64().unwrap_or_default() as f64 / 1000.0
    );
}

fn discovery_outcome(record: &DiagnosticDiscovery) -> Outcome {
    Outcome::bare(if record.ok {
        "discovery_ok"
    } else {
        return Outcome::bare(&discovery_failure_bucket(
            record.failure_diagnostics.as_ref(),
            record.failure_classification.as_deref(),
        ));
    })
}

/// What one plug-in's sweep concluded: the bucket it is counted under and the
/// evidence behind that bucket. The evidence's shape is per bucket - an open
/// error, the rendered extent, the frame's error and the plug-in's own message
/// - so it is a map rather than a type per bucket.
struct Outcome {
    bucket: String,
    detail: Map<String, Value>,
}

impl Outcome {
    fn bare(bucket: &str) -> Self {
        Outcome {
            bucket: bucket.to_owned(),
            detail: Map::new(),
        }
    }
}

/// The secondary layer handed to a plug-in that declares one: every layer slot
/// carries the same structured map unless the run is the no-layer control.
/// Supplying only the first slot makes a multi-layer effect observe a mixture
/// of a real layer and an empty checkout, which is not a useful render probe.
fn probe_layers(
    record: &DiagnosticDiscovery,
    options: &Options,
    pixels: &[u8],
) -> Vec<SessionLayer> {
    if options.no_layer {
        return Vec::new();
    }
    layer_slots_of(&record.parameters)
        .into_iter()
        .map(|slot| SessionLayer {
            slot,
            width: options.width,
            height: options.height,
            rgba: pixels.to_vec(),
            timed: None,
            // The multifilter opens the virtual-buffer layer dynamic so the map
            // can follow a moving scene (#674); a static layer takes a different
            // transport, so a defect can live on one and not the other. The
            // sweep takes the shipping shape.
            dynamic: true,
        })
        .collect()
}

fn sweep_one(
    repository: &Path,
    record: &DiagnosticDiscovery,
    options: &Options,
    input: &[u8],
    layer_pixels: &[u8],
) -> Outcome {
    if !record.ok {
        let mut outcome = Outcome::bare(&format!(
            "not_discovered:{}",
            discovery_failure_bucket(
                record.failure_diagnostics.as_ref(),
                record.failure_classification.as_deref(),
            )
        ));
        if let Some(diagnostics) = &record.failure_diagnostics {
            outcome
                .detail
                .insert("discovery_diagnostics".to_owned(), diagnostics.clone());
        }
        return outcome;
    }
    if record.plugin_kind == aexcompat_aviutl2_multifilter::DiscoveredPluginKind::Aegp {
        return Outcome::bare("discovered:aegp");
    }
    if record.search_roots.is_empty() {
        return Outcome::bare("no_search_roots");
    }

    let smart =
        smart_render_route_supported(record.smart, record.out_flags2) && !options.force_classic;
    let layers = probe_layers(record, options, layer_pixels);
    // The sweep has no host edits, so it has no parameter assignments. This
    // drives the same changed-only contract as an untouched bridge object;
    // `--plugin-defaults` remains a compatible explicit spelling of it.
    let parameters = None;

    let session = RenderSession::open(SessionOpenRequest {
        repository,
        plugin_path: &record.path,
        plugin_sha256: &record.sha256,
        parameters,
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
        dependencies: Vec::new(),
        dependency_search_dirs: record.search_roots.clone(),
        width: options.width,
        height: options.height,
        pixel_format: options.pixel_format,
        time_step: TIME_STEP,
        total_time: TOTAL_TIME,
        time_scale: TIME_SCALE,
        frame_deadline: FRAME_DEADLINE,
        smart,
        gpu_backend: RenderGpuBackend::Auto,
        gpu_runtime_policy: None,
        payload_override: None,
        launch_environment: Default::default(),
    });
    let mut session = match session {
        Ok(session) => session,
        Err(error) => {
            let mut outcome = Outcome::bare("session_open_failed");
            outcome
                .detail
                .insert("session_open_error".to_owned(), json!(error.to_string()));
            return outcome;
        }
    };

    // More than one frame because a session is not a frame: the bridge renders
    // frames continuously into a live session, so an effect whose first frame
    // fails and whose second renders looks like a working effect there and like
    // a failing one to a sweep that only ever asks for frame 0.
    let mut frames: Vec<Outcome> = Vec::with_capacity(options.frames as usize);
    for frame_index in 0..options.frames {
        let frame = frame_outcome(session.render_frame_with_parameters(
            frame_index,
            options.current_time + (frame_index as i32) * TIME_STEP,
            input,
            parameters,
        ));
        // A refused frame invalidates the session, so there is no next frame to
        // ask for; anything else leaves it usable.
        let ended = frame.bucket == "render_frame_failed";
        frames.push(frame);
        if ended {
            break;
        }
    }
    // The sweep's verdict is the first frame that produced pixels, and frame 0's
    // outcome when none did. Reporting the last frame instead would let a
    // session that rendered and then broke read as a plain failure, and
    // reporting frame 0 alone is the case this loop exists to stop hiding.
    let verdict = frames
        .iter()
        .position(|frame| frame.bucket == "rendered")
        .unwrap_or(0);
    let per_frame: Option<Value> = (options.frames > 1).then(|| {
        Value::Array(
            frames
                .iter()
                .enumerate()
                .map(|(index, frame)| json!({ "frame_index": index, "bucket": frame.bucket }))
                .collect(),
        )
    });
    let mut outcome = frames.swap_remove(verdict);
    if let Some(per_frame) = per_frame {
        outcome.detail.insert("frames".to_owned(), per_frame);
    }

    let close = session.close();
    let close_clean = close.get("session_clean") == Some(&Value::Bool(true))
        && close.get("invalidated") == Some(&Value::Bool(false));
    let smart_output_untouched = smart
        && outcome
            .detail
            .get("host_failure_reason")
            .and_then(Value::as_str)
            == Some("smart_output_untouched");
    // A frame the session refused is not one failure but several, and which one
    // decides who is at fault: `worker_exited` is the plug-in taking the process
    // down, `worker_invariant_failure` is the host refusing what came back, and
    // the framing reasons are the transport. The reason is on the close report,
    // so the bucket is only final once the session is closed.
    if outcome.bucket == "render_frame_failed"
        && let Some(reason) = close
            .pointer("/invalidated_reason/reason")
            .and_then(Value::as_str)
    {
        outcome.bucket = format!("render_frame_failed:{reason}");
    }
    let fallback_reason = smart_fallback_reason(smart, smart_output_untouched, &close);
    let fallback_validation = fallback_reason.map(|reason| match reason {
        "smart_output_untouched" => validate_abandoned_smart_untouched_close(&close),
        "smart_worker_heap_corruption" => validate_abandoned_smart_heap_corruption_close(&close),
        _ => unreachable!("fallback reason is locally constructed"),
    });
    let fallback_authorized = fallback_validation.as_ref().is_some_and(Result::is_ok);
    let smart_attempt_evidence = fallback_validation.as_ref().map(|validation| {
        json!({
            "close_validated": validation.is_ok(),
            "rejection": validation.as_ref().err(),
            "frames_ok": close.get("frames_ok"),
            "frames_errored": close.get("frames_errored"),
            "smart_output_untouched_frames": close.get("smart_output_untouched_frames"),
            "worker_classification": close.pointer("/worker/classification"),
            "invalidated": close.get("invalidated")
        })
    });
    attach_close(&mut outcome, close, options.close_report);
    if fallback_reason.is_some() && !fallback_authorized {
        outcome.detail.insert(
            "fallback_rejected".to_owned(),
            json!("smart_attempt_validation"),
        );
    }
    if let Some(fallback) = orchestrate_sweep_classic_fallback(
        options,
        fallback_reason,
        fallback_authorized,
        smart_attempt_evidence,
        |classic_options| sweep_one(repository, record, classic_options, input, layer_pixels),
    ) {
        return fallback;
    }
    if outcome.bucket == "rendered" && !close_clean {
        outcome.bucket = "session_close_failed".to_owned();
    }
    outcome
}

fn orchestrate_sweep_classic_fallback<F>(
    options: &Options,
    fallback_reason: Option<&'static str>,
    fallback_authorized: bool,
    smart_attempt_evidence: Option<Value>,
    launch_classic_once: F,
) -> Option<Outcome>
where
    F: FnOnce(&Options) -> Outcome,
{
    if !fallback_authorized {
        return None;
    }
    let reason = fallback_reason.expect("authorized fallback has a reason");
    let mut classic_options = options.clone();
    classic_options.force_classic = true;
    // Re-run the complete requested frame slice in one fresh Classic session.
    // This covers both a frame that never replied and a worker that exited
    // after earlier frame pixels were observed: no pixels from the unvalidated
    // Smart session become the sweep verdict.
    let mut fallback = launch_classic_once(&classic_options);
    fallback
        .detail
        .insert("fallback_reason".to_owned(), json!(reason));
    if let Some(evidence) = smart_attempt_evidence {
        fallback.detail.insert("smart_attempt".to_owned(), evidence);
    }
    fallback
        .detail
        .insert("render_path".to_owned(), json!("classic_fallback"));
    Some(fallback)
}

fn smart_fallback_reason(
    smart: bool,
    smart_output_untouched: bool,
    close: &Value,
) -> Option<&'static str> {
    if smart_output_untouched {
        return Some("smart_output_untouched");
    }
    (smart && validate_abandoned_smart_heap_corruption_close(close).is_ok())
        .then_some("smart_worker_heap_corruption")
}

fn discovery_failure_bucket(diagnostics: Option<&Value>, classification: Option<&str>) -> String {
    if classification == Some("module_audit_failure") {
        return "module_audit_failure".to_owned();
    }
    let exit_code = diagnostics
        .and_then(|value| value.get("exit_code"))
        .and_then(Value::as_i64);
    match exit_code {
        Some(11) => "exit_11_load_library".to_owned(),
        Some(14) => "exit_14_module_audit".to_owned(),
        Some(12) => diagnostics
            .and_then(|value| value.get("plugin_kind"))
            .and_then(Value::as_str)
            .map(|kind| format!("exit_12_{kind}"))
            .unwrap_or_else(|| "exit_12".to_owned()),
        Some(code) => format!("exit_{code}"),
        None => classification.unwrap_or("unknown").to_owned(),
    }
}

/// Folds the session's close report into a plug-in's evidence: which stage the
/// worker was in, which suites it could not acquire, and which implemented-suite
/// slots it fell through. That is the material a fix is planned from, and the
/// discovery cache throws all of it away in favour of one string.
fn attach_close(outcome: &mut Outcome, close: Value, whole_report: bool) {
    let diagnostics = close.pointer("/worker/diagnostics");
    let mut worker = Map::new();
    for (key, value) in [
        ("classification", close.pointer("/worker/classification")),
        ("exit_code", close.pointer("/worker/exit_code")),
    ] {
        worker.insert(key.to_owned(), value.cloned().unwrap_or(Value::Null));
    }
    for key in [
        "failure_stage",
        "active_stage",
        "missing_suites",
        "unsupported_suite_calls",
        "callback_history",
        // Present only with AEXCOMPAT_EXTENDED_DIAG; this keeps one-off host
        // callback evidence available in the sweep artifact without exposing
        // it during ordinary corpus runs.
        "stderr_tail",
        "load_failure",
        "plugin_kind",
        // What the worker refused to hand out, parsed from its own stderr.
        // `missing_suites` beside it comes from the worker's final report and
        // is empty on this path - the close never propagates it, and a session
        // that ends on a frame error often has no parsable report at all - so
        // this is the field that actually answers "which suite was missing".
        "suite_acquire_failures",
        // Both lists are bounded, and a capped list read as a complete one
        // turns "the sweep did not look further" into "there was nothing
        // further" - the flags have to travel with the lists they qualify.
        "suite_acquire_failures_truncated",
        // Which `get_callback_addr` ids the worker refused, same source. For a
        // `frame_error:516` this is often the whole diagnosis (issue #985).
        "callback_addr_denials",
        "callback_addr_denials_truncated",
        // Host-callback refusals with the refusing condition (issue #995):
        // the rest of the 516 bucket's diagnosis.
        "callback_denials",
        "callback_denials_truncated",
    ] {
        worker.insert(
            key.to_owned(),
            diagnostics
                .and_then(|value| value.get(key))
                .cloned()
                .unwrap_or(Value::Null),
        );
    }
    outcome.detail.insert("worker".to_owned(), worker.into());
    outcome
        .detail
        .insert("session_clean".to_owned(), close["session_clean"].clone());
    outcome.detail.insert(
        "invalidated_reason".to_owned(),
        close["invalidated_reason"].clone(),
    );
    // The pruned worker block above is what a whole sweep can carry; the full
    // close report is what one bucket's drill-down needs, and it is large enough
    // that a 500-plug-in sweep must not hold it by default.
    if whole_report {
        outcome.detail.insert("close_report".to_owned(), close);
    }
}

/// One frame's bucket and evidence. A session renders several and each is
/// classified the same way.
fn frame_outcome(outcome: std::io::Result<FrameOutcome>) -> Outcome {
    let mut detail = Map::new();
    let bucket = match outcome {
        Ok(outcome) => match outcome.status {
            FrameStatus::Rendered {
                pixels,
                width,
                height,
                origin_x,
                origin_y,
            } => {
                detail.insert("pixel_bytes".to_owned(), json!(pixels.len()));
                detail.insert("width".to_owned(), json!(width));
                detail.insert("height".to_owned(), json!(height));
                detail.insert("origin_x".to_owned(), json!(origin_x));
                detail.insert("origin_y".to_owned(), json!(origin_y));
                // A rendered frame of no pixels is not a render: an effect that
                // answers 0x0 has produced nothing, and rounding it into
                // `rendered` is how a sweep reports progress it did not make.
                if pixels.is_empty() || width == 0 || height == 0 {
                    "rendered_empty".to_owned()
                } else {
                    "rendered".to_owned()
                }
            }
            FrameStatus::FrameError {
                render_error,
                missing_dependency,
                return_message,
            } => {
                let name = pf_error_name(render_error);
                detail.insert("render_error".to_owned(), json!(render_error));
                detail.insert("render_error_name".to_owned(), json!(name));
                detail.insert("missing_dependency".to_owned(), json!(missing_dependency));
                // What the plug-in itself said while failing (#707): often the
                // whole diagnosis, and what #704 went looking for.
                detail.insert(
                    "return_message".to_owned(),
                    json!(return_message.map(|message| json!({
                        "selector": message.selector,
                        "text": message.text,
                        "error": message.error,
                        "display_requested": message.display_requested,
                    }))),
                );
                match name {
                    Some(name) => format!("frame_error:{render_error}:{name}"),
                    None => format!("frame_error:{render_error}"),
                }
            }
            FrameStatus::SmartOutputUntouched => {
                detail.insert("render_error".to_owned(), json!(-6));
                detail.insert(
                    "host_failure_reason".to_owned(),
                    json!("smart_output_untouched"),
                );
                "frame_error:-6".to_owned()
            }
        },
        Err(error) => {
            detail.insert("render_frame_error".to_owned(), json!(error.to_string()));
            "render_frame_failed".to_owned()
        }
    };
    Outcome { bucket, detail }
}

fn plugin_record(
    record: &DiagnosticDiscovery,
    name: &PluginName,
    outcome: Outcome,
    elapsed_ms: u128,
) -> Value {
    json!({
        // Two spellings under the names the deleted discover_sweep used, because
        // one committed reader wants each: `tools/aex_plugindata_probe.py`
        // matches a sweep row to a file by basename, and
        // `tools/aex_sweep_checkpoint.py` joins on the relative path. Emitting
        // only one would break its reader silently rather than loudly, since a
        // basename match just stops finding anything.
        //
        // The names are all they share. The checkpoint's own `classify_failure`
        // is keyed on discover_sweep's buckets (`loaded`, `exit_12_*`,
        // `exit_20`, `module_audit*`), which this emits none of, and its
        // `render_status` is hard-coded to "not_attempted_discovery_only" - so
        // it joins these rows and then classifies every one of them as unknown.
        // Reconciling the two vocabularies is issue #960, not something this
        // claims to have done.
        "plugin": name.basename,
        "plugin_relative_path": name.relative,
        // Which scan folder the relative path is under. Two folders can each
        // hold an `Effects/Foo.aex`, and the disambiguation is a field rather
        // than a prefix so a reader joining the path against a folder still
        // has a path to join.
        "scan_folder": name.root,
        "plugin_sha256": record.sha256,
        "plugin_size_bytes": record.byte_size,
        "category": record.category,
        "discovery": {
            "ok": record.ok,
            "plugin_kind": record.plugin_kind,
            "smart": record.smart,
            "out_flags2": record.out_flags2,
            "smart_route_supported":
                smart_render_route_supported(record.smart, record.out_flags2),
            "parameter_count": record.parameters.len(),
            "layer_slots": layer_slots_of(&record.parameters),
            "failure_classification": record.failure_classification,
            "failure_diagnostics": record.failure_diagnostics,
            "cluster_fallback": record.cluster_fallback,
        },
        "bucket": outcome.bucket,
        "detail": outcome.detail,
        "elapsed_ms": elapsed_ms,
    })
}

fn report(
    options: &Options,
    scan: &DiagnosticScan,
    discovery_elapsed: Duration,
    elapsed: Duration,
    buckets: BTreeMap<String, usize>,
    plugins: Vec<Value>,
) -> Value {
    let render = (!options.discovery_only).then(|| {
        json!({
            "width": options.width,
            "height": options.height,
            "pixel_format": options.pixel_format.report_name(),
            "current_time": options.current_time,
            "frames": options.frames,
            "secondary_layer": !options.no_layer,
            "force_classic": options.force_classic,
            "plugin_defaults": options.plugin_defaults,
            "frame_deadline_ms": FRAME_DEADLINE.as_millis(),
            "time_step": TIME_STEP,
            "total_time": TOTAL_TIME,
            "time_scale": TIME_SCALE,
        })
    });
    json!({
        "schema_version": 1,
        "scan": {
            "folder_count": scan.dirs.len(),
            "seen": scan.seen,
            "after_ignore": scan.plugins.len(),
            "swept": plugins.len(),
            "incomplete_reason": scan.incomplete_reason,
            // What narrowed this run. Without them a filtered sweep and a whole
            // one are the same document, and the smaller denominator is
            // invisible to anything comparing two reports.
            "selection": {
                "filter": options.filter,
                "limit": options.limit,
                "skip": options.skip,
            },
            // Absolute folder paths are opt-in: the rest of this report is
            // shareable and they are not (docs/EVIDENCE_POLICY_2026-07-18.md).
            "folders": options.include_scan_paths.then(|| {
                scan.dirs
                    .iter()
                    .map(|root| root.to_string_lossy().into_owned())
                    .collect::<Vec<String>>()
            }),
        },
        "mode": if options.discovery_only { "discovery_only" } else { "render" },
        "render": render,
        "discovery_elapsed_ms": discovery_elapsed.as_millis(),
        "elapsed_ms": elapsed.as_millis(),
        "buckets": buckets,
        "plugins": plugins,
    })
}

/// Where the per-plug-in lines go while the sweep is still running.
fn partial_path(report: &Path) -> PathBuf {
    report.with_extension("partial.jsonl")
}

fn append_line(path: &Path, line: &[u8]) {
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(mut file) => {
            if let Err(error) = file.write_all(line) {
                eprintln!("warning: {} could not be appended: {error}", path.display());
            }
        }
        Err(error) => eprintln!("warning: {} could not be opened: {error}", path.display()),
    }
}

/// Writes through a temporary file, so a sweep killed mid-write leaves the
/// previous report rather than a truncated one.
fn write_report(path: &Path, report: &Value) -> bool {
    let temporary = path.with_extension("json.tmp");
    let Ok(serialized) = serde_json::to_vec_pretty(report) else {
        eprintln!("warning: the report could not be serialized");
        return false;
    };
    if let Err(error) = std::fs::write(&temporary, &serialized) {
        eprintln!(
            "warning: {} could not be written: {error}",
            temporary.display()
        );
        return false;
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        eprintln!("warning: {} could not be replaced: {error}", path.display());
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use aexcompat_aviutl2_multifilter::DiscoveredPluginKind;

    fn discovery_options(json: PathBuf) -> Options {
        Options {
            json: Some(json),
            limit: None,
            skip: 0,
            filter: None,
            pixel_format: RenderPixelFormat::Argb8,
            width: 1,
            height: 1,
            current_time: 0,
            no_layer: false,
            force_classic: false,
            include_scan_paths: false,
            close_report: false,
            plugin_defaults: false,
            frames: 1,
            discovery_only: true,
            dirs: Vec::new(),
        }
    }

    fn failed_discovery(path: PathBuf) -> DiagnosticDiscovery {
        DiagnosticDiscovery {
            path,
            ok: false,
            plugin_kind: DiscoveredPluginKind::Effect,
            sha256: "00".repeat(32),
            byte_size: 123,
            smart: false,
            out_flags2: 0,
            category: None,
            parameters: Vec::new(),
            search_roots: Vec::new(),
            failure_classification: Some("nonzero_exit".to_owned()),
            failure_diagnostics: Some(json!({
                "classification": "nonzero_exit",
                "exit_code": 11,
            })),
            cluster_fallback: Some("worker_exited/invalidated".to_owned()),
        }
    }

    #[test]
    fn rendered_bucket_requires_and_reports_pixel_bytes() {
        let rendered = frame_outcome(Ok(FrameOutcome {
            frame_index: 0,
            status: FrameStatus::Rendered {
                pixels: vec![0; 16],
                width: 2,
                height: 2,
                origin_x: 0,
                origin_y: 0,
            },
        }));
        assert_eq!(rendered.bucket, "rendered");
        assert_eq!(rendered.detail["pixel_bytes"], 16);

        let empty = frame_outcome(Ok(FrameOutcome {
            frame_index: 0,
            status: FrameStatus::Rendered {
                pixels: Vec::new(),
                width: 2,
                height: 2,
                origin_x: 0,
                origin_y: 0,
            },
        }));
        assert_eq!(empty.bucket, "rendered_empty");
        assert_eq!(empty.detail["pixel_bytes"], 0);
    }

    #[test]
    fn discovery_buckets_preserve_exit_12_plugin_kind() {
        let diagnostics = json!({
            "classification": "nonzero_exit",
            "exit_code": 12,
            "plugin_kind": "aegp_candidate"
        });
        assert_eq!(
            discovery_failure_bucket(Some(&diagnostics), Some("nonzero_exit")),
            "exit_12_aegp_candidate"
        );
    }

    #[test]
    fn discovery_buckets_name_load_library_failure() {
        let diagnostics = json!({"classification": "nonzero_exit", "exit_code": 11});
        assert_eq!(
            discovery_failure_bucket(Some(&diagnostics), Some("nonzero_exit")),
            "exit_11_load_library"
        );
    }

    #[test]
    fn sweep_retries_only_the_exact_smart_heap_corruption_boundary() {
        let close = json!({
            "render_path": "smart",
            "session_clean": false,
            "invalidated": true,
            "invalidated_reason": { "reason": "worker_exited" },
            "frames_ok": 0,
            "frames_errored": 0,
            "final_report": null,
            "worker": {
                "classification": "crashed",
                "exit_code": 0xC000_0374u64,
                "diagnostics": { "active_stage": "smart_render_cpu" }
            }
        });
        assert_eq!(
            smart_fallback_reason(true, false, &close),
            Some("smart_worker_heap_corruption")
        );
        assert_eq!(
            smart_fallback_reason(false, false, &close),
            None,
            "Classic failures never recursively retry"
        );
        let mut prior_frames = close.clone();
        *prior_frames.pointer_mut("/frames_ok").unwrap() = json!(3);
        assert_eq!(
            smart_fallback_reason(true, false, &prior_frames),
            Some("smart_worker_heap_corruption"),
            "a close-time exit after prior rendered frames still retries the whole slice"
        );

        let mut wrong_exit = close;
        *wrong_exit.pointer_mut("/worker/exit_code").unwrap() = json!(0xC000_0005u64);
        assert_eq!(smart_fallback_reason(true, false, &wrong_exit), None);
    }

    #[test]
    fn sweep_replays_the_complete_slice_once_and_returns_only_classic_pixels() {
        let mut options = discovery_options(PathBuf::from("unused.json"));
        options.discovery_only = false;
        options.frames = 3;
        options.current_time = 7;
        options.force_classic = false;
        let launches = std::cell::Cell::new(0);
        let smart_evidence = json!({
            "close_validated": true,
            "frames_ok": 2,
            "frames_errored": 0
        });

        let outcome = orchestrate_sweep_classic_fallback(
            &options,
            Some("smart_worker_heap_corruption"),
            true,
            Some(smart_evidence.clone()),
            |classic_options| {
                launches.set(launches.get() + 1);
                assert!(classic_options.force_classic);
                assert_eq!(classic_options.frames, 3);
                assert_eq!(classic_options.current_time, 7);
                let mut outcome = Outcome::bare("rendered");
                outcome.detail.insert("pixel_bytes".into(), json!(16));
                outcome
            },
        )
        .expect("authorized Smart close launches Classic");

        assert_eq!(launches.get(), 1, "the complete slice launches once");
        assert_eq!(outcome.bucket, "rendered");
        assert_eq!(outcome.detail["pixel_bytes"], 16);
        assert_eq!(outcome.detail["render_path"], "classic_fallback");
        assert_eq!(
            outcome.detail["fallback_reason"],
            "smart_worker_heap_corruption"
        );
        assert_eq!(outcome.detail["smart_attempt"], smart_evidence);

        let rejected_launches = std::cell::Cell::new(0);
        assert!(
            orchestrate_sweep_classic_fallback(
                &options,
                Some("smart_worker_heap_corruption"),
                false,
                None,
                |_| {
                    rejected_launches.set(rejected_launches.get() + 1);
                    Outcome::bare("rendered")
                },
            )
            .is_none()
        );
        assert_eq!(rejected_launches.get(), 0);
    }

    #[test]
    fn discovery_only_report_replaces_partial_only_after_final_write() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-render-sweep-{}-{nonce:032x}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let plugin = root.join("failed.aex");
        let records = vec![failed_discovery(plugin.clone())];
        let scan = DiagnosticScan {
            dirs: vec![root.clone()],
            plugins: vec![plugin],
            seen: 1,
            dependency_dirs: Vec::new(),
            incomplete_reason: None,
        };
        let report_path = root.join("discovery.json");
        let options = discovery_options(report_path.clone());
        let partial = partial_path(&report_path);
        let completed = AtomicUsize::new(0);
        record_discovery_progress(
            &records,
            &scan,
            &Mutex::new(Some(partial.clone())),
            &completed,
            1,
        );
        assert_eq!(completed.load(Ordering::Relaxed), 1);
        assert!(
            partial.is_file(),
            "completed failure survives in the sidecar"
        );
        let partial_bytes = std::fs::read(&partial).unwrap();
        let partial_row: Value = serde_json::from_slice(&partial_bytes).unwrap();
        assert_eq!(partial_row["discovery"]["ok"], false);
        assert_eq!(partial_row["bucket"], "exit_11_load_library");

        let report = discovery_only_report(
            &options,
            &scan,
            &records,
            Duration::from_millis(4),
            Duration::from_millis(5),
        );
        finish_report(&options, &report);
        assert!(report_path.is_file());
        assert!(
            !partial.exists(),
            "a successful final report supersedes the sidecar"
        );
        let final_report: Value =
            serde_json::from_slice(&std::fs::read(&report_path).unwrap()).unwrap();
        assert_eq!(final_report["mode"], "discovery_only");
        assert!(final_report["render"].is_null());
        assert_eq!(final_report["buckets"]["exit_11_load_library"], 1);
        assert_eq!(final_report["plugins"][0]["discovery"]["ok"], false);

        let blocked_path = root.join("blocked.json");
        std::fs::create_dir(&blocked_path).unwrap();
        let blocked_partial = partial_path(&blocked_path);
        std::fs::write(&blocked_partial, &partial_bytes).unwrap();
        let blocked_options = discovery_options(blocked_path);
        finish_report(&blocked_options, &report);
        assert!(
            blocked_partial.is_file(),
            "a failed final replacement must retain crash evidence"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
