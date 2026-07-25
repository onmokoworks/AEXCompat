//! Measures what sealing the dependency closure unlocks across a whole plug-in
//! folder (issue #304).
//!
//! For every `*.aex` under the scan folder this resolves the dependency closure
//! the multi-filter would seal, runs the same parameter discovery, and buckets
//! the outcome (loaded / LoadLibrary failure / module audit failure / timeout /
//! other). Run it twice — with and without `--no-deps` — to measure the unlock
//! rate rather than assert it.
//!
//! `--cluster` (issue #405) switches discovery to cluster sessions: plug-ins
//! sharing one dependency-closure identity are inspected inside a single
//! `DiscoverySession` (one seal + one worker for the whole cluster), and the
//! report carries per-cluster open/inspect timings next to the per-plug-in
//! records, so a default run and a `--cluster` run give a before/after
//! comparison of the same sweep.
//!
//! Run:
//!   set AEXCOMPAT_MULTIFILTER_REPOSITORY=C:\path\to\AEXCompat
//!   cargo run --release --example discover_sweep -- <scan-dir> [options]
//!
//! Options:
//!   --deps <dir>   extra dependency search folder (repeatable; defaults to the
//!                  scan folder's AE `Support Files` ancestor when present)
//!   --no-deps      seal nothing, i.e. the pre-#304 behaviour
//!   --cluster      discover same-closure clusters through DiscoverySessions
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

use aexcompat_broker::image_render::inspect_experimental_with_approved_dependencies_and_resources;
use aexcompat_broker::plugin_dependency_closure::{
    DependencyClosureRequest, DependencyProvenance, resolve_dependency_closure,
    survey_dependency_closure,
};
use aexcompat_broker::render_session::{
    DiscoverySession, DiscoverySessionOpenRequest, InspectOutcome,
};
use aexcompat_broker::sealed_load_tree::SealedResourceEntry;
use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Extra sealed inputs (issue #362, mirrors the bridge lib): when the
/// closure never links BIB.dll statically, the host facility DLL beside the
/// plug-in or in a dependency root is appended as one authenticated
/// dependency; `Film Stocks` data files beside the plug-in become sealed
/// data resources staged into `<sealed root>/Film Stocks/`.
fn extra_sealed_inputs(
    plugin: &Path,
    dependencies: &mut Vec<ApprovedImageArtifact>,
    roots: &[PathBuf],
) -> Vec<SealedResourceEntry> {
    let has_bib = dependencies.iter().any(|dependency| {
        dependency
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("bib.dll"))
    });
    if !has_bib {
        let mut candidates: Vec<PathBuf> = plugin
            .parent()
            .map(|parent| parent.join("BIB.dll"))
            .into_iter()
            .collect();
        candidates.extend(roots.iter().map(|root| root.join("BIB.dll")));
        if let Some(bib) = candidates.into_iter().find(|path| path.is_file()) {
            let bytes = std::fs::read(&bib).expect("read BIB.dll");
            dependencies.push(ApprovedImageArtifact {
                path: bib,
                expected_sha256: Sha256::digest(&bytes).into(),
                expected_size: bytes.len() as u64,
            });
        }
    }
    let mut resources = Vec::new();
    let film_stocks = plugin
        .parent()
        .map(|parent| parent.join("Film Stocks"))
        .filter(|dir| dir.is_dir());
    if let Some(dir) = film_stocks {
        for entry in std::fs::read_dir(&dir).expect("read Film Stocks") {
            let path = entry.expect("Film Stocks entry").path();
            let metadata = std::fs::symlink_metadata(&path).expect("Film Stocks metadata");
            if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let bytes = std::fs::read(&path).expect("read grain file");
            resources.push(SealedResourceEntry {
                source: path,
                relative_path: format!("Film Stocks/{name}"),
                expected_sha256: Sha256::digest(&bytes).into(),
                expected_size: bytes.len() as u64,
            });
        }
    }
    resources
}

