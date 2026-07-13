pub mod scattermap;

use crate::host_core::approved_artifact::ApprovalPolicy;
use crate::host_core::descriptor_manifest::ManifestPolicy;

#[derive(Clone, Copy)]
pub struct WorkerSpec {
    pub executable: &'static str,
    pub request_mode: &'static str,
    pub approval: ApprovalPolicy,
}

#[derive(Clone, Copy)]
pub struct L2ObservationPolicy {
    pub allowlist_path: &'static str,
    pub receipt_id: &'static str,
    pub expires: &'static str,
    pub max_timeout_ms: u64,
    pub about_substrings: &'static [&'static str],
    pub out_flags: u64,
    pub out_flags2: u64,
    pub update_params_ui_advertised: bool,
    pub query_dynamic_flags_advertised: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterizedRenderAdapter {
    ScatterMap,
}

pub struct RegisteredProfile {
    pub l2_observation: L2ObservationPolicy,
    pub descriptor_manifest: ManifestPolicy,
    pub parameterized_render: ParameterizedRenderAdapter,
    pub classic_worker: WorkerSpec,
    pub smart_worker: WorkerSpec,
}

static SCATTERMAP: RegisteredProfile = RegisteredProfile {
    l2_observation: L2ObservationPolicy {
        allowlist_path: "target/l2-allowlist/active.local.json",
        receipt_id: "scattermap-l2-20260713-001",
        expires: "2026-08-12T23:59:59+09:00",
        max_timeout_ms: 30_000,
        about_substrings: &["ScatterMap v1.0", "Written in Rust"],
        out_flags: 33_554_432,
        out_flags2: 167_777_280,
        update_params_ui_advertised: false,
        query_dynamic_flags_advertised: false,
    },
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
            find(scattermap::PROFILE_ID)
                .unwrap()
                .l2_observation
                .receipt_id,
            "scattermap-l2-20260713-001"
        );
        assert_eq!(
            find(scattermap::PROFILE_ID).unwrap().parameterized_render,
            ParameterizedRenderAdapter::ScatterMap
        );
        assert!(find("unknown-aex").is_none());
    }
}
