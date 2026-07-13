pub mod scattermap;

use crate::host_core::approved_artifact::ApprovalPolicy;
use crate::host_core::descriptor_manifest::ManifestPolicy;

#[derive(Clone, Copy)]
pub struct WorkerSpec {
    pub executable: &'static str,
    pub request_mode: &'static str,
    pub approval: ApprovalPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterizedRenderAdapter {
    ScatterMap,
}

pub struct RegisteredProfile {
    pub descriptor_manifest: ManifestPolicy,
    pub parameterized_render: ParameterizedRenderAdapter,
    pub classic_worker: WorkerSpec,
    pub smart_worker: WorkerSpec,
}

static SCATTERMAP: RegisteredProfile = RegisteredProfile {
    descriptor_manifest: ManifestPolicy {
        path: "profiles/scattermap/parameter_descriptors.json",
        sha256: "C797EC7C45A603D2C86FB980DC5279E8B075D0E27D466315A2ABFA15BE608C37",
    },
    parameterized_render: ParameterizedRenderAdapter::ScatterMap,
    classic_worker: WorkerSpec {
        executable: "target/minihost-build/aex_render_worker.exe",
        request_mode: "--render-request",
        approval: ApprovalPolicy {
            allowlist_path: "target/render-allowlist/active.local.json",
            stage: "classic_render",
            receipt_id: "scattermap-extended-render-20260713-001",
            expires: "2026-08-12T23:59:59+09:00",
            max_timeout_ms: 5_000,
        },
    },
    smart_worker: WorkerSpec {
        executable: "target/minihost-build/aex_smart_worker.exe",
        request_mode: "--smart-request",
        approval: ApprovalPolicy {
            allowlist_path: "target/smart-allowlist/active.local.json",
            stage: "smartfx_render",
            receipt_id: "scattermap-smartfx-20260713-001",
            expires: "2026-08-12T23:59:59+09:00",
            max_timeout_ms: 5_000,
        },
    },
};

pub fn find(id: &str) -> Option<&'static RegisteredProfile> {
    match id {
        scattermap::PROFILE_ID => Some(&SCATTERMAP),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_explicit_and_unknown_profiles_fail_closed() {
        assert_eq!(
            find(scattermap::PROFILE_ID)
                .unwrap()
                .descriptor_manifest
                .path,
            "profiles/scattermap/parameter_descriptors.json"
        );
        assert_eq!(
            find(scattermap::PROFILE_ID)
                .unwrap()
                .classic_worker
                .request_mode,
            "--render-request"
        );
        assert_eq!(
            find(scattermap::PROFILE_ID).unwrap().parameterized_render,
            ParameterizedRenderAdapter::ScatterMap
        );
        assert!(find("unknown-aex").is_none());
    }
}