/// Cluster session bounds (issue #405, mirror of the bridge lib).
const MAX_CLUSTER_PLUGINS: usize = 256;
const MAX_CLUSTER_MODULE_BOUND: usize = 4096;
const CLUSTER_MODULE_HEADROOM: usize = 256;
const CLUSTER_INSPECT_DEADLINE: std::time::Duration = std::time::Duration::from_secs(300);
/// One-member cluster routing (issue #362, mirror of the bridge lib): a
/// singleton with `deps + SYSTEM_TAIL_ESTIMATE` over the one-shot
/// 128-module audit cap goes through a one-member DiscoverySession instead
/// of failing the one-shot audit.
const ONESHOT_AUDIT_MODULE_LIMIT: usize = 128;
const SYSTEM_TAIL_ESTIMATE: usize = 66;

struct Options {
    scan: PathBuf,
    dependency_dirs: Vec<PathBuf>,
    seal: bool,
    cluster: bool,
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
    let mut cluster = false;
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
            "--cluster" => cluster = true,
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
        "usage: discover_sweep <scan-dir> [--deps <dir>] [--no-deps] [--cluster] [--limit n] [--jobs n] [--json path]",
    );
    if dependency_dirs.is_empty() {
        dependency_dirs.extend(default_dependency_dirs(&scan));
    }
    if cluster && !seal {
        panic!("--cluster needs sealed closures; drop --no-deps");
    }
    Options {
        scan,
        dependency_dirs,
        seal,
        cluster,
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
    let mut dependencies = dependencies;
    let resources = if options.seal {
        extra_sealed_inputs(plugin, &mut dependencies, &roots)
    } else {
        Vec::new()
    };
    let bucket = match &closure {
        Some(Err(error)) => format!("closure_error: {error}"),
        Some(Ok(_)) | None => {
            match inspect_experimental_with_approved_dependencies_and_resources(
                repository,
                plugin,
                sha,
                dependencies.clone(),
                resources.clone(),
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
        "sweeping {} plug-ins (seal dependencies: {}, cluster: {}, search dirs: {}, jobs: {})",
        total,
        options.seal,
        options.cluster,
        options.dependency_dirs.len(),
        options.jobs
    );
    if options.cluster {
        run_cluster_sweep(&options, &repository, &plugins);
        return;
    }
    let sweep_started = Instant::now();

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
    println!("total sweep time: {} ms", sweep_started.elapsed().as_millis());
    if let Some(path) = options.json {
        let report = json!({
            "sealed_dependencies": options.seal,
            "cluster_sessions": false,
            "total_elapsed_ms": sweep_started.elapsed().as_millis(),
            "plugins": records,
            "buckets": buckets,
        });
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
}

/// One plug-in prepared for cluster discovery: hashed, its closure resolved,
/// and its cluster identity computed. Mirrors the bridge's prepare phase.
struct ClusterPrepared {
    plugin: PathBuf,
    sha: String,
    size_bytes: u64,
    dependencies: Vec<ApprovedImageArtifact>,
    sealed_bytes: u64,
    unresolved: usize,
    provenance: Vec<DependencyProvenance>,
    identity: String,
    sealed_resources: Vec<SealedResourceEntry>,
}

/// The closure identity (issue #405): a normalized hash of the sorted
/// `basename:sha256` pairs of the resolved dependency set.
fn closure_identity_of(dependencies: &[ApprovedImageArtifact]) -> String {
    let mut entries: Vec<String> = dependencies
        .iter()
        .map(|dependency| {
            let basename = dependency
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_lowercase();
            format!(
                "{}:{}",
                basename,
                dependency
                    .expected_sha256
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            )
        })
        .collect();
    entries.sort();
    format!("{:x}", Sha256::digest(entries.join("\n").as_bytes()))
}

/// The cluster-mode sweep (issue #405): plug-ins sharing one closure
/// identity are inspected inside a single DiscoverySession — one seal, one
/// worker, one closure LoadLibrary for the whole cluster — and the report
/// carries per-cluster open/inspect timings so a default run and a
/// `--cluster` run give a before/after comparison. Failures are fail-closed
/// (design §6): a member the session died on records
/// `cluster_session_invalidated`, and every not-yet-inspected member falls
/// back to the per-plugin one-shot inspect with a `cluster_fallback` note.
fn run_cluster_sweep(options: &Options, repository: &Path, plugins: &[PathBuf]) {
    let sweep_started = Instant::now();
    let mut records = Vec::new();
    let mut buckets: std::collections::BTreeMap<String, usize> = Default::default();
    let mut prepared: Vec<ClusterPrepared> = Vec::new();

    // Phase 1: hash + resolve every closure (the pre-inspect half).
    for plugin in plugins {
        let started = Instant::now();
        let file_bytes = std::fs::read(plugin).ok();
        let size_bytes = file_bytes.as_ref().map(|bytes| bytes.len() as u64);
        let sha = file_bytes
            .as_ref()
            .map(|bytes| format!("{:x}", Sha256::digest(bytes)));
        let Some(sha) = sha else {
            let record = plugin_record(
                &options.scan,
                plugin,
                "unreadable_file",
                started.elapsed().as_millis(),
                size_bytes,
                None,
                json!({}),
            );
            records.push(record);
            *buckets.entry("unreadable_file".into()).or_default() += 1;
            continue;
        };
        let mut roots: Vec<PathBuf> = Vec::new();
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
        match resolve_dependency_closure(DependencyClosureRequest::new(plugin, &roots)) {
            Ok(closure) => {
                // Extra sealed inputs (issue #362): BIB.dll for BIB-less
                // closures and Film Stocks data files; the identity covers
                // everything sealed into the tree.
                let mut dependencies = closure.dependencies().to_vec();
                let sealed_resources = extra_sealed_inputs(plugin, &mut dependencies, &roots);
                prepared.push(ClusterPrepared {
                    plugin: plugin.clone(),
                    sha,
                    size_bytes: size_bytes.unwrap_or(0),
                    identity: closure_identity_of(&dependencies),
                    dependencies,
                    sealed_bytes: closure.total_bytes(),
                    unresolved: closure.unresolved().len(),
                    provenance: closure.provenance().to_vec(),
                    sealed_resources,
                });
            }
            Err(error) => {
                let bucket = format!("closure_error: {error}");
                let record = plugin_record(
                    &options.scan,
                    plugin,
                    &bucket,
                    started.elapsed().as_millis(),
                    size_bytes,
                    Some(&sha),
                    json!({}),
                );
                records.push(record);
                *buckets.entry(bucket).or_default() += 1;
            }
        }
    }

    // Phase 2: group by identity; clusters of 2+ go through one
    // DiscoverySession, singletons whose closure would exceed the one-shot
    // 128-module audit cap go through a one-member session (issue #362),
    // everything else through the one-shot inspect.
    let mut groups: std::collections::BTreeMap<&str, Vec<usize>> = Default::default();
    for (index, member) in prepared.iter().enumerate() {
        groups.entry(member.identity.as_str()).or_default().push(index);
    }
    let mut cluster_reports = Vec::new();
    let mut singleton_indices = Vec::new();
    let mut cluster_indices = Vec::new();
    for indices in groups.values() {
        if indices.len() >= 2 && indices.len() <= MAX_CLUSTER_PLUGINS {
            cluster_indices.push(indices.clone());
        } else if indices.len() == 1
            && prepared[indices[0]].dependencies.len() + SYSTEM_TAIL_ESTIMATE
                > ONESHOT_AUDIT_MODULE_LIMIT
        {
            cluster_indices.push(indices.clone());
        } else {
            singleton_indices.extend(indices.iter().copied());
        }
    }

    let one_shot = |member: &ClusterPrepared, started: Instant, extra: Value| -> (String, Value) {
        match inspect_experimental_with_approved_dependencies_and_resources(
            repository,
            &member.plugin,
            &member.sha,
            member.dependencies.clone(),
            member.sealed_resources.clone(),
        ) {
            Ok((parameters, _)) => (
                "loaded".to_owned(),
                plugin_record(
                    &options.scan,
                    &member.plugin,
                    "loaded",
                    started.elapsed().as_millis(),
                    Some(member.size_bytes),
                    Some(&member.sha),
                    json!({
                        "parameters": parameters.len(),
                        "sealed": member.dependencies.len(),
                        "sealed_bytes": member.sealed_bytes,
                        "unresolved": member.unresolved,
                        "dependency_provenance": provenance_json(&member.provenance),
                        "cluster_identity": member.identity,
                        "cluster_fallback": extra,
                    }),
                ),
            ),
            Err(error) => {
                let bucket = bucket_of(&error.to_string());
                (
                    bucket.clone(),
                    plugin_record(
                        &options.scan,
                        &member.plugin,
                        &bucket,
                        started.elapsed().as_millis(),
                        Some(member.size_bytes),
                        Some(&member.sha),
                        json!({
                            "sealed": member.dependencies.len(),
                            "unresolved": member.unresolved,
                            "cluster_identity": member.identity,
                            "cluster_fallback": extra,
                        }),
                    ),
                )
            }
        }
    };

    for indices in &cluster_indices {
        let cluster_started = Instant::now();
        let members: Vec<&ClusterPrepared> = indices.iter().map(|index| &prepared[*index]).collect();
        let identity = members[0].identity.clone();
        let declared = members.len() + members[0].dependencies.len();
        // Data resources merge across members: identical entries (same path,
        // same bytes — siblings share one Film Stocks folder) stage once; the
        // same path with different bytes cannot coexist in one flat tree and
        // falls the whole cluster back per-plugin (fail-closed).
        let mut merged_resources: Vec<SealedResourceEntry> = Vec::new();
        let mut resource_collision = false;
        'members: for member in &members {
            for resource in &member.sealed_resources {
                let key = resource.relative_path.to_lowercase();
                match merged_resources
                    .iter()
                    .find(|existing| existing.relative_path.to_lowercase() == key)
                {
                    None => merged_resources.push(resource.clone()),
                    Some(existing)
                        if existing.expected_sha256 == resource.expected_sha256
                            && existing.expected_size == resource.expected_size => {}
                    Some(_) => {
                        resource_collision = true;
                        break 'members;
                    }
                }
            }
        }
        let infeasible = resource_collision
            || declared + CLUSTER_MODULE_HEADROOM > MAX_CLUSTER_MODULE_BOUND;
        let mut open_ms = 0u128;
        let mut fallback_note = Value::Null;
        let session = if infeasible {
            fallback_note = json!("cluster_infeasible");
            None
        } else {
            let open_started = Instant::now();
            let plugins_artifacts: Vec<ApprovedImageArtifact> = members
                .iter()
                .map(|member| ApprovedImageArtifact {
                    path: member.plugin.clone(),
                    expected_sha256: {
                        let bytes = member.sha.as_bytes();
                        let mut digest = [0u8; 32];
                        for (index, pair) in bytes.chunks_exact(2).enumerate() {
                            digest[index] = u8::from_str_radix(
                                std::str::from_utf8(pair).expect("hex"),
                                16,
                            )
                            .expect("hex");
                        }
                        digest
                    },
                    expected_size: member.size_bytes,
                })
                .collect();
            match DiscoverySession::open(DiscoverySessionOpenRequest {
                repository,
                plugins: plugins_artifacts,
                dependencies: members[0].dependencies.clone(),
                sealed_resources: merged_resources.clone(),
                module_bound: (declared + CLUSTER_MODULE_HEADROOM) as u32,
                inspect_deadline: CLUSTER_INSPECT_DEADLINE,
            }) {
                Ok(session) => {
                    open_ms = open_started.elapsed().as_millis();
                    Some(session)
                }
                Err(error) => {
                    open_ms = open_started.elapsed().as_millis();
                    fallback_note = json!(format!("cluster session open failed: {error}"));
                    None
                }
            }
        };

        let mut inspected = 0usize;
        let mut invalidation: Option<String> = None;
        if let Some(mut session) = session {
            for (index, member) in members.iter().enumerate() {
                let inspect_started = Instant::now();
                match session.inspect_plugin(index as u32, index as u32) {
                    Ok(InspectOutcome::Inspected { report }) => {
                        let parameters = report
                            .get("parameters")
                            .and_then(Value::as_array)
                            .map_or(0, Vec::len);
                        records.push(plugin_record(
                            &options.scan,
                            &member.plugin,
                            "loaded",
                            inspect_started.elapsed().as_millis(),
                            Some(member.size_bytes),
                            Some(&member.sha),
                            json!({
                                "parameters": parameters,
                                "sealed": member.dependencies.len(),
                                "cluster_identity": identity,
                                "cluster_inspect": true,
                            }),
                        ));
                        *buckets.entry("loaded".into()).or_default() += 1;
                        inspected += 1;
                    }
                    Ok(InspectOutcome::InspectError { error_kind, .. }) => {
                        let bucket = format!("cluster_inspect_{error_kind}");
                        records.push(plugin_record(
                            &options.scan,
                            &member.plugin,
                            &bucket,
                            inspect_started.elapsed().as_millis(),
                            Some(member.size_bytes),
                            Some(&member.sha),
                            json!({
                                "sealed": member.dependencies.len(),
                                "cluster_identity": identity,
                                "cluster_inspect": true,
                            }),
                        ));
                        *buckets.entry(bucket).or_default() += 1;
                        inspected += 1;
                    }
                    Err(error) => {
                        invalidation = Some(format!("{error}"));
                        records.push(plugin_record(
                            &options.scan,
                            &member.plugin,
                            "cluster_session_invalidated",
                            inspect_started.elapsed().as_millis(),
                            Some(member.size_bytes),
                            Some(&member.sha),
                            json!({
                                "sealed": member.dependencies.len(),
                                "cluster_identity": identity,
                                "cluster_fallback": format!("{error}"),
                            }),
                        ));
                        *buckets
                            .entry("cluster_session_invalidated".into())
                            .or_default() += 1;
                        inspected += 1;
                        break;
                    }
                }
            }
            let close = session.close();
            if invalidation.is_none() && close["session_clean"] != json!(true) {
                invalidation = Some("session close not clean".to_owned());
            }
        }
        // Fail-closed fallback (design §6): every member the session never
        // inspected — open failure, or everything past an invalidation — is
        // re-inspected per-plugin with a fallback note.
        if let Some(reason) = &invalidation {
            fallback_note = json!(reason.clone());
        }
        for member in members.iter().skip(inspected) {
            let (bucket, record) =
                one_shot(member, Instant::now(), fallback_note.clone());
            records.push(record);
            *buckets.entry(bucket).or_default() += 1;
        }
        let total_ms = cluster_started.elapsed().as_millis();
        eprintln!(
            "cluster {} ({} members): open {} ms, total {} ms{}",
            &identity[..8.min(identity.len())],
            members.len(),
            open_ms,
            total_ms,
            if invalidation.is_some() { " (fell back)" } else { "" },
        );
        cluster_reports.push(json!({
            "identity": identity,
            "members": members.len(),
            "open_ms": open_ms,
            "total_ms": total_ms,
            "inspected_in_session": inspected,
            "fallback": fallback_note,
        }));
    }

    for index in singleton_indices {
        let (bucket, record) = one_shot(&prepared[index], Instant::now(), Value::Null);
        records.push(record);
        *buckets.entry(bucket).or_default() += 1;
    }

    println!("\n=== summary ({} plug-ins, {} clusters) ===", records.len(), cluster_reports.len());
    for (bucket, count) in &buckets {
        println!("{count:5}  {bucket}");
    }
    for report in &cluster_reports {
        println!(
            "cluster {}: {} members, open {} ms, total {} ms",
            &report["identity"].as_str().unwrap_or("?")[..8],
            report["members"],
            report["open_ms"],
            report["total_ms"],
        );
    }
    println!("total sweep time: {} ms", sweep_started.elapsed().as_millis());
    if let Some(path) = &options.json {
        let report = json!({
            "sealed_dependencies": options.seal,
            "cluster_sessions": true,
            "total_elapsed_ms": sweep_started.elapsed().as_millis(),
            "clusters": cluster_reports,
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
