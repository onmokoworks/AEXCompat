//! Sweep: discover every AEX the AviUtl2 registration would register, render
//! one frame of each without AviUtl2, and collect why the ones that failed
//! failed (issue #957).
//!
//! Successor to `layer_render_diag.rs` and `discover_sweep.rs`, both deleted by
//! #870 as collateral of the sealed-staging removal. #704's table (AE 2026's
//! 224 Effects, 41 of them answering error 512) came from the former, and until
//! this CLI there was no way to re-measure it after a fix.
//!
//! Enumeration and discovery are the shipping code — `scan_for_diagnostics`
//! resolves the same folders, ignore list and dependency folders
//! `RegisterPlugin` does, and `discover_records_for_diagnostics` runs the same
//! `discover_all`. A sweep that reimplemented either would measure the
//! reimplementation. Neither reads nor writes the discovery cache file, so a
//! sweep cannot demote what a running AviUtl2 depends on.
//!
//! Run the supported binary:
//!   aexcompat-render-sweep --json sweep.json
//!
//! A source checkout can also use:
//!   cargo run --release --bin aexcompat-render-sweep -- --json sweep.json
//!
//! With no folder argument the configured/default scan folders are swept, which
//! is the set AviUtl2 would see. Naming folders replaces those and nothing else.
//!
//! Options:
//!   --repository <root> use this worker root; unlike automatic resolution an
//!                       invalid explicit root fails instead of falling back
//!   --json <path>        write the report here at the end, and one record per
//!                        line to <path>.partial.jsonl while the sweep runs, so
//!                        a killed sweep still leaves what it had
//!   --discovery-only     stop after shipping discovery. With --json, completed
//!                        discovery tasks are appended to the partial sidecar,
//!                        so a long or interrupted pass retains exact progress
//!   --inventory-only     stop after the shipping folder scan, without loading
//!                        any AEX. Records file and path identities for every
//!                        selected candidate
//!   --blocked-path <substr>
//!                        in inventory mode, retain matching candidates but
//!                        classify them as external_blocked (repeatable, no case)
//!   --limit <n>          sweep at most n plug-ins
//!   --skip <n>           start at the n-th, to sweep the corpus in slices.
//!                        Each run reports exactly what it swept and overwrites
//!                        whatever is at its own --json, so give each slice its
//!                        own path; merging them is not something this does
//!   --render-jobs <n>    render independent dependency closures concurrently
//!                        after discovery (default 1). Members of the same
//!                        closure remain serial by default; final report order
//!                        is unchanged
//!   --same-closure-render-jobs <n>
//!                        explicitly allow up to n isolated render workers for
//!                        one resolved dependency closure (default 1). Must not
//!                        exceed --render-jobs; unresolved closures stay serial
//!   --dynamic-same-closure-groups
//!                        opt in to closure-capped work stealing between the
//!                        already-planned cluster groups. Group membership and
//!                        report order stay fixed; unresolved closures stay serial
//!   --filter <substr>    only plug-ins whose file name contains it (no case)
//!   --exclude-path <substr>
//!                        skip plug-ins whose full path contains it (repeatable,
//!                        no case); the report records every exclusion
//!   --verify-pixel-determinism
//!                        repeat rendered plug-ins in a fresh session and compare pixels
//!   --depth 8|16|32      session bit depth (a plug-in can fail at one and not
//!                        another: #777 access-violated at 16 while answering 4
//!                        at 8 and 32)
//!   --size <W>x<H>       session dimensions
//!   --input-image <path> decode a primary image; dimensions must match --size.
//!                        Default remains solid RGBA [32,64,128,255].
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
//!   --plugin-defaults    compatibility flag: both flag values send no parameter
//!                        edits, retaining PARAMS_SETUP defaults. Consult the
//!                        report's effective_input_policy for actual inputs.
//!   --close-report       carry each session's whole close report, not just the
//!                        pruned failure fields. For drilling into one bucket;
//!                        too large to hold for a whole sweep, and not
//!                        shareable - under AEXCOMPAT_EXTENDED_DIAG the close
//!                        report carries the worker's raw stderr, which is
//!                        unbounded in shape and can hold absolute paths
//!   --include-scan-paths record absolute folder paths in the report. Off by
//!                        default: the report is meant to be shareable, and
//!                        private absolute paths are not (EVIDENCE_POLICY §)
//!   --dump-frames <dir>  write every rendered frame's raw pixels to
//!                        <dir>/<plugin>.<sha8>.f<n>.<W>x<H>.<format> so "rendered"
//!                        can be checked against an AE reference (a Scribble
//!                        with no mask renders fully transparent in AE, an
//!                        Inner/Outer Key or Reshape with no mask passes the
//!                        input through; #1253). Raw image contents, so the
//!                        directory is not part of the shareable report; the
//!                        record carries only the pixel SHA-256
//!
//! `AEXCOMPAT_EXTENDED_DIAG=1` additionally turns on the worker's host-callback
//! trace on stderr, which is worth having on a re-run of one bucket, not on a
//! whole sweep.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use aexcompat_aviutl2_multifilter::{
    DiagnosticDiscovery, DiagnosticScan, PluginName,
    discover_records_for_diagnostics_with_progress, layer_slots_of, pf_error_name, plugin_name,
    scan_for_diagnostics_with_worker_root, smart_render_route_supported,
};
use aexcompat_broker::companion_manifest::{ApprovedCompanion, CompanionSuiteIdentity};
use aexcompat_broker::image_render::{RenderGpuBackend, RenderPixelFormat};
use aexcompat_broker::render_session::{
    ClusterRenderPlugins, FrameOutcome, FrameStatus, RenderSession, SessionLayer,
    SessionOpenRequest, SwapOutcome, validate_abandoned_smart_heap_corruption_close,
    validate_abandoned_smart_untouched_close,
};
use aexcompat_broker::secure_image_dispatch::{ApprovedImageArtifact, WorkerKind};
use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

/// Per-frame deadline. The sweep's own bound on a plug-in that never answers;
/// discovery above it has none, by policy — a wrong verdict there is worse than
/// a slow one (#354) — but a frame the caller is waiting on is exactly where a
/// deadline belongs.
const FRAME_DEADLINE: Duration = Duration::from_secs(60);

/// A cluster is one close-validated checkpoint: none of its frame results are
/// final until the shared session closes cleanly. Bounding it limits both the
/// amount of completed frame work awaiting that verdict and the retry cost if
/// a later member invalidates the session.
const MAX_SWEEP_CLUSTER_MEMBERS: usize = 16;

/// A known-bad member can be removed and the healthy remainder retried, but a
/// corpus full of quick explicit errors must not turn one failed optimization
/// into fifteen extra worker launches before the authoritative single-plugin
/// fallback. The first attempt plus three member-removal retries is enough to
/// salvage the common one-outlier case while keeping the overhead bounded.
const MAX_SWEEP_CLUSTER_SALVAGE_ATTEMPTS: usize = 4;

const PRIMARY_RGBA: [u8; 4] = [32, 64, 128, 255];

fn primary_pixels(width: u32, height: u32) -> Vec<u8> {
    (0..width * height).flat_map(|_| PRIMARY_RGBA).collect()
}

fn effective_input_policy(options: &Options) -> Value {
    let primary = match &options.input_rgba {
        Some(bytes) => json!({
            "pattern": "decoded_image", "channel_order": "RGBA",
            "pixel_sha256": format!("{:x}", Sha256::digest(bytes)),
            "width": options.width, "height": options.height
        }),
        None => json!({ "pattern": "solid", "channel_order": "RGBA", "rgba8": PRIMARY_RGBA }),
    };
    json!({
        "primary": primary,
        "parameter_assignments": "none",
        "secondary_layer_selection": if options.no_layer { "none" } else { "first_declared_layer_if_any" },
        "secondary_pattern": if options.no_layer { Value::Null } else { json!("rgba8_x_y_xor_opaque") },
        "semantic_coverage": "execution_probe_not_effect_correctness"
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ExecutableFingerprint {
    sha256: Option<String>,
    size_bytes: Option<u64>,
    error: Option<&'static str>,
}

impl ExecutableFingerprint {
    fn failed(error: &'static str) -> Self {
        Self {
            sha256: None,
            size_bytes: None,
            error: Some(error),
        }
    }

    fn is_complete(&self) -> bool {
        self.sha256.is_some() && self.size_bytes.is_some() && self.error.is_none()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ReportBuildFingerprint {
    schema_version: u32,
    /// Machine-readable evidence limit: this compares the executable paths at
    /// run boundaries. It is not a per-launch admission receipt and cannot
    /// detect a replace-and-restore between those observations.
    scope: &'static str,
    verification: &'static str,
    complete: bool,
    cli: ExecutableFingerprint,
    l2_worker: ExecutableFingerprint,
    classic_worker: ExecutableFingerprint,
    smart_worker: ExecutableFingerprint,
}

fn fingerprint_executable(path: &Path) -> ExecutableFingerprint {
    let Ok(mut file) = std::fs::File::open(path) else {
        return ExecutableFingerprint::failed("open_failed");
    };
    let Ok(metadata) = file.metadata() else {
        return ExecutableFingerprint::failed("metadata_failed");
    };
    if !metadata.is_file() {
        return ExecutableFingerprint::failed("not_a_file");
    }

    let expected_size = metadata.len();
    let expected_modified = metadata.modified().ok();
    let mut read_size = 0u64;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(_) => return ExecutableFingerprint::failed("read_failed"),
        };
        read_size = match read_size.checked_add(read as u64) {
            Some(total) => total,
            None => return ExecutableFingerprint::failed("size_overflow"),
        };
        hasher.update(&buffer[..read]);
    }
    let Ok(after_metadata) = file.metadata() else {
        return ExecutableFingerprint::failed("metadata_failed_after_read");
    };
    if read_size != expected_size
        || after_metadata.len() != expected_size
        || after_metadata.modified().ok() != expected_modified
    {
        return ExecutableFingerprint::failed("file_changed_during_read");
    }

    ExecutableFingerprint {
        sha256: Some(format!("{:x}", hasher.finalize())),
        size_bytes: Some(read_size),
        error: None,
    }
}

fn capture_report_build_fingerprint(
    repository: &Path,
    cli_path: Result<PathBuf, ()>,
) -> ReportBuildFingerprint {
    let cli = cli_path
        .as_deref()
        .map(fingerprint_executable)
        .unwrap_or_else(|_| ExecutableFingerprint::failed("current_exe_unavailable"));
    let worker = |kind: WorkerKind| {
        fingerprint_executable(&repository.join(kind.repository_relative_program()))
    };
    let l2_worker = worker(WorkerKind::Discovery);
    let classic_worker = worker(WorkerKind::Classic);
    let smart_worker = worker(WorkerKind::Smart);
    ReportBuildFingerprint {
        schema_version: 1,
        scope: "executable_path_boundary_snapshot_not_launch_receipt",
        verification: "pre_run_candidate",
        // A start-only observation cannot claim which bytes a later launch
        // admitted. Interrupted JSONL therefore stays explicitly incomplete.
        complete: false,
        cli,
        l2_worker,
        classic_worker,
        smart_worker,
    }
}

fn verify_executable_fingerprint(
    before: &ExecutableFingerprint,
    after: &ExecutableFingerprint,
) -> ExecutableFingerprint {
    if before == after && before.is_complete() {
        before.clone()
    } else if before == after {
        before.clone()
    } else {
        ExecutableFingerprint::failed("changed_between_boundary_snapshots")
    }
}

fn finalize_report_build_fingerprint(
    before: &ReportBuildFingerprint,
    repository: &Path,
    cli_path: Result<PathBuf, ()>,
) -> ReportBuildFingerprint {
    let after = capture_report_build_fingerprint(repository, cli_path);
    let cli = verify_executable_fingerprint(&before.cli, &after.cli);
    let l2_worker = verify_executable_fingerprint(&before.l2_worker, &after.l2_worker);
    let classic_worker =
        verify_executable_fingerprint(&before.classic_worker, &after.classic_worker);
    let smart_worker = verify_executable_fingerprint(&before.smart_worker, &after.smart_worker);
    let complete = [&cli, &l2_worker, &classic_worker, &smart_worker]
        .into_iter()
        .all(|fingerprint| fingerprint.is_complete());
    ReportBuildFingerprint {
        schema_version: 1,
        scope: "executable_path_boundary_snapshot_not_launch_receipt",
        verification: if complete {
            "run_boundary_verified"
        } else {
            "run_boundary_incomplete"
        },
        complete,
        cli,
        l2_worker,
        classic_worker,
        smart_worker,
    }
}

/// The session's timing: one tick of a 30-per-second scale, over a ten-second
/// span. `--frames` advances the timeline by one step per frame.
const TIME_STEP: i32 = 1;
const TIME_SCALE: u32 = 30;
const TOTAL_TIME: i32 = 300;
const MAX_DIMENSION: u32 = 4096;
const MAX_PIXELS: u64 = 16_777_216;

const HELP: &str = r#"AEXCompat batch render sweep

Usage:
  aexcompat-render-sweep [OPTIONS] [SCAN_FOLDER ...]

The default scan folders and dependency roots are the same ones used by the
AviUtl2 multi-filter. Positional folders replace only the scan folders.

Core options:
  --repository <root>              worker root (auto-resolved when omitted)
  --json <path>                    final JSON report and crash sidecar
  --inventory-only                 scan identities without loading AEX files
  --discovery-only                 inspect parameters without rendering
  --filter <substring>             include matching AEX basenames
  --exclude-path <substring>       exclude matching full paths (repeatable)
  --blocked-path <substring>       classify matching inventory paths as blocked
  --limit <count>                  cap selected plug-ins
  --skip <count>                   skip selected plug-ins
  --render-jobs <count>            global render worker cap (default 1)
  --same-closure-render-jobs <n>    per resolved-closure cap (default 1)
  --dynamic-same-closure-groups    closure-capped ready-queue scheduling
  --depth <8|16|32>                output depth (default 8)
  --size <width>x<height>           frame size (default 256x144)
  --time <position>                first timeline position (default 0)
  --frames <count>                 frames per session (default 1)
  --input-image <path>             primary image matching --size
  --no-layer                       omit the generated secondary layer
  --force-classic                  disable smart-render selection
  --plugin-defaults                compatibility flag; parameters stay unchanged
  --verify-pixel-determinism       repeat frames and compare decoded pixels
  --include-scan-paths             include absolute scan paths in the report
  --close-report                   include worker close diagnostics
  --dump-frames <directory>        save decoded raw frames
  --help                           show this help

--blocked-path requires --inventory-only. --inventory-only and
--discovery-only are mutually exclusive.
"#;

#[derive(Clone)]
struct Options {
    repository: Option<PathBuf>,
    input_rgba: Option<Vec<u8>>,
    json: Option<PathBuf>,
    limit: Option<usize>,
    skip: usize,
    render_jobs: usize,
    same_closure_render_jobs: usize,
    dynamic_same_closure_groups: bool,
    filter: Option<String>,
    exclude_paths: Vec<String>,
    blocked_paths: Vec<String>,
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
    inventory_only: bool,
    verify_pixel_determinism: bool,
    dump_frames: Option<PathBuf>,
    dirs: Vec<PathBuf>,
}

enum CliAction {
    Help,
    Run(Options),
}

#[derive(Debug)]
struct CliError {
    kind: &'static str,
    code: &'static str,
    message: String,
    option: Option<&'static str>,
    exit_code: u8,
}

impl CliError {
    fn usage(code: &'static str, option: Option<&'static str>, message: impl Into<String>) -> Self {
        Self {
            kind: "usage",
            code,
            message: message.into(),
            option,
            exit_code: 64,
        }
    }

    fn runtime(code: &'static str, kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            code,
            message: message.into(),
            option: None,
            exit_code: 1,
        }
    }

    fn emit(&self) {
        let fallback = "{\"schema_version\":1,\"program\":\"aexcompat-render-sweep\",\"ok\":false,\"error\":{\"kind\":\"internal\",\"code\":\"error_serialization_failed\",\"message\":\"error serialization failed\",\"option\":null}}";
        eprintln!(
            "{}",
            serde_json::to_string(&json!({
                "schema_version": 1,
                "program": "aexcompat-render-sweep",
                "ok": false,
                "error": {
                    "kind": self.kind,
                    "code": self.code,
                    "message": self.message,
                    "option": self.option,
                }
            }))
            .unwrap_or_else(|_| fallback.to_owned())
        );
    }
}

fn option_value<'a>(
    args: &'a [OsString],
    index: &mut usize,
    option: &'static str,
) -> Result<&'a OsString, CliError> {
    *index += 1;
    let value = args.get(*index).ok_or_else(|| {
        CliError::usage(
            "missing_option_value",
            Some(option),
            format!("{option} requires a value"),
        )
    })?;
    if value.to_string_lossy().starts_with("--") {
        return Err(CliError::usage(
            "missing_option_value",
            Some(option),
            format!("{option} requires a value before the next option"),
        ));
    }
    Ok(value)
}

fn utf8_option_value<'a>(value: &'a OsString, option: &'static str) -> Result<&'a str, CliError> {
    value.to_str().ok_or_else(|| {
        CliError::usage(
            "option_value_not_utf8",
            Some(option),
            format!("{option} requires a UTF-8 value"),
        )
    })
}

fn parse_count(
    value: &OsString,
    option: &'static str,
    code: &'static str,
) -> Result<usize, CliError> {
    utf8_option_value(value, option)?
        .parse()
        .map_err(|_| CliError::usage(code, Some(option), format!("{option} requires a count")))
}

