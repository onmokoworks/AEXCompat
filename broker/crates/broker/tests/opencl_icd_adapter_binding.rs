#![cfg(windows)]

use aexcompat_broker::opencl_icd_adapter_binding::{
    OpenClIcdBindingStatus, collect_opencl_icd_adapter_bindings,
    privacy_bounded_opencl_icd_binding_report,
};

#[test]
fn windows_opencl_icd_binding_self_test_is_privacy_bounded_and_fail_closed() {
    let collection = collect_opencl_icd_adapter_bindings();
    let report = privacy_bounded_opencl_icd_binding_report(&collection);
    let serialized = serde_json::to_string(&report).expect("serialize binding report");
    let reparsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("parse binding report");
    assert_eq!(report, reparsed);
    assert!(!serialized.contains(":\\"));

    for binding in &collection.bindings {
        if binding.status == OpenClIcdBindingStatus::Verified {
            assert!(binding.adapter.is_some());
            assert_eq!(binding.candidate.enabled, Some(true));
            assert!(binding.candidate.identity.is_some());
        } else {
            assert!(
                binding.adapter.is_none(),
                "non-verified bindings must not retain an adapter association"
            );
        }
    }

    eprintln!(
        "{}",
        serde_json::to_string_pretty(&report).expect("pretty binding report")
    );
}
