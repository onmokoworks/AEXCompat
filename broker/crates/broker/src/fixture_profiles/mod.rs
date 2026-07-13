pub mod maskoffset;
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
    pub approval: ApprovalPolicy,
    pub about_substrings: &'static [&'static str],
    pub out_flags: u64,
    pub out_flags2: u64,
    pub update_params_ui_advertised: bool,
    pub query_dynamic_flags_advertised: bool,
}

pub struct ObservationProfile {
    pub l1_approval: ApprovalPolicy,
    pub l2: L2ObservationPolicy,
    pub descriptor_manifest: ManifestPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterizedRenderAdapter {
    ScatterMap,
    MaskOffsetRectangle,
}

pub struct RegisteredProfile {
    pub l2_observation: L2ObservationPolicy,
    pub descriptor_manifest: ManifestPolicy,
    pub parameterized_render: ParameterizedRenderAdapter,
    pub classic_worker: Option<WorkerSpec>,
    pub smart_worker: Option<WorkerSpec>,
}

static SCATTERMAP: RegisteredProfile = RegisteredProfile {
    l2_observation: L2ObservationPolicy {
        approval: ApprovalPolicy {
            allowlist_path: "target/l2-allowlist/active.local.json",
            stage: "L2",
            receipt_id: "scattermap-l2-20260713-001",
            expires: "2026-08-12T23:59:59+09:00",
            max_timeout_ms: 30_000,
        },
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
    classic_worker: Some(WorkerSpec {
        executable: "target/minihost-build/aex_render_worker.exe",
        request_mode: "--render-request",
        approval: ApprovalPolicy {
            allowlist_path: "target/render-allowlist/active.local.json",
            stage: "classic_render",
            receipt_id: "scattermap-extended-render-20260713-001",
            expires: "2026-08-12T23:59:59+09:00",
            max_timeout_ms: 5_000,
        },
    }),
    smart_worker: Some(WorkerSpec {
        executable: "target/minihost-build/aex_smart_worker.exe",
        request_mode: "--smart-request",
        approval: ApprovalPolicy {
            allowlist_path: "target/smart-allowlist/active.local.json",
            stage: "smartfx_render",
            receipt_id: "scattermap-smartfx-20260713-001",
            expires: "2026-08-12T23:59:59+09:00",
            max_timeout_ms: 5_000,
        },
    }),
};

static MASKOFFSET: RegisteredProfile = RegisteredProfile {
    l2_observation: MASKOFFSET_OBSERVATION.l2,
    descriptor_manifest: MASKOFFSET_OBSERVATION.descriptor_manifest,
    parameterized_render: ParameterizedRenderAdapter::MaskOffsetRectangle,
    classic_worker: None,
    smart_worker: Some(WorkerSpec {
        executable: "target/minihost-build/aex_smart_worker.exe",
        request_mode: "--smart-mask-request",
        approval: ApprovalPolicy {
            allowlist_path: "target/smart-allowlist/maskoffset.active.local.json",
            stage: "smartfx_render",
            receipt_id: "maskoffset-smartfx-20260713-001",
            expires: "2026-08-12T23:59:59+09:00",
            max_timeout_ms: 5_000,
        },
    }),
};

static SCATTERMAP_OBSERVATION: ObservationProfile = ObservationProfile {
    l1_approval: ApprovalPolicy {
        allowlist_path: "target/l1-allowlist/active.local.json",
        stage: "L1",
        receipt_id: "scattermap-l1-20260713-001",
        expires: "2026-08-12T23:59:59+09:00",
        max_timeout_ms: 30_000,
    },
    l2: SCATTERMAP.l2_observation,
    descriptor_manifest: SCATTERMAP.descriptor_manifest,
};

static MASKOFFSET_OBSERVATION: ObservationProfile = ObservationProfile {
    l1_approval: ApprovalPolicy {
        allowlist_path: "target/l1-allowlist/maskoffset.active.local.json",
        stage: "L1",
        receipt_id: "maskoffset-l1-20260713-001",
        expires: "2026-08-12T23:59:59+09:00",
        max_timeout_ms: 30_000,
    },
    l2: L2ObservationPolicy {
        approval: ApprovalPolicy {
            allowlist_path: "target/l2-allowlist/maskoffset.active.local.json",
            stage: "L2",
            receipt_id: "maskoffset-l2-20260713-001",
            expires: "2026-08-12T23:59:59+09:00",
            max_timeout_ms: 30_000,
        },
        about_substrings: &["ONMK MaskOffset v1.0", "Written in Rust"],
        out_flags: 4,
        out_flags2: 525_312,
        update_params_ui_advertised: false,
        query_dynamic_flags_advertised: false,
    },
    descriptor_manifest: ManifestPolicy {
        path: "profiles/maskoffset/parameter_descriptors.json",
        sha256: "13876295DB0A58D4B401525E88E44071A1C489D649B146F7149FD408B651DB13",
    },
};

pub fn find(id: &str) -> Option<&'static RegisteredProfile> {
    match id {
        scattermap::PROFILE_ID => Some(&SCATTERMAP),
        maskoffset::PROFILE_ID => Some(&MASKOFFSET),
        _ => None,
    }
}

pub fn find_observation(id: &str) -> Option<&'static ObservationProfile> {
    match id {
        scattermap::PROFILE_ID => Some(&SCATTERMAP_OBSERVATION),
        "maskoffset" => Some(&MASKOFFSET_OBSERVATION),
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
                .unwrap()
                .request_mode,
            "--render-request"
        );
        assert_eq!(
            find(scattermap::PROFILE_ID)
                .unwrap()
                .l2_observation
                .approval
                .receipt_id,
            "scattermap-l2-20260713-001"
        );
        assert_eq!(
            find(scattermap::PROFILE_ID).unwrap().parameterized_render,
            ParameterizedRenderAdapter::ScatterMap
        );
        assert!(find("unknown-aex").is_none());
        assert_eq!(
            find_observation("maskoffset")
                .unwrap()
                .l2
                .approval
                .receipt_id,
            "maskoffset-l2-20260713-001"
        );
        assert!(find("maskoffset").unwrap().classic_worker.is_none());
        assert_eq!(
            find("maskoffset")
                .unwrap()
                .smart_worker
                .unwrap()
                .request_mode,
            "--smart-mask-request"
        );
        assert!(find_observation("unknown-aex").is_none());
        assert_eq!(
            find_observation("maskoffset")
                .unwrap()
                .descriptor_manifest
                .path,
            "profiles/maskoffset/parameter_descriptors.json"
        );
    }
}
