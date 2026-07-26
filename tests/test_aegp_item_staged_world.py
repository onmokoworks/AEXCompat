import json
import os
import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.L2_MAIN
RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_staged_item_runtime.cpp"
HEADER = ROOT / "minihost" / "src" / "worker_aegp_staged_item_runtime.hpp"
ENTRY_WIRING = ROOT / "minihost" / "src" / "worker_entry_wiring.cpp"
RECEIPTS_HEADER = ROOT / "minihost" / "src" / "worker_render_receipts.hpp"
RECEIPTS = ROOT / "minihost" / "src" / "worker_render_receipts.cpp"
LAYER_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_layer_render_runtime.cpp"
LAYER_HEADER = ROOT / "minihost" / "src" / "worker_aegp_layer_render_runtime.hpp"


def _worker() -> Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-timed-layers" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build" / "aex_render_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)


def test_item_checkout_uses_immutable_host_stage_not_reentrant_render():
    host = SOURCE.read_text(encoding="utf-8")
    text = RUNTIME.read_text(encoding="utf-8")
    entry_wiring = ENTRY_WIRING.read_text(encoding="utf-8")
    receipts_header = RECEIPTS_HEADER.read_text(encoding="utf-8")
    for marker in (
        "struct StagedItemWorld",
        "bool publish_world(",
        "struct ResolveSnapshot",
        "bool resolve_plan(",
        "int32_t transform(",
        "render_receipts::register_receipt(",
        "staged_source_pin",
        "thread_local std::vector<ItemRenderStackKey> g_render_stack",
        "resolve_plan(snapshot, plan)",
    ):
        assert marker in text
    for marker in ("struct StagedItemWorld", "ItemRenderStackKey", "snapshot_world(",
                   "struct ResolveSnapshot", "int32_t transform("):
        assert marker not in host
    assert "render_loaded_effect_item_receipt(" not in host
    assert "g_loaded_effect_receipt_mutex" not in host
    assert "g_loaded_effect_receipt_active" not in host
    assert "exercise_loaded_effect_item_receipt(" not in entry_wiring
    assert "loaded_effect_receipt_" not in entry_wiring
    assert "loaded_effect_receipt_" not in receipts_header
    assert "struct Hooks" in HEADER.read_text(encoding="utf-8")


def test_stage_key_covers_render_identity_and_generation():
    text = RUNTIME.read_text(encoding="utf-8")
    for marker in (
        "value.item == item",
        "value.stage_kind == kind",
        "value.effect_instance == effect_instance",
        "same_rational(value.time_step, options.time_step)",
        "value.quality == options.render_quality",
        "value.guide_layers == options.render_guide_layers",
        "value.pixel_format == pixel_format",
        "value.project_generation == project_generation",
        "left.item_identity == right.item_identity",
        "left.stage_kind == right.stage_kind",
        "left.effect_instance == right.effect_instance",
        "value.width == width",
        "value.height == height",
        "value.rowbytes == tight_rowbytes",
        "stage_generation",
        "stage_identity_hash(",
    ):
        assert marker in text


def test_scheduler_policy_dag_boundaries_and_limits_are_explicit():
    header = HEADER.read_text(encoding="utf-8")
    text = RUNTIME.read_text(encoding="utf-8")
    for marker in (
        "enum class SamplingPolicy : uint8_t { exact, hold, nearest }",
        "enum class StageKind : uint8_t { upstream, all_effects, downstream, final_item }",
        "bool register_item(",
        "bool has_item_registration(",
        "bool publish_stage_world(",
        "int32_t publish_registered_receipt(",
    ):
        assert marker in header
    for marker in (
        "compare_positive_fractions(",
        "source_order < 0",
        "policy == SamplingPolicy::hold",
        "policy == SamplingPolicy::nearest",
        "registered || !allow_test_synthetic",
        "plan.trace_hash = hash_mix(context.trace_hash, static_cast<uint8_t>(policy))",
        "requested_time = normalize_rational(options.time)",
        "options.downsample_x",
        "options.roi.left",
        "options.field",
        "for (void* dependency : registration->dependencies)",
        "for (uint64_t effect_instance : registration->effect_instances)",
        "StageKind::upstream, StageKind::all_effects",
        "context.chain.back() == item",
        "g_direct_cycles_rejected",
        "g_indirect_cycles_rejected",
        "kMaxResolveDepth = 8",
        "kMaxResolvedStages = 24",
        "kMaxSchedulerBytes = render_receipts::kMaxReceiptBytes",
        "kMaxResolveTime = std::chrono::milliseconds(250)",
        "g_world_bytes > kMaxSchedulerBytes - tight_bytes",
        "stage_kind != StageKind::final_item && effect_instance == 0",
    ):
        assert marker in text
    assert "ensure_item_registered" not in text
    assert "reinterpret_cast<uintptr_t>(item)" not in text
    assert "select_stage(context.snapshot, item, kind, 0" not in text


