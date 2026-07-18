#!/usr/bin/env python3
"""Create a self-contained AEX conformance evidence bundle."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
import threading
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator
from PIL import Image
from referencing import Registry, Resource

from conformance_bundle_validator import (
    _is_reparse,
    _open_artifact_beneath,
    validate_bundle,
)


ROOT = Path(__file__).resolve().parents[1]
SCHEMAS = ROOT / "schemas"
MAX_DIAGNOSTIC_BYTES = 64 * 1024
MAX_REQUEST_BYTES = 64 * 1024
DEPTH_COMMANDS = {
    "argb8": "--render-experimental-smart-request",
    "argb16": "--render-experimental-smart-request-16",
    "argb32f": "--render-experimental-smart-request-32-cpu",
}
PIXEL_BYTES = {"argb8": 4, "argb16": 8, "argb32f": 16}
RAW_SUFFIX = {"argb8": "rgba8", "argb16": "rgba16le", "argb32f": "rgba32f-le"}


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def validator(name: str) -> Draft202012Validator:
    schema = load_json(SCHEMAS / name)
    manifest = load_json(SCHEMAS / "conformance-manifest.schema.json")
    registry = Registry().with_resource(
        manifest["$id"], Resource.from_contents(manifest)
    )
    return Draft202012Validator(schema, registry=registry)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def bounded_bytes(data: bytes | bytearray | None) -> dict[str, Any]:
    raw = bytes(data or b"")
    truncated = len(raw) > MAX_DIAGNOSTIC_BYTES
    raw = raw[:MAX_DIAGNOSTIC_BYTES]
    return {
        "text": raw.decode("utf-8", errors="replace"),
        "truncated": truncated,
        "bytes_kept": len(raw),
    }


def bounded_text(text: str) -> dict[str, Any]:
    return bounded_bytes(text.encode("utf-8", errors="replace"))


def _real_directory(path: Path) -> Path:
    metadata = path.lstat()
    if stat.S_ISLNK(metadata.st_mode) or _is_reparse(metadata):
        raise ValueError("directory must not be a symlink or reparse point")
    resolved = path.resolve(strict=True)
    resolved_metadata = resolved.lstat()
    if not resolved.is_dir() or stat.S_ISLNK(resolved_metadata.st_mode) or _is_reparse(resolved_metadata):
        raise ValueError("directory must be a real directory")
    return resolved


def _artifact_relative_path(artifact: dict[str, Any]) -> Path:
    return Path(*artifact["path"].split("/"))


def copy_verified_artifact(
    source_root: Path, artifact: dict[str, Any], destination: Path
) -> None:
    """Copy and hash one source artifact through a single stable read handle."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor = None
    temporary_name: str | None = None
    temporary_fd: int | None = None
    try:
        descriptor, identity = _open_artifact_beneath(
            source_root, _artifact_relative_path(artifact)
        )
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode) or _is_reparse(before):
            raise ValueError(f"artifact is not a regular file: {artifact['path']}")
        if before.st_size != artifact["size_bytes"]:
            raise ValueError(f"artifact size changed: {artifact['path']}")

        temporary_fd, temporary_name = tempfile.mkstemp(
            prefix=".aexcompat-artifact-", dir=str(destination.parent)
        )
        digest = hashlib.sha256()
        copied = 0
        with os.fdopen(descriptor, "rb", closefd=True) as source:
            descriptor = None
            with os.fdopen(temporary_fd, "wb", closefd=True) as target:
                temporary_fd = None
                for chunk in iter(lambda: source.read(1024 * 1024), b""):
                    target.write(chunk)
                    digest.update(chunk)
                    copied += len(chunk)
                target.flush()
                os.fsync(target.fileno())
            after = os.fstat(source.fileno())

        after_identity = (
            (after.st_dev, after.st_ino)
            if os.name != "nt"
            else identity
        )
        if copied != artifact["size_bytes"]:
            raise ValueError(f"artifact changed while copied: {artifact['path']}")
        if after_identity != identity or any(
            getattr(before, field) != getattr(after, field)
            for field in ("st_size", "st_mtime_ns", "st_ctime_ns")
        ):
            raise ValueError(f"artifact changed while copied: {artifact['path']}")
        if digest.hexdigest() != artifact["sha256"]:
            raise ValueError(f"artifact SHA-256 does not match: {artifact['path']}")
        os.replace(temporary_name, destination)
        temporary_name = None
    finally:
        if descriptor is not None:
            os.close(descriptor)
        if temporary_fd is not None:
            os.close(temporary_fd)
        if temporary_name is not None:
            try:
                os.unlink(temporary_name)
            except FileNotFoundError:
                pass


