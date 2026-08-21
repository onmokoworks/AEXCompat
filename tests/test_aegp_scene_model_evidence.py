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
KINDS = ("discovery", "classic", "smart")


def reject_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def worker_path(name: str = "aex_worker.exe") -> Path:
    configured = os.environ.get("AEXCOMPAT_SCENE_MODEL_BUILD")
    candidates = [
        Path(configured) / name if configured else None,
        ROOT / "build" / "issue26-scene-model" / "Release" / name,
        ROOT / "target" / "minihost-build" / "Release" / name,
        ROOT / "target" / "minihost-build" / name,
    ]
    found = next((path for path in candidates if path and path.is_file()), None)
    assert found is not None, f"build the production worker first: {name}"
    return found


def test_scene_model_schema_is_valid() -> None:
    Draft202012Validator.check_schema(SCHEMA)




def test_scene_model_evidence_all_production_workers() -> None:
    validator = Draft202012Validator(SCHEMA)
    reports = []
    for kind in KINDS:
        completed = subprocess.run(
            [str(worker_path()), "--kind", kind, "--self-test-aegp-scene-model"],
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
        assert report["unsupported_slot"] == {
            "observed": True,
            "suite": "AEGP Effect Suite",
            "version": 4,
            "slot": 7,
            "error": 4,
            "call_count": 1,
            "distinct_from_invalid_handle": True,
        }
        assert report["unsupported_slots_preserved"] is True
        assert report["order"]["duplicate_layer_stack_rejected"] is True
        assert report["order"]["failure_state_unchanged"] is True
        assert report["order"]["failure_receipt_unchanged"] is True
        reports.append(report)
    assert len(reports) == 3


def test_scene_transaction_mid_apply_rollback_all_production_workers() -> None:
    for kind in KINDS:
        completed = subprocess.run(
            [
                str(worker_path()),
                "--kind",
                kind,
                "--self-test-aegp-scene-mutation-transactions",
            ],
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
        assert report["aegp_scene_mutation_transactions"] == "passed"
        assert report["mid_apply_rollback_observed"] is True
        assert report["rollback_failures"] == 0
        assert report["transaction_failure_byte_invariant"] is True
        assert report["generation_increment_once"] is True
