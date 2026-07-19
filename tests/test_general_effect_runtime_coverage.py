import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REPORT = ROOT / "analysis" / "GENERAL_EFFECT_RUNTIME_COVERAGE_2026-07-16.json"
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
ABI = ROOT / "target" / "pf-suite-abi-probe-build" / "pf-suite-abi.json"


def load_report():
    return json.loads(REPORT.read_text(encoding="utf-8"))


def test_schema_and_compiled_abi_are_grounded_in_probe_result():
    report = load_report()
    abi = json.loads(ABI.read_text(encoding="utf-8"))
    assert report["schema_version"] == 1
    assert report["scope"] == "general_effect_runtime_coverage"
    assert report["compiled_abi"]["architecture"] == abi["architecture"]
    assert report["compiled_abi"]["pointer_size"] == abi["scalars"]["pointer"]
    for suite in (
        "PF_WorldTransformSuite1",
        "PF_PathDataSuite1",
        "AEGP_RenderOptionsSuite1",
        "AEGP_WorldSuite3",
    ):
        assert report["compiled_abi"][suite] == {
            "size": abi["suites"][suite]["size"],
            "slots": abi["suites"][suite]["named_slot_count"],
        }


def test_source_wiring_matches_inventory():
    report = load_report()
    source = SOURCE.read_text(encoding="utf-8")
    suite_abi = (ROOT / "minihost" / "src" / "worker_suite_abi.hpp").read_text(encoding="utf-8")

    world = report["suites"]["PF_WorldTransformSuite1"]
    for slot in world["slots"]:
        assert f"g_world_transform_suite1.{slot['name']} = &{slot['callback']};" in source

    path = report["suites"]["PF_PathDataSuite1"]
    for index, callback in zip(path["implemented_slots"], path["callbacks"]):
        assert f"g_pf_path_data_suite1[{index}] = reinterpret_cast<void*>(&{callback});" in source

    sampling = report["suites"]["PF_SamplingSuites1"]["callbacks"]
    table_names = {"8": "g_sampling8_suite1", "16": "g_sampling16_suite1", "float": "g_sampling_float_suite1"}
    for depth, callbacks in sampling.items():
        for index, callback in enumerate(callbacks):
            assert f"{table_names[depth]}[{index}] = reinterpret_cast<void*>(&{callback});" in source

    fill_ranges = report["suites"]["PF_FillMatteSuite2"]["slots"]
    for group in fill_ranges:
        for index, callback in zip(range(group["range"][0], group["range"][1] + 1), group["callbacks"]):
            assert f"g_fill_matte_suite2[{index}] = reinterpret_cast<void*>(&{callback});" in source


def test_render_options_and_async_receipt_claims_match_current_source():
    report = load_report()
    source = SOURCE.read_text(encoding="utf-8")
    suite_abi = (ROOT / "minihost" / "src" / "worker_suite_abi.hpp").read_text(encoding="utf-8")
    assert report["suites"]["AEGP_RenderOptionsSuite1"]["status"] == "implemented_and_focused_runtime_tested"
    assert "static_assert(sizeof(AegpRenderOptionsSuite1) == 17 * sizeof(void*));" in suite_abi
    assert "receipt->render_options = *options;" in source
    assert "return publish_item_receipt(options, receipt);" in source

    async_state = report["suites"]["AEGP_WorldSuite3"]["async_receipt_integration"]
    assert async_state["status"] == "implemented_for_layer_argb8_argb16_argb32f_and_focused_runtime_tested"
    checkout_start = source.index(
        "int32_t __cdecl checkout_layer_frame_async(",
        source.index("int32_t __cdecl checkout_layer_frame_async(") + 1,
    )
    checkout = source[checkout_start:]
    checkout = checkout[: checkout.index("\n}") + 2]
    assert "snapshot_layer_render_options(options, snapshot)" in checkout
    assert "snapshot.world_type == 1 ? kPixelFormatArgb32" in checkout
    assert "snapshot.world_type == 2 ? kPixelFormatArgb64 : kPixelFormatArgb128" in checkout
    assert "return publish_async_receipt(pixel_format, receipt);" in checkout
    assert '{"AEGP Render Suite", 5, nullptr, &provide_render_suite5}' in source
    assert "&checkin_frame, &get_receipt_world" in source
    assert "aexcompat::world_registry::unregister_borrowed_view(" in source

    assert report["suites"]["AEGP_WorldSuite3"]["slots"][1]["range"] == [2, 8]
    for callback in (
        "aegp_world_get_type",
        "aegp_world_get_size",
        "aegp_world_get_rowbytes",
        "aegp_world_get_base_addr8",
        "aegp_world_get_base_addr16",
        "aegp_world_get_base_addr32",
        "aegp_world_fill_pf_world",
    ):
        assert f"&{callback}" in source


