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
const CLI_CONTRACT_SCHEMA: &str = "aexcompat.harness-cli-contract";
const CLI_CONTRACT_VERSION: u64 = 1;

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

fn approved_adjacent_dependencies(
    aex_path: &Path,
    approved_sha256: &str,
) -> Result<Vec<aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact>, String> {
    let aex_path = aex_path
        .canonicalize()
        .map_err(|error| format!("selected AEX could not be resolved: {error}"))?;
    let discovery = discover_adjacent_imports(&aex_path)?;
    let main = aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact {
        path: aex_path.clone(),
        expected_sha256: decode_sha256(approved_sha256)?,
        expected_size: fs::metadata(aex_path)
            .map_err(|error| format!("selected AEX metadata failed: {error}"))?
            .len(),
    };
    let dependencies = discovery
        .dependencies
        .into_iter()
        .map(|dependency| {
            let basename = dependency
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
                .ok_or_else(|| "adjacent dependency basename is not Unicode".to_owned())?;
            Ok(
                aexcompat_broker::session_dependency_manifest::SessionDependencyDto {
                    path: dependency.path,
                    basename,
                    sha256: dependency.sha256,
                    size: dependency.size,
                },
            )
        })
        .collect::<Result<Vec<_>, String>>()?;
    aexcompat_broker::session_dependency_manifest::validate(
        aexcompat_broker::session_dependency_manifest::SessionDependencyManifestDto {
            schema_version: 1,
            dependencies,
        },
        &main,
    )
    .map(|manifest| manifest.into_approved_image_artifacts())
    .map_err(|error| format!("adjacent dependency manifest rejected: {error}"))
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
