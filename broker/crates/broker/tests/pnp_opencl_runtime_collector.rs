#![cfg(windows)]

use aexcompat_broker::pnp_opencl_runtime_collector::{
    PnpOpenClBindingStatus, PnpOpenClClassification, collect_pnp_opencl_runtime_candidates,
    privacy_bounded_pnp_opencl_report,
};

#[test]
fn windows_pnp_opencl_runtime_self_test_is_privacy_bounded_and_fail_closed() {
    let collection = collect_pnp_opencl_runtime_candidates();
    let report = privacy_bounded_pnp_opencl_report(&collection);
    let serialized = serde_json::to_string(&report).expect("serialize PnP OpenCL report");
    let reparsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("parse PnP OpenCL report");
    assert_eq!(report, reparsed);
    assert!(!serialized.contains(":\\"));
    assert!(!serialized.contains("PCI\\\\VEN_"));
    assert!(!serialized.contains("device_instance_id"));
    assert!(!serialized.contains("hardware_id"));

    for candidate in &collection.candidates {
        if candidate.status == PnpOpenClBindingStatus::Verified {
            assert!(candidate.loader_selected);
            assert_eq!(
                candidate.classification,
                PnpOpenClClassification::IdentityVerified
            );
            assert!(candidate.identity.is_some());
            assert!(candidate.adapter.is_some());
        } else {
            assert!(
                candidate.adapter.is_none(),
                "non-verified candidates must not retain an adapter association"
            );
        }
    }
    assert!(
        report["candidates"]
            .as_array()
            .expect("candidate array")
            .iter()
            .all(|candidate| candidate["backend_ready"] == false)
    );

    eprintln!(
        "{}",
        serde_json::to_string_pretty(&report).expect("pretty PnP OpenCL report")
    );
}
