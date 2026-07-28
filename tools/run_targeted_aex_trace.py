#!/usr/bin/env python3
"""Run a small, SHA-pinned AEX set through render-trace-png.

The input manifest is intentionally local: it names files to execute.  The
emitted report is portable and contains identities rather than those paths.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 1
MAX_CASES = 8
MAX_CAPTURE_BYTES = 8 * 1024 * 1024
MAX_ERROR_BYTES = 4096
CASE_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$")
SHA256 = re.compile(r"^[0-9a-fA-F]{64}$")
PIXEL_FORMATS = {"argb8", "argb16", "argb32f"}


class TraceRunnerError(RuntimeError):
    pass


def _object_without_duplicate_keys(
    pairs: list[tuple[str, Any]],
) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise TraceRunnerError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def strict_json_bytes(payload: bytes, label: str) -> dict[str, Any]:
    if len(payload) > MAX_CAPTURE_BYTES:
        raise TraceRunnerError(f"{label} exceeds {MAX_CAPTURE_BYTES} bytes")
    try:
        value = json.loads(
            payload.decode("utf-8"),
            object_pairs_hook=_object_without_duplicate_keys,
        )
    except (UnicodeError, json.JSONDecodeError) as error:
        raise TraceRunnerError(f"{label} is not strict UTF-8 JSON: {error}") from error
    if not isinstance(value, dict):
        raise TraceRunnerError(f"{label} JSON root must be an object")
    return value


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _require_file(value: Any, label: str) -> Path:
    if not isinstance(value, str) or not value:
        raise TraceRunnerError(f"{label} must be a nonempty path string")
    try:
        path = Path(value).expanduser().resolve(strict=True)
    except OSError as error:
        raise TraceRunnerError(f"{label} cannot be resolved") from error
    if not path.is_file():
        raise TraceRunnerError(f"{label} must be a file")
    return path


def _require_sha(value: Any, label: str) -> str:
    if not isinstance(value, str) or SHA256.fullmatch(value) is None:
        raise TraceRunnerError(f"{label} must be a SHA-256")
    return value.lower()


def _pin_file(value: Any, label: str) -> tuple[Path, str]:
    if not isinstance(value, dict) or set(value) != {"path", "sha256"}:
        raise TraceRunnerError(f"{label} must contain exactly path and sha256")
    path = _require_file(value["path"], f"{label}.path")
    expected = _require_sha(value["sha256"], f"{label}.sha256")
    actual = sha256_file(path)
    if actual != expected:
        raise TraceRunnerError(f"{label} SHA-256 mismatch")
    return path, actual


def load_manifest(path: Path) -> list[dict[str, Any]]:
    manifest = strict_json_bytes(path.read_bytes(), "manifest")
    if set(manifest) != {"schema_version", "cases"}:
        raise TraceRunnerError("manifest keys must be schema_version and cases")
    if manifest["schema_version"] != SCHEMA_VERSION:
        raise TraceRunnerError("manifest schema_version must be 1")
    cases = manifest["cases"]
    if not isinstance(cases, list) or not 1 <= len(cases) <= MAX_CASES:
        raise TraceRunnerError(f"manifest cases must contain 1..{MAX_CASES} entries")
    normalized = []
    seen_ids: set[str] = set()
    for index, case in enumerate(cases):
        label = f"cases[{index}]"
        if not isinstance(case, dict) or set(case) != {
            "id",
            "plugin",
            "input_png",
            "pixel_format",
            "parameters",
        }:
            raise TraceRunnerError(f"{label} has unexpected keys")
        case_id = case["id"]
        if not isinstance(case_id, str) or CASE_ID.fullmatch(case_id) is None:
            raise TraceRunnerError(f"{label}.id is invalid")
        if case_id in seen_ids:
            raise TraceRunnerError(f"duplicate case id: {case_id}")
        seen_ids.add(case_id)
        pixel_format = case["pixel_format"]
        if pixel_format not in PIXEL_FORMATS:
            raise TraceRunnerError(f"{label}.pixel_format is unsupported")
        parameters = case["parameters"]
        if (
            not isinstance(parameters, list)
            or len(parameters) > 64
            or any(
                not isinstance(value, str)
                or not value
                or len(value.encode("utf-8")) > 256
                or "=" not in value
                or value.startswith("-")
                or "/" in value
                or "\\" in value
                for value in parameters
            )
        ):
            raise TraceRunnerError(f"{label}.parameters is invalid")
        plugin, plugin_sha = _pin_file(case["plugin"], f"{label}.plugin")
        input_png, input_sha = _pin_file(case["input_png"], f"{label}.input_png")
        normalized.append(
            {
                "id": case_id,
                "plugin": plugin,
                "plugin_sha256": plugin_sha,
                "input_png": input_png,
                "input_png_sha256": input_sha,
                "pixel_format": pixel_format,
                "parameters": list(parameters),
            }
        )
    return normalized


def _redact_text(value: str, replacements: dict[str, str]) -> str:
    for source, token in sorted(
        replacements.items(), key=lambda item: len(item[0]), reverse=True
    ):
        if source:
            value = value.replace(source, token)
    # Do not let an unanticipated absolute POSIX path become public evidence.
    value = re.sub(r"(?<![\w.-])/(?:[^/\s:;,]+/)*[^/\s:;,]+", "<absolute-path>", value)
    encoded = value.encode("utf-8")
    if len(encoded) > MAX_ERROR_BYTES:
        value = encoded[:MAX_ERROR_BYTES].decode("utf-8", errors="ignore")
    return value


def _sanitize(value: Any, replacements: dict[str, str]) -> Any:
    if isinstance(value, str):
        return _redact_text(value, replacements)
    if isinstance(value, list):
        return [_sanitize(item, replacements) for item in value]
    if isinstance(value, dict):
        return {
            key: _sanitize(item, replacements)
            for key, item in value.items()
        }
    return value


def _parse_failure(
    completed: subprocess.CompletedProcess[bytes],
    replacements: dict[str, str],
) -> dict[str, Any]:
    stderr = completed.stderr[:MAX_CAPTURE_BYTES].decode("utf-8", errors="replace")
    marker = "crash_snapshot="
    snapshot = None
    message = stderr
    if marker in stderr:
        prefix, encoded = stderr.split(marker, 1)
        message = prefix.rstrip()
        try:
            snapshot, _ = json.JSONDecoder(
                object_pairs_hook=_object_without_duplicate_keys
            ).raw_decode(encoded.lstrip())
        except (json.JSONDecodeError, TraceRunnerError):
            snapshot = None
    result: dict[str, Any] = {
        "kind": "worker_error",
        "exit_code": completed.returncode,
        "message": _redact_text(message, replacements),
    }
    if completed.stdout:
        try:
            partial_report = strict_json_bytes(
                completed.stdout, "worker partial failure report"
            )
        except TraceRunnerError:
            partial_report = None
        if partial_report is not None:
            result["partial_report"] = _sanitize(partial_report, replacements)
    if isinstance(snapshot, dict):
        result["crash_snapshot"] = _sanitize(snapshot, replacements)
    return result


def run_case(
    worker: Path,
    case: dict[str, Any],
    run_root: Path,
    timeout_seconds: float,
) -> dict[str, Any]:
    output = run_root / f"{case['id']}.png"
    command = [
        os.fspath(worker),
        "render-trace-png",
        os.fspath(case["plugin"]),
        os.fspath(case["input_png"]),
        os.fspath(output),
        "--pixel-format",
        case["pixel_format"],
        *case["parameters"],
    ]
    replacements = {
        os.fspath(worker): "<worker>",
        os.fspath(case["plugin"]): "<plugin>",
        os.fspath(case["input_png"]): "<input-png>",
        os.fspath(output): "<output-png>",
        os.fspath(run_root): "<run-root>",
        os.fspath(Path.home()): "<home>",
    }
    try:
        completed = subprocess.run(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return {"kind": "timeout"}
    if completed.returncode != 0:
        return _parse_failure(completed, replacements)
    report = strict_json_bytes(completed.stdout, f"worker output for {case['id']}")
    traces = report.get("execution_traces")
    if not isinstance(traces, list) or not traces:
        raise TraceRunnerError(
            f"worker output for {case['id']} has no execution_traces"
        )
    return {
        "kind": "trace",
        "report": _sanitize(report, replacements),
        "output_png_sha256": sha256_file(output) if output.is_file() else None,
    }


def run(args: argparse.Namespace) -> dict[str, Any]:
    manifest_path = args.manifest.resolve(strict=True)
    worker, worker_sha = _pin_file(
        {"path": os.fspath(args.worker), "sha256": args.expected_worker_sha256},
        "worker",
    )
    cases = load_manifest(manifest_path)
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix="aex-targeted-trace-", dir=args.run_parent
    ) as temporary:
        run_root = Path(temporary)
        results = [
            {
                "id": case["id"],
                "plugin_sha256": case["plugin_sha256"],
                "input_png_sha256": case["input_png_sha256"],
                "pixel_format": case["pixel_format"],
                "parameters": case["parameters"],
                "result": run_case(worker, case, run_root, args.timeout),
            }
            for case in cases
        ]
    report = {
        "schema_version": SCHEMA_VERSION,
        "mode": "targeted_macos_x64_runtime_provenance",
        "worker_sha256": worker_sha,
        "case_count": len(results),
        "cases": results,
    }
    output.write_text(
        json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return report


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--worker", type=Path, required=True)
    parser.add_argument("--expected-worker-sha256", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--run-parent", type=Path)
    args = parser.parse_args(argv)
    if not 0 < args.timeout <= 300:
        parser.error("--timeout must be in (0, 300]")
    return args


def main() -> int:
    try:
        report = run(parse_args())
    except (OSError, TraceRunnerError) as error:
        print(f"targeted_aex_trace_error: {error}", file=sys.stderr)
        return 1
    print(
        json.dumps(
            {
                "case_count": report["case_count"],
                "result_kinds": [
                    case["result"]["kind"] for case in report["cases"]
                ],
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
