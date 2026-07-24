//! Measures what sealing the dependency closure unlocks across a whole plug-in
//! folder (issue #304).
//!
//! For every `*.aex` under the scan folder this resolves the dependency closure
//! the multi-filter would seal, runs the same parameter discovery, and buckets
//! the outcome (loaded / LoadLibrary failure / module audit failure / timeout /
//! other). Run it twice — with and without `--no-deps` — to measure the unlock
//! rate rather than assert it.
//!
//! Run:
//!   set AEXCOMPAT_MULTIFILTER_REPOSITORY=C:\path\to\AEXCompat
//!   cargo run --release --example discover_sweep -- <scan-dir> [options]
//!
//! Options:
//!   --deps <dir>   extra dependency search folder (repeatable; defaults to the
//!                  scan folder's AE `Support Files` ancestor when present)
//!   --no-deps      seal nothing, i.e. the pre-#304 behaviour
//!   --limit <n>    stop after n plug-ins
//!   --jobs <n>     dispatch up to n workers concurrently (issue #404; default
//!                  8 — measured at 353 plug-ins: 982s sequential -> 282s).
//!                  Workers are process-isolated (private desktop / Job Object /
//!                  per-dispatch sealed root), so concurrency only changes wall
//!                  time, not the safety model. `--jobs 1` restores the
//!                  historical sequential behaviour; prefer it when the TEMP
//!                  volume is nearly full, since concurrent staging needs a few
//!                  GB of transient headroom per worker.
//!   --survey-only  only measure each closure's size; launch no workers
//!   --json <path>  write the per-plug-in records as JSON

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use aexcompat_broker::image_render::inspect_experimental_with_approved_dependencies_and_diagnostics;
use aexcompat_broker::plugin_dependency_closure::{
    DependencyClosureRequest, DependencyProvenance, resolve_dependency_closure,
    survey_dependency_closure,
};
use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

struct Options {
    scan: PathBuf,
    dependency_dirs: Vec<PathBuf>,
    seal: bool,
    survey_only: bool,
    limit: usize,
    jobs: usize,
    json: Option<PathBuf>,
}

fn parse_options() -> Options {
    parse_options_from(std::env::args().skip(1).collect())
}

/// Argument parsing, split from `env::args` so the contract (including
/// `--jobs` validation) can be pinned by unit tests.
fn parse_options_from(args: Vec<String>) -> Options {
    let mut args = args.into_iter();
    let mut scan = None;
    let mut dependency_dirs = Vec::new();
    let mut seal = true;
    let mut survey_only = false;
    let mut limit = usize::MAX;
    let mut jobs = 8usize;
    let mut json = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--deps" => {
                dependency_dirs.push(PathBuf::from(args.next().expect("--deps needs a dir")))
            }
            "--no-deps" => seal = false,
            "--survey-only" => survey_only = true,
            "--limit" => {
                limit = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .expect("--limit needs a number")
            }
            "--jobs" => {
                jobs = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .filter(|jobs: &usize| *jobs >= 1)
                    .expect("--jobs needs a number >= 1")
            }
            "--json" => json = Some(PathBuf::from(args.next().expect("--json needs a path"))),
            other => scan = Some(PathBuf::from(other)),
        }
    }
    let scan = scan.expect(
        "usage: discover_sweep <scan-dir> [--deps <dir>] [--no-deps] [--limit n] [--jobs n] [--json path]",
    );
    if dependency_dirs.is_empty() {
        dependency_dirs.extend(default_dependency_dirs(&scan));
    }
    Options {
        scan,
        dependency_dirs,
        seal,
        survey_only,
        limit,
        jobs,
        json,
    }
}

/// The nearest ancestor named `Support Files` (the AE runtime folder that holds
/// `dvacore.dll` and friends), if the scan folder sits inside an AE install.
fn default_dependency_dirs(scan: &Path) -> Vec<PathBuf> {
    scan.ancestors()
        .find(|ancestor| {
            ancestor
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("Support Files"))
        })
        .map(Path::to_path_buf)
        .into_iter()
        .collect()
}

fn collect_aex(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_aex(&path, depth + 1, out);
        } else if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("aex"))
        {
            out.push(path);
        }
    }
}

/// Returns a shareable, scan-root-relative identity without leaking the local
/// installation path. `collect_aex` only returns paths below `scan`; a failure
/// here means that invariant was broken and the report must fail closed rather
/// than fall back to an ambiguous basename or an absolute path.
fn normalized_relative_path(scan: &Path, plugin: &Path) -> String {
    let relative = plugin
        .strip_prefix(scan)
        .expect("discovered plug-in must be below the scan root");
    let components: Vec<String> = relative
        .components()
        .map(|component| match component {
            Component::Normal(value) => value.to_string_lossy().into_owned(),
            _ => panic!("discovered plug-in has an unsafe relative path"),
        })
        .collect();
    assert!(
        !components.is_empty(),
        "discovered plug-in must have a relative path"
    );
    components.join("/")
}