def test_effect_boundaries_publish_into_scheduler_and_receipts_keep_evidence():
    layer = LAYER_RUNTIME.read_text(encoding="utf-8")
    layer_header = LAYER_HEADER.read_text(encoding="utf-8")
    receipts_header = RECEIPTS_HEADER.read_text(encoding="utf-8")
    receipts = RECEIPTS.read_text(encoding="utf-8")
    for marker in (
        "publish_scheduler_stage",
        "StageKind::upstream",
        "StageKind::downstream",
        "StageKind::all_effects",
        "loaded_receipt->has_stage_evidence = true",
        "loaded_receipt->stage_identity_hash = stage_identity_hash",
    ):
        assert marker in layer
    assert "publish_scheduler_stage" in layer_header
    assert "prepare_staged_item" in layer_header
    assert "current_effect_instance" in layer_header
    assert "g_hooks.current_effect_instance(options)" in layer
    assert "effect_instance == 0" in layer
    for marker in (
        "bool has_stage_evidence",
        "uint64_t stage_identity_hash",
        "uint64_t item_identity",
        "uint64_t effect_instance",
        "uint64_t trace_hash",
        "suite_abi::AegpTime requested_time",
        "suite_abi::AegpTime source_time",
        "uint32_t resolved_stage_count",
        "uint32_t resolved_depth",
        "uint8_t sampling_policy",
    ):
        assert marker in receipts_header
    assert "output.stage_identity_hash = receipt.draft->stage_identity_hash" in receipts
    assert "output.trace_hash = receipt.draft->trace_hash" in receipts


def test_native_item_stage_pixel_oracle():
    worker = _worker()
    assert worker is not None, "build the production render worker first"
    completed = subprocess.run(
        [str(worker), "--self-test-aegp-item-staged-worlds"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    def reject_duplicate_keys(pairs):
        result = {}
        for key, value in pairs:
            assert key not in result, f"duplicate JSON key: {key}"
            result[key] = value
        return result

    report = json.loads(completed.stdout, object_pairs_hook=reject_duplicate_keys)
    assert report["aegp_item_staged_worlds"] == "passed"
    assert report["immutable_stage"] is True
    assert report["reentrant_render_used"] is False
    assert report["published"] >= 4
    assert report["cache_hits"] >= 9
    assert report["cache_misses"] >= 4
    assert report["cycles_rejected"] >= 1
    assert report["exact_hits"] > 0
    assert report["hold_hits"] > 0
    assert report["nearest_hits"] > 0
    assert report["direct_cycles_rejected"] > 0
    assert report["indirect_cycles_rejected"] > 0
    assert report["depth_limit_rejections"] > 0
    assert report["stage_limit_rejections"] > 0
    assert report["effect_boundary_rejections"] > 0
    assert report["partial_failures"] > 0
    assert report["cleanup_count"] > 0
    assert report["in_flight"] == 0
    assert report["max_in_flight"] >= 2
    assert report["registered_items"] == 0
    assert report["cached_stages"] == 0
    assert report["cached_bytes"] == 0
    assert report["last_trace_hash"] != 0
    assert report["last_stage_identity_hash"] != 0
    assert report["last_resolved_stages"] == 8
    assert report["max_resolved_depth"] >= 2