fn parse_options_from(args: impl IntoIterator<Item = OsString>) -> Result<CliAction, CliError> {
    let args = args.into_iter().collect::<Vec<_>>();
    if args
        .iter()
        .any(|argument| matches!(argument.to_str(), Some("--help" | "-h")))
    {
        return Ok(CliAction::Help);
    }
    let mut input_image = None;
    let mut options = Options {
        repository: None,
        input_rgba: None,
        json: None,
        limit: None,
        skip: 0,
        render_jobs: 1,
        same_closure_render_jobs: 1,
        dynamic_same_closure_groups: false,
        filter: None,
        exclude_paths: Vec::new(),
        blocked_paths: Vec::new(),
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
        inventory_only: false,
        verify_pixel_determinism: false,
        dump_frames: None,
        dirs: Vec::new(),
    };
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        match argument.to_str() {
            Some("--repository") => {
                options.repository = Some(PathBuf::from(option_value(
                    &args,
                    &mut index,
                    "--repository",
                )?));
            }
            Some("--input-image") => {
                input_image = Some(PathBuf::from(option_value(
                    &args,
                    &mut index,
                    "--input-image",
                )?));
            }
            Some("--json") => {
                options.json = Some(PathBuf::from(option_value(&args, &mut index, "--json")?));
            }
            Some("--limit") => {
                options.limit = Some(parse_count(
                    option_value(&args, &mut index, "--limit")?,
                    "--limit",
                    "invalid_limit",
                )?);
            }
            Some("--skip") => {
                options.skip = parse_count(
                    option_value(&args, &mut index, "--skip")?,
                    "--skip",
                    "invalid_skip",
                )?;
            }
            Some("--render-jobs") => {
                options.render_jobs = parse_count(
                    option_value(&args, &mut index, "--render-jobs")?,
                    "--render-jobs",
                    "invalid_render_jobs",
                )?;
                if options.render_jobs == 0 {
                    return Err(CliError::usage(
                        "invalid_render_jobs",
                        Some("--render-jobs"),
                        "--render-jobs must be at least 1",
                    ));
                }
            }
            Some("--same-closure-render-jobs") => {
                options.same_closure_render_jobs = parse_count(
                    option_value(&args, &mut index, "--same-closure-render-jobs")?,
                    "--same-closure-render-jobs",
                    "invalid_same_closure_render_jobs",
                )?;
                if options.same_closure_render_jobs == 0 {
                    return Err(CliError::usage(
                        "invalid_same_closure_render_jobs",
                        Some("--same-closure-render-jobs"),
                        "--same-closure-render-jobs must be at least 1",
                    ));
                }
            }
            Some("--dynamic-same-closure-groups") => options.dynamic_same_closure_groups = true,
            Some("--filter") => {
                let value = option_value(&args, &mut index, "--filter")?;
                options.filter = Some(value.to_string_lossy().to_lowercase());
            }
            Some("--exclude-path") => {
                let value = option_value(&args, &mut index, "--exclude-path")?;
                options
                    .exclude_paths
                    .push(value.to_string_lossy().to_lowercase());
            }
            Some("--blocked-path") => {
                let value = option_value(&args, &mut index, "--blocked-path")?;
                options
                    .blocked_paths
                    .push(value.to_string_lossy().to_lowercase());
            }
            Some("--depth") => {
                let value =
                    utf8_option_value(option_value(&args, &mut index, "--depth")?, "--depth")?;
                options.pixel_format = match value {
                    "8" => RenderPixelFormat::Argb8,
                    "16" => RenderPixelFormat::Argb16,
                    "32" => RenderPixelFormat::Argb32f,
                    other => {
                        return Err(CliError::usage(
                            "invalid_depth",
                            Some("--depth"),
                            format!("--depth takes 8, 16 or 32, not {other}"),
                        ));
                    }
                };
            }
            Some("--size") => {
                let size = utf8_option_value(option_value(&args, &mut index, "--size")?, "--size")?;
                let (width, height) = size.split_once('x').ok_or_else(|| {
                    CliError::usage(
                        "invalid_size",
                        Some("--size"),
                        "--size takes <width>x<height>",
                    )
                })?;
                options.width = width.trim().parse().map_err(|_| {
                    CliError::usage(
                        "invalid_size",
                        Some("--size"),
                        "--size width must be an integer",
                    )
                })?;
                options.height = height.trim().parse().map_err(|_| {
                    CliError::usage(
                        "invalid_size",
                        Some("--size"),
                        "--size height must be an integer",
                    )
                })?;
            }
            Some("--time") => {
                let value =
                    utf8_option_value(option_value(&args, &mut index, "--time")?, "--time")?;
                options.current_time = value.parse().map_err(|_| {
                    CliError::usage(
                        "invalid_time",
                        Some("--time"),
                        "--time requires an integer position",
                    )
                })?;
            }
            Some("--no-layer") => options.no_layer = true,
            Some("--force-classic") => options.force_classic = true,
            Some("--include-scan-paths") => options.include_scan_paths = true,
            Some("--close-report") => options.close_report = true,
            Some("--dump-frames") => {
                options.dump_frames = Some(PathBuf::from(option_value(
                    &args,
                    &mut index,
                    "--dump-frames",
                )?));
            }
            Some("--plugin-defaults") => options.plugin_defaults = true,
            Some("--frames") => {
                let count = parse_count(
                    option_value(&args, &mut index, "--frames")?,
                    "--frames",
                    "invalid_frames",
                )?;
                options.frames = u32::try_from(count).map_err(|_| {
                    CliError::usage("invalid_frames", Some("--frames"), "--frames is too large")
                })?;
                if options.frames == 0 {
                    return Err(CliError::usage(
                        "invalid_frames",
                        Some("--frames"),
                        "--frames must be at least 1",
                    ));
                }
            }
            Some("--discovery-only") => options.discovery_only = true,
            Some("--inventory-only") => options.inventory_only = true,
            Some("--verify-pixel-determinism") => options.verify_pixel_determinism = true,
            Some(other) if other.starts_with('-') => {
                return Err(CliError::usage(
                    "unknown_option",
                    None,
                    format!("unknown option {other}"),
                ));
            }
            _ => options.dirs.push(PathBuf::from(argument)),
        }
        index += 1;
    }

    if options.width == 0
        || options.height == 0
        || options.width > MAX_DIMENSION
        || options.height > MAX_DIMENSION
        || u64::from(options.width) * u64::from(options.height) > MAX_PIXELS
    {
        return Err(CliError::usage(
            "invalid_size",
            Some("--size"),
            format!(
                "--size must be nonzero, at most {MAX_DIMENSION} per dimension, and at most {MAX_PIXELS} pixels"
            ),
        ));
    }
    if !(0..=TOTAL_TIME).contains(&options.current_time) {
        return Err(CliError::usage(
            "invalid_time",
            Some("--time"),
            format!("--time must be within 0..={TOTAL_TIME}"),
        ));
    }
    let span = u64::from(options.frames - 1)
        .checked_mul(u64::from(TIME_STEP.unsigned_abs()))
        .ok_or_else(|| {
            CliError::usage(
                "invalid_time_span",
                Some("--frames"),
                "--time plus --frames overflows the supported timeline",
            )
        })?;
    let last = u64::try_from(options.current_time)
        .unwrap_or_default()
        .checked_add(span)
        .ok_or_else(|| {
            CliError::usage(
                "invalid_time_span",
                Some("--frames"),
                "--time plus --frames overflows the supported timeline",
            )
        })?;
    if last > u64::from(TOTAL_TIME.unsigned_abs()) {
        return Err(CliError::usage(
            "invalid_time_span",
            Some("--frames"),
            format!("--time plus --frames runs to {last}, past the session total of {TOTAL_TIME}"),
        ));
    }
    if options.discovery_only && options.inventory_only {
        return Err(CliError::usage(
            "conflicting_modes",
            None,
            "--discovery-only and --inventory-only are mutually exclusive",
        ));
    }
    if options.same_closure_render_jobs > options.render_jobs {
        return Err(CliError::usage(
            "invalid_same_closure_render_jobs",
            Some("--same-closure-render-jobs"),
            "--same-closure-render-jobs must not exceed --render-jobs",
        ));
    }
    if !options.inventory_only && !options.blocked_paths.is_empty() {
        return Err(CliError::usage(
            "blocked_path_requires_inventory",
            Some("--blocked-path"),
            "--blocked-path requires --inventory-only",
        ));
    }
    if let Some(dir) = &options.dump_frames {
        std::fs::create_dir_all(dir).map_err(|error| {
            CliError::runtime(
                "dump_directory_unavailable",
                "output",
                format!("--dump-frames {}: {error}", dir.display()),
            )
        })?;
    }
    if let Some(path) = input_image {
        options.input_rgba = Some(
            load_primary_image(&path, options.width, options.height).map_err(|error| {
                CliError::runtime(
                    "input_image_unavailable",
                    "input",
                    format!("--input-image {}: {error}", path.display()),
                )
            })?,
        );
    }
    Ok(CliAction::Run(options))
}

fn load_primary_image(path: &Path, width: u32, height: u32) -> std::io::Result<Vec<u8>> {
    let image =
        aexcompat_broker::image_render::decode_bounded_image(path, "sweep primary")?.to_rgba8();
    if image.dimensions() != (width, height) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "image dimensions must match --size; implicit resizing is disabled",
        ));
    }
    Ok(image.into_raw())
}

fn matches_target_filters(path: &Path, include: Option<&str>, excludes: &[String]) -> bool {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let full_path = path.to_string_lossy().to_lowercase();
    include.is_none_or(|filter| file_name.contains(filter))
        && excludes.iter().all(|exclude| !full_path.contains(exclude))
}

/// Runs independent work under a fixed concurrency bound while returning
/// results in input order. Completion order is intentionally not observable in
/// the final report: two sweeps of the same corpus must remain directly
/// comparable even when different plug-ins finish first.
fn bounded_parallel_map_ordered<T, R, F>(items: &[T], jobs: usize, operation: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> R + Sync,
{
    assert!(jobs >= 1, "parallel work requires at least one job");
    if items.is_empty() {
        return Vec::new();
    }
    let next = AtomicUsize::new(0);
    let results = Mutex::new((0..items.len()).map(|_| None).collect::<Vec<Option<R>>>());
    let worker_count = jobs.min(items.len());
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let operation = &operation;
            let next = &next;
            let results = &results;
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    let result = operation(index, item);
                    results.lock().unwrap_or_else(|poison| poison.into_inner())[index] =
                        Some(result);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_else(|poison| poison.into_inner())
        .into_iter()
        .enumerate()
        .map(|(index, result)| {
            result.unwrap_or_else(|| panic!("parallel job {index} did not produce a result"))
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
enum DynamicGroupResult<T> {
    Completed(T),
    Panicked,
}

#[derive(Debug, PartialEq, Eq)]
enum GroupMemberResult<T> {
    Completed(T),
    Panicked,
}

/// Builds a complete result list without finalizing receipts or sidecar rows,
/// even when one member panics. The caller finalizes only after this list has
/// been accepted by the scheduler, so a late panic cannot duplicate siblings.
fn catch_group_members_ordered<T, F>(
    members: &[usize],
    mut operation: F,
) -> Vec<(usize, GroupMemberResult<T>)>
where
    F: FnMut(usize) -> T,
{
    members
        .iter()
        .copied()
        .map(|index| {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(index)))
                    .map_or(GroupMemberResult::Panicked, GroupMemberResult::Completed);
            (index, result)
        })
        .collect()
}

fn finalize_group_members_ordered<T, U, F>(
    members: Vec<(usize, GroupMemberResult<T>)>,
    mut finalize: F,
) -> Vec<(usize, U)>
where
    F: FnMut(usize, GroupMemberResult<T>) -> U,
{
    members
        .into_iter()
        .map(|(index, result)| (index, finalize(index, result)))
        .collect()
}

/// The historical static scheduler persists each completed member before it
/// begins the next one. Keep this separate from dynamic group's pending-result
/// transaction so a later hang or process kill cannot erase earlier progress.
fn map_group_members_immediate<T, U, F, G>(
    members: &[usize],
    mut operation: F,
    mut finalize: G,
) -> Vec<(usize, U)>
where
    F: FnMut(usize) -> T,
    G: FnMut(usize, T) -> U,
{
    members
        .iter()
        .copied()
        .map(|index| {
            let result = operation(index);
            (index, finalize(index, result))
        })
        .collect()
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
enum DynamicScheduleKey {
    Resolved(String),
    Unresolved,
}

#[derive(Clone, Debug)]
struct DynamicScheduleTask {
    plan_index: usize,
    first_group: usize,
    group_count: usize,
    key: DynamicScheduleKey,
    active_limit: usize,
}

struct DynamicScheduleState<T> {
    ready: VecDeque<DynamicScheduleTask>,
    active_by_key: HashMap<DynamicScheduleKey, usize>,
    completed_tasks: usize,
    finalize_panicked: bool,
    results: Vec<Vec<Option<T>>>,
}

#[cfg(test)]
fn dynamic_render_group_map_ordered<T, F>(
    plans: &[RenderPlan],
    jobs: usize,
    operation: F,
) -> Vec<Vec<DynamicGroupResult<T>>>
where
    T: Send,
    F: Fn(usize, usize, &[usize]) -> T + Sync,
{
    dynamic_render_group_map_ordered_then(plans, jobs, operation, |_, _, _, result| result)
}

/// Executes the already-planned render groups under both the global process
/// cap and the per-closure shard cap. Resolved groups are independent isolated
/// worker transactions and may be stolen from any planning shard. An unresolved
/// plan is one task containing its complete group chain, so its historical
/// serial behavior is preserved. Results remain indexed by the stable planning
/// shard/group coordinates rather than nondeterministic executor threads.
///
/// The operation boundary is caught here, outside plug-in-specific catches. A
/// panicking group is passed to `finalize` as a fail-closed marker. Finalization
/// runs on the executor before the closure permit is released, allowing durable
/// progress to be appended once per group rather than delayed until the entire
/// sweep finishes. A finalizer panic is never retried (which could duplicate a
/// partial append); it returns its permit, wakes waiters, and aborts the scheduler.
fn dynamic_render_group_map_ordered_then<T, U, F, G>(
    plans: &[RenderPlan],
    jobs: usize,
    operation: F,
    finalize: G,
) -> Vec<Vec<U>>
where
    T: Send,
    U: Send,
    F: Fn(usize, usize, &[usize]) -> T + Sync,
    G: Fn(usize, usize, &[usize], DynamicGroupResult<T>) -> U + Sync,
{
    assert!(
        jobs >= 1,
        "dynamic render scheduling requires at least one job"
    );
    for plan in plans {
        assert!(
            plan.same_closure_count >= 1,
            "dynamic render plan has no closure permit"
        );
        assert!(
            plan.same_closure_index < plan.same_closure_count,
            "dynamic render plan has invalid shard provenance"
        );
    }

    // Seed the ready queue by group depth, not plan-major order. The first
    // group from every shard is therefore admitted before any shard's second
    // group; once a short shard finishes, its executor can take later work from
    // a still-busy resolved closure. None is deliberately represented by one
    // task spanning the whole plan.
    let max_groups = plans
        .iter()
        .filter(|plan| plan.closure_identity.is_some())
        .map(|plan| plan.groups.len())
        .max()
        .unwrap_or(0);
    let mut ready = VecDeque::new();
    let mut expected_limit_by_key = HashMap::<DynamicScheduleKey, usize>::new();
    for group_index in 0..max_groups.max(1) {
        for (plan_index, plan) in plans.iter().enumerate() {
            let Some(closure) = plan.closure_identity.as_ref() else {
                if group_index == 0 && !plan.groups.is_empty() {
                    ready.push_back(DynamicScheduleTask {
                        plan_index,
                        first_group: 0,
                        group_count: plan.groups.len(),
                        key: DynamicScheduleKey::Unresolved,
                        active_limit: 1,
                    });
                }
                continue;
            };
            if group_index >= plan.groups.len() {
                continue;
            }
            let key = DynamicScheduleKey::Resolved(closure.clone());
            let active_limit = plan.same_closure_count;
            if let Some(previous) = expected_limit_by_key.insert(key.clone(), active_limit) {
                assert_eq!(
                    previous, active_limit,
                    "one resolved closure has inconsistent scheduling caps"
                );
            }
            ready.push_back(DynamicScheduleTask {
                plan_index,
                first_group: group_index,
                group_count: 1,
                key,
                active_limit,
            });
        }
    }

    let total_tasks = ready.len();
    let results = plans
        .iter()
        .map(|plan| (0..plan.groups.len()).map(|_| None).collect())
        .collect();
    if total_tasks == 0 {
        return plans.iter().map(|_| Vec::new()).collect();
    }

    let shared = (
        Mutex::new(DynamicScheduleState {
            ready,
            active_by_key: HashMap::new(),
            completed_tasks: 0,
            finalize_panicked: false,
            results,
        }),
        Condvar::new(),
    );
    let worker_count = jobs.min(total_tasks);
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let operation = &operation;
            let finalize = &finalize;
            let shared = &shared;
            scope.spawn(move || {
                loop {
                    let task = {
                        let (lock, ready_changed) = shared;
                        let mut state = lock.lock().unwrap_or_else(|poison| poison.into_inner());
                        loop {
                            if state.finalize_panicked {
                                return;
                            }
                            if state.completed_tasks == total_tasks {
                                return;
                            }
                            let eligible = state.ready.iter().position(|task| {
                                state.active_by_key.get(&task.key).copied().unwrap_or(0)
                                    < task.active_limit
                            });
                            if let Some(index) = eligible {
                                let task = state
                                    .ready
                                    .remove(index)
                                    .expect("eligible dynamic task disappeared");
                                *state.active_by_key.entry(task.key.clone()).or_default() += 1;
                                break task;
                            }
                            state = ready_changed
                                .wait(state)
                                .unwrap_or_else(|poison| poison.into_inner());
                        }
                    };

                    let mut completed = Vec::with_capacity(task.group_count);
                    for group_index in task.first_group..task.first_group + task.group_count {
                        let run = &plans[task.plan_index].groups[group_index];
                        let pending =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                operation(task.plan_index, group_index, run)
                            }))
                            .map_or(DynamicGroupResult::Panicked, DynamicGroupResult::Completed);
                        let finalized =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                finalize(task.plan_index, group_index, run, pending)
                            }));
                        let Ok(result) = finalized else {
                            let (lock, ready_changed) = shared;
                            let mut state =
                                lock.lock().unwrap_or_else(|poison| poison.into_inner());
                            let active = state
                                .active_by_key
                                .get_mut(&task.key)
                                .expect("dynamic render task lost its active permit");
                            assert!(*active > 0, "dynamic render permit underflow");
                            *active -= 1;
                            state.finalize_panicked = true;
                            ready_changed.notify_all();
                            return;
                        };
                        completed.push((group_index, result));
                    }

                    let (lock, ready_changed) = shared;
                    let mut state = lock.lock().unwrap_or_else(|poison| poison.into_inner());
                    for (group_index, result) in completed {
                        assert!(
                            state.results[task.plan_index][group_index]
                                .replace(result)
                                .is_none(),
                            "dynamic render group completed twice"
                        );
                    }
                    let active = state
                        .active_by_key
                        .get_mut(&task.key)
                        .expect("dynamic render task lost its active permit");
                    assert!(*active > 0, "dynamic render permit underflow");
                    *active -= 1;
                    state.completed_tasks += 1;
                    ready_changed.notify_all();
                }
            });
        }
    });

    let state = shared
        .0
        .into_inner()
        .unwrap_or_else(|poison| poison.into_inner());
    assert!(
        state.active_by_key.values().all(|&active| active == 0),
        "dynamic render scheduler leaked active permits"
    );
    assert!(
        !state.finalize_panicked,
        "dynamic render group finalization panicked"
    );
    assert_eq!(state.completed_tasks, total_tasks);
    state
        .results
        .into_iter()
        .enumerate()
        .map(|(plan_index, groups)| {
            groups
                .into_iter()
                .enumerate()
                .map(|(group_index, result)| {
                    result.unwrap_or_else(|| {
                        panic!(
                            "dynamic render group {plan_index}/{group_index} did not produce a result"
                        )
                    })
                })
                .collect()
        })
        .collect()
}

/// Partitions the corpus into serial lanes. Equal dependency-closure identities
/// must never overlap: real plug-ins may coordinate through vendor-global
/// helpers even though each AEXCompat worker is process-isolated. Lanes whose
/// identities differ may run concurrently.
fn serial_lanes_by_key<T, K, F>(items: &[T], jobs: usize, key_of: F) -> Vec<Vec<usize>>
where
    K: Eq + std::hash::Hash,
    F: Fn(&T) -> K,
{
    if jobs == 1 {
        return vec![(0..items.len()).collect()];
    }
    let mut lane_by_key = HashMap::new();
    let mut lanes: Vec<Vec<usize>> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let key = key_of(item);
        let lane = *lane_by_key.entry(key).or_insert_with(|| {
            lanes.push(Vec::new());
            lanes.len() - 1
        });
        lanes[lane].push(index);
    }
    lanes
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DependencyRenderShard {
    indices: Vec<usize>,
    same_closure_index: usize,
    same_closure_count: usize,
    closure_identity_resolved: bool,
}

/// Retains the established dependency lanes, then explicitly subdivides only
/// lanes with a resolved closure identity. Each subdivision is contiguous and
/// differs in length from its neighbours by at most one, so the default value
/// of one is exactly the historical lane plan while an opt-in split remains
/// stable and auditable. An unresolved identity is never assumed independent.
fn dependency_render_shards<T, K, F>(
    items: &[T],
    render_jobs: usize,
    same_closure_render_jobs: usize,
    key_of: F,
) -> Vec<DependencyRenderShard>
where
    K: Eq + std::hash::Hash,
    F: Fn(&T) -> Option<K>,
{
    assert!(render_jobs >= 1, "render jobs must be at least one");
    assert!(
        (1..=render_jobs).contains(&same_closure_render_jobs),
        "same-closure render jobs must be between one and the global render job cap"
    );
    let lanes = serial_lanes_by_key(items, render_jobs, |item| key_of(item));
    let mut shards = Vec::new();
    for lane in lanes {
        let closure_identity_resolved = lane
            .first()
            .is_some_and(|&index| key_of(&items[index]).is_some());
        let shard_count = if closure_identity_resolved {
            same_closure_render_jobs.min(lane.len().max(1))
        } else {
            1
        };
        if shard_count == 1 {
            shards.push(DependencyRenderShard {
                indices: lane,
                same_closure_index: 0,
                same_closure_count: 1,
                closure_identity_resolved,
            });
            continue;
        }

        let shorter = lane.len() / shard_count;
        let longer_count = lane.len() % shard_count;
        let mut start = 0;
        for same_closure_index in 0..shard_count {
            let length = shorter + usize::from(same_closure_index < longer_count);
            let end = start + length;
            shards.push(DependencyRenderShard {
                indices: lane[start..end].to_vec(),
                same_closure_index,
                same_closure_count: shard_count,
                closure_identity_resolved: true,
            });
            start = end;
        }
        debug_assert_eq!(start, lane.len());
    }
    shards
}

#[derive(Debug)]
struct RenderPlan {
    groups: Vec<Vec<usize>>,
    same_closure_index: usize,
    same_closure_count: usize,
    closure_identity: Option<String>,
}

fn attach_same_closure_shard_evidence(
    record: &mut Value,
    same_closure_index: usize,
    same_closure_count: usize,
) {
    assert!(same_closure_count >= 1);
    assert!(same_closure_index < same_closure_count);
    record["same_closure_shard"] = json!({
        "index": same_closure_index,
        "count": same_closure_count,
    });
}

fn attach_render_work_group_evidence(record: &mut Value, index: usize, count: usize) {
    assert!(count >= 1);
    assert!(index < count);
    record["render_work_group"] = json!({
        "index": index,
        "count": count,
    });
}