fn plugin_record(
    scan: &Path,
    plugin: &Path,
    bucket: &str,
    elapsed_ms: u128,
    size_bytes: Option<u64>,
    sha256: Option<&str>,
    extra: Value,
) -> Value {
    let identity_status = if sha256.is_some() {
        "hashed"
    } else {
        "unreadable_file"
    };
    let mut row = json!({
        // Keep the historical basename for human-readable reports and
        // consumers that only displayed it. The relative path is the stable
        // identity when a scan contains duplicate basenames.
        "plugin": plugin
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<unknown>".into()),
        "plugin_relative_path": normalized_relative_path(scan, plugin),
        "plugin_size_bytes": size_bytes,
        "plugin_sha256": sha256,
        "plugin_identity_status": identity_status,
        "bucket": bucket,
        "elapsed_ms": elapsed_ms,
    });
    if let (Some(row), Some(extra)) = (row.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            row.insert(key.clone(), value.clone());
        }
    }
    row
}

/// The diagnostics JSON the broker embeds in its error text.
fn diagnostics_of(error: &str) -> Option<Value> {
    let start = error.find('{')?;
    serde_json::from_str(&error[start..]).ok()
}

fn bucket_of(error: &str) -> String {
    let Some(diagnostics) = diagnostics_of(error) else {
        return "unparsed_error".into();
    };
    if diagnostics.get("module_audit_failure").is_some() {
        return "module_audit_failure".into();
    }
    let classification = diagnostics
        .get("classification")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    match classification {
        "nonzero_exit" => match diagnostics.get("exit_code").and_then(Value::as_u64) {
            Some(11) => "exit_11_load_library".into(),
            Some(14) => "exit_14_module_audit".into(),
            // Exit 12 covers both "this is genuinely not an Effect plug-in" and
            // "the host could not resolve its Effect entrypoint", which are very
            // different findings, so keep the worker's own kind in the bucket.
            Some(12) => match diagnostics.get("plugin_kind").and_then(Value::as_str) {
                Some(kind) => format!("exit_12_{kind}"),
                None => "exit_12".into(),
            },
            Some(code) => format!("exit_{code}"),
            None => "nonzero_exit".into(),
        },
        other => other.into(),
    }
}

fn provenance_json(sources: &[DependencyProvenance]) -> Value {
    Value::Array(
        sources
            .iter()
            .map(|source| {
                json!({
                    "basename": source.basename,
                    "import_derived": source.import_derived,
                    "string_derived": source.string_derived,
                })
            })
            .collect(),
    )
}

/// One plug-in's sweep result: exactly one record and one bucket, plus the
/// progress line the sequential path printed as it went. Under `--jobs` the
/// outcomes are collected per plug-in index and emitted in scan order, so the
/// JSON records and the bucket summary match a sequential run.
struct SweepOutcome {
    record: Value,
    bucket: String,
    log: String,
}

