use crate::gpu_platform_collector::{
    collect_gpu_platform_identity, enumerate_gpu_adapters, privacy_bounded_identity_report,
};
use crate::runtime_module_policy::RuntimeBackend;
use crate::secure_image_dispatch::{
    ApprovedImageArtifact, SecureImageDispatch, WorkerKind,
    dispatch_secure_image_with_process_memory_limit,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

pub const WGPU_VERSION: &str = "0.19.4";
pub const ELEMENT_COUNT: usize = 64;
const SETUP_MARKER: &str = "AEXCOMPAT_WGPU_DX12_SETUP=";
const SETDOWN_MARKER: &str = "AEXCOMPAT_WGPU_DX12_SETDOWN=";
const AEX_BASENAME: &str = "pf_wgpu_dx12_probe.aex";
const RUNTIME_BASENAME: &str = "aexcompat_wgpu_dx12_runtime.dll";
const PROBE_PROCESS_MEMORY_LIMIT: usize = 1024 * 1024 * 1024;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactIdentity {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactSet {
    aex: ArtifactIdentity,
    runtime: ArtifactIdentity,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceSet {
    aex_cpp: ArtifactIdentity,
    runtime_cargo_toml: ArtifactIdentity,
    runtime_rust: ArtifactIdentity,
    cargo_lock: ArtifactIdentity,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildManifest {
    schema_version: u32,
    probe: String,
    backend: String,
    wgpu_version: String,
    configuration: String,
    cargo_locked: bool,
    artifacts: ArtifactSet,
    sources: SourceSet,
    build_commands: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterObservation {
    pub name: String,
    pub adapter_luid: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_type: String,
    pub driver: String,
    pub driver_info: String,
    pub backend: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SetupObservation {
    pub schema_version: u32,
    pub stage: String,
    pub backend: String,
    pub wgpu_version: String,
    pub wgsl_sha256: String,
    pub element_count: usize,
    pub expected_values: Vec<u32>,
    pub actual_values: Vec<u32>,
    pub expected_sha256: String,
    pub actual_sha256: String,
    pub adapter: Option<AdapterObservation>,
    pub wgpu_compute_ready: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SetdownObservation {
    pub schema_version: u32,
    pub stage: String,
    pub cleanup_complete: bool,
    pub live_state_after_setdown: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModuleAuditEvidence {
    pub status: String,
    pub phase_count: u64,
    pub unknown_count: u64,
    pub plugin_modules: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LifecycleEvidence {
    pub status: String,
    pub global_setup_error: i64,
    pub params_setup_error: i64,
    pub global_setdown_error: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct WgpuDx12PfProbeReport {
    pub schema_version: u32,
    pub probe: &'static str,
    pub backend: &'static str,
    pub wgpu_version: &'static str,
    pub build_manifest_sha256: String,
    pub artifacts: Value,
    pub sources: Value,
    pub driver_identity: Value,
    pub worker_exit: String,
    pub worker_exit_code: u32,
    pub process_memory_limit_bytes: u64,
    pub worker_peak_commit_bytes: Option<u64>,
    pub memory_limit_reached: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub lifecycle: LifecycleEvidence,
    pub module_audit: ModuleAuditEvidence,
    pub global_setup: SetupObservation,
    pub global_setdown: SetdownObservation,
    pub wgpu_compute_ready: bool,
    pub backend_ready: bool,
    pub passed: bool,
}

impl SetupObservation {
    fn validate(&self) -> io::Result<()> {
        let expected = (0..ELEMENT_COUNT as u32)
            .map(|value| value * 3 + 7)
            .collect::<Vec<_>>();
        let expected_hash = values_sha256(&expected);
        let ready = self.schema_version == 1
            && self.stage == "compute_readback_complete"
            && self.backend == "dx12"
            && self.wgpu_version == WGPU_VERSION
            && self.element_count == ELEMENT_COUNT
            && self.expected_values == expected
            && self.actual_values == expected
            && self.expected_sha256 == expected_hash
            && self.actual_sha256 == expected_hash
            && valid_sha256(&self.wgsl_sha256)
            && self.adapter.as_ref().is_some_and(|adapter| {
                adapter.backend == "dx12"
                    && adapter.adapter_luid.len() == 16
                    && adapter
                        .adapter_luid
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            })
            && self.error.is_none();
        if self.wgpu_compute_ready != ready {
            return Err(invalid(
                "wgpu_compute_ready does not match the fixed DX12 readback contract",
            ));
        }
        Ok(())
    }
}

impl SetdownObservation {
    fn validate(&self) -> io::Result<()> {
        if self.schema_version != 1
            || self.stage != "global_setdown"
            || !self.cleanup_complete
            || self.live_state_after_setdown
            || self.error.is_some()
        {
            return Err(invalid("wgpu global setdown cleanup did not complete"));
        }
        Ok(())
    }
}

pub fn run_from_manifest(
    repository: &Path,
    manifest_path: &Path,
) -> io::Result<WgpuDx12PfProbeReport> {
    let manifest_bytes = fs::read(manifest_path)?;
    let manifest: BuildManifest = parse_no_duplicate_json(&manifest_bytes)?;
    validate_manifest(&manifest)?;

    let aex = validate_artifact(repository, &manifest.artifacts.aex, AEX_BASENAME)?;
    let runtime = validate_artifact(repository, &manifest.artifacts.runtime, RUNTIME_BASENAME)?;
    validate_source(
        repository,
        &manifest.sources.aex_cpp,
        "instruments/pf-wgpu-dx12-probe/pf_wgpu_dx12_probe.cpp",
    )?;
    validate_source(
        repository,
        &manifest.sources.runtime_cargo_toml,
        "instruments/pf-wgpu-dx12-probe/runtime/Cargo.toml",
    )?;
    validate_source(
        repository,
        &manifest.sources.runtime_rust,
        "instruments/pf-wgpu-dx12-probe/runtime/src/lib.rs",
    )?;
    validate_source(
        repository,
        &manifest.sources.cargo_lock,
        "instruments/pf-wgpu-dx12-probe/runtime/Cargo.lock",
    )?;

    let mut search_dirs: Vec<PathBuf> = Vec::new();
    for path in [aex.as_path(), runtime.as_path()] {
        if let Some(parent) = path.parent() {
            if !search_dirs.iter().any(|seen| seen == parent) {
                search_dirs.push(parent.to_path_buf());
            }
        }
    }
    let args_before_plugin = vec!["--l2-params-only".to_owned()];
    let args_after_plugin = vec![manifest.artifacts.aex.sha256.clone()];
    let launch = dispatch_secure_image_with_process_memory_limit(
        SecureImageDispatch {
            repository,
            worker_kind: WorkerKind::Discovery,
            plugin: approved(aex, &manifest.artifacts.aex)?,
            // #816: the runtime rides a search directory rather than a staged
            // artifact, and dispatch rejects the staged form outright.
            dependencies: Vec::new(),
            dependency_search_dirs: search_dirs,
            args_before_plugin: &args_before_plugin,
            args_after_plugin: &args_after_plugin,
            timeout: None,
            launch_environment: Default::default(),
        },
        PROBE_PROCESS_MEMORY_LIMIT,
    )?;
    if launch.stdout_truncated || launch.stderr_truncated {
        return Err(invalid("wgpu PF probe worker output was truncated"));
    }
    if launch.classification != crate::ExitClassification::Ok {
        return Err(invalid(format!(
            "wgpu PF probe worker exited as {}",
            launch.classification.as_str()
        )));
    }

    let worker_report = crate::worker_module_audit::parse_report_prefix(&launch.stdout)?;
    let lifecycle = lifecycle_evidence(&worker_report)?;
    let module_audit = module_audit_evidence(&worker_report)?;
    let setup: SetupObservation = parse_marker(&launch.stderr, SETUP_MARKER)?;
    let setdown: SetdownObservation = parse_marker(&launch.stderr, SETDOWN_MARKER)?;
    setup.validate()?;
    if !setup.wgpu_compute_ready {
        return Err(invalid(format!(
            "wgpu DX12 compute did not become ready at stage {}: {}",
            setup.stage,
            setup.error.as_deref().unwrap_or("unspecified failure")
        )));
    }
    setdown.validate()?;

    let adapter = setup
        .adapter
        .as_ref()
        .ok_or_else(|| invalid("wgpu DX12 setup did not identify its adapter"))?;
    let selected_luid = u64::from_str_radix(&adapter.adapter_luid, 16)
        .map_err(|_| invalid("wgpu DX12 adapter LUID is invalid"))?;
    let matching = enumerate_gpu_adapters()?
        .into_iter()
        .filter(|candidate| {
            candidate.adapter_luid == selected_luid
                && u32::from(candidate.pci_vendor_id) == adapter.vendor_id
                && u32::from(candidate.pci_device_id) == adapter.device_id
        })
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        let luids = matching
            .iter()
            .map(|candidate| format!("{:016x}", candidate.adapter_luid))
            .collect::<Vec<_>>()
            .join(",");
        return Err(invalid(format!(
            "wgpu adapter {:04x}:{:04x} matched {} trusted DXGI adapter identities ({luids})",
            adapter.vendor_id,
            adapter.device_id,
            matching.len()
        )));
    }
    let identity =
        collect_gpu_platform_identity(matching[0].adapter_luid, RuntimeBackend::Directx)?;
    let driver_identity = privacy_bounded_identity_report(&identity);

    let plugin_modules = module_audit
        .plugin_modules
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if !plugin_modules.iter().any(|name| name == AEX_BASENAME)
        || !plugin_modules.iter().any(|name| name == RUNTIME_BASENAME)
    {
        return Err(invalid(
            "sealed module audit did not observe both probe artifacts",
        ));
    }

    let wgpu_compute_ready = setup.wgpu_compute_ready;
    let passed = wgpu_compute_ready
        && setdown.cleanup_complete
        && lifecycle.global_setup_error == 0
        && lifecycle.params_setup_error == 0
        && lifecycle.global_setdown_error == 0
        && module_audit.status == "passed"
        && module_audit.unknown_count == 0;

    Ok(WgpuDx12PfProbeReport {
        schema_version: 1,
        probe: "aexcompat.pf-wgpu-dx12-compute",
        backend: "dx12",
        wgpu_version: WGPU_VERSION,
        build_manifest_sha256: format!("{:x}", Sha256::digest(&manifest_bytes)),
        artifacts: serde_json::to_value(&manifest.artifacts)
            .map_err(|error| invalid(error.to_string()))?,
        sources: serde_json::to_value(&manifest.sources)
            .map_err(|error| invalid(error.to_string()))?,
        driver_identity,
        worker_exit: launch.classification.as_str().to_owned(),
        worker_exit_code: launch.exit_code,
        process_memory_limit_bytes: launch.process_memory_limit_bytes,
        worker_peak_commit_bytes: launch.worker_peak_commit_bytes,
        memory_limit_reached: launch.memory_limit_reached,
        stdout_truncated: launch.stdout_truncated,
        stderr_truncated: launch.stderr_truncated,
        lifecycle,
        module_audit,
        global_setup: setup,
        global_setdown: setdown,
        wgpu_compute_ready,
        backend_ready: false,
        passed,
    })
}

pub fn write_report_create_new(
    repository: &Path,
    output: &Path,
    report: &WgpuDx12PfProbeReport,
) -> io::Result<()> {
    let output = prepare_report_output(repository, output)?;
    write_create_new_and_cleanup(&output, |file| {
        serde_json::to_writer_pretty(&mut *file, report)
            .map_err(|error| invalid(error.to_string()))?;
        file.write_all(b"\n")
    })
}

fn write_create_new_and_cleanup(
    output: &Path,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    if let Err(write_error) = write(&mut file) {
        drop(file);
        return match fs::remove_file(output) {
            Ok(()) => Err(write_error),
            Err(cleanup_error) => Err(io::Error::new(
                write_error.kind(),
                format!(
                    "probe report write failed: {write_error}; partial report cleanup failed: {cleanup_error}"
                ),
            )),
        };
    }
    Ok(())
}

fn prepare_report_output(repository: &Path, output: &Path) -> io::Result<PathBuf> {
    let repository = repository.canonicalize()?;
    let target = repository.join("target");
    fs::create_dir_all(&target)?;
    let target = target.canonicalize()?;
    let candidate = if output.is_absolute() {
        output.to_path_buf()
    } else {
        repository.join(output)
    };
    let candidate = canonicalize_with_missing_tail(&candidate)?;
    let relative = candidate
        .strip_prefix(&target)
        .map_err(|_| invalid("probe report output must stay below target"))?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid("probe report output must stay below target"));
    }
    let output = target.join(relative);
    let parent = output
        .parent()
        .ok_or_else(|| invalid("probe report output has no parent"))?;

    let mut existing = parent;
    while !existing.exists() {
        existing = existing
            .parent()
            .ok_or_else(|| invalid("probe report output has no existing ancestor"))?;
    }
    if !existing.canonicalize()?.starts_with(&target) {
        return Err(invalid("probe report output must stay below target"));
    }
    fs::create_dir_all(parent)?;
    if !parent.canonicalize()?.starts_with(&target) {
        return Err(invalid("probe report output must stay below target"));
    }
    Ok(output)
}

fn canonicalize_with_missing_tail(path: &Path) -> io::Result<PathBuf> {
    let mut existing = path;
    let mut missing = Vec::<OsString>::new();
    while !existing.exists() {
        let name = existing
            .file_name()
            .ok_or_else(|| invalid("probe report output has no existing ancestor"))?;
        missing.push(name.to_os_string());
        existing = existing
            .parent()
            .ok_or_else(|| invalid("probe report output has no existing ancestor"))?;
    }
    let mut canonical = existing.canonicalize()?;
    for name in missing.iter().rev() {
        canonical.push(name);
    }
    Ok(canonical)
}

fn validate_manifest(manifest: &BuildManifest) -> io::Result<()> {
    if manifest.schema_version != 1
        || manifest.probe != "aexcompat.pf-wgpu-dx12-compute"
        || manifest.backend != "dx12"
        || manifest.wgpu_version != WGPU_VERSION
        || manifest.configuration != "Release"
        || !manifest.cargo_locked
        || manifest.build_commands.len() != 3
        || manifest
            .build_commands
            .iter()
            .any(|command| command.is_empty() || command.contains('\n'))
    {
        return Err(invalid("wgpu DX12 build manifest contract is invalid"));
    }
    Ok(())
}

fn validate_artifact(
    repository: &Path,
    identity: &ArtifactIdentity,
    basename: &str,
) -> io::Result<PathBuf> {
    let path = resolve_relative(repository, &identity.path)?;
    if path.file_name().and_then(|name| name.to_str()) != Some(basename) {
        return Err(invalid("probe artifact basename is invalid"));
    }
    validate_file_identity(&path, identity)?;
    Ok(path)
}

fn validate_source(
    repository: &Path,
    identity: &ArtifactIdentity,
    expected_path: &str,
) -> io::Result<()> {
    if identity.path != expected_path {
        return Err(invalid("probe source path is invalid"));
    }
    validate_file_identity(&resolve_relative(repository, &identity.path)?, identity)
}

fn resolve_relative(repository: &Path, value: &str) -> io::Result<PathBuf> {
    let relative = Path::new(value);
    if value.contains('\\')
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid("probe manifest path is not a safe relative path"));
    }
    Ok(repository.join(relative))
}

fn validate_file_identity(path: &Path, identity: &ArtifactIdentity) -> io::Result<()> {
    if !valid_sha256(&identity.sha256) || identity.size == 0 {
        return Err(invalid("probe artifact identity is malformed"));
    }
    let bytes = fs::read(path)
        .map_err(|error| invalid(format!("probe artifact is missing or unreadable: {error}")))?;
    if bytes.len() as u64 != identity.size
        || format!("{:x}", Sha256::digest(&bytes)) != identity.sha256
    {
        return Err(invalid(
            "probe artifact identity changed after Release build",
        ));
    }
    Ok(())
}

fn approved(path: PathBuf, identity: &ArtifactIdentity) -> io::Result<ApprovedImageArtifact> {
    Ok(ApprovedImageArtifact {
        path,
        expected_sha256: decode_sha256(&identity.sha256)?,
        expected_size: identity.size,
    })
}

fn decode_sha256(value: &str) -> io::Result<[u8; 32]> {
    if !valid_sha256(value) {
        return Err(invalid("SHA-256 is invalid"));
    }
    let mut output = [0_u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        output[index] = u8::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16)
            .map_err(|_| invalid("SHA-256 is invalid"))?;
    }
    Ok(output)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn values_sha256(values: &[u32]) -> String {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    format!("{:x}", Sha256::digest(bytes))
}

fn parse_marker<T: DeserializeOwned>(stderr: &str, marker: &str) -> io::Result<T> {
    let values = stderr
        .lines()
        .filter_map(|line| line.trim().strip_prefix(marker))
        .collect::<Vec<_>>();
    if values.len() != 1 {
        return Err(invalid(format!(
            "expected exactly one {} diagnostic",
            marker.trim_end_matches('=')
        )));
    }
    parse_no_duplicate_json(values[0].as_bytes())
}

fn lifecycle_evidence(report: &Value) -> io::Result<LifecycleEvidence> {
    let read = |name: &str| {
        report
            .get(name)
            .and_then(Value::as_i64)
            .ok_or_else(|| invalid(format!("worker report lacks {name}")))
    };
    Ok(LifecycleEvidence {
        status: report
            .get("status")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("worker report lacks status"))?
            .to_owned(),
        global_setup_error: read("global_setup_error")?,
        params_setup_error: read("params_setup_error")?,
        global_setdown_error: read("global_setdown_error")?,
    })
}

fn module_audit_evidence(report: &Value) -> io::Result<ModuleAuditEvidence> {
    let audit = report
        .get("module_audit")
        .ok_or_else(|| invalid("worker report lacks module audit"))?;
    let read = |name: &str| {
        audit
            .get(name)
            .and_then(Value::as_u64)
            .ok_or_else(|| invalid(format!("module audit lacks {name}")))
    };
    let plugin_modules = audit
        .get("observed_union")
        .and_then(|value| value.get("plugin"))
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("module audit lacks observed plugin union"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid("module audit plugin entry is invalid"))
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(ModuleAuditEvidence {
        status: audit
            .get("status")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("module audit lacks status"))?
            .to_owned(),
        phase_count: read("phase_count")?,
        unknown_count: read("unknown_count")?,
        plugin_modules,
    })
}

fn parse_no_duplicate_json<T: DeserializeOwned>(bytes: &[u8]) -> io::Result<T> {
    let value = serde_json::from_slice::<NoDuplicateValue>(bytes)
        .map_err(|error| invalid(format!("JSON is invalid: {error}")))?
        .0;
    serde_json::from_value(value)
        .map_err(|error| invalid(format!("JSON schema is invalid: {error}")))
}

struct NoDuplicateValue(Value);

impl<'de> Deserialize<'de> for NoDuplicateValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(NoDuplicateVisitor)
    }
}

struct NoDuplicateVisitor;

impl<'de> serde::de::Visitor<'de> for NoDuplicateVisitor {
    type Value = NoDuplicateValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(json!(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(json!(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .map(NoDuplicateValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::Null))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<NoDuplicateValue>()? {
            values.push(value.0);
        }
        Ok(NoDuplicateValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate object key: {key}"
                )));
            }
            let value = object.next_value::<NoDuplicateValue>()?;
            values.insert(key, value.0);
        }
        Ok(NoDuplicateValue(Value::Object(values)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "aexcompat-wgpu-dx12-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn identity(path: &str, bytes: &[u8]) -> ArtifactIdentity {
        ArtifactIdentity {
            path: path.to_owned(),
            size: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }
    }

    #[test]
    fn missing_and_tampered_artifacts_fail_closed() {
        let root = fixture_root();
        let relative = "target/pf-wgpu-dx12-probe-build/Release/pf_wgpu_dx12_probe.aex";
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bytes = b"sealed fixture";
        fs::write(&path, bytes).unwrap();
        let approved = identity(relative, bytes);
        validate_artifact(&root, &approved, AEX_BASENAME).unwrap();
        let mut uppercase = approved.clone();
        uppercase.sha256.make_ascii_uppercase();
        assert!(validate_artifact(&root, &uppercase, AEX_BASENAME).is_err());
        fs::write(&path, b"tampered fixture").unwrap();
        assert!(validate_artifact(&root, &approved, AEX_BASENAME).is_err());
        fs::remove_file(&path).unwrap();
        assert!(validate_artifact(&root, &approved, AEX_BASENAME).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_json_keys_are_rejected() {
        let bytes = br#"{"path":"a","path":"b","size":1,"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;
        assert!(parse_no_duplicate_json::<ArtifactIdentity>(bytes).is_err());
    }

    #[test]
    fn marker_requires_one_closed_schema_document() {
        let marker = format!(
            "{SETDOWN_MARKER}{}",
            r#"{"schema_version":1,"stage":"global_setdown","cleanup_complete":true,"live_state_after_setdown":false,"error":null}"#
        );
        let parsed: SetdownObservation = parse_marker(&marker, SETDOWN_MARKER).unwrap();
        parsed.validate().unwrap();
        assert!(parse_marker::<SetdownObservation>("", SETDOWN_MARKER).is_err());
        assert!(
            parse_marker::<SetdownObservation>(&format!("{marker}\n{marker}"), SETDOWN_MARKER)
                .is_err()
        );
    }

    #[test]
    fn readiness_requires_exact_64_element_readback() {
        let values = (0..ELEMENT_COUNT as u32)
            .map(|value| value * 3 + 7)
            .collect::<Vec<_>>();
        let hash = values_sha256(&values);
        let mut observation = SetupObservation {
            schema_version: 1,
            stage: "compute_readback_complete".to_owned(),
            backend: "dx12".to_owned(),
            wgpu_version: WGPU_VERSION.to_owned(),
            wgsl_sha256: "a".repeat(64),
            element_count: ELEMENT_COUNT,
            expected_values: values.clone(),
            actual_values: values,
            expected_sha256: hash.clone(),
            actual_sha256: hash,
            adapter: Some(AdapterObservation {
                name: "fixture".to_owned(),
                adapter_luid: "0000000000000001".to_owned(),
                vendor_id: 0x10de,
                device_id: 0x2782,
                device_type: "discretegpu".to_owned(),
                driver: "fixture".to_owned(),
                driver_info: "fixture".to_owned(),
                backend: "dx12".to_owned(),
            }),
            wgpu_compute_ready: true,
            error: None,
        };
        observation.validate().unwrap();
        observation.actual_values[0] ^= 1;
        assert!(observation.validate().is_err());
    }

    #[test]
    fn output_containment_is_checked_before_creating_directories() {
        let root = fixture_root();
        let absolute_output = root.join("target").join("absolute").join("report.json");
        let prepared = prepare_report_output(&root, &absolute_output).unwrap();
        assert_eq!(
            prepared,
            root.canonicalize()
                .unwrap()
                .join("target")
                .join("absolute")
                .join("report.json")
        );

        let outside = root.with_extension("outside");
        let outside_output = outside.join("nested").join("report.json");
        assert!(prepare_report_output(&root, &outside_output).is_err());
        assert!(!outside.exists());

        let traversal = Path::new("target/../escape/report.json");
        assert!(prepare_report_output(&root, traversal).is_err());
        assert!(!root.join("escape").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_report_write_removes_partial_output_and_allows_retry() {
        let root = fixture_root();
        let output = prepare_report_output(&root, Path::new("target/report.json")).unwrap();
        let error = write_create_new_and_cleanup(&output, |file| {
            file.write_all(b"{\"partial\":")?;
            Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "injected trailing write failure",
            ))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WriteZero);
        assert!(!output.exists());

        write_create_new_and_cleanup(&output, |file| file.write_all(b"{}\n")).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"{}\n");
        fs::remove_dir_all(root).unwrap();
    }
}