fn restore_indexed_order<T>(length: usize, groups: Vec<Vec<(usize, T)>>) -> Vec<T> {
    let mut ordered = (0..length).map(|_| None).collect::<Vec<Option<T>>>();
    for (index, value) in groups.into_iter().flatten() {
        assert!(
            index < length,
            "parallel result index is outside the corpus"
        );
        assert!(
            ordered[index].replace(value).is_none(),
            "duplicate parallel result index"
        );
    }
    ordered
        .into_iter()
        .enumerate()
        .map(|(index, value)| value.unwrap_or_else(|| panic!("parallel result {index} is missing")))
        .collect()
}

pub fn main_entry() -> ExitCode {
    match run_cli(std::env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error.emit();
            ExitCode::from(error.exit_code)
        }
    }
}

fn run_cli(args: impl IntoIterator<Item = OsString>) -> Result<(), CliError> {
    let options = match parse_options_from(args)? {
        CliAction::Help => {
            print!("{HELP}");
            return Ok(());
        }
        CliAction::Run(options) => options,
    };
    run(options)
}

fn run(options: Options) -> Result<(), CliError> {
    let cli_path = std::env::current_exe().map_err(|error| {
        CliError::runtime(
            "current_executable_unavailable",
            "environment",
            format!("the current executable path is unavailable: {error}"),
        )
    })?;
    let (scan, repository) = scan_for_diagnostics_with_worker_root(
        (!options.dirs.is_empty()).then(|| options.dirs.clone()),
        options.repository.clone(),
        Some(cli_path.clone()),
    )
    .map_err(|message| CliError::runtime("worker_root_unavailable", "environment", message))?;
    let cli_path = Ok(cli_path);
    let build = capture_report_build_fingerprint(&repository, cli_path.clone());
    // Not exhaustive means the denominator is short by an unknown amount, which
    // is the one thing a sweep's headline number must not hide (#660).
    if let Some(reason) = &scan.incomplete_reason {
        eprintln!("warning: the scan was not exhaustive: {reason}");
    }
    let mut matched: Vec<PathBuf> = scan
        .plugins
        .iter()
        .filter(|path| {
            matches_target_filters(path, options.filter.as_deref(), &options.exclude_paths)
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
    let started = Instant::now();
    if targets.is_empty() {
        eprintln!("nothing to sweep");
        let build = finalize_report_build_fingerprint(&build, &repository, cli_path);
        let report = if options.inventory_only {
            inventory_report(&options, &scan, &build, Vec::new(), started.elapsed())
        } else if options.discovery_only {
            discovery_only_report(
                &options,
                &scan,
                &build,
                &[],
                Duration::ZERO,
                started.elapsed(),
            )
        } else {
            report(
                &options,
                &scan,
                &build,
                Duration::ZERO,
                started.elapsed(),
                BTreeMap::new(),
                Vec::new(),
                Some(0),
                Some(0),
                Some(0),
                Some(0),
            )
        };
        finish_report(&options, &report)?;
        return Ok(());
    }

    if options.inventory_only {
        eprintln!(
            "inventorying {} plug-in file(s) without loading AEX...",
            targets.len()
        );
        let plugins = targets
            .iter()
            .map(|path| inventory_record(path, &scan.dirs, &options, &build))
            .collect::<Vec<_>>();
        let build = finalize_report_build_fingerprint(&build, &repository, cli_path);
        let report = inventory_report(&options, &scan, &build, plugins, started.elapsed());
        finish_report(&options, &report)?;
        return Ok(());
    }
    eprintln!("discovering {} plug-in(s)...", targets.len());
    let discovery_started = Instant::now();
    let run_partial = options.json.as_ref().map(|path| partial_path(path));
    let discovery_partial = Mutex::new(
        options
            .discovery_only
            .then(|| run_partial.as_ref().cloned())
            .flatten(),
    );
    let completed = AtomicUsize::new(0);
    let mut records = after_partial_is_prepared(run_partial.as_deref(), || {
        discover_records_for_diagnostics_with_progress(
            &repository,
            &targets,
            scan.dependency_dirs.clone(),
            |batch| {
                record_discovery_progress(
                    &batch,
                    &scan,
                    &build,
                    &discovery_partial,
                    &completed,
                    targets.len(),
                );
            },
        )
    })?;
    records.sort_by(|left, right| left.path.cmp(&right.path));
    let discovery_elapsed = discovery_started.elapsed();
    eprintln!(
        "discovery: {} of {} inspected in {:.1}s",
        records.iter().filter(|record| record.ok).count(),
        records.len(),
        discovery_elapsed.as_secs_f64(),
    );

    if options.discovery_only {
        let build = finalize_report_build_fingerprint(&build, &repository, cli_path);
        let report = discovery_only_report(
            &options,
            &scan,
            &build,
            &records,
            discovery_elapsed,
            started.elapsed(),
        );
        finish_report(&options, &report)?;
        return Ok(());
    }

    // Built once: they depend only on the session geometry, and rebuilding them
    // per plug-in would memcpy the same few hundred KB a few hundred times.
    let input = options
        .input_rgba
        .clone()
        .unwrap_or_else(|| primary_pixels(options.width, options.height));
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
    let sidecar = if let Some(path) = run_partial {
        Some((path, Mutex::new(())))
    } else {
        None
    };

    let lanes = serial_lanes_by_key(&records, options.render_jobs, |record| {
        record.closure_identity_sha256.clone()
    });
    let render_shards = dependency_render_shards(
        &records,
        options.render_jobs,
        options.same_closure_render_jobs,
        |record| record.closure_identity_sha256.clone(),
    );
    let effective_same_closure_render_jobs = render_shards
        .iter()
        .filter(|shard| shard.closure_identity_resolved)
        .map(|shard| shard.same_closure_count)
        .max()
        .unwrap_or(1);
    let render_plans = render_shards
        .iter()
        .map(|shard| RenderPlan {
            groups: cluster_candidate_groups(&shard.indices, &records, &options),
            same_closure_index: shard.same_closure_index,
            same_closure_count: shard.same_closure_count,
            closure_identity: shard
                .indices
                .first()
                .and_then(|&index| records[index].closure_identity_sha256.clone()),
        })
        .collect::<Vec<_>>();
    let planned_sessions = render_plans
        .iter()
        .map(|plan| plan.groups.len())
        .sum::<usize>();
    let planned_clustered_plugins = render_plans
        .iter()
        .flat_map(|plan| &plan.groups)
        .filter(|group| group.len() >= 2)
        .map(Vec::len)
        .sum::<usize>();
    if options.dynamic_same_closure_groups {
        eprintln!(
            "rendering {} plug-in(s) in {} dependency lane(s), {} shard(s), with {} job(s); same-closure jobs {}/{}, dynamic closure-capped group schedule, {} planned session(s), {} clustered plug-in(s)...",
            records.len(),
            lanes.len(),
            render_plans.len(),
            options.render_jobs.min(render_plans.len()),
            effective_same_closure_render_jobs,
            options.same_closure_render_jobs,
            planned_sessions,
            planned_clustered_plugins,
        );
    } else {
        eprintln!(
            "rendering {} plug-in(s) in {} dependency lane(s), {} shard(s), with {} job(s); same-closure jobs {}/{}, {} planned session(s), {} clustered plug-in(s)...",
            records.len(),
            lanes.len(),
            render_plans.len(),
            options.render_jobs.min(render_plans.len()),
            effective_same_closure_render_jobs,
            options.same_closure_render_jobs,
            planned_sessions,
            planned_clustered_plugins,
        );
    }
    let finish_plugin = |index: usize,
                         record: &DiagnosticDiscovery,
                         outcome: Outcome,
                         elapsed_ms: u128,
                         same_closure_index: usize,
                         same_closure_count: usize,
                         group_index: usize,
                         group_count: usize| {
        let name = plugin_name(&record.path, &scan.dirs);
        // Numbered from the corpus, not from this slice: the number an
        // operator reads off the log is the one they pass back as --skip.
        eprintln!(
            "[{}/{}] {}\t{}\t{elapsed_ms}ms",
            options.skip + index + 1,
            options.skip + records.len(),
            name.relative,
            outcome.bucket,
        );
        let mut record = plugin_record(record, &name, &build, outcome, elapsed_ms);
        attach_same_closure_shard_evidence(&mut record, same_closure_index, same_closure_count);
        if options.dynamic_same_closure_groups {
            attach_render_work_group_evidence(&mut record, group_index, group_count);
        }
        if let Some((path, write_lock)) = &sidecar {
            let mut line = Vec::new();
            if serde_json::to_writer(&mut line, &record).is_ok() {
                line.push(b'\n');
                let _guard = write_lock
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                append_line(path, &line);
            }
        }
        record
    };
    let render_member = |index: usize, clustered: &HashMap<usize, (Outcome, u128)>| {
        let record = &records[index];
        let plugin_started = Instant::now();
        let clustered_result = clustered.get(&index).cloned();
        let clustered_elapsed_ms = clustered_result.as_ref().map(|(_, elapsed)| *elapsed);
        let mut outcome = clustered_result
            .map(|(outcome, _)| outcome)
            .unwrap_or_else(|| {
                sweep_one_caught(
                    &repository,
                    record,
                    &records,
                    &options,
                    options.skip + index,
                    &input,
                    &layer_pixels,
                )
            });
        let verification_started = Instant::now();
        verify_pixel_determinism(&mut outcome, options.verify_pixel_determinism, || {
            let mut repeat_options = options.clone();
            // The primary frame dump is the artifact requested by the caller.
            // A verification pass must not overwrite it.
            repeat_options.dump_frames = None;
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                sweep_one(
                    &repository,
                    record,
                    &records,
                    &repeat_options,
                    options.skip + index,
                    &input,
                    &layer_pixels,
                )
            }))
            .unwrap_or_else(|_| Outcome::bare("sweep_panicked"))
        });
        let elapsed_ms = clustered_elapsed_ms
            .map(|clustered| {
                clustered
                    + options
                        .verify_pixel_determinism
                        .then(|| verification_started.elapsed().as_millis())
                        .unwrap_or_default()
            })
            .unwrap_or_else(|| plugin_started.elapsed().as_millis());
        (outcome, elapsed_ms)
    };
    let execute_dynamic_group = |_plan_index: usize, _group_index: usize, run: &[usize]| {
        let clustered = sweep_cluster_candidates_salvaging(
            &repository,
            run,
            &records,
            &options,
            &input,
            &layer_pixels,
        );
        // Dynamic scheduling catches members into one receipt-free pending
        // result; its scheduler callback finalizes that group exactly once.
        catch_group_members_ordered(run, |index| render_member(index, &clustered))
    };
    let execute_static_group = |plan_index: usize, group_index: usize, run: &[usize]| {
        let plan = &render_plans[plan_index];
        let clustered = sweep_cluster_candidates_salvaging(
            &repository,
            run,
            &records,
            &options,
            &input,
            &layer_pixels,
        );
        map_group_members_immediate(
            run,
            |index| render_member(index, &clustered),
            |index, (outcome, elapsed_ms)| {
                finish_plugin(
                    index,
                    &records[index],
                    outcome,
                    elapsed_ms,
                    plan.same_closure_index,
                    plan.same_closure_count,
                    group_index,
                    plan.groups.len(),
                )
            },
        )
    };
    let finalize_group =
        |plan_index: usize,
         group_index: usize,
         pending: Vec<(usize, GroupMemberResult<(Outcome, u128)>)>| {
            let plan = &render_plans[plan_index];
            finalize_group_members_ordered(pending, |index, result| {
                let (outcome, elapsed_ms) = match result {
                    GroupMemberResult::Completed(completed) => completed,
                    GroupMemberResult::Panicked => (Outcome::bare("sweep_panicked"), 0),
                };
                finish_plugin(
                    index,
                    &records[index],
                    outcome,
                    elapsed_ms,
                    plan.same_closure_index,
                    plan.same_closure_count,
                    group_index,
                    plan.groups.len(),
                )
            })
        };
    let grouped = if options.dynamic_same_closure_groups {
        dynamic_render_group_map_ordered_then(
            &render_plans,
            options.render_jobs,
            &execute_dynamic_group,
            |plan_index, group_index, run, result| {
                let pending = match result {
                    DynamicGroupResult::Completed(pending) => pending,
                    DynamicGroupResult::Panicked => run
                        .iter()
                        .map(|&index| (index, GroupMemberResult::Panicked))
                        .collect(),
                };
                finalize_group(plan_index, group_index, pending)
            },
        )
        .into_iter()
        .map(|groups| groups.into_iter().flatten().collect())
        .collect()
    } else {
        bounded_parallel_map_ordered(&render_plans, options.render_jobs, |plan_index, plan| {
            plan.groups
                .iter()
                .enumerate()
                .flat_map(|(group_index, run)| execute_static_group(plan_index, group_index, run))
                .collect()
        })
    };
    let plugins = restore_indexed_order(records.len(), grouped);
    let mut buckets: BTreeMap<String, usize> = BTreeMap::new();
    for plugin in &plugins {
        if let Some(bucket) = plugin.get("bucket").and_then(Value::as_str) {
            *buckets.entry(bucket.to_owned()).or_default() += 1;
        }
    }

    let build = finalize_report_build_fingerprint(&build, &repository, cli_path);
    let report = report(
        &options,
        &scan,
        &build,
        discovery_elapsed,
        started.elapsed(),
        buckets,
        plugins,
        Some(lanes.len()),
        Some(render_plans.len()),
        Some(effective_same_closure_render_jobs),
        Some(planned_sessions),
    );
    finish_report(&options, &report)?;
    Ok(())
}

fn sweep_one_caught(
    repository: &Path,
    record: &DiagnosticDiscovery,
    records: &[DiagnosticDiscovery],
    options: &Options,
    corpus_index: usize,
    input: &[u8],
    layer_pixels: &[u8],
) -> Outcome {
    catch_sweep_outcome(|| {
        sweep_one(
            repository,
            record,
            records,
            options,
            corpus_index,
            input,
            layer_pixels,
        )
    })
}

fn catch_sweep_outcome<F>(launch: F) -> Outcome
where
    F: FnOnce() -> Outcome,
{
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(launch)).unwrap_or_else(|_| Outcome {
        bucket: "sweep_panicked".to_owned(),
        detail: Map::new(),
    })
}

fn attach_classic_comparison<F>(primary: &mut Outcome, launch: F)
where
    F: FnOnce() -> Outcome,
{
    let comparison = catch_sweep_outcome(launch);
    primary.detail.insert(
        "classic_comparison".to_owned(),
        json!({
            "bucket": comparison.bucket,
            "pixel_sha256": comparison.detail.get("pixel_sha256"),
            "nonzero_alpha_pixels": comparison.detail.get("nonzero_alpha_pixels"),
            "phase_elapsed_ms": comparison.detail.get("phase_elapsed_ms"),
            "session_clean": comparison.detail.get("session_clean"),
            "invalidated_reason": comparison.detail.get("invalidated_reason")
        }),
    );
    primary.detail.insert(
        "comparison_reason".to_owned(),
        json!("smart_output_transparent"),
    );
}

/// Uses the same in-place cluster session as the shipping bridge for the safe
/// subset it pools there: SmartFX effects which share the one secondary-layer
/// slot supplied by the shipping bridge (or all omit it). Healthy
/// members amortize worker/bootstrap teardown across the closure. An explicit
/// member-local failure can be removed under the bounded salvage policy; any
/// ambiguous session-wide failure rejects the whole fast path. Every unresolved
/// member still takes the existing one-plugin path, so failure attribution is
/// never weakened for speed.
fn sweep_cluster_candidates_salvaging(
    repository: &Path,
    candidates: &[usize],
    records: &[DiagnosticDiscovery],
    options: &Options,
    input: &[u8],
    layer_pixels: &[u8],
) -> HashMap<usize, (Outcome, u128)> {
    let mut attempt = |subset: &[usize]| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sweep_cluster_candidates(repository, subset, records, options, input, layer_pixels)
        }))
        .unwrap_or(ClusterAttempt::HardFailure)
    };
    salvage_cluster_candidates(candidates, MAX_SWEEP_CLUSTER_SALVAGE_ATTEMPTS, &mut attempt)
}

enum ClusterAttempt<T> {
    Complete(HashMap<usize, T>),
    /// The worker reached this member and produced an explicit member-local
    /// setup/frame failure. It may be removed before retrying the remainder;
    /// the caller still renders it independently for authoritative evidence.
    RejectMember(usize),
    /// Opening, transport, close validation, panic, or another session-wide
    /// ambiguity. Retrying smaller subsets could repeat the same expensive
    /// failure without adding a discriminating fact, so fall back immediately.
    HardFailure,
}

fn cluster_close_is_clean(close: &Value) -> bool {
    close.get("session_clean") == Some(&Value::Bool(true))
        && close.get("invalidated") == Some(&Value::Bool(false))
}

fn rejected_member_after_close<T>(member: usize, close: &Value) -> ClusterAttempt<T> {
    if cluster_close_is_clean(close) {
        ClusterAttempt::RejectMember(member)
    } else {
        ClusterAttempt::HardFailure
    }
}

/// Recovers the healthy remainder after an explicitly identified bad member.
/// A cluster is accepted only when it returns one result for every requested
/// member. Hard/ambiguous failures stop after one attempt, known bad members
/// are never retried, and `attempts_remaining` bounds a run of explicit errors.
fn salvage_cluster_candidates<T, F>(
    candidates: &[usize],
    attempts_remaining: usize,
    attempt: &mut F,
) -> HashMap<usize, T>
where
    F: FnMut(&[usize]) -> ClusterAttempt<T>,
{
    if candidates.len() < 2 || attempts_remaining == 0 {
        return HashMap::new();
    }
    match attempt(candidates) {
        ClusterAttempt::Complete(outcomes)
            if outcomes.len() == candidates.len()
                && candidates.iter().all(|index| outcomes.contains_key(index)) =>
        {
            outcomes
        }
        ClusterAttempt::RejectMember(rejected) if candidates.contains(&rejected) => {
            let remaining = candidates
                .iter()
                .copied()
                .filter(|index| *index != rejected)
                .collect::<Vec<_>>();
            salvage_cluster_candidates(&remaining, attempts_remaining - 1, attempt)
        }
        ClusterAttempt::Complete(_)
        | ClusterAttempt::RejectMember(_)
        | ClusterAttempt::HardFailure => HashMap::new(),
    }
}