fn sweep_plugin(
    options: &Options,
    repository: &Path,
    index: usize,
    total: usize,
    plugin: &Path,
) -> SweepOutcome {
    let started = Instant::now();
    // Hash and size the source before dispatch so every outcome, including
    // worker/module failures, can be mapped back to the exact file. An
    // unreadable file keeps explicit null identity fields instead of
    // silently dropping its provenance.
    let file_bytes = std::fs::read(plugin).ok();
    let plugin_size_bytes = file_bytes.as_ref().map(|bytes| bytes.len() as u64);
    let plugin_sha256 = file_bytes
        .as_ref()
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)));
    // Every plug-in gets exactly one record and one bucket, including the
    // ones that fail before dispatch: a sweep that silently drops them
    // reports a total that disagrees with its own buckets.
    let record = |bucket: &str, extra: Value| {
        plugin_record(
            &options.scan,
            plugin,
            bucket,
            started.elapsed().as_millis(),
            plugin_size_bytes,
            plugin_sha256.as_deref(),
            extra,
        )
    };
    let name = plugin.file_name().unwrap().to_string_lossy();
    if file_bytes.is_none() {
        return SweepOutcome {
            record: record("unreadable_file", json!({})),
            bucket: "unreadable_file".into(),
            log: format!("[{}/{}] {name} -> unreadable_file", index + 1, total),
        };
    }
    let sha = plugin_sha256
        .as_deref()
        .expect("readable plug-in must have a SHA-256 identity");

    // `--no-deps` seals nothing at all, including helper DLLs sitting next to
    // the plug-in: the point of the baseline is the pre-#304 dispatch, which
    // passed an empty dependency list, so keeping the plug-in's own folder as
    // a root would under-count the load failures it is meant to measure.
    let mut roots: Vec<PathBuf> = Vec::new();
    if options.seal {
        // Canonicalized because the resolver requires absolute roots, and a
        // scan folder may be given relative on the command line.
        roots.extend(
            plugin
                .parent()
                .and_then(|parent| std::fs::canonicalize(parent).ok()),
        );
        for dir in &options.dependency_dirs {
            if let Ok(dir) = std::fs::canonicalize(dir)
                && !roots.contains(&dir)
            {
                roots.push(dir);
            }
        }
    }
    if options.survey_only {
        return match survey_dependency_closure(plugin, &roots) {
            Ok(survey) => SweepOutcome {
                record: record(
                    "surveyed",
                    json!({
                        "closure_modules": survey.modules.len(),
                        "closure_bytes": survey.total_bytes,
                        "unresolved": survey.unresolved.len(),
                        "unreadable_images": survey.unreadable_images,
                        "dependency_provenance": provenance_json(&survey.provenance),
                    }),
                ),
                bucket: "surveyed".into(),
                log: format!(
                    "[{}/{}] {name} -> {} modules, {} bytes",
                    index + 1,
                    total,
                    survey.modules.len(),
                    survey.total_bytes
                ),
            },
            Err(error) => {
                let bucket = format!("survey_error: {error}");
                SweepOutcome {
                    record: record(&bucket, json!({})),
                    log: format!("[{}/{}] {name} -> {bucket}", index + 1, total),
                    bucket,
                }
            }
        };
    }
    // The baseline must not depend on the resolver at all: it reproduces the
    // pre-#304 dispatch, which passed an empty dependency list without ever
    // reading the plug-in's import table. Running the resolver here would let
    // a parse failure bucket a plug-in as `closure_error` in a run whose whole
    // point is what the worker does with no dependencies.
    let closure = options
        .seal
        .then(|| resolve_dependency_closure(DependencyClosureRequest::new(plugin, &roots)));
    // `unresolved` is `null` rather than 0 in the baseline: nothing was
    // resolved there, so reporting a count would read as "nothing was
    // missing" when the honest answer is "not measured".
    let (dependencies, sealed_bytes, unresolved, dependency_provenance): (
        Vec<ApprovedImageArtifact>,
        u64,
        Value,
        Value,
    ) = match &closure {
        Some(Ok(closure)) => (
            closure.dependencies().to_vec(),
            closure.total_bytes(),
            json!(closure.unresolved().len()),
            provenance_json(closure.provenance()),
        ),
        Some(Err(_)) => (Vec::new(), 0, json!(null), json!(null)),
        None => (Vec::new(), 0, json!(null), json!(null)),
    };
    // A dispatch failure keeps its raw error text in the record: the bucket
    // alone cannot distinguish "worker reported structured diagnostics" from
    // an environment failure like a full TEMP volume (os error 112), and a
    // sweep that hides the message makes the latter look like the former.
    let mut dispatch_error: Option<String> = None;
    let bucket = match &closure {
        Some(Err(error)) => format!("closure_error: {error}"),
        Some(Ok(_)) | None => {
            match inspect_experimental_with_approved_dependencies_and_diagnostics(
                repository,
                plugin,
                sha,
                dependencies.clone(),
            ) {
                Ok((parameters, _)) => {
                    return SweepOutcome {
                        record: record(
                            "loaded",
                            json!({
                                "parameters": parameters.len(),
                                "sealed": dependencies.len(),
                                "sealed_bytes": sealed_bytes,
                                "unresolved": unresolved,
                                "dependency_provenance": dependency_provenance,
                            }),
                        ),
                        bucket: "loaded".into(),
                        log: format!(
                            "[{}/{}] {name} -> loaded ({} params, {} deps)",
                            index + 1,
                            total,
                            parameters.len(),
                            dependencies.len()
                        ),
                    };
                }
                Err(error) => {
                    let text = error.to_string();
                    let bucket = bucket_of(&text);
                    dispatch_error = Some(text);
                    bucket
                }
            }
        }
    };
    SweepOutcome {
        record: record(
            &bucket,
            json!({
                "sealed": dependencies.len(),
                "sealed_bytes": sealed_bytes,
                "unresolved": unresolved,
                "dependency_provenance": dependency_provenance,
                "error": dispatch_error,
            }),
        ),
        log: format!(
            "[{}/{}] {name} -> {bucket} ({} deps)",
            index + 1,
            total,
            dependencies.len()
        ),
        bucket,
    }
}

