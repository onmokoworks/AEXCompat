from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
REPORT = (ROOT / "minihost" / "src" / "worker_report.cpp").read_text(encoding="utf-8")
HEADER = (ROOT / "minihost" / "src" / "worker_report.hpp").read_text(encoding="utf-8")
L2 = source_owners.l2_translation_unit_text()
BROKER = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
SELECTOR = (ROOT / "minihost" / "src" / "worker_selector_dispatch.cpp").read_text(
    encoding="utf-8"
)


def test_l2_report_carries_bounded_suite_diagnostics():
    assert "missing_suites_json" in HEADER
    assert "suite_timeline_json" in HEADER
    assert "c.missing_suites_json = missing_suites_report_json()" in L2
    assert "c.suite_timeline_json = suite_timeline_report_json()" in L2
    assert "c.missing_suites_json << c.suite_timeline_json" in REPORT
    assert '\\"missing_suites_truncated\\":' in (
        ROOT / "minihost" / "src" / "worker_suite_registry.cpp"
    ).read_text(encoding="utf-8")
    assert '\\"suite_timeline_truncated\\":' in (
        ROOT / "minihost" / "src" / "worker_suite_registry.cpp"
    ).read_text(encoding="utf-8")


def test_broker_preserves_bounded_timeline_schema_on_failure():
    assert "const MAX_SUITE_TIMELINE_EVENTS: usize = 512;" in BROKER
    assert "fn propagate_suite_timeline(" in BROKER
    assert "timeline.len() >= MAX_SUITE_TIMELINE_EVENTS" in BROKER
    assert "const MAX_SUITE_NAME_LEN: usize = 64;" in BROKER
    assert "const MAX_SUITE_VERSION: i64 = u16::MAX as i64;" in BROKER
    assert 'let Some(selector) = event["selector"]' in BROKER
    assert ".filter(|selector| schema_safe_suite_selector(selector))" in BROKER
    assert '"sequence": sequence' in BROKER
    assert '"action": action' in BROKER
    assert '"result": result' in BROKER
    assert "propagate_suite_timeline(&mut diagnostics, report);" in BROKER
    assert 'diagnostics["suite_timeline_truncated"] = Value::Bool(truncated);' in BROKER
    assert 'diagnostics["missing_suites_truncated"] = Value::Bool(truncated);' in BROKER
    # The structured report remains an Err for a non-ok worker; telemetry is
    # evidence, never a success downgrade.
    assert 'if isolated.classification.as_str() != "ok"' in BROKER
    assert 'return Err(invalid(format!(' in BROKER


def test_module_audit_rejection_is_bounded_structured_and_stays_fail_closed():
    audit = (ROOT / "minihost" / "src" / "runtime_module_audit.cpp").read_text(
        encoding="utf-8"
    )
    assert "kMaxAuditFailureRejections = 16" in audit
    assert '"canonical_path_token\\":' in audit
    assert '"path_class\\":\\"' in audit
    assert "outside_allowed_roots_or_unapproved_policy" in audit
    assert "module_audit_failure_json()" in L2
    assert '"module_audit_failure\\":' in REPORT
    assert "fn module_audit_failure_summary(" in BROKER
    assert '"selector_phase": selector_phase' in BROKER
    failure_copy = BROKER.index("module_audit_failure_summary(report")
    nonzero = BROKER.index('if isolated.classification.as_str() != "ok"', failure_copy)
    assert failure_copy < nonzero
    assert 'return Err(invalid(format!(' in BROKER[nonzero:]