def test_artifact_hashes_and_sizes_authenticate_current_files():
    for artifact in load_report()["evidence_artifacts"]:
        path = ROOT / artifact["path"]
        assert path.stat().st_size == artifact["size_bytes"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == artifact["sha256"]


def test_current_artifact_snapshot_authenticates_worktree_and_production_tools():
    snapshot = load_report()["current_artifact_snapshot"]
    for name, artifact in snapshot.items():
        if name in {"captured_at", "note"}:
            continue
        path = ROOT / artifact["path"]
        assert path.stat().st_size == artifact["size_bytes"], name
        assert hashlib.sha256(path.read_bytes()).hexdigest() == artifact["sha256"], name
    assert "remain bound to their measured artifacts" in snapshot["note"]


def test_latest_recaptured_evidence_is_hash_bound_to_current_files():
    report = load_report()
    expected = {
        "PF_ADV_TIME_V4_RUNTIME_RESULT_2026-07-16.json",
        "PF_COMPOSITE_RECT_16_RUNTIME_RESULT_2026-07-16.json",
        "PF_AEGP_ASYNC_CANCEL_RUNTIME_RESULT_2026-07-16.json",
        "PF_AEGP_ASYNC_LAYER_RECEIPT_RUNTIME_RESULT_2026-07-16.json",
        "PF_AEGP_LAYER_RECEIPT_RUNTIME_RESULT_2026-07-16.json",
        "PF_SAMPLING_FILL_RUNTIME_RESULT_2026-07-16.json",
        "PF_SAMPLING_DEPTH_MATRIX_RESULT_2026-07-17.json",
        "SDK_COLORGRID_ARBITRARY_TIMELINE_RESULT_2026-07-17.json",
        "REAL_AEX_SMART_TIMED_MULTI_LAYER_RESULT_2026-07-17.json",
        "AEGP_ASYNC_RECEIPT_RUNTIME_RESULT_2026-07-16.json",
        "SDK_GRABBA_AEGP_RUNTIME_RESULT_2026-07-16.json",
    }
    assert {Path(artifact["path"]).name for artifact in report["latest_recaptured_evidence"]} == expected
    for artifact in report["latest_recaptured_evidence"]:
        path = ROOT / artifact["path"]
        assert path.stat().st_size == artifact["size_bytes"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == artifact["sha256"]

    current = report["current_artifact_snapshot"]
    for artifact in report["latest_recaptured_evidence"]:
        evidence = json.loads((ROOT / artifact["path"]).read_text(encoding="utf-8"))
        authenticated = evidence.get("authenticated_current_sampling_artifacts", evidence.get("authenticated_artifacts"))
        if authenticated and "source" in authenticated and "worker" in authenticated:
            assert authenticated["source"]["sha256"] == current["source"]["sha256"]
            assert authenticated["worker"]["sha256"] == current["render_worker"]["sha256"]


def test_report_does_not_overclaim_real_ae_or_complete_compatibility():
    report = load_report()
    assert report["overall"]["real_ae_pixel_oracle"] == "not_run"
    assert report["overall"]["complete_after_effects_compatibility"] is False
    assert "does not claim pixel equivalence" in report["overall"]["statement"]
    assert report["suites"]["PF_WorldTransformSuite1"]["slots"][0]["status"] == (
        "implemented_argb8_argb16_argb32f_and_native_runtime_tested"
    )
    assert report["suites"]["PF_SamplingSuites1"]["real_ae_pixel_oracle"] == "pending"
    assert report["suites"]["PF_FillMatteSuite2"]["real_ae_pixel_oracle"] == "pending"


def test_sampling_current_runtime_is_authenticated_and_history_is_separate():
    report = load_report()
    sampling = next(
        artifact
        for artifact in report["evidence_artifacts"]
        if artifact["path"].endswith("pf_sampling_probe.aex")
    )
    runtime_path = ROOT / sampling["runtime_evidence"]
    runtime = json.loads(runtime_path.read_text(encoding="utf-8"))
    authenticated = runtime["authenticated_current_sampling_artifacts"]

    assert sampling["sha256"] == runtime["sampling_probe"]["sha256"]
    assert sampling["sha256"] == authenticated["probe"]["sha256"]
    assert sampling["size_bytes"] == authenticated["probe"]["size_bytes"]
    assert sampling["evidence"] == "authenticated_aexcompat_runtime_snapshot"
    assert sampling["historical_runtime_artifact_sha256"] != sampling["sha256"]
    assert report["suites"]["PF_SamplingSuites1"]["runtime_evidence"] == sampling["runtime_evidence"]
    assert runtime["sampling_probe"]["exit_code"] == 0
    assert runtime["sampling_probe"]["status"] == "render_completed"
    assert runtime["sampling_probe"]["render_error"] == 0
    assert runtime["sampling_probe"]["guard_bytes_intact"] is True
    assert runtime["sampling_probe"]["suite_leases_balanced"] is True

    record_auth = report["suites"]["PF_SamplingSuites1"]["runtime_record_authentication"]
    for artifact in (
        record_auth["record"],
        *record_auth["persisted_reports"],
        *record_auth["persisted_outputs"],
    ):
        path = ROOT / artifact["path"]
        assert path.stat().st_size == artifact["size_bytes"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == artifact["sha256"]
    assert record_auth["transient_outputs_currently_available"] is True

    current = report["current_artifact_snapshot"]
    assert authenticated["source"]["sha256"] == current["source"]["sha256"]
    assert authenticated["worker"]["sha256"] == current["render_worker"]["sha256"]
    assert authenticated["source"]["size_bytes"] == current["source"]["size_bytes"]
    assert authenticated["worker"]["size_bytes"] == current["render_worker"]["size_bytes"]
    assert report["suites"]["PF_SamplingSuites1"]["status"].endswith(
        "authenticated_current_worker_runtime_tested"
    )
    assert "recaptured with the current source" in report["suites"]["PF_SamplingSuites1"]["artifact_note"]