fn sweep_cluster_candidates(
    repository: &Path,
    candidates: &[usize],
    records: &[DiagnosticDiscovery],
    options: &Options,
    input: &[u8],
    layer_pixels: &[u8],
) -> ClusterAttempt<(Outcome, u128)> {
    if !cluster_fast_path_enabled(options) {
        return ClusterAttempt::HardFailure;
    }
    if candidates.len() < 2 || candidates.len() > MAX_SWEEP_CLUSTER_MEMBERS {
        return ClusterAttempt::HardFailure;
    }

    let first = &records[candidates[0]];
    if !cluster_fast_path_eligible(first)
        || candidates.iter().skip(1).any(|&index| {
            let record = &records[index];
            !cluster_fast_path_eligible(record)
                || record.closure_identity_sha256 != first.closure_identity_sha256
                || record.search_roots != first.search_roots
                || cluster_layer_slot(record, options) != cluster_layer_slot(first, options)
        })
    {
        return ClusterAttempt::HardFailure;
    }

    let mut plugins = Vec::with_capacity(candidates.len());
    let mut companions = Vec::new();
    for &index in candidates {
        let record = &records[index];
        let Some(expected_sha256) = decode_sha256(&record.sha256) else {
            return ClusterAttempt::HardFailure;
        };
        plugins.push(ApprovedImageArtifact {
            path: record.path.clone(),
            expected_sha256,
            expected_size: record.byte_size,
        });
        let Ok(providers) = companion_providers_for(record, records) else {
            return ClusterAttempt::HardFailure;
        };
        companions.extend(providers);
    }
    companions.sort_by(|left, right| left.artifact.path.cmp(&right.artifact.path));
    companions.dedup_by(|left, right| left.artifact.path == right.artifact.path);

    let layers = probe_layers(first, options, layer_pixels);
    let open_started = Instant::now();
    let mut session = match RenderSession::open_cluster(
        SessionOpenRequest {
            repository,
            plugin_path: &first.path,
            plugin_sha256: &first.sha256,
            parameters: None,
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
            companions,
            dependency_search_dirs: first.search_roots.clone(),
            width: options.width,
            height: options.height,
            pixel_format: options.pixel_format,
            time_step: TIME_STEP,
            total_time: TOTAL_TIME,
            time_scale: TIME_SCALE,
            frame_deadline: FRAME_DEADLINE,
            smart: true,
            gpu_backend: RenderGpuBackend::Auto,
            gpu_runtime_policy: None,
            payload_override: None,
            launch_environment: Default::default(),
        },
        ClusterRenderPlugins {
            swap_payloads: vec![None; plugins.len()],
            plugins,
            module_bound: aexcompat_broker::cluster_manifest::MAX_CLUSTER_MODULE_BOUND,
        },
    ) {
        Ok(session) => session,
        Err(_) => return ClusterAttempt::HardFailure,
    };
    let open_ms = open_started.elapsed().as_millis();

    let mut outcomes = HashMap::new();
    for (plugin_index, &record_index) in candidates.iter().enumerate() {
        let swap_started = Instant::now();
        if plugin_index > 0 {
            match session.swap_plugin(plugin_index as u32) {
                Ok(SwapOutcome::Swapped) => {}
                Ok(SwapOutcome::PluginError { .. }) => {
                    let close = session.close();
                    return rejected_member_after_close(record_index, &close);
                }
                Err(_) => {
                    let _ = session.close();
                    return ClusterAttempt::HardFailure;
                }
            }
        }
        let swap_ms = swap_started.elapsed().as_millis();
        let frame_started = Instant::now();
        let frame = session.render_frame_with_parameters(
            plugin_index as u32,
            options.current_time,
            input,
            None,
        );
        let transport_failed = frame.is_err();
        let mut outcome = frame_outcome_dumping(frame, None, options.pixel_format);
        let frame_ms = frame_started.elapsed().as_millis();
        // These cases require the per-plugin close evidence used by the existing
        // fallback/classification logic. Abandon the optimization and let the
        // caller reproduce every candidate independently.
        if outcome.bucket != "rendered" {
            let close = session.close();
            return if transport_failed {
                ClusterAttempt::HardFailure
            } else {
                rejected_member_after_close(record_index, &close)
            };
        }
        outcome.detail.insert(
            "phase_elapsed_ms".to_owned(),
            json!({
                "session_open": if plugin_index == 0 { open_ms } else { 0 },
                "plugin_swap": swap_ms,
                "frames": frame_ms,
                "session_close": 0,
            }),
        );
        outcome.detail.insert(
            "cluster_session".to_owned(),
            json!({ "plugin_index": plugin_index, "plugin_count": candidates.len() }),
        );
        let elapsed = (if plugin_index == 0 { open_ms } else { 0 }) + swap_ms + frame_ms;
        outcomes.insert(record_index, (outcome, elapsed));
    }

    let close_started = Instant::now();
    let close = session.close();
    let close_ms = close_started.elapsed().as_millis();
    if !cluster_close_is_clean(&close) {
        return ClusterAttempt::HardFailure;
    }
    for (position, record_index) in candidates.iter().enumerate() {
        let Some((outcome, elapsed)) = outcomes.get_mut(record_index) else {
            return ClusterAttempt::HardFailure;
        };
        if position + 1 == candidates.len() {
            *elapsed += close_ms;
            if let Some(phases) = outcome
                .detail
                .get_mut("phase_elapsed_ms")
                .and_then(Value::as_object_mut)
            {
                phases.insert("session_close".to_owned(), json!(close_ms));
            }
        }
        attach_shared_cluster_close(outcome, &close);
    }
    ClusterAttempt::Complete(outcomes)
}

fn cluster_fast_path_enabled(options: &Options) -> bool {
    options.frames == 1
        && options.dump_frames.is_none()
        && !options.close_report
        && !options.force_classic
}

fn cluster_fast_path_eligible(record: &DiagnosticDiscovery) -> bool {
    record.ok
        && record.plugin_kind == aexcompat_aviutl2_multifilter::DiscoveredPluginKind::Effect
        && record.closure_identity_sha256.is_some()
        && smart_render_route_supported(record.smart, record.out_flags2)
}

fn cluster_layer_slot(record: &DiagnosticDiscovery, options: &Options) -> Option<u32> {
    (!options.no_layer)
        .then(|| layer_slots_of(&record.parameters).into_iter().next())
        .flatten()
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ClusterCandidateKey {
    closure_identity_sha256: String,
    search_roots: Vec<PathBuf>,
    layer_slot: Option<u32>,
}

fn cluster_candidate_key(
    record: &DiagnosticDiscovery,
    options: &Options,
) -> Option<ClusterCandidateKey> {
    cluster_fast_path_eligible(record).then(|| ClusterCandidateKey {
        closure_identity_sha256: record
            .closure_identity_sha256
            .clone()
            .expect("eligible cluster candidate has a closure identity"),
        search_roots: record.search_roots.clone(),
        layer_slot: cluster_layer_slot(record, options),
    })
}

/// Plans stable, bounded cluster transactions for one serial dependency lane.
/// Compatible members need not be adjacent: the sweep restores corpus order at
/// the report boundary, while grouping them here avoids another vendor/runtime
/// bootstrap for every interleaved layer layout. Group order and member order
/// both follow first encounter order, and no group can exceed the close-
/// validation checkpoint bound.
fn cluster_candidate_groups(
    lane: &[usize],
    records: &[DiagnosticDiscovery],
    options: &Options,
) -> Vec<Vec<usize>> {
    if !cluster_fast_path_enabled(options) {
        return lane.iter().map(|&index| vec![index]).collect();
    }

    let mut groups = Vec::<Vec<usize>>::new();
    let mut latest_group_by_key = HashMap::<ClusterCandidateKey, usize>::new();
    for &index in lane {
        let Some(key) = cluster_candidate_key(&records[index], options) else {
            groups.push(vec![index]);
            continue;
        };
        if let Some(&group_index) = latest_group_by_key.get(&key)
            && groups[group_index].len() < MAX_SWEEP_CLUSTER_MEMBERS
        {
            groups[group_index].push(index);
            continue;
        }
        let group_index = groups.len();
        groups.push(vec![index]);
        latest_group_by_key.insert(key, group_index);
    }
    groups
}

fn record_discovery_progress(
    batch: &[DiagnosticDiscovery],
    scan: &DiagnosticScan,
    build: &ReportBuildFingerprint,
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
        let value = plugin_record(record, &name, build, discovery_outcome(record), 0);
        if let Ok(mut line) = serde_json::to_vec(&value) {
            line.push(b'\n');
            append_line(path, &line);
        }
    }
}

fn discovery_only_report(
    options: &Options,
    scan: &DiagnosticScan,
    build: &ReportBuildFingerprint,
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
            plugin_record(record, &name, build, outcome, 0)
        })
        .collect();
    report(
        options,
        scan,
        build,
        discovery_elapsed,
        elapsed,
        buckets,
        plugins,
        None,
        None,
        None,
        None,
    )
}

fn inventory_path_sha256(path: &Path) -> String {
    let identity = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let normalized = identity.to_string_lossy().replace('\\', "/").to_lowercase();
    format!("{:x}", Sha256::digest(normalized.as_bytes()))
}

fn inventory_record(
    path: &Path,
    roots: &[PathBuf],
    options: &Options,
    build: &ReportBuildFingerprint,
) -> Value {
    let name = plugin_name(path, roots);
    let full_path = path.to_string_lossy().to_lowercase();
    let blocked_by = options
        .blocked_paths
        .iter()
        .filter(|needle| full_path.contains(needle.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let blocked = !blocked_by.is_empty();
    let fingerprint = fingerprint_executable(path);
    json!({
        "plugin": name.basename,
        "plugin_relative_path": name.relative,
        "scan_folder": name.root,
        "plugin_path_sha256": inventory_path_sha256(path),
        "plugin_sha256": fingerprint.sha256,
        "plugin_size_bytes": fingerprint.size_bytes,
        "identity_error": fingerprint.error,
        "build": build,
        "final_stage": "scan",
        "execution_classification": if blocked { "external_blocked" } else { "unexecuted" },
        "failure_classification": if blocked { Some("external_blocked") } else { None },
        "bucket": if blocked { "external_blocked" } else { "unexecuted" },
        "detail": {
            "aex_loaded": false,
            "blocked_path_substrings": blocked_by,
        },
        "elapsed_ms": 0,
    })
}

fn inventory_report(
    options: &Options,
    scan: &DiagnosticScan,
    build: &ReportBuildFingerprint,
    mut plugins: Vec<Value>,
    elapsed: Duration,
) -> Value {
    for plugin in &mut plugins {
        plugin["build"] = json!(build);
    }
    let mut buckets: BTreeMap<String, usize> = BTreeMap::new();
    for plugin in &plugins {
        let bucket = plugin["bucket"]
            .as_str()
            .unwrap_or("inventory_record_error");
        *buckets.entry(bucket.to_owned()).or_default() += 1;
    }
    json!({
        "schema_version": 1,
        "build": build,
        "scan": {
            "folder_count": scan.dirs.len(),
            "seen": scan.seen,
            "after_ignore": scan.plugins.len(),
            "swept": plugins.len(),
            "incomplete_reason": scan.incomplete_reason,
            "selection": {
                "filter": options.filter,
                "excluded_path_substrings": options.exclude_paths,
                "blocked_path_substrings": options.blocked_paths,
                "limit": options.limit,
                "skip": options.skip,
            },
            "folders": options.include_scan_paths.then(|| {
                scan.dirs
                    .iter()
                    .map(|root| root.to_string_lossy().into_owned())
                    .collect::<Vec<String>>()
            }),
        },
        "mode": "inventory_only",
        "render": Value::Null,
        "requested_render_conditions": {
            "width": options.width,
            "height": options.height,
            "pixel_format": options.pixel_format.report_name(),
            "current_time": options.current_time,
            "frames": options.frames,
            "secondary_layer": !options.no_layer,
            "force_classic": options.force_classic,
            "plugin_defaults": options.plugin_defaults,
            "effective_input_policy": effective_input_policy(options),
        },
        "discovery_elapsed_ms": 0,
        "elapsed_ms": elapsed.as_millis(),
        "buckets": buckets,
        "plugins": plugins,
    })
}

fn finish_report(options: &Options, report: &Value) -> Result<(), CliError> {
    if let Some(path) = &options.json {
        write_report(path, report).map_err(|error| {
            CliError::runtime(
                "report_publish_failed",
                "output",
                format!("{} could not be published: {error}", path.display()),
            )
        })?;
        let partial = partial_path(path);
        if let Err(error) = std::fs::remove_file(&partial)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(CliError::runtime(
                "partial_cleanup_failed",
                "output",
                format!(
                    "the final report was published but {} could not be removed: {error}",
                    partial.display()
                ),
            ));
        }
        eprintln!("wrote {}", path.display());
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
    Ok(())
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
#[derive(Clone)]
struct Outcome {
    bucket: String,
    detail: Map<String, Value>,
}

/// Optionally repeats a successful pixel-producing render in a fresh session.
/// The primary bucket remains the sweep result; repeat failures and differing
/// bytes are evidence beside it, not a reclassification of the plug-in.
fn verify_pixel_determinism<F>(primary: &mut Outcome, enabled: bool, repeat: F)
where
    F: FnOnce() -> Outcome,
{
    if !enabled {
        return;
    }
    if primary.bucket != "rendered" {
        primary.detail.insert(
            "pixel_determinism".to_owned(),
            json!({ "status": "not_applicable", "repeat_bucket": Value::Null }),
        );
        return;
    }

    let primary_hash = primary
        .detail
        .get("pixel_sha256")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let repeated = repeat();
    let repeat_hash = repeated
        .detail
        .get("pixel_sha256")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let primary_signature = pixel_run_signature(primary);
    let repeat_signature = pixel_run_signature(&repeated);
    let status = if repeated.bucket != "rendered" || primary_hash.is_none() || repeat_hash.is_none()
    {
        "repeat_failed"
    } else if primary_signature == repeat_signature {
        "deterministic"
    } else {
        "nondeterministic"
    };
    primary.detail.insert(
        "pixel_determinism".to_owned(),
        json!({
            "status": status,
            "repeat_bucket": repeated.bucket,
            "repeat_pixel_sha256": repeat_hash,
            "repeat_frames": repeated.detail.get("frames"),
        }),
    );
}

fn pixel_run_signature(outcome: &Outcome) -> Value {
    outcome.detail.get("frames").cloned().unwrap_or_else(|| {
        json!([{
            "frame_index": 0,
            "bucket": outcome.bucket,
            "pixel_sha256": outcome.detail.get("pixel_sha256"),
        }])
    })
}

impl Outcome {
    fn bare(bucket: &str) -> Self {
        Outcome {
            bucket: bucket.to_owned(),
            detail: Map::new(),
        }
    }
}

/// The secondary layer handed to a plug-in that declares one.  The shipping
/// multifilter maps AviUtl2's single virtual buffer to the first declared layer
/// slot; later layer parameters retain their PARAMS_SETUP defaults.  Mirroring
/// that route matters: populating every optional layer changes the effect's
/// meaning and can make a valid default render fail before its render selector.
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
        .take(1)
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

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut result = [0u8; 32];
    for (index, slot) in result.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(result)
}

fn companion_providers_for(
    effect: &DiagnosticDiscovery,
    records: &[DiagnosticDiscovery],
) -> Result<Vec<ApprovedCompanion>, &'static str> {
    let Some(parent) = effect.path.parent() else {
        return Ok(Vec::new());
    };
    let has_sibling_provider = records.iter().any(|provider| {
        provider.ok
            && provider.plugin_kind == aexcompat_aviutl2_multifilter::DiscoveredPluginKind::Aegp
            && !provider.provided_suites.is_empty()
            && provider.path.parent() == Some(parent)
    });
    if has_sibling_provider && !effect.companion_demand_probe_complete {
        return Err("companion_demand_probe_unresolved");
    }
    let mut result = Vec::new();
    let mut claimed = std::collections::BTreeSet::new();
    for provider in records {
        if !provider.ok
            || provider.plugin_kind != aexcompat_aviutl2_multifilter::DiscoveredPluginKind::Aegp
            || provider.provided_suites.is_empty()
            || provider.path.parent() != Some(parent)
        {
            continue;
        }
        let Some(expected_sha256) = decode_sha256(&provider.sha256) else {
            continue;
        };
        let suites: Option<Vec<_>> = provider
            .provided_suites
            .iter()
            .map(|suite| {
                Some(CompanionSuiteIdentity {
                    name: suite.name.clone(),
                    api_version: u32::try_from(suite.api_version).ok().filter(|v| *v != 0)?,
                    internal_version: u32::try_from(suite.internal_version).ok()?,
                })
            })
            .collect();
        let Some(suites) = suites else {
            continue;
        };
        let demanded: Vec<_> = suites
            .iter()
            .filter(|suite| {
                effect.demanded_suites.iter().any(|demand| {
                    demand.name == suite.name
                        && u32::try_from(demand.api_version).ok() == Some(suite.api_version)
                })
            })
            .cloned()
            .collect();
        if demanded.is_empty() {
            continue;
        }
        if demanded.iter().any(|suite| !claimed.insert(suite.clone())) {
            return Err("ambiguous_companion_suite_provider");
        }
        result.push(ApprovedCompanion {
            artifact: ApprovedImageArtifact {
                path: provider.path.clone(),
                expected_sha256,
                expected_size: provider.byte_size,
            },
            suites: demanded,
        });
    }
    result.sort_by(|left, right| left.artifact.path.cmp(&right.artifact.path));
    Ok(result)
}
fn sweep_one(
    repository: &Path,
    record: &DiagnosticDiscovery,
    records: &[DiagnosticDiscovery],
    options: &Options,
    corpus_index: usize,
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
    let companions = match companion_providers_for(record, records) {
        Ok(companions) => companions,
        Err(classification) => return Outcome::bare(classification),
    };

    let session_open_started = Instant::now();
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
        companions,
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
    let session_open_ms = session_open_started.elapsed().as_millis();
    let mut session = match session {
        Ok(session) => session,
        Err(error) => {
            let mut outcome = Outcome::bare("session_open_failed");
            outcome
                .detail
                .insert("session_open_error".to_owned(), json!(error.to_string()));
            outcome.detail.insert(
                "phase_elapsed_ms".to_owned(),
                json!({ "session_open": session_open_ms }),
            );
            return outcome;
        }
    };

    // More than one frame because a session is not a frame: the bridge renders
    // frames continuously into a live session, so an effect whose first frame
    // fails and whose second renders looks like a working effect there and like
    // a failing one to a sweep that only ever asks for frame 0.
    let frame_started = Instant::now();
    let mut frames: Vec<Outcome> = Vec::with_capacity(options.frames as usize);
    for frame_index in 0..options.frames {
        let plugin_stem = record
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("plugin");
        let dump = options.dump_frames.as_deref().map(|dir| FrameDump {
            dir,
            plugin_stem,
            plugin_sha256_prefix: record.sha256.get(..8).unwrap_or(&record.sha256),
            corpus_index,
            frame_index,
            format: match options.pixel_format {
                RenderPixelFormat::Argb8 => "argb8",
                RenderPixelFormat::Argb16 => "argb16",
                RenderPixelFormat::Argb32f => "argb32f",
            },
        });
        let frame = frame_outcome_dumping(
            session.render_frame_with_parameters(
                frame_index,
                options.current_time + (frame_index as i32) * TIME_STEP,
                input,
                parameters,
            ),
            dump,
            options.pixel_format,
        );
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
                .map(|(index, frame)| {
                    json!({
                        "frame_index": index,
                        "bucket": frame.bucket,
                        "pixel_sha256": frame.detail.get("pixel_sha256"),
                    })
                })
                .collect(),
        )
    });
    let frame_ms = frame_started.elapsed().as_millis();
    let mut outcome = frames.swap_remove(verdict);
    if let Some(per_frame) = per_frame {
        outcome.detail.insert("frames".to_owned(), per_frame);
    }

    let close_started = Instant::now();
    let close = session.close();
    let close_ms = close_started.elapsed().as_millis();
    let phase_elapsed = json!({
        "session_open": session_open_ms,
        "frames": frame_ms,
        "session_close": close_ms,
    });
    outcome
        .detail
        .insert("phase_elapsed_ms".to_owned(), phase_elapsed.clone());
    let close_clean = close.get("session_clean") == Some(&Value::Bool(true))
        && close.get("invalidated") == Some(&Value::Bool(false));
    let smart_output_untouched = smart
        && outcome
            .detail
            .get("host_failure_reason")
            .and_then(Value::as_str)
            == Some("smart_output_untouched");
    let smart_output_transparent = smart && outcome.bucket == "rendered_transparent";
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
            "bucket": outcome.bucket,
            "pixel_sha256": outcome.detail.get("pixel_sha256"),
            "nonzero_alpha_pixels": outcome.detail.get("nonzero_alpha_pixels"),
            "worker_classification": close.pointer("/worker/classification"),
            "invalidated": close.get("invalidated")
        })
    });
    let transparent_comparison_authorized =
        smart_output_transparent && validate_smart_transparent_close(&close).is_ok();
    attach_close(&mut outcome, close, options.close_report);
    if fallback_reason.is_some() && !fallback_authorized {
        outcome.detail.insert(
            "fallback_rejected".to_owned(),
            json!("smart_attempt_validation"),
        );
    }
    if transparent_comparison_authorized {
        let mut classic_options = options.clone();
        classic_options.force_classic = true;
        // Preserve the Smart dump as the primary evidence. The comparison is
        // represented by its hash/alpha/bucket and must not overwrite it with
        // Classic bytes under the same deterministic dump name.
        classic_options.dump_frames = None;
        attach_classic_comparison(&mut outcome, || {
            sweep_one(
                repository,
                record,
                records,
                &classic_options,
                corpus_index,
                input,
                layer_pixels,
            )
        });
    }
    if let Some(mut fallback) = orchestrate_sweep_classic_fallback(
        options,
        fallback_reason,
        fallback_authorized,
        smart_attempt_evidence,
        |classic_options| {
            sweep_one(
                repository,
                record,
                records,
                classic_options,
                corpus_index,
                input,
                layer_pixels,
            )
        },
    ) {
        fallback
            .detail
            .insert("smart_attempt_phase_elapsed_ms".to_owned(), phase_elapsed);
        return fallback;
    }
    if matches!(outcome.bucket.as_str(), "rendered" | "rendered_transparent") && !close_clean {
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

/// A fully transparent Smart frame is not publishable sweep evidence, but a
/// cleanly closed session is safe to abandon and replay through Classic. Keep
/// this stricter than a generic successful close: the replay must never hide a
/// frame error, invalidation, worker failure, or selector refusal.
fn validate_smart_transparent_close(close: &Value) -> Result<(), &'static str> {
    if close.get("render_path").and_then(Value::as_str) != Some("smart") {
        return Err("render_path");
    }
    if close.get("session_clean").and_then(Value::as_bool) != Some(true) {
        return Err("session_clean");
    }
    if close.get("invalidated").and_then(Value::as_bool) != Some(false) {
        return Err("invalidated");
    }
    if close.get("frames_ok").and_then(Value::as_u64).unwrap_or(0) == 0 {
        return Err("frames_ok");
    }
    if close.get("frames_errored").and_then(Value::as_u64) != Some(0) {
        return Err("frames_errored");
    }
    if close
        .pointer("/worker/classification")
        .and_then(Value::as_str)
        != Some("ok")
    {
        return Err("worker_classification");
    }
    if close.pointer("/worker/exit_code").and_then(Value::as_i64) != Some(0) {
        return Err("worker_exit_code");
    }
    if close
        .pointer("/final_report/smart_render_selector_dispatched")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err("smart_render_selector_dispatched");
    }
    if close
        .pointer("/final_report/smart_render_error")
        .and_then(Value::as_i64)
        != Some(0)
    {
        return Err("smart_render_error");
    }
    Ok(())
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
        // The inspect column ran and a selector refused (the one-shot exit
        // 20; the session path's `selector_error`). Which selector, and its
        // code, is what a fix is planned from, so it is the bucket (#1063).
        Some(20) => format!("exit_20_{}", selector_error_suffix(diagnostics)),
        Some(code) => format!("exit_{code}"),
        // No exit code: an unclassified session-path failure still names its
        // cause (`identity_changed`, `hash_unavailable`,
        // `inspected_report_unusable`) instead of folding into `unknown`.
        None => classification
            .or_else(|| {
                diagnostics
                    .and_then(|value| value.get("cluster_error_kind"))
                    .and_then(Value::as_str)
            })
            .unwrap_or("unknown")
            .to_owned(),
    }
}

