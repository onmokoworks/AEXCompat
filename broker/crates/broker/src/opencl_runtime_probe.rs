use crate::ExitClassification;
use crate::secure_launch::{SecureLaunchRequest, SecureLaunchResult, secure_launch_without_plugin};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

pub const MAX_PLATFORMS: usize = 16;
pub const MAX_DEVICES_PER_PLATFORM: usize = 64;
pub const MAX_STRING_BYTES: usize = 16 * 1024;
pub const MAX_WORK_ITEM_DIMENSIONS: usize = 8;
pub const MAX_WORKER_STDOUT_BYTES: usize = 1024 * 1024;
pub const COMPUTE_ELEMENT_COUNT: usize = 64;
pub const MAX_BUILD_LOG_BYTES: usize = 16 * 1024;

pub const CL_PLATFORM_PROFILE: u32 = 0x0900;
pub const CL_PLATFORM_VERSION: u32 = 0x0901;
pub const CL_PLATFORM_NAME: u32 = 0x0902;
pub const CL_PLATFORM_VENDOR: u32 = 0x0903;
pub const CL_PLATFORM_EXTENSIONS: u32 = 0x0904;
pub const CL_PLATFORM_ICD_SUFFIX_KHR: u32 = 0x0920;

pub const CL_DEVICE_TYPE: u32 = 0x1000;
pub const CL_DEVICE_VENDOR_ID: u32 = 0x1001;
pub const CL_DEVICE_MAX_COMPUTE_UNITS: u32 = 0x1002;
pub const CL_DEVICE_MAX_WORK_ITEM_DIMENSIONS: u32 = 0x1003;
pub const CL_DEVICE_MAX_WORK_GROUP_SIZE: u32 = 0x1004;
pub const CL_DEVICE_MAX_WORK_ITEM_SIZES: u32 = 0x1005;
pub const CL_DEVICE_MAX_CLOCK_FREQUENCY: u32 = 0x100c;
pub const CL_DEVICE_MAX_MEM_ALLOC_SIZE: u32 = 0x1010;
pub const CL_DEVICE_GLOBAL_MEM_SIZE: u32 = 0x101f;
pub const CL_DEVICE_LOCAL_MEM_SIZE: u32 = 0x1023;
pub const CL_DEVICE_AVAILABLE: u32 = 0x1027;
pub const CL_DEVICE_COMPILER_AVAILABLE: u32 = 0x1028;
pub const CL_DEVICE_NAME: u32 = 0x102b;
pub const CL_DEVICE_VENDOR: u32 = 0x102c;
pub const CL_DRIVER_VERSION: u32 = 0x102d;
pub const CL_DEVICE_PROFILE: u32 = 0x102e;
pub const CL_DEVICE_VERSION: u32 = 0x102f;
pub const CL_DEVICE_EXTENSIONS: u32 = 0x1030;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeFailureKind {
    NoLoader,
    MissingSymbol,
    NoPlatform,
    NoDevice,
    ApiError,
    Malformed,
    CountOverflow,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProbeDiagnostic {
    pub kind: ProbeFailureKind,
    pub operation: String,
    pub api_error: Option<i32>,
    pub platform_index: Option<u32>,
    pub device_index: Option<u32>,
}

impl ProbeDiagnostic {
    pub fn simple(kind: ProbeFailureKind, operation: impl Into<String>) -> Self {
        Self {
            kind,
            operation: operation.into(),
            api_error: None,
            platform_index: None,
            device_index: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApiFailure {
    pub kind: ProbeFailureKind,
    pub operation: String,
    pub api_error: Option<i32>,
}

impl ApiFailure {
    pub fn api(operation: impl Into<String>, code: i32) -> Self {
        Self {
            kind: ProbeFailureKind::ApiError,
            operation: operation.into(),
            api_error: Some(code),
        }
    }

    pub fn malformed(operation: impl Into<String>) -> Self {
        Self {
            kind: ProbeFailureKind::Malformed,
            operation: operation.into(),
            api_error: None,
        }
    }

    pub fn overflow(operation: impl Into<String>) -> Self {
        Self {
            kind: ProbeFailureKind::CountOverflow,
            operation: operation.into(),
            api_error: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlatformObservation {
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub profile: String,
    pub extensions: Vec<String>,
    pub icd_suffix: String,
    pub devices: Vec<DeviceObservation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeviceObservation {
    pub device_type: u64,
    pub vendor_id: u32,
    pub name: String,
    pub vendor: String,
    pub driver_version: String,
    pub version: String,
    pub profile: String,
    pub extensions: Vec<String>,
    pub available: bool,
    pub compiler_available: bool,
    pub max_compute_units: u32,
    pub max_clock_frequency_mhz: u32,
    pub max_work_group_size: u64,
    pub max_work_item_sizes: Vec<u64>,
    pub max_mem_alloc_bytes: u64,
    pub global_mem_bytes: u64,
    pub local_mem_bytes: u64,
    pub compute: ComputeDeviceObservation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputeStage {
    Passed,
    Context,
    Queue,
    Buffer,
    Build,
    Kernel,
    Enqueue,
    Finish,
    Readback,
    Mismatch,
    NotAttempted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueApi {
    WithProperties,
    Legacy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildLogStatus {
    Redacted,
    Overflow,
    Unavailable,
    Malformed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BuildLogObservation {
    pub status: BuildLogStatus,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputeDeviceObservation {
    pub stage: ComputeStage,
    pub queue_api: Option<QueueApi>,
    pub api_error: Option<i32>,
    pub missing_symbols: Vec<String>,
    pub build_log: Option<BuildLogObservation>,
}

impl ComputeDeviceObservation {
    pub fn passed(queue_api: QueueApi) -> Self {
        Self {
            stage: ComputeStage::Passed,
            queue_api: Some(queue_api),
            api_error: None,
            missing_symbols: Vec::new(),
            build_log: None,
        }
    }

    pub fn failed(stage: ComputeStage, api_error: Option<i32>) -> Self {
        Self {
            stage,
            queue_api: None,
            api_error,
            missing_symbols: Vec::new(),
            build_log: None,
        }
    }

    pub fn failed_after_queue(
        stage: ComputeStage,
        api_error: Option<i32>,
        queue_api: QueueApi,
    ) -> Self {
        let mut observation = Self::failed(stage, api_error);
        observation.queue_api = Some(queue_api);
        observation
    }

    pub fn not_attempted(missing_symbols: Vec<String>) -> Self {
        Self {
            stage: ComputeStage::NotAttempted,
            queue_api: None,
            api_error: None,
            missing_symbols,
            build_log: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateStatus {
    Observed,
    Incomplete,
    NoLoader,
    MissingSymbol,
    NoPlatform,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AggregateLoaderObservation {
    pub status: AggregateStatus,
    pub platforms: Vec<PlatformObservation>,
    pub diagnostics: Vec<ProbeDiagnostic>,
    pub compute_ready: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateEvidenceBoundary {
    pub legacy_registry_contract: &'static str,
    pub adapter_binding_contract: &'static str,
    pub pnp_software_key_contract: &'static str,
    pub individual_icd_binding: &'static str,
}

impl Default for CandidateEvidenceBoundary {
    fn default() -> Self {
        Self {
            legacy_registry_contract: "opencl_icd_registry_candidates",
            adapter_binding_contract: "opencl_icd_adapter_driver",
            pnp_software_key_contract: "windows_pnp_opencl_runtime",
            individual_icd_binding: "not_attempted",
        }
    }
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
pub struct SystemOpenClProbeReport {
    pub schema_version: u32,
    pub contract: &'static str,
    pub candidate_evidence: CandidateEvidenceBoundary,
    pub aggregate_loader_observation: Option<AggregateLoaderObservation>,
    pub launch: ProbeLaunchObservation,
    pub compute_ready: bool,
    pub backend_ready: bool,
}

pub trait OpenClApi {
    fn platform_ids(&self, limit: usize) -> Result<Vec<usize>, ApiFailure>;
    fn platform_string(
        &self,
        platform: usize,
        parameter: u32,
        limit: usize,
    ) -> Result<String, ApiFailure>;
    fn device_ids(&self, platform: usize, limit: usize) -> Result<Vec<usize>, ApiFailure>;
    fn device_string(
        &self,
        device: usize,
        parameter: u32,
        limit: usize,
    ) -> Result<String, ApiFailure>;
    fn device_u32(&self, device: usize, parameter: u32) -> Result<u32, ApiFailure>;
    fn device_u64(&self, device: usize, parameter: u32) -> Result<u64, ApiFailure>;
    fn device_usize(&self, device: usize, parameter: u32) -> Result<usize, ApiFailure>;
    fn device_usizes(
        &self,
        device: usize,
        parameter: u32,
        count: usize,
    ) -> Result<Vec<usize>, ApiFailure>;
}

pub trait ComputeDeviceProbe {
    fn probe_device(
        &self,
        platform: usize,
        device: usize,
        platform_version: &str,
        device_version: &str,
    ) -> ComputeDeviceObservation;
}

struct NotAttemptedCompute;

impl ComputeDeviceProbe for NotAttemptedCompute {
    fn probe_device(&self, _: usize, _: usize, _: &str, _: &str) -> ComputeDeviceObservation {
        ComputeDeviceObservation::not_attempted(Vec::new())
    }
}

pub fn collect_with_api(api: &impl OpenClApi) -> AggregateLoaderObservation {
    collect_with_compute(api, &NotAttemptedCompute)
}

pub fn collect_with_compute(
    api: &impl OpenClApi,
    compute: &impl ComputeDeviceProbe,
) -> AggregateLoaderObservation {
    let platforms = match api.platform_ids(MAX_PLATFORMS) {
        Ok(platforms) if platforms.is_empty() => {
            return AggregateLoaderObservation {
                status: AggregateStatus::NoPlatform,
                platforms: Vec::new(),
                diagnostics: vec![ProbeDiagnostic::simple(
                    ProbeFailureKind::NoPlatform,
                    "clGetPlatformIDs",
                )],
                compute_ready: false,
            };
        }
        Ok(platforms) => platforms,
        Err(error) => return failure_observation(error),
    };
    let mut observations = Vec::new();
    let mut diagnostics = Vec::new();
    for (platform_index, platform) in platforms.into_iter().enumerate() {
        match collect_platform(api, compute, platform) {
            Ok(observation) => observations.push(observation),
            Err(mut error) => {
                error.platform_index = Some(platform_index as u32);
                diagnostics.push(error);
            }
        }
    }
    let status = if observations.is_empty() || !diagnostics.is_empty() {
        AggregateStatus::Incomplete
    } else {
        AggregateStatus::Observed
    };
    let compute_ready = status == AggregateStatus::Observed
        && observations.iter().all(|platform| {
            !platform.devices.is_empty()
                && platform
                    .devices
                    .iter()
                    .all(|device| device.compute.stage == ComputeStage::Passed)
        });
    AggregateLoaderObservation {
        status,
        platforms: observations,
        diagnostics,
        compute_ready,
    }
}

pub fn no_loader_observation() -> AggregateLoaderObservation {
    AggregateLoaderObservation {
        status: AggregateStatus::NoLoader,
        platforms: Vec::new(),
        diagnostics: vec![ProbeDiagnostic::simple(
            ProbeFailureKind::NoLoader,
            "LoadLibraryExW(system32:OpenCL.dll)",
        )],
        compute_ready: false,
    }
}

pub fn missing_symbol_observation(symbols: &[&str]) -> AggregateLoaderObservation {
    AggregateLoaderObservation {
        status: AggregateStatus::MissingSymbol,
        platforms: Vec::new(),
        diagnostics: symbols
            .iter()
            .map(|symbol| {
                ProbeDiagnostic::simple(
                    ProbeFailureKind::MissingSymbol,
                    format!("GetProcAddress({symbol})"),
                )
            })
            .collect(),
        compute_ready: false,
    }
}

fn failure_observation(error: ApiFailure) -> AggregateLoaderObservation {
    AggregateLoaderObservation {
        status: AggregateStatus::Incomplete,
        platforms: Vec::new(),
        diagnostics: vec![ProbeDiagnostic {
            kind: error.kind,
            operation: error.operation,
            api_error: error.api_error,
            platform_index: None,
            device_index: None,
        }],
        compute_ready: false,
    }
}

fn collect_platform(
    api: &impl OpenClApi,
    compute: &impl ComputeDeviceProbe,
    platform: usize,
) -> Result<PlatformObservation, ProbeDiagnostic> {
    let name = platform_string(api, platform, CL_PLATFORM_NAME)?;
    let vendor = platform_string(api, platform, CL_PLATFORM_VENDOR)?;
    let version = platform_string(api, platform, CL_PLATFORM_VERSION)?;
    let profile = platform_string(api, platform, CL_PLATFORM_PROFILE)?;
    let extensions = extension_list(platform_string_allow_empty(
        api,
        platform,
        CL_PLATFORM_EXTENSIONS,
    )?)?;
    let icd_suffix = platform_string(api, platform, CL_PLATFORM_ICD_SUFFIX_KHR)?;
    let device_ids = api
        .device_ids(platform, MAX_DEVICES_PER_PLATFORM)
        .map_err(diagnostic)?;
    if device_ids.is_empty() {
        return Err(ProbeDiagnostic::simple(
            ProbeFailureKind::NoDevice,
            "clGetDeviceIDs",
        ));
    }
    let mut devices = Vec::new();
    for (device_index, device) in device_ids.into_iter().enumerate() {
        match collect_device(api, compute, platform, device, &version) {
            Ok(observation) => devices.push(observation),
            Err(mut error) => {
                error.device_index = Some(device_index as u32);
                return Err(error);
            }
        }
    }
    Ok(PlatformObservation {
        name,
        vendor,
        version,
        profile,
        extensions,
        icd_suffix,
        devices,
    })
}

fn collect_device(
    api: &impl OpenClApi,
    compute: &impl ComputeDeviceProbe,
    platform: usize,
    device: usize,
    platform_version: &str,
) -> Result<DeviceObservation, ProbeDiagnostic> {
    let dimensions = api
        .device_u32(device, CL_DEVICE_MAX_WORK_ITEM_DIMENSIONS)
        .map_err(diagnostic)? as usize;
    if dimensions == 0 || dimensions > MAX_WORK_ITEM_DIMENSIONS {
        return Err(ProbeDiagnostic::simple(
            ProbeFailureKind::CountOverflow,
            "clGetDeviceInfo(CL_DEVICE_MAX_WORK_ITEM_DIMENSIONS)",
        ));
    }
    let work_item_sizes = api
        .device_usizes(device, CL_DEVICE_MAX_WORK_ITEM_SIZES, dimensions)
        .map_err(diagnostic)?;
    if work_item_sizes.len() != dimensions {
        return Err(ProbeDiagnostic::simple(
            ProbeFailureKind::Malformed,
            "clGetDeviceInfo(CL_DEVICE_MAX_WORK_ITEM_SIZES)",
        ));
    }
    if work_item_sizes.contains(&0) {
        return Err(ProbeDiagnostic::simple(
            ProbeFailureKind::Malformed,
            "clGetDeviceInfo(CL_DEVICE_MAX_WORK_ITEM_SIZES)",
        ));
    }
    let available = cl_bool(
        api.device_u32(device, CL_DEVICE_AVAILABLE)
            .map_err(diagnostic)?,
        "clGetDeviceInfo(CL_DEVICE_AVAILABLE)",
    )?;
    let compiler_available = cl_bool(
        api.device_u32(device, CL_DEVICE_COMPILER_AVAILABLE)
            .map_err(diagnostic)?,
        "clGetDeviceInfo(CL_DEVICE_COMPILER_AVAILABLE)",
    )?;
    let device_type = api.device_u64(device, CL_DEVICE_TYPE).map_err(diagnostic)?;
    let vendor_id = api
        .device_u32(device, CL_DEVICE_VENDOR_ID)
        .map_err(diagnostic)?;
    let max_compute_units = api
        .device_u32(device, CL_DEVICE_MAX_COMPUTE_UNITS)
        .map_err(diagnostic)?;
    let max_clock_frequency_mhz = api
        .device_u32(device, CL_DEVICE_MAX_CLOCK_FREQUENCY)
        .map_err(diagnostic)?;
    let max_work_group_size = api
        .device_usize(device, CL_DEVICE_MAX_WORK_GROUP_SIZE)
        .map_err(diagnostic)?;
    if device_type == 0
        || vendor_id == 0
        || max_compute_units == 0
        || max_compute_units > 1_048_576
        || max_clock_frequency_mhz > 1_048_576
        || max_work_group_size == 0
    {
        return Err(ProbeDiagnostic::simple(
            ProbeFailureKind::Malformed,
            "clGetDeviceInfo(capability bounds)",
        ));
    }
    let name = device_string(api, device, CL_DEVICE_NAME)?;
    let vendor = device_string(api, device, CL_DEVICE_VENDOR)?;
    let driver_version = device_string(api, device, CL_DRIVER_VERSION)?;
    let version = device_string(api, device, CL_DEVICE_VERSION)?;
    let profile = device_string(api, device, CL_DEVICE_PROFILE)?;
    let extensions = extension_list(device_string_allow_empty(
        api,
        device,
        CL_DEVICE_EXTENSIONS,
    )?)?;
    let compute = compute.probe_device(platform, device, platform_version, &version);
    Ok(DeviceObservation {
        device_type,
        vendor_id,
        name,
        vendor,
        driver_version,
        version,
        profile,
        extensions,
        available,
        compiler_available,
        max_compute_units,
        max_clock_frequency_mhz,
        max_work_group_size: max_work_group_size as u64,
        max_work_item_sizes: work_item_sizes
            .into_iter()
            .map(|value| value as u64)
            .collect(),
        max_mem_alloc_bytes: api
            .device_u64(device, CL_DEVICE_MAX_MEM_ALLOC_SIZE)
            .map_err(diagnostic)?,
        global_mem_bytes: api
            .device_u64(device, CL_DEVICE_GLOBAL_MEM_SIZE)
            .map_err(diagnostic)?,
        local_mem_bytes: api
            .device_u64(device, CL_DEVICE_LOCAL_MEM_SIZE)
            .map_err(diagnostic)?,
        compute,
    })
}

fn platform_string(
    api: &impl OpenClApi,
    platform: usize,
    parameter: u32,
) -> Result<String, ProbeDiagnostic> {
    bounded_string(
        api.platform_string(platform, parameter, MAX_STRING_BYTES)
            .map_err(diagnostic)?,
        "clGetPlatformInfo",
        false,
    )
}

fn platform_string_allow_empty(
    api: &impl OpenClApi,
    platform: usize,
    parameter: u32,
) -> Result<String, ProbeDiagnostic> {
    bounded_string(
        api.platform_string(platform, parameter, MAX_STRING_BYTES)
            .map_err(diagnostic)?,
        "clGetPlatformInfo",
        true,
    )
}

fn device_string(
    api: &impl OpenClApi,
    device: usize,
    parameter: u32,
) -> Result<String, ProbeDiagnostic> {
    bounded_string(
        api.device_string(device, parameter, MAX_STRING_BYTES)
            .map_err(diagnostic)?,
        "clGetDeviceInfo",
        false,
    )
}

fn device_string_allow_empty(
    api: &impl OpenClApi,
    device: usize,
    parameter: u32,
) -> Result<String, ProbeDiagnostic> {
    bounded_string(
        api.device_string(device, parameter, MAX_STRING_BYTES)
            .map_err(diagnostic)?,
        "clGetDeviceInfo",
        true,
    )
}

fn bounded_string(
    value: String,
    operation: &str,
    allow_empty: bool,
) -> Result<String, ProbeDiagnostic> {
    if (!allow_empty && value.is_empty())
        || value.len() > MAX_STRING_BYTES
        || value.contains('\0')
        || value.chars().any(|character| character.is_control())
        || contains_disallowed_metadata_path(&value)
    {
        return Err(ProbeDiagnostic::simple(
            ProbeFailureKind::Malformed,
            operation,
        ));
    }
    Ok(value)
}

fn contains_disallowed_metadata_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.contains('/')
        || value.contains('\\')
        || bytes
            .get(0..2)
            .is_some_and(|prefix| prefix[0].is_ascii_alphabetic() && prefix[1] == b':')
}

fn extension_list(value: String) -> Result<Vec<String>, ProbeDiagnostic> {
    let mut extensions = value
        .split_ascii_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if extensions.len() > 1024
        || extensions
            .iter()
            .any(|item| item.len() > 255 || !item.is_ascii())
    {
        return Err(ProbeDiagnostic::simple(
            ProbeFailureKind::CountOverflow,
            "extension list",
        ));
    }
    extensions.sort();
    extensions.dedup();
    Ok(extensions)
}

fn cl_bool(value: u32, operation: &str) -> Result<bool, ProbeDiagnostic> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ProbeDiagnostic::simple(
            ProbeFailureKind::Malformed,
            operation,
        )),
    }
}

fn diagnostic(error: ApiFailure) -> ProbeDiagnostic {
    ProbeDiagnostic {
        kind: error.kind,
        operation: error.operation,
        api_error: error.api_error,
        platform_index: None,
        device_index: None,
    }
}

pub fn launch_system_opencl_probe(
    worker_program: &Path,
    repository: &Path,
    timeout: Duration,
) -> io::Result<SystemOpenClProbeReport> {
    let worker_bytes = fs::read(worker_program)?;
    let worker_size = worker_bytes.len() as u64;
    let worker_sha256: [u8; 32] = Sha256::digest(&worker_bytes).into();
    let request = SecureLaunchRequest {
        worker_program,
        worker_expected_sha256: worker_sha256,
        worker_expected_size: worker_size,
        args_before_plugin: &[],
        args_after_plugin: &[],
        repository,
        require_module_audit: false,
        launch_environment: Default::default(),
    };
    let result = secure_launch_without_plugin(request, Some(timeout), None)?;
    Ok(report_from_launch(result))
}

fn report_from_launch(result: SecureLaunchResult) -> SystemOpenClProbeReport {
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
    let observation = serde_json::from_str::<AggregateLoaderObservation>(result.stdout.trim());
    match observation {
        Ok(observation) => report(launch, Some(observation)),
        Err(_) => report(
            ProbeLaunchObservation {
                status: ProbeLaunchStatus::MalformedOutput,
                ..launch
            },
            None,
        ),
    }
}

pub fn launch_error_report() -> SystemOpenClProbeReport {
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
    aggregate_loader_observation: Option<AggregateLoaderObservation>,
) -> SystemOpenClProbeReport {
    SystemOpenClProbeReport {
        schema_version: 2,
        contract: "system_opencl_loader_probe",
        candidate_evidence: CandidateEvidenceBoundary::default(),
        compute_ready: aggregate_loader_observation
            .as_ref()
            .is_some_and(|observation| observation.compute_ready),
        aggregate_loader_observation,
        launch,
        backend_ready: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::collections::VecDeque;

    struct MockApi {
        platforms: Result<Vec<usize>, ApiFailure>,
        devices: BTreeMap<usize, Result<Vec<usize>, ApiFailure>>,
        platform_strings: BTreeMap<(usize, u32), Result<String, ApiFailure>>,
        device_strings: BTreeMap<(usize, u32), Result<String, ApiFailure>>,
        device_u32s: BTreeMap<(usize, u32), Result<u32, ApiFailure>>,
        device_u64s: BTreeMap<(usize, u32), Result<u64, ApiFailure>>,
        device_usizes: BTreeMap<(usize, u32), Result<usize, ApiFailure>>,
        device_usize_arrays: BTreeMap<(usize, u32), Result<Vec<usize>, ApiFailure>>,
    }

    impl Default for MockApi {
        fn default() -> Self {
            Self {
                platforms: Ok(Vec::new()),
                devices: BTreeMap::new(),
                platform_strings: BTreeMap::new(),
                device_strings: BTreeMap::new(),
                device_u32s: BTreeMap::new(),
                device_u64s: BTreeMap::new(),
                device_usizes: BTreeMap::new(),
                device_usize_arrays: BTreeMap::new(),
            }
        }
    }

    struct MockCompute {
        observations: RefCell<VecDeque<ComputeDeviceObservation>>,
    }

    impl MockCompute {
        fn new(observations: impl IntoIterator<Item = ComputeDeviceObservation>) -> Self {
            Self {
                observations: RefCell::new(observations.into_iter().collect()),
            }
        }
    }

    impl ComputeDeviceProbe for MockCompute {
        fn probe_device(&self, _: usize, _: usize, _: &str, _: &str) -> ComputeDeviceObservation {
            self.observations
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| ComputeDeviceObservation::not_attempted(Vec::new()))
        }
    }

    impl OpenClApi for MockApi {
        fn platform_ids(&self, limit: usize) -> Result<Vec<usize>, ApiFailure> {
            let values = self.platforms.clone()?;
            if values.len() > limit {
                Err(ApiFailure::overflow("clGetPlatformIDs"))
            } else {
                Ok(values)
            }
        }

        fn platform_string(
            &self,
            platform: usize,
            parameter: u32,
            _: usize,
        ) -> Result<String, ApiFailure> {
            self.platform_strings
                .get(&(platform, parameter))
                .cloned()
                .unwrap_or_else(|| Err(ApiFailure::api("clGetPlatformInfo", -30)))
        }

        fn device_ids(&self, platform: usize, limit: usize) -> Result<Vec<usize>, ApiFailure> {
            let values = self
                .devices
                .get(&platform)
                .cloned()
                .unwrap_or_else(|| Err(ApiFailure::api("clGetDeviceIDs", -30)))?;
            if values.len() > limit {
                Err(ApiFailure::overflow("clGetDeviceIDs"))
            } else {
                Ok(values)
            }
        }

        fn device_string(
            &self,
            device: usize,
            parameter: u32,
            _: usize,
        ) -> Result<String, ApiFailure> {
            self.device_strings
                .get(&(device, parameter))
                .cloned()
                .unwrap_or_else(|| Err(ApiFailure::api("clGetDeviceInfo", -30)))
        }

        fn device_u32(&self, device: usize, parameter: u32) -> Result<u32, ApiFailure> {
            self.device_u32s
                .get(&(device, parameter))
                .cloned()
                .unwrap_or_else(|| Err(ApiFailure::api("clGetDeviceInfo", -30)))
        }

        fn device_u64(&self, device: usize, parameter: u32) -> Result<u64, ApiFailure> {
            self.device_u64s
                .get(&(device, parameter))
                .cloned()
                .unwrap_or_else(|| Err(ApiFailure::api("clGetDeviceInfo", -30)))
        }

        fn device_usize(&self, device: usize, parameter: u32) -> Result<usize, ApiFailure> {
            self.device_usizes
                .get(&(device, parameter))
                .cloned()
                .unwrap_or_else(|| Err(ApiFailure::api("clGetDeviceInfo", -30)))
        }

        fn device_usizes(
            &self,
            device: usize,
            parameter: u32,
            _: usize,
        ) -> Result<Vec<usize>, ApiFailure> {
            self.device_usize_arrays
                .get(&(device, parameter))
                .cloned()
                .unwrap_or_else(|| Err(ApiFailure::api("clGetDeviceInfo", -30)))
        }
    }

    fn valid_mock() -> MockApi {
        let mut api = MockApi {
            platforms: Ok(vec![1]),
            ..Default::default()
        };
        api.devices.insert(1, Ok(vec![11]));
        for (parameter, value) in [
            (CL_PLATFORM_NAME, "Fixture platform"),
            (CL_PLATFORM_VENDOR, "Fixture vendor"),
            (CL_PLATFORM_VERSION, "OpenCL 3.0 Fixture"),
            (CL_PLATFORM_PROFILE, "FULL_PROFILE"),
            (CL_PLATFORM_EXTENSIONS, "cl_khr_icd cl_khr_fp64"),
            (CL_PLATFORM_ICD_SUFFIX_KHR, "FIX"),
        ] {
            api.platform_strings
                .insert((1, parameter), Ok(value.into()));
        }
        for (parameter, value) in [
            (CL_DEVICE_NAME, "Fixture device"),
            (CL_DEVICE_VENDOR, "Fixture vendor"),
            (CL_DRIVER_VERSION, "1.2.3"),
            (CL_DEVICE_VERSION, "OpenCL 3.0"),
            (CL_DEVICE_PROFILE, "FULL_PROFILE"),
            (CL_DEVICE_EXTENSIONS, "cl_khr_fp64"),
        ] {
            api.device_strings.insert((11, parameter), Ok(value.into()));
        }
        for (parameter, value) in [
            (CL_DEVICE_VENDOR_ID, 0x1234),
            (CL_DEVICE_AVAILABLE, 1),
            (CL_DEVICE_COMPILER_AVAILABLE, 1),
            (CL_DEVICE_MAX_COMPUTE_UNITS, 16),
            (CL_DEVICE_MAX_CLOCK_FREQUENCY, 1500),
            (CL_DEVICE_MAX_WORK_ITEM_DIMENSIONS, 3),
        ] {
            api.device_u32s.insert((11, parameter), Ok(value));
        }
        for (parameter, value) in [
            (CL_DEVICE_TYPE, 4),
            (CL_DEVICE_MAX_MEM_ALLOC_SIZE, 1 << 30),
            (CL_DEVICE_GLOBAL_MEM_SIZE, 8 << 30),
            (CL_DEVICE_LOCAL_MEM_SIZE, 64 << 10),
        ] {
            api.device_u64s.insert((11, parameter), Ok(value));
        }
        api.device_usizes
            .insert((11, CL_DEVICE_MAX_WORK_GROUP_SIZE), Ok(1024));
        api.device_usize_arrays.insert(
            (11, CL_DEVICE_MAX_WORK_ITEM_SIZES),
            Ok(vec![1024, 1024, 64]),
        );
        api
    }

    #[test]
    fn normal_multiple_and_zero_observations_are_bounded() {
        let observation = collect_with_api(&valid_mock());
        assert_eq!(observation.status, AggregateStatus::Observed);
        assert_eq!(observation.platforms.len(), 1);
        assert_eq!(observation.platforms[0].devices.len(), 1);
        assert_eq!(
            observation.platforms[0].extensions,
            ["cl_khr_fp64", "cl_khr_icd"]
        );

        let mut multiple = valid_mock();
        multiple.platforms = Ok(vec![1, 2]);
        multiple.platform_strings.extend(
            valid_mock()
                .platform_strings
                .into_iter()
                .map(|((_, parameter), value)| ((2, parameter), value)),
        );
        multiple.devices.insert(2, Ok(vec![11]));
        let observation = collect_with_api(&multiple);
        assert_eq!(observation.status, AggregateStatus::Observed);
        assert_eq!(observation.platforms.len(), 2);

        let zero = MockApi {
            platforms: Ok(Vec::new()),
            ..Default::default()
        };
        assert_eq!(collect_with_api(&zero).status, AggregateStatus::NoPlatform);
    }

    #[test]
    fn api_failures_missing_devices_and_count_overflow_fail_closed() {
        let failed = MockApi {
            platforms: Err(ApiFailure::api("clGetPlatformIDs", -1001)),
            ..Default::default()
        };
        let observation = collect_with_api(&failed);
        assert_eq!(observation.status, AggregateStatus::Incomplete);
        assert_eq!(observation.diagnostics[0].kind, ProbeFailureKind::ApiError);

        let mut no_device = valid_mock();
        no_device.devices.insert(1, Ok(Vec::new()));
        let observation = collect_with_api(&no_device);
        assert_eq!(observation.status, AggregateStatus::Incomplete);
        assert_eq!(observation.diagnostics[0].kind, ProbeFailureKind::NoDevice);

        let overflow = MockApi {
            platforms: Ok((0..=MAX_PLATFORMS).collect()),
            ..Default::default()
        };
        assert_eq!(
            collect_with_api(&overflow).diagnostics[0].kind,
            ProbeFailureKind::CountOverflow
        );
    }

    #[test]
    fn malformed_strings_booleans_and_work_dimensions_fail_closed() {
        let mut huge = valid_mock();
        huge.platform_strings
            .insert((1, CL_PLATFORM_NAME), Ok("x".repeat(MAX_STRING_BYTES + 1)));
        assert_eq!(
            collect_with_api(&huge).diagnostics[0].kind,
            ProbeFailureKind::Malformed
        );

        let mut invalid_bool = valid_mock();
        invalid_bool
            .device_u32s
            .insert((11, CL_DEVICE_AVAILABLE), Ok(2));
        assert_eq!(
            collect_with_api(&invalid_bool).diagnostics[0].kind,
            ProbeFailureKind::Malformed
        );

        let mut dimensions = valid_mock();
        dimensions.device_u32s.insert(
            (11, CL_DEVICE_MAX_WORK_ITEM_DIMENSIONS),
            Ok((MAX_WORK_ITEM_DIMENSIONS + 1) as u32),
        );
        assert_eq!(
            collect_with_api(&dimensions).diagnostics[0].kind,
            ProbeFailureKind::CountOverflow
        );

        let mut huge_capability = valid_mock();
        huge_capability
            .device_u32s
            .insert((11, CL_DEVICE_MAX_COMPUTE_UNITS), Ok(u32::MAX));
        assert_eq!(
            collect_with_api(&huge_capability).diagnostics[0].kind,
            ProbeFailureKind::Malformed
        );

        let mut no_extensions = valid_mock();
        no_extensions
            .platform_strings
            .insert((1, CL_PLATFORM_EXTENSIONS), Ok(String::new()));
        no_extensions
            .device_strings
            .insert((11, CL_DEVICE_EXTENSIONS), Ok(String::new()));
        let observation = collect_with_api(&no_extensions);
        assert_eq!(observation.status, AggregateStatus::Observed);
        assert!(observation.platforms[0].extensions.is_empty());
        assert!(observation.platforms[0].devices[0].extensions.is_empty());
    }

    #[test]
    fn path_like_metadata_is_malformed_and_partial_observation_is_not_success() {
        let path_like_values = [
            r"\Users\name",
            r"C:Users\name",
            r"vendor\private",
            "vendor/private",
            r"C:\Users\name",
            "C:/Users/name",
            r"\\server\share",
            "/usr/lib/vendor",
            "C:drive-relative",
        ];
        let platform_parameters = [
            CL_PLATFORM_NAME,
            CL_PLATFORM_VENDOR,
            CL_PLATFORM_VERSION,
            CL_PLATFORM_PROFILE,
            CL_PLATFORM_EXTENSIONS,
            CL_PLATFORM_ICD_SUFFIX_KHR,
        ];
        let device_parameters = [
            CL_DEVICE_NAME,
            CL_DEVICE_VENDOR,
            CL_DRIVER_VERSION,
            CL_DEVICE_VERSION,
            CL_DEVICE_PROFILE,
            CL_DEVICE_EXTENSIONS,
        ];
        let assert_rejected = |api: &MockApi| {
            let observation = collect_with_api(api);
            assert_eq!(observation.status, AggregateStatus::Incomplete);
            assert!(observation.platforms.is_empty());
            assert_eq!(observation.diagnostics[0].kind, ProbeFailureKind::Malformed);
        };

        // The rejection is decided per value, and the parameter decides only
        // whether the value is inspected at all. Each shape is therefore
        // checked once, and each inspected parameter once, rather than as a
        // cross product that repeats the same decision.
        for value in path_like_values {
            let mut api = valid_mock();
            api.platform_strings
                .insert((1, CL_PLATFORM_NAME), Ok(value.into()));
            assert_rejected(&api);
        }
        for parameter in platform_parameters {
            let mut api = valid_mock();
            api.platform_strings
                .insert((1, parameter), Ok(r"C:\Users\name".into()));
            assert_rejected(&api);
        }
        for parameter in device_parameters {
            let mut api = valid_mock();
            api.device_strings
                .insert((11, parameter), Ok(r"C:\Users\name".into()));
            assert_rejected(&api);
        }

        let mut partial = valid_mock();
        partial.platforms = Ok(vec![1, 2]);
        partial.platform_strings.extend(
            valid_mock()
                .platform_strings
                .into_iter()
                .map(|((_, parameter), value)| ((2, parameter), value)),
        );
        partial.devices.insert(2, Ok(vec![11]));
        partial
            .platform_strings
            .insert((2, CL_PLATFORM_VENDOR), Ok(r"\Users\name".into()));
        let observation = collect_with_api(&partial);
        assert_eq!(observation.status, AggregateStatus::Incomplete);
        assert_eq!(observation.platforms.len(), 1);
        assert_eq!(observation.diagnostics[0].kind, ProbeFailureKind::Malformed);
    }

    #[test]
    fn compute_stage_matrix_and_partial_success_fail_closed() {
        let passed = collect_with_compute(
            &valid_mock(),
            &MockCompute::new([ComputeDeviceObservation::passed(QueueApi::Legacy)]),
        );
        assert!(passed.compute_ready);
        assert_eq!(
            passed.platforms[0].devices[0].compute.stage,
            ComputeStage::Passed
        );

        for stage in [
            ComputeStage::Context,
            ComputeStage::Queue,
            ComputeStage::Buffer,
            ComputeStage::Build,
            ComputeStage::Kernel,
            ComputeStage::Enqueue,
            ComputeStage::Finish,
            ComputeStage::Readback,
            ComputeStage::Mismatch,
            ComputeStage::NotAttempted,
        ] {
            let result = collect_with_compute(
                &valid_mock(),
                &MockCompute::new([if stage == ComputeStage::NotAttempted {
                    ComputeDeviceObservation::not_attempted(vec!["clCreateContext".into()])
                } else if matches!(stage, ComputeStage::Context | ComputeStage::Queue) {
                    ComputeDeviceObservation::failed(stage, Some(-5))
                } else {
                    ComputeDeviceObservation::failed_after_queue(
                        stage,
                        (stage != ComputeStage::Mismatch).then_some(-5),
                        QueueApi::Legacy,
                    )
                }]),
            );
            assert_eq!(result.status, AggregateStatus::Observed);
            assert!(!result.compute_ready);
            assert_eq!(result.platforms[0].devices[0].compute.stage, stage);
            assert_eq!(
                result.platforms[0].devices[0].compute.queue_api,
                if matches!(
                    stage,
                    ComputeStage::Buffer
                        | ComputeStage::Build
                        | ComputeStage::Kernel
                        | ComputeStage::Enqueue
                        | ComputeStage::Finish
                        | ComputeStage::Readback
                        | ComputeStage::Mismatch
                ) {
                    Some(QueueApi::Legacy)
                } else {
                    None
                }
            );
        }

        let mut two_devices = valid_mock();
        two_devices.devices.insert(1, Ok(vec![11, 11]));
        let partial = collect_with_compute(
            &two_devices,
            &MockCompute::new([
                ComputeDeviceObservation::passed(QueueApi::Legacy),
                ComputeDeviceObservation::failed_after_queue(
                    ComputeStage::Readback,
                    Some(-5),
                    QueueApi::Legacy,
                ),
            ]),
        );
        assert_eq!(partial.status, AggregateStatus::Observed);
        assert_eq!(partial.platforms[0].devices.len(), 2);
        assert!(!partial.compute_ready);
    }

    fn launch_result(classification: ExitClassification, stdout: &str) -> SecureLaunchResult {
        SecureLaunchResult {
            classification,
            exit_code: match classification {
                ExitClassification::Ok => 0,
                ExitClassification::NonzeroExit => 7,
                ExitClassification::TimeoutKilled => 0,
                ExitClassification::Crashed => 0xc0000005,
            },
            stdout: stdout.into(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            kill_reason: None,
            worker_peak_commit_bytes: None,
            peak_process_memory_bytes: None,
            peak_job_memory_bytes: None,
            process_memory_limit_bytes: 512 * 1024 * 1024,
            memory_limit_reached: false,
            dismissed_windows: Vec::new(),
            worker_freshness_warning: None,
            module_audit_warning: None,
        }
    }

    #[test]
    fn launcher_classifies_nonzero_crash_timeout_malformed_and_output_bound() {
        for (classification, expected) in [
            (
                ExitClassification::NonzeroExit,
                ProbeLaunchStatus::NonzeroExit,
            ),
            (ExitClassification::Crashed, ProbeLaunchStatus::Crash),
            (
                ExitClassification::TimeoutKilled,
                ProbeLaunchStatus::Timeout,
            ),
        ] {
            let report = report_from_launch(launch_result(classification, ""));
            assert_eq!(report.launch.status, expected);
            assert!(!report.compute_ready);
        }
        let malformed = report_from_launch(launch_result(ExitClassification::Ok, "{bad json"));
        assert_eq!(malformed.launch.status, ProbeLaunchStatus::MalformedOutput);
        assert!(!malformed.compute_ready);
        let mut truncated = launch_result(ExitClassification::Ok, "{}");
        truncated.stdout_truncated = true;
        let truncated = report_from_launch(truncated);
        assert_eq!(truncated.launch.status, ProbeLaunchStatus::OutputTruncated);
        assert!(!truncated.compute_ready);
    }

    #[test]
    fn missing_loader_and_symbols_remain_aggregate_not_candidate_binding() {
        assert_eq!(no_loader_observation().status, AggregateStatus::NoLoader);
        let missing = missing_symbol_observation(&["clGetDeviceInfo"]);
        assert_eq!(missing.status, AggregateStatus::MissingSymbol);
        assert_eq!(
            CandidateEvidenceBoundary::default().individual_icd_binding,
            "not_attempted"
        );
    }
}
