#!/usr/bin/env python3
"""Create a no-load AEX worker/sandbox design packet from a fixture manifest."""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


LAB_ROOT = Path(__file__).resolve().parents[1]
MANIFEST_ROOT = LAB_ROOT / "target" / "fixture-review"
PACKET_ROOT = LAB_ROOT / "target" / "worker-design"

SAFETY_FLAGS = (
    "native_load_performed",
    "render_performed",
    "ae_invoked",
    "ofx_route_invoked",
    "private_payload_copied",
)

BLOCKED_ACTIONS = [
    "copy_selected_aex_fixture",
    "load_aex_dll",
    "call_EffectMain",
    "dispatch_PF_Cmd",
    "start_after_effects",
    "render_with_aex",
    "route_through_ofx",
    "publish_binary_or_payload_metadata",
]


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


def validate_manifest_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("source manifest must have .json extension")
    return resolve_under_root(path, MANIFEST_ROOT, must_exist=True)


def validate_output_path(path: Path) -> Path:
    if path.suffix.lower() != ".json":
        raise ValueError("output path must have .json extension")
    PACKET_ROOT.mkdir(parents=True, exist_ok=True)
    resolved = resolve_under_root(path, PACKET_ROOT, must_exist=False)
    resolved.parent.mkdir(parents=True, exist_ok=True)
    if not resolved.parent.resolve(strict=True).is_relative_to(PACKET_ROOT.resolve(strict=True)):
        raise ValueError(f"output parent must stay under {PACKET_ROOT}")
    return resolved


def load_manifest(path: Path) -> tuple[dict[str, Any], Path]:
    resolved = validate_manifest_path(path)
    with resolved.open("r", encoding="utf-8") as handle:
        manifest = json.load(handle)
    if not isinstance(manifest, dict):
        raise ValueError("source manifest must be a JSON object")
    return manifest, resolved


