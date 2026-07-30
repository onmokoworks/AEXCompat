use crate::sealed_load_tree::{LoadEntry, SealedLoadTree};
use crate::secure_launch::{secure_launch, SecureLaunchRequest, SecureLaunchResult};
use crate::ExitClassification;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

pub const MAX_CUDA_DEVICES: usize = 64;
pub const MAX_DEVICE_NAME_BYTES: usize = 256;
pub const MAX_JIT_LOG_BYTES: usize = 16 * 1024;
pub const MAX_WORKER_STDOUT_BYTES: usize = 1024 * 1024;
pub const COMPUTE_ELEMENT_COUNT: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CudaAggregateStatus {
    Observed,
    Failed,
    Partial,
    Incomplete,
    NoDriver,
    MissingSymbol,
    NoDevice,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CudaFailureKind {
    NoDriver,
    MissingSymbol,
    ApiError,
    NoDevice,
    DeviceLimit,
    DeviceFailure,
    Partial,
    Malformed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CudaStage {
    Passed,
    Metadata,
    Context,
    Allocation,
    HostToDevice,
    Module,
    Jit,
    Function,
    Launch,
    Synchronize,
    DeviceToHost,
    Mismatch,
    CleanupFailed,
    NotAttempted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JitLogStatus {
    Empty,
    Redacted,
    Overflow,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JitLogObservation {
    pub status: JitLogStatus,
    pub length_bytes: u32,
    pub sha256: Option<String>,
}

impl JitLogObservation {
    pub fn empty() -> Self {
        Self {
            status: JitLogStatus::Empty,
            length_bytes: 0,
            sha256: None,
        }
    }

    pub fn unavailable() -> Self {
        Self {
            status: JitLogStatus::Unavailable,
            length_bytes: 0,
            sha256: None,
        }
    }

    pub fn from_bounded_logs(info: &[u8], error: &[u8], overflow: bool) -> Self {
        let length = info.len().saturating_add(error.len());
        if overflow || length > MAX_JIT_LOG_BYTES.saturating_mul(2) {
            return Self {
                status: JitLogStatus::Overflow,
                length_bytes: u32::try_from(length).unwrap_or(u32::MAX),
                sha256: None,
            };
        }
        if length == 0 {
            return Self::empty();
        }
        let mut digest = Sha256::new();
        digest.update((info.len() as u64).to_le_bytes());
        digest.update(info);
        digest.update((error.len() as u64).to_le_bytes());
        digest.update(error);
        Self {
            status: JitLogStatus::Redacted,
            length_bytes: length as u32,
            sha256: Some(format!("{:x}", digest.finalize())),
        }
    }

    fn is_valid(&self) -> bool {
        match self.status {
            JitLogStatus::Empty | JitLogStatus::Unavailable => {
                self.length_bytes == 0 && self.sha256.is_none()
            }
            JitLogStatus::Redacted => {
                self.length_bytes > 0
                    && usize::try_from(self.length_bytes)
                        .is_ok_and(|length| length <= MAX_JIT_LOG_BYTES * 2)
                    && self.sha256.as_deref().is_some_and(valid_sha256)
            }
            JitLogStatus::Overflow => self.sha256.is_none(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PciLocation {
    pub domain: u32,
    pub bus: u32,
    pub device: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CudaCleanupTarget {
    Module,
    OutputMemory,
    InputMemory,
    Context,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CudaCleanupFailure {
    pub target: CudaCleanupTarget,
    pub operation: String,
    pub api_error: i32,
    pub symbolic_info: String,
}

impl CudaCleanupFailure {
    fn is_valid(&self) -> bool {
        self.api_error != 0
            && valid_operation_name(&self.operation)
            && valid_symbolic_error(&self.symbolic_info)
            && match self.target {
                CudaCleanupTarget::Module => self.operation == "cuModuleUnload",
                CudaCleanupTarget::OutputMemory => self.operation == "cuMemFree_v2.output",
                CudaCleanupTarget::InputMemory => self.operation == "cuMemFree_v2.input",
                CudaCleanupTarget::Context => self.operation == "cuCtxDestroy_v2",
            }
    }
}

impl PciLocation {
    fn is_valid(&self) -> bool {
        self.domain <= 0xffff && self.bus <= 0xff && self.device <= 0x1f
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CudaDeviceObservation {
    pub ordinal: u32,
    pub name: Option<String>,
    pub uuid_fingerprint_sha256: Option<String>,
    pub pci_location: Option<PciLocation>,
    pub compute_capability_major: Option<u32>,
    pub compute_capability_minor: Option<u32>,
    pub total_memory_bytes: Option<u64>,
    pub stage: CudaStage,
    pub operation: Option<String>,
    pub api_error: Option<i32>,
    pub jit_log: Option<JitLogObservation>,
    pub cleanup_failures: Vec<CudaCleanupFailure>,
}

impl CudaDeviceObservation {
    pub fn passed(
        ordinal: u32,
        name: String,
        uuid_fingerprint_sha256: String,
        pci_location: PciLocation,
        compute_capability_major: u32,
        compute_capability_minor: u32,
        total_memory_bytes: u64,
    ) -> Self {
        Self {
            ordinal,
            name: Some(name),
            uuid_fingerprint_sha256: Some(uuid_fingerprint_sha256),
            pci_location: Some(pci_location),
            compute_capability_major: Some(compute_capability_major),
            compute_capability_minor: Some(compute_capability_minor),
            total_memory_bytes: Some(total_memory_bytes),
            stage: CudaStage::Passed,
            operation: None,
            api_error: None,
            jit_log: None,
            cleanup_failures: Vec::new(),
        }
    }

    pub fn metadata_failure(
        ordinal: u32,
        operation: impl Into<String>,
        api_error: Option<i32>,
    ) -> Self {
        Self {
            ordinal,
            name: None,
            uuid_fingerprint_sha256: None,
            pci_location: None,
            compute_capability_major: None,
            compute_capability_minor: None,
            total_memory_bytes: None,
            stage: CudaStage::Metadata,
            operation: Some(operation.into()),
            api_error,
            jit_log: None,
            cleanup_failures: Vec::new(),
        }
    }

    pub fn with_compute_failure(
        mut metadata: Self,
        stage: CudaStage,
        operation: impl Into<String>,
        api_error: Option<i32>,
        jit_log: Option<JitLogObservation>,
    ) -> Self {
        metadata.stage = stage;
        metadata.operation = Some(operation.into());
        metadata.api_error = api_error;
        metadata.jit_log = jit_log;
        metadata.cleanup_failures.clear();
        metadata
    }

    pub fn with_cleanup_failures(
        mut observation: Self,
        cleanup_failures: Vec<CudaCleanupFailure>,
    ) -> Self {
        if let Some(first) = cleanup_failures.first() {
            observation.stage = CudaStage::CleanupFailed;
            observation.operation = Some(first.operation.clone());
            observation.api_error = Some(first.api_error);
            observation.jit_log = None;
        }
        observation.cleanup_failures = cleanup_failures;
        observation
    }

    pub fn not_attempted(ordinal: u32, operation: impl Into<String>) -> Self {
        Self {
            ordinal,
            name: None,
            uuid_fingerprint_sha256: None,
            pci_location: None,
            compute_capability_major: None,
            compute_capability_minor: None,
            total_memory_bytes: None,
            stage: CudaStage::NotAttempted,
            operation: Some(operation.into()),
            api_error: None,
            jit_log: None,
            cleanup_failures: Vec::new(),
        }
    }

    fn metadata_complete(&self) -> bool {
        self.name.as_deref().is_some_and(valid_device_name)
            && self
                .uuid_fingerprint_sha256
                .as_deref()
                .is_some_and(valid_sha256)
            && self
                .pci_location
                .as_ref()
                .is_some_and(PciLocation::is_valid)
            && self
                .compute_capability_major
                .is_some_and(|value| value > 0 && value <= 1024)
            && self
                .compute_capability_minor
                .is_some_and(|value| value <= 1024)
            && self.total_memory_bytes.is_some_and(|value| value > 0)
    }

    pub fn validate(&self) -> bool {
        let operation_valid = self.operation.as_deref().is_none_or(valid_operation_name);
        if !operation_valid || self.jit_log.as_ref().is_some_and(|log| !log.is_valid()) {
            return false;
        }
        if self.cleanup_failures.len() > 4
            || self
                .cleanup_failures
                .iter()
                .any(|failure| !failure.is_valid())
        {
            return false;
        }
        if self.stage != CudaStage::CleanupFailed && !self.cleanup_failures.is_empty() {
            return false;
        }
        match self.stage {
            CudaStage::Passed => {
                self.metadata_complete()
                    && self.operation.is_none()
                    && self.api_error.is_none()
                    && self.jit_log.is_none()
                    && self.cleanup_failures.is_empty()
            }
            CudaStage::Metadata | CudaStage::NotAttempted => {
                self.name.is_none()
                    && self.uuid_fingerprint_sha256.is_none()
                    && self.pci_location.is_none()
                    && self.compute_capability_major.is_none()
                    && self.compute_capability_minor.is_none()
                    && self.total_memory_bytes.is_none()
                    && self.operation.is_some()
                    && self.jit_log.is_none()
                    && self.cleanup_failures.is_empty()
                    && (self.stage != CudaStage::NotAttempted || self.api_error.is_none())
            }
            CudaStage::Mismatch => {
                self.metadata_complete()
                    && self.operation.is_some()
                    && self.api_error.is_none()
                    && self.jit_log.is_none()
                    && self.cleanup_failures.is_empty()
            }
            CudaStage::Jit => {
                self.metadata_complete()
                    && self.operation.is_some()
                    && self.api_error.is_some()
                    && self.jit_log.is_some()
                    && self.cleanup_failures.is_empty()
            }
            CudaStage::CleanupFailed => {
                let Some(first) = self.cleanup_failures.first() else {
                    return false;
                };
                self.metadata_complete()
                    && self.operation.as_deref() == Some(first.operation.as_str())
                    && self.api_error == Some(first.api_error)
                    && self.jit_log.is_none()
            }
            _ => {
                self.metadata_complete()
                    && self.operation.is_some()
                    && (self.api_error.is_some()
                        || self
                            .operation
                            .as_deref()
                            .is_some_and(is_null_handle_operation))
                    && self.jit_log.is_none()
                    && self.cleanup_failures.is_empty()
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CudaDiagnostic {
    pub kind: CudaFailureKind,
    pub operation: String,
    pub api_error: Option<i32>,
    pub device_ordinal: Option<u32>,
}

impl CudaDiagnostic {
    pub fn new(
        kind: CudaFailureKind,
        operation: impl Into<String>,
        api_error: Option<i32>,
    ) -> Self {
        Self {
            kind,
            operation: operation.into(),
            api_error,
            device_ordinal: None,
        }
    }

    fn is_valid(&self) -> bool {
        valid_operation_name(&self.operation)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CudaAggregateObservation {
    pub status: CudaAggregateStatus,
    pub driver_version: Option<u32>,
    pub devices: Vec<CudaDeviceObservation>,
    pub diagnostics: Vec<CudaDiagnostic>,
    pub cuda_compute_ready: bool,
}

impl CudaAggregateObservation {
    pub fn validate(&self) -> bool {
        if self.devices.len() > MAX_CUDA_DEVICES
            || self.diagnostics.len() > MAX_CUDA_DEVICES.saturating_add(16)
            || self.devices.iter().any(|device| !device.validate())
            || self.diagnostics.iter().any(|item| !item.is_valid())
            || self
                .devices
                .iter()
                .enumerate()
                .any(|(index, device)| device.ordinal != index as u32)
        {
            return false;
        }
        let expected_ready = self.status == CudaAggregateStatus::Observed
            && !self.devices.is_empty()
            && self
                .devices
                .iter()
                .all(|device| device.stage == CudaStage::Passed);
        if self.cuda_compute_ready != expected_ready {
            return false;
        }
        match self.status {
            CudaAggregateStatus::Observed => {
                self.driver_version.is_some_and(valid_driver_version)
                    && !self.devices.is_empty()
                    && self.diagnostics.is_empty()
                    && self
                        .devices
                        .iter()
                        .all(|device| device.stage == CudaStage::Passed)
            }
            CudaAggregateStatus::Failed => {
                self.driver_version.is_some_and(valid_driver_version)
                    && !self.devices.is_empty()
                    && !self.diagnostics.is_empty()
                    && self
                        .diagnostics
                        .iter()
                        .any(|item| item.kind == CudaFailureKind::DeviceFailure)
                    && self
                        .devices
                        .iter()
                        .all(|device| device.stage != CudaStage::Passed)
                    && !self.cuda_compute_ready
            }
            CudaAggregateStatus::Partial => {
                self.driver_version.is_some_and(valid_driver_version)
                    && !self.devices.is_empty()
                    && !self.diagnostics.is_empty()
                    && !self.cuda_compute_ready
                    && (self
                        .devices
                        .iter()
                        .any(|device| device.stage == CudaStage::Passed)
                        || self
                            .devices
                            .iter()
                            .any(|device| device.stage == CudaStage::NotAttempted)
                        || self
                            .diagnostics
                            .iter()
                            .any(|item| item.kind == CudaFailureKind::DeviceLimit))
            }
            CudaAggregateStatus::NoDevice => {
                self.driver_version.is_some_and(valid_driver_version)
                    && self.devices.is_empty()
                    && !self.diagnostics.is_empty()
                    && !self.cuda_compute_ready
            }
            CudaAggregateStatus::NoDriver
            | CudaAggregateStatus::MissingSymbol
            | CudaAggregateStatus::Incomplete => {
                self.devices.is_empty() && !self.diagnostics.is_empty() && !self.cuda_compute_ready
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiFailure {
    pub kind: CudaFailureKind,
    pub operation: String,
    pub api_error: Option<i32>,
}

impl ApiFailure {
    pub fn api(operation: impl Into<String>, api_error: i32) -> Self {
        Self {
            kind: CudaFailureKind::ApiError,
            operation: operation.into(),
            api_error: Some(api_error),
        }
    }

    pub fn malformed(operation: impl Into<String>) -> Self {
        Self {
            kind: CudaFailureKind::Malformed,
            operation: operation.into(),
            api_error: None,
        }
    }
}

pub trait CudaProbeApi {
    fn initialize(&self) -> Result<(), ApiFailure>;
    fn driver_version(&self) -> Result<u32, ApiFailure>;
    fn device_count(&self, limit: usize) -> Result<usize, ApiFailure>;
    fn probe_device(&self, ordinal: u32) -> CudaDeviceObservation;
}

pub fn collect_with_api(api: &impl CudaProbeApi) -> CudaAggregateObservation {
    if let Err(error) = api.initialize() {
        return aggregate_failure(CudaAggregateStatus::Incomplete, None, error);
    }
    let driver_version = match api.driver_version() {
        Ok(version) if valid_driver_version(version) => version,
        Ok(_) => {
            return aggregate_failure(
                CudaAggregateStatus::Incomplete,
                None,
                ApiFailure::malformed("cuDriverGetVersion"),
            );
        }
        Err(error) => return aggregate_failure(CudaAggregateStatus::Incomplete, None, error),
    };
    let count = match api.device_count(MAX_CUDA_DEVICES) {
        Ok(count) => count,
        Err(error) => {
            return aggregate_failure(CudaAggregateStatus::Incomplete, Some(driver_version), error);
        }
    };
    if count == 0 {
        return CudaAggregateObservation {
            status: CudaAggregateStatus::NoDevice,
            driver_version: Some(driver_version),
            devices: Vec::new(),
            diagnostics: vec![CudaDiagnostic::new(
                CudaFailureKind::NoDevice,
                "cuDeviceGetCount",
                None,
            )],
            cuda_compute_ready: false,
        };
    }
    let inspected_count = count.min(MAX_CUDA_DEVICES);
    let devices = (0..inspected_count)
        .map(|ordinal| api.probe_device(ordinal as u32))
        .collect::<Vec<_>>();
    let passed_count = devices
        .iter()
        .filter(|device| device.stage == CudaStage::Passed)
        .count();
    let has_not_attempted = devices
        .iter()
        .any(|device| device.stage == CudaStage::NotAttempted);
    let limited = count > MAX_CUDA_DEVICES;
    let (status, diagnostics) = if limited || has_not_attempted {
        (
            CudaAggregateStatus::Partial,
            vec![CudaDiagnostic::new(
                if limited {
                    CudaFailureKind::DeviceLimit
                } else {
                    CudaFailureKind::Partial
                },
                if limited {
                    "cuDeviceGetCount.uninspected_limit"
                } else {
                    "device_compute_not_attempted"
                },
                None,
            )],
        )
    } else if passed_count == devices.len() {
        (CudaAggregateStatus::Observed, Vec::new())
    } else if passed_count == 0 {
        (
            CudaAggregateStatus::Failed,
            vec![CudaDiagnostic::new(
                CudaFailureKind::DeviceFailure,
                "device_compute_failed",
                None,
            )],
        )
    } else {
        (
            CudaAggregateStatus::Partial,
            vec![CudaDiagnostic::new(
                CudaFailureKind::Partial,
                "device_compute_partial",
                None,
            )],
        )
    };
    let cuda_compute_ready = status == CudaAggregateStatus::Observed;
    CudaAggregateObservation {
        status,
        driver_version: Some(driver_version),
        devices,
        diagnostics,
        cuda_compute_ready,
    }
}

fn aggregate_failure(
    status: CudaAggregateStatus,
    driver_version: Option<u32>,
    error: ApiFailure,
) -> CudaAggregateObservation {
    CudaAggregateObservation {
        status,
        driver_version,
        devices: Vec::new(),
        diagnostics: vec![CudaDiagnostic::new(
            error.kind,
            error.operation,
            error.api_error,
        )],
        cuda_compute_ready: false,
    }
}

pub fn no_driver_observation() -> CudaAggregateObservation {
    CudaAggregateObservation {
        status: CudaAggregateStatus::NoDriver,
        driver_version: None,
        devices: Vec::new(),
        diagnostics: vec![CudaDiagnostic::new(
            CudaFailureKind::NoDriver,
            "LoadLibraryExW.System32.nvcuda",
            None,
        )],
        cuda_compute_ready: false,
    }
}

pub fn missing_symbol_observation(symbols: &[String]) -> CudaAggregateObservation {
    CudaAggregateObservation {
        status: CudaAggregateStatus::MissingSymbol,
        driver_version: None,
        devices: Vec::new(),
        diagnostics: symbols
            .iter()
            .map(|symbol| {
                CudaDiagnostic::new(
                    CudaFailureKind::MissingSymbol,
                    format!("GetProcAddress.{symbol}"),
                    None,
                )
            })
            .collect(),
        cuda_compute_ready: false,
    }
}

pub fn device_name(value: &[u8]) -> Result<String, ApiFailure> {
    let end = value
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| ApiFailure::malformed("cuDeviceGetName.unterminated"))?;
    let name = std::str::from_utf8(&value[..end])
        .map_err(|_| ApiFailure::malformed("cuDeviceGetName.utf8"))?
        .to_owned();
    if !valid_device_name(&name) {
        return Err(ApiFailure::malformed("cuDeviceGetName.privacy"));
    }
    Ok(name)
}

pub fn uuid_fingerprint(uuid: &[u8; 16]) -> String {
    format!("{:x}", Sha256::digest(uuid))
}

fn valid_device_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_DEVICE_NAME_BYTES
        && value.is_ascii()
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains(':')
        && !value.chars().any(char::is_control)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_operation_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'(' | b')' | b'|')
        })
}

fn is_null_handle_operation(value: &str) -> bool {
    matches!(
        value,
        "cuCtxCreate_v2.null_handle"
            | "cuCtxGetCurrent.preexisting_context"
            | "cuMemAlloc_v2.input.null_handle"
            | "cuMemAlloc_v2.output.null_handle"
            | "cuModuleLoadDataEx.null_handle"
            | "cuModuleGetFunction.null_handle"
    )
}

fn valid_driver_version(value: u32) -> bool {
    (100..=999_999).contains(&value)
}

fn valid_symbolic_error(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeLaunchStatus {
    Observed,
    NonzeroExit,
    Timeout,
    Crash,
    OutputTruncated,
    MalformedOutput,
    LaunchError,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProbeLaunchObservation {
    pub status: ProbeLaunchStatus,
    pub exit_code: Option<u32>,
    pub kill_reason: Option<String>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub memory_limit_reached: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SystemCudaComputeProbeReport {
    pub schema_version: u32,
    pub contract: &'static str,
    pub aggregate_driver_observation: Option<CudaAggregateObservation>,
    pub launch: ProbeLaunchObservation,
    pub cuda_compute_ready: bool,
    pub backend_ready: bool,
}

pub fn launch_system_cuda_compute_probe(
    worker_program: &Path,
    repository: &Path,
    timeout: Duration,
) -> io::Result<SystemCudaComputeProbeReport> {
    let worker_bytes = fs::read(worker_program)?;
    let worker_size = worker_bytes.len() as u64;
    let worker_sha256: [u8; 32] = Sha256::digest(&worker_bytes).into();
    let tree = probe_guard_tree()?;
    let request = SecureLaunchRequest {
        worker_program,
        worker_expected_sha256: worker_sha256,
        worker_expected_size: worker_size,
        plugin_basename: None,
        args_before_plugin: &[],
        args_after_plugin: &[],
        repository,
        require_module_audit: false,
    };
    let result = secure_launch(tree, request, Some(timeout))?;
    Ok(report_from_launch(result))
}

fn probe_guard_tree() -> io::Result<SealedLoadTree> {
    let source = std::env::temp_dir().join(format!(
        "aexcompat-cuda-probe-guard-{:032x}",
        rand::random::<u128>()
    ));
    fs::create_dir(&source)?;
    let source_file = source.join("probe.guard");
    let bytes = b"aexcompat system CUDA probe guard";
    fs::write(&source_file, bytes)?;
    let tree = SealedLoadTree::create(
        LoadEntry {
            source: source_file,
            relative_basename: "probe.guard".into(),
            expected_sha256: Sha256::digest(bytes).into(),
            expected_size: bytes.len() as u64,
        },
        Vec::new(),
    );
    let cleanup = fs::remove_dir_all(&source);
    match (tree, cleanup) {
        (Ok(tree), Ok(())) => Ok(tree),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn report_from_launch(result: SecureLaunchResult) -> SystemCudaComputeProbeReport {
    let launch = ProbeLaunchObservation {
        status: match result.classification {
            ExitClassification::Ok => ProbeLaunchStatus::Observed,
            ExitClassification::NonzeroExit => ProbeLaunchStatus::NonzeroExit,
            ExitClassification::TimeoutKilled => ProbeLaunchStatus::Timeout,
            ExitClassification::Crashed => ProbeLaunchStatus::Crash,
        },
        exit_code: Some(result.exit_code),
        kill_reason: result.kill_reason.map(str::to_owned),
        stdout_truncated: result.stdout_truncated,
        stderr_truncated: result.stderr_truncated,
        memory_limit_reached: result.memory_limit_reached,
    };
    if result.classification != ExitClassification::Ok {
        return report(launch, None);
    }
    if result.stdout_truncated || result.stdout.len() > MAX_WORKER_STDOUT_BYTES {
        return report(
            ProbeLaunchObservation {
                status: ProbeLaunchStatus::OutputTruncated,
                ..launch
            },
            None,
        );
    }
    report_from_worker_json(launch, result.stdout.trim())
}

pub fn report_from_worker_json(
    launch: ProbeLaunchObservation,
    worker_json: &str,
) -> SystemCudaComputeProbeReport {
    match serde_json::from_str::<CudaAggregateObservation>(worker_json) {
        Ok(observation) if observation.validate() => report(launch, Some(observation)),
        _ => report(
            ProbeLaunchObservation {
                status: ProbeLaunchStatus::MalformedOutput,
                ..launch
            },
            None,
        ),
    }
}

pub fn launch_error_report() -> SystemCudaComputeProbeReport {
    report(
        ProbeLaunchObservation {
            status: ProbeLaunchStatus::LaunchError,
            exit_code: None,
            kill_reason: None,
            stdout_truncated: false,
            stderr_truncated: false,
            memory_limit_reached: false,
        },
        None,
    )
}

fn report(
    launch: ProbeLaunchObservation,
    aggregate_driver_observation: Option<CudaAggregateObservation>,
) -> SystemCudaComputeProbeReport {
    SystemCudaComputeProbeReport {
        schema_version: 1,
        contract: "system_cuda_driver_compute_probe",
        cuda_compute_ready: aggregate_driver_observation
            .as_ref()
            .is_some_and(|observation| observation.cuda_compute_ready),
        aggregate_driver_observation,
        launch,
        backend_ready: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    struct MockApi {
        initialize: Result<(), ApiFailure>,
        driver_version: Result<u32, ApiFailure>,
        device_count: Result<usize, ApiFailure>,
        devices: RefCell<VecDeque<CudaDeviceObservation>>,
    }

    impl CudaProbeApi for MockApi {
        fn initialize(&self) -> Result<(), ApiFailure> {
            self.initialize.clone()
        }

        fn driver_version(&self) -> Result<u32, ApiFailure> {
            self.driver_version.clone()
        }

        fn device_count(&self, _: usize) -> Result<usize, ApiFailure> {
            self.device_count.clone()
        }

        fn probe_device(&self, _: u32) -> CudaDeviceObservation {
            self.devices.borrow_mut().pop_front().unwrap()
        }
    }

    fn metadata(ordinal: u32) -> CudaDeviceObservation {
        CudaDeviceObservation::passed(
            ordinal,
            "NVIDIA Fixture".into(),
            "ab".repeat(32),
            PciLocation {
                domain: 0,
                bus: 1,
                device: 0,
            },
            8,
            9,
            12 * 1024 * 1024 * 1024,
        )
    }

    fn mock(devices: Vec<CudaDeviceObservation>) -> MockApi {
        MockApi {
            initialize: Ok(()),
            driver_version: Ok(13_000),
            device_count: Ok(devices.len()),
            devices: RefCell::new(devices.into()),
        }
    }

    #[test]
    fn all_devices_must_pass_and_partial_failure_is_fail_closed() {
        let passed = collect_with_api(&mock(vec![metadata(0), metadata(1)]));
        assert!(passed.cuda_compute_ready);
        assert!(passed.validate());

        let failed = CudaDeviceObservation::with_compute_failure(
            metadata(1),
            CudaStage::Launch,
            "cuLaunchKernel",
            Some(719),
            None,
        );
        let partial = collect_with_api(&mock(vec![metadata(0), failed]));
        assert_eq!(partial.status, CudaAggregateStatus::Partial);
        assert!(!partial.cuda_compute_ready);
        assert_eq!(partial.devices.len(), 2);
        assert_eq!(partial.diagnostics[0].kind, CudaFailureKind::Partial);
        assert!(partial.validate());

        let failed = collect_with_api(&mock(vec![
            CudaDeviceObservation::with_compute_failure(
                metadata(0),
                CudaStage::Launch,
                "cuLaunchKernel",
                Some(719),
                None,
            ),
            CudaDeviceObservation::with_compute_failure(
                metadata(1),
                CudaStage::Mismatch,
                "readback_compare",
                None,
                None,
            ),
        ]));
        assert_eq!(failed.status, CudaAggregateStatus::Failed);
        assert!(!failed.cuda_compute_ready);
        assert!(failed.validate());
    }

    #[test]
    fn initialization_count_and_no_device_failures_remain_distinct() {
        let init = MockApi {
            initialize: Err(ApiFailure::api("cuInit", 3)),
            ..mock(Vec::new())
        };
        assert_eq!(
            collect_with_api(&init).status,
            CudaAggregateStatus::Incomplete
        );

        let no_device = collect_with_api(&mock(Vec::new()));
        assert_eq!(no_device.status, CudaAggregateStatus::NoDevice);
        assert!(!no_device.cuda_compute_ready);

        let limit_devices = (0..=MAX_CUDA_DEVICES)
            .map(|ordinal| metadata(ordinal as u32))
            .collect::<Vec<_>>();
        let limit = collect_with_api(&mock(limit_devices));
        assert_eq!(limit.status, CudaAggregateStatus::Partial);
        assert_eq!(limit.devices.len(), MAX_CUDA_DEVICES);
        assert_eq!(limit.diagnostics[0].kind, CudaFailureKind::DeviceLimit);
        assert!(!limit.cuda_compute_ready);
        assert!(limit.validate());
    }

    #[test]
    fn privacy_metadata_and_jit_evidence_are_bounded() {
        assert!(device_name(b"NVIDIA Fixture\0").is_ok());
        for invalid in [
            b"C:\\Users\\name\0".as_slice(),
            b"vendor/private\0".as_slice(),
            b"unterminated".as_slice(),
        ] {
            assert!(device_name(invalid).is_err());
        }
        let log = JitLogObservation::from_bounded_logs(b"compiler info", b"error detail", false);
        assert_eq!(log.status, JitLogStatus::Redacted);
        assert_eq!(log.sha256.as_deref().unwrap().len(), 64);
        assert!(log.is_valid());
        assert_eq!(
            JitLogObservation::from_bounded_logs(b"", b"", true).status,
            JitLogStatus::Overflow
        );
    }

    #[test]
    fn malformed_worker_observations_are_rejected() {
        let mut wrong_ready = collect_with_api(&mock(vec![metadata(0)]));
        wrong_ready.cuda_compute_ready = false;
        assert!(!wrong_ready.validate());

        let mut raw_path = collect_with_api(&mock(vec![metadata(0)]));
        raw_path.devices[0].name = Some("C:\\Private\\device".into());
        assert!(!raw_path.validate());

        let mut wrong_ordinal = collect_with_api(&mock(vec![metadata(0)]));
        wrong_ordinal.devices[0].ordinal = 4;
        assert!(!wrong_ordinal.validate());
    }

    #[test]
    fn structural_null_handle_failures_round_trip_without_api_error() {
        for (stage, operation) in [
            (CudaStage::Context, "cuCtxCreate_v2.null_handle"),
            (CudaStage::Allocation, "cuMemAlloc_v2.input.null_handle"),
            (CudaStage::Allocation, "cuMemAlloc_v2.output.null_handle"),
            (CudaStage::Module, "cuModuleLoadDataEx.null_handle"),
            (CudaStage::Function, "cuModuleGetFunction.null_handle"),
        ] {
            let observation =
                collect_with_api(&mock(vec![CudaDeviceObservation::with_compute_failure(
                    metadata(0),
                    stage,
                    operation,
                    None,
                    None,
                )]));
            assert_eq!(observation.status, CudaAggregateStatus::Failed);
            assert!(observation.validate(), "{operation}");
            let launch = ProbeLaunchObservation {
                status: ProbeLaunchStatus::Observed,
                exit_code: Some(0),
                kill_reason: None,
                stdout_truncated: false,
                stderr_truncated: false,
                memory_limit_reached: false,
            };
            let json = serde_json::to_string(&observation).unwrap();
            let report = report_from_worker_json(launch, &json);
            assert_eq!(report.launch.status, ProbeLaunchStatus::Observed);
            let device = &report.aggregate_driver_observation.unwrap().devices[0];
            assert_eq!(device.stage, stage);
            assert_eq!(device.operation.as_deref(), Some(operation));
            assert_eq!(device.api_error, None);
        }

        let invalid = collect_with_api(&mock(vec![CudaDeviceObservation::with_compute_failure(
            metadata(0),
            CudaStage::Context,
            "cuCtxCreate_v2",
            None,
            None,
        )]));
        assert!(!invalid.validate());
    }

    #[test]
    fn cleanup_failures_are_structured_bounded_and_fail_closed() {
        let failed = CudaDeviceObservation::with_cleanup_failures(
            metadata(0),
            vec![
                CudaCleanupFailure {
                    target: CudaCleanupTarget::Module,
                    operation: "cuModuleUnload".into(),
                    api_error: 711,
                    symbolic_info: "CUDA_ERROR_UNKNOWN".into(),
                },
                CudaCleanupFailure {
                    target: CudaCleanupTarget::Context,
                    operation: "cuCtxDestroy_v2".into(),
                    api_error: 709,
                    symbolic_info: "CUDA_ERROR_UNKNOWN".into(),
                },
            ],
        );
        assert_eq!(failed.stage, CudaStage::CleanupFailed);
        assert!(failed.validate());
        let aggregate = collect_with_api(&mock(vec![failed.clone()]));
        assert_eq!(aggregate.status, CudaAggregateStatus::Failed);
        assert!(!aggregate.cuda_compute_ready);
        assert!(aggregate.validate());

        let mut wrong_operation = failed.clone();
        wrong_operation.cleanup_failures[0].operation = "cuCtxDestroy_v2".into();
        assert!(!wrong_operation.validate());

        let mut too_many = failed;
        too_many.cleanup_failures = vec![too_many.cleanup_failures[0].clone(); 5];
        assert!(!too_many.validate());
    }

    #[test]
    fn all_devices_not_attempted_are_partial_not_failed() {
        let aggregate = collect_with_api(&mock(vec![
            CudaDeviceObservation::not_attempted(0, "device_probe.not_attempted"),
            CudaDeviceObservation::not_attempted(1, "device_probe.not_attempted"),
        ]));
        assert_eq!(aggregate.status, CudaAggregateStatus::Partial);
        assert!(!aggregate.cuda_compute_ready);
        assert_eq!(aggregate.devices.len(), 2);
        assert_eq!(aggregate.diagnostics[0].kind, CudaFailureKind::Partial);
        assert!(aggregate.validate());
    }

    fn launch_result(classification: ExitClassification, stdout: &str) -> SecureLaunchResult {
        SecureLaunchResult {
            classification,
            exit_code: match classification {
                ExitClassification::Ok => 0,
                ExitClassification::NonzeroExit => 7,
                ExitClassification::TimeoutKilled => 0,
                ExitClassification::Crashed => 0xc000_0005,
            },
            stdout: stdout.into(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            kill_reason: (classification == ExitClassification::TimeoutKilled).then_some("timeout"),
            worker_peak_commit_bytes: None,
            peak_process_memory_bytes: None,
            peak_job_memory_bytes: None,
            process_memory_limit_bytes: 0,
            memory_limit_reached: false,
            dismissed_windows: Vec::new(),
        }
    }

    #[test]
    fn launcher_classifies_nonzero_crash_timeout_malformed_and_output_bound() {
        for (classification, expected) in [
            (
                ExitClassification::NonzeroExit,
                ProbeLaunchStatus::NonzeroExit,
            ),
            (
                ExitClassification::TimeoutKilled,
                ProbeLaunchStatus::Timeout,
            ),
            (ExitClassification::Crashed, ProbeLaunchStatus::Crash),
        ] {
            let report = report_from_launch(launch_result(classification, ""));
            assert_eq!(report.launch.status, expected);
            assert!(!report.cuda_compute_ready);
        }
        let malformed = report_from_launch(launch_result(ExitClassification::Ok, "{bad"));
        assert_eq!(malformed.launch.status, ProbeLaunchStatus::MalformedOutput);
        let mut truncated = launch_result(ExitClassification::Ok, "{}");
        truncated.stdout_truncated = true;
        assert_eq!(
            report_from_launch(truncated).launch.status,
            ProbeLaunchStatus::OutputTruncated
        );
    }
}
