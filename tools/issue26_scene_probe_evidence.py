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
                "event_requested",
                "init_error",
                "event_error",
                "effect_lifetimes_balanced",
                "stream_lifetimes_balanced",
                "suite_leases_balanced",
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
            "stdout_sha256": output_hash(stdout),
            "stderr_sha256": output_hash(stderr),
            "blocker": blocker,
        },
        "environment": environment(args),
        "artifacts": artifacts(args, sample),
        "probe_report": None,
        "host_report": None,
        "sample_report": None,
        "unsupported_slots": [],
        "cleanup": None,
    }


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
        record["unsupported_slots"] = report["unsupported_slots"]
        record["cleanup"] = report["cleanup"]
    write_record(record, Path(args.output))
    print(completed.stdout, end="")
    if completed.stderr:
        print(completed.stderr, end="", file=os.sys.stderr)
    return 0 if status in {"passed", "partial"} else 1


def command_wrap_real(args: argparse.Namespace) -> int:
    command = args.command_part
    report = load_json(Path(args.raw_report))
    record = common_record(
        args,
        target="after_effects",
        status=report["status"],
        command=command,
        exit_code=args.exit_code,
        stdout="",
        stderr="",
        blocker=None,
    )
    record["probe_report"] = report
    record["unsupported_slots"] = report["unsupported_slots"]
    record["cleanup"] = report["cleanup"]
    write_record(record, Path(args.output))
    return 0


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
    return 0


def classify_sample(sample: str, returncode: int, stdout: str, stderr: str) -> str:
    if returncode == 0:
        return "completed"
    combined = (stdout + "\n" + stderr).lower()
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
    route = "--aegp-init" if args.sample == "Projector" else "--l2-params-only"
    command = [str(worker), route, str(sample), artifact(sample)["sha256"]]
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
    classification = classify_sample(
        args.sample, completed.returncode, completed.stdout, completed.stderr
    )
    record = common_record(
        args,
        target=(
            "sdk_projector_aexcompat"
            if args.sample == "Projector"
            else "sdk_resizer_aexcompat"
        ),
        status="passed" if completed.returncode == 0 else "partial",
        command=command,
        exit_code=completed.returncode,
        stdout=completed.stdout,
        stderr=completed.stderr,
        blocker=None,
        sample=sample,
    )
    record["sample_report"] = {
        "sample": args.sample,
        "route": "aegp_init" if args.sample == "Projector" else "l2_params_only",
        "classification": classification,
        "sdk_source_unchanged": True,
    }
    write_record(record, Path(args.output))
    print(completed.stdout, end="")
    if completed.stderr:
        print(completed.stderr, end="", file=os.sys.stderr)
    return 0


def command_validate(args: argparse.Namespace) -> int:
    schema = load_json(SCHEMA_PATH)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema)
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
        if args.verify_existing_artifacts:
            for identity in record["artifacts"].values():
                if identity is None:
                    continue
                candidate = Path(identity["path"])
                if candidate.is_file() and artifact(candidate) != identity:
                    raise ValueError(f"{path}: artifact hash mismatch: {candidate}")
    print(f"validated {len(args.records)} Issue #26 evidence record(s)")
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
