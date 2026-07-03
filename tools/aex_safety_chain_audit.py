#!/usr/bin/env python3
"""Audit the local no-load AEX compatibility evidence chain.

The audit reads JSON artifacts only. It never opens, copies, hashes, loads, or
executes AEX files, and it never invokes AE or OFX runtime paths.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
SAFETY_AUDIT_ROOT = LAB_ROOT / "target" / "safety-audit"

ROOTS = {
    "static_report": LAB_ROOT / "target" / "aex-static-probe",
    "fixture_manifest": LAB_ROOT / "target" / "fixture-review",
    "fixture_decision": LAB_ROOT / "target" / "fixture-approval",
    "worker_design": LAB_ROOT / "target" / "worker-design",
    "worker_selftest": LAB_ROOT / "target" / "worker-selftest",
    "dependency_review": LAB_ROOT / "target" / "dependency-review",
    "load_gate": LAB_ROOT / "target" / "load-gate",
    "native_loader_stub": LAB_ROOT / "target" / "native-loader-stub",
    "ofx_facade": LAB_ROOT / "target" / "ofx-facade",
    "ofx_noop_mock": LAB_ROOT / "target" / "ofx-noop-mock",
}

EXPECTED_KINDS = {
    "static_report": ("report_kind", "aex_static_probe"),
    "fixture_manifest": ("manifest_kind", "aex_fixture_review_manifest"),
    "worker_design": ("packet_kind", "aex_worker_sandbox_design_packet"),
    "worker_selftest": ("report_kind", "aex_no_load_worker_selftest"),
    "dependency_review": ("packet_kind", "aex_dependency_review_packet"),
    "load_gate": ("report_kind", "aex_load_gate_check"),
    "native_loader_stub": ("report_kind", "aex_native_loader_stub_report"),
    "ofx_facade": ("packet_kind", "aex_ofx_facade_deferred_packet"),
    "ofx_noop_mock": ("report_kind", "aex_ofx_noop_mock_selftest"),
}

RUNTIME_FALSE_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "dll_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
    "ofx_plugin_built",
    "ofx_describe_performed",
    "ofx_render_performed",
)


def path_has_traversal(path: Path) -> bool:
    return any(part in ("..", ".") for part in path.parts)


def resolve_under_root(path: Path, root: Path, *, must_exist: bool) -> Path:
    if path_has_traversal(path):
        raise ValueError("path must not contain traversal components")
    absolute = path if path.is_absolute() else LAB_ROOT / path
    resolved_root = root.resolve(strict=True)
    resolved = absolute.resolve(strict=must_exist)
    if not resolved.is_relative_to(resolved_root):
        raise ValueError(f"path must stay under {root}")
    return resolved


def validate_input_path(path: Path, root: Path, label: str) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError(f"{label} must have .json extension")
    return resolve_under_root(path, root, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("safety audit report must have .json extension")
    SAFETY_AUDIT_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, SAFETY_AUDIT_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(SAFETY_AUDIT_ROOT.resolve(strict=True)):
        raise ValueError(f"safety audit report parent must stay under {SAFETY_AUDIT_ROOT}")
    return resolved


def read_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return payload


def load_artifact(label: str, path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_input_path(path, ROOTS[label], label)
    return read_json(resolved), resolved


def artifact_errors(label: str, payload: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    expected = EXPECTED_KINDS.get(label)
    if expected:
        key, value = expected
        if payload.get(key) != value:
            errors.append(f"{label} {key} must be {value}")
    if payload.get("publication_status") != "local-only":
        errors.append(f"{label} publication_status must be local-only")
    for flag in RUNTIME_FALSE_FLAGS:
        if flag in payload and payload.get(flag) is not False:
            errors.append(f"{label} {flag} must be false")
    return errors


def chain_errors(artifacts: dict[str, dict[str, Any]]) -> list[str]:
    errors: list[str] = []
    static_report = artifacts["static_report"]
    if int(static_report.get("aex_count") or static_report.get("summary", {}).get("aex_count") or 0) <= 0:
        errors.append("static_report must contain at least one AEX entry")

    fixture_manifest = artifacts["fixture_manifest"]
    selected = fixture_manifest.get("selected_candidates")
    if not isinstance(selected, list) or not selected:
        errors.append("fixture_manifest must include selected_candidates")

    decision = artifacts["fixture_decision"]
    if decision.get("manifest_kind") not in {"aex_fixture_decision_manifest", "aex_fixture_approval_manifest"}:
        errors.append("fixture_decision manifest_kind must be decision or approval manifest")
    if decision.get("manifest_kind") == "aex_fixture_approval_manifest":
        errors.append("fixture_decision is approval; current no-load audit expects hold/reject state")
    if decision.get("approval_state") != "not_approved_for_load_gate":
        errors.append("fixture_decision approval_state must remain not_approved_for_load_gate")

    design = artifacts["worker_design"]
    primary = design.get("primary_review_candidate")
    if not isinstance(primary, dict):
        errors.append("worker_design must include primary_review_candidate")
    elif primary.get("approval_state") != "not_approved_for_load":
        errors.append("worker_design primary candidate must remain not_approved_for_load")

    selftest = artifacts["worker_selftest"]
    if selftest.get("worker_selftest_passed") is not True:
        errors.append("worker_selftest_passed must be true")
    steps = selftest.get("steps", [])
    blocked_steps = [step for step in steps if isinstance(step, dict) and step.get("step") == "blocked_load_aex"]
    if not blocked_steps or blocked_steps[0].get("code") != "blocked_action":
        errors.append("worker_selftest must prove load_aex failed with blocked_action")

    dependency_review = artifacts["dependency_review"]
    if dependency_review.get("native_load_recommendation") not in {
        "do_not_open_native_load_gate",
        "hold_native_load_until_dependency_review_complete",
    }:
        errors.append("dependency_review must keep native load blocked or pending in this no-load audit")

    load_gate = artifacts["load_gate"]
    if load_gate.get("gate_state") not in {
        "closed_missing_or_invalid_approval",
        "closed_dependency_review_or_invalid_approval",
    }:
        errors.append("load_gate gate_state must remain closed")
    if load_gate.get("approval_state") != "present":
        errors.append("load_gate should reference the hold decision artifact")
    if load_gate.get("dependency_review_state") != "present":
        errors.append("load_gate should reference the dependency review artifact")
    if load_gate.get("dependency_native_load_recommendation") != dependency_review.get("native_load_recommendation"):
        errors.append("load_gate dependency recommendation must match dependency_review")

    native_stub = artifacts["native_loader_stub"]
    if native_stub.get("stub_state") != "refused_gate_closed":
        errors.append("native_loader_stub stub_state must be refused_gate_closed")
    if native_stub.get("accepted_aex_path") is not None:
        errors.append("native_loader_stub accepted_aex_path must be null")

    ofx_facade = artifacts["ofx_facade"]
    if ofx_facade.get("facade_state") != "deferred_loader_not_ready":
        errors.append("ofx_facade facade_state must be deferred_loader_not_ready")
    if ofx_facade.get("ofx_route_action") != "no_op":
        errors.append("ofx_facade ofx_route_action must be no_op")

    ofx_mock = artifacts["ofx_noop_mock"]
    if ofx_mock.get("mock_state") != "mock_identity_completed_route_closed":
        errors.append("ofx_noop_mock mock_state must be mock_identity_completed_route_closed")
    identity = ofx_mock.get("identity_check")
    if not isinstance(identity, dict) or identity.get("pixel_match") is not True or identity.get("dimension_match") is not True:
        errors.append("ofx_noop_mock identity_check must prove pixel and dimension match")

    return errors


def summarize_artifact(label: str, payload: dict[str, Any], path: Path, errors: list[str]) -> dict[str, Any]:
    kind_key, kind_value = EXPECTED_KINDS.get(label, ("manifest_kind", payload.get("manifest_kind")))
    safety = {flag: payload.get(flag) for flag in RUNTIME_FALSE_FLAGS if flag in payload}
    state_keys = [
        "gate_state",
        "approval_state",
        "stub_state",
        "facade_state",
        "mock_state",
        "decision_state",
        "review_state",
        "native_load_recommendation",
        "worker_selftest_passed",
    ]
    states = {key: payload.get(key) for key in state_keys if key in payload}
    return {
        "label": label,
        "path": str(path),
        "kind_key": kind_key,
        "kind": payload.get(kind_key, kind_value),
        "states": states,
        "safety_flags": safety,
        "errors": errors,
    }


def build_audit(inputs: dict[str, Path]) -> dict[str, Any]:
    artifacts: dict[str, dict[str, Any]] = {}
    paths: dict[str, Path] = {}
    artifact_summaries: list[dict[str, Any]] = []
    all_errors: list[str] = []

    for label, path in inputs.items():
        payload, resolved = load_artifact(label, path)
        artifacts[label] = payload
        paths[label] = resolved
        errors = artifact_errors(label, payload)
        all_errors.extend(errors)
        artifact_summaries.append(summarize_artifact(label, payload, resolved, errors))

    chain = chain_errors(artifacts)
    all_errors.extend(chain)
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_no_load_safety_chain_audit",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "audit_passed": not all_errors,
        "audit_state": "no_load_chain_verified" if not all_errors else "no_load_chain_has_errors",
        "artifact_count": len(artifact_summaries),
        "artifacts": artifact_summaries,
        "chain_errors": chain,
        "errors": all_errors,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "notes": [
            "Audit reads JSON artifacts only.",
            "No AEX, AE, OFX, or image runtime path is loaded or invoked.",
            "The current expected state is no-load with native and OFX routes closed/deferred.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Audit no-load AEX compatibility safety chain")
    parser.add_argument("--static-report", required=True)
    parser.add_argument("--fixture-manifest", required=True)
    parser.add_argument("--fixture-decision", required=True)
    parser.add_argument("--worker-design", required=True)
    parser.add_argument("--worker-selftest", required=True)
    parser.add_argument("--dependency-review", required=True)
    parser.add_argument("--load-gate", required=True)
    parser.add_argument("--native-loader-stub", required=True)
    parser.add_argument("--ofx-facade", required=True)
    parser.add_argument("--ofx-noop-mock", required=True)
    parser.add_argument("--out", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    audit = build_audit(
        {
            "static_report": Path(args.static_report),
            "fixture_manifest": Path(args.fixture_manifest),
            "fixture_decision": Path(args.fixture_decision),
            "worker_design": Path(args.worker_design),
            "worker_selftest": Path(args.worker_selftest),
            "dependency_review": Path(args.dependency_review),
            "load_gate": Path(args.load_gate),
            "native_loader_stub": Path(args.native_loader_stub),
            "ofx_facade": Path(args.ofx_facade),
            "ofx_noop_mock": Path(args.ofx_noop_mock),
        }
    )
    written = write_json_create_new(Path(args.out), audit)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
