use crate::gpu_platform_collector::{
    GpuAdapterDeviceBinding, collect_gpu_platform_identity, enumerate_gpu_adapter_device_bindings,
    privacy_bounded_identity_report,
};
use crate::opencl_icd_adapter_binding::{
    OpenClIcdAdapterBinding, OpenClIcdBindingStatus, collect_opencl_icd_adapter_bindings,
};
use crate::runtime_module_identity::{
    AuthenticodeEvidence, IdentityEvidenceErrorKind, PeMachine, RuntimeModuleIdentityEvidence,
    capture_runtime_module_identity,
};
use crate::runtime_module_policy::{GpuPlatformIdentity, RuntimeBackend};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const OPENCL_DRIVER_NAME_NATIVE: &str = "OpenCLDriverName";
pub const OPENCL_DRIVER_NAME_WOW32: &str = "OpenCLDriverNameWow";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PnpOpenClSourceClass {
    DisplayAdapter,
    SoftwareComponent,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PnpOpenClArchitecture {
    Native,
    Wow32,
}

impl PnpOpenClArchitecture {
    fn value_name(self) -> &'static str {
        match self {
            Self::Native => OPENCL_DRIVER_NAME_NATIVE,
            Self::Wow32 => OPENCL_DRIVER_NAME_WOW32,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PnpOpenClClassification {
    IdentityVerified,
    MissingValue,
    WrongRegistryType,
    EmptyValue,
    MalformedMultiString,
    InvalidPath,
    MissingDll,
    InaccessibleDll,
    UnsafeDll,
    InvalidPe,
    ArchitectureMismatch,
    UntrustedDll,
    IdentityUnsupported,
    IdentityIoFailure,
    SoftwareKeyOpenFailure,
    RegistryReadFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PnpOpenClBindingStatus {
    Verified,
    UnverifiedCandidate,
    UnverifiedAdapter,
    UnverifiedNoCatalogEvidence,
    ConflictCatalogDigest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyMergeStatus {
    ExactMatch,
    NoLegacyMatch,
    NotEligible,
    AmbiguousDuplicate,
    ConflictEvidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PnpOpenClDiagnosticKind {
    AdapterEnumerationFailed,
    DisplayEnumerationFailed,
    AdapterDeviceUnmatched,
    AdapterDeviceAmbiguous,
    AdapterIdentityCollectionFailed,
    DevicePropertyReadFailed,
    PendingReboot,
    DeviceStatusFailure,
    ChildTraversalFailed,
    ChildClassReadFailed,
    NoSoftwareComponentChild,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PnpOpenClDiagnostic {
    pub source_class: Option<PnpOpenClSourceClass>,
    pub adapter_luid: Option<String>,
    pub classification: PnpOpenClDiagnosticKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PnpOpenClCandidate {
    pub source_class: PnpOpenClSourceClass,
    pub architecture: PnpOpenClArchitecture,
    pub loader_selected: bool,
    pub path: Option<PathBuf>,
    pub classification: PnpOpenClClassification,
    pub identity: Option<RuntimeModuleIdentityEvidence>,
    pub status: PnpOpenClBindingStatus,
    pub adapter: Option<GpuPlatformIdentity>,
    pub legacy_merge: LegacyMergeStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PnpOpenClCollection {
    pub candidates: Vec<PnpOpenClCandidate>,
    pub diagnostics: Vec<PnpOpenClDiagnostic>,
}

/// Collects the Khronos DCH/PnP software-key contract through supported
/// SetupAPI device APIs. Raw device instance IDs and hardware IDs are used only
/// inside the authoritative adapter/parent match and never leave this module.
pub fn collect_pnp_opencl_runtime_candidates() -> PnpOpenClCollection {
    let legacy = collect_opencl_icd_adapter_bindings();
    let (raw, diagnostics) = platform::collect_raw_candidates();
    collect_with(raw, diagnostics, &RuntimeIdentitySource, &legacy.bindings)
}

pub fn privacy_bounded_pnp_opencl_report(collection: &PnpOpenClCollection) -> serde_json::Value {
    let candidates = collection
        .candidates
        .iter()
        .map(|candidate| {
            let path_text = candidate
                .path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned());
            let identity = candidate.identity.as_ref().map(|identity| {
                serde_json::json!({
                    "size": identity.size,
                    "sha256": hex(&identity.sha256),
                    "pe_machine": pe_machine_name(identity.pe_machine),
                    "volume_serial_number": format!("{:08x}", identity.file_identity.volume_serial_number),
                    "file_index": format!("{:016x}", identity.file_identity.file_index),
                    "authenticode": match identity.authenticode {
                        AuthenticodeEvidence::Embedded => "embedded",
                        AuthenticodeEvidence::Catalog => "catalog",
                    },
                    "catalog_sha256": identity.signing_catalog_sha256.map(|digest| hex(&digest)),
                })
            });
            serde_json::json!({
                "source_class": candidate.source_class,
                "architecture": candidate.architecture,
                "loader_selected": candidate.loader_selected,
                "value_name": candidate.architecture.value_name(),
                "classification": candidate.classification,
                "path_basename": path_text.as_deref().and_then(windows_basename),
                "path_fingerprint_sha256": path_text.as_deref().map(path_fingerprint),
                "identity": identity,
                "status": candidate.status,
                "authoritative_evidence": if candidate.status == PnpOpenClBindingStatus::Verified {
                    Some("pnp_software_key_catalog_adapter_exact_match")
                } else {
                    None
                },
                "adapter": candidate.adapter.as_ref().map(privacy_bounded_identity_report),
                "legacy_merge": candidate.legacy_merge,
                "backend_ready": false,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema_version": 1,
        "contract": "windows_pnp_opencl_runtime",
        "host_architecture": host_architecture(),
        "candidates": candidates,
        "diagnostics": collection.diagnostics,
    })
}

#[derive(Clone, Debug)]
struct RawPnpCandidate {
    source_class: PnpOpenClSourceClass,
    architecture: PnpOpenClArchitecture,
    loader_selected: bool,
    value: RawPnpValue,
    adapter: Option<GpuPlatformIdentity>,
}

#[derive(Clone, Debug)]
enum RawPnpValue {
    Missing,
    WrongType,
    Empty,
    MalformedMultiString,
    Path(String),
    SoftwareKeyOpenFailure,
    ReadFailure,
}

#[derive(Clone, Copy, Debug)]
enum IdentityFailure {
    InvalidPath,
    Missing,
    Inaccessible,
    Unsafe,
    InvalidPe,
    Untrusted,
    Unsupported,
    Io,
}

trait IdentitySource {
    fn capture(&self, path: &Path) -> Result<RuntimeModuleIdentityEvidence, IdentityFailure>;
}

struct RuntimeIdentitySource;

impl IdentitySource for RuntimeIdentitySource {
    fn capture(&self, path: &Path) -> Result<RuntimeModuleIdentityEvidence, IdentityFailure> {
        capture_runtime_module_identity(path).map_err(|error| match error.kind {
            IdentityEvidenceErrorKind::InvalidPath => IdentityFailure::InvalidPath,
            IdentityEvidenceErrorKind::UnsafeFile => IdentityFailure::Unsafe,
            IdentityEvidenceErrorKind::InvalidPe => IdentityFailure::InvalidPe,
            IdentityEvidenceErrorKind::Unsupported => IdentityFailure::Unsupported,
            IdentityEvidenceErrorKind::UntrustedSignature => IdentityFailure::Untrusted,
            IdentityEvidenceErrorKind::Io => classify_identity_io(path),
        })
    }
}

fn classify_identity_io(path: &Path) -> IdentityFailure {
    match std::fs::metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => IdentityFailure::Missing,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            IdentityFailure::Inaccessible
        }
        _ => IdentityFailure::Io,
    }
}

fn collect_with<I: IdentitySource>(
    raw: Vec<RawPnpCandidate>,
    diagnostics: Vec<PnpOpenClDiagnostic>,
    identities: &I,
    legacy: &[OpenClIcdAdapterBinding],
) -> PnpOpenClCollection {
    let mut candidates = raw
        .into_iter()
        .map(|raw| classify_candidate(raw, identities, legacy))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.source_class
            .cmp(&right.source_class)
            .then_with(|| {
                left.adapter
                    .as_ref()
                    .map(|adapter| adapter.adapter_luid)
                    .cmp(&right.adapter.as_ref().map(|adapter| adapter.adapter_luid))
            })
            .then_with(|| left.architecture.cmp(&right.architecture))
            .then_with(|| {
                left.path
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_ascii_lowercase())
                    .cmp(
                        &right
                            .path
                            .as_ref()
                            .map(|path| path.to_string_lossy().to_ascii_lowercase()),
                    )
            })
    });
    PnpOpenClCollection {
        candidates,
        diagnostics,
    }
}

fn classify_candidate<I: IdentitySource>(
    raw: RawPnpCandidate,
    identities: &I,
    legacy: &[OpenClIcdAdapterBinding],
) -> PnpOpenClCandidate {
    let path = match &raw.value {
        RawPnpValue::Path(value) if valid_windows_dll_path(value) => Some(PathBuf::from(value)),
        _ => None,
    };
    let mut classification = match &raw.value {
        RawPnpValue::Missing => PnpOpenClClassification::MissingValue,
        RawPnpValue::WrongType => PnpOpenClClassification::WrongRegistryType,
        RawPnpValue::Empty => PnpOpenClClassification::EmptyValue,
        RawPnpValue::MalformedMultiString => PnpOpenClClassification::MalformedMultiString,
        RawPnpValue::Path(_) if path.is_none() => PnpOpenClClassification::InvalidPath,
        RawPnpValue::SoftwareKeyOpenFailure => PnpOpenClClassification::SoftwareKeyOpenFailure,
        RawPnpValue::ReadFailure => PnpOpenClClassification::RegistryReadFailure,
        RawPnpValue::Path(_) => PnpOpenClClassification::IdentityIoFailure,
    };
    let mut identity = None;
    if let Some(candidate_path) = path.as_deref() {
        match identities.capture(candidate_path) {
            Ok(observed) if machine_matches_architecture(observed.pe_machine, raw.architecture) => {
                classification = PnpOpenClClassification::IdentityVerified;
                identity = Some(observed);
            }
            Ok(_) => classification = PnpOpenClClassification::ArchitectureMismatch,
            Err(IdentityFailure::InvalidPath) => {
                classification = PnpOpenClClassification::InvalidPath
            }
            Err(IdentityFailure::Missing) => classification = PnpOpenClClassification::MissingDll,
            Err(IdentityFailure::Inaccessible) => {
                classification = PnpOpenClClassification::InaccessibleDll
            }
            Err(IdentityFailure::Unsafe) => classification = PnpOpenClClassification::UnsafeDll,
            Err(IdentityFailure::InvalidPe) => classification = PnpOpenClClassification::InvalidPe,
            Err(IdentityFailure::Untrusted) => {
                classification = PnpOpenClClassification::UntrustedDll
            }
            Err(IdentityFailure::Unsupported) => {
                classification = PnpOpenClClassification::IdentityUnsupported
            }
            Err(IdentityFailure::Io) => classification = PnpOpenClClassification::IdentityIoFailure,
        }
    }
    let (status, adapter) = bind_candidate(
        raw.loader_selected,
        classification,
        identity.as_ref(),
        raw.adapter,
    );
    let legacy_merge = merge_legacy(status, identity.as_ref(), adapter.as_ref(), legacy);
    PnpOpenClCandidate {
        source_class: raw.source_class,
        architecture: raw.architecture,
        loader_selected: raw.loader_selected,
        path,
        classification,
        identity,
        status,
        adapter,
        legacy_merge,
    }
}

fn bind_candidate(
    loader_selected: bool,
    classification: PnpOpenClClassification,
    identity: Option<&RuntimeModuleIdentityEvidence>,
    adapter: Option<GpuPlatformIdentity>,
) -> (PnpOpenClBindingStatus, Option<GpuPlatformIdentity>) {
    let Some(identity) = identity
        .filter(|_| loader_selected && classification == PnpOpenClClassification::IdentityVerified)
    else {
        return (PnpOpenClBindingStatus::UnverifiedCandidate, None);
    };
    let Some(adapter) = adapter else {
        return (PnpOpenClBindingStatus::UnverifiedAdapter, None);
    };
    if identity.authenticode != AuthenticodeEvidence::Catalog
        || identity.signing_catalog_sha256.is_none()
    {
        return (PnpOpenClBindingStatus::UnverifiedNoCatalogEvidence, None);
    }
    if identity.signing_catalog_sha256 != Some(adapter.driver_catalog_sha256) {
        return (PnpOpenClBindingStatus::ConflictCatalogDigest, None);
    }
    (PnpOpenClBindingStatus::Verified, Some(adapter))
}

fn merge_legacy(
    status: PnpOpenClBindingStatus,
    identity: Option<&RuntimeModuleIdentityEvidence>,
    adapter: Option<&GpuPlatformIdentity>,
    legacy: &[OpenClIcdAdapterBinding],
) -> LegacyMergeStatus {
    if status != PnpOpenClBindingStatus::Verified {
        return LegacyMergeStatus::NotEligible;
    }
    let (Some(identity), Some(adapter)) = (identity, adapter) else {
        return LegacyMergeStatus::NotEligible;
    };
    let identity_matches = legacy
        .iter()
        .filter(|binding| {
            binding
                .candidate
                .identity
                .as_ref()
                .is_some_and(|legacy_identity| same_module_identity(identity, legacy_identity))
        })
        .collect::<Vec<_>>();
    let exact = identity_matches
        .iter()
        .filter(|binding| {
            binding.status == OpenClIcdBindingStatus::Verified
                && binding.adapter.as_ref() == Some(adapter)
                && binding
                    .candidate
                    .identity
                    .as_ref()
                    .and_then(|legacy_identity| legacy_identity.signing_catalog_sha256)
                    == identity.signing_catalog_sha256
        })
        .count();
    match exact {
        1 => LegacyMergeStatus::ExactMatch,
        count if count > 1 => LegacyMergeStatus::AmbiguousDuplicate,
        _ if identity_matches.is_empty() => LegacyMergeStatus::NoLegacyMatch,
        _ => LegacyMergeStatus::ConflictEvidence,
    }
}

fn same_module_identity(
    left: &RuntimeModuleIdentityEvidence,
    right: &RuntimeModuleIdentityEvidence,
) -> bool {
    left.sha256 == right.sha256
        && left.file_identity == right.file_identity
        && left.signing_catalog_sha256 == right.signing_catalog_sha256
}

fn valid_windows_dll_path(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 32_767
        || value.chars().any(char::is_control)
        || !value.to_ascii_lowercase().ends_with(".dll")
    {
        return false;
    }
    let absolute_drive = value.as_bytes().get(1) == Some(&b':')
        && value
            .as_bytes()
            .get(2)
            .is_some_and(|separator| matches!(separator, b'\\' | b'/'));
    let absolute_unc = value.starts_with(r"\\") && value[2..].contains(['\\', '/']);
    (absolute_drive || absolute_unc)
        && !value
            .split(['\\', '/'])
            .any(|component| component == "." || component == "..")
}

fn machine_matches_architecture(machine: PeMachine, architecture: PnpOpenClArchitecture) -> bool {
    match architecture {
        PnpOpenClArchitecture::Wow32 => machine == PeMachine::I386,
        PnpOpenClArchitecture::Native => match host_architecture() {
            "x86_64" => machine == PeMachine::Amd64,
            "aarch64" => machine == PeMachine::Arm64,
            "x86" => machine == PeMachine::I386,
            "arm" => machine == PeMachine::Arm,
            _ => false,
        },
    }
}

fn host_architecture() -> &'static str {
    std::env::consts::ARCH
}

const REG_SZ_TYPE: u32 = 1;
const REG_MULTI_SZ_TYPE: u32 = 7;

fn parse_registry_strings(value_type: u32, data: &[u8]) -> Vec<(bool, RawPnpValue)> {
    if value_type != REG_SZ_TYPE && value_type != REG_MULTI_SZ_TYPE {
        return vec![(true, RawPnpValue::WrongType)];
    }
    if data.len() < 2 || data.len() % 2 != 0 {
        return vec![(true, RawPnpValue::MalformedMultiString)];
    }
    let words = data
        .chunks_exact(2)
        .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    if value_type == REG_SZ_TYPE {
        let Some(terminator) = words.iter().position(|word| *word == 0) else {
            return vec![(true, RawPnpValue::MalformedMultiString)];
        };
        if terminator + 1 != words.len() {
            return vec![(true, RawPnpValue::MalformedMultiString)];
        }
        return match String::from_utf16(&words[..terminator]) {
            Ok(value) if value.is_empty() => vec![(true, RawPnpValue::Empty)],
            Ok(value) => vec![(true, RawPnpValue::Path(value))],
            Err(_) => vec![(true, RawPnpValue::MalformedMultiString)],
        };
    }

    if words.len() < 2
        || words[words.len() - 2..] != [0, 0]
        || words[..words.len() - 2]
            .windows(2)
            .any(|pair| pair == [0, 0])
    {
        return vec![(true, RawPnpValue::MalformedMultiString)];
    }
    if words == [0, 0] {
        return vec![(true, RawPnpValue::Empty)];
    }
    let mut decoded = Vec::new();
    for value in words[..words.len() - 2].split(|word| *word == 0) {
        if value.is_empty() {
            return vec![(true, RawPnpValue::MalformedMultiString)];
        }
        match String::from_utf16(value) {
            Ok(value) if !value.is_empty() => decoded.push(value),
            _ => return vec![(true, RawPnpValue::MalformedMultiString)],
        }
    }
    if decoded.is_empty() {
        return vec![(true, RawPnpValue::Empty)];
    }
    decoded
        .into_iter()
        .enumerate()
        .map(|(index, value)| (index == 0, RawPnpValue::Path(value)))
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeviceProbeResult {
    Valid,
    PendingReboot,
    StatusFailure,
}

fn classify_device_status(api_success: bool, status: u32, problem: u32) -> DeviceProbeResult {
    const DN_NEED_RESTART_VALUE: u32 = 0x100;
    const DN_HAS_PROBLEM_VALUE: u32 = 0x400;
    const CM_PROB_NEED_RESTART_VALUE: u32 = 14;

    if !api_success {
        DeviceProbeResult::StatusFailure
    } else if status & DN_NEED_RESTART_VALUE != 0
        || (status & DN_HAS_PROBLEM_VALUE != 0 && problem == CM_PROB_NEED_RESTART_VALUE)
    {
        DeviceProbeResult::PendingReboot
    } else if status & DN_HAS_PROBLEM_VALUE != 0 {
        DeviceProbeResult::StatusFailure
    } else {
        DeviceProbeResult::Valid
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChildTraversalResult {
    Item,
    End,
    Failure,
}

fn classify_child_traversal(configret: u32) -> ChildTraversalResult {
    match configret {
        0 => ChildTraversalResult::Item,
        13 => ChildTraversalResult::End,
        _ => ChildTraversalResult::Failure,
    }
}

fn windows_basename(value: &str) -> Option<&str> {
    value
        .rsplit(['\\', '/'])
        .find(|component| !component.is_empty())
}

fn path_fingerprint(value: &str) -> String {
    hex(&Sha256::digest(value.to_ascii_lowercase().as_bytes()))
}

fn pe_machine_name(machine: PeMachine) -> String {
    match machine {
        PeMachine::I386 => "i386".into(),
        PeMachine::Amd64 => "amd64".into(),
        PeMachine::Arm => "arm".into(),
        PeMachine::Arm64 => "arm64".into(),
        PeMachine::Unknown(value) => format!("unknown_{value:04x}"),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub fn collect_raw_candidates() -> (Vec<RawPnpCandidate>, Vec<PnpOpenClDiagnostic>) {
        (
            Vec::new(),
            vec![PnpOpenClDiagnostic {
                source_class: None,
                adapter_luid: None,
                classification: PnpOpenClDiagnosticKind::AdapterEnumerationFailed,
            }],
        )
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::mem::{size_of, zeroed};
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        CM_DEVNODE_STATUS_FLAGS, CM_DRP_CLASSGUID, CM_Get_Child, CM_Get_DevNode_Registry_PropertyW,
        CM_Get_DevNode_Status, CM_Get_Sibling, CM_Open_DevNode_Key, CM_PROB, CM_REGISTRY_SOFTWARE,
        CR_SUCCESS, DIGCF_PRESENT, GUID_DEVCLASS_DISPLAY, HDEVINFO, RegDisposition_OpenExisting,
        SETUP_DI_REGISTRY_PROPERTY, SP_DEVINFO_DATA, SPDRP_ADDRESS, SPDRP_BUSNUMBER,
        SPDRP_HARDWAREID, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
        SetupDiGetClassDevsW, SetupDiGetDeviceRegistryPropertyW,
    };
    use windows::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, HWND,
    };
    use windows::Win32::System::Registry::{
        HKEY, KEY_QUERY_VALUE, REG_VALUE_TYPE, RegCloseKey, RegQueryValueExW,
    };
    use windows::core::{GUID, PCWSTR, w};

    const MAX_DEVICES: u32 = 4096;
    const MAX_PROPERTY_BYTES: usize = 16 * 1024;
    const SOFTWARE_COMPONENT_CLASS: &str = "{5c4c3332-344d-483c-8739-259e934c9cc8}";

    struct DeviceInfoSet(HDEVINFO);

    impl Drop for DeviceInfoSet {
        fn drop(&mut self) {
            unsafe {
                let _ = SetupDiDestroyDeviceInfoList(self.0);
            }
        }
    }

    struct RegistryKey(HKEY);

    impl Drop for RegistryKey {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    pub fn collect_raw_candidates() -> (Vec<RawPnpCandidate>, Vec<PnpOpenClDiagnostic>) {
        let mut diagnostics = Vec::new();
        let adapter_bindings = match enumerate_gpu_adapter_device_bindings() {
            Ok(bindings) => bindings,
            Err(_) => {
                return (
                    Vec::new(),
                    vec![PnpOpenClDiagnostic {
                        source_class: None,
                        adapter_luid: None,
                        classification: PnpOpenClDiagnosticKind::AdapterEnumerationFailed,
                    }],
                );
            }
        };
        let mut platform_identities = BTreeMap::new();
        for binding in &adapter_bindings {
            match collect_gpu_platform_identity(
                binding.adapter.adapter_luid,
                RuntimeBackend::Opencl,
            ) {
                Ok(identity) => {
                    platform_identities.insert(binding.adapter.adapter_luid, identity);
                }
                Err(_) => diagnostics.push(PnpOpenClDiagnostic {
                    source_class: Some(PnpOpenClSourceClass::DisplayAdapter),
                    adapter_luid: Some(format!("{:016x}", binding.adapter.adapter_luid)),
                    classification: PnpOpenClDiagnosticKind::AdapterIdentityCollectionFailed,
                }),
            }
        }
        let display_set = match class_devices(&GUID_DEVCLASS_DISPLAY) {
            Ok(set) => set,
            Err(_) => {
                diagnostics.push(PnpOpenClDiagnostic {
                    source_class: Some(PnpOpenClSourceClass::DisplayAdapter),
                    adapter_luid: None,
                    classification: PnpOpenClDiagnosticKind::DisplayEnumerationFailed,
                });
                return (Vec::new(), diagnostics);
            }
        };
        let mut raw = Vec::new();
        for device in enum_devices(
            &display_set,
            PnpOpenClSourceClass::DisplayAdapter,
            &mut diagnostics,
        ) {
            let matches = matching_adapters(display_set.0, &device, &adapter_bindings);
            let binding = match matches.as_slice() {
                [binding] => **binding,
                [] => {
                    diagnostics.push(PnpOpenClDiagnostic {
                        source_class: Some(PnpOpenClSourceClass::DisplayAdapter),
                        adapter_luid: None,
                        classification: PnpOpenClDiagnosticKind::AdapterDeviceUnmatched,
                    });
                    continue;
                }
                _ => {
                    diagnostics.push(PnpOpenClDiagnostic {
                        source_class: Some(PnpOpenClSourceClass::DisplayAdapter),
                        adapter_luid: None,
                        classification: PnpOpenClDiagnosticKind::AdapterDeviceAmbiguous,
                    });
                    continue;
                }
            };
            let adapter_luid = Some(format!("{:016x}", binding.adapter.adapter_luid));
            match device_status(device.DevInst) {
                DeviceProbeResult::Valid => {}
                DeviceProbeResult::PendingReboot => {
                    diagnostics.push(PnpOpenClDiagnostic {
                        source_class: Some(PnpOpenClSourceClass::DisplayAdapter),
                        adapter_luid,
                        classification: PnpOpenClDiagnosticKind::PendingReboot,
                    });
                    continue;
                }
                DeviceProbeResult::StatusFailure => {
                    diagnostics.push(PnpOpenClDiagnostic {
                        source_class: Some(PnpOpenClSourceClass::DisplayAdapter),
                        adapter_luid,
                        classification: PnpOpenClDiagnosticKind::DeviceStatusFailure,
                    });
                    continue;
                }
            }
            let platform = platform_identities
                .get(&binding.adapter.adapter_luid)
                .cloned();
            raw.extend(read_device_candidates(
                device.DevInst,
                PnpOpenClSourceClass::DisplayAdapter,
                platform.clone(),
            ));
            collect_software_component_children(
                device.DevInst,
                platform,
                &mut raw,
                &mut diagnostics,
            );
        }
        (raw, diagnostics)
    }

    fn class_devices(class: &GUID) -> windows::core::Result<DeviceInfoSet> {
        unsafe { SetupDiGetClassDevsW(Some(class), PCWSTR::null(), HWND::default(), DIGCF_PRESENT) }
            .map(DeviceInfoSet)
    }

    fn enum_devices(
        set: &DeviceInfoSet,
        source_class: PnpOpenClSourceClass,
        diagnostics: &mut Vec<PnpOpenClDiagnostic>,
    ) -> Vec<SP_DEVINFO_DATA> {
        let mut devices = Vec::new();
        for index in 0..MAX_DEVICES {
            let mut device = SP_DEVINFO_DATA {
                cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
                ..unsafe { zeroed() }
            };
            match unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut device) } {
                Ok(()) => devices.push(device),
                Err(error) if error.code() == ERROR_NO_MORE_ITEMS.to_hresult() => return devices,
                Err(_) => {
                    diagnostics.push(PnpOpenClDiagnostic {
                        source_class: Some(source_class),
                        adapter_luid: None,
                        classification: PnpOpenClDiagnosticKind::DevicePropertyReadFailed,
                    });
                    return devices;
                }
            }
        }
        diagnostics.push(PnpOpenClDiagnostic {
            source_class: Some(source_class),
            adapter_luid: None,
            classification: PnpOpenClDiagnosticKind::DevicePropertyReadFailed,
        });
        devices
    }

    fn matching_adapters<'a>(
        set: HDEVINFO,
        device: &SP_DEVINFO_DATA,
        adapters: &'a [GpuAdapterDeviceBinding],
    ) -> Vec<&'a GpuAdapterDeviceBinding> {
        let Ok(bus_number) = property_u32(set, device, SPDRP_BUSNUMBER) else {
            return Vec::new();
        };
        let Ok(address) = property_u32(set, device, SPDRP_ADDRESS) else {
            return Vec::new();
        };
        let Ok(hardware_ids) = property_strings(set, device, SPDRP_HARDWAREID) else {
            return Vec::new();
        };
        adapters
            .iter()
            .filter(|binding| {
                binding.bus_number == bus_number
                    && ((binding.device_number << 16) | binding.function_number) == address
                    && hardware_ids
                        .iter()
                        .any(|id| hardware_id_matches(id, binding.adapter))
            })
            .collect()
    }

    fn read_device_candidates(
        devinst: u32,
        source_class: PnpOpenClSourceClass,
        adapter: Option<GpuPlatformIdentity>,
    ) -> Vec<RawPnpCandidate> {
        let mut key = HKEY::default();
        let result = unsafe {
            CM_Open_DevNode_Key(
                devinst,
                KEY_QUERY_VALUE.0,
                0,
                RegDisposition_OpenExisting,
                &mut key,
                CM_REGISTRY_SOFTWARE,
            )
        };
        if result != CR_SUCCESS {
            return architectures()
                .map(|architecture| RawPnpCandidate {
                    source_class,
                    architecture,
                    loader_selected: true,
                    value: RawPnpValue::SoftwareKeyOpenFailure,
                    adapter: adapter.clone(),
                })
                .collect();
        }
        let key = RegistryKey(key);
        architectures()
            .flat_map(|architecture| {
                registry_values(key.0, architecture).into_iter().map({
                    let adapter = adapter.clone();
                    move |(loader_selected, value)| RawPnpCandidate {
                        source_class,
                        architecture,
                        loader_selected,
                        value,
                        adapter: adapter.clone(),
                    }
                })
            })
            .collect()
    }

    fn architectures() -> impl Iterator<Item = PnpOpenClArchitecture> {
        [PnpOpenClArchitecture::Native, PnpOpenClArchitecture::Wow32].into_iter()
    }

    fn registry_values(key: HKEY, architecture: PnpOpenClArchitecture) -> Vec<(bool, RawPnpValue)> {
        let name = match architecture {
            PnpOpenClArchitecture::Native => w!("OpenCLDriverName"),
            PnpOpenClArchitecture::Wow32 => w!("OpenCLDriverNameWow"),
        };
        let mut value_type = REG_VALUE_TYPE(0);
        let mut size = 0u32;
        let status = unsafe {
            RegQueryValueExW(
                key,
                name,
                None,
                Some(&mut value_type),
                None,
                Some(&mut size),
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return vec![(true, RawPnpValue::Missing)];
        }
        if status == ERROR_ACCESS_DENIED {
            return vec![(true, RawPnpValue::ReadFailure)];
        }
        if !status.is_ok() {
            return vec![(true, RawPnpValue::ReadFailure)];
        }
        if size < 2 || size as usize > MAX_PROPERTY_BYTES || size % 2 != 0 {
            return vec![(true, RawPnpValue::MalformedMultiString)];
        }
        let mut bytes = vec![0u8; size as usize];
        let status = unsafe {
            RegQueryValueExW(
                key,
                name,
                None,
                Some(&mut value_type),
                Some(bytes.as_mut_ptr()),
                Some(&mut size),
            )
        };
        if status == ERROR_MORE_DATA || !status.is_ok() {
            return vec![(true, RawPnpValue::ReadFailure)];
        }
        bytes.truncate((size as usize).min(bytes.len()));
        parse_registry_strings(value_type.0, &bytes)
    }

    fn device_status(devinst: u32) -> DeviceProbeResult {
        let mut status = CM_DEVNODE_STATUS_FLAGS(0);
        let mut problem = CM_PROB(0);
        let result = unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, devinst, 0) };
        classify_device_status(result == CR_SUCCESS, status.0, problem.0)
    }

    fn device_class_guid(devinst: u32) -> Result<String, ()> {
        let mut property_type = 0u32;
        let mut bytes = vec![0u8; MAX_PROPERTY_BYTES];
        let mut required = bytes.len() as u32;
        let result = unsafe {
            CM_Get_DevNode_Registry_PropertyW(
                devinst,
                CM_DRP_CLASSGUID,
                Some(&mut property_type),
                Some(bytes.as_mut_ptr().cast()),
                &mut required,
                0,
            )
        };
        if result != CR_SUCCESS
            || property_type != windows::Win32::System::Registry::REG_SZ.0
            || required < 2
            || required as usize > bytes.len()
            || required % 2 != 0
        {
            return Err(());
        }
        let words = bytes[..required as usize]
            .chunks_exact(2)
            .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
            .take_while(|word| *word != 0)
            .collect::<Vec<_>>();
        String::from_utf16(&words).map_err(|_| ())
    }

    fn collect_software_component_children(
        parent: u32,
        adapter: Option<GpuPlatformIdentity>,
        raw: &mut Vec<RawPnpCandidate>,
        diagnostics: &mut Vec<PnpOpenClDiagnostic>,
    ) {
        let adapter_luid = adapter
            .as_ref()
            .map(|identity| format!("{:016x}", identity.adapter_luid));
        let mut child = 0u32;
        let first = unsafe { CM_Get_Child(&mut child, parent, 0) };
        if classify_child_traversal(first.0) == ChildTraversalResult::End {
            diagnostics.push(PnpOpenClDiagnostic {
                source_class: Some(PnpOpenClSourceClass::SoftwareComponent),
                adapter_luid,
                classification: PnpOpenClDiagnosticKind::NoSoftwareComponentChild,
            });
            return;
        }
        if classify_child_traversal(first.0) == ChildTraversalResult::Failure {
            diagnostics.push(PnpOpenClDiagnostic {
                source_class: Some(PnpOpenClSourceClass::SoftwareComponent),
                adapter_luid,
                classification: PnpOpenClDiagnosticKind::ChildTraversalFailed,
            });
            return;
        }

        let mut found = false;
        let mut traversal_complete = true;
        let mut child_raw = Vec::new();
        loop {
            match device_class_guid(child) {
                Ok(class_guid) if class_guid.eq_ignore_ascii_case(SOFTWARE_COMPONENT_CLASS) => {
                    found = true;
                    match device_status(child) {
                        DeviceProbeResult::Valid => child_raw.extend(read_device_candidates(
                            child,
                            PnpOpenClSourceClass::SoftwareComponent,
                            adapter.clone(),
                        )),
                        DeviceProbeResult::PendingReboot => diagnostics.push(PnpOpenClDiagnostic {
                            source_class: Some(PnpOpenClSourceClass::SoftwareComponent),
                            adapter_luid: adapter_luid.clone(),
                            classification: PnpOpenClDiagnosticKind::PendingReboot,
                        }),
                        DeviceProbeResult::StatusFailure => diagnostics.push(PnpOpenClDiagnostic {
                            source_class: Some(PnpOpenClSourceClass::SoftwareComponent),
                            adapter_luid: adapter_luid.clone(),
                            classification: PnpOpenClDiagnosticKind::DeviceStatusFailure,
                        }),
                    }
                }
                Ok(_) => {}
                Err(()) => {
                    traversal_complete = false;
                    diagnostics.push(PnpOpenClDiagnostic {
                        source_class: Some(PnpOpenClSourceClass::SoftwareComponent),
                        adapter_luid: adapter_luid.clone(),
                        classification: PnpOpenClDiagnosticKind::ChildClassReadFailed,
                    });
                }
            }

            let current = child;
            let next = unsafe { CM_Get_Sibling(&mut child, current, 0) };
            if classify_child_traversal(next.0) == ChildTraversalResult::End {
                break;
            }
            if classify_child_traversal(next.0) == ChildTraversalResult::Failure {
                traversal_complete = false;
                diagnostics.push(PnpOpenClDiagnostic {
                    source_class: Some(PnpOpenClSourceClass::SoftwareComponent),
                    adapter_luid: adapter_luid.clone(),
                    classification: PnpOpenClDiagnosticKind::ChildTraversalFailed,
                });
                break;
            }
        }
        if traversal_complete {
            raw.extend(child_raw);
        }
        if !found {
            diagnostics.push(PnpOpenClDiagnostic {
                source_class: Some(PnpOpenClSourceClass::SoftwareComponent),
                adapter_luid,
                classification: PnpOpenClDiagnosticKind::NoSoftwareComponentChild,
            });
        }
    }

    fn property_u32(
        set: HDEVINFO,
        device: &SP_DEVINFO_DATA,
        property: SETUP_DI_REGISTRY_PROPERTY,
    ) -> windows::core::Result<u32> {
        let mut bytes = [0u8; 4];
        let mut value_type = 0u32;
        let mut required = 0u32;
        unsafe {
            SetupDiGetDeviceRegistryPropertyW(
                set,
                device,
                property,
                Some(&mut value_type),
                Some(&mut bytes),
                Some(&mut required),
            )
        }?;
        if required != 4 || value_type != windows::Win32::System::Registry::REG_DWORD.0 {
            return Err(windows::core::Error::from_win32());
        }
        Ok(u32::from_ne_bytes(bytes))
    }

    fn property_strings(
        set: HDEVINFO,
        device: &SP_DEVINFO_DATA,
        property: SETUP_DI_REGISTRY_PROPERTY,
    ) -> windows::core::Result<Vec<String>> {
        let mut bytes = vec![0u8; MAX_PROPERTY_BYTES];
        let mut value_type = 0u32;
        let mut required = 0u32;
        unsafe {
            SetupDiGetDeviceRegistryPropertyW(
                set,
                device,
                property,
                Some(&mut value_type),
                Some(&mut bytes),
                Some(&mut required),
            )
        }?;
        if required as usize > bytes.len()
            || required % 2 != 0
            || value_type != windows::Win32::System::Registry::REG_MULTI_SZ.0
        {
            return Err(windows::core::Error::from_win32());
        }
        let words = bytes[..required as usize]
            .chunks_exact(2)
            .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        words
            .split(|word| *word == 0)
            .filter(|value| !value.is_empty())
            .map(|value| String::from_utf16(value).map_err(|_| windows::core::Error::from_win32()))
            .collect()
    }

    fn hardware_id_matches(
        value: &str,
        expected: crate::gpu_platform_collector::GpuAdapterObservation,
    ) -> bool {
        let value = value.to_ascii_lowercase();
        value.starts_with("pci\\")
            && value.contains(&format!("ven_{:04x}", expected.pci_vendor_id))
            && value.contains(&format!("dev_{:04x}", expected.pci_device_id))
            && value.contains(&format!("subsys_{:08x}", expected.pci_subsystem_id))
            && value.contains(&format!("rev_{:02x}", expected.pci_revision_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opencl_icd_collector::{
        OpenClIcdCandidate, OpenClIcdClassification, OpenClRegistryView,
    };
    use crate::runtime_module_identity::FileIdentity;

    struct FixtureIdentities {
        outcomes: BTreeMap<String, Result<RuntimeModuleIdentityEvidence, IdentityFailure>>,
    }

    impl IdentitySource for FixtureIdentities {
        fn capture(&self, path: &Path) -> Result<RuntimeModuleIdentityEvidence, IdentityFailure> {
            self.outcomes
                .get(&path.to_string_lossy().to_ascii_lowercase())
                .cloned()
                .unwrap_or(Err(IdentityFailure::Io))
        }
    }

    fn platform(luid: u64, catalog: u8) -> GpuPlatformIdentity {
        GpuPlatformIdentity {
            backend: RuntimeBackend::Opencl,
            adapter_luid: luid,
            pci_vendor_id: 0x10de,
            pci_device_id: 0x2684,
            pci_subsystem_id: 0x1458_4100,
            pci_revision_id: 0xa1,
            driver_inf: "oem42.inf".into(),
            driver_catalog_sha256: [catalog; 32],
            driver_version: "32.0.15.9999".into(),
            os_build: 26100,
        }
    }

    fn identity(
        path: &str,
        machine: PeMachine,
        module: u8,
        catalog: Option<u8>,
    ) -> RuntimeModuleIdentityEvidence {
        RuntimeModuleIdentityEvidence {
            canonical_path: PathBuf::from(path),
            size: 4096,
            sha256: [module; 32],
            pe_machine: machine,
            file_identity: FileIdentity {
                volume_serial_number: 0x1234_5678,
                file_index: 0x1234_5678_90ab_cdef,
            },
            authenticode: if catalog.is_some() {
                AuthenticodeEvidence::Catalog
            } else {
                AuthenticodeEvidence::Embedded
            },
            signing_catalog_sha256: catalog.map(|seed| [seed; 32]),
        }
    }

    fn native_machine() -> PeMachine {
        match host_architecture() {
            "x86_64" => PeMachine::Amd64,
            "aarch64" => PeMachine::Arm64,
            "x86" => PeMachine::I386,
            "arm" => PeMachine::Arm,
            _ => PeMachine::Unknown(0),
        }
    }

    fn raw(
        source_class: PnpOpenClSourceClass,
        architecture: PnpOpenClArchitecture,
        value: RawPnpValue,
        adapter: Option<GpuPlatformIdentity>,
    ) -> RawPnpCandidate {
        RawPnpCandidate {
            source_class,
            architecture,
            loader_selected: true,
            value,
            adapter,
        }
    }

    fn utf16_bytes(words: &[u16]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_ne_bytes()).collect()
    }

    fn multi_sz(values: &[&str]) -> Vec<u8> {
        let mut words = Vec::new();
        for value in values {
            words.extend(value.encode_utf16());
            words.push(0);
        }
        words.push(0);
        utf16_bytes(&words)
    }

    fn legacy(
        identity: RuntimeModuleIdentityEvidence,
        adapter: GpuPlatformIdentity,
    ) -> OpenClIcdAdapterBinding {
        OpenClIcdAdapterBinding {
            candidate: OpenClIcdCandidate {
                registry_view: OpenClRegistryView::Registry64,
                path: Some(identity.canonical_path.clone()),
                enabled: Some(true),
                classification: OpenClIcdClassification::IdentityVerified,
                identity: Some(identity),
            },
            status: OpenClIcdBindingStatus::Verified,
            adapter: Some(adapter),
        }
    }

    #[test]
    fn registry_string_parser_matches_khronos_native_and_multi_sz_selection() {
        let path = r"C:\Vendor\opencl.dll";
        let mut reg_sz = path.encode_utf16().collect::<Vec<_>>();
        reg_sz.push(0);
        let parsed = parse_registry_strings(REG_SZ_TYPE, &utf16_bytes(&reg_sz));
        assert!(matches!(
            parsed.as_slice(),
            [(true, RawPnpValue::Path(value))] if value == path
        ));

        let second = r"C:\Vendor\diagnostic-only.dll";
        let parsed = parse_registry_strings(REG_MULTI_SZ_TYPE, &multi_sz(&[path, second]));
        assert!(matches!(
            parsed.as_slice(),
            [
                (true, RawPnpValue::Path(first)),
                (false, RawPnpValue::Path(extra))
            ] if first == path && extra == second
        ));
    }

    #[test]
    fn registry_string_parser_rejects_empty_malformed_and_unterminated_values() {
        assert!(matches!(
            parse_registry_strings(REG_MULTI_SZ_TYPE, &utf16_bytes(&[0, 0])).as_slice(),
            [(true, RawPnpValue::Empty)]
        ));
        assert!(matches!(
            parse_registry_strings(REG_MULTI_SZ_TYPE, &utf16_bytes(&[b'a' as u16, 0])).as_slice(),
            [(true, RawPnpValue::MalformedMultiString)]
        ));
        assert!(matches!(
            parse_registry_strings(REG_MULTI_SZ_TYPE, &[0xff]).as_slice(),
            [(true, RawPnpValue::MalformedMultiString)]
        ));
        assert!(matches!(
            parse_registry_strings(REG_SZ_TYPE, &utf16_bytes(&[0xd800, 0])).as_slice(),
            [(true, RawPnpValue::MalformedMultiString)]
        ));
        assert!(matches!(
            parse_registry_strings(4, &utf16_bytes(&[0, 0])).as_slice(),
            [(true, RawPnpValue::WrongType)]
        ));
    }

    #[test]
    fn configmgr_status_and_child_traversal_fail_closed() {
        assert_eq!(classify_device_status(true, 0, 0), DeviceProbeResult::Valid);
        assert_eq!(
            classify_device_status(true, 0x100, 0),
            DeviceProbeResult::PendingReboot
        );
        assert_eq!(
            classify_device_status(true, 0x400, 14),
            DeviceProbeResult::PendingReboot
        );
        assert_eq!(
            classify_device_status(true, 0x400, 31),
            DeviceProbeResult::StatusFailure
        );
        assert_eq!(
            classify_device_status(false, 0, 0),
            DeviceProbeResult::StatusFailure
        );
        assert_eq!(classify_child_traversal(0), ChildTraversalResult::Item);
        assert_eq!(classify_child_traversal(13), ChildTraversalResult::End);
        assert_eq!(classify_child_traversal(5), ChildTraversalResult::Failure);
        assert!(machine_matches_architecture(
            PeMachine::I386,
            PnpOpenClArchitecture::Wow32
        ));
        assert!(!machine_matches_architecture(
            PeMachine::Amd64,
            PnpOpenClArchitecture::Wow32
        ));
        let expected_native = match host_architecture() {
            "x86_64" => PeMachine::Amd64,
            "aarch64" => PeMachine::Arm64,
            "x86" => PeMachine::I386,
            "arm" => PeMachine::Arm,
            _ => PeMachine::Unknown(0),
        };
        if !matches!(expected_native, PeMachine::Unknown(_)) {
            assert!(machine_matches_architecture(
                expected_native,
                PnpOpenClArchitecture::Native
            ));
        }
    }

    #[test]
    fn extra_multi_sz_elements_are_diagnostic_only_and_never_verified() {
        let path = r"C:\Vendor\extra.dll";
        let adapter = platform(7, 0xaa);
        let observed = identity(path, native_machine(), 1, Some(0xaa));
        let fixture = FixtureIdentities {
            outcomes: BTreeMap::from([(path.to_ascii_lowercase(), Ok(observed))]),
        };
        let result = collect_with(
            vec![RawPnpCandidate {
                source_class: PnpOpenClSourceClass::SoftwareComponent,
                architecture: PnpOpenClArchitecture::Native,
                loader_selected: false,
                value: RawPnpValue::Path(path.into()),
                adapter: Some(adapter),
            }],
            Vec::new(),
            &fixture,
            &[],
        );
        assert_eq!(
            result.candidates[0].classification,
            PnpOpenClClassification::IdentityVerified
        );
        assert_eq!(
            result.candidates[0].status,
            PnpOpenClBindingStatus::UnverifiedCandidate
        );
        assert!(result.candidates[0].adapter.is_none());
    }

    #[test]
    fn fixtures_cover_sources_architectures_and_fail_closed_value_matrix() {
        let x64 = r"C:\Vendor\opencl64.dll";
        let x86 = r"C:\Vendor\opencl32.dll";
        let missing = r"C:\Vendor\missing.dll";
        let adapter = platform(7, 0xaa);
        let fixture = FixtureIdentities {
            outcomes: BTreeMap::from([
                (
                    x64.to_ascii_lowercase(),
                    Ok(identity(x64, native_machine(), 1, Some(0xaa))),
                ),
                (
                    x86.to_ascii_lowercase(),
                    Ok(identity(x86, PeMachine::I386, 2, Some(0xaa))),
                ),
                (missing.to_ascii_lowercase(), Err(IdentityFailure::Missing)),
            ]),
        };
        let result = collect_with(
            vec![
                raw(
                    PnpOpenClSourceClass::DisplayAdapter,
                    PnpOpenClArchitecture::Native,
                    RawPnpValue::Path(x64.into()),
                    Some(adapter.clone()),
                ),
                raw(
                    PnpOpenClSourceClass::SoftwareComponent,
                    PnpOpenClArchitecture::Wow32,
                    RawPnpValue::Path(x86.into()),
                    Some(adapter.clone()),
                ),
                raw(
                    PnpOpenClSourceClass::DisplayAdapter,
                    PnpOpenClArchitecture::Wow32,
                    RawPnpValue::Missing,
                    Some(adapter.clone()),
                ),
                raw(
                    PnpOpenClSourceClass::DisplayAdapter,
                    PnpOpenClArchitecture::Wow32,
                    RawPnpValue::WrongType,
                    Some(adapter.clone()),
                ),
                raw(
                    PnpOpenClSourceClass::DisplayAdapter,
                    PnpOpenClArchitecture::Wow32,
                    RawPnpValue::Empty,
                    Some(adapter.clone()),
                ),
                raw(
                    PnpOpenClSourceClass::DisplayAdapter,
                    PnpOpenClArchitecture::Native,
                    RawPnpValue::Path("relative.dll".into()),
                    Some(adapter.clone()),
                ),
                raw(
                    PnpOpenClSourceClass::DisplayAdapter,
                    PnpOpenClArchitecture::Native,
                    RawPnpValue::Path(missing.into()),
                    Some(adapter.clone()),
                ),
                raw(
                    PnpOpenClSourceClass::SoftwareComponent,
                    PnpOpenClArchitecture::Native,
                    RawPnpValue::SoftwareKeyOpenFailure,
                    Some(adapter.clone()),
                ),
                raw(
                    PnpOpenClSourceClass::SoftwareComponent,
                    PnpOpenClArchitecture::Native,
                    RawPnpValue::ReadFailure,
                    Some(adapter),
                ),
            ],
            Vec::new(),
            &fixture,
            &[],
        );
        assert_eq!(
            result
                .candidates
                .iter()
                .filter(|candidate| candidate.status == PnpOpenClBindingStatus::Verified)
                .count(),
            2
        );
        for expected in [
            PnpOpenClClassification::MissingValue,
            PnpOpenClClassification::WrongRegistryType,
            PnpOpenClClassification::EmptyValue,
            PnpOpenClClassification::InvalidPath,
            PnpOpenClClassification::MissingDll,
            PnpOpenClClassification::SoftwareKeyOpenFailure,
            PnpOpenClClassification::RegistryReadFailure,
        ] {
            assert!(
                result
                    .candidates
                    .iter()
                    .any(|candidate| candidate.classification == expected)
            );
        }
        assert!(result.candidates.iter().all(|candidate| {
            candidate.status == PnpOpenClBindingStatus::Verified
                || (candidate.identity.is_none() && candidate.adapter.is_none())
        }));
    }

    #[test]
    fn exact_catalog_and_adapter_are_required_and_legacy_merge_is_strict() {
        let path = r"C:\Vendor\opencl64.dll";
        let adapter = platform(7, 0xaa);
        let observed = identity(path, native_machine(), 1, Some(0xaa));
        let fixture = FixtureIdentities {
            outcomes: BTreeMap::from([(path.to_ascii_lowercase(), Ok(observed.clone()))]),
        };
        let exact = collect_with(
            vec![raw(
                PnpOpenClSourceClass::DisplayAdapter,
                PnpOpenClArchitecture::Native,
                RawPnpValue::Path(path.into()),
                Some(adapter.clone()),
            )],
            Vec::new(),
            &fixture,
            &[legacy(observed.clone(), adapter.clone())],
        );
        assert_eq!(exact.candidates[0].status, PnpOpenClBindingStatus::Verified);
        assert_eq!(
            exact.candidates[0].legacy_merge,
            LegacyMergeStatus::ExactMatch
        );

        let duplicate = collect_with(
            vec![raw(
                PnpOpenClSourceClass::DisplayAdapter,
                PnpOpenClArchitecture::Native,
                RawPnpValue::Path(path.into()),
                Some(adapter.clone()),
            )],
            Vec::new(),
            &fixture,
            &[
                legacy(observed.clone(), adapter.clone()),
                legacy(observed.clone(), adapter.clone()),
            ],
        );
        assert_eq!(
            duplicate.candidates[0].legacy_merge,
            LegacyMergeStatus::AmbiguousDuplicate
        );

        let mut conflicting_adapter = adapter.clone();
        conflicting_adapter.adapter_luid = 9;
        let conflict = collect_with(
            vec![raw(
                PnpOpenClSourceClass::DisplayAdapter,
                PnpOpenClArchitecture::Native,
                RawPnpValue::Path(path.into()),
                Some(adapter.clone()),
            )],
            Vec::new(),
            &fixture,
            &[legacy(observed.clone(), conflicting_adapter)],
        );
        assert_eq!(
            conflict.candidates[0].legacy_merge,
            LegacyMergeStatus::ConflictEvidence
        );

        let mismatch_fixture = FixtureIdentities {
            outcomes: BTreeMap::from([(
                path.to_ascii_lowercase(),
                Ok(identity(path, native_machine(), 1, Some(0xbb))),
            )]),
        };
        let mismatch = collect_with(
            vec![raw(
                PnpOpenClSourceClass::DisplayAdapter,
                PnpOpenClArchitecture::Native,
                RawPnpValue::Path(path.into()),
                Some(adapter),
            )],
            Vec::new(),
            &mismatch_fixture,
            &[],
        );
        assert_eq!(
            mismatch.candidates[0].status,
            PnpOpenClBindingStatus::ConflictCatalogDigest
        );
        assert!(mismatch.candidates[0].adapter.is_none());
    }

    #[test]
    fn report_is_privacy_bounded_and_never_claims_backend_readiness() {
        let path = r"C:\Private Vendor\opencl64.dll";
        let adapter = platform(7, 0xaa);
        let observed = identity(path, native_machine(), 1, Some(0xaa));
        let fixture = FixtureIdentities {
            outcomes: BTreeMap::from([(path.to_ascii_lowercase(), Ok(observed.clone()))]),
        };
        let collection = collect_with(
            vec![raw(
                PnpOpenClSourceClass::SoftwareComponent,
                PnpOpenClArchitecture::Native,
                RawPnpValue::Path(path.into()),
                Some(adapter.clone()),
            )],
            Vec::new(),
            &fixture,
            &[legacy(observed, adapter)],
        );
        let report = privacy_bounded_pnp_opencl_report(&collection);
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains(r"C:\Private Vendor"));
        assert!(!serialized.contains("PCI\\VEN_"));
        assert_eq!(report["candidates"][0]["path_basename"], "opencl64.dll");
        assert_eq!(report["candidates"][0]["status"], "verified");
        assert_eq!(report["candidates"][0]["legacy_merge"], "exact_match");
        assert_eq!(report["candidates"][0]["backend_ready"], false);
    }
}