/// Names the selector an exit-20 discovery failure stopped at, from the
/// worker's report fields: `unattributed` when the record does not carry them
/// (one written before the fields were recorded); else the first nonzero of
/// GLOBAL_SETUP / PARAMS_SETUP / GLOBAL_SETDOWN with its code; else the
/// parameter-count contract when the selectors all returned 0; else
/// `no_selector_error` (the worker refused for a reason the report fields do
/// not carry, e.g. the arbitrary defaults could not be disposed). A negative
/// field is the worker's "not invoked" sentinel (-1), never a plug-in code, so
/// it is skipped rather than named.
fn selector_error_suffix(diagnostics: Option<&Value>) -> String {
    let field = |name: &str| {
        diagnostics
            .and_then(|value| value.get(name))
            .and_then(Value::as_i64)
    };
    for (name, label) in [
        ("global_setup_error", "global_setup"),
        ("params_setup_error", "params_setup"),
        ("global_setdown_error", "global_setdown"),
    ] {
        match field(name) {
            Some(code) if code <= 0 => continue,
            Some(code) => return format!("{label}:{code}"),
            // A field the record never carried: nothing after it can be
            // read as "the earlier selectors passed".
            None => return "unattributed".to_owned(),
        }
    }
    match (field("reported_num_params"), field("parameter_count")) {
        (Some(reported), Some(count)) if reported != count + 1 => {
            format!("param_count_contract:{reported}_vs_{count}")
        }
        _ => "no_selector_error".to_owned(),
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
    // How the Premiere GPU-filter route ended, lifted out of `stage_events`
    // (issue #1271). The whole event list is too big for a 300-plug-in sweep,
    // but this one identifier is what a sweep reader needs: the route's faults
    // are contained, so a plug-in whose GPU route died on entry renders
    // through the PF path and lands in `rendered` with nothing else saying the
    // route was tried. Absent when the route never ran, and also when the
    // capped event list did not reach it: the cap is per session, so on a
    // many-frame session this names an early frame's outcome. The entry rule is
    // the same every frame, but neither the entry nor the outcome is: at 8/16
    // only a frame whose own PF selector answered 512 enters the route at all,
    // and a frame that enters can decline where an earlier one committed. So a
    // late decline can sit behind an early `committed` on a multi-frame
    // session.
    if let Some(reason) = diagnostics
        .and_then(|value| value.get("stage_events"))
        .and_then(|events| events.as_array())
        .and_then(|events| {
            events
                .iter()
                .rev()
                .find(|event| {
                    event.get("stage").and_then(Value::as_str) == Some("pr_gpu_route")
                        && event.get("state").and_then(Value::as_str) == Some("end")
                })
                .and_then(|event| event.pointer("/errors/reason"))
        })
    {
        worker.insert("pr_gpu_route".to_owned(), reason.clone());
    }
    // How the route was entered, when it was the PF CPU path's own refusal that
    // sent the frame there (issue #1283). Without this a `rendered` record
    // whose route committed cannot be told from one whose CPU path worked, so a
    // host callback refusal the GPU route then stood in for would be invisible
    // in the default record - and the default record is what a corpus
    // comparison reads. The worker only puts a reason on the `begin` line when
    // the retry is what entered the route, so its absence is the ordinary case.
    if let Some(reason) = diagnostics
        .and_then(|value| value.get("stage_events"))
        .and_then(|events| events.as_array())
        .and_then(|events| {
            events
                .iter()
                .rev()
                .find(|event| {
                    event.get("stage").and_then(Value::as_str) == Some("pr_gpu_route")
                        && event.get("state").and_then(Value::as_str) == Some("begin")
                })
                .and_then(|event| event.pointer("/errors/reason"))
        })
    {
        worker.insert("pr_gpu_route_entered_from".to_owned(), reason.clone());
    }
    // Why a frame with pixels can come out of a session whose plug-in rendered
    // nothing: a SmartFX PreRender that promises an empty `result_rect` gets
    // the effect's input copied into the output instead of an empty frame
    // (issue #1285), which puts it in `rendered` rather than `rendered_empty`.
    // `input_copied` means the copy happened; any other reason means the host
    // declined and the frame stayed empty. Named `_reason` because the worker
    // and session reports already carry a boolean `empty_result_passthrough`,
    // and one name holding a bool in one record and a string in another is how
    // a reader's filter silently matches nothing. Same per-session caveat as
    // `pr_gpu_route`: this names the last such frame the capped event list
    // reached.
    if let Some(reason) = diagnostics
        .and_then(|value| value.get("stage_events"))
        .and_then(|events| events.as_array())
        .and_then(|events| {
            events
                .iter()
                .rev()
                .find(|event| {
                    event.get("stage").and_then(Value::as_str)
                        == Some("smart_empty_result_passthrough")
                        && event.get("state").and_then(Value::as_str) == Some("end")
                })
                .and_then(|event| event.pointer("/errors/reason"))
        })
    {
        worker.insert("empty_result_passthrough_reason".to_owned(), reason.clone());
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

/// Records only session-wide facts from a clean shared close. The worker's
/// detailed close diagnostics describe whichever plug-in was active last, so
/// copying them onto every cluster member would falsely attribute one effect's
/// route and callback evidence to all of its neighbours.
fn attach_shared_cluster_close(outcome: &mut Outcome, close: &Value) {
    let mut worker = Map::new();
    for (key, value) in [
        ("classification", close.pointer("/worker/classification")),
        ("exit_code", close.pointer("/worker/exit_code")),
    ] {
        worker.insert(key.to_owned(), value.cloned().unwrap_or(Value::Null));
    }
    outcome.detail.insert("worker".to_owned(), worker.into());
    outcome
        .detail
        .insert("session_clean".to_owned(), close["session_clean"].clone());
    outcome.detail.insert(
        "invalidated_reason".to_owned(),
        close["invalidated_reason"].clone(),
    );
    if let Some(cluster) = outcome
        .detail
        .get_mut("cluster_session")
        .and_then(Value::as_object_mut)
    {
        cluster.insert("close_shared".to_owned(), Value::Bool(true));
    }
}

/// The unit tests' shape of `frame_outcome_dumping`: no dump.
#[cfg(test)]
fn frame_outcome(outcome: std::io::Result<FrameOutcome>) -> Outcome {
    frame_outcome_dumping(outcome, None, RenderPixelFormat::Argb8)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AlphaEvidence {
    nonzero: usize,
    invalid: usize,
}

fn alpha_evidence(pixels: &[u8], format: RenderPixelFormat) -> Option<AlphaEvidence> {
    let pixel_bytes = match format {
        RenderPixelFormat::Argb8 => 4,
        RenderPixelFormat::Argb16 => 8,
        RenderPixelFormat::Argb32f => 16,
    };
    if pixels.len() % pixel_bytes != 0 {
        return None;
    }
    let mut evidence = AlphaEvidence {
        nonzero: 0,
        invalid: 0,
    };
    match format {
        RenderPixelFormat::Argb8 => {
            evidence.nonzero = pixels.chunks_exact(4).filter(|pixel| pixel[3] != 0).count();
        }
        RenderPixelFormat::Argb16 => {
            evidence.nonzero = pixels
                .chunks_exact(8)
                .filter(|pixel| u16::from_le_bytes([pixel[6], pixel[7]]) != 0)
                .count();
        }
        RenderPixelFormat::Argb32f => {
            for pixel in pixels.chunks_exact(16) {
                let alpha = f32::from_le_bytes([pixel[12], pixel[13], pixel[14], pixel[15]]);
                if !alpha.is_finite() || alpha < 0.0 {
                    evidence.invalid += 1;
                } else if alpha > 0.0 {
                    evidence.nonzero += 1;
                }
            }
        }
    }
    Some(evidence)
}

/// Where a rendered frame's raw pixels go under `--dump-frames`: the directory,
/// the plug-in's file stem, a prefix of its SHA-256, its stable corpus index,
/// the frame index and the pixel-format tag for the file name. The corpus index
/// keeps byte-identical copies from different dependency roots from racing on
/// the same dump path when their lanes run concurrently.
struct FrameDump<'a> {
    dir: &'a Path,
    plugin_stem: &'a str,
    plugin_sha256_prefix: &'a str,
    corpus_index: usize,
    frame_index: u32,
    format: &'static str,
}

/// One frame's bucket and evidence. A session renders several and each is
/// classified the same way. With a `FrameDump` the pixels of a rendered frame
/// are also written out (an empty frame writes nothing: it is `rendered_empty`,
/// and there is nothing to compare).
fn frame_outcome_dumping(
    outcome: std::io::Result<FrameOutcome>,
    dump: Option<FrameDump<'_>>,
    format: RenderPixelFormat,
) -> Outcome {
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
                let alpha = alpha_evidence(&pixels, format);
                detail.insert(
                    "nonzero_alpha_pixels".to_owned(),
                    json!(alpha.map(|evidence| evidence.nonzero)),
                );
                detail.insert(
                    "invalid_alpha_pixels".to_owned(),
                    json!(alpha.map(|evidence| evidence.invalid)),
                );
                // The hash of the bytes the worker answered, so two runs (or a
                // run and an AE reference decoded to the same layout) can be
                // compared without carrying the pixels in the report.
                detail.insert(
                    "pixel_sha256".to_owned(),
                    json!(format!("{:x}", Sha256::digest(&pixels))),
                );
                if let Some(dump) = dump.filter(|_| !pixels.is_empty() && width != 0 && height != 0)
                {
                    let path = dump.dir.join(format!(
                        "{}.{}.r{}.f{}.{}x{}.{}",
                        dump.plugin_stem,
                        dump.plugin_sha256_prefix,
                        dump.corpus_index,
                        dump.frame_index,
                        width,
                        height,
                        dump.format
                    ));
                    match std::fs::write(&path, &pixels) {
                        Ok(()) => {
                            detail.insert(
                                "dumped_frame".to_owned(),
                                json!(path.file_name().and_then(|n| n.to_str()).unwrap_or("")),
                            );
                        }
                        Err(error) => {
                            detail.insert("dump_error".to_owned(), json!(error.to_string()));
                        }
                    }
                }
                // A rendered frame of no pixels is not a render: an effect that
                // answers 0x0 has produced nothing, and rounding it into
                // `rendered` is how a sweep reports progress it did not make.
                if pixels.is_empty() || width == 0 || height == 0 {
                    "rendered_empty".to_owned()
                } else if alpha.is_some_and(|evidence| evidence.invalid != 0) {
                    "rendered_invalid_alpha".to_owned()
                } else if alpha.is_some_and(|evidence| evidence.nonzero == 0) {
                    "rendered_transparent".to_owned()
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
    build: &ReportBuildFingerprint,
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
        "build": build,
        "category": record.category,
        "discovery": {
            "ok": record.ok,
            "plugin_kind": record.plugin_kind,
            "provided_suites": record.provided_suites,
            "demanded_suites": record.demanded_suites,
            "companion_demand_probe_complete": record.companion_demand_probe_complete,
            "smart": record.smart,
            "out_flags2": record.out_flags2,
            "smart_route_supported":
                smart_render_route_supported(record.smart, record.out_flags2),
            "parameter_count": record.parameters.len(),
            "layer_slots": layer_slots_of(&record.parameters),
            "failure_classification": record.failure_classification,
            "failure_diagnostics": record.failure_diagnostics,
            "cluster_fallback": record.cluster_fallback,
            "closure_identity_sha256": record.closure_identity_sha256.as_deref(),
        },
        "bucket": outcome.bucket,
        "detail": outcome.detail,
        "elapsed_ms": elapsed_ms,
    })
}

fn report(
    options: &Options,
    scan: &DiagnosticScan,
    build: &ReportBuildFingerprint,
    discovery_elapsed: Duration,
    elapsed: Duration,
    buckets: BTreeMap<String, usize>,
    mut plugins: Vec<Value>,
    dependency_lane_count: Option<usize>,
    render_shard_count: Option<usize>,
    effective_same_closure_render_jobs: Option<usize>,
    render_group_count: Option<usize>,
) -> Value {
    // Render partial rows are written as each plug-in completes and retain the
    // explicit pre-run candidate. Only the atomic final report can carry the
    // end-of-run verification, so replace every in-memory row here.
    for plugin in &mut plugins {
        plugin["build"] = json!(build);
    }
    let distinct_dependency_closure_count = plugins
        .iter()
        .map(|plugin| {
            plugin
                .pointer("/discovery/closure_identity_sha256")
                .and_then(Value::as_str)
                .unwrap_or("unresolved")
        })
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let render = (!options.discovery_only).then(|| {
        let mut render = json!({
            "width": options.width,
            "height": options.height,
            "pixel_format": options.pixel_format.report_name(),
            "current_time": options.current_time,
            "frames": options.frames,
            "secondary_layer": !options.no_layer,
            "force_classic": options.force_classic,
            "plugin_defaults": options.plugin_defaults,
            "effective_input_policy": effective_input_policy(options),
            "verify_pixel_determinism": options.verify_pixel_determinism,
            "render_jobs": options.render_jobs,
            "effective_render_jobs": options.render_jobs.min(render_shard_count.unwrap_or(0)),
            "requested_same_closure_render_jobs": options.same_closure_render_jobs,
            "effective_same_closure_render_jobs": effective_same_closure_render_jobs,
            "dependency_lane_count": dependency_lane_count,
            "render_shard_count": render_shard_count,
            "render_group_count": render_group_count,
            "distinct_dependency_closure_count": distinct_dependency_closure_count,
            "frame_deadline_ms": FRAME_DEADLINE.as_millis(),
            "time_step": TIME_STEP,
            "total_time": TOTAL_TIME,
            "time_scale": TIME_SCALE,
        });
        if options.dynamic_same_closure_groups {
            render.as_object_mut().unwrap().insert(
                "dynamic_group_schedule".to_owned(),
                json!({
                    "requested_mode": "closure_capped_ready_queue",
                    "effective_mode": "closure_capped_ready_queue",
                    "global_process_cap": options.render_jobs,
                    "requested_same_closure_cap": options.same_closure_render_jobs,
                    "effective_same_closure_cap": effective_same_closure_render_jobs,
                    "render_group_count": render_group_count,
                    "unresolved_closure_unit": "original_serial_lane",
                }),
            );
        }
        render
    });
    json!({
        "schema_version": 1,
        "build": build,
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
                "excluded_path_substrings": options.exclude_paths,
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

fn prepare_partial(path: &Path) -> Result<(), CliError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::runtime(
            "partial_prepare_failed",
            "output",
            format!(
                "stale partial report {} could not be removed before this run: {error}",
                path.display()
            ),
        )),
    }
}

fn after_partial_is_prepared<T>(
    path: Option<&Path>,
    action: impl FnOnce() -> T,
) -> Result<T, CliError> {
    if let Some(path) = path {
        prepare_partial(path)?;
    }
    Ok(action())
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
fn write_report(path: &Path, report: &Value) -> std::io::Result<()> {
    let temporary = path.with_extension("json.tmp");
    let serialized = serde_json::to_vec_pretty(report)
        .map_err(|error| std::io::Error::other(format!("report serialization failed: {error}")))?;
    if let Err(error) = std::fs::write(&temporary, &serialized) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(error) = replace_report_file(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
fn replace_report_file(temporary: &Path, path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let temporary = temporary
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: both buffers are NUL-terminated and remain alive for the call.
    if unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            path.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_report_file(temporary: &Path, path: &Path) -> std::io::Result<()> {
    std::fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aexcompat_aviutl2_multifilter::DiscoveredPluginKind;
    use aexcompat_broker::image_render::InteractiveParameter;

    #[test]
    fn effective_policy_matches_pixels_and_legacy_default_flag() {
        let mut options = discovery_options(PathBuf::new());
        let policy = effective_input_policy(&options);
        let bytes = primary_pixels(3, 2);
        assert_eq!(bytes.len(), 24);
        for pixel in bytes.chunks_exact(4) {
            assert_eq!(json!(pixel), policy["primary"]["rgba8"]);
        }
        assert_eq!(policy["parameter_assignments"], "none");
        options.plugin_defaults = true;
        assert_eq!(effective_input_policy(&options), policy);
        options.no_layer = true;
        let no_layer = effective_input_policy(&options);
        assert_eq!(no_layer["secondary_layer_selection"], "none");
        assert!(no_layer["secondary_pattern"].is_null());
        assert_eq!(
            policy["secondary_layer_selection"],
            "first_declared_layer_if_any"
        );
    }

    fn discovery_options(json: PathBuf) -> Options {
        Options {
            repository: None,
            input_rgba: None,
            json: Some(json),
            limit: None,
            skip: 0,
            render_jobs: 1,
            same_closure_render_jobs: 1,
            dynamic_same_closure_groups: false,
            filter: None,
            exclude_paths: Vec::new(),
            blocked_paths: Vec::new(),
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
            inventory_only: false,
            verify_pixel_determinism: false,
            dump_frames: None,
            dirs: Vec::new(),
        }
    }

    #[test]
    fn bounded_parallel_map_respects_the_job_cap_and_preserves_input_order() {
        use std::sync::Barrier;

        let items = [0usize, 1, 2, 3];
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let pair = Barrier::new(2);
        let output = bounded_parallel_map_ordered(&items, 2, |index, value| {
            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            // Both worker threads must be inside the operation together for
            // each pair. This proves actual overlap without a wall-clock
            // performance assertion that would be flaky on a loaded runner.
            pair.wait();
            if index % 2 == 0 {
                std::thread::yield_now();
            }
            active.fetch_sub(1, Ordering::SeqCst);
            value * 10
        });

        assert_eq!(peak.load(Ordering::SeqCst), 2);
        assert_eq!(output, vec![0, 10, 20, 30]);
    }

    #[test]
    fn bounded_parallel_map_does_not_create_empty_work_or_accept_zero_jobs() {
        let empty: Vec<u8> = bounded_parallel_map_ordered::<u8, u8, _>(&[], 3, |_, value| *value);
        assert!(empty.is_empty());
        assert!(
            std::panic::catch_unwind(|| {
                bounded_parallel_map_ordered(&[1u8], 0, |_, value| *value)
            })
            .is_err()
        );
    }

    fn dynamic_plan(
        closure_identity: Option<&str>,
        same_closure_index: usize,
        same_closure_count: usize,
        groups: &[&[usize]],
    ) -> RenderPlan {
        RenderPlan {
            groups: groups.iter().map(|group| group.to_vec()).collect(),
            same_closure_index,
            same_closure_count,
            closure_identity: closure_identity.map(str::to_owned),
        }
    }

    #[test]
    fn group_finalization_keeps_completed_members_and_emits_each_index_once_after_late_panic() {
        let plans = [dynamic_plan(Some("shared"), 0, 1, &[&[10, 11, 12, 13]])];
        let attempted = Mutex::new(Vec::new());
        let finalized_indices = Mutex::new(Vec::new());
        let finalized = dynamic_render_group_map_ordered_then(
            &plans,
            1,
            |_plan, _group, run| {
                catch_group_members_ordered(run, |index| {
                    attempted.lock().unwrap().push(index);
                    if index == 12 {
                        panic!("synthetic late member panic");
                    }
                    format!("rendered-{index}")
                })
            },
            |_plan, _group, run, pending| {
                let pending = match pending {
                    DynamicGroupResult::Completed(pending) => pending,
                    DynamicGroupResult::Panicked => run
                        .iter()
                        .map(|&index| (index, GroupMemberResult::Panicked))
                        .collect(),
                };
                finalize_group_members_ordered(pending, |index, result| {
                    finalized_indices.lock().unwrap().push(index);
                    match result {
                        GroupMemberResult::Completed(value) => value,
                        GroupMemberResult::Panicked => "sweep_panicked".to_owned(),
                    }
                })
            },
        );

        assert_eq!(*attempted.lock().unwrap(), vec![10, 11, 12, 13]);
        assert_eq!(
            finalized,
            vec![vec![vec![
                (10, "rendered-10".to_owned()),
                (11, "rendered-11".to_owned()),
                (12, "sweep_panicked".to_owned()),
                (13, "rendered-13".to_owned()),
            ]]]
        );
        assert_eq!(*finalized_indices.lock().unwrap(), vec![10, 11, 12, 13]);
        assert_eq!(
            finalized_indices
                .lock()
                .unwrap()
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            4,
            "no index may be finalized or appended twice"
        );
    }

    #[test]
    fn static_group_finalizes_each_member_before_starting_the_next() {
        use std::sync::mpsc::channel;

        let gate = (Mutex::new(false), Condvar::new());
        let gate_timeouts = AtomicUsize::new(0);
        let (event_tx, event_rx) = channel::<(&'static str, usize)>();

        let (events, results) = std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                map_group_members_immediate(
                    &[0, 1],
                    |index| {
                        event_tx.send(("started", index)).unwrap();
                        if index == 1 {
                            let (lock, changed) = &gate;
                            let mut released = lock.lock().unwrap();
                            while !*released {
                                let (next, timeout) = changed
                                    .wait_timeout(released, Duration::from_secs(5))
                                    .unwrap();
                                released = next;
                                if timeout.timed_out() {
                                    gate_timeouts.fetch_add(1, Ordering::SeqCst);
                                    break;
                                }
                            }
                        }
                        index * 10
                    },
                    |index, result| {
                        event_tx.send(("finalized", index)).unwrap();
                        result
                    },
                )
            });

            let events = (0..3)
                .filter_map(|_| event_rx.recv_timeout(Duration::from_secs(5)).ok())
                .collect::<Vec<_>>();
            *gate.0.lock().unwrap() = true;
            gate.1.notify_all();
            (events, handle.join().unwrap())
        });

        assert_eq!(
            events,
            vec![("started", 0), ("finalized", 0), ("started", 1)],
            "the static path must persist member zero before member one can block"
        );
        assert_eq!(gate_timeouts.load(Ordering::SeqCst), 0);
        assert_eq!(results, vec![(0, 0), (1, 10)]);
    }

    #[test]
    fn dynamic_group_finalizes_progress_before_a_following_group_unblocks() {
        use std::sync::mpsc::channel;

        let plans = [dynamic_plan(Some("shared"), 0, 1, &[&[0], &[1]])];
        let gate = (Mutex::new(false), Condvar::new());
        let gate_timeouts = AtomicUsize::new(0);
        let (event_tx, event_rx) = channel::<(&'static str, usize)>();

        let (events, results) = std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                dynamic_render_group_map_ordered_then(
                    &plans,
                    2,
                    |_plan, group, _| {
                        event_tx.send(("started", group)).unwrap();
                        if group == 1 {
                            let (lock, changed) = &gate;
                            let mut released = lock.lock().unwrap();
                            while !*released {
                                let (next, timeout) = changed
                                    .wait_timeout(released, Duration::from_secs(5))
                                    .unwrap();
                                released = next;
                                if timeout.timed_out() {
                                    gate_timeouts.fetch_add(1, Ordering::SeqCst);
                                    break;
                                }
                            }
                        }
                        group
                    },
                    |_plan, group, _run, result| {
                        event_tx.send(("finalized", group)).unwrap();
                        result
                    },
                )
            });

            let events = (0..3)
                .filter_map(|_| event_rx.recv_timeout(Duration::from_secs(5)).ok())
                .collect::<Vec<_>>();
            *gate.0.lock().unwrap() = true;
            gate.1.notify_all();
            (events, handle.join().unwrap())
        });

        assert_eq!(
            events,
            vec![("started", 0), ("finalized", 0), ("started", 1)],
            "the completed group's durable-progress callback must run while the next group is still blocked"
        );
        assert_eq!(gate_timeouts.load(Ordering::SeqCst), 0);
        assert_eq!(
            results,
            vec![vec![
                DynamicGroupResult::Completed(0),
                DynamicGroupResult::Completed(1),
            ]]
        );
    }

    #[test]
    fn dynamic_group_finalize_panic_returns_its_permit_before_failing_closed() {
        use std::sync::Barrier;

        let plans = [
            dynamic_plan(Some("a"), 0, 1, &[&[0]]),
            dynamic_plan(Some("b"), 0, 1, &[&[1]]),
        ];
        let both_active = Barrier::new(2);
        let result = std::panic::catch_unwind(|| {
            dynamic_render_group_map_ordered_then(
                &plans,
                2,
                |plan, group, _| {
                    both_active.wait();
                    (plan, group)
                },
                |plan, _group, _run, result| {
                    if plan == 0 {
                        panic!("synthetic finalization panic");
                    }
                    result
                },
            )
        });
        let panic = result.expect_err("finalizer panic must fail the scheduler closed");
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&'static str>().copied())
            .unwrap_or("non-string panic");
        assert_eq!(message, "dynamic render group finalization panicked");
    }

    #[test]
    fn dynamic_group_scheduler_steals_later_work_while_an_earlier_group_is_busy() {
        use std::sync::Barrier;

        let plans = [
            dynamic_plan(Some("shared"), 0, 2, &[&[0], &[1]]),
            dynamic_plan(Some("shared"), 1, 2, &[&[2]]),
        ];
        let first_wave = Barrier::new(2);
        let release_early_group = (Mutex::new(false), Condvar::new());
        let later_group_ran = AtomicUsize::new(0);

        let results = dynamic_render_group_map_ordered(&plans, 2, |plan, group, _| {
            if group == 0 {
                first_wave.wait();
            }
            match (plan, group) {
                (0, 0) => {
                    let (lock, changed) = &release_early_group;
                    let mut released = lock.lock().unwrap();
                    while !*released {
                        let (next, timeout) = changed
                            .wait_timeout(released, Duration::from_secs(5))
                            .unwrap();
                        assert!(!timeout.timed_out(), "later ready work was not stolen");
                        released = next;
                    }
                }
                (0, 1) => {
                    later_group_ran.store(1, Ordering::SeqCst);
                    let (lock, changed) = &release_early_group;
                    *lock.lock().unwrap() = true;
                    changed.notify_all();
                }
                _ => {}
            }
            (plan, group)
        });

        assert_eq!(later_group_ran.load(Ordering::SeqCst), 1);
        assert_eq!(
            results,
            vec![
                vec![
                    DynamicGroupResult::Completed((0, 0)),
                    DynamicGroupResult::Completed((0, 1)),
                ],
                vec![DynamicGroupResult::Completed((1, 0))],
            ]
        );
    }

    #[test]
    fn dynamic_group_scheduler_enforces_global_and_per_closure_caps() {
        use std::collections::BTreeSet;
        use std::sync::mpsc::{RecvTimeoutError, channel};

        let plans = [
            dynamic_plan(Some("a"), 0, 3, &[&[0], &[3]]),
            dynamic_plan(Some("a"), 1, 3, &[&[1], &[4]]),
            dynamic_plan(Some("a"), 2, 3, &[&[2], &[5]]),
            dynamic_plan(Some("b"), 0, 1, &[&[6], &[7], &[8]]),
        ];
        let gate = (Mutex::new((BTreeSet::new(), false)), Condvar::new());
        let (started_tx, started_rx) = channel::<(usize, usize)>();
        let global_active = AtomicUsize::new(0);
        let global_peak = AtomicUsize::new(0);
        let closure_a_active = AtomicUsize::new(0);
        let closure_a_peak = AtomicUsize::new(0);
        let gate_timeouts = AtomicUsize::new(0);

        let observations = std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                dynamic_render_group_map_ordered(&plans, 4, |plan, group, _| {
                    let task = (plan, group);
                    let global_now = global_active.fetch_add(1, Ordering::SeqCst) + 1;
                    global_peak.fetch_max(global_now, Ordering::SeqCst);
                    if plan < 3 {
                        let closure_now = closure_a_active.fetch_add(1, Ordering::SeqCst) + 1;
                        closure_a_peak.fetch_max(closure_now, Ordering::SeqCst);
                    }
                    started_tx.send(task).unwrap();

                    let (lock, changed) = &gate;
                    let mut state = lock.lock().unwrap();
                    while !state.1 && !state.0.contains(&task) {
                        let (next, timeout) =
                            changed.wait_timeout(state, Duration::from_secs(5)).unwrap();
                        state = next;
                        if timeout.timed_out() {
                            gate_timeouts.fetch_add(1, Ordering::SeqCst);
                            break;
                        }
                    }
                    drop(state);

                    if plan < 3 {
                        closure_a_active.fetch_sub(1, Ordering::SeqCst);
                    }
                    global_active.fetch_sub(1, Ordering::SeqCst);
                    task
                })
            });

            let initial = (0..4)
                .filter_map(|_| started_rx.recv_timeout(Duration::from_secs(5)).ok())
                .collect::<BTreeSet<_>>();
            let global_cap_held = matches!(
                started_rx.recv_timeout(Duration::from_millis(100)),
                Err(RecvTimeoutError::Timeout)
            );

            let release = |task| {
                gate.0.lock().unwrap().0.insert(task);
                gate.1.notify_all();
            };
            release((3, 0));
            let after_b0 = started_rx.recv_timeout(Duration::from_secs(5)).ok();
            release((3, 1));
            let after_b1 = started_rx.recv_timeout(Duration::from_secs(5)).ok();
            let caps_held = matches!(
                started_rx.recv_timeout(Duration::from_millis(100)),
                Err(RecvTimeoutError::Timeout)
            );

            release((3, 2));
            let closure_cap_held = matches!(
                started_rx.recv_timeout(Duration::from_millis(100)),
                Err(RecvTimeoutError::Timeout)
            );
            release((0, 0));
            let after_a0 = started_rx.recv_timeout(Duration::from_secs(5)).ok();

            {
                let mut state = gate.0.lock().unwrap();
                state.1 = true;
            }
            gate.1.notify_all();
            let results = handle.join().unwrap();
            (
                initial,
                global_cap_held,
                after_b0,
                after_b1,
                caps_held,
                closure_cap_held,
                after_a0,
                results,
            )
        });

        let (
            initial,
            global_cap_held,
            after_b0,
            after_b1,
            caps_held,
            closure_cap_held,
            after_a0,
            results,
        ) = observations;
        assert_eq!(initial, BTreeSet::from([(0, 0), (1, 0), (2, 0), (3, 0)]));
        assert!(global_cap_held, "a fifth task exceeded the global cap");
        assert_eq!(
            after_b0,
            Some((3, 1)),
            "a fourth same-closure task bypassed the three-permit cap"
        );
        assert_eq!(after_b1, Some((3, 2)));
        assert!(caps_held, "work started while all four permits were held");
        assert!(
            closure_cap_held,
            "same-closure work started while all three closure permits were held"
        );
        assert!(matches!(after_a0, Some((plan, 1)) if plan < 3));
        assert_eq!(global_peak.load(Ordering::SeqCst), 4);
        assert_eq!(closure_a_peak.load(Ordering::SeqCst), 3);
        assert_eq!(gate_timeouts.load(Ordering::SeqCst), 0);
        let results = results.into_iter().flatten().collect::<Vec<_>>();
        assert_eq!(results.len(), 9);
        assert!(
            results
                .into_iter()
                .all(|result| matches!(result, DynamicGroupResult::Completed((_plan, _group))))
        );
    }

    #[test]
    fn dynamic_group_scheduler_keeps_unresolved_lane_as_one_serial_unit() {
        let plans = [dynamic_plan(None, 0, 1, &[&[0], &[1], &[2]])];
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let order = Mutex::new(Vec::new());

        let results = dynamic_render_group_map_ordered(&plans, 3, |_plan, group, _| {
            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            order.lock().unwrap().push(group);
            active.fetch_sub(1, Ordering::SeqCst);
            group
        });

        assert_eq!(peak.load(Ordering::SeqCst), 1);
        assert_eq!(*order.lock().unwrap(), vec![0, 1, 2]);
        assert_eq!(
            results,
            vec![vec![
                DynamicGroupResult::Completed(0),
                DynamicGroupResult::Completed(1),
                DynamicGroupResult::Completed(2),
            ]]
        );
    }

    #[test]
    fn dynamic_group_scheduler_releases_permit_after_panic_and_finishes_following_work() {
        let plans = [dynamic_plan(Some("shared"), 0, 1, &[&[0], &[1]])];
        let calls = AtomicUsize::new(0);
        let results = dynamic_render_group_map_ordered(&plans, 2, |_plan, group, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            if group == 0 {
                panic!("synthetic render-group panic");
            }
            group
        });

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            results,
            vec![vec![
                DynamicGroupResult::Panicked,
                DynamicGroupResult::Completed(1),
            ]]
        );
    }

    #[test]
    fn dynamic_group_scheduler_preserves_final_order_and_missing_duplicate_checks() {
        let plans = [
            dynamic_plan(Some("shared"), 0, 2, &[&[4, 0], &[3]]),
            dynamic_plan(Some("shared"), 1, 2, &[&[5, 1], &[2]]),
        ];
        let scheduled = dynamic_render_group_map_ordered(&plans, 2, |_plan, _group, run| {
            run.iter()
                .map(|&index| (index, index * 10))
                .collect::<Vec<_>>()
        });
        let grouped = scheduled
            .into_iter()
            .map(|groups| {
                groups
                    .into_iter()
                    .flat_map(|result| match result {
                        DynamicGroupResult::Completed(group) => group,
                        DynamicGroupResult::Panicked => Vec::new(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            restore_indexed_order(6, grouped),
            vec![0, 10, 20, 30, 40, 50]
        );

        let duplicate = [dynamic_plan(Some("duplicate"), 0, 1, &[&[0], &[0]])];
        let duplicate = dynamic_render_group_map_ordered(&duplicate, 2, |_, _, run| {
            run.iter().map(|&index| (index, index)).collect::<Vec<_>>()
        })
        .into_iter()
        .map(|groups| {
            groups
                .into_iter()
                .flat_map(|result| match result {
                    DynamicGroupResult::Completed(group) => group,
                    DynamicGroupResult::Panicked => Vec::new(),
                })
                .collect::<Vec<_>>()
        })
        .collect();
        assert!(
            std::panic::catch_unwind(|| restore_indexed_order(2, duplicate)).is_err(),
            "a dynamically scheduled duplicate must not be hidden"
        );

        let missing = [dynamic_plan(Some("missing"), 0, 1, &[&[0]])];
        let missing = dynamic_render_group_map_ordered(&missing, 2, |_, _, run| {
            run.iter().map(|&index| (index, index)).collect::<Vec<_>>()
        })
        .into_iter()
        .map(|groups| {
            groups
                .into_iter()
                .flat_map(|result| match result {
                    DynamicGroupResult::Completed(group) => group,
                    DynamicGroupResult::Panicked => Vec::new(),
                })
                .collect::<Vec<_>>()
        })
        .collect();
        assert!(
            std::panic::catch_unwind(|| restore_indexed_order(2, missing)).is_err(),
            "a dynamically scheduled missing result must not be hidden"
        );
    }

    #[test]
    fn dynamic_group_scheduler_preserves_empty_plan_coordinates() {
        let plans = [dynamic_plan(Some("empty"), 0, 1, &[])];
        let results = dynamic_render_group_map_ordered(&plans, 3, |_, _, _| unreachable!());
        assert_eq!(results, vec![Vec::<DynamicGroupResult<()>>::new()]);
    }

    #[test]
    fn dynamic_group_scheduler_rejects_invalid_or_inconsistent_closure_caps() {
        let no_permit = [dynamic_plan(Some("shared"), 0, 0, &[&[0]])];
        assert!(
            std::panic::catch_unwind(|| {
                dynamic_render_group_map_ordered(&no_permit, 1, |_, _, _| ())
            })
            .is_err()
        );

        let inconsistent = [
            dynamic_plan(Some("shared"), 0, 1, &[&[0]]),
            dynamic_plan(Some("shared"), 1, 2, &[&[1]]),
        ];
        assert!(
            std::panic::catch_unwind(|| {
                dynamic_render_group_map_ordered(&inconsistent, 2, |_, _, _| ())
            })
            .is_err()
        );
    }

    #[test]
    fn target_filters_apply_filename_inclusion_and_repeatable_path_exclusions() {
        let excluded = vec!["\\maxon\\".to_owned(), "\\sapphire\\".to_owned()];
        assert!(matches_target_filters(
            Path::new(r"C:\Plug-ins\Other\Glow.aex"),
            Some("glow"),
            &excluded,
        ));
        assert!(!matches_target_filters(
            Path::new(r"C:\Plug-ins\MAXON\Glow.aex"),
            Some("glow"),
            &excluded,
        ));
        assert!(!matches_target_filters(
            Path::new(r"C:\Plug-ins\Sapphire\Blur.aex"),
            None,
            &excluded,
        ));
        assert!(!matches_target_filters(
            Path::new(r"C:\Plug-ins\Other\Blur.aex"),
            Some("glow"),
            &excluded,
        ));
    }

    #[test]
    fn dependency_identity_lanes_serialize_equals_and_restore_corpus_order() {
        let keys = [Some("shared"), Some("other"), Some("shared"), None, None];
        assert_eq!(
            serial_lanes_by_key(&keys, 1, |key| *key),
            vec![vec![0, 1, 2, 3, 4]],
            "the default one-job path preserves historical execution order"
        );
        let lanes = serial_lanes_by_key(&keys, 2, |key| *key);
        assert_eq!(lanes, vec![vec![0, 2], vec![1], vec![3, 4]]);

        let completion_order = vec![
            vec![(1, "one")],
            vec![(3, "three"), (4, "four")],
            vec![(0, "zero"), (2, "two")],
        ];
        assert_eq!(
            restore_indexed_order(5, completion_order),
            vec!["zero", "one", "two", "three", "four"]
        );
    }

    #[test]
    fn same_closure_shards_balance_ten_without_duplicates_and_keep_default_lanes() {
        let keys = [Some("shared"); 10];
        let historical = serial_lanes_by_key(&keys, 2, |key| *key);
        let default = dependency_render_shards(&keys, 2, 1, |key| *key);
        assert_eq!(
            default
                .iter()
                .map(|shard| shard.indices.clone())
                .collect::<Vec<_>>(),
            historical,
            "the default must retain the exact historical lane membership"
        );
        assert_eq!(default[0].same_closure_index, 0);
        assert_eq!(default[0].same_closure_count, 1);

        let split = dependency_render_shards(&keys, 2, 2, |key| *key);
        assert_eq!(split.len(), 2);
        assert_eq!(split[0].indices, (0..5).collect::<Vec<_>>());
        assert_eq!(split[1].indices, (5..10).collect::<Vec<_>>());
        assert_eq!(split[0].same_closure_index, 0);
        assert_eq!(split[1].same_closure_index, 1);
        assert!(
            split
                .iter()
                .all(|shard| { shard.same_closure_count == 2 && shard.closure_identity_resolved })
        );

        let scheduled = split
            .iter()
            .flat_map(|shard| shard.indices.iter().copied())
            .collect::<Vec<_>>();
        assert_eq!(scheduled, (0..10).collect::<Vec<_>>());
        assert_eq!(
            scheduled
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            scheduled.len(),
            "no corpus index may be assigned to more than one shard"
        );
    }

    #[test]
    fn same_closure_shards_keep_multiple_keys_separate_and_none_serial() {
        let keys = [
            Some("a"),
            None,
            Some("a"),
            Some("b"),
            None,
            Some("a"),
            Some("b"),
        ];
        for render_jobs in [1, 3] {
            let historical = serial_lanes_by_key(&keys, render_jobs, |key| *key);
            let default = dependency_render_shards(&keys, render_jobs, 1, |key| *key)
                .into_iter()
                .map(|shard| shard.indices)
                .collect::<Vec<_>>();
            assert_eq!(
                default, historical,
                "default sharding changed the historical {render_jobs}-job plan"
            );
        }
        let shards = dependency_render_shards(&keys, 3, 2, |key| *key);
        assert_eq!(
            shards,
            vec![
                DependencyRenderShard {
                    indices: vec![0, 2],
                    same_closure_index: 0,
                    same_closure_count: 2,
                    closure_identity_resolved: true,
                },
                DependencyRenderShard {
                    indices: vec![5],
                    same_closure_index: 1,
                    same_closure_count: 2,
                    closure_identity_resolved: true,
                },
                DependencyRenderShard {
                    indices: vec![1, 4],
                    same_closure_index: 0,
                    same_closure_count: 1,
                    closure_identity_resolved: false,
                },
                DependencyRenderShard {
                    indices: vec![3],
                    same_closure_index: 0,
                    same_closure_count: 2,
                    closure_identity_resolved: true,
                },
                DependencyRenderShard {
                    indices: vec![6],
                    same_closure_index: 1,
                    same_closure_count: 2,
                    closure_identity_resolved: true,
                },
            ]
        );
    }

    #[test]
    fn same_closure_shard_bounds_and_order_restoration_fail_closed() {
        let keys = [Some("shared")];
        assert!(
            std::panic::catch_unwind(|| { dependency_render_shards(&keys, 2, 0, |key| *key) })
                .is_err()
        );
        assert!(
            std::panic::catch_unwind(|| { dependency_render_shards(&keys, 2, 3, |key| *key) })
                .is_err()
        );
        assert!(
            std::panic::catch_unwind(|| { restore_indexed_order(2, vec![vec![(0, 10), (0, 20)]]) })
                .is_err(),
            "a duplicate result must not be hidden"
        );
        assert!(
            std::panic::catch_unwind(|| restore_indexed_order(2, vec![vec![(0, 10)]])).is_err(),
            "a missing result must not be hidden"
        );
    }

    #[test]
    fn render_report_records_requested_effective_and_per_record_shard_evidence() {
        let mut options = discovery_options(PathBuf::from("unused.json"));
        options.discovery_only = false;
        options.render_jobs = 2;
        options.same_closure_render_jobs = 2;
        let scan = DiagnosticScan {
            dirs: vec![PathBuf::from("root")],
            plugins: Vec::new(),
            seen: 1,
            dependency_dirs: Vec::new(),
            incomplete_reason: None,
        };
        let build = capture_report_build_fingerprint(Path::new("missing"), Err(()));
        let mut plugin = json!({
            "discovery": { "closure_identity_sha256": "shared" },
            "bucket": "rendered",
            "build": Value::Null,
        });
        attach_same_closure_shard_evidence(&mut plugin, 1, 2);
        let static_report = report(
            &options,
            &scan,
            &build,
            Duration::from_millis(3),
            Duration::from_millis(5),
            BTreeMap::from([("rendered".to_owned(), 1)]),
            vec![plugin],
            Some(1),
            Some(2),
            Some(2),
            Some(3),
        );

        assert_eq!(static_report["render"]["render_jobs"], 2);
        assert_eq!(static_report["render"]["effective_render_jobs"], 2);
        assert_eq!(
            static_report["render"]["requested_same_closure_render_jobs"],
            2
        );
        assert_eq!(
            static_report["render"]["effective_same_closure_render_jobs"],
            2
        );
        assert_eq!(static_report["render"]["dependency_lane_count"], 1);
        assert_eq!(static_report["render"]["render_shard_count"], 2);
        assert_eq!(
            static_report["plugins"][0]["same_closure_shard"]["index"],
            1
        );
        assert_eq!(
            static_report["plugins"][0]["same_closure_shard"]["count"],
            2
        );
        assert!(
            static_report["render"]
                .get("dynamic_group_schedule")
                .is_none()
        );
        assert!(
            static_report["plugins"][0]
                .get("render_work_group")
                .is_none()
        );

        options.dynamic_same_closure_groups = true;
        let mut dynamic_plugin = static_report["plugins"][0].clone();
        attach_render_work_group_evidence(&mut dynamic_plugin, 2, 3);
        let dynamic_report = report(
            &options,
            &scan,
            &build,
            Duration::from_millis(3),
            Duration::from_millis(5),
            BTreeMap::from([("rendered".to_owned(), 1)]),
            vec![dynamic_plugin],
            Some(1),
            Some(2),
            Some(2),
            Some(3),
        );
        assert_eq!(
            dynamic_report["render"]["dynamic_group_schedule"]["requested_mode"],
            "closure_capped_ready_queue"
        );
        assert_eq!(
            dynamic_report["render"]["dynamic_group_schedule"]["effective_mode"],
            "closure_capped_ready_queue"
        );
        assert_eq!(
            dynamic_report["render"]["dynamic_group_schedule"]["global_process_cap"],
            2
        );
        assert_eq!(
            dynamic_report["render"]["dynamic_group_schedule"]["render_group_count"],
            3
        );
        assert_eq!(
            dynamic_report["plugins"][0]["same_closure_shard"],
            json!({"index": 1, "count": 2})
        );
        assert_eq!(
            dynamic_report["plugins"][0]["render_work_group"],
            json!({"index": 2, "count": 3})
        );
    }

    #[test]
    fn primary_image_decodes_exact_rgba_and_rejects_invalid_inputs() {
        let path = std::env::temp_dir().join(format!("aex-sweep-input-{}.png", std::process::id()));
        // Two independently encoded RGBA pixels, including transparent RGB.
        let png = [
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 244, 34, 127, 138, 0, 0, 0, 17, 73, 68, 65, 84, 120, 156, 99, 224, 81,
            178, 248, 31, 21, 224, 198, 0, 0, 10, 134, 2, 86, 235, 24, 252, 245, 0, 0, 0, 0, 73,
            69, 78, 68, 174, 66, 96, 130,
        ];
        std::fs::write(&path, png).unwrap();
        let pixels = load_primary_image(&path, 2, 1).unwrap();
        assert_eq!(pixels, [12, 34, 56, 255, 90, 80, 70, 0]);
        assert!(load_primary_image(&path, 1, 2).is_err());
        let mut options = discovery_options(PathBuf::new());
        options.width = 2;
        options.input_rgba = Some(pixels.clone());
        let policy = effective_input_policy(&options);
        assert_eq!(policy["primary"]["pattern"], "decoded_image");
        assert_eq!(
            policy["primary"]["pixel_sha256"],
            format!("{:x}", Sha256::digest(&pixels))
        );
        std::fs::write(&path, b"not an image").unwrap();
        assert!(load_primary_image(&path, 2, 1).is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(load_primary_image(&path, 2, 1).is_err());
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
            provided_suites: Vec::new(),
            demanded_suites: Vec::new(),
            companion_demand_probe_complete: false,
            search_roots: Vec::new(),
            closure_identity_sha256: None,
            failure_classification: Some("nonzero_exit".to_owned()),
            failure_diagnostics: Some(json!({
                "classification": "nonzero_exit",
                "exit_code": 11,
            })),
            cluster_fallback: Some("worker_exited/invalidated".to_owned()),
        }
    }

    fn parameter(slot: u32, kind: &str) -> InteractiveParameter {
        InteractiveParameter {
            slot,
            name: format!("parameter {slot}"),
            kind: kind.to_owned(),
            minimum: 0.0,
            maximum: 1.0,
            value: 0.0,
            choices: Vec::new(),
            color: [0; 4],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }
    }

    fn write_build_layout(root: &Path) -> PathBuf {
        let cli = root.join("render_sweep.exe");
        std::fs::write(&cli, b"render-sweep-v1").unwrap();
        for (kind, bytes) in [
            (WorkerKind::Discovery, b"l2-v1".as_slice()),
            (WorkerKind::Classic, b"classic-v1".as_slice()),
            (WorkerKind::Smart, b"smart-v1".as_slice()),
        ] {
            let worker = root.join(kind.repository_relative_program());
            std::fs::create_dir_all(worker.parent().unwrap()).unwrap();
            std::fs::write(worker, bytes).unwrap();
        }
        cli
    }

    #[test]
    fn build_fingerprint_tracks_worker_bytes_and_fails_open_identity_explicitly() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-render-sweep-build-{}-{nonce:032x}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let cli = write_build_layout(&root);

        let first = capture_report_build_fingerprint(&root, Ok(cli.clone()));
        assert!(!first.complete);
        assert_eq!(
            first.scope,
            "executable_path_boundary_snapshot_not_launch_receipt"
        );
        assert_eq!(first.verification, "pre_run_candidate");
        assert!(first.smart_worker.error.is_none());
        assert_eq!(first.smart_worker.size_bytes, Some(8));

        let stable = finalize_report_build_fingerprint(&first, &root, Ok(cli.clone()));
        assert!(stable.complete);
        assert_eq!(stable.verification, "run_boundary_verified");

        let smart = root.join(WorkerKind::Smart.repository_relative_program());
        std::fs::write(&smart, b"smart-v2-changed").unwrap();
        let changed = finalize_report_build_fingerprint(&first, &root, Ok(cli.clone()));
        assert!(!changed.complete);
        assert_eq!(changed.verification, "run_boundary_incomplete");
        assert_eq!(changed.smart_worker.sha256, None);
        assert_eq!(
            changed.smart_worker.error,
            Some("changed_between_boundary_snapshots")
        );
        assert_eq!(first.cli, changed.cli);
        // All routes share one worker image: changing it invalidates every
        // route fingerprint, while the independent CLI image stays unchanged.
        assert_eq!(changed.l2_worker, changed.smart_worker);
        assert_eq!(changed.classic_worker, changed.smart_worker);

        // Boundary evidence deliberately cannot prove continuous identity or
        // actual per-launch admission. Restore the candidate bytes and pin that
        // limitation in the machine-readable scope instead of overclaiming.
        std::fs::write(&smart, b"smart-v1").unwrap();
        let restored = finalize_report_build_fingerprint(&first, &root, Ok(cli.clone()));
        assert!(restored.complete);
        assert_eq!(restored.verification, "run_boundary_verified");
        assert_eq!(
            restored.scope,
            "executable_path_boundary_snapshot_not_launch_receipt"
        );

        std::fs::remove_file(&smart).unwrap();
        let missing_start = capture_report_build_fingerprint(&root, Ok(cli.clone()));
        let missing = finalize_report_build_fingerprint(&missing_start, &root, Ok(cli));
        assert!(!missing.complete);
        assert_eq!(missing.smart_worker.sha256, None);
        assert_eq!(missing.smart_worker.size_bytes, None);
        assert_eq!(missing.smart_worker.error, Some("open_failed"));
        assert_eq!(missing.l2_worker, missing.smart_worker);
        assert_eq!(missing.classic_worker, missing.smart_worker);
        let serialized = serde_json::to_string(&missing).unwrap();
        assert!(
            !serialized.contains(&root.to_string_lossy().to_string()),
            "the shareable fingerprint must not disclose its source paths"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sweep_maps_the_single_shipping_virtual_buffer_to_only_the_first_layer_slot() {
        let mut record = failed_discovery(PathBuf::from("multiple-layers.aex"));
        record.ok = true;
        record.parameters = vec![
            parameter(1, "float"),
            parameter(3, "layer"),
            parameter(8, "layer"),
            parameter(13, "layer"),
        ];
        let mut options = discovery_options(PathBuf::from("unused.json"));
        options.discovery_only = false;
        let pixels = vec![0x7f; 4];

        let layers = probe_layers(&record, &options, &pixels);

        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].slot, 3);
        assert_eq!(layers[0].rgba, pixels);
        assert!(layers[0].dynamic);

        options.no_layer = true;
        assert!(probe_layers(&record, &options, &[0; 4]).is_empty());
    }

    #[test]
    fn cluster_fast_path_tracks_the_shipping_layer_slot_and_verification_stays_enabled() {
        let mut record = failed_discovery(PathBuf::from("smart.aex"));
        record.ok = true;
        record.smart = true;
        record.closure_identity_sha256 = Some("closure".to_owned());
        record.failure_classification = None;
        record.failure_diagnostics = None;
        assert!(cluster_fast_path_eligible(&record));

        let mut options = discovery_options(PathBuf::from("unused.json"));
        options.discovery_only = false;
        assert_eq!(cluster_layer_slot(&record, &options), None);

        record.parameters.push(parameter(3, "layer"));
        record.parameters.push(parameter(8, "layer"));
        assert!(cluster_fast_path_eligible(&record));
        assert_eq!(cluster_layer_slot(&record, &options), Some(3));
        options.no_layer = true;
        assert_eq!(cluster_layer_slot(&record, &options), None);

        record.plugin_kind = DiscoveredPluginKind::Aegp;
        assert!(!cluster_fast_path_eligible(&record));

        options.no_layer = false;
        assert!(cluster_fast_path_enabled(&options));
        options.verify_pixel_determinism = true;
        assert!(cluster_fast_path_enabled(&options));
        options.frames = 2;
        assert!(!cluster_fast_path_enabled(&options));
    }

    #[test]
    fn cluster_groups_join_interleaved_exact_keys_and_keep_safe_boundaries() {
        let eligible = |name: &str, closure: &str, root: &str, layer: Option<u32>| {
            let mut record = failed_discovery(PathBuf::from(name));
            record.ok = true;
            record.smart = true;
            record.closure_identity_sha256 = Some(closure.to_owned());
            record.search_roots = vec![PathBuf::from(root)];
            record.failure_classification = None;
            record.failure_diagnostics = None;
            if let Some(slot) = layer {
                record.parameters.push(parameter(slot, "layer"));
            }
            record
        };
        let mut records = vec![
            eligible("a.aex", "same", "root-a", Some(3)),
            eligible("b.aex", "same", "root-a", Some(8)),
            eligible("c.aex", "other", "root-b", Some(3)),
            eligible("d.aex", "same", "root-a", Some(3)),
            failed_discovery(PathBuf::from("not-smart.aex")),
            eligible("e.aex", "same", "root-a", Some(8)),
            eligible("f.aex", "same", "root-b", Some(3)),
        ];
        let lane = (0..records.len()).collect::<Vec<_>>();
        let mut options = discovery_options(PathBuf::from("unused.json"));
        options.discovery_only = false;
        assert_eq!(
            cluster_candidate_groups(&lane, &records, &options),
            vec![vec![0, 3], vec![1, 5], vec![2], vec![4], vec![6]]
        );

        options.no_layer = true;
        assert_eq!(
            cluster_candidate_groups(&lane, &records, &options),
            vec![vec![0, 1, 3, 5], vec![2], vec![4], vec![6]]
        );

        options.frames = 2;
        assert_eq!(
            cluster_candidate_groups(&lane, &records, &options),
            lane.iter().map(|&index| vec![index]).collect::<Vec<_>>()
        );

        records = (0..MAX_SWEEP_CLUSTER_MEMBERS * 2 + 2)
            .map(|index| eligible(&format!("bounded-{index}.aex"), "same", "root", Some(3)))
            .collect();
        options.frames = 1;
        options.no_layer = false;
        let groups =
            cluster_candidate_groups(&(0..records.len()).collect::<Vec<_>>(), &records, &options);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0], (0..16).collect::<Vec<_>>());
        assert_eq!(groups[1], (16..32).collect::<Vec<_>>());
        assert_eq!(groups[2], vec![32, 33]);
    }

    #[test]
    fn identified_failed_member_is_excluded_once_before_salvaging_the_remainder() {
        let candidates = (0..8).collect::<Vec<_>>();
        let mut attempts = Vec::<Vec<usize>>::new();
        let mut attempt = |subset: &[usize]| {
            attempts.push(subset.to_vec());
            if subset.contains(&5) {
                ClusterAttempt::RejectMember(5)
            } else {
                ClusterAttempt::Complete(
                    subset
                        .iter()
                        .map(|&index| (index, index * 10))
                        .collect::<HashMap<_, _>>(),
                )
            }
        };

        let outcomes = salvage_cluster_candidates(
            &candidates,
            MAX_SWEEP_CLUSTER_SALVAGE_ATTEMPTS,
            &mut attempt,
        );

        assert_eq!(
            attempts,
            vec![vec![0, 1, 2, 3, 4, 5, 6, 7], vec![0, 1, 2, 3, 4, 6, 7],]
        );
        assert_eq!(
            outcomes
                .keys()
                .copied()
                .collect::<std::collections::BTreeSet<_>>(),
            [0, 1, 2, 3, 4, 6, 7].into_iter().collect()
        );
        assert_eq!(outcomes[&6], 60);
    }

    #[test]
    fn hard_cluster_failure_is_not_retried_and_member_retries_are_bounded() {
        let candidates = (0..8).collect::<Vec<_>>();
        let mut hard_attempts = 0;
        let mut hard = |_: &[usize]| {
            hard_attempts += 1;
            ClusterAttempt::<usize>::HardFailure
        };
        assert!(
            salvage_cluster_candidates(&candidates, MAX_SWEEP_CLUSTER_SALVAGE_ATTEMPTS, &mut hard,)
                .is_empty()
        );
        assert_eq!(hard_attempts, 1);

        let mut rejected = Vec::new();
        let mut every_member_fails = |subset: &[usize]| {
            rejected.push(subset[0]);
            ClusterAttempt::<usize>::RejectMember(subset[0])
        };
        assert!(
            salvage_cluster_candidates(
                &candidates,
                MAX_SWEEP_CLUSTER_SALVAGE_ATTEMPTS,
                &mut every_member_fails,
            )
            .is_empty()
        );
        assert_eq!(rejected, vec![0, 1, 2, 3]);
    }

    #[test]
    fn member_failure_with_dirty_close_stops_salvage_after_one_attempt() {
        let candidates = (0..8).collect::<Vec<_>>();
        let dirty_close = json!({ "session_clean": false, "invalidated": true });
        let mut attempts = 0;
        let mut attempt = |_: &[usize]| {
            attempts += 1;
            rejected_member_after_close::<usize>(0, &dirty_close)
        };

        assert!(
            salvage_cluster_candidates(
                &candidates,
                MAX_SWEEP_CLUSTER_SALVAGE_ATTEMPTS,
                &mut attempt,
            )
            .is_empty()
        );
        assert_eq!(attempts, 1);
        assert!(matches!(
            rejected_member_after_close::<usize>(
                3,
                &json!({ "session_clean": true, "invalidated": false })
            ),
            ClusterAttempt::RejectMember(3)
        ));
    }

    #[test]
    fn shared_cluster_close_does_not_misattribute_the_last_plugins_diagnostics() {
        let mut outcome = Outcome::bare("rendered");
        outcome.detail.insert(
            "cluster_session".to_owned(),
            json!({ "plugin_index": 0, "plugin_count": 2 }),
        );
        attach_shared_cluster_close(
            &mut outcome,
            &json!({
                "session_clean": true,
                "invalidated": false,
                "invalidated_reason": Value::Null,
                "worker": {
                    "classification": "ok",
                    "exit_code": 0,
                    "diagnostics": {
                        "failure_stage": "belongs_to_the_last_plugin",
                        "stage_events": [{
                            "stage": "pr_gpu_route",
                            "state": "end",
                            "errors": { "reason": "belongs_to_the_last_plugin" }
                        }]
                    }
                }
            }),
        );

        assert_eq!(outcome.detail["session_clean"], true);
        assert_eq!(outcome.detail["worker"]["classification"], "ok");
        assert_eq!(outcome.detail["worker"]["exit_code"], 0);
        assert_eq!(outcome.detail["cluster_session"]["close_shared"], true);
        assert!(outcome.detail["worker"].get("failure_stage").is_none());
        assert!(outcome.detail["worker"].get("pr_gpu_route").is_none());
    }

    #[test]
    fn rendered_bucket_requires_and_reports_pixel_bytes() {
        let rendered = frame_outcome(Ok(FrameOutcome {
            frame_index: 0,
            status: FrameStatus::Rendered {
                pixels: [1, 2, 3, 255].repeat(4),
                width: 2,
                height: 2,
                origin_x: 0,
                origin_y: 0,
            },
        }));
        assert_eq!(rendered.bucket, "rendered");
        assert_eq!(rendered.detail["pixel_bytes"], 16);
        assert_eq!(rendered.detail["nonzero_alpha_pixels"], 4);

        let transparent = frame_outcome(Ok(FrameOutcome {
            frame_index: 0,
            status: FrameStatus::Rendered {
                pixels: [32, 64, 128, 0].repeat(4),
                width: 2,
                height: 2,
                origin_x: 0,
                origin_y: 0,
            },
        }));
        assert_eq!(transparent.bucket, "rendered_transparent");
        assert_eq!(transparent.detail["nonzero_alpha_pixels"], 0);

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
    fn alpha_evidence_handles_16_bit_and_rejects_invalid_float_alpha() {
        let mut rgba16 = Vec::new();
        for alpha in [0u16, 32768u16] {
            for channel in [1u16, 2, 3, alpha] {
                rgba16.extend_from_slice(&channel.to_le_bytes());
            }
        }
        assert_eq!(
            alpha_evidence(&rgba16, RenderPixelFormat::Argb16),
            Some(AlphaEvidence {
                nonzero: 1,
                invalid: 0
            })
        );

        let mut rgba32 = Vec::new();
        for alpha in [-0.0f32, -0.25, f32::NAN, f32::INFINITY, 0.5] {
            for channel in [0.0f32, 0.0, 0.0, alpha] {
                rgba32.extend_from_slice(&channel.to_le_bytes());
            }
        }
        assert_eq!(
            alpha_evidence(&rgba32, RenderPixelFormat::Argb32f),
            Some(AlphaEvidence {
                nonzero: 1,
                invalid: 3
            })
        );

        let invalid = frame_outcome_dumping(
            Ok(FrameOutcome {
                frame_index: 0,
                status: FrameStatus::Rendered {
                    pixels: rgba32,
                    width: 5,
                    height: 1,
                    origin_x: 0,
                    origin_y: 0,
                },
            }),
            None,
            RenderPixelFormat::Argb32f,
        );
        assert_eq!(invalid.bucket, "rendered_invalid_alpha");
        assert_eq!(invalid.detail["invalid_alpha_pixels"], 3);
    }

    #[test]
    fn frame_dumps_do_not_collide_for_identical_binaries_in_different_records() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "aexcompat-render-sweep-dumps-{}-{nonce:032x}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let dump = |corpus_index, pixels: Vec<u8>| {
            frame_outcome_dumping(
                Ok(FrameOutcome {
                    frame_index: 0,
                    status: FrameStatus::Rendered {
                        pixels,
                        width: 1,
                        height: 1,
                        origin_x: 0,
                        origin_y: 0,
                    },
                }),
                Some(FrameDump {
                    dir: &dir,
                    plugin_stem: "SameName",
                    plugin_sha256_prefix: "01234567",
                    corpus_index,
                    frame_index: 0,
                    format: "argb8",
                }),
                RenderPixelFormat::Argb8,
            )
        };
        let first = dump(7, vec![1, 2, 3, 4]);
        let second = dump(19, vec![5, 6, 7, 8]);
        let first_name = first.detail["dumped_frame"].as_str().unwrap();
        let second_name = second.detail["dumped_frame"].as_str().unwrap();

        assert_ne!(first_name, second_name);
        assert_eq!(std::fs::read(dir.join(first_name)).unwrap(), [1, 2, 3, 4]);
        assert_eq!(std::fs::read(dir.join(second_name)).unwrap(), [5, 6, 7, 8]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn rendered_pixels(bytes: &[u8]) -> Outcome {
        frame_outcome(Ok(FrameOutcome {
            frame_index: 0,
            status: FrameStatus::Rendered {
                pixels: bytes.to_vec(),
                width: 1,
                height: 1,
                origin_x: 0,
                origin_y: 0,
            },
        }))
    }

    #[test]
    fn pixel_determinism_records_equal_and_different_fresh_session_outputs() {
        let mut equal = rendered_pixels(&[1, 2, 3, 4]);
        verify_pixel_determinism(&mut equal, true, || rendered_pixels(&[1, 2, 3, 4]));
        assert_eq!(equal.bucket, "rendered");
        assert_eq!(
            equal.detail["pixel_determinism"],
            json!({
                "status": "deterministic",
                "repeat_bucket": "rendered",
                "repeat_pixel_sha256": equal.detail["pixel_sha256"],
                "repeat_frames": Value::Null,
            })
        );

        let mut different = rendered_pixels(&[1, 2, 3, 4]);
        let expected_repeat = rendered_pixels(&[4, 3, 2, 1]);
        let expected_hash = expected_repeat.detail["pixel_sha256"].clone();
        verify_pixel_determinism(&mut different, true, || expected_repeat);
        assert_eq!(different.bucket, "rendered");
        assert_eq!(
            different.detail["pixel_determinism"],
            json!({
                "status": "nondeterministic",
                "repeat_bucket": "rendered",
                "repeat_pixel_sha256": expected_hash,
                "repeat_frames": Value::Null,
            })
        );
    }

    #[test]
    fn pixel_determinism_compares_every_frame_in_order() {
        let first_hash = rendered_pixels(&[1, 2, 3, 4]).detail["pixel_sha256"].clone();
        let second_hash = rendered_pixels(&[5, 6, 7, 8]).detail["pixel_sha256"].clone();
        let changed_second_hash = rendered_pixels(&[8, 7, 6, 5]).detail["pixel_sha256"].clone();
        let frames = |second: Value| {
            json!([
                { "frame_index": 0, "bucket": "rendered", "pixel_sha256": first_hash },
                { "frame_index": 1, "bucket": "rendered", "pixel_sha256": second },
            ])
        };
        let mut primary = rendered_pixels(&[1, 2, 3, 4]);
        primary
            .detail
            .insert("frames".to_owned(), frames(second_hash));
        let mut repeated = rendered_pixels(&[1, 2, 3, 4]);
        repeated
            .detail
            .insert("frames".to_owned(), frames(changed_second_hash));

        verify_pixel_determinism(&mut primary, true, || repeated);

        assert_eq!(
            primary.detail["pixel_determinism"]["status"],
            "nondeterministic"
        );
        assert_eq!(
            primary.detail["pixel_determinism"]["repeat_frames"][0]["pixel_sha256"],
            primary.detail["frames"][0]["pixel_sha256"]
        );
        assert_ne!(
            primary.detail["pixel_determinism"]["repeat_frames"][1]["pixel_sha256"],
            primary.detail["frames"][1]["pixel_sha256"]
        );
    }

    #[test]
    fn pixel_determinism_skips_non_rendered_and_preserves_repeat_failures_as_evidence() {
        let called = std::cell::Cell::new(false);
        let mut failed = Outcome::bare("frame_error:1");
        verify_pixel_determinism(&mut failed, true, || {
            called.set(true);
            rendered_pixels(&[0; 4])
        });
        assert!(!called.get());
        assert_eq!(failed.bucket, "frame_error:1");
        assert_eq!(
            failed.detail["pixel_determinism"],
            json!({ "status": "not_applicable", "repeat_bucket": Value::Null })
        );

        let mut rendered = rendered_pixels(&[1; 4]);
        verify_pixel_determinism(&mut rendered, true, || Outcome::bare("session_open_failed"));
        assert_eq!(rendered.bucket, "rendered");
        assert_eq!(
            rendered.detail["pixel_determinism"],
            json!({
                "status": "repeat_failed",
                "repeat_bucket": "session_open_failed",
                "repeat_pixel_sha256": Value::Null,
                "repeat_frames": Value::Null,
            })
        );

        let mut disabled = rendered_pixels(&[2; 4]);
        verify_pixel_determinism(&mut disabled, false, || panic!("repeat must stay opt-in"));
        assert!(!disabled.detail.contains_key("pixel_determinism"));
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

    /// Issue #1063: an exit-20 discovery failure is bucketed by the selector
    /// that refused and its code, and an unclassified session-path failure by
    /// its cause, so `not_discovered:unknown` is reserved for a record with
    /// genuinely nothing in it.
    #[test]
    fn discovery_buckets_name_the_refusing_selector() {
        let selector = |setup: i64, params: i64, setdown: i64| {
            json!({
                "classification": "nonzero_exit",
                "cluster_error_kind": "selector_error",
                "exit_code": 20,
                "global_setup_error": setup,
                "params_setup_error": params,
                "global_setdown_error": setdown,
                "reported_num_params": 0,
                "parameter_count": 0,
            })
        };
        assert_eq!(
            discovery_failure_bucket(Some(&selector(14, -1, -1)), Some("nonzero_exit")),
            "exit_20_global_setup:14"
        );
        assert_eq!(
            discovery_failure_bucket(Some(&selector(0, 13, 0)), Some("nonzero_exit")),
            "exit_20_params_setup:13"
        );
        assert_eq!(
            discovery_failure_bucket(Some(&selector(0, 0, 25)), Some("nonzero_exit")),
            "exit_20_global_setdown:25"
        );
        // Selectors all 0: the parameter-count contract is what failed.
        let mut contract = selector(0, 0, 0);
        contract["reported_num_params"] = json!(9);
        contract["parameter_count"] = json!(7);
        assert_eq!(
            discovery_failure_bucket(Some(&contract), Some("nonzero_exit")),
            "exit_20_param_count_contract:9_vs_7"
        );
        // Selectors 0 and the count contract holds (reported = declared + 1):
        // the worker refused for a reason the report fields do not carry.
        let mut consistent = selector(0, 0, 0);
        consistent["reported_num_params"] = json!(8);
        consistent["parameter_count"] = json!(7);
        assert_eq!(
            discovery_failure_bucket(Some(&consistent), Some("nonzero_exit")),
            "exit_20_no_selector_error"
        );
        // -1 is the worker's "not invoked" sentinel, not a selector code: a
        // setdown that never ran after a clean setup/params pair does not
        // become the refusing selector.
        let mut sentinel = selector(0, 0, -1);
        sentinel["reported_num_params"] = json!(8);
        sentinel["parameter_count"] = json!(7);
        assert_eq!(
            discovery_failure_bucket(Some(&sentinel), Some("nonzero_exit")),
            "exit_20_no_selector_error"
        );
        // A record without the report fields (a pre-#1063 one-shot record).
        let bare = json!({"classification": "nonzero_exit", "exit_code": 20});
        assert_eq!(
            discovery_failure_bucket(Some(&bare), Some("nonzero_exit")),
            "exit_20_unattributed"
        );
        // Unclassified session-path failures name their cause.
        let unclassified =
            json!({"classification": null, "cluster_error_kind": "identity_changed"});
        assert_eq!(
            discovery_failure_bucket(Some(&unclassified), None),
            "identity_changed"
        );
        assert_eq!(discovery_failure_bucket(None, None), "unknown");
    }

    #[test]
    fn the_premiere_gpu_route_outcome_reaches_the_record_without_extended_diag() {
        // The route's faults are contained, so a plug-in whose GPU route died
        // on entry renders through the PF path and lands in `rendered`. What
        // keeps that from being a silent success is this key, and it depends on
        // three shapes the broker owns - the stage name, the `end` state and
        // the `reason` inside `errors`. This pins the sweep side against them;
        // the broker side that produces them is pinned by
        // `the_premiere_gpu_route_outcome_survives_the_allowlist_and_the_reason_filter`
        // in the broker's image_render tests, because fixtures built here
        // cannot notice a change on that side.
        let close_with = |events: Value| {
            json!({
                "session_clean": true,
                "invalidated_reason": Value::Null,
                "worker": {
                    "classification": "ok",
                    "exit_code": 0,
                    "diagnostics": { "stage_events": events }
                }
            })
        };
        let route_event = |state: &str, reason: Value| json!({"stage": "pr_gpu_route", "state": state, "errors": {"reason": reason}});

        let mut declined = Outcome::bare("rendered");
        attach_close(
            &mut declined,
            close_with(json!([
                route_event("begin", Value::Null),
                route_event("end", json!("startup_fault")),
            ])),
            false,
        );
        assert_eq!(
            declined.detail["worker"]["pr_gpu_route"],
            json!("startup_fault"),
            "a declined route has to be readable off an ordinary rendered record"
        );

        let mut committed = Outcome::bare("rendered");
        attach_close(
            &mut committed,
            close_with(json!([
                route_event("end", json!("output_frame_alloc")),
                json!({"stage": "smart_render_cpu", "state": "end", "errors": {"error": 0}}),
                route_event("end", json!("committed")),
            ])),
            false,
        );
        assert_eq!(
            committed.detail["worker"]["pr_gpu_route"],
            json!("committed"),
            "the last entry into the route is the one the record names"
        );

        // The other stage carries a `reason` of its own (classic_output_resize
        // really does, issue #984), so an extraction that stopped checking the
        // stage name would mislabel that reason as the route's outcome.
        let mut never_ran = Outcome::bare("rendered");
        attach_close(
            &mut never_ran,
            close_with(json!([json!({
                "stage": "classic_output_resize",
                "state": "end",
                "errors": {"reason": "output_resize_refused"}
            })])),
            false,
        );
        assert!(
            never_ran.detail["worker"].get("pr_gpu_route").is_none(),
            "an effect that never entered the route carries no key at all"
        );

        // Two ways an entered route still carries no reason, and they take
        // different paths through the search: a `_begin` with no `_end` at all
        // (the worker died inside the route) finds nothing, while an `_end`
        // whose reason the broker's shape check dropped finds an event without
        // one. Neither may invent a reason; `active_stage` is what names those.
        let mut died_inside = Outcome::bare("render_frame_failed");
        attach_close(
            &mut died_inside,
            close_with(json!([route_event("begin", Value::Null)])),
            false,
        );
        assert!(died_inside.detail["worker"].get("pr_gpu_route").is_none());

        let mut no_reason = Outcome::bare("render_frame_failed");
        attach_close(
            &mut no_reason,
            close_with(json!([
                route_event("begin", Value::Null),
                json!({"stage": "pr_gpu_route", "state": "end", "errors": {}}),
            ])),
            false,
        );
        assert!(no_reason.detail["worker"].get("pr_gpu_route").is_none());

        let mut missing_events = Outcome::bare("rendered");
        attach_close(
            &mut missing_events,
            json!({
                "session_clean": true,
                "invalidated_reason": Value::Null,
                "worker": {"classification": "ok", "exit_code": 0, "diagnostics": {}}
            }),
            false,
        );
        assert!(
            missing_events.detail["worker"]
                .get("pr_gpu_route")
                .is_none()
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
        let mut post_frame_exit = prior_frames.clone();
        *post_frame_exit
            .pointer_mut("/invalidated_reason/reason")
            .unwrap() = json!("premature_exit");
        assert_eq!(
            smart_fallback_reason(true, false, &post_frame_exit),
            Some("smart_worker_heap_corruption"),
            "a heap death after the frame reply but before close still discards Smart pixels"
        );
        let mut during_close_exit = prior_frames.clone();
        *during_close_exit
            .pointer_mut("/invalidated_reason/reason")
            .unwrap() = json!("worker_exited_during_close");
        assert_eq!(
            smart_fallback_reason(true, false, &during_close_exit),
            Some("smart_worker_heap_corruption"),
            "a heap death after close delivery still discards Smart pixels"
        );

        let mut wrong_exit = close;
        *wrong_exit.pointer_mut("/worker/exit_code").unwrap() = json!(0xC000_0005u64);
        assert_eq!(smart_fallback_reason(true, false, &wrong_exit), None);
    }

    #[test]
    fn transparent_smart_comparison_requires_a_clean_completed_selector() {
        let close = json!({
            "render_path": "smart",
            "session_clean": true,
            "invalidated": false,
            "frames_ok": 1,
            "frames_errored": 0,
            "final_report": {
                "smart_render_selector_dispatched": true,
                "smart_render_error": 0
            },
            "worker": { "classification": "ok", "exit_code": 0 }
        });
        assert_eq!(validate_smart_transparent_close(&close), Ok(()));

        let mut refused = close.clone();
        *refused
            .pointer_mut("/final_report/smart_render_error")
            .unwrap() = json!(4);
        assert_eq!(
            validate_smart_transparent_close(&refused),
            Err("smart_render_error")
        );
    }

    #[test]
    fn classic_comparison_panic_preserves_the_primary_smart_evidence() {
        let mut primary = Outcome::bare("rendered_transparent");
        primary
            .detail
            .insert("pixel_sha256".into(), json!("smart-sha"));
        primary
            .detail
            .insert("nonzero_alpha_pixels".into(), json!(0));

        attach_classic_comparison(&mut primary, || panic!("comparison fixture"));

        assert_eq!(primary.bucket, "rendered_transparent");
        assert_eq!(primary.detail["pixel_sha256"], "smart-sha");
        assert_eq!(primary.detail["nonzero_alpha_pixels"], 0);
        assert_eq!(
            primary.detail["classic_comparison"]["bucket"],
            "sweep_panicked"
        );
        assert_eq!(
            primary.detail["comparison_reason"],
            "smart_output_transparent"
        );
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
    fn inventory_records_identity_and_blocking_without_discovery() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-inventory-{}-{nonce:032x}",
            std::process::id()
        ));
        let vendor = root.join("Maxon");
        std::fs::create_dir_all(&vendor).unwrap();
        let plugin = vendor.join("effect.aex");
        std::fs::write(&plugin, b"first identity").unwrap();

        let mut options = discovery_options(root.join("unused.json"));
        options.discovery_only = false;
        options.inventory_only = true;
        options.blocked_paths = vec!["maxon".to_owned()];
        let build = capture_report_build_fingerprint(&root, Err(()));
        let first = inventory_record(&plugin, std::slice::from_ref(&root), &options, &build);
        assert_eq!(first["plugin_relative_path"], "Maxon/effect.aex");
        assert_eq!(first["scan_folder"], 0);
        assert_eq!(first["final_stage"], "scan");
        assert_eq!(first["execution_classification"], "external_blocked");
        assert_eq!(first["failure_classification"], "external_blocked");
        assert_eq!(first["detail"]["aex_loaded"], false);
        assert_eq!(first["detail"]["blocked_path_substrings"], json!(["maxon"]));
        assert_eq!(first["plugin_size_bytes"], 14);
        assert!(
            first["plugin_path_sha256"]
                .as_str()
                .is_some_and(|hash| hash.len() == 64)
        );

        std::fs::write(&plugin, b"second identity").unwrap();
        let second = inventory_record(&plugin, std::slice::from_ref(&root), &options, &build);
        assert_eq!(second["plugin_path_sha256"], first["plugin_path_sha256"]);
        assert_ne!(second["plugin_sha256"], first["plugin_sha256"]);

        let scan = DiagnosticScan {
            dirs: vec![root.clone()],
            plugins: vec![plugin],
            seen: 1,
            dependency_dirs: Vec::new(),
            incomplete_reason: None,
        };
        let report = inventory_report(
            &options,
            &scan,
            &build,
            vec![second],
            Duration::from_millis(3),
        );
        assert_eq!(report["mode"], "inventory_only");
        assert!(report["render"].is_null());
        assert_eq!(report["scan"]["after_ignore"], 1);
        assert_eq!(report["scan"]["swept"], 1);
        assert_eq!(report["buckets"]["external_blocked"], 1);
        assert_eq!(
            report["buckets"]
                .as_object()
                .unwrap()
                .values()
                .map(|v| v.as_u64().unwrap())
                .sum::<u64>(),
            report["plugins"].as_array().unwrap().len() as u64
        );

        std::fs::remove_dir_all(root).unwrap();
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
        let mut options = discovery_options(report_path.clone());
        options.exclude_paths = vec!["maxon".to_owned(), "sapphire".to_owned()];
        let cli = write_build_layout(&root);
        let build_start = capture_report_build_fingerprint(&root, Ok(cli.clone()));
        let build = finalize_report_build_fingerprint(&build_start, &root, Ok(cli));
        let partial = partial_path(&report_path);
        let completed = AtomicUsize::new(0);
        record_discovery_progress(
            &records,
            &scan,
            &build_start,
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
        assert_eq!(partial_row["build"]["complete"], false);
        assert_eq!(partial_row["build"]["verification"], "pre_run_candidate");

        let report = discovery_only_report(
            &options,
            &scan,
            &build,
            &records,
            Duration::from_millis(4),
            Duration::from_millis(5),
        );
        finish_report(&options, &report).unwrap();
        assert!(report_path.is_file());
        assert!(
            !partial.exists(),
            "a successful final report supersedes the sidecar"
        );
        let final_report: Value =
            serde_json::from_slice(&std::fs::read(&report_path).unwrap()).unwrap();
        assert_eq!(final_report["mode"], "discovery_only");
        assert!(final_report["render"].is_null());
        assert_eq!(
            final_report["scan"]["selection"]["excluded_path_substrings"],
            json!(["maxon", "sapphire"])
        );
        assert_eq!(final_report["buckets"]["exit_11_load_library"], 1);
        assert_eq!(final_report["plugins"][0]["discovery"]["ok"], false);
        assert_eq!(final_report["build"]["complete"], true);
        assert_eq!(
            final_report["build"]["verification"],
            "run_boundary_verified"
        );
        assert_eq!(final_report["plugins"][0]["build"], final_report["build"]);
        for executable in ["cli", "l2_worker", "classic_worker", "smart_worker"] {
            assert_eq!(
                final_report["build"][executable]["sha256"],
                partial_row["build"][executable]["sha256"]
            );
            assert_eq!(
                final_report["build"][executable]["size_bytes"],
                partial_row["build"][executable]["size_bytes"]
            );
        }

        let blocked_path = root.join("blocked.json");
        std::fs::create_dir(&blocked_path).unwrap();
        let blocked_partial = partial_path(&blocked_path);
        std::fs::write(&blocked_partial, &partial_bytes).unwrap();
        let blocked_options = discovery_options(blocked_path);
        let error = finish_report(&blocked_options, &report).unwrap_err();
        assert_eq!(error.code, "report_publish_failed");
        assert!(
            blocked_partial.is_file(),
            "a failed final replacement must retain crash evidence"
        );
        assert!(
            !blocked_options
                .json
                .as_ref()
                .unwrap()
                .with_extension("json.tmp")
                .exists(),
            "a failed final replacement must remove its temporary file"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_partial_failure_prevents_the_discovery_action() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-render-sweep-partial-order-{}-{nonce:032x}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let partial = root.join("locked.partial.jsonl");
        std::fs::create_dir(&partial).unwrap();
        let action_calls = AtomicUsize::new(0);

        let error = after_partial_is_prepared(Some(&partial), || {
            action_calls.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap_err();

        assert_eq!(error.code, "partial_prepare_failed");
        assert_eq!(action_calls.load(Ordering::Relaxed), 0);
        assert!(partial.is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }
}
