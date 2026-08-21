pub mod maskoffset;
pub mod scattermap;

use crate::host_core::approved_artifact::SelectionPolicy;
use crate::host_core::descriptor_manifest::ManifestPolicy;

#[derive(Clone, Copy)]
pub struct WorkerSpec {
    pub executable: &'static str,
    pub request_mode: &'static str,
    pub selection: SelectionPolicy,
}

#[derive(Clone, Copy)]
pub struct L2ObservationPolicy {
    pub selection: SelectionPolicy,
}

pub struct ObservationProfile {
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
        selection: SelectionPolicy {
            allowlist_path: "target/l2-allowlist/active.local.json",
        },
    },
    descriptor_manifest: ManifestPolicy {
        path: "profiles/scattermap/parameter_descriptors.json",
    },
    parameterized_render: ParameterizedRenderAdapter::ScatterMap,
    classic_worker: Some(WorkerSpec {
        executable: "target/minihost-build/aex_worker.exe",
        request_mode: "--render-request",
        selection: SelectionPolicy {
            allowlist_path: "target/render-allowlist/active.local.json",
        },
    }),
    smart_worker: Some(WorkerSpec {
        executable: "target/minihost-build/aex_worker.exe",
        request_mode: "--smart-request",
        selection: SelectionPolicy {
            allowlist_path: "target/smart-allowlist/active.local.json",
        },
    }),
};

static MASKOFFSET: RegisteredProfile = RegisteredProfile {
    l2_observation: MASKOFFSET_OBSERVATION.l2,
    descriptor_manifest: MASKOFFSET_OBSERVATION.descriptor_manifest,
    parameterized_render: ParameterizedRenderAdapter::MaskOffsetRectangle,
    classic_worker: None,
    smart_worker: Some(WorkerSpec {
        executable: "target/minihost-build/aex_worker.exe",
        request_mode: "--smart-mask-request",
        selection: SelectionPolicy {
            allowlist_path: "target/smart-allowlist/maskoffset.active.local.json",
        },
    }),
};

static SCATTERMAP_OBSERVATION: ObservationProfile = ObservationProfile {
    l2: SCATTERMAP.l2_observation,
    descriptor_manifest: SCATTERMAP.descriptor_manifest,
};

static MASKOFFSET_OBSERVATION: ObservationProfile = ObservationProfile {
    l2: L2ObservationPolicy {
        selection: SelectionPolicy {
            allowlist_path: "target/l2-allowlist/maskoffset.active.local.json",
        },
    },
    descriptor_manifest: ManifestPolicy {
        path: "profiles/maskoffset/parameter_descriptors.json",
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
                .selection
                .allowlist_path,
            "target/l2-allowlist/active.local.json"
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
                .selection
                .allowlist_path,
            "target/l2-allowlist/maskoffset.active.local.json"
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
