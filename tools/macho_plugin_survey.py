#!/usr/bin/env python3
"""Survey After Effects macOS `.plugin` bundles for issue #1847.

Reads only Mach-O load commands, symbol tables, and the bundle's resource
fork names through the macOS developer tools (`lipo`, `otool`, `nm`). Prints
aggregate counts only: no plug-in names, paths, or hashes, so the summary can
be shared. Pass one or more directories to scan recursively.
"""

from __future__ import annotations

import argparse
import collections
import json
import plistlib
import subprocess
import sys
from pathlib import Path

LIBRARY_FAMILIES = {
    "libSystem": ("/usr/lib/libSystem",),
    "libc++": ("/usr/lib/libc++", "/usr/lib/libc++abi"),
    "libobjc": ("/usr/lib/libobjc",),
    "CoreFoundation": ("CoreFoundation.framework",),
    "Foundation": ("Foundation.framework",),
    "AppKit/Cocoa": ("AppKit.framework", "Cocoa.framework"),
    "Metal": ("Metal.framework", "MetalKit.framework", "MetalPerformanceShaders"),
    "OpenGL": ("OpenGL.framework",),
    "OpenCL": ("OpenCL.framework",),
    "CoreGraphics/ImageIO": ("CoreGraphics.framework", "ImageIO.framework", "ApplicationServices"),
    "Accelerate": ("Accelerate.framework",),
    "Security/Network": ("Security.framework", "SystemConfiguration", "CFNetwork", "Network.framework"),
    "IOKit": ("IOKit.framework",),
    "@rpath/@loader_path": ("@rpath/", "@loader_path/", "@executable_path/"),
}
SYMBOL_MARKERS = {
    "objc_msgSend": "_objc_msgSend",
    "__cxa_throw": "___cxa_throw",
    "__gxx_personality_v0": "___gxx_personality_v0",
    "_Unwind_Resume": "__Unwind_Resume",
    "pthread_create": "_pthread_create",
    "dispatch_*": "_dispatch_",
    "_Block_copy": "__Block_copy",
    "__tlv_bootstrap": "__tlv_bootstrap",
    "dlopen": "_dlopen",
    "CFBundleGetBundleWithIdentifier": "_CFBundleGetBundleWithIdentifier",
}


def run(*argv: str) -> str:
    completed = subprocess.run(argv, capture_output=True, text=True, check=False)
    return completed.stdout if completed.returncode == 0 else ""


def bundle_executable(bundle: Path) -> Path | None:
    info = bundle / "Contents" / "Info.plist"
    name = None
    try:
        name = plistlib.loads(info.read_bytes()).get("CFBundleExecutable")
    except Exception:
        pass
    macos = bundle / "Contents" / "MacOS"
    if name and (macos / name).is_file():
        return macos / name
    files = [path for path in macos.glob("*") if path.is_file()] if macos.is_dir() else []
    return files[0] if len(files) == 1 else None


def bucket(value: int, edges: tuple[int, ...]) -> str:
    for edge in edges:
        if value <= edge:
            return f"<={edge}"
    return f">{edges[-1]}"


def survey(bundle: Path, counts: dict[str, collections.Counter]) -> None:
    executable = bundle_executable(bundle)
    if executable is None:
        counts["bundle"]["no_executable"] += 1
        return
    archs = tuple(sorted(run("lipo", "-archs", str(executable)).split()))
    counts["arch_set"][" + ".join(archs) or "unknown"] += 1
    if "x86_64" not in archs:
        counts["bundle"]["no_x86_64_slice"] += 1
        return
    counts["bundle"]["has_x86_64_slice"] += 1
    resources = bundle / "Contents" / "Resources"
    rsrc = list(resources.glob("*.rsrc")) if resources.is_dir() else []
    counts["pipl_resource"]["has_.rsrc" if rsrc else "no_.rsrc"] += 1
    load = run("otool", "-arch", "x86_64", "-l", str(executable))
    for command in ("LC_DYLD_CHAINED_FIXUPS", "LC_DYLD_INFO_ONLY", "LC_DYLD_EXPORTS_TRIE"):
        counts["load_command"][f"{command}={'yes' if command in load else 'no'}"] += 1
    header = run("otool", "-arch", "x86_64", "-hv", str(executable)).split("\n")
    for line in header:
        parts = line.split()
        if parts and parts[0] == "MH_MAGIC_64":
            counts["filetype"][parts[4] if len(parts) > 4 else "unknown"] += 1
    has_init = "__mod_init_func" in load or "__init_offsets" in load
    counts["constructors"]["has_initializers" if has_init else "none"] += 1
    counts["tlv"]["has_thread_locals" if "__thread_vars" in load else "none"] += 1
    libraries = [line.strip().split(" (")[0] for line in run("otool", "-arch", "x86_64", "-L", str(executable)).splitlines()[1:]]
    for family, needles in LIBRARY_FAMILIES.items():
        if any(any(needle in library for needle in needles) for library in libraries):
            counts["library_family"][family] += 1
    counts["linked_library_count"][bucket(len(libraries), (3, 6, 10, 20))] += 1
    undefined = run("nm", "-arch", "x86_64", "-u", "-j", str(executable)).split()
    counts["undefined_symbol_count"][bucket(len(undefined), (25, 50, 100, 200, 400))] += 1
    for marker, prefix in SYMBOL_MARKERS.items():
        if any(symbol.startswith(prefix) for symbol in undefined):
            counts["symbol_marker"][marker] += 1
    exported = run("nm", "-arch", "x86_64", "-g", "-U", "-j", str(executable)).split()
    named_entries = sum(1 for symbol in exported if symbol in ("_EffectMain", "_EntryPointFunc", "_PluginMain"))
    counts["entry_export"]["EffectMain/EntryPointFunc/PluginMain" if named_entries else "other_only"] += 1
    counts["exported_symbol_count"][bucket(len(exported), (1, 3, 10, 50))] += 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("roots", nargs="+", type=Path)
    parser.add_argument("--json", action="store_true", help="print JSON instead of text")
    args = parser.parse_args()
    bundles = sorted({path.resolve() for root in args.roots for path in root.rglob("*.plugin") if path.is_dir()})
    counts: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    counts["bundle"]["total"] = len(bundles)
    for bundle in bundles:
        survey(bundle, counts)
    result = {group: dict(sorted(counter.items())) for group, counter in sorted(counts.items())}
    if args.json:
        json.dump(result, sys.stdout, indent=2, ensure_ascii=False)
        print()
    else:
        for group, values in result.items():
            print(f"[{group}]")
            for key, value in values.items():
                print(f"  {key}: {value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