/// How many worker threads to spawn for `total` plug-ins: never more threads
/// than plug-ins, and always at least one so a zero-plug-in scan still spawns
/// a thread that exits immediately instead of taking a special-case path.
fn effective_jobs(jobs: usize, total: usize) -> usize {
    jobs.max(1).min(total.max(1))
}

/// Runs `work` for every index in `0..total` on a scoped thread pool and
/// returns the results in index order. The workers are already
/// process-isolated (private desktop, Job Object, per-dispatch
/// randomly-named sealed root, per-process audit), so running several
/// dispatches concurrently changes only wall time, not the safety model.
/// Each result lands back in its own slot, so the report keeps scan order no
/// matter which thread finished first.
fn run_pool<T: Send>(jobs: usize, total: usize, work: &(dyn Fn(usize) -> T + Sync)) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let mut slots: Vec<Option<T>> = (0..total).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..effective_jobs(jobs, total))
            .map(|_| {
                scope.spawn(|| {
                    let mut local = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= total {
                            break;
                        }
                        local.push((index, work(index)));
                    }
                    local
                })
            })
            .collect();
        for handle in handles {
            for (index, value) in handle.join().expect("sweep worker thread panicked") {
                slots[index] = Some(value);
            }
        }
    });
    slots
        .into_iter()
        .map(|slot| slot.expect("every plug-in must be processed exactly once"))
        .collect()
}

/// Aggregates one outcome per plug-in into the report records and the bucket
/// summary, preserving input (scan) order.
fn collect_report(
    outcomes: Vec<SweepOutcome>,
) -> (Vec<Value>, std::collections::BTreeMap<String, usize>) {
    let mut buckets: std::collections::BTreeMap<String, usize> = Default::default();
    let mut records = Vec::with_capacity(outcomes.len());
    for outcome in outcomes {
        *buckets.entry(outcome.bucket).or_default() += 1;
        records.push(outcome.record);
    }
    (records, buckets)
}

