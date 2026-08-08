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
use std::time::{Duration, Instant};

use aexcompat_aviutl2_multifilter::{
    DiagnosticDiscovery, DiagnosticScan, PluginName, discover_records_for_diagnostics,
    layer_slots_of, pf_error_name, plugin_name, scan_for_diagnostics,
};
use aexcompat_broker::image_render::{RenderGpuBackend, RenderPixelFormat};
use aexcompat_broker::render_session::{
    FrameOutcome, FrameStatus, RenderSession, SessionLayer, SessionOpenRequest,
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
    let mut records =
        discover_records_for_diagnostics(&repository, &targets, scan.dependency_dirs.clone());
    records.sort_by(|left, right| left.path.cmp(&right.path));
    let discovery_elapsed = discovery_started.elapsed();
    eprintln!(
        "discovery: {} of {} inspected in {:.1}s",
        records.iter().filter(|record| record.ok).count(),
        records.len(),
        discovery_elapsed.as_secs_f64(),
    );

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
    if let Some(path) = &options.json {
        if write_report(path, &report) {
            // The per-plug-in lines were the crash insurance; the whole report
            // supersedes them, so leaving them behind would age into a second,
            // stale answer beside the first. Only once it is actually on disk:
            // deleting them after a failed write throws away the run.
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
        started.elapsed().as_secs_f64()
    );
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
    if record.search_roots.is_empty() {
        return Outcome::bare("no_search_roots");
    }

    let smart = record.smart && !options.force_classic;
    let layers = probe_layers(record, options, layer_pixels);
    // The bridge overlays each frame's current config values onto the exposed
    // defaults and sends them with the frame; a sweep that instead relied on the
    // open-time baseline would drive a path the bridge never drives (a frame
    // with no `parameters` attribute takes different worker handling - issue
    // #883's UPDATE_PARAMS_UI among it). The sweep has no host config to read,
    // so it sends the discovered values unchanged.
    let parameters = (!options.plugin_defaults).then_some(&record.parameters[..]);

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
    attach_close(&mut outcome, close, options.close_report);
    outcome
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
                width,
                height,
                origin_x,
                origin_y,
                ..
            } => {
                detail.insert("width".to_owned(), json!(width));
                detail.insert("height".to_owned(), json!(height));
                detail.insert("origin_x".to_owned(), json!(origin_x));
                detail.insert("origin_y".to_owned(), json!(origin_y));
                // A rendered frame of no pixels is not a render: an effect that
                // answers 0x0 has produced nothing, and rounding it into
                // `rendered` is how a sweep reports progress it did not make.
                if width == 0 || height == 0 {
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
            "smart": record.smart,
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
        "render": {
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
        },
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
}