def artifact_for(path: Path, root: Path) -> dict[str, Any]:
    return {
        "path": path.relative_to(root).as_posix(),
        "sha256": sha256(path),
        "size_bytes": path.stat().st_size,
    }


def world(width: int, height: int, depth: str, premultiplication: str) -> dict[str, Any]:
    return {
        "width": width,
        "height": height,
        "row_bytes": width * PIXEL_BYTES[depth],
        "pixel_format": depth,
        "premultiplication": premultiplication,
        "extent_hint": {"left": 0, "top": 0, "right": width, "bottom": height},
    }


def write_native_input(source: Path, destination: Path, depth: str) -> tuple[int, int]:
    with Image.open(source) as image:
        rgba = image.convert("RGBA")
        width, height = rgba.size
        samples = rgba.tobytes()
    destination.parent.mkdir(parents=True, exist_ok=True)
    if depth == "argb8":
        destination.write_bytes(samples)
    elif depth == "argb16":
        with destination.open("wb") as stream:
            for sample in samples:
                stream.write(struct.pack("<H", (sample * 32768 + 127) // 255))
    else:
        with destination.open("wb") as stream:
            for sample in samples:
                stream.write(struct.pack("<f", sample / 255.0))
    return width, height


def failed_result(
    depth: str, input_world: dict[str, Any], classification: str = "nonzero_exit"
) -> dict[str, Any]:
    return {
        "depth": depth,
        "classification": classification,
        "selector": {"render_path": "smartfx", "completed": False, "error_code": None},
        "input_world": input_world,
        "world": None,
        "raw_input": None,
        "raw_output": None,
        "output_sha256": None,
        "suite_timeline": [],
        "oracle": {"state": "not_captured", "identity_match": False, "exact": False},
    }


def normalize_harness_report(
    depth: str,
    value: dict[str, Any],
    output: Path,
    input_world: dict[str, Any],
    premultiplication: str,
) -> dict[str, Any]:
    if not value.get("passed") or not output.is_file():
        return failed_result(depth, input_world, "invalid_output")
    try:
        width = int(value["width"])
        height = int(value["height"])
    except (KeyError, TypeError, ValueError):
        return failed_result(depth, input_world, "invalid_output")
    result = {
        "depth": depth,
        "classification": "ok",
        "selector": {
            "render_path": value.get("render_path", "smartfx"),
            "completed": True,
            "error_code": 0,
        },
        "input_world": input_world,
        "world": world(width, height, depth, premultiplication),
        "raw_input": None,
        "raw_output": None,
        "output_sha256": sha256(output),
        "suite_timeline": value.get("suite_timeline", [])[:65536],
        "oracle": {"state": "not_captured", "identity_match": False, "exact": False},
    }
    missing = value.get("missing_suites")
    if isinstance(missing, list) and missing:
        result["missing_suites"] = missing[:16]
    return result


def _stream_to_bounded_file(stream, destination: Path, state: dict[str, Any]) -> None:
    kept = 0
    try:
        with destination.open("wb") as output:
            while True:
                chunk = stream.read(64 * 1024)
                if not chunk:
                    break
                if kept < MAX_DIAGNOSTIC_BYTES:
                    retained = chunk[: MAX_DIAGNOSTIC_BYTES - kept]
                    output.write(retained)
                    kept += len(retained)
                if len(chunk) > max(0, MAX_DIAGNOSTIC_BYTES - kept):
                    state["truncated"] = True
    except (OSError, ValueError) as error:
        state["error"] = str(error)
    state["bytes_kept"] = kept


def _read_bounded_file(path: Path, state: dict[str, Any]) -> dict[str, Any]:
    try:
        result = bounded_bytes(path.read_bytes())
    except OSError as error:
        result = bounded_text(str(error))
        result["read_error"] = True
    result["truncated"] = bool(result.get("truncated") or state.get("truncated"))
    result["bytes_kept"] = state.get("bytes_kept", result.get("bytes_kept", 0))
    if state.get("error"):
        result["stream_error"] = state["error"]
    return result


def _private_path_argument(value: str) -> bool:
    return Path(value).is_absolute() or ":\\" in value or value.startswith("\\\\")


def structured_argv(
    command: list[str], bundle_root: Path, known_paths: dict[str, tuple[str, str]]
) -> list[dict[str, Any]]:
    known = {str(Path(key)).casefold(): value for key, value in known_paths.items()}
    result = []
    for index, value in enumerate(command):
        role_value = known.get(str(Path(value)).casefold())
        if role_value:
            role, relative = role_value
            result.append({"index": index, "kind": "path", "role": role, "value": f"<bundle>/{relative}"})
        elif index == 0:
            result.append({"index": index, "kind": "executable", "value": Path(value).name})
        elif _private_path_argument(value):
            result.append({"index": index, "kind": "private_path", "value": f"<private>/{Path(value).name}"})
        else:
            result.append({"index": index, "kind": "argument", "value": value})
    return result


def run_process(
    command: list[str], cwd: Path, environment: dict[str, str]
) -> tuple[int | None, str, str, bool, dict[str, Any]]:
    with tempfile.TemporaryDirectory(prefix="aexcompat-process-") as temporary:
        stdout_path = Path(temporary) / "stdout.bin"
        stderr_path = Path(temporary) / "stderr.bin"
        stdout_state: dict[str, Any] = {}
        stderr_state: dict[str, Any] = {}
        try:
            process = subprocess.Popen(
                command,
                cwd=cwd,
                env=environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
        except OSError as error:
            return None, "", "", False, {"spawn_error": str(error)}

        stdout_thread = threading.Thread(
            target=_stream_to_bounded_file,
            args=(process.stdout, stdout_path, stdout_state),
            daemon=True,
        )
        stderr_thread = threading.Thread(
            target=_stream_to_bounded_file,
            args=(process.stderr, stderr_path, stderr_state),
            daemon=True,
        )
        stdout_thread.start()
        stderr_thread.start()
        timed_out = False
        try:
            returncode = process.wait(timeout=120)
        except subprocess.TimeoutExpired:
            timed_out = True
            process.kill()
            returncode = process.wait()
        for thread in (stdout_thread, stderr_thread):
            thread.join(timeout=5)
        if stdout_thread.is_alive() or stderr_thread.is_alive():
            for stream in (process.stdout, process.stderr):
                if stream is not None:
                    stream.close()
            stdout_thread.join(timeout=1)
            stderr_thread.join(timeout=1)

        stdout = _read_bounded_file(stdout_path, stdout_state)["text"]
        stderr = _read_bounded_file(stderr_path, stderr_state)["text"]
        return returncode, stdout, stderr, timed_out, {
            "stdout": _read_bounded_file(stdout_path, stdout_state),
            "stderr": _read_bounded_file(stderr_path, stderr_state),
        }


def _parameter_assignment(parameter: dict[str, Any]) -> dict[str, Any]:
    value = parameter["value"]
    assignment: dict[str, Any] = {"slot": parameter["index"]}
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        assignment["value"] = value
    elif isinstance(value, str):
        assignment["text"] = value
    elif isinstance(value, list) and all(isinstance(item, (int, float)) and not isinstance(item, bool) for item in value):
        if "color" in parameter["type"].casefold() and len(value) == 4:
            assignment["color"] = value
        else:
            assignment["components"] = value
    else:
        raise ValueError(f"parameter {parameter['index']} cannot be represented by harness sidecar")
    return assignment


def write_request_sidecar(manifest: dict[str, Any], depth: str, destination: Path) -> None:
    del depth  # The pixel format is selected by the canonical CLI flag.
    time_value = manifest["execution"]["time"]["value"]
    time_scale = manifest["execution"]["time"]["scale"]
    if not 0 <= time_value <= 10_000_000:
        raise ValueError("execution.time.value is outside the harness timing range")
    if not 1 <= time_scale <= 1_000_000:
        raise ValueError("execution.time.scale is outside the harness timing range")
    request = {
        "schema_version": 1,
        "timing": {"frame": time_value, "time_scale": time_scale, "time_step": 1},
        "assignments": [_parameter_assignment(item) for item in manifest["execution"]["parameters"]],
    }
    encoded = (json.dumps(request, indent=2, sort_keys=True) + "\n").encode("utf-8")
    if len(encoded) > MAX_REQUEST_BYTES:
        raise ValueError("generated harness request exceeds 64 KiB")
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(encoded)


def run_depth(
    depth: str,
    harness: Path,
    plugin: Path,
    input_path: Path,
    output: Path,
    request_path: Path,
    dump_dir: Path,
    adapter: Path | None,
    input_world: dict[str, Any],
    premultiplication: str,
    bundle_root: Path,
) -> tuple[dict[str, Any], dict[str, Any]]:
    if adapter:
        command = [
            sys.executable,
            str(adapter),
            "--depth",
            depth,
            "--runner",
            str(harness),
            "--plugin",
            str(plugin),
            "--input",
            str(input_path),
            "--output",
            str(output),
            "--request",
            str(request_path),
            "--world-dump-dir",
            str(dump_dir),
        ]
    else:
        command = [
            str(harness),
            DEPTH_COMMANDS[depth],
            str(plugin),
            str(input_path),
            str(output),
            str(request_path),
        ]
    known_paths = {
        str(harness): ("runner", harness.relative_to(bundle_root).as_posix()),
        str(plugin): ("plugin", plugin.relative_to(bundle_root).as_posix()),
        str(input_path): ("input", input_path.relative_to(bundle_root).as_posix()),
        str(output): ("output", output.relative_to(bundle_root).as_posix()),
        str(request_path): ("request", request_path.relative_to(bundle_root).as_posix()),
    }
    detail: dict[str, Any] = {
        "argv": structured_argv(command, bundle_root, known_paths),
        "cwd": "<repo>",
    }
    environment = dict(os.environ)
    environment["AEXCOMPAT_DUMP_WORLDS_DIR"] = dump_dir.relative_to(ROOT).as_posix()
    environment["AEXCOMPAT_CHECKSUM_DETAIL"] = "1"
    returncode, stdout, stderr, timed_out, stream_detail = run_process(command, ROOT, environment)
    detail.update(stream_detail)
    if stream_detail.get("spawn_error"):
        detail["spawn_error"] = bounded_text(stream_detail["spawn_error"])
        return failed_result(depth, input_world, "host_validation_error"), detail
    if timed_out:
        return failed_result(depth, input_world, "timeout_killed"), detail
    if returncode != 0:
        return failed_result(depth, input_world), detail
    try:
        value = json.loads(stdout)
    except json.JSONDecodeError:
        return failed_result(depth, input_world, "invalid_output"), detail
    if not isinstance(value, dict):
        return failed_result(depth, input_world, "invalid_output"), detail
    if adapter:
        if value.get("classification") == "ok":
            if not output.is_file() or value.get("output_sha256") != sha256(output):
                return failed_result(depth, input_world, "invalid_output"), detail
        return value, detail
    return normalize_harness_report(depth, value, output, input_world, premultiplication), detail


def attach_raw_artifacts(
    result: dict[str, Any],
    depth: str,
    source_input: Path,
    output: Path,
    dump_dir: Path,
    bundle_root: Path,
) -> None:
    raw_root = bundle_root / "raw" / depth
    raw_root.mkdir(parents=True, exist_ok=True)
    input_candidates = sorted(dump_dir.glob("*input*")) if dump_dir.exists() else []
    output_candidates = sorted(dump_dir.glob("*output*")) if dump_dir.exists() else []
    raw_input = raw_root / f"input.{RAW_SUFFIX[depth]}"
    if input_candidates:
        shutil.copyfile(input_candidates[-1], raw_input)
    else:
        write_native_input(source_input, raw_input, depth)
    result["raw_input"] = artifact_for(raw_input, bundle_root)

    if result["classification"] != "ok":
        result["raw_output"] = None
        return
    raw_output = raw_root / f"output.{RAW_SUFFIX[depth]}"
    if output_candidates:
        shutil.copyfile(output_candidates[-1], raw_output)
    elif depth != "argb8" and output.with_suffix(f".{RAW_SUFFIX[depth]}").is_file():
        shutil.copyfile(output.with_suffix(f".{RAW_SUFFIX[depth]}"), raw_output)
    elif depth == "argb8":
        try:
            write_native_input(output, raw_output, depth)
        except Exception:
            result.update({"classification": "invalid_output", "world": None, "raw_output": None, "output_sha256": None})
            return
    else:
        result.update({"classification": "invalid_output", "world": None, "raw_output": None, "output_sha256": None})
        return
    result["raw_output"] = artifact_for(raw_output, bundle_root)
    result["output_sha256"] = result["raw_output"]["sha256"]


def _mismatched_pixels(expected: Path, actual: Path, depth: str) -> int:
    expected_size = expected.stat().st_size
    actual_size = actual.stat().st_size
    if expected_size != actual_size or expected_size % PIXEL_BYTES[depth] != 0:
        return 1
    mismatched = 0
    with expected.open("rb") as left, actual.open("rb") as right:
        while True:
            left_chunk = left.read(1024 * 1024)
            right_chunk = right.read(1024 * 1024)
            if not left_chunk and not right_chunk:
                break
            pixel_bytes = PIXEL_BYTES[depth]
            for offset in range(0, len(left_chunk), pixel_bytes):
                if left_chunk[offset : offset + pixel_bytes] != right_chunk[offset : offset + pixel_bytes]:
                    mismatched = min(0xFFFFFFFF, mismatched + 1)
    return mismatched


def attach_oracle(
    result: dict[str, Any],
    depth: str,
    oracle_manifest: dict[str, Any],
    bundle_root: Path,
    oracle_state: str,
    manifest_identity_match: bool,
) -> None:
    if oracle_state != "captured" or result["classification"] != "ok" or result["raw_output"] is None:
        return
    oracle_path = bundle_root / oracle_manifest["path"]
    actual_path = bundle_root / result["raw_output"]["path"]
    expected_hash = oracle_manifest["sha256"]
    actual_hash = result["raw_output"]["sha256"]
    mismatched = _mismatched_pixels(oracle_path, actual_path, depth)
    result["oracle"] = {
        "state": "captured",
        "identity_match": manifest_identity_match and expected_hash == actual_hash,
        "exact": manifest_identity_match and expected_hash == actual_hash and mismatched == 0,
        "expected_sha256": expected_hash,
        "actual_sha256": actual_hash,
        "mismatched_pixels": mismatched,
    }


def runner_identity() -> dict[str, Any]:
    path = Path(__file__)
    return {
        "path": "<repo>/tools/run-conformance-bundle.py",
        "sha256": sha256(path),
        "size_bytes": path.stat().st_size,
    }


def write_run_state(
    output_root: Path, status: str, depth_diagnostics: dict[str, Any], report_written: bool
) -> None:
    path = output_root / "diagnostics" / "run.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    document = {
        "schema_version": 1,
        "status": status,
        "report_written": report_written,
        "bundle_runner": runner_identity(),
        "depths": depth_diagnostics,
    }
    path.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_failure_evidence(output_root: Path, stage: str, error: BaseException) -> None:
    path = output_root / "diagnostics" / "failure.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    document = {
        "kind": "aexcompat.conformance.failure-evidence",
        "schema_version": 1,
        "status": "failed",
        "stage": stage,
        "error_type": type(error).__name__,
        "error": bounded_text(str(error)),
        "report_written": False,
        "report_status": "not_a_report",
        "run_evidence": "diagnostics/run.json",
    }
    path.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--adapter-command", type=Path)
    args = parser.parse_args()

    output_root = args.out.resolve()
    output_root.mkdir(parents=True, exist_ok=False)
    depth_diagnostics: dict[str, Any] = {}
    write_run_state(output_root, "started", depth_diagnostics, False)
    stage = "load_manifest"
    try:
        manifest_path = args.manifest.resolve()
        manifest = load_json(manifest_path)
        stage = "validate_manifest"
        validator("conformance-manifest.schema.json").validate(manifest)
        source_root = _real_directory(manifest_path.parent)

        artifacts = [
            manifest["plugin"]["aex"],
            *manifest["plugin"]["dependencies"],
            manifest["input"],
            manifest["runner"],
        ]
        oracle_artifacts = manifest["oracle"].get("artifacts", {})
        artifacts.extend(oracle_artifacts.values())
        by_path: dict[str, dict[str, Any]] = {}
        for item in artifacts:
            previous = by_path.get(item["path"])
            if previous is not None and previous != item:
                raise ValueError(f"conflicting artifact identity: {item['path']}")
            by_path[item["path"]] = item

        stage = "copy_pinned_artifacts"
        for item in by_path.values():
            copy_verified_artifact(source_root, item, output_root / item["path"])
        (output_root / "manifest.json").write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )

        outputs = output_root / "outputs"
        requests = output_root / "requests"
        diagnostics_dir = output_root / "diagnostics"
        outputs.mkdir()
        requests.mkdir()
        diagnostics_dir.mkdir(exist_ok=True)
        results = []
        with Image.open(output_root / manifest["input"]["path"]) as source_image:
            input_width, input_height = source_image.size

        for depth in manifest["requested_depths"]:
            stage = f"render_{depth}"
            request_path = requests / f"{depth}.json"
            write_request_sidecar(manifest, depth, request_path)
            output = outputs / f"{depth}.png"
            dump_dir = ROOT / "target" / f"conformance-{manifest['fixture_id']}-{os.getpid()}-{depth}"
            if dump_dir.exists():
                shutil.rmtree(dump_dir)
            input_world = world(
                input_width,
                input_height,
                depth,
                manifest["execution"]["premultiplication"],
            )
            result, detail = run_depth(
                depth,
                output_root / manifest["runner"]["path"],
                output_root / manifest["plugin"]["aex"]["path"],
                output_root / manifest["input"]["path"],
                output,
                request_path,
                dump_dir,
                args.adapter_command.resolve() if args.adapter_command else None,
                input_world,
                manifest["execution"]["premultiplication"],
                output_root,
            )
            attach_raw_artifacts(
                result,
                depth,
                output_root / manifest["input"]["path"],
                output,
                dump_dir,
                output_root,
            )
            if dump_dir.exists():
                shutil.move(str(dump_dir), str(outputs / f"{depth}-worlds"))
            if manifest["oracle"]["state"] == "captured":
                attach_oracle(
                    result,
                    depth,
                    oracle_artifacts[depth],
                    output_root,
                    manifest["oracle"]["state"],
                    manifest["oracle"]["identity_match"],
                )
            detail["request"] = artifact_for(request_path, output_root)
            results.append(result)
            depth_diagnostics[depth] = detail
            write_run_state(output_root, "running", depth_diagnostics, False)

        stage = "validate_report"
        report = {
            "schema_version": 1,
            "fixture_id": manifest["fixture_id"],
            "identities": {
                "aex": manifest["plugin"]["aex"],
                "dependencies": manifest["plugin"]["dependencies"],
                "input": manifest["input"],
                "runner": manifest["runner"],
            },
            "parameters": [
                {
                    "index": parameter["index"],
                    "type": parameter["type"],
                    "initial_value": parameter["value"],
                    "host_range": None,
                    "user_range": None,
                }
                for parameter in manifest["execution"]["parameters"]
            ],
            "results": results,
        }
        validate_bundle(manifest, report, output_root)
        (output_root / "report.json").write_text(
            json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        write_run_state(output_root, "completed", depth_diagnostics, True)
    except Exception as error:
        write_failure_evidence(output_root, stage, error)
        write_run_state(output_root, "failed", depth_diagnostics, False)
        raise
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
