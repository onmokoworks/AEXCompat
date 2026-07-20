#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, RichText};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant, SystemTime},
};

const DIAGNOSTIC_SCHEMA: &str = "aexcompat.harness-diagnostic";
const DIAGNOSTIC_VERSION: u64 = 1;
const MAX_DIAGNOSTIC_FILE_BYTES: u64 = 64 * 1024;
const MAX_DIAGNOSTIC_FILES: usize = 256;
const MAX_DIAGNOSTIC_TOTAL_BYTES: u64 = 8 * 1024 * 1024;
const MAX_DIAGNOSTIC_SUMMARY_BYTES: usize = 1024;
const MAX_AGGREGATE_SHAS: usize = 4096;
const MAX_AGGREGATE_FILES: usize = 65_536;
const MAX_AGGREGATE_BYTES: u64 = 64 * 1024 * 1024;

const SCATTERMAP_HASH: &str = "223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB";
const MASKOFFSET_HASH: &str = "B7C41F4F906FCE74B26BD2F06520F6BFDF75DCD1A2682DBB85D50D1DE833877B";

fn profile_for_hash(hash: &str) -> Option<&'static str> {
    match hash {
        SCATTERMAP_HASH => Some("scattermap"),
        MASKOFFSET_HASH => Some("maskoffset"),
        _ => None,
    }
}

fn decode_sha256(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("SHA-256 must be exactly 64 hexadecimal characters".into());
    }
    let mut digest = [0; 32];
    for (output, pair) in digest.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        let pair = std::str::from_utf8(pair).map_err(|error| error.to_string())?;
        *output = u8::from_str_radix(pair, 16).map_err(|error| error.to_string())?;
    }
    Ok(digest)
}

struct Selection {
    path: PathBuf,
    size: u64,
    sha256: String,
    profile: Option<&'static str>,
    modified: Option<SystemTime>,
}

#[derive(Clone, Debug)]
struct SessionDependency {
    path: PathBuf,
    size: u64,
    sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum ImportKind {
    Normal,
    Delay,
}

impl ImportKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Delay => "delay",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreflightImportWarning {
    basename: String,
    kind: ImportKind,
}

#[derive(Debug, Default)]
struct AdjacentImportDiscovery {
    dependencies: Vec<SessionDependency>,
    warnings: Vec<PreflightImportWarning>,
}

fn resolve_adjacent_import(
    imported_name: &str,
    kind: ImportKind,
    adjacent: &std::collections::HashMap<String, PathBuf>,
    warnings: &mut std::collections::BTreeMap<(String, ImportKind), String>,
) -> Option<PathBuf> {
    if is_system_import_name(imported_name) {
        return None;
    }
    let imported_path = Path::new(imported_name);
    if !imported_name.is_ascii()
        || imported_path.file_name().and_then(|name| name.to_str()) != Some(imported_name)
    {
        return None;
    }
    let key = imported_name.to_ascii_lowercase();
    match adjacent.get(&key) {
        Some(path) => Some(path.clone()),
        None => {
            warnings
                .entry((key, kind))
                .or_insert_with(|| imported_name.to_owned());
            None
        }
    }
}

const MAX_DISCOVERY_FILE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DISCOVERY_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ADJACENT_ENTRIES: usize = 4096;

fn read_bounded_pe(path: &Path) -> Result<Vec<u8>, String> {
    let size = fs::metadata(path).map_err(|error| error.to_string())?.len();
    if size == 0 || size > MAX_DISCOVERY_FILE_BYTES {
        return Err(format!(
            "PE image size must be 1..={MAX_DISCOVERY_FILE_BYTES} bytes: {}",
            path.display()
        ));
    }
    fs::read(path).map_err(|error| error.to_string())
}

fn is_system_import_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("api-ms-win-") || lower.starts_with("ext-ms-win-") {
        return true;
    }
    std::env::var_os("WINDIR")
        .is_some_and(|windows| PathBuf::from(windows).join("System32").join(name).is_file())
}

fn parse_delay_import_table(
    bytes: &[u8],
    table_offset: usize,
    size: usize,
    image_base: u64,
    is_64: bool,
    mut rva_to_offset: impl FnMut(usize) -> Option<usize>,
) -> Result<Vec<String>, String> {
    const DESCRIPTOR_SIZE: usize = 32;
    const MAX_DESCRIPTORS: usize = 2048;
    const MAX_DLL_NAME: usize = 260;
    if size < DESCRIPTOR_SIZE
        || size > DESCRIPTOR_SIZE * MAX_DESCRIPTORS
        || size % DESCRIPTOR_SIZE != 0
    {
        return Err("PE delay-import directory has an invalid size".into());
    }
    let table_end = table_offset
        .checked_add(size)
        .filter(|end| *end <= bytes.len())
        .ok_or("PE delay-import directory exceeds file bounds")?;
    let mut libraries = Vec::new();
    let mut terminated = false;
    for descriptor in bytes[table_offset..table_end]
        .chunks_exact(DESCRIPTOR_SIZE)
        .take(MAX_DESCRIPTORS)
    {
        let read_u32 =
            |offset: usize| u32::from_le_bytes(descriptor[offset..offset + 4].try_into().unwrap());
        let attributes = read_u32(0);
        let name_pointer = read_u32(4);
        if descriptor.iter().all(|byte| *byte == 0) {
            terminated = true;
            break;
        }
        if attributes & !1 != 0 || name_pointer == 0 {
            return Err("PE delay-import descriptor is malformed".into());
        }
        let name_rva = if attributes & 1 == 1 {
            u64::from(name_pointer)
        } else {
            if is_64 {
                return Err("PE32+ delay-import descriptors must use RVA attributes".into());
            }
            u64::from(name_pointer)
                .checked_sub(image_base)
                .ok_or("PE delay-import VA precedes image base")?
        };
        let name_rva = usize::try_from(name_rva)
            .map_err(|_| "PE delay-import name RVA does not fit this host")?;
        let name_offset =
            rva_to_offset(name_rva).ok_or("PE delay-import DLL name does not map to file data")?;
        let tail = bytes
            .get(name_offset..)
            .ok_or("PE delay-import DLL name exceeds file bounds")?;
        let length = tail
            .iter()
            .take(MAX_DLL_NAME + 1)
            .position(|byte| *byte == 0)
            .filter(|length| *length > 0 && *length <= MAX_DLL_NAME)
            .ok_or("PE delay-import DLL name is not bounded and NUL-terminated")?;
        let name = std::str::from_utf8(&tail[..length])
            .map_err(|_| "PE delay-import DLL name is not UTF-8/ASCII")?;
        let path = Path::new(name);
        if path.file_name().and_then(|value| value.to_str()) != Some(name)
            || !path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("dll"))
        {
            return Err("PE delay-import name must be a DLL basename".into());
        }
        libraries.push(name.to_owned());
    }
    if !terminated {
        return Err("PE delay-import directory has no zero terminator".into());
    }
    libraries.sort_by_key(|name| name.to_ascii_lowercase());
    libraries.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    Ok(libraries)
}

fn delay_import_libraries(bytes: &[u8], pe: &goblin::pe::PE<'_>) -> Result<Vec<String>, String> {
    let Some(optional) = pe.header.optional_header.as_ref() else {
        return Err("PE image has no optional header".into());
    };
    let Some(directory) = optional.data_directories.get_delay_import_descriptor() else {
        return Ok(Vec::new());
    };
    let options = goblin::pe::options::ParseOptions::default();
    let table_offset = goblin::pe::utils::find_offset(
        directory.virtual_address as usize,
        &pe.sections,
        optional.windows_fields.file_alignment,
        &options,
    )
    .ok_or("PE delay-import directory does not map to file data")?;
    parse_delay_import_table(
        bytes,
        table_offset,
        directory.size as usize,
        optional.windows_fields.image_base,
        pe.is_64,
        |rva| {
            goblin::pe::utils::find_offset(
                rva,
                &pe.sections,
                optional.windows_fields.file_alignment,
                &options,
            )
        },
    )
}

fn discover_adjacent_imports(aex_path: &Path) -> Result<AdjacentImportDiscovery, String> {
    const MAX_DEPENDENCIES: usize = 64;
    let directory = aex_path
        .parent()
        .ok_or("Selected AEX has no parent directory")?;
    let mut adjacent = std::collections::HashMap::new();
    for (entry_index, entry) in fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .enumerate()
    {
        if entry_index == MAX_ADJACENT_ENTRIES {
            return Err(format!(
                "AEX directory exceeds {MAX_ADJACENT_ENTRIES} entries"
            ));
        }
        let path = entry.map_err(|error| error.to_string())?.path();
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("dll"))
        {
            let Some(name) = path
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_owned)
            else {
                continue;
            };
            if !name.is_ascii() {
                continue;
            }
            let key = name.to_ascii_lowercase();
            if adjacent.insert(key, path).is_some() {
                return Err(format!(
                    "Ambiguous case-insensitive dependency name: {name}"
                ));
            }
        }
    }

    let mut pending = vec![aex_path.to_path_buf()];
    let mut visited = std::collections::HashSet::new();
    let mut dependencies = Vec::new();
    let mut warnings = std::collections::BTreeMap::new();
    let mut total_dependency_bytes = 0u64;
    while let Some(module_path) = pending.pop() {
        let bytes = read_bounded_pe(&module_path)?;
        let pe = goblin::pe::PE::parse(&bytes).map_err(|error| {
            format!(
                "Could not inspect PE imports for {}: {error}",
                module_path.display()
            )
        })?;
        let normal_libraries = pe
            .libraries
            .iter()
            .map(|name| ((*name).to_owned(), ImportKind::Normal))
            .collect::<Vec<_>>();
        let mut imported_libraries = normal_libraries;
        imported_libraries.extend(
            delay_import_libraries(&bytes, &pe)?
                .into_iter()
                .map(|name| (name, ImportKind::Delay)),
        );
        for (imported_name, kind) in imported_libraries {
            let key = imported_name.to_ascii_lowercase();
            let Some(path) =
                resolve_adjacent_import(&imported_name, kind, &adjacent, &mut warnings)
            else {
                continue;
            };
            if !visited.insert(key) {
                continue;
            }
            if dependencies.len() == MAX_DEPENDENCIES {
                return Err(format!(
                    "Adjacent dependency graph exceeds {MAX_DEPENDENCIES} DLLs"
                ));
            }
            let dependency_bytes = read_bounded_pe(&path)?;
            total_dependency_bytes = total_dependency_bytes
                .checked_add(dependency_bytes.len() as u64)
                .filter(|total| *total <= MAX_DISCOVERY_TOTAL_BYTES)
                .ok_or("Adjacent dependency graph exceeds the 1 GiB byte limit")?;
            dependencies.push(SessionDependency {
                path: path.clone(),
                size: dependency_bytes.len() as u64,
                sha256: format!("{:X}", Sha256::digest(&dependency_bytes)),
            });
            pending.push(path);
        }
    }
    dependencies.sort_by(|left, right| {
        left.path
            .to_string_lossy()
            .to_ascii_lowercase()
            .cmp(&right.path.to_string_lossy().to_ascii_lowercase())
    });
    let mut warnings = warnings
        .into_iter()
        .map(|((_, kind), basename)| PreflightImportWarning { basename, kind })
        .collect::<Vec<_>>();
    warnings.sort_by(|left, right| {
        left.basename
            .to_ascii_lowercase()
            .cmp(&right.basename.to_ascii_lowercase())
            .then_with(|| left.kind.cmp(&right.kind))
    });
    Ok(AdjacentImportDiscovery {
        dependencies,
        warnings,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DispatchIdentity {
    sha256: String,
    size: u64,
}

struct TaskResult {
    success: bool,
    body: String,
    output: Option<PathBuf>,
    identity: Option<DispatchIdentity>,
    operation: Option<String>,
    diagnostic_eligible: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DiagnosticHistory {
    count: usize,
    latest: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct MissingSuiteGap {
    name: String,
    version: i32,
    sha_count: usize,
    event_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct MissingSuiteAggregate {
    top: Vec<MissingSuiteGap>,
    discovered_sha_count: usize,
    scanned_sha_count: usize,
    valid_failure_event_count: usize,
    skipped_count: usize,
    truncated: bool,
}

fn is_plain_directory(metadata: &fs::Metadata) -> bool {
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 == 0
    }
    #[cfg(not(windows))]
    true
}

fn is_plain_file(metadata: &fs::Metadata) -> bool {
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 == 0
    }
    #[cfg(not(windows))]
    true
}

fn validated_event_suites(value: &serde_json::Value, sha: &str) -> Option<Vec<MissingSuite>> {
    let valid = value.as_object().is_some_and(|object| object.len() == 8)
        && value.get("schema").and_then(|v| v.as_str()) == Some(DIAGNOSTIC_SCHEMA)
        && value.get("version").and_then(|v| v.as_u64()) == Some(DIAGNOSTIC_VERSION)
        && value.get("timestamp").and_then(|v| v.as_u64()).is_some()
        && value.get("success").and_then(|v| v.as_bool()) == Some(false)
        && value
            .get("operation")
            .and_then(|v| v.as_str())
            .is_some_and(|v| !v.is_empty() && v.len() <= MAX_DIAGNOSTIC_SUMMARY_BYTES)
        && value
            .get("summary")
            .and_then(|v| v.as_str())
            .is_some_and(|v| v.len() <= MAX_DIAGNOSTIC_SUMMARY_BYTES)
        && value
            .get("identity")
            .and_then(|v| v.as_object())
            .is_some_and(|identity| identity.len() == 2)
        && value.pointer("/identity/sha256").and_then(|v| v.as_str()) == Some(sha)
        && value
            .pointer("/identity/size")
            .and_then(|v| v.as_u64())
            .is_some()
        && value
            .get("diagnostics")
            .and_then(|v| v.as_object())
            .is_some();
    if !valid {
        return None;
    }
    Some(
        value
            .pointer("/diagnostics/missing_suites")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|suite| {
                let name = suite.get("name")?.as_str()?;
                let version = i32::try_from(suite.get("version")?.as_i64()?).ok()?;
                (version > 0 && valid_suite_name(name)).then(|| MissingSuite {
                    name: name.to_owned(),
                    version,
                })
            })
            .fold(Vec::new(), |mut suites, suite| {
                if suites.len() < 16 && !suites.contains(&suite) {
                    suites.push(suite);
                }
                suites
            }),
    )
}

fn aggregate_missing_suites(repository: &Path) -> MissingSuiteAggregate {
    let root = repository.join("target/harness-diagnostics");
    let Ok(entries) = fs::read_dir(root) else {
        return MissingSuiteAggregate::default();
    };
    let mut aggregate = MissingSuiteAggregate::default();
    let mut directories = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            aggregate.skipped_count += 1;
            continue;
        };
        let name = entry.file_name();
        let Some(sha) = name.to_str().filter(|name| decode_sha256(name).is_ok()) else {
            aggregate.skipped_count += 1;
            continue;
        };
        let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
            aggregate.skipped_count += 1;
            continue;
        };
        if !is_plain_directory(&metadata) {
            aggregate.skipped_count += 1;
            continue;
        }
        aggregate.discovered_sha_count += 1;
        if directories.len() == MAX_AGGREGATE_SHAS {
            aggregate.truncated = true;
            continue;
        }
        directories.push((sha.to_ascii_lowercase(), entry.path()));
    }
    directories.sort_by(|left, right| left.0.cmp(&right.0));
    let mut counts = std::collections::BTreeMap::<(String, i32), (usize, usize)>::new();
    let mut file_count = 0usize;
    let mut total_bytes = 0u64;
    for (sha, directory) in directories {
        aggregate.scanned_sha_count += 1;
        let Ok(files) = fs::read_dir(directory) else {
            aggregate.skipped_count += 1;
            continue;
        };
        let mut seen_for_sha = std::collections::BTreeSet::new();
        for file in files {
            if file_count == MAX_AGGREGATE_FILES {
                aggregate.truncated = true;
                break;
            }
            let Ok(file) = file else {
                aggregate.skipped_count += 1;
                continue;
            };
            let path = file.path();
            if !file
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".local.json"))
            {
                aggregate.skipped_count += 1;
                continue;
            }
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                aggregate.skipped_count += 1;
                continue;
            };
            if !is_plain_file(&metadata)
                || metadata.len() > MAX_DIAGNOSTIC_FILE_BYTES
                || total_bytes.saturating_add(metadata.len()) > MAX_AGGREGATE_BYTES
            {
                aggregate.skipped_count += 1;
                if total_bytes.saturating_add(metadata.len()) > MAX_AGGREGATE_BYTES {
                    aggregate.truncated = true;
                }
                continue;
            }
            file_count += 1;
            total_bytes += metadata.len();
            let Ok(file) = fs::File::open(path) else {
                aggregate.skipped_count += 1;
                continue;
            };
            let mut bytes = Vec::new();
            if file
                .take(MAX_DIAGNOSTIC_FILE_BYTES + 1)
                .read_to_end(&mut bytes)
                .is_err()
            {
                aggregate.skipped_count += 1;
                continue;
            }
            let Some(suites) = serde_json::from_slice(&bytes)
                .ok()
                .as_ref()
                .and_then(|value| validated_event_suites(value, &sha))
            else {
                aggregate.skipped_count += 1;
                continue;
            };
            aggregate.valid_failure_event_count += 1;
            for suite in suites {
                let key = (suite.name, suite.version);
                counts.entry(key.clone()).or_default().1 += 1;
                seen_for_sha.insert(key);
            }
        }
        for key in seen_for_sha {
            counts.entry(key).or_default().0 += 1;
        }
        if file_count == MAX_AGGREGATE_FILES {
            break;
        }
    }
    aggregate.top = counts
        .into_iter()
        .map(
            |((name, version), (sha_count, event_count))| MissingSuiteGap {
                name,
                version,
                sha_count,
                event_count,
            },
        )
        .collect();
    aggregate.top.sort_by(|left, right| {
        right
            .sha_count
            .cmp(&left.sha_count)
            .then_with(|| right.event_count.cmp(&left.event_count))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.version.cmp(&right.version))
    });
    aggregate.top.truncate(10);
    aggregate
}

fn bounded_summary(value: &str) -> String {
    if value.contains(['\\', '/', ':']) {
        return "redacted".into();
    }
    let mut output = value
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_DIAGNOSTIC_SUMMARY_BYTES)
        .collect::<String>();
    while output.len() > MAX_DIAGNOSTIC_SUMMARY_BYTES {
        output.pop();
    }
    output
}

fn summary_component(value: &str) -> String {
    bounded_summary(
        &value
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.') {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>(),
    )
}

fn diagnostic_summary(success: bool, body: &str) -> String {
    if success {
        if let Some(value) = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .as_ref()
            .and_then(render_diagnostics)
        {
            return bounded_summary(&format!(
                "render_path={}; pixel_format={}; classification={}; gpu_fallback={}",
                summary_component(&value.render_path),
                summary_component(&value.pixel_format),
                summary_component(&value.worker_classification),
                value.gpu_fallback_used
            ));
        }
        return "completed".into();
    }
    if let Some(value) = failure_diagnostics(body) {
        return bounded_summary(&format!(
            "classification={}; stage={}; exit_code={}",
            summary_component(&value.classification),
            summary_component(value.failure_stage.as_deref().unwrap_or("unknown")),
            value
                .exit_code
                .map(|code| code.to_string())
                .as_deref()
                .unwrap_or("unknown")
        ));
    }
    "failed safely".into()
}

fn diagnostic_details(success: bool, body: &str) -> serde_json::Value {
    if let Some(value) = (!success).then(|| failure_diagnostics(body)).flatten() {
        return serde_json::json!({
            "classification": value.classification,
            "failure_stage": value.failure_stage,
            "exit_code": value.exit_code,
            "selector_error": value.selector_error,
            "last_seh_selector": value.last_seh_selector,
            "last_seh_error": value.last_seh_error,
            "last_seh_exception_code": value.last_seh_exception_code,
            "missing_suites": value.missing_suites.into_iter().map(|suite| {
                serde_json::json!({"name": suite.name, "version": suite.version})
            }).collect::<Vec<_>>(),
        });
    }
    if let Some(value) = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .as_ref()
        .and_then(render_diagnostics)
    {
        return serde_json::json!({
            "classification": value.worker_classification,
            "render_path": value.render_path,
            "pixel_format": value.pixel_format,
            "gpu_fallback_used": value.gpu_fallback_used,
        });
    }
    serde_json::json!({"classification": if success { "completed" } else { "failed_safely" }})
}

fn diagnostic_directory(repository: &Path, sha256: &str) -> Option<PathBuf> {
    decode_sha256(sha256).ok()?;
    Some(
        repository
            .join("target/harness-diagnostics")
            .join(sha256.to_ascii_lowercase()),
    )
}

fn persist_diagnostic_with_nonce(
    repository: &Path,
    identity: &DispatchIdentity,
    operation: &str,
    success: bool,
    summary: &str,
    details: &serde_json::Value,
    nonce: &str,
) -> Result<PathBuf, String> {
    let directory =
        diagnostic_directory(repository, &identity.sha256).ok_or("invalid dispatch SHA-256")?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let destination = directory.join(format!("{nonce}.local.json"));
    if destination.exists() {
        return Err("diagnostic event collision".into());
    }
    let temporary = directory.join(format!("{nonce}.tmp"));
    let dto = serde_json::json!({
        "schema": DIAGNOSTIC_SCHEMA,
        "version": DIAGNOSTIC_VERSION,
        "identity": {"sha256": identity.sha256.to_ascii_lowercase(), "size": identity.size},
        "operation": bounded_summary(operation),
        "timestamp": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)
            .map(|value| value.as_millis()).unwrap_or_default(),
        "success": success,
        "summary": bounded_summary(summary),
        "diagnostics": details,
    });
    let bytes = serde_json::to_vec(&dto).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    drop(file);
    if destination.exists() {
        let _ = fs::remove_file(&temporary);
        return Err("diagnostic event collision".into());
    }
    fs::rename(&temporary, &destination).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        error.to_string()
    })?;
    Ok(destination)
}

fn persist_diagnostic(
    repository: &Path,
    identity: &DispatchIdentity,
    operation: &str,
    success: bool,
    summary: &str,
    details: &serde_json::Value,
) -> Result<PathBuf, String> {
    let nonce = format!(
        "{}-{}-{:?}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default(),
        std::process::id(),
        thread::current().id()
    )
    .replace(['(', ')', ' '], "");
    persist_diagnostic_with_nonce(
        repository, identity, operation, success, summary, details, &nonce,
    )
}

fn persist_preflight_warnings(
    repository: &Path,
    identity: &DispatchIdentity,
    warnings: &[PreflightImportWarning],
) -> Result<(), String> {
    if warnings.is_empty() {
        return Ok(());
    }
    let details = serde_json::json!({
        "preflight_warnings": warnings.iter().map(|warning| serde_json::json!({
            "basename": warning.basename,
            "kind": warning.kind.as_str(),
        })).collect::<Vec<_>>()
    });
    persist_diagnostic(
        repository,
        identity,
        "preflight_import_discovery",
        true,
        "non-system imports may be unavailable",
        &details,
    )?;
    Ok(())
}

