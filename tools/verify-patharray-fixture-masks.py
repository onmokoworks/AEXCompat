#!/usr/bin/env python3
"""Headless PathArray mask + layer-image regression; uses an already-built plug-in.

Usage: python tools/verify-patharray-fixture-masks.py <harness.exe> <path_array.dll> <new-output-dir>
Requires no third-party Python packages. Keeps fixtures, raw artifacts and measurements.
Records all six cases as passed, failed, or not_applicable from host capability
evidence. Exits zero only if at least one applicable case ran and all passed.
"""

import hashlib
import json
import math
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import zlib

ROOT = Path(__file__).resolve().parent.parent

# PF flags, matching the host's depth negotiation. SmartFX is the AE-selected
# route when advertised; its legacy Render route is not part of this matrix.
SUPPORTS_SMART_RENDER = 1 << 10
DEEP_COLOR_AWARE = 1 << 25
FLOAT_COLOR_AWARE = 1 << 12


class HarnessError(RuntimeError):
    def __init__(self, message, report):
        super().__init__(message)
        self.report = report


def applicability(report, route, depth):
    """Require explicit host flags; never infer non-applicability from an error."""
    evidence = report.get("depth_provenance", report)
    if not isinstance(evidence, dict):
        raise ValueError("host capability evidence is missing")
    flags = evidence.get("advertised_out_flags")
    flags2 = evidence.get("advertised_out_flags2")
    if any(type(value) is not int or not 0 <= value <= 0xffffffff for value in (flags, flags2)):
        raise ValueError("host advertised flags are missing or invalid")
    smart = bool(flags2 & SUPPORTS_SMART_RENDER)
    supported = depth == "argb8" or bool(
        (flags & DEEP_COLOR_AWARE) if depth == "argb16" else (flags2 & FLOAT_COLOR_AWARE))
    reported_support = evidence.get("advertised_depth_supported")
    if reported_support is not None and (type(reported_support) is not bool or reported_support != supported):
        raise ValueError("host depth support contradicts advertised flags")
    description = (f"host advertised_out_flags={flags}, advertised_out_flags2={flags2}; "
                   f"SUPPORTS_SMART_RENDER={smart}; advertised_depth_supported={supported}")
    if route == "classic" and smart:
        return False, description + "; AE selects SmartFX instead of classic Render"
    if route == "smart" and not smart:
        return False, description + "; SmartFX is not declared"
    if not supported:
        return False, description + f"; native {depth} is not declared"
    return True, description


def matrix_passed(results):
    applicable = [result for result in results if result["status"] != "not_applicable"]
    return bool(applicable) and all(result["status"] == "passed" for result in applicable)


def png(path, width, height, pixels):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    rows = b"".join(b"\0" + pixels[y * width * 4:(y + 1) * width * 4] for y in range(height))
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">2I5B", width, height, 8, 6, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def run(harness, args, log):
    # communicate waits for this exact child. On interruption, kill and reap only
    # our harness; its kill-on-close Job Object owns the native worker lifetime.
    with subprocess.Popen([str(harness), "--headless", *map(str, args)], cwd=ROOT,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE) as process:
        try:
            stdout, stderr = process.communicate()
        except BaseException:
            process.kill()
            process.communicate()
            raise
        log.with_suffix(".stderr.txt").write_bytes(stderr)
        log.write_bytes(stdout)
        if process.returncode:
            diagnostic = stderr.decode("utf-8", errors="replace")
            # Render failures retain the worker's report after the diagnostics.
            # Malformed/missing evidence is not permission to skip a case.
            try:
                report, _ = json.JSONDecoder().raw_decode(diagnostic.partition("report=")[2])
            except ValueError:
                report = {}
            reason = diagnostic.split("diagnostics=", 1)[0].strip()
            if "partial_unwritten_output" in diagnostic:
                reason += " partial_unwritten_output"
            raise HarnessError(f"{args[0]} exited {process.returncode}: {reason} (full diagnostic in {log.with_suffix('.stderr.txt').name})", report)
    return json.loads(stdout)


def measure(output, fixture_path, word, white, masked):
    metadata = json.loads((output / "final/output.json").read_text(encoding="utf-8"))
    pixels = list(struct.iter_unpack("<4" + word, (output / "final/output.bin").read_bytes()))
    assert len(pixels) == 96 * 64, "unexpected output geometry"
    assert all(math.isfinite(v) for pixel in pixels for v in pixel), "nonfinite output"
    visible = [pixel for pixel in pixels if pixel[0] > 0]
    if not masked:
        assert not visible, "mask-free control produced visible pixels"
        assert "fixture_case" not in metadata["comparison_identity"]
        return None
    identity = metadata["comparison_identity"]["fixture_case"]
    assert identity["fixture_sha256"] == hashlib.sha256(fixture_path.read_bytes()).hexdigest()
    partial = [pixel for pixel in visible if pixel[0] < white]
    darkening = max((max(0, white - channel) for pixel in visible for channel in pixel[1:]), default=0)
    deviation = max((abs(white - channel) for pixel in visible for channel in pixel[1:]), default=0)
    return {"copied_pixels": len(visible), "partial_alpha_pixels": len(partial),
            "max_rgb_darkening": darkening, "max_rgb_deviation": deviation,
            "straight_white": bool(visible) and deviation == 0,
            "passed": bool(visible and partial) and deviation == 0}


