#![cfg(windows)]

use aexcompat_broker::opencl_icd_collector::{
    OpenClIcdClassification, collect_opencl_icd_candidates, privacy_bounded_opencl_icd_report,
};
use std::collections::BTreeSet;

#[test]
fn windows_registry_collection_is_bounded_and_fail_closed() {
    let collection = collect_opencl_icd_candidates();
    let report = privacy_bounded_opencl_icd_report(&collection);
    let serialized = serde_json::to_string(&report).expect("serialize collector report");
    assert!(!serialized.contains(":\\"));

    let mut candidate_keys = BTreeSet::new();
    for candidate in &collection.candidates {
        let key = (
            candidate.registry_view,
            candidate
                .path
                .as_ref()
                .map(|path| path.to_string_lossy().to_ascii_lowercase()),
        );
        assert!(candidate_keys.insert(key), "duplicate ICD candidate");
        if candidate.classification == OpenClIcdClassification::IdentityVerified {
            assert_eq!(candidate.enabled, Some(true));
            assert!(candidate.path.is_some());
            assert!(candidate.identity.is_some());
        } else {
            assert!(
                candidate.identity.is_none(),
                "rejected candidates must not retain identity evidence"
            );
        }
    }

    eprintln!(
        "{}",
        serde_json::to_string_pretty(&report).expect("pretty collector report")
    );
}