fn load_diagnostic_history(repository: &Path, sha256: &str) -> DiagnosticHistory {
    let Some(directory) = diagnostic_directory(repository, sha256) else {
        return DiagnosticHistory::default();
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return DiagnosticHistory::default();
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".local.json"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.reverse();
    let mut history = DiagnosticHistory::default();
    let mut total = 0u64;
    for path in paths.into_iter().take(MAX_DIAGNOSTIC_FILES) {
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.len() > MAX_DIAGNOSTIC_FILE_BYTES
            || total.saturating_add(metadata.len()) > MAX_DIAGNOSTIC_TOTAL_BYTES
        {
            continue;
        }
        total += metadata.len();
        let Ok(file) = fs::File::open(&path) else {
            continue;
        };
        let mut bytes = Vec::new();
        if file
            .take(MAX_DIAGNOSTIC_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() as u64 > MAX_DIAGNOSTIC_FILE_BYTES
        {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let valid = value.as_object().is_some_and(|object| object.len() == 8)
            && value.get("schema").and_then(|v| v.as_str()) == Some(DIAGNOSTIC_SCHEMA)
            && value.get("version").and_then(|v| v.as_u64()) == Some(DIAGNOSTIC_VERSION)
            && value.get("timestamp").and_then(|v| v.as_u64()).is_some()
            && value.get("success").and_then(|v| v.as_bool()).is_some()
            && value
                .get("diagnostics")
                .and_then(|v| v.as_object())
                .is_some()
            && value
                .get("operation")
                .and_then(|v| v.as_str())
                .is_some_and(|v| !v.is_empty() && v.len() <= MAX_DIAGNOSTIC_SUMMARY_BYTES)
            && value
                .get("summary")
                .and_then(|v| v.as_str())
                .is_some_and(|v| v.len() <= MAX_DIAGNOSTIC_SUMMARY_BYTES)
            && value
                .get("identity")
                .and_then(|v| v.as_object())
                .is_some_and(|identity| identity.len() == 2)
            && value
                .pointer("/identity/size")
                .and_then(|v| v.as_u64())
                .is_some()
            && value
                .pointer("/identity/sha256")
                .and_then(|v| v.as_str())
                .is_some_and(|value| value == sha256.to_ascii_lowercase());
        let Some(summary) = valid
            .then(|| value.get("summary").and_then(|v| v.as_str()))
            .flatten()
        else {
            continue;
        };
        history.count += 1;
        if history.latest.is_none() {
            history.latest = Some(bounded_summary(summary));
        }
    }
    history
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum TaskKind {
    #[default]
    Generic,
    IdentifyAex,
    InspectParameters,
}

#[derive(Debug, Default, PartialEq)]
struct RenderDiagnostics {
    render_path: String,
    pixel_format: String,
    worker_classification: String,
    gpu_fallback_used: bool,
    gpu_attempt_classification: Option<String>,
    gpu_failure_stage: Option<String>,
    final_stages: Vec<String>,
    gpu_stages: Vec<String>,
}

#[derive(Debug, PartialEq)]
struct FailureDiagnostics {
    classification: String,
    failure_stage: Option<String>,
    exit_code: Option<i64>,
    elapsed_ms: Option<u64>,
    selector_error: Option<i64>,
    missing_suites: Vec<MissingSuite>,
    last_seh_selector: Option<String>,
    last_seh_error: Option<i64>,
    last_seh_exception_code: Option<u64>,
    stages: Vec<String>,
}

#[derive(Debug, PartialEq)]
struct MissingSuite {
    name: String,
    version: i32,
}

#[derive(Debug, Clone, PartialEq)]
struct PixelComparison {
    width: u32,
    height: u32,
    differing_pixels: u64,
    max_channel_error: u8,
    mean_absolute_error: f64,
}

impl PixelComparison {
    fn exact(&self) -> bool {
        self.differing_pixels == 0
    }

    fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "stage": "ae_reference_pixel_comparison",
            "status": if self.exact() { "pixel_exact" } else { "pixel_difference" },
            "pixel_exact": self.exact(),
            "width": self.width,
            "height": self.height,
            "pixel_count": u64::from(self.width) * u64::from(self.height),
            "differing_pixels": self.differing_pixels,
            "max_channel_error": self.max_channel_error,
            "mean_absolute_error": self.mean_absolute_error,
        })
    }
}

fn compare_images(reference: &Path, output: &Path) -> Result<PixelComparison, String> {
    let reference = image::open(reference)
        .map_err(|error| format!("Reference image could not be decoded: {error}"))?
        .into_rgba8();
    let output = image::open(output)
        .map_err(|error| format!("AEX output could not be decoded: {error}"))?
        .into_rgba8();
    if reference.dimensions() != output.dimensions() {
        return Err(format!(
            "Image size mismatch: AE reference is {}x{}, AEX output is {}x{}",
            reference.width(),
            reference.height(),
            output.width(),
            output.height()
        ));
    }
    let mut differing_pixels = 0u64;
    let mut absolute_error = 0u64;
    let mut max_channel_error = 0u8;
    for (reference, output) in reference
        .as_raw()
        .chunks_exact(4)
        .zip(output.as_raw().chunks_exact(4))
    {
        let mut pixel_differs = false;
        for channel in 0..4 {
            let error = reference[channel].abs_diff(output[channel]);
            absolute_error += u64::from(error);
            max_channel_error = max_channel_error.max(error);
            pixel_differs |= error != 0;
        }
        differing_pixels += u64::from(pixel_differs);
    }
    let channel_count = u64::from(reference.width()) * u64::from(reference.height()) * 4;
    Ok(PixelComparison {
        width: reference.width(),
        height: reference.height(),
        differing_pixels,
        max_channel_error,
        mean_absolute_error: absolute_error as f64 / channel_count.max(1) as f64,
    })
}

fn assign_layer_paths(
    parameters: &mut [aexcompat_broker::image_render::InteractiveParameter],
    assignments: &[std::ffi::OsString],
) -> Result<(), String> {
    if assignments.is_empty() || assignments.len() % 2 != 0 {
        return Err("layer assignments must be SLOT IMAGE pairs".into());
    }
    let mut assigned_slots = Vec::new();
    let mut planned = Vec::new();
    for assignment in assignments.chunks_exact(2) {
        let slot = assignment[0]
            .to_string_lossy()
            .parse::<u32>()
            .map_err(|_| "layer slot must be a positive integer".to_owned())?;
        if slot == 0 {
            return Err("layer slot must be a positive integer".into());
        }
        if assigned_slots.contains(&slot) {
            return Err(format!("layer slot {slot} was assigned more than once"));
        }
        let index = parameters
            .iter()
            .position(|item| item.slot == slot)
            .ok_or_else(|| format!("AEX exposes no parameter at slot {slot}"))?;
        if parameters[index].kind != "layer" {
            return Err(format!("parameter slot {slot} is not a Layer input"));
        }
        planned.push((index, PathBuf::from(&assignment[1])));
        assigned_slots.push(slot);
    }
    for (index, path) in planned {
        parameters[index].layer_path = Some(path);
    }
    Ok(())
}

fn apply_typed_assignments(
    parameters: &mut Vec<aexcompat_broker::image_render::InteractiveParameter>,
    document: &serde_json::Value,
) -> Result<(), String> {
    let root = document
        .as_object()
        .ok_or_else(|| "assignment document must be an object".to_owned())?;
    if root.keys().any(|key| {
        !matches!(
            key.as_str(),
            "schema_version" | "assignments" | "timing" | "host_context"
        )
    }) || root
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return Err("assignment document schema is invalid".into());
    }
    let assignments = root
        .get("assignments")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "assignment document has no assignments array".to_owned())?;
    if assignments.len() > 1024 {
        return Err("assignment count exceeds the parameter limit".into());
    }
    let mut updated = parameters.clone();
    let mut assigned_slots = Vec::new();
    for assignment in assignments {
        let object = assignment
            .as_object()
            .ok_or_else(|| "each assignment must be an object".to_owned())?;
        if object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "slot" | "value" | "color" | "components" | "layer" | "text"
            )
        }) {
            return Err("assignment contains an unknown field".into());
        }
        let slot = object
            .get("slot")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value != 0)
            .ok_or_else(|| "assignment slot must be a positive integer".to_owned())?;
        if assigned_slots.contains(&slot) {
            return Err(format!("parameter slot {slot} was assigned more than once"));
        }
        let parameter = updated
            .iter_mut()
            .find(|item| item.slot == slot)
            .ok_or_else(|| format!("AEX exposes no parameter at slot {slot}"))?;
        let value_field_count = ["value", "color", "components", "layer", "text"]
            .into_iter()
            .filter(|field| object.contains_key(*field))
            .count();
        if value_field_count != 1 {
            return Err(format!(
                "parameter slot {slot} must have exactly one typed value"
            ));
        }
        match parameter.kind.as_str() {
            "integer" | "float" | "path" => {
                let value = object
                    .get("value")
                    .and_then(serde_json::Value::as_f64)
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| format!("parameter slot {slot} requires a finite value"))?;
                if value < parameter.minimum
                    || value > parameter.maximum
                    || (matches!(parameter.kind.as_str(), "integer" | "path")
                        && value.fract() != 0.0)
                {
                    return Err(format!("parameter slot {slot} value is out of range"));
                }
                parameter.value = value;
            }
            "color" => {
                let values = object
                    .get("color")
                    .and_then(serde_json::Value::as_array)
                    .filter(|values| values.len() == 4)
                    .ok_or_else(|| format!("parameter slot {slot} requires ARGB8 color"))?;
                let mut color = [0u8; 4];
                for (index, value) in values.iter().enumerate() {
                    color[index] = value
                        .as_u64()
                        .and_then(|value| u8::try_from(value).ok())
                        .ok_or_else(|| format!("parameter slot {slot} color is invalid"))?;
                }
                parameter.color = color;
            }
            "angle" | "point" | "point3d" => {
                let expected = match parameter.kind.as_str() {
                    "angle" => 1,
                    "point" => 2,
                    _ => 3,
                };
                let values = object
                    .get("components")
                    .and_then(serde_json::Value::as_array)
                    .filter(|values| values.len() == expected)
                    .ok_or_else(|| format!("parameter slot {slot} component count is invalid"))?;
                for (index, value) in values.iter().enumerate() {
                    let value = value
                        .as_f64()
                        .filter(|value| {
                            value.is_finite() && *value >= -32768.0 && *value <= 32768.0
                        })
                        .ok_or_else(|| format!("parameter slot {slot} component is invalid"))?;
                    parameter.components[index] = value;
                }
            }
            "layer" => {
                let path = object
                    .get("layer")
                    .and_then(serde_json::Value::as_str)
                    .filter(|path| !path.is_empty())
                    .ok_or_else(|| format!("parameter slot {slot} requires a layer path"))?;
                parameter.layer_path = Some(PathBuf::from(path));
            }
            "arbitrary_data" => {
                let text = object
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .filter(|text| !text.is_empty() && text.len() <= 4096 && !text.contains('\0'))
                    .ok_or_else(|| format!("parameter slot {slot} requires bounded text"))?;
                parameter.debug_summary = Some(text.to_owned());
            }
            _ => return Err(format!("parameter slot {slot} is not assignable")),
        }
        assigned_slots.push(slot);
    }
    *parameters = updated;
    Ok(())
}

fn typed_request_timing(
    document: &serde_json::Value,
) -> Result<aexcompat_broker::image_render::RenderTiming, String> {
    let Some(timing) = document.get("timing") else {
        return Ok(aexcompat_broker::image_render::RenderTiming::default());
    };
    let timing = timing
        .as_object()
        .ok_or_else(|| "timing must be an object".to_owned())?;
    if timing.keys().any(|key| {
        !matches!(
            key.as_str(),
            "frame" | "fps" | "time_scale" | "time_step" | "duration_frames"
        )
    }) {
        return Err("timing contains an unknown field".into());
    }
    let frame = timing
        .get("frame")
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .filter(|value| (0..=10_000_000).contains(value))
        .ok_or_else(|| "timing frame must be an integer within 0..=10000000".to_owned())?;
    let (time_scale, time_step) = match (
        timing.get("fps"),
        timing.get("time_scale"),
        timing.get("time_step"),
    ) {
        (Some(fps), None, None) => (
            fps.as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| (1..=1000).contains(value))
                .ok_or_else(|| "timing fps must be an integer within 1..=1000".to_owned())?,
            1,
        ),
        (None, Some(time_scale), Some(time_step)) => (
            time_scale
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| (1..=1_000_000).contains(value))
                .ok_or_else(|| {
                    "timing time_scale must be an integer within 1..=1000000".to_owned()
                })?,
            time_step
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .filter(|value| (1..=100_000).contains(value))
                .ok_or_else(|| {
                    "timing time_step must be an integer within 1..=100000".to_owned()
                })?,
        ),
        _ => return Err("timing requires either fps or the time_scale/time_step pair".to_owned()),
    };
    let duration_frames = match timing.get("duration_frames") {
        Some(value) => value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .filter(|value| *value > frame && *value <= 10_000_001)
            .ok_or_else(|| {
                "timing duration_frames must be an integer greater than frame and at most 10000001"
                    .to_owned()
            })?,
        None => frame.saturating_add(1),
    };
    render_timing(frame, duration_frames, time_scale, time_step)
}

fn typed_request_host_context(
    document: &serde_json::Value,
) -> Result<Option<aexcompat_broker::render_request::HostContext>, String> {
    document
        .get("host_context")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| format!("host_context is invalid: {error}"))
}

fn render_timing(
    frame: i32,
    duration_frames: i32,
    time_scale: u32,
    time_step: i32,
) -> Result<aexcompat_broker::image_render::RenderTiming, String> {
    if frame < 0
        || duration_frames <= frame
        || time_scale == 0
        || time_step <= 0
        || time_scale > 1_000_000
        || time_step > 100_000
    {
        return Err("render timing is outside the supported range".into());
    }
    let current_time = frame
        .checked_mul(time_step)
        .ok_or_else(|| "current_time exceeds the 32-bit AE time range".to_owned())?;
    let total_time = duration_frames
        .checked_mul(time_step)
        .ok_or_else(|| "total_time exceeds the 32-bit AE time range".to_owned())?;
    Ok(aexcompat_broker::image_render::RenderTiming {
        current_time,
        time_step,
        total_time,
        time_scale,
    })
}

fn typed_request_document(
    parameters: &[aexcompat_broker::image_render::InteractiveParameter],
    frame: i32,
    time_scale: u32,
    time_step: i32,
    duration_frames: i32,
    host_context: Option<&aexcompat_broker::render_request::HostContext>,
) -> serde_json::Value {
    let assignments = parameters
        .iter()
        .filter_map(|parameter| {
            let mut assignment = serde_json::Map::new();
            assignment.insert("slot".into(), serde_json::json!(parameter.slot));
            match parameter.kind.as_str() {
                "integer" | "float" | "path" => {
                    assignment.insert("value".into(), serde_json::json!(parameter.value));
                }
                "color" => {
                    assignment.insert("color".into(), serde_json::json!(parameter.color));
                }
                "angle" | "point" | "point3d" => {
                    let components = parameter.components.get(..parameter.component_count)?;
                    assignment.insert("components".into(), serde_json::json!(components));
                }
                "layer" => {
                    let path = parameter.layer_path.as_ref()?;
                    assignment.insert("layer".into(), serde_json::json!(path.to_string_lossy()));
                }
                "arbitrary_data" => {
                    let text = parameter.debug_summary.as_ref()?;
                    assignment.insert("text".into(), serde_json::json!(text));
                }
                _ => return None,
            }
            Some(serde_json::Value::Object(assignment))
        })
        .collect::<Vec<_>>();
    let timing = if time_step == 1 && time_scale <= 1000 {
        serde_json::json!({
            "frame": frame, "fps": time_scale, "duration_frames": duration_frames
        })
    } else {
        serde_json::json!({
            "frame": frame, "time_scale": time_scale, "time_step": time_step,
            "duration_frames": duration_frames
        })
    };
    let mut document = serde_json::json!({
        "schema_version": 1,
        "timing": timing,
        "assignments": assignments,
    });
    if let Some(context) = host_context {
        document["host_context"] = serde_json::to_value(context).unwrap_or_default();
    }
    document
}

impl Default for FailureDiagnostics {
    fn default() -> Self {
        Self {
            classification: "host_validation_error".into(),
            failure_stage: None,
            exit_code: None,
            elapsed_ms: None,
            selector_error: None,
            missing_suites: Vec::new(),
            last_seh_selector: None,
            last_seh_error: None,
            last_seh_exception_code: None,
            stages: Vec::new(),
        }
    }
}

#[derive(Debug, Default, PartialEq)]
struct MatrixCase {
    render_path: String,
    pixel_format: String,
    passed: bool,
    applicable: bool,
    classification: String,
    failure_stage: Option<String>,
    selector_error: Option<i64>,
    output_png: Option<String>,
    output_relation: Option<String>,
    differing_input_pixels: Option<u64>,
    error: Option<String>,
}

fn compatibility_matrix(report: &serde_json::Value) -> Option<Vec<MatrixCase>> {
    (report.get("stage")?.as_str()? == "effect_compatibility_matrix").then(|| {
        report["cases"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|case| MatrixCase {
                render_path: case["render_path"].as_str().unwrap_or("unknown").to_owned(),
                pixel_format: case["pixel_format"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_owned(),
                passed: case["passed"].as_bool().unwrap_or(false),
                applicable: case["applicable"].as_bool().unwrap_or(true),
                classification: case["classification"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_owned(),
                failure_stage: case["failure_stage"].as_str().map(str::to_owned),
                selector_error: case["selector_error"].as_i64(),
                output_png: case["output_png"].as_str().map(str::to_owned),
                output_relation: case["output_relation"].as_str().map(str::to_owned),
                differing_input_pixels: case["differing_input_pixels"].as_u64(),
                error: case["error"].as_str().map(str::to_owned),
            })
            .collect()
    })
}

fn run_effect_matrix(
    repository: &Path,
    plugin_path: &Path,
    hash: &str,
    input: &Path,
    output_root: &Path,
    parameters: &[aexcompat_broker::image_render::InteractiveParameter],
    timing: aexcompat_broker::image_render::RenderTiming,
    reference_root: Option<&Path>,
    host_context: Option<&aexcompat_broker::render_request::HostContext>,
) -> serde_json::Value {
    use aexcompat_broker::image_render::RenderPixelFormat;
    let formats = [
        (RenderPixelFormat::Argb8, "argb8"),
        (RenderPixelFormat::Argb16, "argb16"),
        (RenderPixelFormat::Argb32f, "argb32f"),
    ];
    let mut cases = Vec::with_capacity(6);
    for smart in [false, true] {
        for (pixel_format, format_name) in formats {
            let path_name = if smart { "smartfx" } else { "classic" };
            let output = output_root.join(format!("{path_name}-{format_name}.png"));
            let result =
                aexcompat_broker::image_render::render_experimental_image_at_time_with_format_and_context(
                    repository,
                    plugin_path,
                    hash,
                    input,
                    &output,
                    parameters,
                    timing,
                    smart,
                    pixel_format,
                    host_context,
                );
            cases.push(match result {
                Ok(report) => {
                    let mut case = serde_json::json!({
                        "render_path": path_name,
                        "pixel_format": format_name,
                        "render_completed": true,
                        "applicable": true,
                        "passed": true,
                        "classification": report["worker_classification"],
                        "failure_stage": serde_json::Value::Null,
                        "selector_error": serde_json::Value::Null,
                        "output_png": output,
                    });
                    if report["width"] != report["input_width"]
                        || report["height"] != report["input_height"]
                    {
                        case["output_relation"] = serde_json::json!("different_dimensions");
                        case["differing_input_pixels"] = serde_json::Value::Null;
                    } else {
                        match compare_images(input, &output) {
                            Ok(comparison) => {
                                case["output_relation"] =
                                    serde_json::json!(if comparison.exact() {
                                        "pixel_exact_passthrough"
                                    } else {
                                        "pixels_changed"
                                    });
                                case["differing_input_pixels"] =
                                    serde_json::json!(comparison.differing_pixels);
                            }
                            Err(error) => {
                                case["output_relation"] =
                                    serde_json::json!("comparison_unavailable");
                                case["output_comparison_error"] = serde_json::json!(error);
                            }
                        }
                    }
                    if let Some(reference_root) = reference_root {
                        let reference =
                            reference_root.join(format!("{path_name}-{format_name}.png"));
                        case["reference_png"] = serde_json::json!(reference);
                        match compare_images(&reference, &output) {
                            Ok(comparison) => {
                                case["pixel_exact"] = serde_json::json!(comparison.exact());
                                case["differing_pixels"] =
                                    serde_json::json!(comparison.differing_pixels);
                                case["max_channel_error"] =
                                    serde_json::json!(comparison.max_channel_error);
                                case["mean_absolute_error"] =
                                    serde_json::json!(comparison.mean_absolute_error);
                                if !comparison.exact() {
                                    case["passed"] = serde_json::json!(false);
                                    case["classification"] = serde_json::json!("pixel_difference");
                                    case["failure_stage"] = serde_json::json!("pixel_comparison");
                                }
                            }
                            Err(error) => {
                                case["passed"] = serde_json::json!(false);
                                case["pixel_exact"] = serde_json::Value::Null;
                                case["classification"] =
                                    serde_json::json!("reference_validation_error");
                                case["failure_stage"] = serde_json::json!("pixel_comparison");
                                case["error"] = serde_json::json!(error);
                            }
                        }
                    }
                    case
                }
                Err(error) => {
                    let message = error.to_string();
                    let diagnostics = failure_diagnostics(&message).unwrap_or_default();
                    serde_json::json!({
                        "render_path": path_name,
                        "pixel_format": format_name,
                        "render_completed": false,
                        "applicable": !matches!(
                            diagnostics.classification.as_str(),
                            "unsupported_pixel_depth" | "unsupported_render_path" |
                                "unsupported_media_type"
                        ),
                        "passed": false,
                        "classification": diagnostics.classification,
                        "failure_stage": diagnostics.failure_stage,
                        "selector_error": diagnostics.selector_error,
                        "error": matrix_error_summary(&message),
                    })
                }
            });
        }
    }
    let passed = cases.iter().filter(|case| case["passed"] == true).count();
    let unsupported = cases
        .iter()
        .filter(|case| case["applicable"] == false)
        .count();
    let applicable = cases.len() - unsupported;
    let failed = applicable - passed;
    serde_json::json!({
        "schema_version": 1,
        "stage": "effect_compatibility_matrix",
        "case_count": cases.len(),
        "applicable_count": applicable,
        "passed_count": passed,
        "failed_count": failed,
        "unsupported_count": unsupported,
        "reference_mode": reference_root.is_some(),
        "cases": cases,
    })
}

fn json_after_marker(text: &str, marker: &str) -> Option<serde_json::Value> {
    let tail = text.split_once(marker)?.1;
    serde_json::Deserializer::from_str(tail)
        .into_iter::<serde_json::Value>()
        .next()?
        .ok()
}

fn failure_diagnostics(message: &str) -> Option<FailureDiagnostics> {
    let diagnostics = json_after_marker(message, "diagnostics=")
        .or_else(|| json_after_marker(message, "worker report unavailable: "))?;
    let report = json_after_marker(message, "report=");
    let selector_error = report.as_ref().and_then(|value| {
        [
            "render_error",
            "smart_render_error",
            "pre_render_error",
            "gpu_device_setup_error",
            "gpu_device_setdown_error",
        ]
        .into_iter()
        .find_map(|field| value[field].as_i64().filter(|error| *error != 0))
    });
    let failure_stage = diagnostics["failure_stage"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            (report
                .as_ref()
                .and_then(|value| value["result_rects_valid"].as_bool())
                == Some(false))
            .then(|| "result_rect_validation".to_owned())
        });
    let unsupported_depth = report
        .as_ref()
        .and_then(|value| value["depth_supported"].as_bool())
        == Some(false);
    let unsupported_render_path = report
        .as_ref()
        .and_then(|value| value["smart_render_supported"].as_bool())
        == Some(false);
    let unsupported_media_type = report
        .as_ref()
        .and_then(|value| value["image_render_supported"].as_bool())
        == Some(false);
    let missing_suites = diagnostics["missing_suites"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|suite| {
            let name = suite["name"].as_str()?;
            let version = i32::try_from(suite["version"].as_i64()?).ok()?;
            valid_suite_name(name)
                .then(|| MissingSuite {
                    name: name.to_owned(),
                    version,
                })
                .filter(|suite| suite.version > 0)
        })
        .fold(Vec::new(), |mut suites, suite| {
            if suites.len() < 16 && !suites.contains(&suite) {
                suites.push(suite);
            }
            suites
        });
    let last_seh_selector = report
        .as_ref()
        .and_then(|value| value["last_seh_selector"].as_str())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 32
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
        })
        .map(str::to_owned);
    Some(FailureDiagnostics {
        classification: if unsupported_media_type {
            "unsupported_media_type".to_owned()
        } else if unsupported_render_path {
            "unsupported_render_path".to_owned()
        } else if unsupported_depth {
            "unsupported_pixel_depth".to_owned()
        } else {
            diagnostics["classification"]
                .as_str()
                .unwrap_or("worker_error")
                .to_owned()
        },
        failure_stage: if unsupported_media_type {
            Some("media_type_negotiation".to_owned())
        } else if unsupported_render_path {
            Some("render_path_negotiation".to_owned())
        } else if unsupported_depth {
            Some("pixel_depth_negotiation".to_owned())
        } else {
            failure_stage
        },
        exit_code: diagnostics["exit_code"].as_i64(),
        elapsed_ms: diagnostics["elapsed_ms"].as_u64(),
        selector_error: if unsupported_render_path || unsupported_depth {
            None
        } else {
            selector_error
        },
        missing_suites,
        last_seh_selector,
        last_seh_error: report
            .as_ref()
            .and_then(|value| value["last_seh_error"].as_i64())
            .filter(|value| i32::try_from(*value).is_ok()),
        last_seh_exception_code: report
            .as_ref()
            .and_then(|value| value["last_seh_exception_code"].as_u64())
            .filter(|value| u32::try_from(*value).is_ok()),
        stages: completed_stages(Some(&diagnostics)),
    })
}

fn valid_suite_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 96
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'.' | b'_' | b'-'))
}

fn matrix_error_summary(message: &str) -> String {
    if let Some(report) = json_after_marker(message, "report=") {
        if report["smart_render_supported"].as_bool() == Some(false) {
            return "AEX did not advertise SmartFX render support".into();
        }
        if report["depth_supported"].as_bool() == Some(false) {
            return "AEX did not advertise support for the requested pixel depth".into();
        }
        if report["result_rects_valid"].as_bool() == Some(false) {
            return "SmartFX did not return a valid result rectangle".into();
        }
    }
    message.chars().take(512).collect()
}