fn main() {
    let options = parse_options();
    let repository = PathBuf::from(
        std::env::var_os("AEXCOMPAT_MULTIFILTER_REPOSITORY")
            .expect("set AEXCOMPAT_MULTIFILTER_REPOSITORY"),
    );
    let mut plugins = Vec::new();
    collect_aex(&options.scan, 0, &mut plugins);
    plugins.sort();
    plugins.truncate(options.limit);
    let total = plugins.len();
    eprintln!(
        "sweeping {} plug-ins (seal dependencies: {}, search dirs: {}, jobs: {})",
        total,
        options.seal,
        options.dependency_dirs.len(),
        options.jobs
    );

    let work = |index: usize| {
        let outcome = sweep_plugin(&options, &repository, index, total, &plugins[index]);
        eprintln!("{}", outcome.log);
        outcome
    };
    let outcomes = if options.jobs <= 1 {
        (0..total).map(&work).collect()
    } else {
        run_pool(options.jobs, total, &work)
    };
    let (records, buckets) = collect_report(outcomes);

    println!("\n=== summary ({} plug-ins) ===", records.len());
    for (bucket, count) in &buckets {
        println!("{count:5}  {bucket}");
    }
    if let Some(path) = options.json {
        let report = json!({
            "sealed_dependencies": options.seal,
            "plugins": records,
            "buckets": buckets,
        });
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_disambiguates_duplicate_basenames_without_absolute_paths() {
        let scan = Path::new(r"C:\scan");
        let first = scan.join("one").join("Same.aex");
        let second = scan.join("two").join("Same.aex");

        let first_record = plugin_record(scan, &first, "unreadable_file", 0, None, None, json!({}));
        let second_record =
            plugin_record(scan, &second, "unreadable_file", 0, None, None, json!({}));

        assert_eq!(first_record["plugin"], "Same.aex");
        assert_eq!(second_record["plugin"], "Same.aex");
        assert_eq!(first_record["plugin_relative_path"], "one/Same.aex");
        assert_eq!(second_record["plugin_relative_path"], "two/Same.aex");
        assert_ne!(
            first_record["plugin_relative_path"],
            second_record["plugin_relative_path"]
        );
        assert!(
            first_record["plugin_relative_path"]
                .as_str()
                .is_some_and(|path| !path.contains(':') && !path.contains(".."))
        );
        assert!(first_record["plugin_sha256"].is_null());
        assert!(first_record["plugin_size_bytes"].is_null());
        assert_eq!(first_record["plugin_identity_status"], "unreadable_file");
    }

    #[test]
    fn readable_identity_is_recorded_with_size_and_sha() {
        let scan = Path::new(r"C:\scan");
        let plugin = scan.join("Same.aex");
        let record = plugin_record(
            scan,
            &plugin,
            "loaded",
            4,
            Some(3),
            Some("abc123"),
            json!({"parameters": 2}),
        );

        assert_eq!(record["plugin_relative_path"], "Same.aex");
        assert_eq!(record["plugin_size_bytes"], 3);
        assert_eq!(record["plugin_sha256"], "abc123");
        assert_eq!(record["plugin_identity_status"], "hashed");
        assert_eq!(record["parameters"], 2);
    }

    #[test]
    fn default_jobs_is_eight_and_the_flag_overrides_it() {
        let options = parse_options_from(vec!["scan".into()]);
        assert_eq!(options.jobs, 8);
        assert_eq!(options.scan, PathBuf::from("scan"));

        let options = parse_options_from(vec!["scan".into(), "--jobs".into(), "3".into()]);
        assert_eq!(options.jobs, 3);
    }

    #[test]
    #[should_panic(expected = "--jobs needs a number >= 1")]
    fn jobs_zero_is_rejected() {
        parse_options_from(vec!["scan".into(), "--jobs".into(), "0".into()]);
    }

    #[test]
    #[should_panic(expected = "--jobs needs a number >= 1")]
    fn jobs_non_numeric_is_rejected() {
        parse_options_from(vec!["scan".into(), "--jobs".into(), "many".into()]);
    }

    #[test]
    fn effective_jobs_clamps_to_the_plugin_count_and_keeps_one_thread_for_empty_scans() {
        assert_eq!(effective_jobs(8, 353), 8);
        assert_eq!(effective_jobs(8, 3), 3);
        assert_eq!(effective_jobs(8, 0), 1);
        assert_eq!(effective_jobs(16, 0), 1);
    }

    #[test]
    fn run_pool_restores_scan_order_and_covers_every_index() {
        let results = run_pool(8, 353, &|index| {
            // Varying per-index cost shuffles completion order across threads;
            // the returned vector must still be in index order.
            std::thread::sleep(std::time::Duration::from_millis((index % 4) as u64));
            index
        });
        assert_eq!(results, (0..353).collect::<Vec<_>>());
    }

    #[test]
    fn run_pool_returns_empty_for_a_zero_plugin_scan() {
        let results = run_pool(8, 0, &|index| index);
        assert!(results.is_empty());
    }

    #[test]
    fn collect_report_keeps_scan_order_and_counts_one_bucket_per_plugin() {
        let outcome = |bucket: &str, marker: &str| SweepOutcome {
            record: json!({"marker": marker}),
            bucket: bucket.into(),
            log: String::new(),
        };
        let (records, buckets) = collect_report(vec![
            outcome("loaded", "first"),
            outcome("exit_20", "second"),
            outcome("loaded", "third"),
        ]);

        assert_eq!(records.len(), 3);
        assert_eq!(records[0]["marker"], "first");
        assert_eq!(records[1]["marker"], "second");
        assert_eq!(records[2]["marker"], "third");
        assert_eq!(buckets.get("loaded"), Some(&2));
        assert_eq!(buckets.get("exit_20"), Some(&1));
        assert_eq!(buckets.values().sum::<usize>(), 3);
    }

    #[test]
    fn bucket_of_preserves_structured_diagnostics() {
        assert_eq!(
            bucket_of(r#"worker failed: {"classification":"nonzero_exit","exit_code":11}"#),
            "exit_11_load_library"
        );
        assert_eq!(
            bucket_of(
                r#"worker failed: {"classification":"nonzero_exit","exit_code":12,"plugin_kind":"aegp_candidate"}"#
            ),
            "exit_12_aegp_candidate"
        );
        assert_eq!(
            bucket_of(r#"worker failed: {"module_audit_failure":{"reason":"unsigned"}}"#),
            "module_audit_failure"
        );
    }

    #[test]
    fn bucket_of_pins_environment_failures_to_unparsed_error() {
        // os error 112 (ERROR_DISK_FULL) carries no structured diagnostics;
        // the bucket must stay distinct from worker-reported failures so a
        // full TEMP volume cannot masquerade as a plug-in load result.
        assert_eq!(
            bucket_of("ディスクに十分な空き領域がありません。 (os error 112)"),
            "unparsed_error"
        );
        assert_eq!(bucket_of("some plain io error"), "unparsed_error");
    }
}
