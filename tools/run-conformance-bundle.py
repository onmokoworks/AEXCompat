#!/usr/bin/env python3
"""Create a self-contained AEX conformance evidence bundle."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
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


def strict_json_loads(text: str) -> Any:
    def reject_constant(value: str) -> None:
        raise ValueError(f"non-finite JSON number is not permitted: {value}")

    value = json.loads(text, parse_constant=reject_constant)

    def reject_overflowed_numbers(item: Any) -> None:
        if isinstance(item, float) and not math.isfinite(item):
            raise ValueError(f"non-finite JSON number is not permitted: {item}")
        if isinstance(item, dict):
            for child in item.values():
                reject_overflowed_numbers(child)
        elif isinstance(item, list):
            for child in item:
                reject_overflowed_numbers(child)

    reject_overflowed_numbers(value)
    return value


MAX_PROTOCOL_BYTES = int(
    strict_json_loads((SCHEMAS / "conformance-report.schema.json").read_text(encoding="utf-8"))[
        "x-protocol-max-bytes"
    ]
)
DEPTH_COMMANDS = {
    ("classic", "argb8"): "--render-experimental-request",
    ("classic", "argb16"): "--render-experimental-request-16",
    ("classic", "argb32f"): "--render-experimental-request-32",
    ("smartfx", "argb8"): "--render-experimental-smart-request",
    ("smartfx", "argb16"): "--render-experimental-smart-request-16",
    ("smartfx", "argb32f"): "--render-experimental-smart-request-32-cpu",
}
PIXEL_BYTES = {"argb8": 4, "argb16": 8, "argb32f": 16}
RAW_SUFFIX = {"argb8": "rgba8", "argb16": "rgba16le", "argb32f": "rgba32f-le"}
NATIVE_WORKERS = (
    "target/minihost-build/aex_l2_worker.exe",
    "target/minihost-build/aex_render_worker.exe",
    "target/minihost-build/aex_smart_worker.exe",
)
RESERVED_BUNDLE_FILES = {"manifest.json", "report.json"}
RESERVED_BUNDLE_DIRECTORIES = {"diagnostics", "outputs", "raw", "requests", "target"}


def load_json(path: Path) -> Any:
    return strict_json_loads(path.read_text(encoding="utf-8"))


def validate_artifact_destination(path: str) -> None:
    normalized = path.replace("\\", "/").casefold()
    first = normalized.split("/", 1)[0]
    if (
        any(normalized == name or normalized.startswith(name + "/") for name in RESERVED_BUNDLE_FILES)
        or first in RESERVED_BUNDLE_DIRECTORIES
    ):
        raise ValueError(f"artifact path collides with generated bundle content: {path}")


def meaningful_selector_error(value: dict[str, Any]) -> int | None:
    for field in (
        "pre_render_error",
        "smart_render_error",
        "render_error",
        "smart_render_selector_error",
        "selector_error",
    ):
        candidate = value.get(field)
        if isinstance(candidate, int) and not isinstance(candidate, bool) and candidate not in (0, -1):
            return candidate
    return None


def reported_render_path_matches(value: dict[str, Any], expected: str) -> bool:
    if "render_path" in value and value.get("render_path") != expected:
        return False
    selector = value.get("selector")
    if isinstance(selector, dict) and "render_path" in selector:
        return selector.get("render_path") == expected
    return True


def is_crash_exit_code(returncode: int) -> bool:
    return returncode < 0 or returncode >= 0xC0000000


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


def bounded_protocol_bytes(data: bytes | bytearray | None) -> dict[str, Any]:
    raw = bytes(data or b"")
    truncated = len(raw) > MAX_PROTOCOL_BYTES
    raw = raw[:MAX_PROTOCOL_BYTES]
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


def copy_native_workers(bundle_root: Path) -> list[dict[str, Any]]:
    identities = []
    for relative in NATIVE_WORKERS:
        source = ROOT / Path(*relative.split("/"))
        identity = artifact_for(source, ROOT)
        destination = bundle_root / Path(*relative.split("/"))
        if destination.exists():
            raise ValueError(f"native worker destination collides with an artifact: {relative}")
        copy_verified_artifact(ROOT, identity, destination)
        identities.append(identity)
    return identities


def world(width: int, height: int, depth: str, premultiplication: str) -> dict[str, Any]:
    return {
        "width": width,
        "height": height,
        "row_bytes": width * PIXEL_BYTES[depth],
        "pixel_format": depth,
        "premultiplication": premultiplication,
        "extent_hint": {"left": 0, "top": 0, "right": width, "bottom": height},
    }


def write_native_input(
    source: Path, destination: Path, depth: str, premultiplication: str = "straight"
) -> tuple[int, int]:
    with Image.open(source) as image:
        rgba = image.convert("RGBA")
        width, height = rgba.size
        samples = bytearray(rgba.tobytes())
    for offset in range(0, len(samples), 4):
        alpha = samples[offset + 3]
        if premultiplication == "premultiplied":
            for channel in range(3):
                samples[offset + channel] = (samples[offset + channel] * alpha + 127) // 255
        elif premultiplication == "opaque":
            samples[offset + 3] = 255
        elif premultiplication != "straight":
            raise ValueError(f"unsupported premultiplication: {premultiplication}")
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
    depth: str,
    input_world: dict[str, Any],
    classification: str = "nonzero_exit",
    render_path: str = "smartfx",
) -> dict[str, Any]:
    return {
        "depth": depth,
        "classification": classification,
        "selector": {"render_path": render_path, "completed": False, "error_code": None},
        "input_world": input_world,
        "world": None,
        "raw_input": None,
        "raw_output": None,
        "output_sha256": None,
        "suite_timeline": None,
        "oracle": {"state": "not_captured", "identity_match": False, "exact": False},
    }


def _is_sha256_hex(value: Any) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(character in "0123456789abcdef" for character in value)
    )


def empty_smartfx_result(
    depth: str, value: dict[str, Any], input_world: dict[str, Any]
) -> dict[str, Any]:
    # A SmartFX render that legally answers an empty result_rect renders no
    # pixels: the broker reports zero geometry and an empty output, hashed as
    # sha256(b""). world/raw_output stay null (attach_raw_artifacts leaves
    # raw_output None for any non-"ok" result); output_sha256 carries the empty
    # hash the schema/validator require for empty_result.
    actual_input_world = value.get("input_world")
    output_sha256 = value.get("output_sha256")
    if not isinstance(actual_input_world, dict) or not _is_sha256_hex(output_sha256):
        return failed_result(depth, input_world, "invalid_output", "smartfx")
    return {
        "depth": depth,
        "classification": "empty_result",
        "selector": {"render_path": "smartfx", "completed": True, "error_code": 0},
        "input_world": actual_input_world,
        "world": None,
        "raw_input": None,
        "raw_output": None,
        "output_sha256": output_sha256,
        "suite_timeline": schema_valid_suite_timeline(value.get("suite_timeline")),
        "_parameter_metadata": value["parameter_metadata"],
        "oracle": {"state": "not_captured", "identity_match": False, "exact": False},
    }


def world_from_report_dims(
    value: dict[str, Any], depth: str, premultiplication: str
) -> dict[str, Any] | None:
    # The Classic worker report exposes geometry as top-level width/height/rowbytes
    # and emits no premultiplication (only begin_smart does), so the broker forwards
    # the input_world/output_world objects as null. Rebuild the output world (a
    # Classic render fills the whole frame) from those worker dims, pinning
    # pixel_format to the requested depth and premultiplication to the manifest's
    # requested alpha mode the broker pre-transformed the input to.
    try:
        width = int(value["width"])
        height = int(value["height"])
        row_bytes = int(value["row_bytes"])
    except (KeyError, TypeError, ValueError):
        return None
    if width < 0 or height < 0 or row_bytes < 0:
        return None
    return {
        "width": width,
        "height": height,
        "row_bytes": row_bytes,
        "pixel_format": depth,
        "premultiplication": premultiplication,
        "extent_hint": {"left": 0, "top": 0, "right": width, "bottom": height},
    }


def normalize_harness_report(
    depth: str,
    value: dict[str, Any],
    output: Path,
    input_world: dict[str, Any],
    premultiplication: str,
    render_path: str = "smartfx",
) -> dict[str, Any]:
    if not value.get("passed") or not isinstance(value.get("parameter_metadata"), list):
        return failed_result(depth, input_world, "invalid_output", render_path)
    # A SmartFX render may legally answer an empty result_rect: the broker
    # reports empty_result_rect with zero geometry and intentionally writes no
    # PNG. Requiring output.is_file() here would mislabel that schema-supported
    # empty_result as invalid_output, dropping a valid conformance result.
    if render_path == "smartfx" and value.get("empty_result_rect") is True:
        return empty_smartfx_result(depth, value, input_world)
    if not output.is_file():
        return failed_result(depth, input_world, "invalid_output", render_path)
    # SmartFX reports carry explicit input_world/output_world objects
    # (begin_smart); the Classic report only exposes top-level
    # width/height/rowbytes/pixel_format/premultiplication plus world_debug_json,
    # so the broker forwards those world objects as null. Rebuild the output
    # world from the authoritative worker dims (a Classic render fills the whole
    # frame) and use the known input world when the report omits them, otherwise
    # a successful Classic render would be mislabeled invalid_output.
    actual_input_world = value.get("input_world")
    actual_world = value.get("output_world")
    if not isinstance(actual_world, dict):
        actual_world = world_from_report_dims(value, depth, premultiplication)
    if not isinstance(actual_input_world, dict):
        actual_input_world = input_world
    if not isinstance(actual_world, dict) or not isinstance(actual_input_world, dict):
        return failed_result(depth, input_world, "invalid_output", render_path)
    try:
        int(actual_world["width"])
        int(actual_world["height"])
    except (KeyError, TypeError, ValueError):
        return failed_result(depth, input_world, "invalid_output", render_path)
    result = {
        "depth": depth,
        "classification": "ok",
        "selector": {
            "render_path": render_path,
            "completed": True,
            "error_code": 0,
        },
        "input_world": actual_input_world,
        "world": actual_world,
        "raw_input": None,
        "raw_output": None,
        "output_sha256": sha256(output),
        "suite_timeline": schema_valid_suite_timeline(value.get("suite_timeline")),
        "_parameter_metadata": value["parameter_metadata"],
        "oracle": {"state": "not_captured", "identity_match": False, "exact": False},
    }
    return result


# Mirrors the missing_suites[*].name pattern in conformance-report.schema.json.
# The native collectors accept a wider set (dots, up to 96 bytes), so the report
# schema is the stricter authority the bundle must satisfy.
_MISSING_SUITE_NAME = re.compile(r"[A-Za-z][A-Za-z0-9 _-]{0,62}[A-Za-z0-9]")


def schema_valid_missing_suites(missing: Any) -> list[dict[str, Any]]:
    if not isinstance(missing, list):
        return []
    valid: list[dict[str, Any]] = []
    seen: set[tuple[str, int]] = set()
    for entry in missing:
        if not isinstance(entry, dict):
            continue
        name = entry.get("name")
        version = entry.get("version")
        if (
            not isinstance(name, str)
            or _MISSING_SUITE_NAME.fullmatch(name) is None
            or not isinstance(version, int)
            or isinstance(version, bool)
            or not 1 <= version <= 65535
        ):
            continue
        key = (name, version)
        if key in seen:
            continue
        seen.add(key)
        valid.append({"name": name, "version": version})
        if len(valid) >= 16:
            break
    return valid


# Mirrors $defs/suite_event in conformance-report.schema.json. The native suite
# collector accepts names the report schema rejects (e.g. dotted names), so a
# timeline entry naming such a suite must be dropped before it reaches the bundle.
_SUITE_EVENT_SELECTOR = re.compile(r"[A-Za-z0-9_ -]+")
_SUITE_EVENT_KEYS = {"sequence", "action", "name", "version", "selector", "result"}


def _is_bounded_int(value: Any, low: int, high: int) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and low <= value <= high


def _valid_suite_event(event: Any) -> bool:
    if not isinstance(event, dict) or set(event.keys()) != _SUITE_EVENT_KEYS:
        return False
    name = event["name"]
    selector = event["selector"]
    return (
        _is_bounded_int(event["sequence"], 0, 4294967295)
        and event["action"] in ("acquire", "release")
        and isinstance(name, str)
        and _MISSING_SUITE_NAME.fullmatch(name) is not None
        and _is_bounded_int(event["version"], 1, 65535)
        and isinstance(selector, str)
        and 1 <= len(selector) <= 64
        and _SUITE_EVENT_SELECTOR.fullmatch(selector) is not None
        and isinstance(event["result"], int)
        and not isinstance(event["result"], bool)
    )


def schema_valid_suite_timeline(timeline: Any) -> list[dict[str, Any]] | None:
    # Drop timeline entries the report schema would reject so a single
    # schema-incompatible suite name cannot make validate_bundle discard the whole
    # bundle. A non-list (or absent) timeline stays null.
    if not isinstance(timeline, list):
        return None
    return [event for event in timeline if _valid_suite_event(event)][:65536]


def normalize_structured_failure(
    depth: str,
    value: dict[str, Any],
    input_world: dict[str, Any],
    render_path: str = "smartfx",
) -> dict[str, Any]:
    classification = value.get("classification")
    if value.get("plugin_kind") in {"aegp_candidate", "unknown_no_effect_entrypoint"}:
        classification = "loader_error"
    # Schema-filter the reported missing suites up front. Only entries whose name
    # the report schema accepts make a render a missing_suite; an unfiltered copy
    # of a schema-incompatible name would make validate_bundle reject the whole
    # report and discard the otherwise-useful bounded evidence.
    raw_missing = value.get("missing_suites")
    if not raw_missing and isinstance(value.get("worker_diagnostics"), dict):
        raw_missing = value["worker_diagnostics"].get("missing_suites")
    missing = schema_valid_missing_suites(raw_missing)
    # The native harness reports a process-level classification that is usually
    # the generic `nonzero_exit`, alongside report evidence (render_error,
    # pre_render_error, depth_supported, missing_suites). Treat `nonzero_exit`
    # (and any unrecognized value) as refinable so the evidence branches below
    # can upgrade it to the actionable selector_error/unsupported/missing_suite,
    # while keeping specific classes (crashes, timeouts, loader/host errors) final.
    if classification not in {
        "loader_error",
        "unsupported",
        "selector_error",
        "missing_suite",
        "crashed",
        "timeout_killed",
        "invalid_output",
        "host_validation_error",
    }:
        worker_classification = value.get("worker_classification")
        if worker_classification in {"crashed", "timeout_killed", "host_validation_error"}:
            classification = worker_classification
        elif missing:
            classification = "missing_suite"
        elif meaningful_selector_error(value) is not None:
            classification = "selector_error"
        elif value.get("depth_supported") is False or value.get("smart_render_supported") is False:
            classification = "unsupported"
        else:
            classification = "nonzero_exit"
    selector = value.get("selector")
    if not isinstance(selector, dict):
        selector = {
            "render_path": render_path,
            "completed": False,
            "error_code": meaningful_selector_error(value),
        }
    actual_input_world = value.get("input_world")
    if not isinstance(actual_input_world, dict):
        actual_input_world = input_world
    actual_world = value.get("output_world")
    if not isinstance(actual_world, dict):
        actual_world = None
    result = {
        "depth": depth,
        "classification": classification,
        "selector": selector,
        "input_world": actual_input_world,
        "world": actual_world,
        "raw_input": None,
        "raw_output": None,
        "output_sha256": None,
        "suite_timeline": schema_valid_suite_timeline(value.get("suite_timeline")),
        "oracle": {"state": "not_captured", "identity_match": False, "exact": False},
    }
    if isinstance(value.get("parameter_metadata"), list):
        result["_parameter_metadata"] = value["parameter_metadata"]
    if value.get("plugin_kind") in {"aegp_candidate", "unknown_no_effect_entrypoint"}:
        result["plugin_kind"] = value["plugin_kind"]
    # Reconcile the classification with the schema-valid missing suites: a
    # generic nonzero_exit with valid missing suites becomes missing_suite, while
    # a missing_suite with no schema-valid entry falls back to nonzero_exit
    # (the schema requires at least one entry for missing_suite).
    if missing and result["classification"] == "nonzero_exit":
        result["classification"] = "missing_suite"
    if result["classification"] == "missing_suite":
        if missing:
            result["missing_suites"] = missing
        else:
            result["classification"] = "nonzero_exit"
    return result


def _stream_to_bounded_file(
    stream, destination: Path, state: dict[str, Any], limit: int
) -> None:
    kept = 0
    try:
        with destination.open("wb") as output:
            while True:
                chunk = stream.read(64 * 1024)
                if not chunk:
                    break
                remaining = max(0, limit - kept)
                if kept < limit:
                    retained = chunk[:remaining]
                    output.write(retained)
                    kept += len(retained)
                if len(chunk) > remaining:
                    state["truncated"] = True
    except (OSError, ValueError) as error:
        state["error"] = str(error)
    state["bytes_kept"] = kept


def _read_bounded_file(path: Path, state: dict[str, Any], limit: int) -> dict[str, Any]:
    try:
        result = bounded_protocol_bytes(path.read_bytes()) if limit == MAX_PROTOCOL_BYTES else bounded_bytes(path.read_bytes())
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
            args=(process.stdout, stdout_path, stdout_state, MAX_PROTOCOL_BYTES),
            daemon=True,
        )
        stderr_thread = threading.Thread(
            target=_stream_to_bounded_file,
            args=(process.stderr, stderr_path, stderr_state, MAX_DIAGNOSTIC_BYTES),
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

        stdout_detail = _read_bounded_file(stdout_path, stdout_state, MAX_PROTOCOL_BYTES)
        stderr_detail = _read_bounded_file(stderr_path, stderr_state, MAX_DIAGNOSTIC_BYTES)
        stdout = stdout_detail["text"]
        stderr = stderr_detail["text"]
        return returncode, stdout, stderr, timed_out, {
            "stdout": stdout_detail,
            "stderr": stderr_detail,
        }


def _parameter_assignment(
    parameter: dict[str, Any], pinned_paths: dict[str, str]
) -> dict[str, Any]:
    value = parameter["value"]
    assignment: dict[str, Any] = {"slot": parameter["index"]}
    parameter_type = parameter["type"].casefold().replace(" ", "_")
    if isinstance(value, bool):
        assignment["value"] = 1 if value else 0
    elif isinstance(value, (int, float)):
        assignment["value"] = value
    elif isinstance(value, str):
        if parameter_type == "layer":
            normalized = value.replace("\\", "/")
            canonical = pinned_paths.get(normalized.casefold())
            if canonical is None:
                raise ValueError(
                    f"layer parameter {parameter['index']} must reference a pinned bundle artifact"
                )
            # Persist a bundle-relative path so the request sidecar stays portable
            # (moving or replaying the bundle elsewhere keeps resolving) and never
            # leaks the creator's absolute filesystem layout. The harness resolves
            # it against the request's own bundle root at execution time, the same
            # way it resolves pinned dependencies.
            assignment["layer"] = canonical.replace("\\", "/")
        else:
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
    artifacts = [
        manifest["plugin"]["aex"],
        *manifest["plugin"]["dependencies"],
        manifest["input"],
        manifest["runner"],
        *manifest.get("oracle", {}).get("artifacts", {}).values(),
    ]
    pinned_paths = {
        artifact["path"].replace("\\", "/").casefold(): artifact["path"]
        for artifact in artifacts
    }
    request = {
        "schema_version": 1,
        "timing": {"frame": time_value, "time_scale": time_scale, "time_step": 1},
        "assignments": [
            _parameter_assignment(item, pinned_paths)
            for item in manifest["execution"]["parameters"]
        ],
        "dependencies": manifest["plugin"]["dependencies"],
        "render_settings": {
            "premultiplication": manifest["execution"]["premultiplication"],
            "color_management": manifest["execution"]["color_management"],
            "linear_light": manifest["execution"]["linear_light"],
            "renderer": manifest["execution"]["renderer"],
        },
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
    render_path: str,
) -> tuple[dict[str, Any], dict[str, Any]]:
    if adapter:
        command = [
            sys.executable,
            str(adapter),
            "--depth",
            depth,
            "--render-path",
            render_path,
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
            DEPTH_COMMANDS[(render_path, depth)],
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
    environment["AEXCOMPAT_REPOSITORY_ROOT"] = str(bundle_root)
    environment["AEXCOMPAT_DUMP_WORLDS_DIR"] = dump_dir.relative_to(bundle_root).as_posix()
    environment["AEXCOMPAT_CHECKSUM_DETAIL"] = "1"
    returncode, stdout, stderr, timed_out, stream_detail = run_process(command, ROOT, environment)
    detail.update(stream_detail)
    if stream_detail.get("spawn_error"):
        detail["spawn_error"] = bounded_text(stream_detail["spawn_error"])
        return failed_result(depth, input_world, "host_validation_error", render_path), detail
    if timed_out:
        return failed_result(depth, input_world, "timeout_killed", render_path), detail
    if is_crash_exit_code(returncode):
        return failed_result(depth, input_world, "crashed", render_path), detail
    try:
        value = strict_json_loads(stdout)
    except (json.JSONDecodeError, ValueError):
        if returncode != 0:
            return failed_result(depth, input_world, render_path=render_path), detail
        return failed_result(depth, input_world, "invalid_output", render_path), detail
    if not isinstance(value, dict):
        if returncode != 0:
            return failed_result(depth, input_world, render_path=render_path), detail
        return failed_result(depth, input_world, "invalid_output", render_path), detail
    if not reported_render_path_matches(value, render_path):
        return failed_result(depth, input_world, "host_validation_error", render_path), detail
    if returncode != 0:
        return normalize_structured_failure(depth, value, input_world, render_path), detail
    if adapter:
        if value.get("classification") == "ok":
            if (
                not output.is_file()
                or value.get("output_sha256") != sha256(output)
                or not isinstance(value.get("parameter_metadata"), list)
            ):
                return failed_result(depth, input_world, "invalid_output", render_path), detail
        parameter_metadata = value.pop("parameter_metadata", None)
        if isinstance(parameter_metadata, list):
            value["_parameter_metadata"] = parameter_metadata
        return value, detail
    return normalize_harness_report(
        depth, value, output, input_world, premultiplication, render_path
    ), detail


def attach_raw_artifacts(
    result: dict[str, Any],
    depth: str,
    source_input: Path,
    output: Path,
    dump_dir: Path,
    bundle_root: Path,
    premultiplication: str,
) -> None:
    raw_root = bundle_root / "raw" / depth
    raw_root.mkdir(parents=True, exist_ok=True)
    input_candidates = sorted(dump_dir.glob("*input*")) if dump_dir.exists() else []
    output_candidates = sorted(dump_dir.glob("*output*")) if dump_dir.exists() else []
    raw_input = raw_root / f"input.{RAW_SUFFIX[depth]}"
    if input_candidates:
        shutil.copyfile(input_candidates[-1], raw_input)
    else:
        write_native_input(source_input, raw_input, depth, premultiplication)
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
    pixel_bytes = PIXEL_BYTES[depth]
    common_complete_bytes = (min(expected_size, actual_size) // pixel_bytes) * pixel_bytes
    smaller_has_partial_pixel = min(expected_size, actual_size) % pixel_bytes != 0
    expected_pixels = (expected_size + pixel_bytes - 1) // pixel_bytes
    actual_pixels = (actual_size + pixel_bytes - 1) // pixel_bytes
    mismatched = min(
        0xFFFFFFFF,
        int(smaller_has_partial_pixel) + abs(expected_pixels - actual_pixels),
    )
    with expected.open("rb") as left, actual.open("rb") as right:
        remaining = common_complete_bytes
        while remaining:
            chunk_size = min(1024 * 1024, remaining)
            left_chunk = _read_exact_comparison_chunk(left, chunk_size, "oracle")
            right_chunk = _read_exact_comparison_chunk(right, chunk_size, "render output")
            for offset in range(0, len(left_chunk), pixel_bytes):
                if left_chunk[offset : offset + pixel_bytes] != right_chunk[offset : offset + pixel_bytes]:
                    mismatched = min(0xFFFFFFFF, mismatched + 1)
            remaining -= len(left_chunk)
    return mismatched


def _read_exact_comparison_chunk(stream: Any, size: int, label: str) -> bytes:
    chunks = []
    remaining = size
    while remaining:
        chunk = stream.read(remaining)
        if not chunk:
            raise ValueError(f"{label} changed size during pixel comparison")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def attach_oracle(
    result: dict[str, Any],
    depth: str,
    oracle_manifest: dict[str, Any] | None,
    bundle_root: Path,
    oracle_state: str,
    manifest_identity_match: bool,
) -> None:
    if oracle_state != "captured":
        result["oracle"] = {
            "state": oracle_state,
            "identity_match": manifest_identity_match,
            "exact": False,
        }
        return
    result["oracle"] = {
        "state": "not_captured",
        "identity_match": manifest_identity_match,
        "exact": False,
    }
    if result["classification"] != "ok" or result["raw_output"] is None:
        return
    if oracle_manifest is None:
        raise ValueError(f"captured oracle is missing for {depth}")
    oracle_path = bundle_root / oracle_manifest["path"]
    actual_path = bundle_root / result["raw_output"]["path"]
    expected_hash = oracle_manifest["sha256"]
    actual_hash = result["raw_output"]["sha256"]
    mismatched = _mismatched_pixels(oracle_path, actual_path, depth)
    result["oracle"] = {
        "state": "captured",
        "identity_match": manifest_identity_match,
        "exact": manifest_identity_match and mismatched == 0,
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
            validate_artifact_destination(item["path"])
            destination_key = item["path"].replace("\\", "/").casefold()
            if any(
                destination_key.startswith(existing + "/")
                or existing.startswith(destination_key + "/")
                for existing in by_path
            ):
                raise ValueError(f"artifact path collides with another artifact: {item['path']}")
            previous = by_path.get(destination_key)
            if previous is not None and previous != item:
                raise ValueError(f"conflicting artifact identity: {item['path']}")
            by_path[destination_key] = item

        stage = "copy_pinned_artifacts"
        for item in by_path.values():
            copy_verified_artifact(source_root, item, output_root / item["path"])
        worker_identities = copy_native_workers(output_root) if args.adapter_command is None else []
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
        observed_parameter_metadata = None
        with Image.open(output_root / manifest["input"]["path"]) as source_image:
            input_width, input_height = source_image.size

        for depth in manifest["requested_depths"]:
            stage = f"render_{depth}"
            request_path = requests / f"{depth}.json"
            write_request_sidecar(manifest, depth, request_path)
            output = outputs / f"{depth}.png"
            dump_dir = output_root / "target" / f"conformance-{manifest['fixture_id']}-{os.getpid()}-{depth}"
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
                manifest["execution"]["render_path"],
            )
            parameter_metadata = result.pop("_parameter_metadata", None)
            if parameter_metadata is not None:
                if observed_parameter_metadata is None:
                    observed_parameter_metadata = parameter_metadata
                elif parameter_metadata != observed_parameter_metadata:
                    raise ValueError("native parameter metadata differs between requested depths")
            attach_raw_artifacts(
                result,
                depth,
                output_root / manifest["input"]["path"],
                output,
                dump_dir,
                output_root,
                manifest["execution"]["premultiplication"],
            )
            if dump_dir.exists():
                shutil.move(str(dump_dir), str(outputs / f"{depth}-worlds"))
            attach_oracle(
                result,
                depth,
                oracle_artifacts.get(depth),
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
                "workers": worker_identities,
            },
            "parameters": observed_parameter_metadata or [],
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