def validate_manifest(manifest: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if manifest.get("manifest_kind") != "aex_fixture_review_manifest":
        errors.append("source manifest_kind must be aex_fixture_review_manifest")
    if int(manifest.get("schema_version") or 0) < 1:
        errors.append("source schema_version must be >= 1")
    if manifest.get("publication_status") != "local-only":
        errors.append("source publication_status must be local-only")
    for flag in SAFETY_FLAGS:
        if manifest.get(flag) is not False:
            errors.append(f"source {flag} must be false")
    selected = manifest.get("selected_candidates")
    if not isinstance(selected, list):
        errors.append("source selected_candidates must be a list")
    elif not selected:
        errors.append("source selected_candidates must not be empty")
    else:
        for index, candidate in enumerate(selected):
            if not isinstance(candidate, dict):
                errors.append(f"selected candidate {index} must be an object")
                continue
            if candidate.get("review_status") != "static_review_candidate":
                errors.append(f"selected candidate {index} must be static_review_candidate")
            if candidate.get("compatibility_class") != "classic_pf_effect_candidate":
                errors.append(f"selected candidate {index} must be classic_pf_effect_candidate")
            if candidate.get("effect_main_export_present") is not True:
                errors.append(f"selected candidate {index} must export EffectMain")
            if candidate.get("pipl_signal_present") is not True:
                errors.append(f"selected candidate {index} must have PiPL signal")
            if int(candidate.get("aegp_marker_count") or 0) != 0:
                errors.append(f"selected candidate {index} must not include AEGP markers")
    safety_gate = manifest.get("safety_gate")
    if not isinstance(safety_gate, dict):
        errors.append("source safety_gate must be an object")
    else:
        blocked = safety_gate.get("blocked_actions", [])
        for action in ("load_aex_dll", "call_EffectMain", "render_with_aex", "route_through_ofx"):
            if action not in blocked:
                errors.append(f"source safety_gate must block {action}")
    return errors


def compact_design_candidate(candidate: dict[str, Any]) -> dict[str, Any]:
    return {
        "relative_path": candidate.get("relative_path"),
        "file_name": candidate.get("file_name"),
        "size_bytes": candidate.get("size_bytes"),
        "compatibility_class": candidate.get("compatibility_class"),
        "fixture_candidate_score": candidate.get("fixture_candidate_score"),
        "machine_label": candidate.get("machine_label"),
        "pipl_signal_present": candidate.get("pipl_signal_present"),
        "effect_main_export_present": candidate.get("effect_main_export_present"),
        "effect_main_marker_present": candidate.get("effect_main_marker_present"),
        "aegp_marker_count": candidate.get("aegp_marker_count"),
        "resource_types": candidate.get("resource_types", []),
        "import_dll_names": candidate.get("import_dll_names", []),
        "approval_state": "not_approved_for_load",
    }


def build_worker_protocol() -> dict[str, Any]:
    return {
        "transport": "stdio_or_named_pipe_json_lines",
        "message_rules": [
            "Controller owns all paths and sends only normalized create-new output paths.",
            "Worker must report safety_state before any fixture or image operation.",
            "Worker must fail closed on unknown messages.",
            "AEX-related executable actions stay unavailable until an explicit later gate opens.",
        ],
        "controller_to_worker_allowed_now": [
            {"type": "hello", "purpose": "version and safety-state handshake"},
            {"type": "inspect_environment", "purpose": "report process bitness and no-load capabilities"},
            {"type": "inspect_ppm", "purpose": "read dimensions for a generated P6 PPM fixture"},
            {"type": "transform_ppm_identity", "purpose": "copy pixel values into create-new output fixture"},
            {"type": "quit", "purpose": "stop the worker"},
        ],
        "worker_to_controller_allowed_now": [
            {"type": "hello_ack", "fields": ["schema_version", "safety_state"]},
            {"type": "environment_report", "fields": ["process_bitness", "native_load_enabled"]},
            {"type": "ppm_summary", "fields": ["width", "height", "bytes"]},
            {"type": "created_output", "fields": ["path", "operation"]},
            {"type": "error", "fields": ["code", "message"]},
        ],
        "messages_reserved_until_later_gate": [
            {"type": "load_aex", "requires": ["fixture approval", "sandbox review", "explicit user approval"]},
            {"type": "call_effect_main", "requires": ["load gate", "selector allowlist"]},
            {"type": "render_frame", "requires": ["render gate", "input/output validator"]},
            {"type": "ofx_describe", "requires": ["OFX facade gate"]},
        ],
    }


def build_worker_boundary() -> dict[str, Any]:
    return {
        "state": "design_only_no_native_load",
        "process_model": "out_of_process_worker_required_before_load",
        "host_process_rule": "controller must never load AEX binaries",
        "worker_process_rule": "worker may not load AEX binaries in this design packet",
        "path_policy": {
            "read_existing": [
                "approved static probe JSON",
                "approved fixture review manifest JSON",
                "generated PPM fixtures under target/ppm-fixtures",
            ],
            "create_new_only": [
                "worker reports under target/worker-design or a later worker-owned target root",
            ],
            "forbidden_without_approval": [
                "copying AEX files",
                "moving AEX files",
                "deleting AEX files",
                "writing beside source AEX files",
            ],
        },
        "runtime_limits_to_define_before_load": [
            "process timeout",
            "memory ceiling",
            "crash isolation",
            "stdout/stderr capture",
            "job-object or equivalent child cleanup",
            "input/output path allowlist",
        ],
    }


def build_gate_sequence() -> list[dict[str, Any]]:
    return [
        {
            "gate": "G0_static_probe",
            "status": "satisfied_by_source_chain",
            "evidence": "schema2 static probe report with no runtime flags",
        },
        {
            "gate": "G1_fixture_review_manifest",
            "status": "satisfied_by_source_manifest",
            "evidence": "fixture manifest with selected and hold candidates",
        },
        {
            "gate": "G2_worker_design_packet",
            "status": "this_artifact",
            "evidence": "design-only packet; no worker executable load path is enabled",
        },
        {
            "gate": "G3_manual_fixture_approval",
            "status": "not_satisfied",
            "required_before": ["copy_selected_aex_fixture", "load_aex_dll"],
        },
        {
            "gate": "G4_no_load_worker_selftest",
            "status": "not_satisfied",
            "required_before": ["load_aex_dll"],
        },
        {
            "gate": "G5_native_load_gate",
            "status": "closed",
            "required_before": ["call_EffectMain", "render_with_aex", "route_through_ofx"],
        },
        {
            "gate": "G6_render_validation_gate",
            "status": "closed",
            "required_before": ["render_with_aex", "ofx_facade_render"],
        },
    ]


def build_image_fixture_plan() -> dict[str, Any]:
    return {
        "current_tool": "tools/ppm_fixture_tool.py",
        "current_formats": ["P6 PPM"],
        "allowed_now": [
            "generate tiny PPM fixtures",
            "inspect PPM dimensions",
            "identity/invert transforms without AEX involvement",
        ],
        "future_worker_selftests": [
            "worker handshake returns native_load_enabled=false",
            "worker inspects generated PPM fixture dimensions",
            "worker performs identity transform into create-new output",
            "controller compares PPM headers and pixel byte length",
        ],
        "not_allowed_yet": [
            "passing pixels to EffectMain",
            "claiming AEX render equivalence",
            "routing image frames through OFX",
        ],
    }


def build_packet_payload(
    manifest: dict[str, Any],
    source_manifest_path: Path,
    *,
    candidate_limit: int = 3,
) -> dict[str, Any]:
    errors = validate_manifest(manifest)
    if errors:
        raise ValueError("; ".join(errors))
    selected = [
        compact_design_candidate(candidate)
        for candidate in manifest.get("selected_candidates", [])[: max(0, candidate_limit)]
    ]
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_worker_sandbox_design_packet",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "source_manifest": str(source_manifest_path),
        "source_manifest_schema_version": manifest.get("schema_version"),
        "source_report": manifest.get("source_report"),
        "source_summary": manifest.get("source_summary", {}),
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "design_state": "no_load_worker_boundary_only",
        "primary_review_candidate": selected[0] if selected else None,
        "candidate_design_inputs": selected,
        "worker_boundary": build_worker_boundary(),
        "ipc_protocol": build_worker_protocol(),
        "image_fixture_plan": build_image_fixture_plan(),
        "gate_sequence": build_gate_sequence(),
        "ofx_position": {
            "state": "deferred",
            "rule": "OFX facade work may translate host-facing concepts only after direct worker boundaries are reviewed.",
            "blocked_until": ["G3_manual_fixture_approval", "G4_no_load_worker_selftest", "G5_native_load_gate"],
        },
        "blocked_actions": BLOCKED_ACTIONS,
        "requires_explicit_user_approval_before": [
            "copying any selected AEX fixture",
            "creating a worker executable that loads AEX",
            "loading an AEX DLL",
            "calling EffectMain or dispatching PF selectors",
            "claiming render compatibility",
            "opening an OFX route",
        ],
        "next_implementation_slices": [
            "Add no-load worker harness that only handles hello/inspect_ppm/identity PPM messages.",
            "Add controller-side verifier for worker safety_state and create-new output paths.",
            "Add fixture approval manifest revision only after user approval of one candidate.",
        ],
        "notes": [
            "This packet is a design artifact derived from fixture manifest JSON only.",
            "No AEX file is opened, copied, hashed, loaded, or executed by this tool.",
            "The controller and worker concepts are specified for future implementation and are not enabled here.",
        ],
    }


def write_json_create_new(path: Path, payload: dict[str, Any]) -> Path:
    resolved = validate_output_path(path)
    with resolved.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    return resolved


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build no-load AEX worker/sandbox design packet")
    parser.add_argument("--manifest", required=True, help="Fixture review manifest under target/fixture-review")
    parser.add_argument("--out", required=True, help="Create-new design packet JSON under target/worker-design")
    parser.add_argument("--candidate-limit", type=int, default=3, help="Candidate design input limit")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    manifest, source_path = load_manifest(Path(args.manifest))
    packet = build_packet_payload(manifest, source_path, candidate_limit=args.candidate_limit)
    written = write_json_create_new(Path(args.out), packet)
    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
