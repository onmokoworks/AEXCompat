#!/usr/bin/env python3
"""Survey After Effects macOS `.plugin` bundles for issue #1847.

Reads Mach-O headers, load commands and symbol tables through the macOS
developer tools (`lipo`, `otool`, `nm`), and looks for a `PiPL` resource in the
bundle's `.rsrc` files or the executable's resource fork. Prints aggregate
counts only: no plug-in names, paths, or hashes, so the summary can be shared.
`--detail` adds counts of `@rpath` library leaf names and of exported
entry-like symbol names. Those are vendor-chosen names, so keep `--detail`
output private for third-party plug-ins and share only the plain summary. Pass one or more directories to scan recursively.
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import re
import plistlib
import subprocess
import sys
from pathlib import Path

LIBRARY_FAMILIES = {
    "libSystem": ("/usr/lib/libSystem",),
    "libc++": ("/usr/lib/libc++", "/usr/lib/libc++abi"),
    "libobjc": ("/usr/lib/libobjc",),
    "CoreFoundation": ("/CoreFoundation.framework/",),
    "Foundation": ("/Foundation.framework/",),
    "AppKit/Cocoa": ("/AppKit.framework/", "/Cocoa.framework/"),
    "Metal": ("/Metal.framework/", "/MetalKit.framework/", "/MetalPerformanceShaders.framework/"),
    "OpenGL": ("/OpenGL.framework/",),
    "OpenCL": ("/OpenCL.framework/",),
    "CoreGraphics/ImageIO": (
        "/CoreGraphics.framework/",
        "/ImageIO.framework/",
        "/ApplicationServices.framework/",
    ),
    "Accelerate": ("/Accelerate.framework/",),
    "Security/Network": (
        "/Security.framework/",
        "/SystemConfiguration.framework/",
        "/CFNetwork.framework/",
        "/Network.framework/",
    ),
    "IOKit": ("/IOKit.framework/",),
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


# C symbols only: mangled C++ names (`__Z…`) would spell out vendor classes.
ENTRY_LIKE = re.compile(r"^_[A-Za-z]\w*(Main|EntryPoint)\w*$")
FILETYPES = {"BUNDLE", "DYLIB", "EXECUTE", "OBJECT", "DYLINKER", "KEXTBUNDLE"}


class ToolError(Exception):
    pass


def run(*argv: str) -> str:
    completed = subprocess.run(argv, capture_output=True, text=True, check=False)
    if completed.returncode != 0:
        raise ToolError(argv[0])
    return completed.stdout


def has_pipl(bundle: Path, executable: Path) -> str:
    resources = bundle / "Contents" / "Resources"
    rsrc = sorted(resources.glob("*.rsrc")) if resources.is_dir() else []
    for path in rsrc:
        if b"PiPL" in path.read_bytes():
            return "PiPL_in_.rsrc_file"
    fork = Path(str(executable) + "/..namedfork/rsrc")
    try:
        if b"PiPL" in fork.read_bytes():
            return "PiPL_in_resource_fork"
    except OSError:
        pass
    return ".rsrc_without_PiPL" if rsrc else "no_PiPL_found"


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


def survey(bundle: Path, counts: dict[str, collections.Counter], detail: bool) -> None:
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
    counts["pipl"][has_pipl(bundle, executable)] += 1
    load = run("otool", "-arch", "x86_64", "-l", str(executable))
    for command in ("LC_DYLD_CHAINED_FIXUPS", "LC_DYLD_INFO_ONLY", "LC_DYLD_EXPORTS_TRIE"):
        counts["load_command"][f"{command}={'yes' if command in load else 'no'}"] += 1
    header = run("otool", "-arch", "x86_64", "-hv", str(executable)).split()
    filetype = next((token for token in header if token in FILETYPES), "unknown")
    counts["filetype"][filetype] += 1
    has_init = "__mod_init_func" in load or "__init_offsets" in load
    counts["constructors"]["has_initializers" if has_init else "none"] += 1
    counts["tlv"]["has_thread_locals" if "__thread_vars" in load else "none"] += 1
    libraries = [
        line.strip().split(" (")[0]
        for line in run("otool", "-arch", "x86_64", "-L", str(executable)).splitlines()[1:]
    ]
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
    named_entries = sum(
        1 for symbol in exported if symbol in ("_EffectMain", "_EntryPointFunc", "_PluginMain")
    )
    counts["entry_export"]["EffectMain/EntryPointFunc/PluginMain" if named_entries else "other_only"] += 1
    counts["exported_symbol_count"][bucket(len(exported), (1, 3, 10, 50))] += 1
    if detail:
        for library in libraries:
            if library.startswith(("@rpath/", "@loader_path/", "@executable_path/")):
                counts["detail_rpath_leaf"][library.rsplit("/", 1)[-1]] += 1
        for symbol in exported:
            if ENTRY_LIKE.match(symbol):
                counts["detail_entry_like_export"][symbol] += 1


def find_bundles(roots: list[Path], counts: dict[str, collections.Counter]) -> list[Path]:
    bundles = set()
    for root in roots:
        def unreadable(error: OSError) -> None:
            counts["scan"]["unreadable_folder"] += 1

        for folder, subfolders, _files in os.walk(root, onerror=unreadable):
            for name in list(subfolders):
                if name.endswith(".plugin"):
                    bundles.add(Path(folder, name).resolve())
                    subfolders.remove(name)
    return sorted(bundles)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("roots", nargs="+", type=Path)
    parser.add_argument("--json", action="store_true", help="print JSON instead of text")
    parser.add_argument(
        "--detail",
        action="store_true",
        help=(
            "also count @rpath library leaf names and entry-like exported symbol names; "
            "these can identify third-party plug-ins, so do not share them"
        ),
    )
    args = parser.parse_args()
    counts: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    bundles = find_bundles(args.roots, counts)
    counts["bundle"]["total"] = len(bundles)
    for bundle in bundles:
        try:
            survey(bundle, counts, args.detail)
        except ToolError as error:
            counts["tool_error"][str(error)] += 1
        except OSError:
            counts["tool_error"]["read"] += 1
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