def test_selector_invocation_distinguishes_normal_return_from_seh_containment():
    assert "kMaxSelectorInvocationDiagnostics = 64" in (
        ROOT / "minihost" / "src" / "worker_selector_dispatch.hpp"
    ).read_text(encoding="utf-8")
    assert '"invocation_completed_normally\\":' in SELECTOR
    assert '"raw_return_code\\":' in SELECTOR
    assert '"host_result_code\\":' in SELECTOR
    assert '"seh_caught\\":' in SELECTOR
    assert '"seh_code\\":' in SELECTOR
    assert '"fault_module_class\\":' in SELECTOR
    assert '"plugin_rva\\":' in SELECTOR
    assert '"access_type\\":' in SELECTOR
    assert '"fault_address\\":' in SELECTOR
    assert '"registers\\":' in SELECTOR
    assert '"stack_pointer_values\\":' in SELECTOR
    assert '"global_data_handoff\\":{' in SELECTOR
    assert '"input_at_entry\\":' in SELECTOR
    assert '"output_after_return\\":' in SELECTOR
    assert '"same_identity_as_previous_output\\":' in SELECTOR
    assert '"effect_ref_at_entry\\":' in SELECTOR
    assert '"same_identity_as_global_setup_entry\\":' in SELECTOR
    assert "kInEffectRefOffset = 184" in SELECTOR
    assert '"appl_id_at_entry\\":' in SELECTOR
    assert '"printable_code\\":' in SELECTOR
    assert '"hex_u32\\":\\\"0x"' in SELECTOR
    assert '"same_value_as_global_setup_entry\\":' in SELECTOR
    assert "host_setting_source" in SELECTOR
    assert "worker_effect_bootstrap" in SELECTOR
    assert "kInApplicationIdOffset = 204" in SELECTOR
    assert '"version_at_entry\\":' in SELECTOR
    assert '"raw_packed_u32\\":\\\"0x"' in SELECTOR
    assert '"major\\":' in SELECTOR
    assert '"minor\\":' in SELECTOR
    assert "kInSpecVersionOffset = 196" in SELECTOR
    assert "kMaxSelectorStackValues = 6" in (
        ROOT / "minihost" / "src" / "worker_selector_dispatch.hpp"
    ).read_text(encoding="utf-8")
    assert "BCryptGenRandom" in SELECTOR
    assert "capture_global_data_state" in SELECTOR
    assert "process_local_token" in SELECTOR
    assert "safe_pointer_classification" in BROKER
    assert "safe_global_data_handoff" in BROKER
    assert "safe_effect_ref_entry" in BROKER
    assert "safe_application_id_entry" in BROKER
    assert "safe_spec_version_entry" in BROKER
    assert "kMaxHostCallbackTimelineRecords = 128" in (
        ROOT / "minihost" / "src" / "worker_selector_dispatch.hpp"
    ).read_text(encoding="utf-8")
    assert '"host_callback_timeline\\":{' in SELECTOR
    assert '"call_count\\":' in SELECTOR
    assert '"classification\\":\\\""' in SELECTOR
    assert "record_host_callback_invocation" in SELECTOR
    assert "fn propagate_host_callback_timeline(" in BROKER
    assert "kMaxExtendedLookupTimelineRecords = 128" in (
        ROOT / "minihost" / "src" / "worker_selector_dispatch.hpp"
    ).read_text(encoding="utf-8")
    assert '"extended_lookup_timeline\\":{' in SELECTOR
    assert '"opaque_table_classification\\":\\\""' in SELECTOR
    assert '"raw_private_table_state\\":\\\""' in SELECTOR
    assert '"windows_resource_source_state\\":\\\""' in SELECTOR
    assert '"lookup_id\\":' in SELECTOR
    assert '"outcome\\":\\\""' in SELECTOR
    assert "record_extended_lookup_diagnostic" in L2
    assert "ExtendedLookupStringTableState::valid" in L2
    assert "ExtendedLookupStringTableState::none" in L2
    assert "ExtendedLookupStringTableState::invalid" in L2
    assert "classify_extended_lookup_table" in L2
    assert "classify_loaded_module_provenance" in L2
    assert "VirtualQuery(table" in SELECTOR
    assert "GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS" in SELECTOR
    for classification in (
        "null",
        "active_effect_module",
        "active_resource_module",
        "other_loaded_sealed_module",
        "other_loaded_system_module",
        "unrecognized",
    ):
        assert classification in SELECTOR
    assert "fn propagate_extended_lookup_timeline(" in BROKER
    assert (
        "propagate_extended_lookup_timeline(&mut diagnostics, report);"
        in BROKER
    )
    assert (
        "propagate_extended_lookup_timeline(&mut diagnostics, &final_report);"
        in BROKER
    )
    assert "kMaxExtendedAllocationTimelineRecords = 128" in (
        ROOT / "minihost" / "src" / "worker_selector_dispatch.hpp"
    ).read_text(encoding="utf-8")
    assert '"extended_allocation_timeline\\":{' in SELECTOR
    assert '"global_setup_live_allocation_count\\":' in SELECTOR
    assert '"invalid_frees\\":' in SELECTOR
    assert '"double_frees\\":' in SELECTOR
    assert "observe_extended_allocation" in SELECTOR
    assert "observe_extended_free" in SELECTOR
    assert "fn propagate_extended_allocation_timeline(" in BROKER
    assert "c.selector_invocations_json = selector_invocations_report_json()" in L2
    assert "c.selector_invocations_json" in REPORT
    assert "fn propagate_selector_invocations(" in BROKER
    normal_return = SELECTOR.index(
        "if (invocation_completed_normally) *invocation_completed_normally = true;"
    )
    audit_result = SELECTOR.index("return g_audit_passed() ? error : kAuditFailure;")
    assert normal_return < audit_result
