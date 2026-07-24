#!/usr/bin/env python3
"""Closed native-loader stub for AEX compatibility work.

This tool reads load-gate JSON evidence only. It intentionally accepts no AEX
path and never opens, copies, hashes, loads, or executes AEX files.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
LOAD_GATE_ROOT = LAB_ROOT / "target" / "load-gate"
LOADER_STUB_ROOT = LAB_ROOT / "target" / "native-loader-stub"

SAFETY_FLAGS = (
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
)

REQUIRED_BLOCKED_ACTIONS = (
    "load_aex_dll",
    "call_EffectMain",
    "render_with_aex",
    "route_through_ofx",
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


def validate_load_gate_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("load gate report must have .json extension")
    return resolve_under_root(path, LOAD_GATE_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("native loader stub report must have .json extension")
    LOADER_STUB_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, LOADER_STUB_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(LOADER_STUB_ROOT.resolve(strict=True)):
        raise ValueError(f"native loader stub report parent must stay under {LOADER_STUB_ROOT}")
    return resolved


def load_gate_report(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_load_gate_path(path)
    with resolved.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("load gate report must be a JSON object")
    return payload, resolved


def validate_load_gate_report(report: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if report.get("report_kind") != "aex_load_gate_check":
        errors.append("load gate report_kind must be aex_load_gate_check")
    if report.get("publication_status") != "local-only":
        errors.append("load gate publication_status must be local-only")
    for flag in SAFETY_FLAGS:
        if report.get(flag) is not False:
            errors.append(f"load gate {flag} must be false")
    candidate = report.get("primary_review_candidate")
    if not isinstance(candidate, dict):
        errors.append("load gate primary_review_candidate must be an object")
    blocked = report.get("blocked_actions", [])
    if not isinstance(blocked, list):
        errors.append("load gate blocked_actions must be a list")
    else:
        for action in REQUIRED_BLOCKED_ACTIONS:
            if action not in blocked:
                errors.append(f"load gate must keep {action} blocked")
    return errors


def build_stub_report(gate_report: dict[str, Any], gate_report_path: Path) -> dict[str, Any]:
    evidence_errors = validate_load_gate_report(gate_report)
    gate_state = gate_report.get("gate_state")
    gate_errors = gate_report.get("gate_errors", [])
    if evidence_errors:
        stub_state = "invalid_evidence_refused"
        refusal_reasons = evidence_errors
    elif gate_state != "preconditions_satisfied_no_load_performed":
        stub_state = "refused_gate_closed"
        refusal_reasons = [f"load gate state is {gate_state}"] + [str(error) for error in gate_errors]
    else:
        stub_state = "stub_ready_no_load_performed"
        refusal_reasons = [
            "preconditions are reported satisfied, but this stub intentionally performs no native load",
            "a separate loader implementation and explicit user action are still required",
        ]

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_stub_report",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_load_gate_report": str(gate_report_path),
        "source_gate_state": gate_state,
        "source_approval_state": gate_report.get("approval_state"),
        "primary_review_candidate": gate_report.get("primary_review_candidate"),
        "stub_state": stub_state,
        "loader_action": "no_op",
        "refusal_reasons": refusal_reasons,
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "accepted_aex_path": None,
        "blocked_actions": [
            "accept_aex_path",
            "open_aex_file",
            "copy_selected_aex_fixture",
            "load_aex_dll",
            "call_EffectMain",
            "dispatch_PF_Cmd",
            "render_with_aex",
            "route_through_ofx",
        ],
        "allowed_next_actions": [
            "manual fixture approval review",
            "repeat load gate check after approval artifacts change",
        ]
        if stub_state != "stub_ready_no_load_performed"
        else [
            "design a separate loader implementation that still defaults closed",
            "request explicit user approval before accepting any AEX path",
        ],
        "notes": [
            "This stub reads load-gate JSON evidence only.",
            "It accepts no AEX path.",
            "It opens, copies, hashes, loads, and executes no AEX file.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run closed no-load AEX native loader stub")
    parser.add_argument("--load-gate", required=True, help="Load gate report JSON under target/load-gate")
    parser.add_argument("--out", required=True, help="Create-new stub report JSON under target/native-loader-stub")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    gate_report, gate_path = load_gate_report(Path(args.load_gate))
    report = build_stub_report(gate_report, gate_path)
    written = write_json_create_new(Path(args.out), report)
    print(written)
    return 1 if report["stub_state"] == "invalid_evidence_refused" else 0


if __name__ == "__main__":
    raise SystemExit(main())
