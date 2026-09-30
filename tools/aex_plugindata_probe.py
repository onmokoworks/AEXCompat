#!/usr/bin/env python3
"""Live PluginData registration probe for real .aex plug-ins (issue #326).

Many bundled AE effects carry no PiPL resource and register through the
``PluginDataEntryFunction`` ABI instead. The worker's fail-closed validation
of that registration (``record_plugin_data_registration`` in
``minihost/src/l2_main.cpp``) is what currently decides whether such a
plug-in gets an Effect dispatch at all, so reproducing what a real plug-in
actually registers is the ground truth for that decision.

This probe loads one real .aex with a bounded DLL search scope (the plug-in's
own folder plus, when present, the ancestor ``Support Files`` folder, mirroring
the sealed dependency roots the broker uses), calls its
``PluginDataEntryFunction3`` / ``PluginDataEntryFunction2`` /
``PluginDataEntryFunction`` (preferred in that order, the same order the
worker resolves) exactly once, and records every registration callback as
JSON. It never calls the effect entrypoint, never renders, and never starts
After Effects.

Safety notes:

- The call is third-party code executing in-process. Batch mode therefore
  isolates every plug-in in a child process (``--child``) so a crash inside
  one plug-in cannot take down the sweep, exactly like the broker's isolated
  workers do.
- ``worker_verdict`` only *mirrors* the worker's current validation rules for
  comparison; it does not change them. Keep it in sync with
  ``record_plugin_data_registration`` in ``minihost/src/l2_main_support.inc``:
  ``kPluginDataApiMajor`` / ``kPluginDataApiMinor`` (13.29), reserved_info
  unvalidated, and multi-registration accepted first-wins (issue #326).
"""

from __future__ import annotations

import argparse
import ctypes
import json
import subprocess
import sys
from ctypes import wintypes
from pathlib import Path
from typing import Any

# Mirror of the worker's validation constants (minihost/src/l2_main.cpp). The
# probe reports what real plug-ins register *against* these rules; it does not
# relax them.
WORKER_PLUGIN_DATA_API_MAJOR = 13
# The bundled AE effects register api 13.29 (Adobe builds against a newer
# internal SDK than the public 25.2 headers); the worker's ceiling moved to
# match (l2_main_support.inc, issue #326).
WORKER_PLUGIN_DATA_API_MINOR = 29
WORKER_HOST_NAME = b"AEXCompat"
WORKER_HOST_VERSION = b"2025"

LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR = 0x00000100
LOAD_LIBRARY_SEARCH_DEFAULT_DIRS = 0x00001000

_CB1 = ctypes.CFUNCTYPE(
    ctypes.c_int32,
    ctypes.c_void_p,
    ctypes.c_char_p,
    ctypes.c_char_p,
    ctypes.c_char_p,
    ctypes.c_char_p,
    ctypes.c_int32,
    ctypes.c_int32,
    ctypes.c_int32,
    ctypes.c_int32,
)
_CB2 = ctypes.CFUNCTYPE(
    ctypes.c_int32,
    ctypes.c_void_p,
    ctypes.c_char_p,
    ctypes.c_char_p,
    ctypes.c_char_p,
    ctypes.c_char_p,
    ctypes.c_int32,
    ctypes.c_int32,
    ctypes.c_int32,
    ctypes.c_int32,
    ctypes.c_char_p,
)
# void* for the callback slot keeps the entry prototype identical for v1/v2;
# the concrete callback object is cast at the call site.
_ENTRY = ctypes.CFUNCTYPE(
    ctypes.c_int32,
    ctypes.c_void_p,
    ctypes.c_void_p,
    ctypes.c_void_p,
    ctypes.c_char_p,
    ctypes.c_char_p,
)


def _decode(raw: bytes | None) -> str | None:
    if raw is None:
        return None
    return raw.decode("utf-8", "replace")


