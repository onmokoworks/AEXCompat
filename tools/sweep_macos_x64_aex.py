#!/usr/bin/env python3
"""Run a SHA-addressed Windows inventory subset through both macOS x64 guests."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import select
import signal
import struct
import subprocess
import tempfile
import time
from collections import Counter
from pathlib import Path
from typing import BinaryIO

from PIL import Image


SCHEMA_VERSION = 1
MAX_MESSAGE_BYTES = 64 * 1024
MAX_ERROR_BYTES = 4096
START_TIMEOUT_SECONDS = 10.0
RENDER_TIMEOUT_SECONDS = 30.0
CLOSE_TIMEOUT_SECONDS = 2.0


class SweepError(RuntimeError):
    pass


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _reject_duplicate_pairs(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise SweepError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json_strict(path: Path) -> dict[str, object]:
    try:
        value = json.loads(
            path.read_text(encoding="utf-8"),
            object_pairs_hook=_reject_duplicate_pairs,
        )
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise SweepError(f"read strict JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise SweepError(f"JSON root must be an object: {path}")
    return value


def inventory_by_sha(inventory: dict[str, object]) -> dict[str, list[dict[str, object]]]:
    if inventory.get("schema_version") != 1:
        raise SweepError("Windows inventory schema_version must be 1")
    entries = inventory.get("entries")
    if not isinstance(entries, list) or not entries:
        raise SweepError("Windows inventory entries must be a nonempty array")
    result: dict[str, list[dict[str, object]]] = {}
    for index, value in enumerate(entries):
        if not isinstance(value, dict):
            raise SweepError(f"inventory entry {index} is not an object")
        sha = value.get("sha256")
        if sha is None:
            continue
        if not isinstance(sha, str) or len(sha) != 64:
            raise SweepError(f"inventory entry {index} has an invalid SHA-256")
        result.setdefault(sha.lower(), []).append(value)
    return result


def map_corpus(
    inventory: dict[str, object], corpus_roots: list[Path]
) -> list[dict[str, object]]:
    by_sha = inventory_by_sha(inventory)
    files: list[Path] = []
    for root in corpus_roots:
        resolved = root.resolve(strict=True)
        if not resolved.is_dir():
            raise SweepError(f"corpus root is not a directory: {resolved}")
        files.extend(path for path in resolved.rglob("*") if path.is_file() and path.suffix.lower() == ".aex")
    files = sorted(set(files), key=lambda path: os.fspath(path).casefold())
    if not files:
        raise SweepError("Mac corpus contains no AEX files")
    seen_sha: dict[str, Path] = {}
    mapped = []
    for path in files:
        sha = sha256_file(path)
        previous = seen_sha.get(sha)
        if previous is not None:
            raise SweepError(f"duplicate Mac corpus SHA: {previous.name} and {path.name}")
        seen_sha[sha] = path
        matches = by_sha.get(sha, [])
        if not matches:
            raise SweepError(f"Mac AEX is absent from Windows inventory: {path.name} ({sha})")
        architectures = sorted({str(match.get("architecture")) for match in matches})
        if architectures != ["x64"]:
            raise SweepError(f"Mac AEX inventory architecture is not exactly x64: {path.name} {architectures}")
        mapped.append(
            {
                "path": path,
                "name": path.name,
                "sha256": sha,
                "windows_match_count": len(matches),
                "windows_root_categories": sorted(
                    {str(match.get("root_category")) for match in matches}
                ),
                "windows_source_categories": sorted(
                    {str(match.get("source_category")) for match in matches}
                ),
            }
        )
    return mapped


def png_to_argb8(path: Path) -> tuple[int, int, bytes]:
    with Image.open(path) as image:
        rgba = image.convert("RGBA")
        width, height = rgba.size
        if width <= 0 or height <= 0 or width > 1920 or height > 1080:
            raise SweepError(f"input PNG dimensions are unsupported: {width}x{height}")
        source = rgba.tobytes()
    argb = bytearray(len(source))
    for offset in range(0, len(source), 4):
        red, green, blue, alpha = source[offset : offset + 4]
        argb[offset : offset + 4] = bytes((alpha, red, green, blue))
    return width, height, bytes(argb)


def write_message(stream: BinaryIO, value: dict[str, object]) -> None:
    payload = json.dumps(value, separators=(",", ":"), ensure_ascii=True).encode("ascii")
    if not payload or len(payload) > MAX_MESSAGE_BYTES:
        raise SweepError("control request exceeds the protocol bound")
    stream.write(struct.pack("<I", len(payload)))
    stream.write(payload)
    stream.flush()


def _read_exact_timeout(stream: BinaryIO, size: int, deadline: float) -> bytes:
    chunks = bytearray()
    descriptor = stream.fileno()
    while len(chunks) < size:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise SweepError("control response timed out")
        readable, _, _ = select.select([descriptor], [], [], remaining)
        if not readable:
            raise SweepError("control response timed out")
        chunk = os.read(descriptor, size - len(chunks))
        if not chunk:
            raise SweepError("worker closed its control stream")
        chunks.extend(chunk)
    return bytes(chunks)


def read_message(stream: BinaryIO, timeout: float) -> dict[str, object]:
    deadline = time.monotonic() + timeout
    length = struct.unpack("<I", _read_exact_timeout(stream, 4, deadline))[0]
    if length == 0 or length > MAX_MESSAGE_BYTES:
        raise SweepError(f"invalid control response length: {length}")
    payload = _read_exact_timeout(stream, length, deadline)
    try:
        value = json.loads(payload, object_pairs_hook=_reject_duplicate_pairs)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise SweepError(f"invalid control response JSON: {error}") from error
    if not isinstance(value, dict):
        raise SweepError("control response root is not an object")
    return value


def require_exact_keys(value: dict[str, object], expected: set[str], label: str) -> None:
    if set(value) != expected:
        raise SweepError(f"{label} keys differ: {sorted(value)}")


def validate_setup(value: object) -> None:
    if not isinstance(value, dict):
        raise SweepError("session setup is not an object")
    require_exact_keys(
        value,
        {
            "schema_version",
            "execution_backend",
            "global_setup_error",
            "params_setup_error",
            "advertised_num_params",
            "out_flags",
            "out_flags2",
            "parameters",
            "suite_requests",
            "unsupported_suite_calls",
            "dropped_unsupported_suite_calls",
        },
        "session setup",
    )
    if (
        value.get("schema_version") != 1
        or not isinstance(value.get("execution_backend"), str)
        or value.get("global_setup_error") != 0
        or value.get("params_setup_error") != 0
        or not isinstance(value.get("parameters"), list)
        or not isinstance(value.get("suite_requests"), list)
        or not isinstance(value.get("unsupported_suite_calls"), list)
        or not isinstance(value.get("dropped_unsupported_suite_calls"), int)
    ):
        raise SweepError(f"invalid session setup: {value}")


def validate_ready(value: dict[str, object], pid: int) -> None:
    require_exact_keys(value, {"v", "type", "worker_pid", "setup"}, "session_ready")
    if (
        value.get("v") != 1
        or value.get("type") != "session_ready"
        or value.get("worker_pid") != pid
        or not isinstance(value.get("setup"), dict)
    ):
        raise SweepError(f"invalid session_ready: {value}")
    validate_setup(value["setup"])


def validate_probe(value: dict[str, object], pid: int) -> None:
    require_exact_keys(
        value,
        {"v", "type", "worker_pid", "status", "guards_intact", "render_error"},
        "session_probed",
    )
    if (
        value.get("v") != 1
        or value.get("type") != "session_probed"
        or value.get("worker_pid") != pid
        or value.get("status") != "ok"
        or value.get("guards_intact") is not True
        or value.get("render_error") != 0
    ):
        raise SweepError(f"resident admission probe failed: {value}")


def validate_close(value: dict[str, object], pid: int, expected_frames: int) -> None:
    require_exact_keys(value, {"v", "type", "worker_pid", "setup", "close"}, "session_closed")
    close = value.get("close")
    if not isinstance(close, dict):
        raise SweepError("session_closed has no close object")
    require_exact_keys(
        close,
        {
            "schema_version",
            "execution_backend",
            "frames_rendered",
            "frame_setdown_error",
            "sequence_setdown_error",
            "global_setdown_error",
            "suite_requests",
            "unsupported_suite_calls",
            "dropped_unsupported_suite_calls",
            "session_clean",
        },
        "session close report",
    )
    if (
        value.get("v") != 1
        or value.get("type") != "session_closed"
        or value.get("worker_pid") != pid
        or close.get("schema_version") != 1
        or close.get("frames_rendered") != expected_frames
        or close.get("frame_setdown_error") != 0
        or close.get("sequence_setdown_error") != 0
        or close.get("global_setdown_error") != 0
        or close.get("session_clean") is not True
    ):
        raise SweepError(f"resident cleanup was not clean: {value}")


def validate_frame(
    value: dict[str, object], width: int, height: int, expected_checksum: str
) -> None:
    require_exact_keys(
        value,
        {"v", "type", "frame_index", "status", "output", "render_error", "generation"},
        "frame_done",
    )
    output = value.get("output")
    if not isinstance(output, dict):
        raise SweepError(f"frame_done has no output: {value}")
    require_exact_keys(
        output,
        {"width", "height", "rowbytes", "pixel_format", "checksum", "guards_intact"},
        "frame output",
    )
    if (
        value.get("v") != 1
        or value.get("type") != "frame_done"
        or value.get("frame_index") != 0
        or value.get("status") != "ok"
        or value.get("render_error") != 0
        or value.get("generation") != 1
        or output.get("width") != width
        or output.get("height") != height
        or output.get("rowbytes") != width * 4
        or output.get("pixel_format") != "argb8"
        or output.get("checksum") != expected_checksum
        or output.get("guards_intact") is not True
    ):
        raise SweepError(f"resident frame invariants failed: {value}")


def spawn_worker(
    worker: Path,
    plugin: Path,
    input_slot: Path,
    output_slot: Path,
    width: int,
    height: int,
) -> subprocess.Popen[bytes]:
    return subprocess.Popen(
        [
            os.fspath(worker),
            "session",
            os.fspath(plugin),
            os.fspath(input_slot),
            os.fspath(output_slot),
            str(width),
            str(height),
            "30",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )


def signal_worker_group(process: subprocess.Popen[bytes], signal_number: int) -> None:
    try:
        os.killpg(process.pid, signal_number)
    except ProcessLookupError:
        pass


def read_stderr_bounded(process: subprocess.Popen[bytes]) -> str:
    if process.stderr is None:
        return ""
    descriptor = process.stderr.fileno()
    chunks = bytearray()
    while len(chunks) <= MAX_ERROR_BYTES:
        readable, _, _ = select.select([descriptor], [], [], 0)
        if not readable:
            break
        chunk = os.read(descriptor, min(4096, MAX_ERROR_BYTES + 1 - len(chunks)))
        if not chunk:
            break
        chunks.extend(chunk)
    return bytes(chunks[:MAX_ERROR_BYTES]).decode("utf-8", errors="replace")


def terminate_worker(process: subprocess.Popen[bytes]) -> str:
    if process.stdin:
        try:
            process.stdin.close()
        except (BrokenPipeError, OSError, ValueError):
            pass
    if process.poll() is None:
        signal_worker_group(process, signal.SIGTERM)
        try:
            process.wait(timeout=CLOSE_TIMEOUT_SECONDS)
        except subprocess.TimeoutExpired:
            signal_worker_group(process, signal.SIGKILL)
            try:
                process.wait(timeout=CLOSE_TIMEOUT_SECONDS)
            except subprocess.TimeoutExpired:
                return "worker process group did not exit after SIGKILL"
    return read_stderr_bounded(process)


def close_worker(process: subprocess.Popen[bytes], expected_frames: int) -> dict[str, object]:
    if process.stdin is None or process.stdout is None:
        raise SweepError("worker control pipes are unavailable")
    write_message(process.stdin, {"v": 1, "type": "close"})
    process.stdin.close()
    response = read_message(process.stdout, CLOSE_TIMEOUT_SECONDS)
    validate_close(response, process.pid, expected_frames)
    try:
        returncode = process.wait(timeout=CLOSE_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired as error:
        cleanup = terminate_worker(process)
        detail = f"; {cleanup}" if cleanup else ""
        raise SweepError(f"worker exceeded close deadline{detail}") from error
    stderr = read_stderr_bounded(process)
    if returncode != 0 or stderr:
        raise SweepError(f"worker close exit={returncode} stderr={stderr}")
    return response


def launch_ready(
    worker: Path,
    plugin: Path,
    input_slot: Path,
    output_slot: Path,
    width: int,
    height: int,
) -> tuple[subprocess.Popen[bytes], dict[str, object]]:
    process = spawn_worker(worker, plugin, input_slot, output_slot, width, height)
    try:
        if process.stdout is None:
            raise SweepError("worker stdout is unavailable")
        ready = read_message(process.stdout, START_TIMEOUT_SECONDS)
        validate_ready(ready, process.pid)
        return process, ready
    except Exception as error:
        stderr = terminate_worker(process)
        if stderr:
            raise SweepError(f"{error}; worker stderr: {stderr}") from error
        raise


def run_backend(
    worker: Path,
    plugin: Path,
    argb8: bytes,
    width: int,
    height: int,
    directory: Path,
) -> dict[str, object]:
    input_slot = directory / "input.argb8"
    output_slot = directory / "output.argb8"
    input_slot.write_bytes(argb8)
    output_slot.write_bytes(bytes(len(argb8)))
    probe, probe_ready = launch_ready(
        worker, plugin, input_slot, output_slot, width, height
    )
    try:
        if probe.stdin is None or probe.stdout is None:
            raise SweepError("probe control pipes are unavailable")
        write_message(probe.stdin, {"v": 1, "type": "probe"})
        validate_probe(read_message(probe.stdout, RENDER_TIMEOUT_SECONDS), probe.pid)
        close_worker(probe, 0)
    except Exception as error:
        stderr = terminate_worker(probe)
        if stderr:
            raise SweepError(f"{error}; worker stderr: {stderr}") from error
        raise

    process, ready = launch_ready(
        worker, plugin, input_slot, output_slot, width, height
    )
    try:
        if process.stdin is None or process.stdout is None:
            raise SweepError("render control pipes are unavailable")
        write_message(
            process.stdin,
            {
                "v": 2,
                "type": "render_frame",
                "frame_index": 0,
                "current_time": {"value": 0, "scale": 30},
                "parameters": "v2|",
            },
        )
        frame = read_message(process.stdout, RENDER_TIMEOUT_SECONDS)
        output = output_slot.read_bytes()
        if len(output) != len(argb8):
            raise SweepError(f"output slot size differs: {len(output)} != {len(argb8)}")
        checksum = hashlib.sha256(output).hexdigest()
        validate_frame(frame, width, height, checksum)
        close = close_worker(process, 1)
    except Exception as error:
        stderr = terminate_worker(process)
        if stderr:
            raise SweepError(f"{error}; worker stderr: {stderr}") from error
        raise
    return {
        "status": "rendered",
        "fresh_after_probe": probe_ready["worker_pid"] != ready["worker_pid"],
        "execution_backend": ready["setup"].get("execution_backend"),
        "output_sha256": checksum,
        "suite_requests": close["close"].get("suite_requests", []),
        "unsupported_suite_calls": close["close"].get("unsupported_suite_calls", []),
        "session_clean": True,
    }


def classify_failure(message: str) -> str:
    lowered = message.casefold()
    for needle, bucket in (
        ("timed out", "timeout"),
        ("cleanup", "cleanup"),
        ("missing import", "import"),
        ("unsupported import", "import"),
        ("dllmain", "DllMain"),
        ("tls", "TLS"),
        ("seh", "SEH"),
        ("thread", "thread"),
        ("suite", "Suite"),
        ("callback", "callback"),
        ("entry", "entrypoint"),
        ("pixel", "pixel-format"),
        ("metal", "platform-service"),
        ("vulkan", "platform-service"),
    ):
        if needle in lowered:
            return bucket
    return "guest-runtime"


def run_sweep(args: argparse.Namespace) -> dict[str, object]:
    inventory_path = args.inventory.resolve(strict=True)
    summary_path = args.windows_summary.resolve(strict=True)
    inventory_sha = sha256_file(inventory_path)
    summary_sha = sha256_file(summary_path)
    if args.expected_inventory_sha256 and inventory_sha != args.expected_inventory_sha256:
        raise SweepError("Windows inventory SHA-256 differs from the expected identity")
    if args.expected_summary_sha256 and summary_sha != args.expected_summary_sha256:
        raise SweepError("Windows summary SHA-256 differs from the expected identity")
    inventory = load_json_strict(inventory_path)
    windows_summary = load_json_strict(summary_path)
    if windows_summary.get("schema_version") != 1:
        raise SweepError("Windows summary schema_version must be 1")
    mapped = map_corpus(inventory, args.corpus_root)
    width, height, argb8 = png_to_argb8(args.input_png.resolve(strict=True))
    workers = {
        "native": args.native_worker.resolve(strict=True),
        "unicorn": args.unicorn_worker.resolve(strict=True),
    }
    for name, worker in workers.items():
        if not worker.is_file():
            raise SweepError(f"{name} worker is not a file: {worker}")
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    run_root = output.parent / f"{output.stem}-runs"
    run_root.mkdir(parents=True, exist_ok=True)
    entries = []
    counts: Counter[str] = Counter()
    for index, item in enumerate(mapped):
        backend_results = {}
        for backend, worker in workers.items():
            directory = run_root / f"{index:04d}-{item['sha256'][:12]}-{backend}"
            directory.mkdir(parents=True, exist_ok=True)
            started = time.monotonic()
            try:
                result = run_backend(
                    worker,
                    item["path"],
                    argb8,
                    width,
                    height,
                    directory,
                )
                result["elapsed_ms"] = round((time.monotonic() - started) * 1000, 3)
                counts[f"{backend}:rendered"] += 1
            except Exception as error:
                message = str(error)[:MAX_ERROR_BYTES]
                bucket = classify_failure(message)
                result = {
                    "status": "failed",
                    "failure_class": bucket,
                    "error": message,
                    "elapsed_ms": round((time.monotonic() - started) * 1000, 3),
                }
                counts[f"{backend}:{bucket}"] += 1
            backend_results[backend] = result
        entries.append(
            {
                key: value
                for key, value in item.items()
                if key != "path"
            }
            | {"backends": backend_results}
        )
    report = {
        "schema_version": SCHEMA_VERSION,
        "mode": "macos_x64_guest_inventory_sweep",
        "source": {
            "windows_inventory_sha256": inventory_sha,
            "windows_summary_sha256": summary_sha,
            "windows_inventory_entries": len(inventory["entries"]),
            "mapped_entries": len(mapped),
            "input_png_sha256": sha256_file(args.input_png),
            "input_dimensions": [width, height],
            "native_worker_sha256": sha256_file(workers["native"]),
            "unicorn_worker_sha256": sha256_file(workers["unicorn"]),
        },
        "summary": {
            "entry_count": len(entries),
            "backend_attempts": len(entries) * len(workers),
            "counts": dict(sorted(counts.items())),
        },
        "entries": entries,
    }
    output.write_text(
        json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return report


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument("--windows-summary", type=Path, required=True)
    parser.add_argument("--corpus-root", type=Path, action="append", required=True)
    parser.add_argument("--input-png", type=Path, required=True)
    parser.add_argument("--native-worker", type=Path, required=True)
    parser.add_argument("--unicorn-worker", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--expected-inventory-sha256")
    parser.add_argument("--expected-summary-sha256")
    return parser.parse_args()


def main() -> int:
    try:
        report = run_sweep(parse_args())
    except SweepError as error:
        print(f"macos_aex_sweep_error: {error}", file=os.sys.stderr)
        return 1
    print(json.dumps(report["summary"], ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