fn completed_stages(diagnostics: Option<&serde_json::Value>) -> Vec<String> {
    diagnostics
        .and_then(|value| value.get("stage_events"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|event| event.get("state").and_then(serde_json::Value::as_str) == Some("end"))
        .filter_map(|event| {
            let stage = event.get("stage")?.as_str()?;
            let errors = event.get("errors").cloned().unwrap_or_default();
            Some(
                if errors.as_object().is_some_and(|values| !values.is_empty()) {
                    format!("{stage} {errors}")
                } else {
                    stage.to_owned()
                },
            )
        })
        .collect()
}

fn render_diagnostics(report: &serde_json::Value) -> Option<RenderDiagnostics> {
    (report.get("stage")?.as_str()? == "interactive_image_render").then(|| {
        let gpu_attempt = report.get("gpu_attempt").filter(|value| !value.is_null());
        RenderDiagnostics {
            render_path: report["render_path"]
                .as_str()
                .unwrap_or("unknown")
                .to_owned(),
            pixel_format: report["pixel_format"]
                .as_str()
                .unwrap_or("unknown")
                .to_owned(),
            worker_classification: report["worker_classification"]
                .as_str()
                .unwrap_or("unknown")
                .to_owned(),
            gpu_fallback_used: report["gpu_fallback_used"].as_bool().unwrap_or(false),
            gpu_attempt_classification: gpu_attempt
                .and_then(|value| value["worker_classification"].as_str())
                .map(str::to_owned),
            gpu_failure_stage: gpu_attempt
                .and_then(|value| value["worker_diagnostics"]["failure_stage"].as_str())
                .map(str::to_owned),
            final_stages: completed_stages(report.get("worker_diagnostics")),
            gpu_stages: completed_stages(
                gpu_attempt.and_then(|value| value.get("worker_diagnostics")),
            ),
        }
    })
}

fn apply_dynamic_ui_report(
    parameters: &mut [aexcompat_broker::image_render::InteractiveParameter],
    report: &serde_json::Value,
) -> bool {
    if report.get("user_changed_param_requested") != Some(&serde_json::json!(true))
        || report.get("user_changed_param_error") != Some(&serde_json::json!(0))
    {
        return false;
    }
    let Some(rows) = report
        .get("parameters")
        .and_then(serde_json::Value::as_array)
    else {
        return false;
    };
    let updates = parameters
        .iter()
        .map(|parameter| {
            rows.iter()
                .find(|row| {
                    row.get("index").and_then(serde_json::Value::as_u64)
                        == Some(parameter.slot as u64)
                })
                .and_then(|row| row.get("ui_flags").and_then(serde_json::Value::as_u64))
                .map(|flags| (flags & (1 << 5) == 0, flags & (1 << 9) == 0))
        })
        .collect::<Option<Vec<_>>>();
    let Some(updates) = updates else {
        return false;
    };
    for (parameter, (enabled, visible)) in parameters.iter_mut().zip(updates) {
        parameter.enabled = enabled;
        parameter.visible = visible;
    }
    true
}

/// The AEX's own SUPPORTS_SMART_RENDER declaration from the parameter
/// inspection diagnostics; None until an inspection has completed (issue #105).
fn advertised_smart_render(report: &serde_json::Value) -> Option<bool> {
    report["worker_diagnostics"]["smart_render_advertised"].as_bool()
}

/// Per-frame watchdog deadline for resident-session renders, matching the
/// broker's interactive one-shot timeout.
const LIVE_RENDER_FRAME_DEADLINE_MS: u64 = 30_000;

/// Static configuration a resident render session was opened with (issue
/// #107). A key change means the running worker cannot carry the next render:
/// the session thread closes it and opens a fresh one (SEQUENCE_SETUP runs
/// again; the reopen is visible in the report's `resident_session` facts).
#[cfg(windows)]
#[derive(Clone, PartialEq)]
struct LiveSessionKey {
    plugin_sha256: String,
    /// Identity of every approved dependency: staged basename, size, and
    /// content hash. The sealed tree stages dependencies by basename, so a
    /// renamed DLL with identical bytes still needs a fresh session.
    dependency_identities: Vec<String>,
    /// Parameter structure only (slots, kinds, ranges, choices); values ride
    /// each frame's v:2 message and must not force a reopen.
    parameter_signature: String,
    width: u32,
    height: u32,
    pixel_format: aexcompat_broker::image_render::RenderPixelFormat,
    time_step: i32,
    total_time: i32,
    time_scale: u32,
}

// Platform-independent (and unit-tested) even though the resident session
// that consumes it is Windows-only.
#[cfg_attr(not(windows), allow(dead_code))]
fn parameter_structure_signature(
    parameters: &[aexcompat_broker::image_render::InteractiveParameter],
) -> String {
    serde_json::to_string(
        &parameters
            .iter()
            .map(|parameter| {
                serde_json::json!({
                    "slot": parameter.slot,
                    "kind": parameter.kind,
                    "name": parameter.name,
                    "minimum": parameter.minimum,
                    "maximum": parameter.maximum,
                    "choices": parameter.choices,
                    "component_count": parameter.component_count,
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_default()
}

#[cfg(windows)]
struct LiveRenderRequest {
    repository: PathBuf,
    plugin_path: PathBuf,
    plugin_sha256: String,
    dependencies: Vec<aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact>,
    parameters: Vec<aexcompat_broker::image_render::InteractiveParameter>,
    input_path: PathBuf,
    timing: aexcompat_broker::image_render::RenderTiming,
    pixel_format: aexcompat_broker::image_render::RenderPixelFormat,
    output: PathBuf,
    respond: mpsc::Sender<TaskResult>,
    identity: DispatchIdentity,
    diagnostic_eligible: bool,
}

#[cfg(windows)]
enum LiveCommand {
    Render(Box<LiveRenderRequest>),
    /// AEX change, approval invalidation, or shutdown: the resident worker
    /// must not outlive the selection it was opened for.
    Close,
}

#[cfg(windows)]
struct LiveSessionHandle {
    sender: mpsc::Sender<LiveCommand>,
}

#[cfg(windows)]
struct DecodedInput {
    path: PathBuf,
    content_sha256: [u8; 32],
    width: u32,
    height: u32,
    rgba: std::sync::Arc<Vec<u8>>,
}

/// State owned by the GUI's single background session thread (issue #107
/// design: one async session thread inside the GUI process drives the
/// out-of-process resident worker; the UI thread never blocks on it).
#[cfg(windows)]
struct LiveSessionState {
    decoded: Option<DecodedInput>,
    open: Option<(
        LiveSessionKey,
        aexcompat_broker::image_render::InteractiveRenderSession,
    )>,
    /// Counts session opens; each reopen means temporal state restarted.
    session_generation: u64,
    /// Unclean close summary of the previous session, surfaced in the next
    /// render report instead of being dropped silently.
    pending_close_summary: Option<serde_json::Value>,
}

#[cfg(windows)]
impl LiveSessionState {
    fn close_current(&mut self) {
        if let Some((_, session)) = self.open.take() {
            let summary = session.close();
            if summary.get("session_clean") != Some(&serde_json::json!(true)) {
                self.pending_close_summary = Some(summary);
            }
        }
    }

    fn decode_input(&mut self, path: &Path) -> Result<&DecodedInput, String> {
        // The cache key is the file's content hash, not its metadata: the
        // one-shot path re-decoded every render, so an overwrite that
        // preserves size and mtime (mtime-keeping tools, coarse filesystem
        // timestamps) must still invalidate here. Hashing the encoded bytes
        // per render is cheap next to a decode; only the decode is reused.
        // The read is bounded like the decoder's allocation limit so a
        // mispicked huge file cannot balloon the session thread.
        const MAX_ENCODED_INPUT_BYTES: u64 = 64 * 1024 * 1024;
        let encoded_size = fs::metadata(path).map_err(|error| error.to_string())?.len();
        if encoded_size > MAX_ENCODED_INPUT_BYTES {
            return Err("input image file exceeds the live-render size bound".into());
        }
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        let content_sha256: [u8; 32] = Sha256::digest(&bytes).into();
        let stale = !self.decoded.as_ref().is_some_and(|cached| {
            cached.path.as_path() == path && cached.content_sha256 == content_sha256
        });
        if stale {
            let decoded = aexcompat_broker::image_render::decode_bounded_image(path, "input")
                .map_err(|error| error.to_string())?;
            let (width, height) = (decoded.width(), decoded.height());
            self.decoded = Some(DecodedInput {
                path: path.to_path_buf(),
                content_sha256,
                width,
                height,
                rgba: std::sync::Arc::new(decoded.into_rgba8().into_raw()),
            });
        }
        Ok(self.decoded.as_ref().expect("just cached"))
    }
}

/// One live render on the session thread: decode (cached), reopen the session
/// when the static key changed, render through the resident worker, and fall
/// back to the one-shot transport when the session infrastructure cannot
/// carry the render (open failure or invalidation), mirroring the broker's
/// length-1 wrapper fallback policy.
#[cfg(windows)]
fn live_render(state: &mut LiveSessionState, request: &LiveRenderRequest) -> Result<(String, Option<PathBuf>), String> {
    use aexcompat_broker::image_render::{InteractiveRenderSession, InteractiveSessionOpen};

    let (width, height, rgba) = {
        let decoded = state.decode_input(&request.input_path)?;
        (decoded.width, decoded.height, decoded.rgba.clone())
    };
    let key = LiveSessionKey {
        plugin_sha256: request.plugin_sha256.clone(),
        dependency_identities: request
            .dependencies
            .iter()
            .map(|artifact| {
                let basename = artifact
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let hash: String = artifact
                    .expected_sha256
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                format!("{basename}:{}:{hash}", artifact.expected_size)
            })
            .collect(),
        parameter_signature: parameter_structure_signature(&request.parameters),
        width,
        height,
        pixel_format: request.pixel_format,
        time_step: request.timing.time_step,
        total_time: request.timing.total_time,
        time_scale: request.timing.time_scale,
    };
    if state.open.as_ref().is_some_and(|(open_key, _)| *open_key != key) {
        state.close_current();
    }
    let one_shot = |reason: String| -> Result<(String, Option<PathBuf>), String> {
        let mut report =
            aexcompat_broker::image_render::render_experimental_image_with_approved_dependencies(
                &request.repository,
                &request.plugin_path,
                &request.plugin_sha256,
                &request.input_path,
                &request.output,
                &request.parameters,
                request.timing,
                false,
                request.pixel_format,
                None,
                None,
                aexcompat_broker::image_render::RenderGpuBackend::Auto,
                request.dependencies.clone(),
            )
            .map_err(|error| format!("{error} (after resident session fallback: {reason})"))?;
        report["resident_session_fallback"] = serde_json::json!(reason);
        let body = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
        Ok((body, Some(request.output.clone())))
    };
    if state.open.is_none() {
        let opened = InteractiveRenderSession::open(InteractiveSessionOpen {
            repository: &request.repository,
            plugin_id: "experimental",
            plugin_path: &request.plugin_path,
            plugin_sha256: &request.plugin_sha256,
            parameters: (!request.parameters.is_empty()).then_some(request.parameters.as_slice()),
            dependencies: request.dependencies.clone(),
            width,
            height,
            pixel_format: request.pixel_format,
            time_step: request.timing.time_step,
            total_time: request.timing.total_time,
            time_scale: request.timing.time_scale,
            timeout_ms: LIVE_RENDER_FRAME_DEADLINE_MS,
        });
        match opened {
            Ok(session) => {
                state.session_generation += 1;
                state.open = Some((key.clone(), session));
            }
            Err(error) => return one_shot(format!("session open failed: {error}")),
        }
    }
    let (_, session) = state.open.as_mut().expect("session just ensured");
    let parameters = (!request.parameters.is_empty()).then_some(request.parameters.as_slice());
    match session.render(&rgba, request.timing.current_time, parameters, &request.output) {
        Ok(mut report) => {
            report["resident_session"]["session_generation"] =
                serde_json::json!(state.session_generation);
            if let Some(summary) = state.pending_close_summary.take() {
                report["previous_session_close"] = summary;
            }
            let passed = report.get("passed") == Some(&serde_json::json!(true));
            let body =
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
            if passed {
                Ok((body, Some(request.output.clone())))
            } else {
                // Frame-local compatibility error: the session stays open and
                // the failure reports like a one-shot render failure.
                Err(body)
            }
        }
        Err(error) => {
            let invalidated = state
                .open
                .as_ref()
                .is_some_and(|(_, session)| session.invalidated());
            if invalidated {
                state.close_current();
                one_shot(format!("session invalidated: {error}"))
            } else {
                // Rejected request (bad timing or parameter set); the session
                // itself is still usable for the next render.
                Err(format!("resident session rejected the render: {error}"))
            }
        }
    }
}

#[cfg(windows)]
fn spawn_live_session_thread(receiver: mpsc::Receiver<LiveCommand>) {
    thread::spawn(move || {
        let mut state = LiveSessionState {
            decoded: None,
            open: None,
            session_generation: 0,
            pending_close_summary: None,
        };
        loop {
            match receiver.recv() {
                Ok(LiveCommand::Render(request)) => {
                    let outcome = live_render(&mut state, &request);
                    let (success, body, output) = match outcome {
                        Ok((body, output)) => (true, body, output),
                        Err(body) => (false, body, None),
                    };
                    let _ = request.respond.send(TaskResult {
                        success,
                        body,
                        output,
                        identity: Some(request.identity.clone()),
                        operation: Some("render_image".into()),
                        diagnostic_eligible: request.diagnostic_eligible,
                    });
                }
                Ok(LiveCommand::Close) => state.close_current(),
                // The app dropped the handle: close the worker and exit.
                Err(_) => {
                    state.close_current();
                    return;
                }
            }
        }
    });
}

struct HarnessApp {
    repository: PathBuf,
    selection: Option<Selection>,
    session_approved: bool,
    dependencies: Vec<SessionDependency>,
    approved_dependencies: Vec<aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact>,
    approval_check: bool,
    trust_rebuilds: bool,
    selection_stale: bool,
    last_identity_check: Instant,
    input_image: Option<PathBuf>,
    audio_input: Option<PathBuf>,
    audio_effect_only: bool,
    input_preview: Option<egui::TextureHandle>,
    output_image: Option<PathBuf>,
    preview: Option<egui::TextureHandle>,
    reference_image: Option<PathBuf>,
    reference_preview: Option<egui::TextureHandle>,
    viewer_open: bool,
    viewer_mode: u8,
    viewer_zoom: f32,
    viewer_pan: egui::Vec2,
    pixel_comparison: Option<Result<PixelComparison, String>>,
    parameters: Vec<aexcompat_broker::image_render::InteractiveParameter>,
    parameter_defaults: Vec<aexcompat_broker::image_render::InteractiveParameter>,
    host_context: Option<aexcompat_broker::render_request::HostContext>,
    smart_render: bool,
    smart_render_advertised: Option<bool>,
    pixel_format: aexcompat_broker::image_render::RenderPixelFormat,
    gpu_backend: aexcompat_broker::image_render::RenderGpuBackend,
    frame: i32,
    frames_per_second: u32,
    frame_time_step: i32,
    duration_frames: i32,
    custom_ui_click_point: [u16; 2],
    custom_ui_click_color: [f32; 4],
    apply_custom_ui_click_to_render: bool,
    apply_custom_ui_draw_to_render: bool,
    custom_ui_drag_end: [u16; 2],
    custom_ui_drag_steps: u8,
    custom_ui_keycode: u32,
    custom_ui_key_modifiers: u16,
    busy: bool,
    rendering: bool,
    status: String,
    report: String,
    render_diagnostics: Option<RenderDiagnostics>,
    failure_diagnostics: Option<FailureDiagnostics>,
    matrix_results: Vec<MatrixCase>,
    receiver: Option<Receiver<TaskResult>>,
    task_kind: TaskKind,
    inspect_after_refresh: bool,
    diagnostic_history: DiagnosticHistory,
    missing_suite_aggregate: MissingSuiteAggregate,
    preflight_warnings: Vec<PreflightImportWarning>,
    diagnostic_warning: Option<String>,
    live_render: bool,
    pending_parameter_slot: Option<u32>,
    pending_live_render: bool,
    live_render_due: Option<Instant>,
    render_after_parameter_change: bool,
    /// Command channel into the background session thread (issue #107); the
    /// resident worker lives on the other side of it. Dropped with the app,
    /// which closes the session gracefully. The session transport is
    /// Windows-only; other targets keep the one-shot path.
    #[cfg(windows)]
    live_session: Option<LiveSessionHandle>,
}

impl HarnessApp {
    fn new(repository: PathBuf) -> Self {
        let missing_suite_aggregate = aggregate_missing_suites(&repository);
        Self {
            repository,
            selection: None,
            session_approved: false,
            dependencies: Vec::new(),
            approved_dependencies: Vec::new(),
            approval_check: false,
            trust_rebuilds: false,
            selection_stale: false,
            last_identity_check: Instant::now(),
            input_image: None,
            audio_input: None,
            audio_effect_only: false,
            input_preview: None,
            output_image: None,
            preview: None,
            reference_image: None,
            reference_preview: None,
            viewer_open: false,
            viewer_mode: 0,
            viewer_zoom: 1.0,
            viewer_pan: egui::Vec2::ZERO,
            pixel_comparison: None,
            parameters: Vec::new(),
            parameter_defaults: Vec::new(),
            host_context: None,
            smart_render: false,
            smart_render_advertised: None,
            pixel_format: aexcompat_broker::image_render::RenderPixelFormat::Argb8,
            gpu_backend: aexcompat_broker::image_render::RenderGpuBackend::Auto,
            frame: 0,
            frames_per_second: 30,
            frame_time_step: 1,
            duration_frames: 300,
            custom_ui_click_point: [20, 20],
            custom_ui_click_color: [0.125, 0.25, 0.75, 1.0],
            apply_custom_ui_click_to_render: false,
            apply_custom_ui_draw_to_render: false,
            custom_ui_drag_end: [20, 20],
            custom_ui_drag_steps: 4,
            custom_ui_keycode: 0x8000_0041,
            custom_ui_key_modifiers: 0,
            busy: false,
            rendering: false,
            status: "Select an AEX file. Selection does not execute native code.".into(),
            report: String::new(),
            render_diagnostics: None,
            failure_diagnostics: None,
            matrix_results: Vec::new(),
            receiver: None,
            task_kind: TaskKind::Generic,
            inspect_after_refresh: false,
            diagnostic_history: DiagnosticHistory::default(),
            missing_suite_aggregate,
            preflight_warnings: Vec::new(),
            diagnostic_warning: None,
            live_render: true,
            pending_parameter_slot: None,
            pending_live_render: false,
            live_render_due: None,
            render_after_parameter_change: false,
            #[cfg(windows)]
            live_session: None,
        }
    }

    /// Tells the session thread to close the resident worker. Required
    /// whenever the AEX selection or its approval changes: the worker must
    /// not outlive the selection it was opened for.
    fn close_live_session(&mut self) {
        #[cfg(windows)]
        if let Some(handle) = &self.live_session {
            if handle.sender.send(LiveCommand::Close).is_err() {
                self.live_session = None;
            }
        }
    }

    fn accept_adjacent_discovery(&mut self, discovery: AdjacentImportDiscovery) {
        self.dependencies = discovery.dependencies;
        self.preflight_warnings = discovery.warnings;
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let identity = DispatchIdentity {
            sha256: selection.sha256.clone(),
            size: selection.size,
        };
        if selection.profile.is_none() {
            if let Err(error) =
                persist_preflight_warnings(&self.repository, &identity, &self.preflight_warnings)
            {
                self.diagnostic_warning = Some(format!("Diagnostic save warning: {error}"));
            }
        }
        self.diagnostic_history = load_diagnostic_history(&self.repository, &identity.sha256);
        self.missing_suite_aggregate = aggregate_missing_suites(&self.repository);
    }

    fn show_image_viewer(&mut self, ctx: &egui::Context) {
        if !self.viewer_open {
            return;
        }
        let mut open = self.viewer_open;
        egui::Window::new("FHD Image Viewer")
            .open(&mut open)
            .default_size(egui::vec2(1600.0, 900.0))
            .min_size(egui::vec2(640.0, 360.0))
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.viewer_mode, 0, "Input");
                    ui.selectable_value(&mut self.viewer_mode, 1, "AEX output");
                    ui.selectable_value(&mut self.viewer_mode, 2, "Compare");
                    ui.separator();
                    ui.label("FHD canvas / aspect-fit");
                    ui.separator();
                    ui.monospace(format!("{:.0}%", self.viewer_zoom * 100.0));
                    if ui.small_button("Fit").clicked() {
                        self.viewer_zoom = 1.0;
                        self.viewer_pan = egui::Vec2::ZERO;
                    }
                });
                ui.separator();
                match self.viewer_mode {
                    0 => show_viewer_texture(
                        ui,
                        "Input",
                        self.input_preview.as_ref(),
                        &mut self.viewer_zoom,
                        &mut self.viewer_pan,
                    ),
                    1 => show_viewer_texture(
                        ui,
                        "AEX output",
                        self.preview.as_ref(),
                        &mut self.viewer_zoom,
                        &mut self.viewer_pan,
                    ),
                    _ => ui.columns(2, |columns| {
                        show_viewer_texture(
                            &mut columns[0],
                            "Input",
                            self.input_preview.as_ref(),
                            &mut self.viewer_zoom,
                            &mut self.viewer_pan,
                        );
                        show_viewer_texture(
                            &mut columns[1],
                            "AEX output",
                            self.preview.as_ref(),
                            &mut self.viewer_zoom,
                            &mut self.viewer_pan,
                        );
                    }),
                }
            });
        self.viewer_open = open;
    }

    fn reset_and_choose_aex(&mut self) {
        // The selection is gone the moment the reset starts; the resident
        // worker for it must not outlive a cancelled or failed re-pick.
        self.close_live_session();
        self.selection = None;
        self.session_approved = false;
        self.approved_dependencies.clear();
        self.trust_rebuilds = false;
        self.selection_stale = false;
        self.diagnostic_history = DiagnosticHistory::default();
        self.diagnostic_warning = None;
        self.preflight_warnings.clear();
        self.parameters.clear();
        self.parameter_defaults.clear();
        self.audio_input = None;
        self.audio_effect_only = false;
        self.smart_render = false;
        self.smart_render_advertised = None;
        self.host_context = None;
        self.choose_aex();
    }

    fn show_workspace_viewer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.viewer_mode, 0, "INPUT");
            ui.selectable_value(&mut self.viewer_mode, 1, "AEX OUTPUT");
            ui.selectable_value(&mut self.viewer_mode, 2, "COMPARE");
            ui.separator();
            let label = match self.viewer_mode {
                0 => self.input_preview.as_ref().map(|texture| texture.size()),
                1 => self.preview.as_ref().map(|texture| texture.size()),
                _ => None,
            };
            if let Some([width, height]) = label {
                ui.monospace(format!("{width} x {height}"));
            } else {
                ui.weak("FHD workspace / aspect fit");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Pop out").clicked() {
                    self.viewer_open = true;
                }
                if ui.small_button("Fit").clicked() {
                    self.viewer_zoom = 1.0;
                    self.viewer_pan = egui::Vec2::ZERO;
                }
                ui.monospace(format!("{:.0}%", self.viewer_zoom * 100.0));
                if self.rendering {
                    ui.weak("Rendering...");
                    ui.spinner();
                }
            });
        });
        ui.separator();

        let viewer_height = (ui.available_height() * 0.62).clamp(280.0, 860.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), viewer_height),
            egui::Layout::top_down(egui::Align::Center),
            |ui| match self.viewer_mode {
                0 => show_viewer_texture(
                    ui,
                    "Input",
                    self.input_preview.as_ref(),
                    &mut self.viewer_zoom,
                    &mut self.viewer_pan,
                ),
                1 if self.preview.is_some() => show_viewer_texture(
                    ui,
                    "AEX output",
                    self.preview.as_ref(),
                    &mut self.viewer_zoom,
                    &mut self.viewer_pan,
                ),
                1 => {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            if self.busy {
                                ui.spinner();
                                ui.label("Rendering AEX output...");
                            } else {
                                ui.colored_label(
                                    Color32::from_rgb(225, 155, 65),
                                    RichText::new("AEX output is not available").strong(),
                                );
                                ui.label(&self.status);
                                if let Some(first_line) = self.report.lines().next() {
                                    ui.monospace(first_line);
                                }
                            }
                        });
                    });
                }
                _ => ui.columns(2, |columns| {
                    show_viewer_texture(
                        &mut columns[0],
                        "Input",
                        self.input_preview.as_ref(),
                        &mut self.viewer_zoom,
                        &mut self.viewer_pan,
                    );
                    show_viewer_texture(
                        &mut columns[1],
                        "AEX output",
                        self.preview.as_ref(),
                        &mut self.viewer_zoom,
                        &mut self.viewer_pan,
                    );
                }),
            },
        );
    }

    fn spawn<F>(&mut self, work: F)
    where
        F: FnOnce() -> Result<(String, Option<PathBuf>), String> + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = work();
            let _ = sender.send(match result {
                Ok((body, output)) => TaskResult {
                    success: true,
                    body,
                    output,
                    identity: None,
                    operation: None,
                    diagnostic_eligible: false,
                },
                Err(body) => TaskResult {
                    success: false,
                    body,
                    output: None,
                    identity: None,
                    operation: None,
                    diagnostic_eligible: false,
                },
            });
        });
        self.receiver = Some(receiver);
        self.busy = true;
        self.task_kind = TaskKind::Generic;
    }

    fn spawn_native<F>(&mut self, operation: &'static str, work: F)
    where
        F: FnOnce() -> Result<(String, Option<PathBuf>), String> + Send + 'static,
    {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let identity = DispatchIdentity {
            sha256: selection.sha256.clone(),
            size: selection.size,
        };
        let diagnostic_eligible = selection.profile.is_none();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = work();
            let (success, body, output) = match result {
                Ok((body, output)) => (true, body, output),
                Err(body) => (false, body, None),
            };
            let _ = sender.send(TaskResult {
                success,
                body,
                output,
                identity: Some(identity),
                operation: Some(operation.into()),
                diagnostic_eligible,
            });
        });
        self.receiver = Some(receiver);
        self.busy = true;
        self.task_kind = TaskKind::Generic;
    }

    fn choose_aex(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("After Effects plug-in", &["aex"])
            .pick_file()
        else {
            return;
        };
        self.status = "Computing AEX identity...".into();
        self.spawn(move || {
            let bytes = read_bounded_pe(&path)?;
            let hash = format!("{:X}", Sha256::digest(&bytes));
            Ok((
                format!("{}\n{}\n{}", path.display(), bytes.len(), hash),
                None,
            ))
        });
        self.task_kind = TaskKind::IdentifyAex;
    }

    fn add_dependency(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Windows dependency", &["dll"])
            .pick_file()
        else {
            return;
        };
        match read_bounded_pe(&path) {
            Ok(bytes) => {
                let key = path.to_string_lossy().to_lowercase();
                if self
                    .dependencies
                    .iter()
                    .any(|item| item.path.to_string_lossy().to_lowercase() == key)
                {
                    self.status = "That dependency DLL is already listed.".into();
                    return;
                }
                self.dependencies.push(SessionDependency {
                    path,
                    size: bytes.len() as u64,
                    sha256: format!("{:X}", Sha256::digest(&bytes)),
                });
                match self.approve_session() {
                    Ok(()) => self.status = "Dependency manifest refreshed.".into(),
                    Err(error) => {
                        self.invalidate_session_approval("Dependency manifest validation failed.");
                        self.report = error;
                    }
                }
            }
            Err(error) => {
                self.status = "Dependency DLL could not be read.".into();
                self.report = error.to_string();
            }
        }
    }

    fn invalidate_session_approval(&mut self, status: &str) {
        self.session_approved = false;
        self.approval_check = false;
        self.approved_dependencies.clear();
        self.status = status.into();
        self.close_live_session();
    }

    fn approve_session(&mut self) -> Result<(), String> {
        // Every dependency-set change funnels through re-approval; the
        // resident worker still holds the previous approved set and must not
        // idle past it, so close eagerly rather than lazily at the next
        // render.
        self.close_live_session();
        let selection = self.selection.as_ref().ok_or("No AEX is selected")?;
        let main = aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact {
            path: selection.path.clone(),
            expected_sha256: decode_sha256(&selection.sha256)?,
            expected_size: selection.size,
        };
        let dependencies = self
            .dependencies
            .iter()
            .map(|item| {
                serde_json::json!({
                    "path": item.path,
                    "basename": item.path.file_name().and_then(|name| name.to_str()).unwrap_or(""),
                    "sha256": item.sha256,
                    "size": item.size,
                })
            })
            .collect::<Vec<_>>();
        let json = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "dependencies": dependencies,
        }))
        .map_err(|error| error.to_string())?;
        let manifest =
            aexcompat_broker::session_dependency_manifest::parse_and_validate(&json, &main)
                .map_err(|error| error.to_string())?;
        self.approved_dependencies = manifest.into_approved_image_artifacts();
        self.session_approved = true;
        Ok(())
    }

    fn choose_input(&mut self, ctx: &egui::Context) {
        let selected = rfd::FileDialog::new()
            .add_filter(
                "Image",
                &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"],
            )
            .pick_file();
        let Some(path) = selected else {
            return;
        };
        match load_preview(ctx, "input", &path) {
            Ok(preview) => {
                self.input_image = Some(path);
                self.input_preview = Some(preview);
                self.output_image = None;
                self.preview = None;
                self.pixel_comparison = None;
                self.status = "Input image loaded. Ready to render.".into();
                self.start_live_render_if_ready();
            }
            Err(error) => {
                self.status = "Input image could not be decoded.".into();
                self.report = error;
            }
        }
    }

    fn start_live_render_if_ready(&mut self) {
        if self.live_render
            && !self.busy
            && !self.audio_effect_only
            && self.selection.is_some()
            && self.input_image.is_some()
        {
            self.quick_render();
            self.viewer_mode = 1;
        }
    }

    fn choose_audio_input(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Mono float32 LE audio", &["f32"])
            .pick_file()
        else {
            return;
        };
        self.audio_input = Some(path);
        self.status = "Audio input selected. Ready to render.".into();
    }

    fn choose_reference(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "AE reference image",
                &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"],
            )
            .pick_file()
        else {
            return;
        };
        match load_preview(ctx, "reference", &path) {
            Ok(preview) => {
                self.reference_image = Some(path);
                self.reference_preview = Some(preview);
                self.refresh_pixel_comparison();
                self.status = "AE reference image loaded.".into();
            }
            Err(error) => {
                self.status = "AE reference image could not be decoded.".into();
                self.report = error;
            }
        }
    }

    fn load_debug_request(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AEXCompat debug request", &["json"])
            .pick_file()
        else {
            return;
        };
        let result = (|| {
            let bytes = fs::read(&path).map_err(|error| error.to_string())?;
            if bytes.len() > 64 * 1024 {
                return Err("assignment document exceeds 64 KiB".to_owned());
            }
            let document: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            let timing = typed_request_timing(&document)?;
            let mut parameters = self.parameters.clone();
            apply_typed_assignments(&mut parameters, &document)?;
            let host_context = typed_request_host_context(&document)?;
            Ok((parameters, timing, host_context))
        })();
        match result {
            Ok((parameters, timing, host_context)) => {
                self.parameters = parameters;
                self.host_context = host_context;
                self.frame = timing.current_time / timing.time_step;
                self.frames_per_second = timing.time_scale;
                self.frame_time_step = timing.time_step;
                self.duration_frames = timing.total_time / timing.time_step;
                self.status = format!("Loaded debug request: {}", path.display());
            }
            Err(error) => {
                self.status = "Debug request was rejected without changing controls.".into();
                self.report = error;
            }
        }
    }

    fn save_debug_request(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AEXCompat debug request", &["json"])
            .set_file_name("aex-debug-request.json")
            .save_file()
        else {
            return;
        };
        let document = typed_request_document(
            &self.parameters,
            self.frame,
            self.frames_per_second,
            self.frame_time_step,
            self.duration_frames,
            self.host_context.as_ref(),
        );
        match serde_json::to_vec_pretty(&document)
            .map_err(|error| error.to_string())
            .and_then(|bytes| fs::write(&path, bytes).map_err(|error| error.to_string()))
        {
            Ok(()) => self.status = format!("Saved debug request: {}", path.display()),
            Err(error) => {
                self.status = "Debug request could not be saved.".into();
                self.report = error;
            }
        }
    }

    fn refresh_pixel_comparison(&mut self) {
        self.pixel_comparison = match (&self.reference_image, &self.output_image) {
            (Some(reference), Some(output)) => Some(compare_images(reference, output)),
            _ => None,
        };
    }

    fn refresh_aex(&mut self) {
        let Some(selected) = &self.selection else {
            return;
        };
        let path = selected.path.clone();
        let previous_hash = selected.sha256.clone();
        match read_bounded_pe(&path) {
            Ok(bytes) => {
                let hash = format!("{:X}", Sha256::digest(&bytes));
                let metadata = fs::metadata(&path).ok();
                let profile = profile_for_hash(&hash);
                let identity_changed = hash != previous_hash;
                if identity_changed {
                    // Do not keep a worker holding the previous build alive.
                    self.close_live_session();
                }
                self.selection = Some(Selection {
                    path: path.clone(),
                    size: bytes.len() as u64,
                    sha256: hash,
                    profile,
                    modified: metadata.and_then(|value| value.modified().ok()),
                });
                self.selection_stale = false;
                self.parameters.clear();
                self.audio_input = None;
                self.audio_effect_only = false;
                self.smart_render_advertised = None;
                self.host_context = None;
                self.output_image = None;
                self.preview = None;
                if identity_changed {
                    self.approved_dependencies.clear();
                    match discover_adjacent_imports(&path) {
                        Ok(discovery) => {
                            self.accept_adjacent_discovery(discovery);
                            match self.approve_session() {
                                Ok(()) => {
                                    self.inspect_after_refresh = true;
                                    self.status =
                                        "Rebuilt AEX identity and dependencies refreshed.".into();
                                }
                                Err(error) => {
                                    self.invalidate_session_approval(
                                        "Rebuilt dependency manifest validation failed.",
                                    );
                                    self.report = error;
                                }
                            }
                        }
                        Err(error) => {
                            self.invalidate_session_approval(
                                "Rebuilt dependency discovery failed safely.",
                            );
                            self.report = error;
                        }
                    }
                } else {
                    self.status = "AEX identity is unchanged.".into();
                }
            }
            Err(error) => {
                self.status = "Could not reload the selected AEX.".into();
                self.report = error.to_string();
            }
        }
    }

    fn check_selected_identity(&mut self) {
        if self.last_identity_check.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.last_identity_check = Instant::now();
        let Some(selected) = &self.selection else {
            return;
        };
        let Ok(metadata) = fs::metadata(&selected.path) else {
            self.selection_stale = true;
            self.status = "Selected AEX is unavailable. Reload after the build finishes.".into();
            self.close_live_session();
            return;
        };
        let modified = metadata.modified().ok();
        let hash_changed = read_bounded_pe(&selected.path).map_or(true, |bytes| {
            !format!("{:X}", Sha256::digest(bytes)).eq_ignore_ascii_case(&selected.sha256)
        });
        if metadata.len() != selected.size || modified != selected.modified || hash_changed {
            if !self.selection_stale {
                self.status =
                    "AEX build changed. Reload its identity before native execution.".into();
            }
            self.selection_stale = true;
            self.session_approved = false;
            self.approved_dependencies.clear();
            // The resident worker still holds the previous build; a rebuilt
            // AEX must go through a fresh session open.
            self.close_live_session();
        }
        if self.dependencies.iter().any(|dependency| {
            read_bounded_pe(&dependency.path).map_or(true, |bytes| {
                bytes.len() as u64 != dependency.size
                    || !format!("{:X}", Sha256::digest(&bytes))
                        .eq_ignore_ascii_case(&dependency.sha256)
            })
        }) {
            self.invalidate_session_approval(
                "A dependency DLL changed. Re-add it and approve the session again.",
            );
        }
    }

    fn inspect_parameters_async(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Loading Effect Controls...".into();
        self.spawn_native("inspect_parameters", move || {
            let (parameters, diagnostics) =
                aexcompat_broker::image_render::inspect_experimental_with_diagnostics(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            let report = serde_json::json!({
                "stage": "parameter_inspection",
                "parameters": parameters,
                "worker_diagnostics": diagnostics,
            });
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
        self.task_kind = TaskKind::InspectParameters;
    }

    fn inspect_external_dependencies(&mut self, missing_only: bool) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = if missing_only {
            "Inspecting missing external dependencies..."
        } else {
            "Inspecting all external dependencies..."
        }
        .into();
        self.spawn_native(
            if missing_only {
                "inspect_missing_dependencies"
            } else {
                "inspect_dependencies"
            },
            move || {
                let report =
                    aexcompat_broker::image_render::inspect_experimental_external_dependencies(
                        &repository,
                        &plugin_path,
                        &hash,
                        missing_only,
                    )
                    .map_err(|error| error.to_string())?;
                Ok((
                    serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                    None,
                ))
            },
        );
    }

    fn probe_options_dialog(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing the advertised options dialog...".into();
        self.spawn_native("probe_options_dialog", move || {
            let report = aexcompat_broker::image_render::probe_experimental_options_dialog(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_automatic_options_dialog(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing the sequence-requested options dialog...".into();
        self.spawn_native("probe_automatic_options_dialog", move || {
            let report =
                aexcompat_broker::image_render::probe_experimental_automatic_options_dialog(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_nop_render(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing NOP_RENDER source passthrough...".into();
        self.spawn_native("probe_nop_render", move || {
            let report = aexcompat_broker::image_render::probe_experimental_nop_render(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_smart_nop_render(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing SmartFX NOP_RENDER source passthrough...".into();
        self.spawn_native("probe_smart_nop_render", move || {
            let report = aexcompat_broker::image_render::probe_experimental_smart_nop_render(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_input_buffer_write(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing advertised input-buffer write access...".into();
        self.spawn_native("probe_input_buffer_write", move || {
            let report = aexcompat_broker::image_render::probe_experimental_input_buffer_write(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_smart_input_buffer_write(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing SmartFX input-buffer write access...".into();
        self.spawn_native("probe_smart_input_buffer_write", move || {
            let report =
                aexcompat_broker::image_render::probe_experimental_smart_input_buffer_write(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_frame_resize(&mut self, expand: bool) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = if expand {
            "Probing advertised FRAME_SETUP expansion...".into()
        } else {
            "Probing advertised FRAME_SETUP shrink...".into()
        };
        self.spawn_native(
            if expand {
                "probe_frame_expansion"
            } else {
                "probe_frame_shrink"
            },
            move || {
                let report = if expand {
                    aexcompat_broker::image_render::probe_experimental_expand_buffer(
                        &repository,
                        &plugin_path,
                        &hash,
                    )
                } else {
                    aexcompat_broker::image_render::probe_experimental_shrink_buffer(
                        &repository,
                        &plugin_path,
                        &hash,
                    )
                }
                .map_err(|error| error.to_string())?;
                Ok((
                    serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                    None,
                ))
            },
        );
    }

    fn probe_persistent_sequence(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing two frames in one isolated sequence...".into();
        self.spawn_native("probe_persistent_sequence", move || {
            let report = aexcompat_broker::image_render::probe_experimental_persistent_sequence(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_flattened_sequence(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing sequence save/reload ownership...".into();
        self.spawn_native("probe_flattened_sequence", move || {
            let report = aexcompat_broker::image_render::probe_experimental_flattened_sequence(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_copied_flattened_sequence(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing non-destructive sequence save...".into();
        self.spawn_native("probe_copied_flattened_sequence", move || {
            let report =
                aexcompat_broker::image_render::probe_experimental_copied_flattened_sequence(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_custom_ui_cursor(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching an isolated custom UI cursor event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_cursor(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI requested the eyedropper cursor.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI cursor event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_draw(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Recording an isolated custom UI draw event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_draw(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI draw commands were recorded safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
                self.apply_custom_ui_draw_to_render = true;
                self.apply_custom_ui_click_to_render = false;
            }
            Err(error) => {
                self.status = "Custom UI draw event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_lifecycle(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching an isolated custom UI lifecycle...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_lifecycle(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI lifecycle completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI lifecycle failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_idle(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching one custom UI idle event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_idle(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI idle lifecycle completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI idle event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_keydown(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching one custom UI key event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_keydown(
            &self.repository,
            &selection.path,
            &selection.sha256,
            self.custom_ui_click_point,
            self.custom_ui_keycode,
            self.custom_ui_key_modifiers,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI key lifecycle completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI key event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_mouse_exited(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching a Layer/Comp custom UI mouse-exited event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_mouse_exited(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI mouse-exited lifecycle completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI mouse-exited event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_click(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching an isolated custom UI click event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_click(
            &self.repository,
            &selection.path,
            &selection.sha256,
            self.custom_ui_click_point,
            self.custom_ui_click_color,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI click changed the effect value safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
                self.apply_custom_ui_click_to_render = true;
                self.apply_custom_ui_draw_to_render = false;
            }
            Err(error) => {
                self.status = "Custom UI click failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_drag(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching a bounded custom UI drag sequence...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_drag(
            &self.repository,
            &selection.path,
            &selection.sha256,
            self.custom_ui_click_point,
            self.custom_ui_drag_end,
            self.custom_ui_drag_steps,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI drag sequence completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI drag failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn render_to(&mut self, output: PathBuf) {
        let Some(selection) = &self.selection else {
            return;
        };
        let Some(input) = self.input_image.clone() else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        let registered = selection.profile == Some("scattermap");
        let parameters = self.parameters.clone();
        let host_context = self.host_context.clone();
        let smart = self.smart_render;
        let pixel_format = self.pixel_format;
        let gpu_backend = self.gpu_backend;
        let audio_sidecar = self.audio_input.clone();
        let dependencies = self.approved_dependencies.clone();
        let custom_ui_action = if self.apply_custom_ui_click_to_render {
            Some(aexcompat_broker::image_render::RenderUiAction::Click {
                point: self.custom_ui_click_point,
                color: self.custom_ui_click_color,
            })
        } else if self.apply_custom_ui_draw_to_render {
            Some(aexcompat_broker::image_render::RenderUiAction::Draw)
        } else {
            None
        };
        let timing = match render_timing(
            self.frame,
            self.duration_frames,
            self.frames_per_second,
            self.frame_time_step,
        ) {
            Ok(timing) => timing,
            Err(error) => {
                self.status = "Render timing is invalid.".into();
                self.report = error;
                return;
            }
        };
        if audio_sidecar.is_some()
            && (smart
                || pixel_format != aexcompat_broker::image_render::RenderPixelFormat::Argb8
                || host_context.is_some()
                || custom_ui_action.is_some())
        {
            self.status = "Audio sidecar requires plain classic ARGB8 rendering.".into();
            self.report = "Disable SmartFX, deep color, mask/spatial/render context, and custom UI actions before rendering with audio.".into();
            return;
        }
        if audio_sidecar.is_some() && !dependencies.is_empty() {
            self.status =
                "Dependency DLLs are not supported by the audio-sidecar render path.".into();
            self.report =
                "Remove dependencies or disable the audio sidecar before rendering.".into();
            return;
        }
        let use_registered_default = registered
            && parameters.is_empty()
            && self.frame == 0
            && custom_ui_action.is_none()
            && pixel_format == aexcompat_broker::image_render::RenderPixelFormat::Argb8
            && audio_sidecar.is_none();
        // Resident-session eligibility mirrors the broker's length-1 wrapper:
        // plain classic CPU renders only. Anything else keeps the one-shot
        // transport below (issue #107). The session transport is
        // Windows-only; other targets always render one-shot.
        #[cfg(windows)]
        {
        let live_eligible = !smart
            && host_context.is_none()
            && custom_ui_action.is_none()
            && audio_sidecar.is_none()
            && gpu_backend == aexcompat_broker::image_render::RenderGpuBackend::Auto
            && !use_registered_default
            && !parameters.iter().any(|parameter| parameter.kind == "layer");
        if live_eligible {
            let identity = DispatchIdentity {
                sha256: hash.clone(),
                size: self.selection.as_ref().map(|item| item.size).unwrap_or_default(),
            };
            let diagnostic_eligible = self
                .selection
                .as_ref()
                .is_some_and(|item| item.profile.is_none());
            let (respond, receiver) = mpsc::channel();
            let request = LiveRenderRequest {
                repository,
                plugin_path,
                plugin_sha256: hash,
                dependencies,
                parameters,
                input_path: input,
                timing,
                pixel_format,
                output,
                respond,
                identity,
                diagnostic_eligible,
            };
            if self.live_session.is_none() {
                let (sender, commands) = mpsc::channel();
                spawn_live_session_thread(commands);
                self.live_session = Some(LiveSessionHandle { sender });
            }
            let sent = self
                .live_session
                .as_ref()
                .expect("session handle just ensured")
                .sender
                .send(LiveCommand::Render(Box::new(request)));
            if sent.is_ok() {
                self.status = "Rendering through the resident session...".into();
                self.rendering = true;
                self.receiver = Some(receiver);
                self.busy = true;
                self.task_kind = TaskKind::Generic;
                return;
            }
            // The session thread is gone; drop the handle so the next render
            // starts a fresh one. Nothing rendered on this attempt.
            self.live_session = None;
            self.status = "The session thread had exited; press Render to retry.".into();
            return;
        }
        }
        self.status = "Rendering in an isolated worker...".into();
        self.rendering = true;
        self.spawn_native("render_image", move || {
            let report = if let Some(audio) = audio_sidecar {
                aexcompat_broker::image_render::render_experimental_image_with_audio_sidecar(
                    &repository,
                    &plugin_path,
                    &hash,
                    &input,
                    &audio,
                    &output,
                    &parameters,
                    timing,
                )
            } else if !smart && use_registered_default && dependencies.is_empty() {
                aexcompat_broker::image_render::render_image(
                    &repository,
                    "scattermap",
                    &input,
                    &output,
                )
            } else {
                aexcompat_broker::image_render::render_experimental_image_with_approved_dependencies(
                    &repository,
                    &plugin_path,
                    &hash,
                    &input,
                    &output,
                    &parameters,
                    timing,
                    smart,
                    pixel_format,
                    host_context.as_ref(),
                    custom_ui_action,
                    gpu_backend,
                    dependencies,
                )
            }
            .map_err(|error| error.to_string())?;
            let body = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
            Ok((body, Some(output)))
        });
    }

    fn render_and_save(&mut self) {
        let Some(output) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .set_file_name("aex-output.png")
            .save_file()
        else {
            return;
        };
        self.render_to(output);
    }

    fn render_audio_and_save(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let Some(input) = self.audio_input.clone() else {
            return;
        };
        let Some(output) = rfd::FileDialog::new()
            .add_filter("Mono float32 LE audio", &["f32"])
            .set_file_name("aex-output.f32")
            .save_file()
        else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        let parameters = self.parameters.clone();
        self.status = "Rendering audio in an isolated worker...".into();
        self.spawn_native("render_audio", move || {
            let report = aexcompat_broker::image_render::render_experimental_audio(
                &repository,
                &plugin_path,
                &hash,
                &input,
                &output,
                &parameters,
            )
            .map_err(|error| error.to_string())?;
            let mut report = report;
            report["published_output"] = serde_json::json!(output);
            let body = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
            Ok((body, None))
        });
    }

    fn quick_render(&mut self) {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let output = self
            .repository
            .join("target/harness-output")
            .join(format!("render-{nonce}.png"));
        self.render_to(output);
    }

    fn run_compatibility_matrix(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let Some(input) = self.input_image.clone() else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        let parameters = self.parameters.clone();
        let host_context = self.host_context.clone();
        let timing = match render_timing(
            self.frame,
            self.duration_frames,
            self.frames_per_second,
            self.frame_time_step,
        ) {
            Ok(timing) => timing,
            Err(error) => {
                self.status = "Matrix timing is invalid.".into();
                self.report = error;
                return;
            }
        };
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let output_root = repository
            .join("target/harness-matrix")
            .join(nonce.to_string());
        self.status = "Running six isolated Effect compatibility cases...".into();
        self.matrix_results.clear();
        self.spawn_native("compatibility_matrix", move || {
            let report = run_effect_matrix(
                &repository,
                &plugin_path,
                &hash,
                &input,
                &output_root,
                &parameters,
                timing,
                None,
                host_context.as_ref(),
            );
            let body = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
            Ok((body, None))
        });
    }

    fn trigger_button(&mut self, slot: u32) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        let parameters = self.parameters.clone();
        self.status = format!("Dispatching PF_Cmd_USER_CHANGED_PARAM for slot {slot}...");
        self.spawn_native("user_changed_parameter", move || {
            let report = aexcompat_broker::image_render::trigger_experimental_button(
                &repository,
                &plugin_path,
                &hash,
                slot,
                &parameters,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn initialize_aegp(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Initializing AEGP in an isolated worker...".into();
        self.spawn_native("initialize_aegp", move || {
            let report = aexcompat_broker::image_render::initialize_experimental_aegp(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn update_aegp_menu(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Dispatching an isolated AEGP update-menu event...".into();
        self.spawn_native("update_aegp_menu", move || {
            let report = aexcompat_broker::image_render::dispatch_experimental_aegp_update_menu(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn dispatch_aegp_idle(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Dispatching one isolated AEGP idle event...".into();
        self.spawn_native("dispatch_aegp_idle", move || {
            let report = aexcompat_broker::image_render::dispatch_experimental_aegp_idle(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn dispatch_aegp_command_roundtrip(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Dispatching an isolated AEGP command ON/OFF roundtrip...".into();
        self.spawn_native("dispatch_aegp_command_roundtrip", move || {
            let report =
                aexcompat_broker::image_render::dispatch_experimental_aegp_command_roundtrip(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn dispatch_aegp_active_idle_roundtrip(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Running AEGP ON / active idle / OFF in isolation...".into();
        self.spawn_native("dispatch_aegp_active_idle_roundtrip", move || {
            let report =
                aexcompat_broker::image_render::dispatch_experimental_aegp_active_idle_roundtrip(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn dispatch_aegp_comp_idle_roundtrip(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Running AEGP ON / comp idle / OFF in isolation...".into();
        self.spawn_native("dispatch_aegp_comp_idle_roundtrip", move || {
            let report =
                aexcompat_broker::image_render::dispatch_experimental_aegp_comp_idle_roundtrip(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn show_effect_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(RichText::new("Effect Controls").size(20.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !self.busy && !self.parameter_defaults.is_empty(),
                        egui::Button::new("Reset All"),
                    )
                    .clicked()
                {
                    self.parameters = self.parameter_defaults.clone();
                    self.pending_parameter_slot = None;
                    self.pending_live_render = self.live_render;
                    self.live_render_due = self
                        .live_render
                        .then(|| Instant::now() + std::time::Duration::from_millis(500));
                }
            });
        });
        if let Some(selection) = &self.selection {
            ui.label(
                selection
                    .path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Selected AEX"),
            );
        } else {
            ui.label("Select an AEX to load its parameters.");
        }
        ui.separator();
        if self.busy && self.task_kind == TaskKind::InspectParameters {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading parameters...");
            });
        } else if self.selection.is_some() && self.parameters.is_empty() {
            ui.label("This effect exposed no editable parameters.");
            if ui.small_button("Reload controls").clicked() {
                self.inspect_parameters_async();
            }
        }

        let mut clicked_button = None;
        let parameter_defaults = self.parameter_defaults.clone();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for parameter in &mut self.parameters {
                    if !parameter.visible {
                        continue;
                    }
                    if parameter.kind == "group_start" {
                        ui.add_space(8.0);
                        ui.label(RichText::new(&parameter.name).strong());
                        continue;
                    }
                    if parameter.kind == "group_end" {
                        ui.separator();
                        continue;
                    }
                    if parameter.kind == "button" {
                        if ui
                            .add_enabled(
                                parameter.enabled && !self.busy,
                                egui::Button::new(&parameter.name),
                            )
                            .clicked()
                        {
                            clicked_button = Some(parameter.slot);
                        }
                        continue;
                    }
                    if matches!(parameter.kind.as_str(), "custom" | "no_data") {
                        ui.label(&parameter.name);
                        ui.small(format!("{} (read-only)", parameter.kind));
                        continue;
                    }

                    let previous_value = parameter.value;
                    let previous_color = parameter.color;
                    let previous_components = parameter.components;
                    let previous_layer = parameter.layer_path.clone();
                    let previous_summary = parameter.debug_summary.clone();
                    ui.add_enabled_ui(parameter.enabled && !self.busy, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&parameter.name).small());
                            if let Some(default) = parameter_defaults
                                .iter()
                                .find(|default| default.slot == parameter.slot)
                            {
                                let changed_from_default = parameter.value != default.value
                                    || parameter.color != default.color
                                    || parameter.components != default.components
                                    || parameter.layer_path != default.layer_path
                                    || parameter.debug_summary != default.debug_summary;
                                if ui
                                    .add_enabled(
                                        changed_from_default,
                                        egui::Button::new("Reset").small(),
                                    )
                                    .clicked()
                                {
                                    parameter.value = default.value;
                                    parameter.color = default.color;
                                    parameter.components = default.components;
                                    parameter.layer_path = default.layer_path.clone();
                                    parameter.debug_summary = default.debug_summary.clone();
                                }
                            }
                        });
                        if parameter.kind == "layer" {
                            ui.horizontal(|ui| {
                                if ui.small_button("Choose image").clicked() {
                                    parameter.layer_path = rfd::FileDialog::new()
                                        .add_filter(
                                            "Image",
                                            &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"],
                                        )
                                        .pick_file();
                                }
                                ui.label(
                                    parameter
                                        .layer_path
                                        .as_ref()
                                        .and_then(|path| path.file_name())
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("Not connected"),
                                );
                            });
                        } else if parameter.kind == "arbitrary_data" {
                            ui.add(
                                egui::TextEdit::singleline(
                                    parameter.debug_summary.get_or_insert_with(String::new),
                                )
                                .desired_width(f32::INFINITY),
                            );
                        } else if parameter.kind == "path" {
                            ui.add(
                                egui::DragValue::new(&mut parameter.value)
                                    .range(0.0..=parameter.maximum),
                            );
                        } else if matches!(parameter.kind.as_str(), "angle" | "point" | "point3d") {
                            ui.horizontal(|ui| {
                                for (index, label) in ["X", "Y", "Z"]
                                    .iter()
                                    .enumerate()
                                    .take(parameter.component_count)
                                {
                                    ui.label(*label);
                                    ui.add(
                                        egui::DragValue::new(&mut parameter.components[index])
                                            .speed(0.1),
                                    );
                                }
                            });
                        } else if parameter.kind == "color" {
                            let mut color = Color32::from_rgba_unmultiplied(
                                parameter.color[1],
                                parameter.color[2],
                                parameter.color[3],
                                parameter.color[0],
                            );
                            if ui.color_edit_button_srgba(&mut color).changed() {
                                parameter.color = [color.a(), color.r(), color.g(), color.b()];
                            }
                        } else if !parameter.choices.is_empty() {
                            let mut selected = parameter.value as usize;
                            egui::ComboBox::from_id_salt(("effect-control", parameter.slot))
                                .selected_text(
                                    parameter
                                        .choices
                                        .get(selected.saturating_sub(1))
                                        .map(String::as_str)
                                        .unwrap_or("Unknown"),
                                )
                                .show_ui(ui, |ui| {
                                    for (index, choice) in parameter.choices.iter().enumerate() {
                                        ui.selectable_value(&mut selected, index + 1, choice);
                                    }
                                });
                            parameter.value = selected as f64;
                        } else if parameter.kind == "integer"
                            && parameter.minimum == 0.0
                            && parameter.maximum == 1.0
                        {
                            let mut checked = parameter.value != 0.0;
                            if ui.checkbox(&mut checked, "Enabled").changed() {
                                parameter.value = f64::from(checked);
                            }
                        } else {
                            ui.add(
                                egui::Slider::new(
                                    &mut parameter.value,
                                    parameter.minimum..=parameter.maximum,
                                )
                                .show_value(true),
                            );
                        }
                    });
                    let changed = parameter.value != previous_value
                        || parameter.color != previous_color
                        || parameter.components != previous_components
                        || parameter.layer_path != previous_layer
                        || parameter.debug_summary != previous_summary;
                    if changed {
                        if parameter.supervised {
                            self.pending_parameter_slot = Some(parameter.slot);
                            self.live_render_due =
                                Some(Instant::now() + std::time::Duration::from_millis(500));
                        } else if self.live_render {
                            self.pending_live_render = true;
                            self.live_render_due =
                                Some(Instant::now() + std::time::Duration::from_millis(500));
                        }
                    }
                    ui.add_space(4.0);
                }
            });
        if let Some(slot) = clicked_button {
            self.pending_parameter_slot = Some(slot);
            self.pending_live_render = self.live_render;
            self.live_render_due = Some(Instant::now() + std::time::Duration::from_millis(500));
        }
    }

    fn dispatch_pending_parameter_change(&mut self, ctx: &egui::Context) {
        let Some(due) = self.live_render_due else {
            return;
        };
        if self.busy || Instant::now() < due {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
            return;
        }
        self.live_render_due = None;
        if let Some(slot) = self.pending_parameter_slot.take() {
            self.render_after_parameter_change = self.live_render && self.input_image.is_some();
            self.trigger_button(slot);
        } else if self.pending_live_render && self.live_render && self.input_image.is_some() {
            self.pending_live_render = false;
            self.quick_render();
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let result = self
            .receiver
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok());
        let Some(result) = result else {
            if self.busy {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            return;
        };
        let task_kind = self.task_kind;
        self.busy = false;
        self.rendering = false;
        if result.diagnostic_eligible {
            if let (Some(identity), Some(operation)) = (&result.identity, &result.operation) {
                let summary = diagnostic_summary(result.success, &result.body);
                let details = diagnostic_details(result.success, &result.body);
                match persist_diagnostic(
                    &self.repository,
                    identity,
                    operation,
                    result.success,
                    &summary,
                    &details,
                ) {
                    Ok(_) => {
                        self.diagnostic_warning = None;
                        self.diagnostic_history = self
                            .selection
                            .as_ref()
                            .map(|selection| {
                                load_diagnostic_history(&self.repository, &selection.sha256)
                            })
                            .unwrap_or_default();
                        self.missing_suite_aggregate = aggregate_missing_suites(&self.repository);
                    }
                    Err(error) => {
                        self.diagnostic_warning = Some(format!("Diagnostic save warning: {error}"))
                    }
                }
            }
        }
        self.failure_diagnostics = (!result.success)
            .then(|| failure_diagnostics(&result.body))
            .flatten();
        if !result.success {
            self.render_diagnostics = None;
        }
        self.matrix_results = serde_json::from_str(&result.body)
            .ok()
            .as_ref()
            .and_then(compatibility_matrix)
            .unwrap_or_default();
        self.status = if result.success {
            "Completed"
        } else {
            "Failed safely"
        }
        .into();
        let mut inspect_selected_aex = false;
        if task_kind == TaskKind::IdentifyAex && result.success {
            let mut lines = result.body.lines();
            if let (Some(path), Some(size), Some(hash)) = (lines.next(), lines.next(), lines.next())
            {
                let profile = profile_for_hash(hash);
                // A newly selected AEX replaces whatever the resident worker
                // was opened for.
                self.close_live_session();
                self.selection = Some(Selection {
                    path: path.into(),
                    size: size.parse().unwrap_or(0),
                    sha256: hash.into(),
                    profile,
                    modified: fs::metadata(path)
                        .ok()
                        .and_then(|value| value.modified().ok()),
                });
                self.session_approved = true;
                self.approved_dependencies.clear();
                self.approval_check = false;
                self.selection_stale = false;
                self.diagnostic_history = load_diagnostic_history(&self.repository, hash);
                match discover_adjacent_imports(Path::new(path)) {
                    Ok(discovery) => {
                        self.accept_adjacent_discovery(discovery);
                        match self.approve_session() {
                            Ok(()) => inspect_selected_aex = true,
                            Err(error) => {
                                self.invalidate_session_approval(
                                    "Automatic dependency manifest validation failed.",
                                );
                                self.report = error;
                            }
                        }
                    }
                    Err(error) => {
                        self.invalidate_session_approval(
                            "Automatic adjacent dependency discovery failed safely.",
                        );
                        self.report = error;
                    }
                }
            }
        }
        let effect_controls_ready = task_kind == TaskKind::InspectParameters && result.success;
        if effect_controls_ready {
            if let Ok(report) = serde_json::from_str::<serde_json::Value>(&result.body) {
                if let Ok(parameters) = serde_json::from_value(report["parameters"].clone()) {
                    self.parameters = parameters;
                    self.parameter_defaults = self.parameters.clone();
                    self.audio_effect_only = report["worker_diagnostics"]["audio_effect_only"]
                        .as_bool()
                        .unwrap_or(false);
                    self.smart_render_advertised = advertised_smart_render(&report);
                    if let Some(advertised) = self.smart_render_advertised {
                        self.smart_render = advertised;
                    }
                    self.status = format!(
                        "Effect Controls ready: {} editable parameter(s). Render path: {}.",
                        self.parameters.len(),
                        if self.smart_render { "SmartFX" } else { "Classic" }
                    );
                }
            }
        }
        if let Some(output) = result.output {
            self.render_diagnostics = serde_json::from_str(&result.body)
                .ok()
                .as_ref()
                .and_then(render_diagnostics);
            self.output_image = Some(output.clone());
            if let Ok(preview) = load_preview(ctx, "output", &output) {
                self.preview = Some(preview);
                self.viewer_mode = 1;
            }
            self.refresh_pixel_comparison();
        }
        if let Ok(report) = serde_json::from_str(&result.body) {
            apply_dynamic_ui_report(&mut self.parameters, &report);
        }
        self.report = result.body;
        self.receiver = None;
        self.task_kind = TaskKind::Generic;
        if inspect_selected_aex {
            self.inspect_parameters_async();
        } else if effect_controls_ready {
            self.start_live_render_if_ready();
        } else if self.render_after_parameter_change && result.success {
            self.render_after_parameter_change = false;
            self.quick_render();
        } else {
            self.render_after_parameter_change = false;
        }
    }
}

impl eframe::App for HarnessApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_visuals(egui::Visuals::dark());
        self.poll(ctx);
        self.dispatch_pending_parameter_change(ctx);
        self.check_selected_identity();
        if self.inspect_after_refresh && !self.busy {
            self.inspect_after_refresh = false;
            self.inspect_parameters_async();
        }
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("AEXCompat").size(22.0));
                ui.weak("EFFECT LAB");
                ui.separator();
                ui.label(RichText::new("SOURCE").small().strong());
                if ui
                    .add_enabled(!self.busy, egui::Button::new("AEX..."))
                    .clicked()
                {
                    self.reset_and_choose_aex();
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Image..."))
                    .clicked()
                {
                    self.choose_input(ctx);
                    self.viewer_mode = 0;
                }
                ui.separator();
                ui.label(RichText::new("PREVIEW").small().strong());
                let can_render =
                    !self.busy && self.selection.is_some() && self.input_image.is_some();
                if ui
                    .add_enabled(can_render, egui::Button::new("Render"))
                    .clicked()
                {
                    self.quick_render();
                }
                ui.separator();
                let live_render_changed =
                    ui.checkbox(&mut self.live_render, "Auto Update").changed();
                if live_render_changed && !self.live_render {
                    self.pending_live_render = false;
                    if self.pending_parameter_slot.is_none() {
                        self.live_render_due = None;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.busy {
                        ui.spinner();
                    }
                    ui.label(RichText::new(&self.status).small());
                });
            });
            ui.add_space(6.0);
        });
        egui::SidePanel::left("effect_controls")
            .default_width(340.0)
            .min_width(260.0)
            .max_width(460.0)
            .resizable(true)
            .show(ctx, |ui| self.show_effect_controls(ui));
        egui::CentralPanel::default().show(ctx, |ui| {
            self.show_workspace_viewer(ui);
            ui.separator();
            egui::CollapsingHeader::new("Analysis, render settings and diagnostics")
                .default_open(false)
                .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Change AEX source..."))
                .clicked()
            {
                self.reset_and_choose_aex();
            }
            if let Some(selected) = &self.selection {
                let selected_path = selected.path.display().to_string();
                let selected_size = selected.size;
                let selected_hash = selected.sha256.clone();
                let selected_profile = selected.profile;
                ui.group(|ui| {
                    ui.label(RichText::new(selected_path).strong());
                    ui.collapsing("Binary details and dependency DLLs", |ui| {
                    ui.label(format!("{} bytes", selected_size));
                    ui.monospace(&selected_hash);
                    if self.selection_stale {
                        ui.colored_label(Color32::from_rgb(210, 75, 55), "Build changed: native execution is paused until reload");
                    }
                    if let Some(profile) = selected_profile {
                        ui.colored_label(Color32::from_rgb(30, 150, 95), format!("Registered profile: {profile}"));
                    }
                    ui.separator();
                    ui.label(RichText::new("Local diagnostics").strong());
                    ui.label(format!("Events for selected SHA: {}", self.diagnostic_history.count));
                    ui.label(format!("Latest: {}", self.diagnostic_history.latest.as_deref().unwrap_or("none")));
                    if let Some(warning) = &self.diagnostic_warning {
                        ui.colored_label(Color32::from_rgb(210, 145, 40), warning);
                    }
                    if ui.button("Reload diagnostics").clicked() {
                        self.diagnostic_history = load_diagnostic_history(&self.repository, &selected_hash);
                        self.missing_suite_aggregate = aggregate_missing_suites(&self.repository);
                    }
                    let aggregate = &self.missing_suite_aggregate;
                    ui.label(RichText::new("Missing Suite gaps across all SHA directories").strong());
                    ui.label(format!(
                        "Coverage: {}/{} SHA directories; {} validated failure events",
                        aggregate.scanned_sha_count,
                        aggregate.discovered_sha_count,
                        aggregate.valid_failure_event_count
                    ));
                    ui.label(format!(
                        "Skipped entries/files: {}; truncated: {}",
                        aggregate.skipped_count,
                        if aggregate.truncated { "yes" } else { "no" }
                    ));
                    for gap in &aggregate.top {
                        ui.monospace(format!(
                            "{}@{}  SHA gaps={}  events={}",
                            gap.name, gap.version, gap.sha_count, gap.event_count
                        ));
                    }
                    ui.separator();
                    ui.label(RichText::new("Session dependency DLLs").strong());
                    if self.dependencies.is_empty() {
                        ui.label("No additional DLLs selected.");
                    }
                    for warning in &self.preflight_warnings {
                        ui.colored_label(
                            Color32::from_rgb(210, 145, 40),
                            format!(
                                "Preflight note: {} ({}) was not found beside the AEX; it may be supplied by the runtime environment.",
                                warning.basename,
                                warning.kind.as_str()
                            ),
                        );
                    }
                    let mut remove = None;
                    for (index, dependency) in self.dependencies.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let basename = dependency.path.file_name().and_then(|name| name.to_str()).unwrap_or("<invalid>");
                            let short_hash = dependency.sha256.get(..12).unwrap_or(&dependency.sha256);
                            ui.monospace(format!("{basename}  {short_hash}...  {} bytes", dependency.size));
                            if ui.add_enabled(!self.busy, egui::Button::new("Remove")).clicked() {
                                remove = Some(index);
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.busy, egui::Button::new("Add DLL")).clicked() {
                            self.add_dependency();
                        }
                        if ui.add_enabled(!self.busy && !self.dependencies.is_empty(), egui::Button::new("Clear all")).clicked() {
                            self.dependencies.clear();
                            self.preflight_warnings.clear();
                            self.approved_dependencies.clear();
                            self.session_approved = true;
                            self.status = "Dependency list cleared.".into();
                            self.close_live_session();
                        }
                    });
                    if let Some(index) = remove {
                        self.dependencies.remove(index);
                        if self.dependencies.is_empty() {
                            self.approved_dependencies.clear();
                            self.session_approved = true;
                            self.close_live_session();
                        } else if let Err(error) = self.approve_session() {
                            self.invalidate_session_approval("Dependency manifest validation failed.");
                            self.report = error;
                        }
                    }
                    if selected_profile.is_none() {
                        ui.colored_label(Color32::from_rgb(215, 145, 40), "Unregistered AEX: direct isolated execution enabled");
                        ui.label("The selected binary is hashed automatically and runs in a timeout-limited restricted worker. This is not a complete security sandbox.");
                    }
                    if ui.add_enabled(!self.busy, egui::Button::new("Reload rebuilt AEX")).clicked() {
                        self.refresh_aex();
                    }
                    });
                });
                if self.session_approved && !self.selection_stale {
                    ui.add_space(10.0);
                    if ui.add_enabled(!self.busy, egui::Button::new("Reload Effect Controls")).clicked() { self.inspect_parameters_async(); }
                    ui.collapsing("Developer probes and diagnostics", |ui| {
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.busy, egui::Button::new("Inspect all dependencies")).clicked() { self.inspect_external_dependencies(false); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Inspect missing dependencies")).clicked() { self.inspect_external_dependencies(true); }
                    });
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe 2-frame persistent sequence")).clicked() { self.probe_persistent_sequence(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe sequence save/reload")).clicked() { self.probe_flattened_sequence(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe non-destructive sequence save")).clicked() { self.probe_copied_flattened_sequence(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe options dialog")).clicked() { self.probe_options_dialog(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe automatic options dialog")).clicked() { self.probe_automatic_options_dialog(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe NOP_RENDER passthrough")).clicked() { self.probe_nop_render(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe SmartFX NOP_RENDER passthrough")).clicked() { self.probe_smart_nop_render(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe input-buffer write access")).clicked() { self.probe_input_buffer_write(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe SmartFX input-buffer write access")).clicked() { self.probe_smart_input_buffer_write(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe FRAME_SETUP expansion")).clicked() { self.probe_frame_resize(true); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe FRAME_SETUP shrink")).clicked() { self.probe_frame_resize(false); }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events & 4 != 0)
                        && ui.add_enabled(!self.busy, egui::Button::new("Probe custom UI cursor")).clicked()
                    {
                        self.probe_custom_ui_cursor();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0)
                        && ui.add_enabled(!self.busy, egui::Button::new("Record custom UI draw")).clicked()
                    {
                        self.probe_custom_ui_draw();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0) {
                        let changed = ui.checkbox(
                            &mut self.apply_custom_ui_draw_to_render,
                            "Draw custom UI before each render",
                        ).changed();
                        if changed && self.apply_custom_ui_draw_to_render {
                            self.apply_custom_ui_click_to_render = false;
                        }
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0)
                        && ui.add_enabled(!self.busy, egui::Button::new("Test custom UI lifecycle")).clicked()
                    {
                        self.probe_custom_ui_lifecycle();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0)
                        && ui.add_enabled(!self.busy, egui::Button::new("Dispatch custom UI idle")).clicked()
                    {
                        self.probe_custom_ui_idle();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0) {
                        ui.group(|ui| {
                            ui.label(RichText::new("Custom UI key event").strong());
                            ui.horizontal(|ui| {
                                ui.label("Keycode");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_keycode).range(0u32..=0xC000_FFFFu32));
                                ui.label(format!("0x{:08X}", self.custom_ui_keycode));
                                ui.label("Modifiers");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_key_modifiers));
                                if ui.add_enabled(!self.busy, egui::Button::new("Dispatch key")).clicked() {
                                    self.probe_custom_ui_keydown();
                                }
                            });
                            ui.label("Default is printable A. The custom UI click X/Y values are used as the screen point.");
                        });
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events & 4 != 0) {
                        ui.group(|ui| {
                            ui.label(RichText::new("Custom UI click").strong());
                            ui.horizontal(|ui| {
                                ui.label("X");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_click_point[0]).range(0..=8192));
                                ui.label("Y");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_click_point[1]).range(0..=8192));
                                ui.color_edit_button_rgba_unmultiplied(&mut self.custom_ui_click_color);
                                if ui.add_enabled(!self.busy, egui::Button::new("Dispatch click")).clicked() {
                                    self.probe_custom_ui_click();
                                }
                            });
                            let changed = ui.checkbox(
                                &mut self.apply_custom_ui_click_to_render,
                                "Apply this click before each render",
                            ).changed();
                            if changed && self.apply_custom_ui_click_to_render {
                                self.apply_custom_ui_draw_to_render = false;
                            }
                        });
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events & 3 != 0) {
                        ui.group(|ui| {
                            ui.label(RichText::new("Comp / Layer custom UI drag").strong());
                            ui.horizontal(|ui| {
                                ui.label("End X");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_drag_end[0]).range(0..=8192));
                                ui.label("End Y");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_drag_end[1]).range(0..=8192));
                                ui.label("Steps");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_drag_steps).range(1..=32));
                                if ui.add_enabled(!self.busy, egui::Button::new("Dispatch drag")).clicked() {
                                    self.probe_custom_ui_drag();
                                }
                            });
                            ui.label("The custom UI click X/Y values above are used as the drag start.");
                            if ui.add_enabled(!self.busy, egui::Button::new("Dispatch mouse exited")).clicked() {
                                self.probe_custom_ui_mouse_exited();
                            }
                        });
                    }
                    ui.collapsing("AEGP diagnostics (advanced)", |ui| {
                        if ui.add_enabled(!self.busy, egui::Button::new("Initialize as AEGP")).clicked() { self.initialize_aegp(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Dispatch AEGP update-menu")).clicked() { self.update_aegp_menu(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Dispatch one AEGP idle tick")).clicked() { self.dispatch_aegp_idle(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Run AEGP command ON/OFF roundtrip")).clicked() { self.dispatch_aegp_command_roundtrip(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Run AEGP active-idle roundtrip")).clicked() { self.dispatch_aegp_active_idle_roundtrip(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Run AEGP comp-idle roundtrip")).clicked() { self.dispatch_aegp_comp_idle_roundtrip(); }
                    });
                    });
                    // Effect parameters live in the persistent left-side Effect Controls panel.
                    if false {
                    let mut clicked_button = None;
                    for parameter in &mut self.parameters {
                        if !parameter.visible {
                            continue;
                        }
                        if parameter.kind == "group_start" {
                            ui.add_space(6.0);
                            ui.label(RichText::new(&parameter.name).strong().size(16.0));
                            continue;
                        }
                        if parameter.kind == "group_end" {
                            ui.separator();
                            continue;
                        }
                        if parameter.kind == "button" {
                            if ui
                                .add_enabled(
                                    parameter.enabled && !self.busy,
                                    egui::Button::new(&parameter.name),
                                )
                                .clicked()
                            {
                                clicked_button = Some(parameter.slot);
                            }
                            continue;
                        }
                        if matches!(
                            parameter.kind.as_str(),
                            "custom" | "no_data"
                        ) {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(&parameter.name);
                                ui.monospace(format!("{} (read-only)", parameter.kind));
                                if parameter.custom_ui_events != 0 {
                                    ui.monospace(format!(
                                        "custom UI {}x{}, events=0x{:X}",
                                        parameter.control_size[0],
                                        parameter.control_size[1],
                                        parameter.custom_ui_events
                                    ));
                                }
                                if let Some(summary) = &parameter.debug_summary {
                                    ui.collapsing("Observed value", |ui| {
                                        ui.monospace(summary);
                                    });
                                } else {
                                    ui.label("No printable value exposed by the effect.");
                                }
                            });
                            continue;
                        }
                        let previous_value = parameter.value;
                        let previous_color = parameter.color;
                        let previous_components = parameter.components;
                        let previous_layer = parameter.layer_path.clone();
                        let previous_summary = parameter.debug_summary.clone();
                        ui.add_enabled_ui(parameter.enabled, |ui| ui.horizontal(|ui| {
                            ui.label(&parameter.name);
                            if parameter.kind == "layer" {
                                if ui.button("Select image").clicked() {
                                    parameter.layer_path = rfd::FileDialog::new()
                                        .add_filter("Image", &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"])
                                        .pick_file();
                                }
                                if let Some(path) = &parameter.layer_path {
                                    ui.monospace(path.display().to_string());
                                } else {
                                    ui.label("Not connected");
                                }
                            } else if parameter.kind == "arbitrary_data" {
                                let text = parameter.debug_summary.get_or_insert_with(String::new);
                                ui.add(egui::TextEdit::singleline(text).desired_width(320.0));
                                ui.label("PRINT/SCAN text");
                            } else if parameter.kind == "path" {
                                ui.add(egui::DragValue::new(&mut parameter.value).range(0.0..=parameter.maximum).speed(1.0));
                                ui.label("0=None, 1..N=mask index");
                            } else if matches!(parameter.kind.as_str(), "angle" | "point" | "point3d") {
                                let labels = ["X", "Y", "Z"];
                                for (index, label) in labels.iter().enumerate().take(parameter.component_count) {
                                    ui.label(*label);
                                    ui.add(egui::DragValue::new(&mut parameter.components[index]).speed(0.1).range(-32768.0..=32768.0));
                                }
                            } else if parameter.kind == "color" {
                                let mut color = Color32::from_rgba_unmultiplied(parameter.color[1], parameter.color[2], parameter.color[3], parameter.color[0]);
                                if ui.color_edit_button_srgba(&mut color).changed() {
                                    parameter.color = [color.a(), color.r(), color.g(), color.b()];
                                }
                            } else if !parameter.choices.is_empty() {
                                let mut selected = parameter.value as usize;
                                egui::ComboBox::from_id_salt(parameter.slot)
                                    .selected_text(parameter.choices.get(selected.saturating_sub(1)).map(String::as_str).unwrap_or("Unknown"))
                                    .show_ui(ui, |ui| {
                                        for (index, choice) in parameter.choices.iter().enumerate() {
                                            ui.selectable_value(&mut selected, index + 1, choice);
                                        }
                                    });
                                parameter.value = selected as f64;
                            } else if parameter.kind == "integer" && parameter.minimum == 0.0 && parameter.maximum == 1.0 {
                                let mut checked = parameter.value != 0.0;
                                if ui.checkbox(&mut checked, "").changed() { parameter.value = if checked { 1.0 } else { 0.0 }; }
                            } else {
                                ui.add(egui::Slider::new(&mut parameter.value, parameter.minimum..=parameter.maximum));
                            }
                        }));
                        if parameter.supervised
                            && (parameter.value != previous_value
                                || parameter.color != previous_color
                                || parameter.components != previous_components
                                || parameter.layer_path != previous_layer
                                || parameter.debug_summary != previous_summary)
                        {
                            clicked_button = Some(parameter.slot);
                        }
                    }
                    if let Some(slot) = clicked_button {
                        self.trigger_button(slot);
                    }
                    }
                    ui.horizontal(|ui| {
                        ui.label("Render path:");
                        ui.selectable_value(&mut self.smart_render, false, "Classic");
                        ui.selectable_value(&mut self.smart_render, true, "SmartFX");
                        if let Some(advertised) = self.smart_render_advertised {
                            ui.weak(if advertised { "advertised: SmartFX" } else { "advertised: Classic" });
                            if self.smart_render != advertised {
                                ui.colored_label(egui::Color32::from_rgb(230, 180, 60), "manual override");
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        use aexcompat_broker::image_render::RenderPixelFormat;
                        ui.label("Pixel depth:");
                        ui.selectable_value(&mut self.pixel_format, RenderPixelFormat::Argb8, "8 bpc");
                        ui.selectable_value(&mut self.pixel_format, RenderPixelFormat::Argb16, "16 bpc");
                        ui.selectable_value(&mut self.pixel_format, RenderPixelFormat::Argb32f, "32 bpc float");
                    });
                    ui.horizontal(|ui| {
                        use aexcompat_broker::image_render::RenderGpuBackend;
                        ui.label("GPU backend:");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::Auto, "Auto");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::Cuda, "CUDA");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::OpenCl, "OpenCL");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::DirectX, "DirectX");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::Cpu, "CPU");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Frame:");
                        if ui.add(egui::DragValue::new(&mut self.frame).range(0..=10_000_000)).changed() {
                            self.duration_frames = self.duration_frames.max(self.frame.saturating_add(1));
                        }
                        ui.label("Duration frames:");
                        ui.add(egui::DragValue::new(&mut self.duration_frames).range(self.frame.saturating_add(1)..=10_000_001));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Time scale:");
                        ui.add(egui::DragValue::new(&mut self.frames_per_second).range(1..=1_000_000));
                        ui.label("Frame step:");
                        ui.add(egui::DragValue::new(&mut self.frame_time_step).range(1..=100_000));
                        ui.label(format!("{:.5} fps", self.frames_per_second as f64 / self.frame_time_step as f64));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Rate presets:");
                        for (label, scale, step) in [("23.976", 24_000, 1_001), ("29.97", 30_000, 1_001), ("59.94", 60_000, 1_001)] {
                            if ui.button(label).clicked() {
                                self.frames_per_second = scale;
                                self.frame_time_step = step;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        let enabled = self.host_context.as_ref().and_then(|context| context.spatial).is_some();
                        let mut requested = enabled;
                        if ui.checkbox(&mut requested, "Spatial context").changed() {
                            if requested {
                                let context = self.host_context.get_or_insert_with(|| aexcompat_broker::render_request::HostContext {
                                    mask_scene: aexcompat_broker::render_request::MaskScene { masks: Vec::new() },
                                    spatial: None,
                                    render_environment: None,
                                    aux_channels: Vec::new(),
                                    alpha_as_coverage_params: Vec::new(),
                                });
                                context.spatial = Some(aexcompat_broker::render_request::SpatialContext {
                                    downsample_x: aexcompat_broker::render_request::RationalScale { numerator: 1, denominator: 1 },
                                    downsample_y: aexcompat_broker::render_request::RationalScale { numerator: 1, denominator: 1 },
                                    pixel_aspect_ratio: aexcompat_broker::render_request::RationalScale { numerator: 1, denominator: 1 },
                                    full_resolution_width: None,
                                    full_resolution_height: None,
                                    pre_effect_source_origin_x: None,
                                    pre_effect_source_origin_y: None,
                                });
                            } else if let Some(context) = &mut self.host_context {
                                context.spatial = None;
                                if context.mask_scene.masks.is_empty() { self.host_context = None; }
                            }
                        }
                        if enabled {
                            for (label, x, y, par) in [
                                ("Full", (1, 1), (1, 1), (1, 1)),
                                ("Half", (1, 2), (1, 2), (1, 1)),
                                ("Quarter", (1, 4), (1, 4), (1, 1)),
                                ("D1/DV NTSC", (1, 1), (1, 1), (10, 11)),
                            ] {
                                if ui.button(label).clicked() {
                                    if let Some(spatial) = self.host_context.as_mut().and_then(|context| context.spatial.as_mut()) {
                                        spatial.downsample_x = aexcompat_broker::render_request::RationalScale { numerator: x.0, denominator: x.1 };
                                        spatial.downsample_y = aexcompat_broker::render_request::RationalScale { numerator: y.0, denominator: y.1 };
                                        spatial.pixel_aspect_ratio = aexcompat_broker::render_request::RationalScale { numerator: par.0, denominator: par.1 };
                                        if let Some((width, height)) = self.input_image.as_ref().and_then(|path| image::image_dimensions(path).ok()) {
                                            spatial.full_resolution_width = width.checked_mul(x.1 as u32).and_then(|value| value.checked_div(x.0 as u32));
                                            spatial.full_resolution_height = height.checked_mul(y.1 as u32).and_then(|value| value.checked_div(y.0 as u32));
                                        }
                                    }
                                }
                            }
                        }
                    });
                    if let Some(spatial) = self.host_context.as_mut().and_then(|context| context.spatial.as_mut()) {
                        ui.horizontal(|ui| {
                            for (label, ratio) in [
                                ("Downsample X", &mut spatial.downsample_x),
                                ("Downsample Y", &mut spatial.downsample_y),
                                ("Pixel aspect", &mut spatial.pixel_aspect_ratio),
                            ] {
                                ui.label(label);
                                ui.add(egui::DragValue::new(&mut ratio.numerator).range(1..=1_000_000));
                                ui.label("/");
                                ui.add(egui::DragValue::new(&mut ratio.denominator).range(1..=1_000_000));
                            }
                        });
                        ui.horizontal(|ui| {
                            let mut explicit = spatial.full_resolution_width.is_some() && spatial.full_resolution_height.is_some();
                            if ui.checkbox(&mut explicit, "Explicit full-resolution size").changed() {
                                if explicit {
                                    let dimensions = self.input_image.as_ref().and_then(|path| image::image_dimensions(path).ok()).unwrap_or((1, 1));
                                    spatial.full_resolution_width = Some(dimensions.0);
                                    spatial.full_resolution_height = Some(dimensions.1);
                                } else {
                                    spatial.full_resolution_width = None;
                                    spatial.full_resolution_height = None;
                                }
                            }
                            if let (Some(width), Some(height)) = (&mut spatial.full_resolution_width, &mut spatial.full_resolution_height) {
                                ui.add(egui::DragValue::new(width).range(1..=32768));
                                ui.label("x");
                                ui.add(egui::DragValue::new(height).range(1..=32768));
                            }
                        });
                        ui.horizontal(|ui| {
                            let mut explicit = spatial.pre_effect_source_origin_x.is_some()
                                && spatial.pre_effect_source_origin_y.is_some();
                            if ui.checkbox(&mut explicit, "Pre-effect source origin").changed() {
                                if explicit {
                                    spatial.pre_effect_source_origin_x = Some(0);
                                    spatial.pre_effect_source_origin_y = Some(0);
                                } else {
                                    spatial.pre_effect_source_origin_x = None;
                                    spatial.pre_effect_source_origin_y = None;
                                }
                            }
                            if let (Some(x), Some(y)) = (
                                &mut spatial.pre_effect_source_origin_x,
                                &mut spatial.pre_effect_source_origin_y,
                            ) {
                                ui.label("X");
                                ui.add(egui::DragValue::new(x).range(-32768..=32768));
                                ui.label("Y");
                                ui.add(egui::DragValue::new(y).range(-32768..=32768));
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        let enabled = self.host_context.as_ref().and_then(|context| context.render_environment).is_some();
                        let mut requested = enabled;
                        if ui.checkbox(&mut requested, "Render environment").changed() {
                            if requested {
                                let context = self.host_context.get_or_insert_with(|| aexcompat_broker::render_request::HostContext {
                                    mask_scene: aexcompat_broker::render_request::MaskScene { masks: Vec::new() },
                                    spatial: None,
                                    render_environment: None,
                                    aux_channels: Vec::new(),
                                    alpha_as_coverage_params: Vec::new(),
                                });
                                context.render_environment = Some(aexcompat_broker::render_request::RenderEnvironment {
                                    quality: aexcompat_broker::render_request::RenderQuality::High,
                                    field: aexcompat_broker::render_request::RenderField::Frame,
                                    shutter_angle: 0.0,
                                    shutter_phase: 0.0,
                                });
                            } else if let Some(context) = &mut self.host_context {
                                context.render_environment = None;
                                if context.mask_scene.masks.is_empty() && context.spatial.is_none() { self.host_context = None; }
                            }
                        }
                    });
                    if let Some(environment) = self.host_context.as_mut().and_then(|context| context.render_environment.as_mut()) {
                        ui.horizontal(|ui| {
                            use aexcompat_broker::render_request::{RenderField, RenderQuality};
                            ui.label("Quality:");
                            ui.selectable_value(&mut environment.quality, RenderQuality::Low, "Low");
                            ui.selectable_value(&mut environment.quality, RenderQuality::High, "High");
                            ui.label("Field:");
                            ui.selectable_value(&mut environment.field, RenderField::Frame, "Frame");
                            ui.selectable_value(&mut environment.field, RenderField::Upper, "Upper");
                            ui.selectable_value(&mut environment.field, RenderField::Lower, "Lower");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Shutter angle:");
                            ui.add(egui::DragValue::new(&mut environment.shutter_angle).speed(0.01).range(0.0..=1.0));
                            ui.label("Shutter phase:");
                            ui.add(egui::DragValue::new(&mut environment.shutter_phase).speed(0.01).range(-1.0..=1.0));
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.busy && !self.parameters.is_empty(), egui::Button::new("Load debug request...")).clicked() { self.load_debug_request(); }
                        if ui.add_enabled(!self.busy && !self.parameters.is_empty(), egui::Button::new("Save debug request...")).clicked() { self.save_debug_request(); }
                    });
                    if let Some(mask_count) = self.host_context.as_ref().map(|context| context.mask_scene.masks.len()) {
                        ui.horizontal(|ui| {
                            ui.label(format!("Host mask context: {mask_count} mask(s)"));
                            if ui.add_enabled(!self.busy, egui::Button::new("Clear masks")).clicked() {
                                if let Some(context) = &mut self.host_context {
                                    context.mask_scene.masks.clear();
                                    if context.spatial.is_none() && context.render_environment.is_none() { self.host_context = None; }
                                }
                            }
                        });
                    }
                    if self.audio_effect_only {
                        ui.colored_label(Color32::from_rgb(30, 120, 170), RichText::new("Audio-only Effect").strong());
                        ui.label("Transport: 44.1 kHz, mono, float32 little-endian raw samples");
                        if ui.add_enabled(!self.busy, egui::Button::new("Change audio source (.f32)...")).clicked() { self.choose_audio_input(); }
                        if let Some(path) = &self.audio_input { ui.monospace(path.display().to_string()); }
                        if ui.add_enabled(!self.busy && self.audio_input.is_some(), egui::Button::new("4. Render and save audio (.f32)...")).clicked() { self.render_audio_and_save(); }
                    } else {
                        if ui.add_enabled(!self.busy, egui::Button::new("Change image source...")).clicked() { self.choose_input(ctx); }
                        if let Some(path) = &self.input_image { ui.monospace(path.display().to_string()); }
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!self.busy, egui::Button::new("Select visual audio sidecar (.f32, optional)")).clicked() { self.choose_audio_input(); }
                            if self.audio_input.is_some() && ui.add_enabled(!self.busy, egui::Button::new("Clear sidecar")).clicked() { self.audio_input = None; }
                        });
                        if let Some(path) = &self.audio_input { ui.monospace(format!("Audio sidecar: {}", path.display())); }
                        if self.audio_input.is_some() {
                            ui.label("Sidecar mode: classic ARGB8, mono float32 LE, 44.1 kHz");
                        }
                        if ui.add_enabled(!self.busy, egui::Button::new("Select AE reference output (optional)")).clicked() { self.choose_reference(ctx); }
                        if let Some(path) = &self.reference_image { ui.monospace(format!("Reference: {}", path.display())); }
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!self.busy && self.input_image.is_some(), egui::Button::new("Render current frame")).clicked() { self.quick_render(); }
                            if ui.add_enabled(!self.busy && self.input_image.is_some(), egui::Button::new("Render and save PNG...")).clicked() { self.render_and_save(); }
                            if ui.add_enabled(!self.busy && self.input_image.is_some(), egui::Button::new("Run 6-case compatibility matrix")).clicked() { self.run_compatibility_matrix(); }
                        });
                    }
                }
            }
            ui.separator();
            ui.label(RichText::new(&self.status).strong());
            if let Some(path) = &self.output_image { ui.monospace(format!("Output: {}", path.display())); }
            if self.input_preview.is_some() || self.preview.is_some() || self.reference_preview.is_some() {
                if ui.button("Open FHD image viewer").clicked() {
                    self.viewer_open = true;
                    self.viewer_mode = if self.preview.is_some() { 2 } else { 0 };
                }
                ui.columns(3, |columns| {
                    show_preview(&mut columns[0], "Input", self.input_preview.as_ref());
                    show_preview(&mut columns[1], "AEX output", self.preview.as_ref());
                    show_preview(&mut columns[2], "AE reference", self.reference_preview.as_ref());
                });
            }
            if let Some(comparison) = &self.pixel_comparison {
                ui.group(|ui| match comparison {
                    Ok(comparison) => {
                        let color = if comparison.exact() {
                            Color32::from_rgb(30, 150, 95)
                        } else {
                            Color32::from_rgb(215, 145, 40)
                        };
                        ui.colored_label(
                            color,
                            RichText::new(if comparison.exact() {
                                "Pixel-exact match with AE reference"
                            } else {
                                "Pixel difference from AE reference"
                            })
                            .strong(),
                        );
                        ui.monospace(format!(
                            "{}x{} | differing pixels: {} / {} | max channel error: {} | MAE: {:.6}",
                            comparison.width,
                            comparison.height,
                            comparison.differing_pixels,
                            u64::from(comparison.width) * u64::from(comparison.height),
                            comparison.max_channel_error,
                            comparison.mean_absolute_error
                        ));
                    }
                    Err(error) => {
                        ui.colored_label(Color32::from_rgb(210, 75, 55), RichText::new("AE reference comparison unavailable").strong());
                        ui.label(error);
                    }
                });
            }
            if let Some(diagnostics) = &self.render_diagnostics {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Effect render diagnostics").strong());
                        ui.monospace(format!(
                            "{} / {} / {}",
                            diagnostics.render_path,
                            diagnostics.pixel_format,
                            diagnostics.worker_classification
                        ));
                    });
                    if diagnostics.gpu_fallback_used {
                        ui.colored_label(
                            Color32::from_rgb(215, 145, 40),
                            format!(
                                "GPU attempt {} at {}; output rejected, fresh CPU worker succeeded",
                                diagnostics
                                    .gpu_attempt_classification
                                    .as_deref()
                                    .unwrap_or("failed"),
                                diagnostics.gpu_failure_stage.as_deref().unwrap_or("unknown stage")
                            ),
                        );
                    }
                    ui.collapsing("Final selector timeline", |ui| {
                        for stage in &diagnostics.final_stages {
                            ui.monospace(stage);
                        }
                    });
                    if !diagnostics.gpu_stages.is_empty() {
                        ui.collapsing("Rejected GPU selector timeline", |ui| {
                            for stage in &diagnostics.gpu_stages {
                                ui.monospace(stage);
                            }
                        });
                    }
                });
            }
            if let Some(diagnostics) = &self.failure_diagnostics {
                ui.group(|ui| {
                    let worker_succeeded = diagnostics.classification == "ok";
                    ui.colored_label(
                        if worker_succeeded {
                            Color32::from_rgb(205, 135, 35)
                        } else {
                            Color32::from_rgb(210, 75, 55)
                        },
                        RichText::new(if worker_succeeded {
                            "Effect worker succeeded; host validation stopped the result"
                        } else {
                            "Effect worker failed safely"
                        })
                        .strong(),
                    );
                    ui.monospace(format!(
                        "classification={} stage={} selector_error={} exit={} elapsed={}ms",
                        diagnostics.classification,
                        diagnostics.failure_stage.as_deref().unwrap_or("unknown"),
                        diagnostics
                            .selector_error
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".into()),
                        diagnostics
                            .exit_code
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".into()),
                        diagnostics
                            .elapsed_ms
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".into()),
                    ));
                    if let Some(selector) = &diagnostics.last_seh_selector {
                        ui.monospace(format!(
                            "seh_selector={} seh_error={} exception_code={}",
                            selector,
                            diagnostics
                                .last_seh_error
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "unknown".into()),
                            diagnostics
                                .last_seh_exception_code
                                .map(|value| format!("0x{value:08X}"))
                                .unwrap_or_else(|| "unknown".into()),
                        ));
                    }
                    if !diagnostics.missing_suites.is_empty() {
                        ui.label(RichText::new("Missing suites").strong());
                        for suite in &diagnostics.missing_suites {
                            ui.monospace(format!("suite:{}@{}", suite.name, suite.version));
                        }
                    }
                    ui.collapsing("Completed selector timeline", |ui| {
                        for stage in &diagnostics.stages {
                            ui.monospace(stage);
                        }
                    });
                });
            }
            if !self.matrix_results.is_empty() {
                ui.group(|ui| {
                    ui.label(RichText::new("Effect compatibility matrix").strong());
                    egui::Grid::new("effect_compatibility_matrix")
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Path");
                            ui.label("Depth");
                            ui.label("Result");
                            ui.label("Details");
                            ui.end_row();
                            for case in &self.matrix_results {
                                ui.monospace(&case.render_path);
                                ui.monospace(&case.pixel_format);
                                if case.passed {
                                    ui.colored_label(Color32::from_rgb(30, 150, 95), "PASS");
                                    let relation = match (
                                        case.output_relation.as_deref(),
                                        case.differing_input_pixels,
                                    ) {
                                        (Some("pixels_changed"), Some(count)) => {
                                            format!("pixels changed: {count}")
                                        }
                                        (Some(value), _) => value.replace('_', " "),
                                        _ => case.output_png.clone().unwrap_or_default(),
                                    };
                                    ui.monospace(relation);
                                } else if !case.applicable {
                                    ui.colored_label(Color32::from_rgb(215, 145, 40), "UNSUPPORTED");
                                    ui.monospace("AEX did not advertise this pixel depth");
                                } else {
                                    ui.colored_label(Color32::from_rgb(210, 75, 55), "FAIL");
                                    let mut details = format!(
                                        "{} / {} / error {}",
                                        case.classification,
                                        case.failure_stage.as_deref().unwrap_or("unknown"),
                                        case.selector_error
                                            .map(|value| value.to_string())
                                            .unwrap_or_else(|| "unknown".into())
                                    );
                                    if let Some(error) = &case.error {
                                        details.push_str(" / ");
                                        details.push_str(error);
                                    }
                                    ui.monospace(details);
                                }
                                ui.end_row();
                            }
                        });
                });
            }
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.report).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
            });
                });
                });
        });
        self.show_image_viewer(ctx);
    }
}

fn load_preview(
    ctx: &egui::Context,
    texture_name: &str,
    path: &Path,
) -> Result<egui::TextureHandle, String> {
    let image = image::open(path).map_err(|error| error.to_string())?;
    let rgba = image.into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(ctx.load_texture(
        texture_name,
        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        egui::TextureOptions::LINEAR,
    ))
}

fn show_preview(ui: &mut egui::Ui, label: &str, texture: Option<&egui::TextureHandle>) {
    ui.label(RichText::new(label).strong());
    let Some(texture) = texture else {
        ui.label("Not available");
        return;
    };
    let available = ui.available_width().max(1.0);
    let scale = (available / texture.size()[0] as f32).min(1.0);
    ui.image((
        texture.id(),
        egui::vec2(
            texture.size()[0] as f32 * scale,
            texture.size()[1] as f32 * scale,
        ),
    ));
}

fn show_viewer_texture(
    ui: &mut egui::Ui,
    label: &str,
    texture: Option<&egui::TextureHandle>,
    zoom: &mut f32,
    pan: &mut egui::Vec2,
) {
    let Some(texture) = texture else {
        ui.centered_and_justified(|ui| {
            ui.label(format!("{label} is not available"));
        });
        return;
    };
    let source = egui::vec2(texture.size()[0] as f32, texture.size()[1] as f32);
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).strong());
        ui.monospace(format!("{} x {}", texture.size()[0], texture.size()[1]));
        ui.weak("Wheel to zoom / drag to pan");
    });
    let available = ui.available_size().max(egui::vec2(1.0, 1.0));
    let (viewport, response) = ui.allocate_exact_size(available, egui::Sense::click_and_drag());
    if response.hovered() {
        let scroll = ui.input(|input| input.raw_scroll_delta.y);
        if scroll != 0.0 {
            let previous_zoom = *zoom;
            *zoom = (*zoom * (scroll * 0.0025).exp()).clamp(0.25, 8.0);
            if let Some(pointer) = response.hover_pos() {
                let pointer_from_center = pointer - viewport.center();
                *pan = pointer_from_center - (pointer_from_center - *pan) * (*zoom / previous_zoom);
            }
        }
    }
    if response.dragged_by(egui::PointerButton::Primary)
        || response.dragged_by(egui::PointerButton::Middle)
    {
        *pan += response.drag_delta();
    }
    let fit_scale = (available.x / source.x).min(available.y / source.y);
    let display = source * fit_scale * *zoom;
    let image_rect = egui::Rect::from_center_size(viewport.center() + *pan, display);
    ui.painter().with_clip_rect(viewport).image(
        texture.id(),
        image_rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

fn repository_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent()?
                .parent()?
                .parent()?
                .parent()
                .map(Path::to_path_buf)
        })
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../..")
                .canonicalize()
                .unwrap()
        })
}

fn main() -> eframe::Result {
    let repository = repository_root();
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 4 && args[1] == "--compare-images" {
        match compare_images(Path::new(&args[2]), Path::new(&args[3])) {
            Ok(comparison) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&comparison.report()).unwrap()
                );
                if !comparison.exact() {
                    std::process::exit(2);
                }
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 5 && args[1] == "--render-experimental-matrix" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let report = run_effect_matrix(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
            None,
            None,
        );
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return Ok(());
    }
    if args.len() == 6 && args[1] == "--render-experimental-reference-matrix" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let report = run_effect_matrix(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[5]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
            Some(Path::new(&args[4])),
            None,
        );
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        if report["failed_count"].as_u64().unwrap_or(6) != 0 {
            std::process::exit(2);
        }
        return Ok(());
    }
    if args.len() == 4 && args[1] == "--render-image" {
        let report = aexcompat_broker::image_render::render_image(
            &repository,
            "scattermap",
            Path::new(&args[2]),
            Path::new(&args[3]),
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 5
        && matches!(
            args[1].to_string_lossy().as_ref(),
            "--render-experimental"
                | "--render-experimental-auto"
                | "--render-experimental-16"
                | "--render-experimental-16-deep"
                | "--render-experimental-32"
                | "--render-experimental-smart"
                | "--render-experimental-smart-16"
                | "--render-experimental-smart-16-deep"
                | "--render-experimental-smart-32"
                | "--render-experimental-smart-32-cpu"
        )
    {
        use aexcompat_broker::image_render::RenderPixelFormat;
        let command = args[1].to_string_lossy();
        let auto_path = command == "--render-experimental-auto";
        let deep16_png = command.ends_with("-16-deep");
        let pixel_format = if command.ends_with("-16") || deep16_png {
            RenderPixelFormat::Argb16
        } else if command.ends_with("-32") || command.ends_with("-32-cpu") {
            RenderPixelFormat::Argb32f
        } else {
            RenderPixelFormat::Argb8
        };
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let (parameters, inspection) =
            match aexcompat_broker::image_render::inspect_experimental_with_diagnostics(
                &repository,
                plugin,
                &hash,
            ) {
                Ok(inspected) => inspected,
                Err(error) if auto_path => {
                    eprintln!(
                        "automatic render-path selection needs a successful parameter \
                         inspection: {error}"
                    );
                    std::process::exit(1);
                }
                Err(_) => Default::default(),
            };
        let smart_advertised = inspection["smart_render_advertised"]
            .as_bool()
            .unwrap_or(false);
        let smart = if auto_path {
            smart_advertised
        } else {
            command.contains("smart")
        };
        let report = if deep16_png {
            aexcompat_broker::image_render::render_experimental_image_at_time_with_deep16_png(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                aexcompat_broker::image_render::RenderTiming::default(),
                smart,
            )
        } else if command.ends_with("-32-cpu") {
            aexcompat_broker::image_render::render_experimental_image_at_time_with_format_context_ui_action_and_gpu_backend(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                aexcompat_broker::image_render::RenderTiming::default(),
                smart,
                pixel_format,
                None,
                None,
                aexcompat_broker::image_render::RenderGpuBackend::Cpu,
            )
        } else {
            aexcompat_broker::image_render::render_experimental_image_at_time_with_format(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                aexcompat_broker::image_render::RenderTiming::default(),
                smart,
                pixel_format,
            )
        };
        match report {
            Ok(mut value) => {
                if auto_path {
                    value["render_path_source"] = serde_json::json!("advertised_out_flags2");
                    value["smart_render_advertised"] = serde_json::json!(smart_advertised);
                }
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 6
        && (args[1] == "--render-experimental-request"
            || args[1] == "--render-experimental-smart-request")
    {
        let smart = args[1] == "--render-experimental-smart-request";
        let request_path = Path::new(&args[5]);
        let request_bytes = fs::read(request_path).unwrap_or_else(|error| {
            eprintln!("assignment document could not be read: {error}");
            std::process::exit(1);
        });
        if request_bytes.len() > 64 * 1024 {
            eprintln!("assignment document exceeds 64 KiB");
            std::process::exit(1);
        }
        let document: serde_json::Value =
            serde_json::from_slice(&request_bytes).unwrap_or_else(|error| {
                eprintln!("assignment document is not valid JSON: {error}");
                std::process::exit(1);
            });
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let timing = typed_request_timing(&document).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(1);
        });
        let host_context = typed_request_host_context(&document).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(1);
        });
        if let Err(error) = apply_typed_assignments(&mut parameters, &document) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        let report = aexcompat_broker::image_render::render_experimental_image_at_time_with_format_and_context(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            timing,
            smart,
            aexcompat_broker::image_render::RenderPixelFormat::Argb8,
            host_context.as_ref(),
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 6 && args[1] == "--render-experimental-audio-request" {
        let request_bytes = fs::read(Path::new(&args[5])).unwrap_or_else(|error| {
            eprintln!("assignment document could not be read: {error}");
            std::process::exit(1);
        });
        if request_bytes.len() > 64 * 1024 {
            eprintln!("assignment document exceeds 64 KiB");
            std::process::exit(1);
        }
        let document: serde_json::Value =
            serde_json::from_slice(&request_bytes).unwrap_or_else(|error| {
                eprintln!("assignment document is not valid JSON: {error}");
                std::process::exit(1);
            });
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        if let Err(error) = apply_typed_assignments(&mut parameters, &document) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        match aexcompat_broker::image_render::render_experimental_audio(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 6 && args[1] == "--render-experimental-image-audio-sidecar" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        match aexcompat_broker::image_render::render_experimental_image_with_audio_sidecar(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            Path::new(&args[5]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() >= 7
        && args.len() % 2 == 1
        && (args[1] == "--render-experimental-layer-slots"
            || args[1] == "--render-experimental-smart-layer-slots")
    {
        let smart = args[1] == "--render-experimental-smart-layer-slots";
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        if let Err(error) = assign_layer_paths(&mut parameters, &args[5..]) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        let report = aexcompat_broker::image_render::render_experimental_image_at_time_with_format(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
            smart,
            aexcompat_broker::image_render::RenderPixelFormat::Argb8,
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() >= 7
        && args.len() % 2 == 1
        && (args[1] == "--render-experimental-param"
            || args[1] == "--render-experimental-smart-param")
    {
        let smart = args[1] == "--render-experimental-smart-param";
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let mut assigned_slots = Vec::new();
        for assignment in args[5..].chunks_exact(2) {
            let slot = assignment[0].to_string_lossy().parse::<u32>().unwrap_or(0);
            let value = assignment[1]
                .to_string_lossy()
                .parse::<f64>()
                .unwrap_or(f64::NAN);
            if assigned_slots.contains(&slot) {
                eprintln!("parameter slot {slot} was assigned more than once");
                std::process::exit(1);
            }
            let Some(parameter) = parameters.iter_mut().find(|item| item.slot == slot) else {
                eprintln!("AEX exposes no parameter at slot {slot}");
                std::process::exit(1);
            };
            if !value.is_finite() || value < parameter.minimum || value > parameter.maximum {
                eprintln!(
                    "parameter value must be within {}..={}",
                    parameter.minimum, parameter.maximum
                );
                std::process::exit(1);
            }
            parameter.value = value;
            assigned_slots.push(slot);
        }
        let report = aexcompat_broker::image_render::render_experimental_image_at_time_with_format(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
            smart,
            aexcompat_broker::image_render::RenderPixelFormat::Argb8,
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 7 && args[1] == "--render-experimental-smart-32-cpu-time" {
        let plugin = Path::new(&args[2]);
        let frame = args[5].to_string_lossy().parse::<i32>().unwrap_or(-1);
        let fps = args[6].to_string_lossy().parse::<u32>().unwrap_or(0);
        let timing = aexcompat_broker::image_render::RenderTiming {
            current_time: frame,
            time_step: 1,
            total_time: frame.saturating_add(1),
            time_scale: fps,
        };
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let report = aexcompat_broker::image_render::render_experimental_image_at_time_with_format_context_ui_action_and_gpu_backend(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            timing,
            true,
            aexcompat_broker::image_render::RenderPixelFormat::Argb32f,
            None,
            None,
            aexcompat_broker::image_render::RenderGpuBackend::Cpu,
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 7
        && (args[1] == "--render-experimental-time"
            || args[1] == "--render-experimental-smart-time")
    {
        let smart = args[1] == "--render-experimental-smart-time";
        let plugin = Path::new(&args[2]);
        let frame = args[5].to_string_lossy().parse::<i32>().unwrap_or(-1);
        let fps = args[6].to_string_lossy().parse::<u32>().unwrap_or(0);
        let timing = aexcompat_broker::image_render::RenderTiming {
            current_time: frame,
            time_step: 1,
            total_time: frame.saturating_add(1),
            time_scale: fps,
        };
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let report = if smart {
            aexcompat_broker::image_render::render_experimental_smart_image_at_time(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                timing,
            )
        } else {
            aexcompat_broker::image_render::render_experimental_image_at_time(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                timing,
            )
        };
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 6
        && (args[1] == "--render-experimental-layer"
            || args[1] == "--render-experimental-smart-layer")
    {
        let smart = args[1] == "--render-experimental-smart-layer";
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let Some(layer) = parameters.iter_mut().find(|item| item.kind == "layer") else {
            eprintln!("AEX exposes no secondary layer parameter");
            std::process::exit(1);
        };
        layer.layer_path = Some(PathBuf::from(&args[4]));
        let report = if smart {
            aexcompat_broker::image_render::render_experimental_smart_image(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[5]),
                &parameters,
            )
        } else {
            aexcompat_broker::image_render::render_experimental_image(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[5]),
                &parameters,
            )
        };
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if (6..=13).contains(&args.len())
        && (args[1] == "--render-experimental-layers"
            || args[1] == "--render-experimental-smart-layers")
    {
        let smart = args[1] == "--render-experimental-smart-layers";
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let layers = parameters
            .iter_mut()
            .filter(|item| item.kind == "layer")
            .collect::<Vec<_>>();
        if args.len() - 5 > layers.len() {
            eprintln!("more secondary images were supplied than observed layer parameters");
            std::process::exit(1);
        }
        for (layer, path) in layers.into_iter().zip(args[5..].iter()) {
            layer.layer_path = Some(PathBuf::from(path));
        }
        let report = if smart {
            aexcompat_broker::image_render::render_experimental_smart_image(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
            )
        } else {
            aexcompat_broker::image_render::render_experimental_image(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
            )
        };
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--inspect-experimental" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 5 && args[1] == "--inspect-experimental-runtime-policy" {
        let plugin = Path::new(&args[2]);
        let backend = match args[3].to_string_lossy().as_ref() {
            "cuda" => aexcompat_broker::runtime_module_policy::RuntimeBackend::Cuda,
            "opencl" => aexcompat_broker::runtime_module_policy::RuntimeBackend::Opencl,
            "directx" => aexcompat_broker::runtime_module_policy::RuntimeBackend::Directx,
            "opengl" => aexcompat_broker::runtime_module_policy::RuntimeBackend::Opengl,
            _ => {
                eprintln!("runtime policy backend must be cuda, opencl, directx, or opengl");
                std::process::exit(1);
            }
        };
        let policy = match fs::read(&args[4])
            .and_then(|bytes| aexcompat_broker::runtime_module_policy::parse_and_validate(&bytes))
        {
            Ok(policy) => policy,
            Err(error) => {
                eprintln!("runtime module policy rejected: {error}");
                std::process::exit(1);
            }
        };
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::inspect_experimental_with_runtime_policy(
            &repository,
            plugin,
            &hash,
            &policy,
            backend,
        ) {
            Ok((parameters, diagnostics)) => println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "parameters": parameters,
                    "diagnostics": diagnostics,
                }))
                .unwrap()
            ),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 4 && args[1] == "--inspect-experimental-dependencies" {
        let plugin = Path::new(&args[2]);
        let mode = args[3].to_string_lossy();
        if mode != "all" && mode != "missing" {
            eprintln!("dependency mode must be all or missing");
            std::process::exit(1);
        }
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::inspect_experimental_external_dependencies(
            &repository,
            plugin,
            &hash,
            mode == "missing",
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-options-dialog" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_options_dialog(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-automatic-options-dialog" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_automatic_options_dialog(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-nop-render" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_nop_render(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-smart-nop-render" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_smart_nop_render(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-input-buffer-write" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_input_buffer_write(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-smart-input-buffer-write" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_smart_input_buffer_write(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3
        && (args[1] == "--probe-experimental-expand-buffer"
            || args[1] == "--probe-experimental-shrink-buffer")
    {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let result = if args[1] == "--probe-experimental-expand-buffer" {
            aexcompat_broker::image_render::probe_experimental_expand_buffer(
                &repository,
                plugin,
                &hash,
            )
        } else {
            aexcompat_broker::image_render::probe_experimental_shrink_buffer(
                &repository,
                plugin,
                &hash,
            )
        };
        match result {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-persistent-sequence" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_persistent_sequence(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-flattened-sequence" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_flattened_sequence(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-copied-flattened-sequence" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_copied_flattened_sequence(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 4 && args[1] == "--trigger-experimental-button" {
        let plugin = Path::new(&args[2]);
        let slot = args[3].to_string_lossy().parse::<u32>().unwrap_or(0);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let parameters = match aexcompat_broker::image_render::inspect_experimental(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(parameters) => parameters,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        };
        match aexcompat_broker::image_render::trigger_experimental_button(
            &repository,
            plugin,
            &hash,
            slot,
            &parameters,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 5 && args[1] == "--trigger-experimental-request" {
        let plugin = Path::new(&args[2]);
        let slot = args[3].to_string_lossy().parse::<u32>().unwrap_or(0);
        let request_path = Path::new(&args[4]);
        let request_bytes = fs::read(request_path).unwrap_or_else(|error| {
            eprintln!("assignment document could not be read: {error}");
            std::process::exit(1);
        });
        if request_bytes.len() > 64 * 1024 {
            eprintln!("assignment document exceeds 64 KiB");
            std::process::exit(1);
        }
        let document: serde_json::Value =
            serde_json::from_slice(&request_bytes).unwrap_or_else(|error| {
                eprintln!("assignment document is invalid JSON: {error}");
                std::process::exit(1);
            });
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_else(|error| {
                    eprintln!("{error}");
                    std::process::exit(1);
                });
        if let Err(error) = apply_typed_assignments(&mut parameters, &document) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        match aexcompat_broker::image_render::trigger_experimental_button(
            &repository,
            plugin,
            &hash,
            slot,
            &parameters,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--initialize-experimental-aegp" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::initialize_experimental_aegp(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-update-menu" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_update_menu(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-idle" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_idle(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-command-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_command_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-active-idle-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_active_idle_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-comp-idle-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_comp_idle_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-keyframe-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_keyframe_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-seek-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_seek_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-trim-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_trim_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-switch-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_switch_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "AEXCompat Image Harness",
        options,
        Box::new(move |_cc| Ok(Box::new(HarnessApp::new(repository)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_signature_ignores_values_but_pins_structure() {
        let parameter = |slot: u32, kind: &str, value: f64| {
            serde_json::from_value::<aexcompat_broker::image_render::InteractiveParameter>(
                serde_json::json!({
                    "slot": slot, "name": "amount", "kind": kind,
                    "minimum": 0.0, "maximum": 100.0, "value": value,
                    "choices": [], "color": [0, 0, 0, 0],
                    "components": [0.0, 0.0, 0.0], "component_count": 0,
                    "layer_path": null, "enabled": true, "visible": true,
                    "supervised": false,
                }),
            )
            .expect("parameter fixture")
        };
        // A value change is a per-frame update, never a session reopen.
        assert_eq!(
            parameter_structure_signature(&[parameter(1, "float", 1.0)]),
            parameter_structure_signature(&[parameter(1, "float", 99.0)]),
        );
        // Structure changes must produce a different key.
        assert_ne!(
            parameter_structure_signature(&[parameter(1, "float", 1.0)]),
            parameter_structure_signature(&[parameter(2, "float", 1.0)]),
        );
        assert_ne!(
            parameter_structure_signature(&[parameter(1, "float", 1.0)]),
            parameter_structure_signature(&[parameter(1, "integer", 1.0)]),
        );
    }

    #[test]
    fn advertised_smart_render_reads_inspection_diagnostics_only() {
        let smart = serde_json::json!({
            "worker_diagnostics": { "smart_render_advertised": true }
        });
        assert_eq!(advertised_smart_render(&smart), Some(true));
        let classic = serde_json::json!({
            "worker_diagnostics": { "smart_render_advertised": false }
        });
        assert_eq!(advertised_smart_render(&classic), Some(false));
        // Reports from older workers without the field must not force a
        // default-path change.
        let missing = serde_json::json!({ "worker_diagnostics": {} });
        assert_eq!(advertised_smart_render(&missing), None);
        assert_eq!(advertised_smart_render(&serde_json::json!({})), None);
    }

    fn temporary_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "aexcompat-diagnostics-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn test_identity(byte: u8) -> DispatchIdentity {
        DispatchIdentity {
            sha256: format!("{byte:02x}").repeat(32),
            size: 123,
        }
    }

    fn persist_missing_suite_event(
        root: &Path,
        identity: &DispatchIdentity,
        nonce: &str,
        suites: serde_json::Value,
    ) {
        persist_diagnostic_with_nonce(
            root,
            identity,
            "render",
            false,
            "failed safely",
            &serde_json::json!({"classification":"failed", "missing_suites":suites}),
            nonce,
        )
        .unwrap();
    }

    #[test]
    fn missing_suite_aggregate_prefers_sha_coverage_and_separates_case_and_version() {
        let root = temporary_directory("aggregate-bias");
        let first = test_identity(0x10);
        let second = test_identity(0x20);
        for nonce in ["a", "b", "c"] {
            persist_missing_suite_event(
                &root,
                &first,
                nonce,
                serde_json::json!([{"name":"Repeated Suite","version":1}]),
            );
        }
        persist_missing_suite_event(
            &root,
            &first,
            "d",
            serde_json::json!([
                {"name":"Covered Suite","version":1},
                {"name":"covered suite","version":1},
                {"name":"Covered Suite","version":2}
            ]),
        );
        persist_missing_suite_event(
            &root,
            &second,
            "e",
            serde_json::json!([{"name":"Covered Suite","version":1}]),
        );
        let aggregate = aggregate_missing_suites(&root);
        assert_eq!(aggregate.top[0].name, "Covered Suite");
        assert_eq!(
            (aggregate.top[0].sha_count, aggregate.top[0].event_count),
            (2, 2)
        );
        assert!(aggregate.top.iter().any(|gap| gap.name == "covered suite"));
        assert!(aggregate
            .top
            .iter()
            .any(|gap| gap.name == "Covered Suite" && gap.version == 2));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_suite_aggregate_skips_corrupt_oversize_and_sha_mismatch_without_private_fields() {
        let root = temporary_directory("aggregate-bounds");
        let identity = test_identity(0x30);
        persist_missing_suite_event(
            &root,
            &identity,
            "valid",
            serde_json::json!([{"name":"PF World Suite","version":2}]),
        );
        let directory = diagnostic_directory(&root, &identity.sha256).unwrap();
        fs::write(directory.join("corrupt.local.json"), b"not-json").unwrap();
        fs::File::create(directory.join("oversize.local.json"))
            .unwrap()
            .set_len(MAX_DIAGNOSTIC_FILE_BYTES + 1)
            .unwrap();
        let mismatch = fs::read(directory.join("valid.local.json")).unwrap();
        let mut mismatch: serde_json::Value = serde_json::from_slice(&mismatch).unwrap();
        mismatch["identity"]["sha256"] = serde_json::json!("ff".repeat(32));
        fs::write(
            directory.join("mismatch.local.json"),
            serde_json::to_vec(&mismatch).unwrap(),
        )
        .unwrap();
        let aggregate = aggregate_missing_suites(&root);
        assert_eq!(aggregate.valid_failure_event_count, 1);
        assert!(aggregate.skipped_count >= 3);
        let debug = format!("{aggregate:?}");
        assert!(!debug.contains(&identity.sha256));
        assert!(!debug.contains("local.json"));
        assert!(!debug.contains("failed safely"));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn missing_suite_aggregate_rejects_reparse_sha_directories() {
        use std::os::windows::fs::symlink_dir;
        let root = temporary_directory("aggregate-reparse");
        let target = temporary_directory("aggregate-reparse-target");
        let diagnostics = root.join("target/harness-diagnostics");
        fs::create_dir_all(&diagnostics).unwrap();
        let link = diagnostics.join("ab".repeat(32));
        if symlink_dir(&target, &link).is_ok() {
            let aggregate = aggregate_missing_suites(&root);
            assert_eq!(aggregate.scanned_sha_count, 0);
            assert_eq!(aggregate.skipped_count, 1);
        }
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn adjacent_import_resolution_tracks_normal_delay_missing_present_and_system() {
        let mut adjacent = std::collections::HashMap::new();
        adjacent.insert("present.dll".into(), PathBuf::from("present.dll"));
        let mut warnings = std::collections::BTreeMap::new();
        assert!(resolve_adjacent_import(
            "present.dll",
            ImportKind::Normal,
            &adjacent,
            &mut warnings
        )
        .is_some());
        assert!(resolve_adjacent_import(
            "missing.dll",
            ImportKind::Normal,
            &adjacent,
            &mut warnings
        )
        .is_none());
        assert!(resolve_adjacent_import(
            "missing.dll",
            ImportKind::Delay,
            &adjacent,
            &mut warnings
        )
        .is_none());
        resolve_adjacent_import("missing.dll", ImportKind::Normal, &adjacent, &mut warnings);
        resolve_adjacent_import(
            "api-ms-win-core-file-l1-1-0.dll",
            ImportKind::Delay,
            &adjacent,
            &mut warnings,
        );
        assert_eq!(warnings.len(), 2);
        assert!(warnings.contains_key(&("missing.dll".into(), ImportKind::Normal)));
        assert!(warnings.contains_key(&("missing.dll".into(), ImportKind::Delay)));
        resolve_adjacent_import(
            "C:\\private\\secret.dll",
            ImportKind::Normal,
            &adjacent,
            &mut warnings,
        );
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn preflight_warning_event_contains_only_basenames_and_kinds() {
        let root = temporary_directory("preflight-privacy");
        let identity = test_identity(0x42);
        persist_preflight_warnings(
            &root,
            &identity,
            &[PreflightImportWarning {
                basename: "helper.dll".into(),
                kind: ImportKind::Delay,
            }],
        )
        .unwrap();
        let directory = diagnostic_directory(&root, &identity.sha256).unwrap();
        let path = fs::read_dir(directory)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(value["success"], true);
        assert_eq!(
            value["diagnostics"]["preflight_warnings"][0],
            serde_json::json!({
                "basename":"helper.dll", "kind":"delay"
            })
        );
        let diagnostics = value["diagnostics"].to_string();
        assert!(!diagnostics.contains("path"));
        assert!(!diagnostics.contains("error"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_dispatch_keeps_identity_when_selection_changes() {
        let mut app = HarnessApp::new(temporary_directory("identity-race"));
        app.selection = Some(Selection {
            path: PathBuf::from("first.aex"),
            size: 11,
            sha256: "11".repeat(32),
            profile: None,
            modified: None,
        });
        app.spawn_native("race_test", || Ok(("ok".into(), None)));
        app.selection = Some(Selection {
            path: PathBuf::from("second.aex"),
            size: 22,
            sha256: "22".repeat(32),
            profile: None,
            modified: None,
        });
        let result = app.receiver.take().unwrap().recv().unwrap();
        assert_eq!(
            result.identity,
            Some(DispatchIdentity {
                sha256: "11".repeat(32),
                size: 11
            })
        );
        assert_eq!(result.operation.as_deref(), Some("race_test"));
    }

    #[test]
    fn diagnostic_dto_is_private_bounded_and_collision_safe() {
        let root = temporary_directory("privacy");
        let identity = test_identity(0x33);
        let secret = "C:\\Users\\private\\effect.aex RAW_STDERR image-pixels ";
        assert_eq!(diagnostic_summary(false, secret), "failed safely");
        let summary = secret.to_owned();
        let path = persist_diagnostic_with_nonce(
            &root,
            &identity,
            "render_image",
            false,
            &summary,
            &diagnostic_details(false, secret),
            "same",
        )
        .unwrap();
        assert!(persist_diagnostic_with_nonce(
            &root,
            &identity,
            "render_image",
            true,
            "new",
            &serde_json::json!({"classification":"completed"}),
            "same"
        )
        .is_err());
        let bytes = fs::read(path).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(!text.contains("Users"));
        assert!(!text.contains("RAW_STDERR"));
        assert!(!text.contains("image-pixels"));
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["summary"], "redacted");
        let keys = value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            keys,
            [
                "identity",
                "diagnostics",
                "operation",
                "schema",
                "success",
                "summary",
                "timestamp",
                "version"
            ]
            .into_iter()
            .map(str::to_owned)
            .collect()
        );
        assert!(value["summary"].as_str().unwrap().len() <= MAX_DIAGNOSTIC_SUMMARY_BYTES);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn diagnostic_reader_ignores_corrupt_oversize_sha_mismatch_and_temp() {
        let root = temporary_directory("reader");
        let identity = test_identity(0x44);
        let directory = diagnostic_directory(&root, &identity.sha256).unwrap();
        fs::create_dir_all(&directory).unwrap();
        persist_diagnostic_with_nonce(
            &root,
            &identity,
            "valid",
            true,
            "valid summary",
            &serde_json::json!({"classification":"completed"}),
            "001",
        )
        .unwrap();
        fs::write(directory.join("002.local.json"), b"not-json").unwrap();
        fs::write(
            directory.join("003.local.json"),
            vec![b'x'; MAX_DIAGNOSTIC_FILE_BYTES as usize + 1],
        )
        .unwrap();
        let mismatch = serde_json::json!({"schema":DIAGNOSTIC_SCHEMA,"version":DIAGNOSTIC_VERSION,
            "identity":{"sha256":"55".repeat(32),"size":1},"summary":"wrong"});
        fs::write(
            directory.join("004.local.json"),
            serde_json::to_vec(&mismatch).unwrap(),
        )
        .unwrap();
        fs::write(directory.join("005.tmp"), b"ignored").unwrap();
        let history = load_diagnostic_history(&root, &identity.sha256);
        assert_eq!(
            history,
            DiagnosticHistory {
                count: 1,
                latest: Some("valid summary".into())
            }
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn diagnostic_details_preserve_bounded_compatibility_keys() {
        let body = concat!(
            r#"failed: diagnostics={"classification":"nonzero_exit","failure_stage":"render","exit_code":7,"missing_suites":[{"name":"PF World Suite","version":2}]}, report="#,
            r#"{"last_seh_selector":"RENDER","last_seh_error":512,"last_seh_exception_code":3221225477}"#,
        );
        let details = diagnostic_details(false, body);
        assert_eq!(details["classification"], "nonzero_exit");
        assert_eq!(details["failure_stage"], "render");
        assert_eq!(details["last_seh_selector"], "RENDER");
        assert_eq!(details["missing_suites"][0]["name"], "PF World Suite");
        assert_eq!(details["missing_suites"][0]["version"], 2);
        assert!(!details.to_string().contains("failed: diagnostics="));
    }

    fn temporary_aex(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "aexcompat-harness-{name}-{}-{}.{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            "aex"
        ))
    }

    fn temporary_png(name: &str) -> PathBuf {
        temporary_aex(name).with_extension("png")
    }

    fn parameter(slot: u32, kind: &str) -> aexcompat_broker::image_render::InteractiveParameter {
        aexcompat_broker::image_render::InteractiveParameter {
            slot,
            name: format!("Parameter {slot}"),
            kind: kind.into(),
            minimum: 0.0,
            maximum: 1.0,
            value: 0.0,
            choices: Vec::new(),
            color: [255, 0, 0, 0],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: kind == "button",
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }
    }

    #[test]
    fn layer_cli_assignments_are_multi_slot_and_fail_closed() {
        let assignments = ["2", "map.png", "9", "background.png"].map(std::ffi::OsString::from);
        let mut parameters = vec![
            parameter(1, "float"),
            parameter(2, "layer"),
            parameter(9, "layer"),
        ];
        assign_layer_paths(&mut parameters, &assignments).unwrap();
        assert_eq!(
            parameters[1].layer_path.as_deref(),
            Some(Path::new("map.png"))
        );
        assert_eq!(
            parameters[2].layer_path.as_deref(),
            Some(Path::new("background.png"))
        );

        let duplicate = ["2", "a.png", "2", "b.png"].map(std::ffi::OsString::from);
        let mut rejected = vec![parameter(2, "layer")];
        assert!(assign_layer_paths(&mut rejected, &duplicate)
            .unwrap_err()
            .contains("more than once"));
        assert!(rejected[0].layer_path.is_none());
        let wrong_type = ["1", "a.png"].map(std::ffi::OsString::from);
        assert!(assign_layer_paths(&mut parameters, &wrong_type)
            .unwrap_err()
            .contains("not a Layer input"));
        let unknown = ["77", "a.png"].map(std::ffi::OsString::from);
        assert!(assign_layer_paths(&mut parameters, &unknown)
            .unwrap_err()
            .contains("no parameter"));
    }

    #[test]
    fn typed_assignment_document_is_strict_typed_and_atomic() {
        let mut parameters = vec![
            parameter(1, "integer"),
            parameter(2, "color"),
            parameter(3, "point"),
            parameter(4, "layer"),
            parameter(5, "arbitrary_data"),
        ];
        parameters[0].maximum = 10.0;
        parameters[2].component_count = 2;
        let document = serde_json::json!({
            "schema_version": 1,
            "assignments": [
                {"slot": 1, "value": 7},
                {"slot": 2, "color": [255, 20, 40, 60]},
                {"slot": 3, "components": [320.0, 180.0]},
                {"slot": 4, "layer": "map.png"},
                {"slot": 5, "text": "value=7"}
            ]
        });
        apply_typed_assignments(&mut parameters, &document).unwrap();
        let default_timing = typed_request_timing(&document).unwrap();
        assert_eq!(default_timing.current_time, 0);
        assert_eq!(default_timing.time_scale, 1);
        assert_eq!(parameters[0].value, 7.0);
        assert_eq!(parameters[1].color, [255, 20, 40, 60]);
        assert_eq!(parameters[2].components[..2], [320.0, 180.0]);
        assert_eq!(
            parameters[3].layer_path.as_deref(),
            Some(Path::new("map.png"))
        );
        assert_eq!(parameters[4].debug_summary.as_deref(), Some("value=7"));
        let saved = typed_request_document(&parameters, 12, 60, 1, 600, None);
        let saved_timing = typed_request_timing(&saved).unwrap();
        assert_eq!(saved_timing.current_time, 12);
        assert_eq!(saved_timing.time_scale, 60);
        assert_eq!(saved_timing.total_time, 600);
        let mut roundtripped = vec![
            parameter(1, "integer"),
            parameter(2, "color"),
            parameter(3, "point"),
            parameter(4, "layer"),
            parameter(5, "arbitrary_data"),
        ];
        roundtripped[0].maximum = 10.0;
        roundtripped[2].component_count = 2;
        apply_typed_assignments(&mut roundtripped, &saved).unwrap();
        assert_eq!(
            serde_json::to_value(&roundtripped).unwrap(),
            serde_json::to_value(&parameters).unwrap()
        );

        let before = parameters.clone();
        for invalid in [
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":1,"value":3},{"slot":1,"value":4}
            ]}),
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":1,"value":3,"unknown":true}
            ]}),
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":2,"value":3}
            ]}),
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":1,"value":11}
            ]}),
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":5,"text":""}
            ]}),
        ] {
            assert!(apply_typed_assignments(&mut parameters, &invalid).is_err());
            assert_eq!(
                serde_json::to_value(&parameters).unwrap(),
                serde_json::to_value(&before).unwrap()
            );
        }

        let timed = serde_json::json!({
            "schema_version": 1,
            "timing": {"frame": 30, "fps": 24},
            "assignments": []
        });
        let timing = typed_request_timing(&timed).unwrap();
        assert_eq!(timing.current_time, 30);
        assert_eq!(timing.time_step, 1);
        assert_eq!(timing.total_time, 31);
        assert_eq!(timing.time_scale, 24);
        let duration_timing = typed_request_timing(&serde_json::json!({
            "timing":{"frame":30,"fps":24,"duration_frames":240}
        }))
        .unwrap();
        assert_eq!(duration_timing.total_time, 240);
        let fractional_timing = typed_request_timing(&serde_json::json!({
            "timing":{
                "frame":30,"time_scale":30000,"time_step":1001,"duration_frames":300
            }
        }))
        .unwrap();
        assert_eq!(fractional_timing.current_time, 30_030);
        assert_eq!(fractional_timing.time_step, 1_001);
        assert_eq!(fractional_timing.total_time, 300_300);
        assert_eq!(fractional_timing.time_scale, 30_000);
        for invalid_timing in [
            serde_json::json!({"timing":{"frame":-1,"fps":30}}),
            serde_json::json!({"timing":{"frame":1,"fps":0}}),
            serde_json::json!({"timing":{"frame":1,"fps":30,"extra":true}}),
            serde_json::json!({"timing":{"frame":30,"fps":30,"duration_frames":30}}),
            serde_json::json!({"timing":{"frame":1,"time_scale":30000}}),
            serde_json::json!({"timing":{"frame":1,"fps":30,"time_scale":30000,"time_step":1001}}),
            serde_json::json!({"timing":{"frame":10000000,"time_scale":1000000,"time_step":100000,"duration_frames":10000001}}),
        ] {
            assert!(typed_request_timing(&invalid_timing).is_err());
        }
    }

    #[test]
    fn supervised_change_applies_dynamic_ui_flags_atomically() {
        let mut parameters = vec![parameter(1, "integer"), parameter(2, "float")];
        parameters[0].supervised = true;
        let report = serde_json::json!({
            "user_changed_param_requested": true,
            "user_changed_param_error": 0,
            "parameters": [
                {"index": 1, "ui_flags": 1 << 5},
                {"index": 2, "ui_flags": 1 << 9}
            ]
        });
        assert!(apply_dynamic_ui_report(&mut parameters, &report));
        assert!(!parameters[0].enabled);
        assert!(parameters[0].visible);
        assert!(parameters[1].enabled);
        assert!(!parameters[1].visible);

        let before = serde_json::to_value(&parameters).unwrap();
        let incomplete = serde_json::json!({
            "user_changed_param_requested": true,
            "user_changed_param_error": 0,
            "parameters": [{"index": 1, "ui_flags": 0}]
        });
        assert!(!apply_dynamic_ui_report(&mut parameters, &incomplete));
        assert_eq!(serde_json::to_value(&parameters).unwrap(), before);
    }

    #[test]
    fn ae_reference_comparison_reports_exact_and_bounded_pixel_error() {
        let reference_path = temporary_png("reference");
        let exact_path = temporary_png("exact");
        let changed_path = temporary_png("changed");
        let reference =
            image::RgbaImage::from_raw(2, 1, vec![10, 20, 30, 255, 40, 50, 60, 128]).unwrap();
        reference.save(&reference_path).unwrap();
        reference.save(&exact_path).unwrap();
        let changed =
            image::RgbaImage::from_raw(2, 1, vec![10, 20, 30, 255, 40, 54, 60, 128]).unwrap();
        changed.save(&changed_path).unwrap();

        let exact = compare_images(&reference_path, &exact_path).unwrap();
        assert!(exact.exact());
        assert_eq!(exact.max_channel_error, 0);
        assert_eq!(exact.mean_absolute_error, 0.0);

        let changed = compare_images(&reference_path, &changed_path).unwrap();
        assert!(!changed.exact());
        assert_eq!(changed.differing_pixels, 1);
        assert_eq!(changed.max_channel_error, 4);
        assert_eq!(changed.mean_absolute_error, 0.5);

        for path in [reference_path, exact_path, changed_path] {
            fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn ae_reference_comparison_rejects_dimension_mismatch() {
        let reference_path = temporary_png("reference-size");
        let output_path = temporary_png("output-size");
        image::RgbaImage::new(2, 2).save(&reference_path).unwrap();
        image::RgbaImage::new(3, 2).save(&output_path).unwrap();
        let error = compare_images(&reference_path, &output_path).unwrap_err();
        assert!(error.contains("AE reference is 2x2"));
        assert!(error.contains("AEX output is 3x2"));
        fs::remove_file(reference_path).unwrap();
        fs::remove_file(output_path).unwrap();
    }

    #[test]
    fn only_exact_registered_hashes_are_recognized() {
        assert_ne!(SCATTERMAP_HASH, MASKOFFSET_HASH);
        assert_eq!(SCATTERMAP_HASH.len(), 64);
    }

    #[test]
    fn effect_diagnostics_separate_rejected_gpu_and_final_cpu_timelines() {
        let report = serde_json::json!({
            "stage": "interactive_image_render",
            "render_path": "smartfx",
            "pixel_format": "argb32f",
            "worker_classification": "ok",
            "gpu_fallback_used": true,
            "worker_diagnostics": { "stage_events": [
                { "stage": "smart_pre_render", "state": "end", "errors": { "error": 0 } },
                { "stage": "smart_render_cpu", "state": "end", "errors": { "error": 0 } }
            ]},
            "gpu_attempt": {
                "worker_classification": "nonzero_exit",
                "worker_diagnostics": {
                    "failure_stage": "gpu_device_setdown",
                    "stage_events": [
                        { "stage": "smart_render_gpu", "state": "end", "errors": { "error": 0 } },
                        { "stage": "gpu_device_setdown", "state": "end", "errors": { "error": 512 } }
                    ]
                }
            }
        });
        let diagnostics = render_diagnostics(&report).unwrap();
        assert_eq!(diagnostics.render_path, "smartfx");
        assert_eq!(diagnostics.pixel_format, "argb32f");
        assert!(diagnostics.gpu_fallback_used);
        assert_eq!(
            diagnostics.gpu_attempt_classification.as_deref(),
            Some("nonzero_exit")
        );
        assert_eq!(
            diagnostics.gpu_failure_stage.as_deref(),
            Some("gpu_device_setdown")
        );
        assert!(diagnostics.final_stages[1].starts_with("smart_render_cpu"));
        assert!(diagnostics.gpu_stages[0].starts_with("smart_render_gpu"));
        assert!(diagnostics.gpu_stages[1].contains("512"));
    }

    #[test]
    fn failed_worker_diagnostics_are_extracted_from_bounded_error_text() {
        let message = concat!(
            "isolated AEX image render failed validation: diagnostics=",
            r#"{"classification":"nonzero_exit","failure_stage":"smart_render_cpu","exit_code":22,"elapsed_ms":19,"stage_events":[{"stage":"smart_pre_render","state":"end","errors":{"error":0}},{"stage":"smart_render_cpu","state":"end","errors":{"error":25}}]}"#,
            r#", report={"smart_render_error":25}"#,
        );
        let diagnostics = failure_diagnostics(message).unwrap();
        assert_eq!(diagnostics.classification, "nonzero_exit");
        assert_eq!(
            diagnostics.failure_stage.as_deref(),
            Some("smart_render_cpu")
        );
        assert_eq!(diagnostics.exit_code, Some(22));
        assert_eq!(diagnostics.elapsed_ms, Some(19));
        assert_eq!(diagnostics.selector_error, Some(25));
        assert_eq!(diagnostics.stages.len(), 2);
        assert!(diagnostics.stages[1].contains("25"));
    }

    #[test]
    fn failure_diagnostics_keep_only_bounded_suites_and_seh_fields() {
        let message = concat!(
            r#"failed: diagnostics={"classification":"crashed","missing_suites":[{"name":"PF World Suite","version":2},{"name":"PF World Suite","version":2},{"name":"C:\\private\\suite","version":1},{"name":"Bad","version":-1}]}, report="#,
            r#"{"last_seh_selector":"SMART_RENDER_GPU","last_seh_error":512,"last_seh_exception_code":3221225477}"#,
        );
        let diagnostics = failure_diagnostics(message).unwrap();
        assert_eq!(
            diagnostics.missing_suites,
            vec![MissingSuite {
                name: "PF World Suite".into(),
                version: 2
            }]
        );
        assert_eq!(
            diagnostics.last_seh_selector.as_deref(),
            Some("SMART_RENDER_GPU")
        );
        assert_eq!(diagnostics.last_seh_error, Some(512));
        assert_eq!(diagnostics.last_seh_exception_code, Some(0xC0000005));
    }

    #[test]
    fn failure_diagnostics_reject_unbounded_seh_and_suite_values() {
        let message = format!(
            "failed: diagnostics={{\"classification\":\"crashed\",\"missing_suites\":[{{\"name\":\"{}\",\"version\":1}}]}}, report={{\"last_seh_selector\":\"{}\",\"last_seh_error\":4294967296,\"last_seh_exception_code\":4294967296}}",
            "A".repeat(97),
            "A".repeat(33),
        );
        let diagnostics = failure_diagnostics(&message).unwrap();
        assert!(diagnostics.missing_suites.is_empty());
        assert_eq!(diagnostics.last_seh_selector, None);
        assert_eq!(diagnostics.last_seh_error, None);
        assert_eq!(diagnostics.last_seh_exception_code, None);
    }

    #[test]
    fn malformed_failure_diagnostics_do_not_escape_the_ui_boundary() {
        assert!(failure_diagnostics("worker report unavailable: not-json").is_none());
        assert!(failure_diagnostics("unrelated error").is_none());
    }

    #[test]
    fn invalid_smartfx_rect_is_reported_without_copying_the_full_worker_report() {
        let message = concat!(
            r#"failed: diagnostics={"classification":"nonzero_exit","failure_stage":null}, report="#,
            r#"{"result_rects_valid":false,"width":0,"height":0,"large":"payload"}"#,
        );
        let diagnostics = failure_diagnostics(message).unwrap();
        assert_eq!(
            diagnostics.failure_stage.as_deref(),
            Some("result_rect_validation")
        );
        assert_eq!(
            matrix_error_summary(message),
            "SmartFX did not return a valid result rectangle"
        );
    }

    #[test]
    fn unsupported_depth_is_not_misreported_as_a_selector_or_rect_failure() {
        let message = concat!(
            r#"failed: diagnostics={"classification":"nonzero_exit","failure_stage":"render"}, report="#,
            r#"{"depth_supported":false,"smart_render_error":-1,"result_rects_valid":false}"#,
        );
        let diagnostics = failure_diagnostics(message).unwrap();
        assert_eq!(diagnostics.classification, "unsupported_pixel_depth");
        assert_eq!(
            diagnostics.failure_stage.as_deref(),
            Some("pixel_depth_negotiation")
        );
        assert_eq!(diagnostics.selector_error, None);
        assert_eq!(
            matrix_error_summary(message),
            "AEX did not advertise support for the requested pixel depth"
        );
    }

    #[test]
    fn unsupported_smart_render_path_precedes_depth_and_selector_failures() {
        let message = concat!(
            r#"failed: diagnostics={"classification":"nonzero_exit","failure_stage":"smart_render"}, report="#,
            r#"{"smart_render_supported":false,"depth_supported":false,"smart_render_error":-1,"result_rects_valid":false}"#,
        );
        let diagnostics = failure_diagnostics(message).unwrap();
        assert_eq!(diagnostics.classification, "unsupported_render_path");
        assert_eq!(
            diagnostics.failure_stage.as_deref(),
            Some("render_path_negotiation")
        );
        assert_eq!(diagnostics.selector_error, None);
        assert_eq!(
            matrix_error_summary(message),
            "AEX did not advertise SmartFX render support"
        );
    }

    #[test]
    fn matrix_rows_preserve_success_and_failure_diagnostics() {
        let report = serde_json::json!({
            "stage": "effect_compatibility_matrix",
            "cases": [
                {"render_path":"classic","pixel_format":"argb8","passed":true,
                 "classification":"ok","output_png":"classic.png",
                 "output_relation":"pixels_changed","differing_input_pixels":42},
                {"render_path":"smartfx","pixel_format":"argb32f","passed":false,
                 "classification":"crashed","failure_stage":"smart_render_gpu",
                 "selector_error":512,"error":"GPU selector crashed"},
                {"render_path":"classic","pixel_format":"argb16","passed":false,
                 "applicable":false,"classification":"unsupported_pixel_depth",
                 "failure_stage":"pixel_depth_negotiation"}
            ]
        });
        let cases = compatibility_matrix(&report).unwrap();
        assert_eq!(cases.len(), 3);
        assert!(cases[0].passed);
        assert!(cases[0].applicable);
        assert_eq!(cases[0].output_png.as_deref(), Some("classic.png"));
        assert_eq!(cases[0].output_relation.as_deref(), Some("pixels_changed"));
        assert_eq!(cases[0].differing_input_pixels, Some(42));
        assert!(!cases[1].passed);
        assert_eq!(cases[1].classification, "crashed");
        assert_eq!(cases[1].failure_stage.as_deref(), Some("smart_render_gpu"));
        assert_eq!(cases[1].selector_error, Some(512));
        assert_eq!(cases[1].error.as_deref(), Some("GPU selector crashed"));
        assert!(!cases[2].passed);
        assert!(!cases[2].applicable);
        assert_eq!(cases[2].classification, "unsupported_pixel_depth");
    }

    #[test]
    fn rebuilt_dev_binary_is_rehashed_and_automatically_enabled() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-reload-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("reload.aex");
        let mut first_build = fs::read(std::env::current_exe().unwrap()).unwrap();
        fs::write(&path, &first_build).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let first_hash = format!("{:X}", Sha256::digest(&first_build));
        let mut app = HarnessApp::new(std::env::temp_dir());
        app.selection = Some(Selection {
            path: path.clone(),
            size: metadata.len(),
            sha256: first_hash.clone(),
            profile: None,
            modified: metadata.modified().ok(),
        });
        app.session_approved = true;
        app.trust_rebuilds = true;
        app.last_identity_check = Instant::now() - Duration::from_secs(1);

        first_build.extend_from_slice(b"second build");
        fs::write(&path, &first_build).unwrap();
        app.check_selected_identity();
        assert!(app.selection_stale);

        app.refresh_aex();
        let refreshed = app.selection.as_ref().unwrap();
        assert_ne!(refreshed.sha256, first_hash);
        assert!(!app.selection_stale);
        assert!(app.session_approved, "{} / {}", app.status, app.report);
        assert!(app.inspect_after_refresh);
        assert!(app.parameters.is_empty());
        assert!(app.preview.is_none());

        app.trust_rebuilds = false;
        first_build.extend_from_slice(b"third build");
        fs::write(&path, &first_build).unwrap();
        app.refresh_aex();
        assert!(app.session_approved);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adjacent_import_discovery_accepts_a_valid_pe_and_rejects_malformed_input() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-import-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let valid = root.join("valid.aex");
        fs::copy(std::env::current_exe().unwrap(), &valid).unwrap();
        let discovery = discover_adjacent_imports(&valid).unwrap();
        assert!(discovery.dependencies.is_empty());

        let malformed = root.join("malformed.aex");
        fs::write(&malformed, b"not a PE image").unwrap();
        assert!(discover_adjacent_imports(&malformed)
            .unwrap_err()
            .contains("Could not inspect PE imports"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn delay_import_table_is_bounded_terminated_and_basename_only() {
        let mut bytes = vec![0u8; 96];
        bytes[0..4].copy_from_slice(&1u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[64..75].copy_from_slice(b"helper.dll\0");
        assert_eq!(
            parse_delay_import_table(&bytes, 0, 64, 0x400000, true, |rva| {
                (rva == 0x1000).then_some(64)
            })
            .unwrap(),
            ["helper.dll"]
        );

        assert!(
            parse_delay_import_table(&bytes, 0, 63, 0x400000, true, |_| Some(64))
                .unwrap_err()
                .contains("invalid size")
        );
        assert!(
            parse_delay_import_table(&bytes, 0, 32, 0x400000, true, |_| Some(64))
                .unwrap_err()
                .contains("zero terminator")
        );

        bytes[64..75].copy_from_slice(b"..\\bad.dll\0");
        assert!(
            parse_delay_import_table(&bytes, 0, 64, 0x400000, true, |_| Some(64))
                .unwrap_err()
                .contains("DLL basename")
        );
    }

    #[test]
    fn automatic_dependency_discovery_excludes_system_names_and_oversized_pe_files() {
        assert!(is_system_import_name("api-ms-win-core-file-l1-1-0.dll"));
        if std::env::var_os("WINDIR").is_some() {
            assert!(is_system_import_name("kernel32.dll"));
        }

        let path = temporary_aex("oversized");
        fs::File::create(&path)
            .unwrap()
            .set_len(MAX_DISCOVERY_FILE_BYTES + 1)
            .unwrap();
        assert!(read_bounded_pe(&path)
            .unwrap_err()
            .contains("PE image size"));
        fs::remove_file(path).unwrap();
    }
}