def _kind_code(kind: int) -> str:
    # The callback receives an A_long OSType in big-endian character order
    # ('eFKT' == 0x65464B54), the same orientation as the SDK compiler literal.
    return bytes(
        ((kind >> 24) & 0xFF, (kind >> 16) & 0xFF, (kind >> 8) & 0xFF, kind & 0xFF)
    ).decode("ascii", "replace")


def _registration(
    name: bytes | None,
    match_name: bytes | None,
    category: bytes | None,
    entrypoint: bytes | None,
    kind: int,
    api_major: int,
    api_minor: int,
    reserved_info: int,
    support_url: bytes | None = None,
) -> dict[str, Any]:
    return {
        "name": _decode(name),
        "match_name": _decode(match_name),
        "category": _decode(category),
        "entrypoint": _decode(entrypoint),
        "kind_code": _kind_code(kind),
        "api_major": api_major,
        "api_minor": api_minor,
        "reserved_info": reserved_info,
        "support_url": _decode(support_url),
    }


def probe_plugin_data(path: Path) -> dict[str, Any]:
    """Load one .aex and capture its PluginData registration callbacks.

    The result always has a ``status``: ``no_export`` when neither entrypoint
    variant exists, ``entry_error`` when the call itself returned non-zero, or
    ``called`` otherwise. ``registrations`` holds every callback the plug-in
    made, in order.
    """
    result: dict[str, Any] = {
        "plugin": path.name,
        "path": str(path),
        "status": "called",
        "export_variant": None,
        "entry_result": None,
        "entrypoint_exported": None,
        "registrations": [],
    }
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.AddDllDirectory.argtypes = [wintypes.LPCWSTR]
    kernel32.AddDllDirectory.restype = wintypes.LPVOID
    kernel32.LoadLibraryExW.argtypes = [
        wintypes.LPCWSTR,
        wintypes.HANDLE,
        wintypes.DWORD,
    ]
    kernel32.LoadLibraryExW.restype = wintypes.HMODULE
    kernel32.GetProcAddress.argtypes = [wintypes.HMODULE, ctypes.c_char_p]
    kernel32.GetProcAddress.restype = ctypes.c_void_p
    kernel32.FreeLibrary.argtypes = [wintypes.HMODULE]

    kernel32.AddDllDirectory(str(path.parent))
    for ancestor in path.parents:
        if ancestor.name.lower() == "support files":
            kernel32.AddDllDirectory(str(ancestor))
            break

    module = kernel32.LoadLibraryExW(
        str(path),
        None,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
    )
    if not module:
        result["status"] = "load_error"
        result["load_error"] = ctypes.get_last_error()
        return result
    try:
        # PluginDataEntryFunction3 is the entry the AE 2026 bundle ships (VR*,
        # Fast_Blur, Sharpen); it is not in the public 25.2 header. Probed with
        # the v2 callback signature to observe whether the registration values
        # come through intact or are shifted - the ground truth for whether the
        # worker can call it the same way (issue #326). v3 is preferred over v2
        # over v1 the way the worker resolves them.
        entry_addr = kernel32.GetProcAddress(module, b"PluginDataEntryFunction3")
        variant = "v3"
        if not entry_addr:
            entry_addr = kernel32.GetProcAddress(module, b"PluginDataEntryFunction2")
            variant = "v2"
        if not entry_addr:
            entry_addr = kernel32.GetProcAddress(module, b"PluginDataEntryFunction")
            variant = "v1"
        if not entry_addr:
            result["status"] = "no_export"
            return result
        result["export_variant"] = variant

        registrations: list[dict[str, Any]] = result["registrations"]
        if variant in ("v2", "v3"):

            @_CB2
            def callback(_ptr, name, match, category, entry, kind, major, minor, reserved, url):
                registrations.append(
                    _registration(name, match, category, entry, kind, major, minor, reserved, url)
                )
                return 0

        else:

            @_CB1
            def callback(_ptr, name, match, category, entry, kind, major, minor, reserved):
                registrations.append(
                    _registration(name, match, category, entry, kind, major, minor, reserved)
                )
                return 0

        entry = _ENTRY(entry_addr)
        returned = entry(
            None,
            ctypes.cast(callback, ctypes.c_void_p),
            None,
            WORKER_HOST_NAME,
            WORKER_HOST_VERSION,
        )
        result["entry_result"] = returned
        if returned != 0:
            result["status"] = "entry_error"
            return result
        # The worker's next step after accepting a registration is
        # GetProcAddress on the FIRST registered symbol (Threshold.aex
        # registers "MainEntry" but exports nothing by that name). Multi-effect
        # bundles register more than once and the worker discovers them as the
        # first, so score the first here too (issue #326).
        if registrations and registrations[0]["entrypoint"]:
            symbol = registrations[0]["entrypoint"].encode("ascii", "ignore")
            result["entrypoint_exported"] = bool(kernel32.GetProcAddress(module, symbol))
        return result
    finally:
        kernel32.FreeLibrary(module)


