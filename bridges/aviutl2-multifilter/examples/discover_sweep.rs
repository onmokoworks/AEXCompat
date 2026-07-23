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
//!   --survey-only  only measure each closure's size; launch no workers
//!   --json <path>  write the per-plug-in records as JSON

use std::path::{Component, Path, PathBuf};
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
    json: Option<PathBuf>,
}

fn parse_options() -> Options {
    let mut args = std::env::args().skip(1);
    let mut scan = None;
    let mut dependency_dirs = Vec::new();
    let mut seal = true;
    let mut survey_only = false;
    let mut limit = usize::MAX;
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
            "--json" => json = Some(PathBuf::from(args.next().expect("--json needs a path"))),
            other => scan = Some(PathBuf::from(other)),
        }
    }
    let scan = scan.expect(
        "usage: discover_sweep <scan-dir> [--deps <dir>] [--no-deps] [--limit n] [--json path]",
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
    eprintln!(
        "sweeping {} plug-ins (seal dependencies: {}, search dirs: {})",
        plugins.len(),
        options.seal,
        options.dependency_dirs.len()
    );

    let mut records = Vec::new();
    let mut buckets: std::collections::BTreeMap<String, usize> = Default::default();
    for (index, plugin) in plugins.iter().enumerate() {
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
        if file_bytes.is_none() {
            records.push(record("unreadable_file", json!({})));
            *buckets.entry("unreadable_file".into()).or_default() += 1;
            continue;
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
            match survey_dependency_closure(plugin, &roots) {
                Ok(survey) => {
                    records.push(record(
                        "surveyed",
                        json!({
                            "closure_modules": survey.modules.len(),
                            "closure_bytes": survey.total_bytes,
                            "unresolved": survey.unresolved.len(),
                            "unreadable_images": survey.unreadable_images,
                            "dependency_provenance": provenance_json(&survey.provenance),
                        }),
                    ));
                    *buckets.entry("surveyed".into()).or_default() += 1;
                    eprintln!(
                        "[{}/{}] {} -> {} modules, {} bytes",
                        index + 1,
                        plugins.len(),
                        plugin.file_name().unwrap().to_string_lossy(),
                        survey.modules.len(),
                        survey.total_bytes
                    );
                }
                Err(error) => {
                    let bucket = format!("survey_error: {error}");
                    records.push(record(&bucket, json!({})));
                    *buckets.entry(bucket).or_default() += 1;
                }
            }
            continue;
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
        let bucket = match &closure {
            Some(Err(error)) => format!("closure_error: {error}"),
            Some(Ok(_)) | None => {
                match inspect_experimental_with_approved_dependencies_and_diagnostics(
                    &repository,
                    plugin,
                    &sha,
                    dependencies.clone(),
                ) {
                    Ok((parameters, _)) => {
                        records.push(record(
                            "loaded",
                            json!({
                                "parameters": parameters.len(),
                                "sealed": dependencies.len(),
                                "sealed_bytes": sealed_bytes,
                                "unresolved": unresolved,
                                "dependency_provenance": dependency_provenance,
                            }),
                        ));
                        *buckets.entry("loaded".into()).or_default() += 1;
                        eprintln!(
                            "[{}/{}] {} -> loaded ({} params, {} deps)",
                            index + 1,
                            plugins.len(),
                            plugin.file_name().unwrap().to_string_lossy(),
                            parameters.len(),
                            dependencies.len()
                        );
                        continue;
                    }
                    Err(error) => bucket_of(&error.to_string()),
                }
            }
        };
        records.push(record(
            &bucket,
            json!({
                "sealed": dependencies.len(),
                "sealed_bytes": sealed_bytes,
                "unresolved": unresolved,
                "dependency_provenance": dependency_provenance,
            }),
        ));
        *buckets.entry(bucket.clone()).or_default() += 1;
        eprintln!(
            "[{}/{}] {} -> {} ({} deps)",
            index + 1,
            plugins.len(),
            plugin.file_name().unwrap().to_string_lossy(),
            bucket,
            dependencies.len()
        );
    }

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
}
