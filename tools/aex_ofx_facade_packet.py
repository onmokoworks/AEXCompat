#!/usr/bin/env python3
"""Deferred OFX facade packet for AEX compatibility planning.

The packet reads native-loader stub JSON only. It never opens, copies, hashes,
loads, executes, renders, or routes through AEX/OFX.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
LOADER_STUB_ROOT = LAB_ROOT / "target" / "native-loader-stub"
OFX_FACADE_ROOT = LAB_ROOT / "target" / "ofx-facade"

SAFETY_FLAGS = (
    "native_load_enabled",
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
    "aex_file_opened",
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


def validate_loader_stub_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("native loader stub report must have .json extension")
    return resolve_under_root(path, LOADER_STUB_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("OFX facade packet must have .json extension")
    OFX_FACADE_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, OFX_FACADE_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(OFX_FACADE_ROOT.resolve(strict=True)):
        raise ValueError(f"OFX facade packet parent must stay under {OFX_FACADE_ROOT}")
    return resolved


def load_loader_stub(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_loader_stub_path(path)
    with resolved.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError("native loader stub report must be a JSON object")
    return payload, resolved


def validate_loader_stub(stub: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if stub.get("report_kind") != "aex_native_loader_stub_report":
        errors.append("loader stub report_kind must be aex_native_loader_stub_report")
    if stub.get("publication_status") != "local-only":
        errors.append("loader stub publication_status must be local-only")
    if stub.get("loader_action") != "no_op":
        errors.append("loader_action must be no_op")
    if stub.get("accepted_aex_path") is not None:
        errors.append("accepted_aex_path must be null")
    for flag in SAFETY_FLAGS:
        if stub.get(flag) is not False:
            errors.append(f"loader stub {flag} must be false")
    return errors


def build_ofx_mapping_plan() -> dict[str, Any]:
    return {
        "state": "planning_only",
        "host_surface": "OFX Image Effect facade",
        "planned_describe_phase": [
            "Expose static placeholder identity effect only after an OFX host harness exists.",
            "Do not surface AEX-derived parameter metadata until native loader gate and PiPL parser review are complete.",
        ],
        "planned_render_phase": [
            "Route only generated image fixtures through controller-owned buffers after render gate opens.",
            "Keep AEX render equivalence claims disabled until image-output validation artifacts exist.",
        ],
        "blocked_until": [
            "fixture approval",
            "native loader implementation with explicit user approval",
            "AEX describe/parameter extraction gate",
            "render validation gate",
        ],
    }


def build_packet(stub_report: dict[str, Any], stub_report_path: Path) -> dict[str, Any]:
    errors = validate_loader_stub(stub_report)
    stub_state = stub_report.get("stub_state")
    if errors:
        facade_state = "invalid_loader_evidence_deferred"
        refusal_reasons = errors
    elif stub_state != "stub_ready_no_load_performed":
        facade_state = "deferred_loader_not_ready"
        refusal_reasons = [f"loader stub state is {stub_state}"] + [str(reason) for reason in stub_report.get("refusal_reasons", [])]
    else:
        facade_state = "deferred_pending_ofx_facade_review"
        refusal_reasons = [
            "loader stub is ready but still performs no native load",
            "OFX route requires a separate reviewed facade implementation and explicit user approval",
        ]

    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_ofx_facade_deferred_packet",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_native_loader_stub": str(stub_report_path),
        "source_stub_state": stub_state,
        "primary_review_candidate": stub_report.get("primary_review_candidate"),
        "facade_state": facade_state,
        "ofx_route_action": "no_op",
        "refusal_reasons": refusal_reasons,
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "mapping_plan": build_ofx_mapping_plan(),
        "blocked_actions": [
            "accept_aex_path",
            "load_aex_dll",
            "call_EffectMain",
            "dispatch_PF_Cmd",
            "build_ofx_binary",
            "ofx_describe_from_aex",
            "ofx_render_with_aex",
            "route_through_ofx",
        ],
        "allowed_next_actions": [
            "write OFX facade design notes",
            "build a separate OFX no-op host/mock only if it never references AEX files",
            "repeat this packet after loader gate evidence changes",
        ],
        "notes": [
            "This packet is OFX planning only.",
            "It reads native-loader stub JSON evidence only.",
            "No AEX or OFX runtime path is opened, loaded, built, invoked, or rendered.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Create deferred OFX facade packet from native-loader stub")
    parser.add_argument("--loader-stub", required=True, help="Native loader stub JSON under target/native-loader-stub")
    parser.add_argument("--out", required=True, help="Create-new OFX facade packet under target/ofx-facade")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    stub, stub_path = load_loader_stub(Path(args.loader_stub))
    packet = build_packet(stub, stub_path)
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