def _valid_export_name(text: str | None) -> bool:
    # Mirror of valid_plugin_data_export_name in minihost/src/l2_main.cpp.
    if not text or len(text) > 127:
        return False
    if not (text[0] == "_" or text[0].isalpha()):
        return False
    return all(ch == "_" or ch.isalnum() for ch in text[1:])


def _valid_text(text: str | None, required: bool) -> bool:
    # Mirror of valid_plugin_data_text: present, printable, and non-empty when
    # required. Termination/readability are C-side concerns; anything ctypes
    # decoded already survived those.
    if text is None:
        return False
    if required and not text:
        return False
    return all(ch.isprintable() for ch in text)


def worker_verdict(result: dict[str, Any]) -> dict[str, Any]:
    """Score a probe result against the worker's current fail-closed rules.

    Returns ``accepted`` plus the list of rule names that would reject the
    registration, so a sweep can attribute every failure to a specific rule
    instead of a single opaque bucket.
    """
    reasons: list[str] = []
    if result["status"] == "load_error":
        return {"accepted": False, "reasons": ["load_error"]}
    if result["status"] == "no_export":
        return {"accepted": False, "reasons": ["no_export"]}
    if result["status"] == "entry_error":
        return {"accepted": False, "reasons": ["entry_error"]}
    registrations = result["registrations"]
    # Multi-effect bundles (Fast_Blur registers two) are accepted first-wins by
    # the worker (record_plugin_data_registration keeps the first and accepts
    # the rest), so the verdict scores the FIRST registration, not a
    # single-callback requirement (issue #326).
    if not registrations:
        reasons.append("no_registration")
    else:
        reg = registrations[0]
        if not (
            _valid_text(reg["name"], True)
            and _valid_text(reg["match_name"], True)
            and _valid_text(reg["category"], True)
            and _valid_export_name(reg["entrypoint"])
        ):
            reasons.append("registration_text")
        if reg["kind_code"] != "eFKT":
            reasons.append("kind")
        major, minor = reg["api_major"], reg["api_minor"]
        if (
            major <= 0
            or major > WORKER_PLUGIN_DATA_API_MAJOR
            or minor < 0
            or (major == WORKER_PLUGIN_DATA_API_MAJOR and minor > WORKER_PLUGIN_DATA_API_MINOR)
        ):
            reasons.append("api_version")
        # reserved_info is deliberately NOT scored: the worker stopped
        # validating it (l2_main.cpp / l2_main_support.inc) because in the wild
        # it is a plugin-defined opaque value (0/1/8/9 observed), not the SDK
        # sample's constant (issue #326).
    if result.get("entrypoint_exported") is False:
        reasons.append("entrypoint_not_exported")
    return {"accepted": not reasons, "reasons": reasons}


def _sweep_plugin_names(sweep_path: Path) -> set[str]:
    document = json.loads(sweep_path.read_text(encoding="utf-8"))
    return {row["plugin"] for row in document.get("plugins", [])}


