import json
import os
import subprocess
from pathlib import Path

from jsonschema import Draft202012Validator


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = json.loads(
    (ROOT / "schemas" / "aegp-scene-model-selftest.schema.json").read_text(
        encoding="utf-8"
    )
)
WORKERS = ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe")


def reject_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def worker_path(name: str) -> Path:
    configured = os.environ.get("AEXCOMPAT_SCENE_MODEL_BUILD")
    candidates = [
        Path(configured) / name if configured else None,
        ROOT / "build" / "issue26-scene-model" / "Release" / name,
        ROOT / "target" / "minihost-build" / "Release" / name,
    ]
    found = next((path for path in candidates if path and path.is_file()), None)
    assert found is not None, f"build the production worker first: {name}"
    return found


def test_scene_model_schema_is_valid() -> None:
    Draft202012Validator.check_schema(SCHEMA)


def test_scene_model_source_contract_connects_registry_scheduler_receipts() -> None:
    runtime = (
        ROOT / "minihost" / "src" / "worker_aegp_staged_item_runtime.cpp"
    ).read_text(encoding="utf-8")
    receipts = (
        ROOT / "minihost" / "src" / "worker_render_receipts.cpp"
    ).read_text(encoding="utf-8")
    transaction = (
        ROOT / "minihost" / "src" / "worker_aegp_scene_transaction.hpp"
    ).read_text(encoding="utf-8")
    routing = (
        ROOT / "minihost" / "src" / "worker_custom_selftest_routing.cpp"
    ).read_text(encoding="utf-8")
    wiring = (
        ROOT / "minihost" / "src" / "worker_l2_render_abi.cpp"
    ).read_text(encoding="utf-8")
    for marker in (
        "register_scene_item(",
        "publish_scene_stage_world(",
        "stable_scene_identity(",
        "registration_graph_has_cycle(",
        "dependency_identity_hash",
        "effect_order_hash",
        "invalidate_scene_generation(",
        "stale_stage_invalidations",
        "invalid_handle_rejections",
        "render_receipts::invalidate_scene_generation",
        "notify_generation_invalidated(project_id_",
        'L"--self-test-aegp-scene-model"',
        "unsupported_slots_preserved",
        "&publish_scene_scheduler_stage",
    ):
        assert (
            marker in runtime
            or marker in receipts
            or marker in transaction
            or marker in routing
            or marker in wiring
        )
    assert "register_scene_item(" in wiring
    assert "publish_scene_stage_world(" in wiring


def test_scene_model_evidence_all_production_workers() -> None:
    validator = Draft202012Validator(SCHEMA)
    reports = []
    for name in WORKERS:
        completed = subprocess.run(
            [str(worker_path(name)), "--self-test-aegp-scene-model"],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=60,
            check=False,
        )
        assert completed.returncode == 0, completed.stderr or completed.stdout
        report = json.loads(
            completed.stdout, object_pairs_hook=reject_duplicate_keys
        )
        validator.validate(report)
        assert report["generation"]["after"] == report["generation"]["before"] + 1
        reports.append(report)
    assert len(reports) == 3