def main():
    if len(sys.argv) != 4:
        raise SystemExit(__doc__)
    harness, plugin = (Path(arg).resolve(strict=True) for arg in sys.argv[1:3])
    scratch = Path(sys.argv[3]).resolve()
    scratch.mkdir(parents=True, exist_ok=False)
    aex = scratch / "PathArray.aex"
    shutil.copyfile(plugin, aex)
    parameters = run(harness, ["--inspect-experimental", aex], scratch / "inspect.json")
    assert isinstance(parameters, list)
    slots = {p["slot"]: p for p in parameters}
    for slot, name in {1: "Source Layer", 2: "Mask Index", 3: "Copies", 9: "Scale (%)",
                       12: "Opacity (%)", 14: "Composite on Original"}.items():
        assert slots[slot]["name"] == name, slots[slot]
    slots[1]["layer_path"] = "source.png"
    for slot, value in {2: 1, 3: 6, 9: 73, 12: 100, 14: 0}.items():
        slots[slot]["value"] = value
    png(scratch / "primary.png", 96, 64, bytes(96 * 64 * 4))
    disc = bytearray()
    for y in range(16):
        for x in range(16):
            radius = math.hypot(x - 7.5, y - 7.5)
            alpha = round(255 * min(1, max(0, (6.5 - radius) / 2)))
            disc.extend((255, 255, 255, alpha) if alpha else (0, 0, 0, 0))
    png(scratch / "source.png", 16, 16, disc)
    scene = {"masks": [{"open": False, "vertices": [
        {"x": 16, "y": 16}, {"x": 80, "y": 16}, {"x": 80, "y": 48}, {"x": 16, "y": 48}]}]}
    results = []
    control_errors = []
    for route in ("classic", "smart"):
        for depth, word, white in (("argb8", "B", 255), ("argb16", "H", 32768), ("argb32f", "f", 1.0)):
            case = {"path": route, "depth": depth}
            probe_error = None
            try:
                try:
                    report = run(harness, ["--render-experimental-session", aex,
                                 scratch / "primary.png", scratch / f"probe-{route}-{depth}.png",
                                 depth, route, 0, 1, 1], scratch / f"probe-{route}-{depth}.json")
                except HarnessError as error:
                    report, probe_error = error.report, str(error)
                applicable, evidence = applicability(report, route, depth)
                case["evidence"] = evidence
                if not applicable:
                    result = {**case, "status": "not_applicable"}
                    results.append(result)
                    print(json.dumps(result), flush=True)
                    continue
                if probe_error:
                    raise RuntimeError(probe_error)
            except (RuntimeError, ValueError, AttributeError, OSError) as error:
                result = {**case, "status": "failed", "capability_probe_error": str(error)}
                results.append(result)
                print(json.dumps(result), flush=True)
                continue
            case_control_errors = []
            fixture = {"schema": "aexcompat.render_fixture", "schema_version": 1,
                       "primary_layer": "primary.png", "parameters": parameters,
                       "pixel_format": depth, "render_path": route, "premultiplication": "straight",
                       "timing": {"current_time": 0, "time_step": 1, "total_time": 1, "time_scale": 1},
                       "final_artifact": "raw", "checkpoints": [{"id": "output", "stage": f"{route}-output"}]}
            for masked in (False, True):
                name = f"{route}-{depth}-{'masks' if masked else 'absent'}"
                if masked:
                    fixture["mask_scene"] = scene
                fixture_path = scratch / f"{name}.json"
                fixture_path.write_text(json.dumps(fixture), encoding="utf-8")
                output = scratch / name
                try:
                    run(harness, ["--render-fixture", aex, fixture_path, output], scratch / f"{name}.report.json")
                    measured = measure(output, fixture_path, word, white, masked)
                except RuntimeError as error:
                    if output.exists():
                        case_control_errors.append({"case": name, "validation_error": "failed render published partial output"})
                    if masked:
                        result = {**case, "status": "failed", "copied_pixels": None,
                                  "max_rgb_darkening": None, "straight_white": False,
                                  "passed": False, "render_error": str(error)}
                        results.append(result)
                        print(json.dumps(result), flush=True)
                    else:
                        case_control_errors.append({"case": name, "render_error": str(error)})
                    continue
                except (AssertionError, ValueError, KeyError, OSError, struct.error) as error:
                    result = {**case, "status": "failed", "copied_pixels": None,
                              "max_rgb_darkening": None, "straight_white": False,
                              "passed": False, "validation_error": str(error)}
                    (results if masked else case_control_errors).append(result)
                    print(json.dumps(result), flush=True)
                    continue
                if not masked:
                    continue
                result = {**case, **measured,
                          "status": "passed" if measured["passed"] and not case_control_errors else "failed"}
                if case_control_errors:
                    result["control_errors"] = case_control_errors
                    result["passed"] = False
                results.append(result)
                print(json.dumps(result), flush=True)
            control_errors.extend(case_control_errors)
    (scratch / "summary.json").write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    (scratch / "control-errors.json").write_text(json.dumps(control_errors, indent=2) + "\n", encoding="utf-8")
    # Retain all six measurements even when the external plug-in fails whiteness.
    if not matrix_passed(results):
        raise SystemExit("PathArray render or straight-white check failed; see summary.json")


if __name__ == "__main__":
    main()