def _scan_plugins(scan_dir: Path) -> list[Path]:
    return sorted(scan_dir.rglob("*.aex"))


def run_batch(paths: list[Path], scan_dir: Path, limit: int | None) -> dict[str, Any]:
    """Probe every plug-in in a child process and aggregate worker verdicts.

    Iterates concrete paths rather than basenames: two plug-ins can share a
    basename (``Effects/Threshold.aex`` and ``Effects/CycoreFXHD/Threshold.aex``
    are different binaries), and a name-keyed map would silently probe only one
    of them.
    """
    rows = []
    reason_counts: dict[str, int] = {}
    status_counts: dict[str, int] = {}
    for path in paths[: limit or None]:
        name = path.name
        try:
            completed = subprocess.run(
                [sys.executable, str(Path(__file__).resolve()), "--child", str(path)],
                capture_output=True,
                text=True,
                timeout=120,
            )
        except subprocess.TimeoutExpired:
            row = {"plugin": name, "status": "child_timeout"}
            verdict = {"accepted": False, "reasons": ["child_timeout"]}
        else:
            if completed.returncode != 0:
                row = {"plugin": name, "status": "child_crashed"}
                verdict = {"accepted": False, "reasons": ["child_crashed"]}
            else:
                probed = json.loads(completed.stdout)
                verdict = worker_verdict(probed)
                row = {**probed, "verdict": verdict}
        rows.append(row)
        status_counts[row["status"]] = status_counts.get(row["status"], 0) + 1
        for reason in verdict["reasons"]:
            reason_counts[reason] = reason_counts.get(reason, 0) + 1
    return {
        "scan_dir": str(scan_dir),
        "total": len(rows),
        "status_counts": dict(sorted(status_counts.items())),
        "worker_reject_reasons": dict(sorted(reason_counts.items(), key=lambda kv: -kv[1])),
        "worker_accepted": sum(1 for row in rows if row.get("verdict", {}).get("accepted")),
        "plugins": rows,
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Probe a real .aex's PluginData registration (read-only)."
    )
    parser.add_argument("input", nargs="?", help=".aex file to probe")
    parser.add_argument("--child", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--batch", metavar="DIR", help="probe every .aex under DIR recursively")
    parser.add_argument(
        "--sweep",
        metavar="JSON",
        help="restrict --batch to the plug-in names in a discover_sweep JSON",
    )
    parser.add_argument("--limit", type=int, default=None, help="stop after N plug-ins")
    parser.add_argument("--out", metavar="JSON", help="write the batch report to a file")
    args = parser.parse_args()

    if args.child:
        if not args.input:
            parser.error("--child needs an .aex path")
        print(json.dumps(probe_plugin_data(Path(args.input)), ensure_ascii=False))
        return 0

    if args.batch or args.sweep:
        if not args.batch:
            parser.error("--sweep needs --batch to locate the plug-ins")
        scan_dir = Path(args.batch).resolve()
        paths = _scan_plugins(scan_dir)
        if args.sweep:
            # The sweep JSON keys on basenames, which can collide; keep every
            # colliding path so each real binary is probed exactly once.
            names = _sweep_plugin_names(Path(args.sweep))
            paths = [path for path in paths if path.name in names]
        report = run_batch(paths, scan_dir, args.limit)
        text = json.dumps(report, indent=2, ensure_ascii=False)
        if args.out:
            Path(args.out).write_text(text + "\n", encoding="utf-8")
        print(
            json.dumps(
                {key: report[key] for key in ("total", "status_counts", "worker_reject_reasons", "worker_accepted")},
                indent=2,
                ensure_ascii=False,
            )
        )
        return 0

    if not args.input:
        parser.error("give an .aex path, or --batch/--sweep")
    print(json.dumps(probe_plugin_data(Path(args.input).resolve()), indent=2, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
