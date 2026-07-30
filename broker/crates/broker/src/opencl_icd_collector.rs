use crate::runtime_module_identity::{
    AuthenticodeEvidence, IdentityEvidenceErrorKind, PeMachine, RuntimeModuleIdentityEvidence,
    capture_runtime_module_identity,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const OPENCL_ICD_REGISTRY_PATH: &str = r"SOFTWARE\Khronos\OpenCL\Vendors";
const REGISTRY_DWORD_TYPE: u32 = 4;
const MAX_REGISTRY_VALUES: u32 = 4096;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenClRegistryView {
    Registry64,
    Registry32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenClIcdClassification {
    IdentityVerified,
    Disabled,
    MalformedRegistryValue,
    InvalidPath,
    MissingDll,
    InaccessibleDll,
    UnsafeDll,
    InvalidPe,
    ArchitectureMismatch,
    UntrustedDll,
    IdentityUnsupported,
    IdentityIoFailure,
    RegistryReadFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenClRegistryDiagnosticKind {
    MissingRegistryKey,
    InaccessibleRegistryKey,
    RegistryReadFailure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenClIcdCandidate {
    pub registry_view: OpenClRegistryView,
    pub path: Option<PathBuf>,
    pub enabled: Option<bool>,
    pub classification: OpenClIcdClassification,
    pub identity: Option<RuntimeModuleIdentityEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OpenClRegistryDiagnostic {
    pub registry_view: OpenClRegistryView,
    pub classification: OpenClRegistryDiagnosticKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenClIcdCollection {
    pub candidates: Vec<OpenClIcdCandidate>,
    pub diagnostics: Vec<OpenClRegistryDiagnostic>,
}

/// Enumerates both Windows registry views used by the legacy Khronos OpenCL
/// ICD registration contract. Every value is classified independently: a bad
/// entry never suppresses evidence for the remaining candidates.
///
/// This slice deliberately does not associate a legacy global registry value
/// with a DXGI adapter or active driver package. The registry key itself does
/// not carry sufficient evidence for that relationship.
pub fn collect_opencl_icd_candidates() -> OpenClIcdCollection {
    collect_with(&platform::WindowsRegistrySource, &RuntimeIdentitySource)
}

/// Produces a stable, privacy-bounded JSON report. Full registry paths remain
/// available to the local caller through `OpenClIcdCandidate::path`, while the
/// shareable report emits only a basename, a path fingerprint, and file
/// identity evidence.
pub fn privacy_bounded_opencl_icd_report(collection: &OpenClIcdCollection) -> serde_json::Value {
    let candidates = collection
        .candidates
        .iter()
        .map(|candidate| {
            let path_text = candidate
                .path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned());
            let basename = path_text.as_deref().and_then(windows_basename);
            let path_fingerprint_sha256 = path_text.as_deref().map(path_fingerprint);
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
                })
            });
            serde_json::json!({
                "registry_view": candidate.registry_view,
                "enabled": candidate.enabled,
                "classification": candidate.classification,
                "path_basename": basename,
                "path_fingerprint_sha256": path_fingerprint_sha256,
                "identity": identity,
                "adapter_association": "unverified",
                "backend_ready": false,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema_version": 1,
        "registry_path": OPENCL_ICD_REGISTRY_PATH,
        "candidates": candidates,
        "diagnostics": collection.diagnostics,
    })
}

#[derive(Clone, Debug)]
struct RawRegistryValue {
    name: Option<String>,
    value_type: u32,
    data: Vec<u8>,
    read_failure: bool,
}

#[derive(Clone, Copy, Debug)]
enum RegistrySourceFailure {
    Missing,
    Inaccessible,
    ReadFailure,
}

trait RegistrySource {
    fn enumerate(
        &self,
        view: OpenClRegistryView,
    ) -> Result<Vec<RawRegistryValue>, RegistrySourceFailure>;
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

fn collect_with<R: RegistrySource, I: IdentitySource>(
    registry: &R,
    identities: &I,
) -> OpenClIcdCollection {
    let mut collection = OpenClIcdCollection {
        candidates: Vec::new(),
        diagnostics: Vec::new(),
    };
    for view in [
        OpenClRegistryView::Registry64,
        OpenClRegistryView::Registry32,
    ] {
        match registry.enumerate(view) {
            Ok(values) => {
                collection.candidates.extend(
                    values
                        .into_iter()
                        .map(|value| classify_candidate(view, value, identities)),
                );
            }
            Err(failure) => collection.diagnostics.push(OpenClRegistryDiagnostic {
                registry_view: view,
                classification: match failure {
                    RegistrySourceFailure::Missing => {
                        OpenClRegistryDiagnosticKind::MissingRegistryKey
                    }
                    RegistrySourceFailure::Inaccessible => {
                        OpenClRegistryDiagnosticKind::InaccessibleRegistryKey
                    }
                    RegistrySourceFailure::ReadFailure => {
                        OpenClRegistryDiagnosticKind::RegistryReadFailure
                    }
                },
            }),
        }
    }
    collection.candidates.sort_by(|left, right| {
        let left_path = left
            .path
            .as_ref()
            .map(|path| path.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let right_path = right
            .path
            .as_ref()
            .map(|path| path.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        left.registry_view
            .cmp(&right.registry_view)
            .then_with(|| left_path.cmp(&right_path))
            .then_with(|| {
                format!("{:?}", left.classification).cmp(&format!("{:?}", right.classification))
            })
    });
    collection
}

fn classify_candidate<I: IdentitySource>(
    view: OpenClRegistryView,
    value: RawRegistryValue,
    identities: &I,
) -> OpenClIcdCandidate {
    let enabled = parse_enabled(value.value_type, &value.data);
    let path = value
        .name
        .as_deref()
        .filter(|name| valid_windows_dll_path(name))
        .map(PathBuf::from);

    let classification = if value.read_failure {
        OpenClIcdClassification::RegistryReadFailure
    } else if enabled.is_none() || value.name.is_none() {
        OpenClIcdClassification::MalformedRegistryValue
    } else if path.is_none() {
        OpenClIcdClassification::InvalidPath
    } else if enabled == Some(false) {
        OpenClIcdClassification::Disabled
    } else {
        let path = path.as_ref().expect("validated path");
        match identities.capture(path) {
            Ok(identity) if machine_matches_view(identity.pe_machine, view) => {
                return OpenClIcdCandidate {
                    registry_view: view,
                    path: Some(path.clone()),
                    enabled,
                    classification: OpenClIcdClassification::IdentityVerified,
                    identity: Some(identity),
                };
            }
            Ok(_) => OpenClIcdClassification::ArchitectureMismatch,
            Err(IdentityFailure::InvalidPath) => OpenClIcdClassification::InvalidPath,
            Err(IdentityFailure::Missing) => OpenClIcdClassification::MissingDll,
            Err(IdentityFailure::Inaccessible) => OpenClIcdClassification::InaccessibleDll,
            Err(IdentityFailure::Unsafe) => OpenClIcdClassification::UnsafeDll,
            Err(IdentityFailure::InvalidPe) => OpenClIcdClassification::InvalidPe,
            Err(IdentityFailure::Untrusted) => OpenClIcdClassification::UntrustedDll,
            Err(IdentityFailure::Unsupported) => OpenClIcdClassification::IdentityUnsupported,
            Err(IdentityFailure::Io) => OpenClIcdClassification::IdentityIoFailure,
        }
    };
    OpenClIcdCandidate {
        registry_view: view,
        path,
        enabled,
        classification,
        identity: None,
    }
}

fn parse_enabled(value_type: u32, data: &[u8]) -> Option<bool> {
    (value_type == REGISTRY_DWORD_TYPE && data.len() == 4)
        .then(|| u32::from_le_bytes(data.try_into().expect("four-byte DWORD")) == 0)
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
    if !absolute_drive && !absolute_unc {
        return false;
    }
    !value
        .split(['\\', '/'])
        .any(|component| component == "." || component == "..")
}

fn machine_matches_view(machine: PeMachine, view: OpenClRegistryView) -> bool {
    matches!(
        (machine, view),
        (
            PeMachine::Amd64 | PeMachine::Arm64,
            OpenClRegistryView::Registry64
        ) | (
            PeMachine::I386 | PeMachine::Arm,
            OpenClRegistryView::Registry32
        )
    )
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

    pub struct WindowsRegistrySource;

    impl RegistrySource for WindowsRegistrySource {
        fn enumerate(
            &self,
            _: OpenClRegistryView,
        ) -> Result<Vec<RawRegistryValue>, RegistrySourceFailure> {
            Err(RegistrySourceFailure::ReadFailure)
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS,
        ERROR_PATH_NOT_FOUND,
    };
    use windows::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, RegCloseKey,
        RegEnumValueW, RegOpenKeyExW,
    };
    use windows::core::{PCWSTR, PWSTR};

    const MAX_VALUE_NAME_WORDS: usize = 32_768;
    const MAX_VALUE_DATA_BYTES: usize = 64;

    pub struct WindowsRegistrySource;

    struct RegistryKey(HKEY);

    impl Drop for RegistryKey {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    impl RegistrySource for WindowsRegistrySource {
        fn enumerate(
            &self,
            view: OpenClRegistryView,
        ) -> Result<Vec<RawRegistryValue>, RegistrySourceFailure> {
            let key_path: Vec<u16> = OPENCL_ICD_REGISTRY_PATH
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let view_flag = match view {
                OpenClRegistryView::Registry64 => KEY_WOW64_64KEY,
                OpenClRegistryView::Registry32 => KEY_WOW64_32KEY,
            };
            let mut key = HKEY::default();
            let status = unsafe {
                RegOpenKeyExW(
                    HKEY_LOCAL_MACHINE,
                    PCWSTR(key_path.as_ptr()),
                    0,
                    KEY_READ | view_flag,
                    &mut key,
                )
            };
            if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
                return Err(RegistrySourceFailure::Missing);
            }
            if status == ERROR_ACCESS_DENIED {
                return Err(RegistrySourceFailure::Inaccessible);
            }
            if !status.is_ok() {
                return Err(RegistrySourceFailure::ReadFailure);
            }
            let key = RegistryKey(key);
            let mut values = Vec::new();
            let mut enumeration_complete = false;
            for index in 0..MAX_REGISTRY_VALUES {
                let mut name = vec![0u16; MAX_VALUE_NAME_WORDS];
                let mut name_len = (name.len() - 1) as u32;
                let mut value_type = 0u32;
                let mut data = vec![0u8; MAX_VALUE_DATA_BYTES];
                let mut data_len = data.len() as u32;
                let status = unsafe {
                    RegEnumValueW(
                        key.0,
                        index,
                        PWSTR(name.as_mut_ptr()),
                        &mut name_len,
                        None,
                        Some(&mut value_type),
                        Some(data.as_mut_ptr()),
                        Some(&mut data_len),
                    )
                };
                if status == ERROR_NO_MORE_ITEMS {
                    enumeration_complete = true;
                    break;
                }
                let safe_name_len = (name_len as usize).min(name.len());
                let decoded_name = std::ffi::OsString::from_wide(&name[..safe_name_len])
                    .into_string()
                    .ok();
                if status == ERROR_MORE_DATA {
                    values.push(RawRegistryValue {
                        name: decoded_name,
                        value_type,
                        data: Vec::new(),
                        read_failure: false,
                    });
                    continue;
                }
                if !status.is_ok() {
                    if values.is_empty() {
                        return Err(RegistrySourceFailure::ReadFailure);
                    }
                    values.push(RawRegistryValue {
                        name: None,
                        value_type: 0,
                        data: Vec::new(),
                        read_failure: true,
                    });
                    enumeration_complete = true;
                    break;
                }
                data.truncate((data_len as usize).min(data.len()));
                values.push(RawRegistryValue {
                    name: decoded_name,
                    value_type,
                    data,
                    read_failure: false,
                });
            }
            if !enumeration_complete {
                values.push(RawRegistryValue {
                    name: None,
                    value_type: 0,
                    data: Vec::new(),
                    read_failure: true,
                });
            }
            Ok(values)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_module_identity::FileIdentity;
    use std::collections::BTreeMap;

    struct FixtureRegistry {
        values: BTreeMap<OpenClRegistryView, Vec<RawRegistryValue>>,
        failures: BTreeMap<OpenClRegistryView, RegistrySourceFailure>,
    }

    impl RegistrySource for FixtureRegistry {
        fn enumerate(
            &self,
            view: OpenClRegistryView,
        ) -> Result<Vec<RawRegistryValue>, RegistrySourceFailure> {
            if let Some(failure) = self.failures.get(&view) {
                return Err(*failure);
            }
            Ok(self.values.get(&view).cloned().unwrap_or_default())
        }
    }

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

    fn raw(path: &str, enabled_dword: u32) -> RawRegistryValue {
        RawRegistryValue {
            name: Some(path.into()),
            value_type: REGISTRY_DWORD_TYPE,
            data: enabled_dword.to_le_bytes().to_vec(),
            read_failure: false,
        }
    }

    fn identity(path: &str, machine: PeMachine, seed: u8) -> RuntimeModuleIdentityEvidence {
        RuntimeModuleIdentityEvidence {
            canonical_path: PathBuf::from(path),
            size: 4096 + u64::from(seed),
            sha256: [seed; 32],
            pe_machine: machine,
            file_identity: FileIdentity {
                volume_serial_number: 0x1234_0000 | u32::from(seed),
                file_index: 0x5678_0000 | u64::from(seed),
            },
            authenticode: AuthenticodeEvidence::Embedded,
            signing_catalog_sha256: None,
        }
    }

    #[test]
    fn fixture_classifies_multiple_enabled_disabled_missing_and_malformed_candidates() {
        let enabled = r"C:\Windows\System32\vendor-opencl.dll";
        let disabled = r"C:\Windows\System32\disabled-opencl.dll";
        let missing = r"C:\Windows\System32\missing-opencl.dll";
        let inaccessible = r"C:\Windows\System32\locked-opencl.dll";
        let malformed = RawRegistryValue {
            name: Some(r"C:\Windows\System32\bad-opencl.dll".into()),
            value_type: 1,
            data: b"not-a-dword".to_vec(),
            read_failure: false,
        };
        let registry = FixtureRegistry {
            values: BTreeMap::from([
                (
                    OpenClRegistryView::Registry64,
                    vec![
                        raw(missing, 0),
                        raw(enabled, 0),
                        raw(disabled, 1),
                        malformed,
                        RawRegistryValue {
                            name: None,
                            value_type: 0,
                            data: Vec::new(),
                            read_failure: true,
                        },
                    ],
                ),
                (OpenClRegistryView::Registry32, vec![raw(inaccessible, 0)]),
            ]),
            failures: BTreeMap::new(),
        };
        let identities = FixtureIdentities {
            outcomes: BTreeMap::from([
                (
                    enabled.to_ascii_lowercase(),
                    Ok(identity(enabled, PeMachine::Amd64, 7)),
                ),
                (missing.to_ascii_lowercase(), Err(IdentityFailure::Missing)),
                (
                    inaccessible.to_ascii_lowercase(),
                    Err(IdentityFailure::Inaccessible),
                ),
            ]),
        };

        let result = collect_with(&registry, &identities);
        assert_eq!(result.candidates.len(), 6);
        assert_eq!(
            result
                .candidates
                .iter()
                .filter(|candidate| candidate.classification
                    == OpenClIcdClassification::IdentityVerified)
                .count(),
            1
        );
        for expected in [
            OpenClIcdClassification::Disabled,
            OpenClIcdClassification::MissingDll,
            OpenClIcdClassification::InaccessibleDll,
            OpenClIcdClassification::MalformedRegistryValue,
            OpenClIcdClassification::RegistryReadFailure,
        ] {
            assert!(
                result
                    .candidates
                    .iter()
                    .any(|candidate| candidate.classification == expected)
            );
        }
        let verified = result
            .candidates
            .iter()
            .find(|candidate| candidate.classification == OpenClIcdClassification::IdentityVerified)
            .unwrap();
        assert_eq!(verified.enabled, Some(true));
        assert_eq!(verified.path.as_deref(), Some(Path::new(enabled)));
        assert!(verified.identity.is_some());
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn fixture_is_fail_closed_for_invalid_paths_architecture_and_registry_access() {
        let wrong_arch = r"C:\Windows\System32\wrong-arch.dll";
        let registry = FixtureRegistry {
            values: BTreeMap::from([(
                OpenClRegistryView::Registry64,
                vec![
                    raw(r"relative-opencl.dll", 0),
                    raw(wrong_arch, 0),
                    RawRegistryValue {
                        name: None,
                        value_type: REGISTRY_DWORD_TYPE,
                        data: 0u32.to_le_bytes().to_vec(),
                        read_failure: false,
                    },
                ],
            )]),
            failures: BTreeMap::from([(
                OpenClRegistryView::Registry32,
                RegistrySourceFailure::Inaccessible,
            )]),
        };
        let identities = FixtureIdentities {
            outcomes: BTreeMap::from([(
                wrong_arch.to_ascii_lowercase(),
                Ok(identity(wrong_arch, PeMachine::I386, 9)),
            )]),
        };

        let result = collect_with(&registry, &identities);
        assert!(result.candidates.iter().all(|candidate| {
            candidate.classification != OpenClIcdClassification::IdentityVerified
                && candidate.identity.is_none()
        }));
        assert!(
            result.candidates.iter().any(|candidate| {
                candidate.classification == OpenClIcdClassification::InvalidPath
            })
        );
        assert!(result.candidates.iter().any(|candidate| {
            candidate.classification == OpenClIcdClassification::ArchitectureMismatch
        }));
        assert!(result.candidates.iter().any(|candidate| {
            candidate.classification == OpenClIcdClassification::MalformedRegistryValue
        }));
        assert_eq!(
            result.diagnostics,
            vec![OpenClRegistryDiagnostic {
                registry_view: OpenClRegistryView::Registry32,
                classification: OpenClRegistryDiagnosticKind::InaccessibleRegistryKey,
            }]
        );
    }

    #[test]
    fn report_is_deterministic_privacy_bounded_and_has_unique_closed_keys() {
        let path = r"C:\Vendor Private\OpenCL\vendor-opencl.dll";
        let collection = OpenClIcdCollection {
            candidates: vec![OpenClIcdCandidate {
                registry_view: OpenClRegistryView::Registry64,
                path: Some(PathBuf::from(path)),
                enabled: Some(true),
                classification: OpenClIcdClassification::IdentityVerified,
                identity: Some(identity(path, PeMachine::Amd64, 11)),
            }],
            diagnostics: vec![],
        };
        let report = privacy_bounded_opencl_icd_report(&collection);
        let serialized = serde_json::to_string(&report).unwrap();
        let reparsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(report, reparsed);
        assert!(!serialized.contains(r"C:\Vendor Private"));
        assert_eq!(
            report["candidates"][0]["path_basename"],
            "vendor-opencl.dll"
        );
        assert_eq!(
            report["candidates"][0]["path_fingerprint_sha256"]
                .as_str()
                .unwrap()
                .len(),
            64
        );
        assert_eq!(report["candidates"][0]["adapter_association"], "unverified");
        assert_eq!(report["candidates"][0]["backend_ready"], false);
        let root_keys = report
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            root_keys,
            vec![
                "candidates",
                "diagnostics",
                "registry_path",
                "schema_version"
            ]
        );
    }
}
