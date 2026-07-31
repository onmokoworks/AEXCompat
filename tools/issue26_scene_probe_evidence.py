#!/usr/bin/env python3
"""Create and validate strict Issue #26 public-AEGP evidence records."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator

import issue26_sdk_provenance as sdk_provenance


ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "schemas" / "issue26-scene-probe-evidence.schema.json"
EMPTY_SHA256 = hashlib.sha256(b"").hexdigest()


def reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(
        path.read_text(encoding="utf-8-sig"),
        object_pairs_hook=reject_duplicate_keys,
    )
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain one JSON object")
    return value


def digest_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def artifact(path: Path) -> dict[str, Any]:
    resolved = path.resolve(strict=True)
    if not resolved.is_file():
        raise ValueError(f"artifact is not a file: {resolved}")
    return {
        "path": str(resolved),
        "sha256": digest_bytes(resolved.read_bytes()),
        "size_bytes": resolved.stat().st_size,
    }


def environment(args: argparse.Namespace) -> dict[str, Any]:
    return {
        "after_effects": {
            "version": args.ae_version,
            "executable": artifact(Path(args.after_effects)),
        },
        "sdk": {
            "root": str(Path(args.sdk_root).resolve(strict=True)),
            "aefx_api_version": args.sdk_api_version,
            "guide": artifact(Path(args.sdk_guide)),
        },
    }


def artifacts(
    args: argparse.Namespace, sample: Path | None = None
) -> dict[str, Any]:
    return {
        "probe": artifact(Path(args.probe)),
        "fixture": artifact(Path(args.fixture)),
        "worker": artifact(Path(args.worker)),
        "sample": artifact(sample) if sample else None,
    }


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def output_hash(value: str) -> str:
    return digest_bytes(value.encode("utf-8"))


def host_summary(stdout: str) -> dict[str, Any] | None:
    for line in reversed(stdout.splitlines()):
        try:
            value = json.loads(line, object_pairs_hook=reject_duplicate_keys)
        except (json.JSONDecodeError, ValueError):
            continue
        if isinstance(value, dict) and value.get("stage") == "aegp_init":
            keys = (
                "schema_version",
                "stage",
                "status",
                "driver_major_version",
                "driver_minor_version",
                "event_requested",
                "init_error",
                "event_error",
                "entry_invoked",
                "entry_fault",
                "entry_exception_code",
                "forced_suite_releases",
                "boundary_regression_mode",
                "boundary_regression_passed",
                "effect_lifetimes_balanced",
                "stream_lifetimes_balanced",
                "collection_lifetimes_balanced",
                "aegp_memory_lifetimes_balanced",
                "suite_leases_balanced",
                "suite_acquires",
                "suite_releases",
                "live_suite_reference_count",
                "receipts_created",
                "receipts_checked_in",
                "live_receipts",
            )
            return {key: value[key] for key in keys}
    return None


def write_record(record: dict[str, Any], output: Path) -> None:
    schema = load_json(SCHEMA_PATH)
    Draft202012Validator.check_schema(schema)
    Draft202012Validator(schema).validate(record)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(record, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


def common_record(
    args: argparse.Namespace,
    *,
    target: str,
    status: str,
    command: list[str],
    exit_code: int | None,
    stdout: str,
    stderr: str,
    blocker: dict[str, Any] | None,
    sample: Path | None = None,
) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "evidence_kind": "issue26-public-aegp-scene",
        "target": target,
        "recorded_utc": utc_now(),
        "status": status,
        "execution": {
            "command": command,
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
            "stdout_sha256": output_hash(stdout),
            "stderr_sha256": output_hash(stderr),
            "blocker": blocker,
        },
        "environment": environment(args),
        "artifacts": {
            **artifacts(args, sample),
            "build_receipt": None,
            "fixture_metadata": None,
            "raw_probe_report": None,
        },
        "probe_report": None,
        "host_report": None,
        "fixture_report": None,
        "sample_report": None,
        "unsupported_slots": [],
        "cleanup": None,
    }


def coverage_complete(report: dict[str, Any]) -> bool:
    coverage = report["coverage"]
    return all(
        (
            coverage["identity_enumeration"],
            coverage["effect_order"]["observed"],
            coverage["effect_order"]["total"],
            coverage["stream_metadata"],
            coverage["parent_camera_zoom"]["parent"],
            coverage["parent_camera_zoom"]["camera"],
            coverage["parent_camera_zoom"]["zoom"],
            coverage["keyframes"]["interpolation"],
            coverage["keyframes"]["ease"],
            coverage["keyframes"]["spatial_tangents"],
            coverage["transaction"]["cancel_observed"],
            coverage["transaction"]["cancel_unchanged"],
            coverage["transaction"]["commit_observed"],
            coverage["transaction"]["commit_incremented"],
            coverage["transaction"]["cleanup_observed"],
            coverage["transaction"]["cleanup_restored"],
            coverage["generation"]["stale_owner_rejected"],
        )
    )


def probe_readiness_errors(
    report: dict[str, Any], target: str
) -> list[str]:
    errors: list[str] = []
    identities = report["identities"]
    streams = report["streams"]
    coverage = report["coverage"]
    cleanup = report["cleanup"]

    identity_tokens = [value["token"] for value in identities]
    if len(identity_tokens) != len(set(identity_tokens)):
        errors.append("duplicate identity token")
    identity_keys = [
        (
            value["kind"],
            value["stable_id"],
            value["owner_id"],
            value["ordinal"],
        )
        for value in identities
    ]
    if len(identity_keys) != len(set(identity_keys)):
        errors.append("duplicate identity position")
    stable_ids = [value["stable_id"] for value in identities]
    if target == "aexcompat" and len(stable_ids) != len(set(stable_ids)):
        errors.append("duplicate stable identity")
    by_kind: dict[str, list[dict[str, Any]]] = {}
    for value in identities:
        by_kind.setdefault(value["kind"], []).append(value)
    minimums = (
        {
            "project": 2,
            "folder": 3,
            "footage": 1,
            "comp": 3,
            "layer": 3,
            "effect": 1,
        }
        if target == "aexcompat"
        else {
            "project": 1,
            "folder": 1,
            "footage": 1,
            "comp": 1,
            "layer": 1,
            "effect": 1,
        }
    )
    identity_complete = all(
        len(by_kind.get(kind, [])) >= minimum
        for kind, minimum in minimums.items()
    )
    if not identity_complete:
        errors.append("identity family coverage incomplete")
    project_ids = {
        value["stable_id"] for value in by_kind.get("project", [])
    }
    comp_ids = {value["stable_id"] for value in by_kind.get("comp", [])}
    layer_ids = {value["stable_id"] for value in by_kind.get("layer", [])}
    if any(
        value["owner_id"] != -1
        for value in by_kind.get("project", [])
    ):
        errors.append("project identity has owner")
    for kind in ("folder", "footage", "comp"):
        if any(
            value["owner_id"] not in project_ids
            for value in by_kind.get(kind, [])
        ):
            errors.append(f"{kind} identity has foreign project owner")
    if any(
        value["owner_id"] not in comp_ids
        for value in by_kind.get("layer", [])
    ):
        errors.append("layer identity has foreign composition owner")
    if any(
        value["owner_id"] not in layer_ids
        for value in by_kind.get("effect", [])
    ):
        errors.append("effect identity has foreign layer owner")

    def require_total_ordinals(
        values: list[dict[str, Any]], owner_key: str
    ) -> bool:
        groups: dict[int, list[int]] = {}
        for value in values:
            groups.setdefault(value[owner_key], []).append(value["ordinal"])
        return all(
            sorted(ordinals) == list(range(len(ordinals)))
            for ordinals in groups.values()
        )

    item_values = [
        value
        for kind in ("folder", "footage", "comp")
        for value in by_kind.get(kind, [])
    ]
    if sorted(
        value["ordinal"] for value in by_kind.get("project", [])
    ) != list(range(len(by_kind.get("project", [])))):
        errors.append("project ordinals are not total")
    if not require_total_ordinals(item_values, "owner_id"):
        errors.append("project item ordinals are not total")
    if not require_total_ordinals(by_kind.get("layer", []), "owner_id"):
        errors.append("composition layer ordinals are not total")
    if not require_total_ordinals(by_kind.get("effect", []), "owner_id"):
        errors.append("layer effect ordinals are not total")
    if coverage["identity_enumeration"] != identity_complete:
        errors.append("identity coverage flag contradicts identities")

    stream_tokens = [value["token"] for value in streams]
    if not streams or len(stream_tokens) != len(set(stream_tokens)):
        errors.append("stream identities are empty or duplicated")
    effect_tokens = {
        value["token"] for value in by_kind.get("effect", [])
    }
    stream_groups: dict[str, list[dict[str, Any]]] = {}
    for value in streams:
        if value["owner"] not in effect_tokens:
            errors.append("stream has foreign effect owner")
        stream_groups.setdefault(value["owner"], []).append(value)
    streams_complete = bool(stream_groups)
    for group in stream_groups.values():
        ordinals = sorted(value["ordinal"] for value in group)
        if ordinals != list(range(len(group))):
            errors.append("effect stream ordinals are not total")
            streams_complete = False
        input_streams = [
            value
            for value in group
            if value["ordinal"] == 0 and value["type"] == 9
        ]
        if len(input_streams) != 1:
            errors.append("effect input-layer stream is missing")
            streams_complete = False
    if coverage["stream_metadata"] != streams_complete:
        errors.append("stream metadata flag contradicts streams")

    transaction = coverage["transaction"]
    before = transaction["before"]
    derived_transaction = {
        "cancel_observed": transaction["after_cancel"] >= 0,
        "cancel_unchanged": transaction["after_cancel"] == before,
        "commit_observed": transaction["after_commit"] >= 0,
        "commit_incremented": transaction["after_commit"] == before + 1,
        "cleanup_observed": transaction["after_cleanup"] >= 0,
        "cleanup_restored": transaction["after_cleanup"] == before,
    }
    if before < 0:
        errors.append("transaction baseline is absent")
    for key, derived in derived_transaction.items():
        if transaction[key] != derived:
            errors.append(f"transaction {key} contradicts counters")

    cleanup_balanced = all(
        (
            cleanup["suite_acquires"] == cleanup["suite_releases"],
            cleanup["stream_acquires"]
            == cleanup["stream_releases"]
            + cleanup["invalidated_children"],
            cleanup["effect_acquires"] == cleanup["effect_releases"],
            cleanup["applied_effects"]
            == cleanup["removed_applied_effects"],
            cleanup["keyframe_mutations_committed"]
            == cleanup["keyframe_mutations_reverted"],
            cleanup["keyframe_mutations_committed"] == 1,
            cleanup["applied_effects"] == 1,
            cleanup["invalidated_children"] >= 1,
            cleanup["residual_scene_mutations"] == 0,
            transaction["after_cleanup"] == before,
        )
    )
    if cleanup["balanced"] != cleanup_balanced:
        errors.append("cleanup balanced flag contradicts counters")
    if not cleanup_balanced:
        errors.append("cleanup counters are not balanced")

    internally_ready = (
        identity_complete
        and streams_complete
        and coverage_complete(report)
        and cleanup_balanced
        and not report["unsupported_slots"]
        and not errors
    )
    if (report["status"] == "passed") != internally_ready:
        errors.append("probe status contradicts derived readiness")
    return errors


def host_readiness_errors(
    host: dict[str, Any], *, boundary: bool
) -> list[str]:
    errors: list[str] = []
    lifetime_balanced = all(
        (
            host["entry_invoked"],
            host["driver_major_version"] > 0,
            host["effect_lifetimes_balanced"],
            host["stream_lifetimes_balanced"],
            host["collection_lifetimes_balanced"],
            host["aegp_memory_lifetimes_balanced"],
            host["suite_leases_balanced"],
            host["suite_acquires"] > 0,
            host["suite_acquires"] == host["suite_releases"],
            host["live_suite_reference_count"] == 0,
            host["receipts_created"] == host["receipts_checked_in"],
            host["live_receipts"] == 0,
        )
    )
    if not lifetime_balanced:
        errors.append("host lifetime counters are not balanced")
    if boundary:
        if (
            host["status"] != "initialization_failed"
            or host["init_error"] == 0
            or host["entry_fault"] != "cpp_exception"
            or host["entry_exception_code"] != 0
            or host["forced_suite_releases"] != 0
            or not host["boundary_regression_mode"]
            or not host["boundary_regression_passed"]
            or host["event_requested"] != "none"
            or host["event_error"] != 0
        ):
            errors.append("guarded C++ boundary fields are contradictory")
    elif (
        host["status"] not in {"event_completed", "initialized"}
        or host["init_error"] != 0
        or host["event_error"] != 0
        or host["entry_fault"] != "none"
        or host["entry_exception_code"] != 0
        or host["forced_suite_releases"] != 0
        or host["boundary_regression_mode"]
        or host["boundary_regression_passed"]
        or not host["event_requested"]
    ):
        errors.append("successful host fields are contradictory")
    return errors


def readiness_errors(record: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    execution = record["execution"]
    if output_hash(execution["stdout"]) != execution["stdout_sha256"]:
        errors.append("stdout hash mismatch")
    if output_hash(execution["stderr"]) != execution["stderr_sha256"]:
        errors.append("stderr hash mismatch")
    if record["status"] != "passed":
        errors.append(f"status={record['status']}")
        return errors
    if not execution["stdout"]:
        errors.append("passed record has empty stdout")
    if execution["exit_code"] != 0:
        errors.append(f"passed record exit_code={execution['exit_code']}")
    if execution["blocker"] is not None:
        errors.append("passed record has blocker")
    target = record["target"]
    if target in {"aexcompat", "after_effects"}:
        report = record["probe_report"]
        cleanup = record["cleanup"]
        if report is None or report["status"] != "passed":
            errors.append("passed probe record lacks passed probe_report")
        else:
            errors.extend(probe_readiness_errors(report, target))
        if target == "aexcompat":
            host = record["host_report"]
            if (
                host is None
                or host["status"] not in {"event_completed", "initialized"}
            ):
                errors.append(
                    "passed AEXCompat record lacks successful host_report"
                )
            else:
                derived_host = host_summary(execution["stdout"])
                if derived_host != host:
                    errors.append("host_report is not derived from stdout")
                errors.extend(
                    host_readiness_errors(host, boundary=False)
                )
        elif record["fixture_report"] is None:
            errors.append("passed real-AE record lacks fixture_report")
        if cleanup is None or not cleanup["balanced"]:
            errors.append("cleanup is not balanced")
        if record["unsupported_slots"]:
            errors.append("required public operation is unsupported")
    else:
        sample = record["sample_report"]
        if sample is None or sample["status"] != "passed":
            errors.append("passed sample record lacks passed sample_report")
        elif sample["classification"] != classify_sample(
            sample["sample"],
            execution["exit_code"],
            execution["stdout"],
            execution["stderr"],
            record["host_report"],
        ):
            errors.append("sample classification is not derived from output")
        if target == "sdk_projector_aexcompat":
            host = record["host_report"]
            if (
                host is None
                or host["status"] != "initialization_failed"
                or not host["boundary_regression_passed"]
                or host["entry_fault"] != "cpp_exception"
            ):
                errors.append("Projector guarded-boundary proof is incomplete")
            else:
                derived_host = host_summary(execution["stdout"])
                if derived_host != host:
                    errors.append("Projector host_report is not derived")
                errors.extend(host_readiness_errors(host, boundary=True))
        if not sample or not sample["sdk_source_unchanged"]:
            errors.append("SDK source unchanged proof is absent")
    return errors


def verify_record_environment_and_artifacts(
    path: Path, record: dict[str, Any]
) -> None:
    sdk_root = Path(record["environment"]["sdk"]["root"])
    if not sdk_root.is_dir():
        raise ValueError(f"{path}: SDK root missing: {sdk_root}")
    for group in (
        record["environment"]["after_effects"],
        record["environment"]["sdk"],
    ):
        for value in group.values():
            if not isinstance(value, dict) or "path" not in value:
                continue
            candidate = Path(value["path"])
            if not candidate.is_file() or artifact(candidate) != value:
                raise ValueError(
                    f"{path}: environment artifact mismatch: {candidate}"
                )
    for identity in record["artifacts"].values():
        if identity is None:
            continue
        candidate = Path(identity["path"])
        if not candidate.is_file() or artifact(candidate) != identity:
            raise ValueError(f"{path}: artifact mismatch: {candidate}")
    sample_report = record["sample_report"]
    if sample_report is not None:
        receipt_identity = record["artifacts"]["build_receipt"]
        sample_identity = record["artifacts"]["sample"]
        if receipt_identity is None or sample_identity is None:
            raise ValueError(f"{path}: sample provenance artifacts are absent")
        receipt = load_json(Path(receipt_identity["path"]))
        matches = [
            value
            for value in receipt.get("samples", [])
            if value.get("sample") == sample_report["sample"]
        ]
        if len(matches) != 1:
            raise ValueError(
                f"{path}: build receipt sample provenance is ambiguous"
            )
        receipt_sample = matches[0]
        provenance_errors = sdk_provenance.validate_sample_receipt(
            receipt_sample
        )
        if provenance_errors:
            raise ValueError(
                f"{path}: SDK provenance validation failed: "
                + "; ".join(provenance_errors)
            )
        if (
            sample_report["build_receipt_sha256"]
            != receipt_identity["sha256"]
            or receipt_sample["artifact_sha256"] != sample_identity["sha256"]
            or receipt_sample["artifact_size"] != sample_identity["size_bytes"]
            or Path(receipt_sample["artifact"]).resolve(strict=True)
            != Path(sample_identity["path"]).resolve(strict=True)
            or not receipt_sample["sdk_source_unchanged"]
            or receipt_sample["source_inputs_sha256_before"]
            != receipt_sample["source_inputs_sha256_after"]
            or sample_report["source_inputs_sha256"]
            != receipt_sample["source_inputs_sha256_after"]
            or sample_report["sdk_source_unchanged"]
            != receipt_sample["sdk_source_unchanged"]
        ):
            raise ValueError(
                f"{path}: sample provenance does not match build receipt"
            )
    fixture_report = record["fixture_report"]
    fixture_identity = record["artifacts"]["fixture_metadata"]
    if fixture_report is None:
        if fixture_identity is not None:
            raise ValueError(f"{path}: fixture metadata is not derived")
    else:
        if fixture_identity is None:
            raise ValueError(f"{path}: fixture metadata artifact is absent")
        derived_fixture = load_json(Path(fixture_identity["path"]))
        if fixture_report != derived_fixture:
            raise ValueError(f"{path}: fixture_report is not derived")
    raw_probe_identity = record["artifacts"]["raw_probe_report"]
    if record["probe_report"] is None:
        if raw_probe_identity is not None:
            raise ValueError(f"{path}: unexpected raw probe report artifact")
    else:
        if raw_probe_identity is None:
            raise ValueError(f"{path}: raw probe report artifact is absent")
        if load_json(Path(raw_probe_identity["path"])) != record["probe_report"]:
            raise ValueError(f"{path}: probe_report is not derived")


def command_probe(args: argparse.Namespace) -> int:
    probe = Path(args.probe).resolve(strict=True)
    worker = Path(args.worker).resolve(strict=True)
    raw_output = Path(args.raw_report).resolve()
    raw_output.parent.mkdir(parents=True, exist_ok=True)
    if raw_output.exists():
        raw_output.unlink()
    probe_sha = artifact(probe)["sha256"]
    command = [
        str(worker),
        "--aegp-comp-idle-roundtrip",
        str(probe),
        probe_sha,
    ]
    child_env = os.environ.copy()
    child_env["ISSUE26_SCENE_PROBE_EVIDENCE"] = str(raw_output)
    completed = subprocess.run(
        command,
        cwd=ROOT,
        env=child_env,
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
        timeout=args.timeout,
        check=False,
    )
    report = load_json(raw_output) if raw_output.is_file() else None
    summary = host_summary(completed.stdout)
    status = (
        report["status"]
        if completed.returncode == 0 and report is not None and summary is not None
        else "failed"
    )
    blocker = None
    if status == "failed":
        blocker = {
            "code": "aexcompat_probe_execution_failed",
            "message": "The public AEGP probe did not complete under AEXCompat.",
            "details": [
                f"exit_code={completed.returncode}",
                f"raw_report_present={raw_output.is_file()}",
                f"host_report_present={summary is not None}",
            ],
        }
    record = common_record(
        args,
        target="aexcompat",
        status=status,
        command=command,
        exit_code=completed.returncode,
        stdout=completed.stdout,
        stderr=completed.stderr,
        blocker=blocker,
    )
    record["probe_report"] = report
    record["host_report"] = summary
    if report is not None:
        record["artifacts"]["raw_probe_report"] = artifact(raw_output)
        record["unsupported_slots"] = report["unsupported_slots"]
        record["cleanup"] = report["cleanup"]
    write_record(record, Path(args.output))
    print(completed.stdout, end="")
    if completed.stderr:
        print(completed.stderr, end="", file=os.sys.stderr)
    return 0 if not readiness_errors(record) else 1


def command_wrap_real(args: argparse.Namespace) -> int:
    command = args.command_part
    report = load_json(Path(args.raw_report))
    fixture_path = Path(args.fixture_metadata).resolve(strict=True)
    fixture_report = load_json(fixture_path)
    stdout = Path(args.stdout_file).read_text(
        encoding="utf-8", errors="replace"
    )
    stderr = Path(args.stderr_file).read_text(
        encoding="utf-8", errors="replace"
    )
    status = report["status"] if args.exit_code == 0 else "failed"
    blocker = None
    if args.exit_code != 0:
        blocker = {
            "code": "real_ae_execution_failed",
            "message": "After Effects exited nonzero during the public probe.",
            "details": [f"exit_code={args.exit_code}"],
        }
    record = common_record(
        args,
        target="after_effects",
        status=status,
        command=command,
        exit_code=args.exit_code,
        stdout=stdout,
        stderr=stderr,
        blocker=blocker,
    )
    record["artifacts"]["fixture_metadata"] = artifact(fixture_path)
    record["artifacts"]["raw_probe_report"] = artifact(
        Path(args.raw_report)
    )
    record["probe_report"] = report
    record["fixture_report"] = fixture_report
    record["unsupported_slots"] = report["unsupported_slots"]
    record["cleanup"] = report["cleanup"]
    write_record(record, Path(args.output))
    return 0 if not readiness_errors(record) else 1


def command_blocker(args: argparse.Namespace) -> int:
    command = args.command_part
    details = args.detail
    record = common_record(
        args,
        target="after_effects",
        status="blocked",
        command=command,
        exit_code=None,
        stdout="",
        stderr="",
        blocker={
            "code": args.blocker_code,
            "message": args.blocker_message,
            "details": details,
        },
    )
    write_record(record, Path(args.output))
    return 1


def classify_sample(
    sample: str,
    returncode: int,
    stdout: str,
    stderr: str,
    host: dict[str, Any] | None,
) -> str:
    combined = (stdout + "\n" + stderr).lower()
    if (
        sample == "Projector"
        and returncode == 0
        and host is not None
        and host["status"] == "initialization_failed"
        and host["boundary_regression_passed"]
        and host["entry_fault"] == "cpp_exception"
        and host["entry_exception_code"] == 0
        and host["forced_suite_releases"] == 0
        and "stage:suite_acquire_failed" in combined
        and "name=aegp file import manager suite" in combined
        and "version=3" in combined
    ):
        return "guarded_initialization_failure"
    if returncode == 0:
        return "completed"
    if (
        "suite_acquire_failed" in combined
        or ("suite" in combined and ("missing" in combined or "unsupported" in combined))
    ):
        return "unsupported_suite"
    if "load_failure" in combined:
        return "load_failure"
    return "nonzero_exit"


def command_sample(args: argparse.Namespace) -> int:
    sample = Path(args.sample_artifact).resolve(strict=True)
    worker = Path(args.worker).resolve(strict=True)
    receipt_path = Path(args.build_receipt).resolve(strict=True)
    receipt = load_json(receipt_path)
    matches = [
        value
        for value in receipt.get("samples", [])
        if value.get("sample") == args.sample
    ]
    if len(matches) != 1:
        raise ValueError(
            f"build receipt must contain exactly one {args.sample} entry"
        )
    receipt_sample = matches[0]
    provenance_errors = sdk_provenance.validate_sample_receipt(receipt_sample)
    if provenance_errors:
        raise ValueError(
            "SDK build receipt failed independent validation: "
            + "; ".join(provenance_errors)
        )
    sample_identity = artifact(sample)
    if (
        Path(receipt_sample["artifact"]).resolve(strict=True) != sample
        or receipt_sample["artifact_sha256"] != sample_identity["sha256"]
        or receipt_sample["artifact_size"] != sample_identity["size_bytes"]
        or not receipt_sample["sdk_source_unchanged"]
        or receipt_sample["source_inputs_sha256_before"]
        != receipt_sample["source_inputs_sha256_after"]
    ):
        raise ValueError(
            "sample artifact/source provenance does not match build receipt"
        )
    route = (
        "--aegp-init-boundary-test"
        if args.sample == "Projector"
        else "--l2-params-only"
    )
    command = [str(worker), route, str(sample), sample_identity["sha256"]]
    completed = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
        timeout=args.timeout,
        check=False,
    )
    summary = host_summary(completed.stdout)
    classification = classify_sample(
        args.sample,
        completed.returncode,
        completed.stdout,
        completed.stderr,
        summary,
    )
    expected = (
        classification == "guarded_initialization_failure"
        if args.sample == "Projector"
        else classification == "completed"
    )
    record = common_record(
        args,
        target=(
            "sdk_projector_aexcompat"
            if args.sample == "Projector"
            else "sdk_resizer_aexcompat"
        ),
        status="passed" if expected else "failed",
        command=command,
        exit_code=completed.returncode,
        stdout=completed.stdout,
        stderr=completed.stderr,
        blocker=None,
        sample=sample,
    )
    record["artifacts"]["build_receipt"] = artifact(receipt_path)
    record["host_report"] = summary
    record["sample_report"] = {
        "sample": args.sample,
        "status": "passed" if expected else "failed",
        "route": (
            "aegp_init_boundary_test"
            if args.sample == "Projector"
            else "l2_params_only"
        ),
        "classification": classification,
        "sdk_source_unchanged": receipt_sample["sdk_source_unchanged"],
        "build_receipt_sha256": record["artifacts"]["build_receipt"]["sha256"],
        "source_inputs_sha256": receipt_sample[
            "source_inputs_sha256_after"
        ],
    }
    write_record(record, Path(args.output))
    print(completed.stdout, end="")
    if completed.stderr:
        print(completed.stderr, end="", file=os.sys.stderr)
    return 0 if not readiness_errors(record) else 1


def command_validate(args: argparse.Namespace) -> int:
    schema = load_json(SCHEMA_PATH)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema)
    incomplete: list[str] = []
    for value in args.records:
        path = Path(value)
        record = load_json(path)
        validator.validate(record)
        report = record["probe_report"]
        if report is not None:
            if record["unsupported_slots"] != report["unsupported_slots"]:
                raise ValueError(f"{path}: unsupported_slots is not derived")
            if record["cleanup"] != report["cleanup"]:
                raise ValueError(f"{path}: cleanup is not derived")
        verify_record_environment_and_artifacts(path, record)
        errors = readiness_errors(record)
        if errors:
            incomplete.append(f"{path}: {', '.join(errors)}")
    print(f"validated {len(args.records)} Issue #26 evidence record(s)")
    if incomplete:
        for value in incomplete:
            print(value, file=os.sys.stderr)
        return 1
    return 0


def add_environment_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--probe", required=True)
    parser.add_argument("--fixture", required=True)
    parser.add_argument("--worker", required=True)
    parser.add_argument("--after-effects", required=True)
    parser.add_argument("--ae-version", required=True)
    parser.add_argument("--sdk-root", required=True)
    parser.add_argument("--sdk-api-version", required=True, type=int)
    parser.add_argument("--sdk-guide", required=True)
    parser.add_argument("--output", required=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    probe = subparsers.add_parser("probe-aexcompat")
    add_environment_arguments(probe)
    probe.add_argument("--raw-report", required=True)
    probe.add_argument("--timeout", type=int, default=60)
    probe.set_defaults(handler=command_probe)

    real = subparsers.add_parser("wrap-real")
    add_environment_arguments(real)
    real.add_argument("--raw-report", required=True)
    real.add_argument("--fixture-metadata", required=True)
    real.add_argument("--stdout-file", required=True)
    real.add_argument("--stderr-file", required=True)
    real.add_argument("--command-part", action="append", required=True)
    real.add_argument("--exit-code", required=True, type=int)
    real.set_defaults(handler=command_wrap_real)

    blocker = subparsers.add_parser("record-real-blocker")
    add_environment_arguments(blocker)
    blocker.add_argument("--command-part", action="append", required=True)
    blocker.add_argument("--blocker-code", required=True)
    blocker.add_argument("--blocker-message", required=True)
    blocker.add_argument("--detail", action="append", default=[])
    blocker.set_defaults(handler=command_blocker)

    sample = subparsers.add_parser("sample-aexcompat")
    add_environment_arguments(sample)
    sample.add_argument("--sample", choices=("Projector", "Resizer"), required=True)
    sample.add_argument("--sample-artifact", required=True)
    sample.add_argument("--build-receipt", required=True)
    sample.add_argument("--timeout", type=int, default=60)
    sample.set_defaults(handler=command_sample)

    validate = subparsers.add_parser("validate")
    validate.add_argument("records", nargs="+")
    validate.add_argument("--verify-existing-artifacts", action="store_true")
    validate.set_defaults(handler=command_validate)

    args = parser.parse_args()
    return args.handler(args)


if __name__ == "__main__":
    raise SystemExit(main())
