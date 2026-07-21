#!/usr/bin/env python3
"""List After Effects .aex plug-ins with identity read statically from PiPL.

Given a single ``.aex`` file or a directory (scanned recursively), this prints a
human-readable table of each plug-in's Name, Match Name, Category, Kind, Win64
entrypoint, and version, plus a fail-closed dispatch classification. Identity is
decoded from the real PiPL resource bytes via ``aex_pipl_identity`` without ever
loading the module, calling an entrypoint, starting After Effects, or rendering.

Examples:
    uv run python tools/aex_list.py --input path/to/plugins
    uv run python tools/aex_list.py --input one.aex --out target/aex-pipl-identity/one.json
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

LAB_ROOT = Path(__file__).resolve().parents[1]
TOOLS_ROOT = LAB_ROOT / "tools"
if str(TOOLS_ROOT) not in sys.path:
    sys.path.insert(0, str(TOOLS_ROOT))

import aex_pipl_identity as identity  # noqa: E402  (path shim above is required first)

COLUMNS = [
    ("name", "NAME", 22),
    ("match_name", "MATCH NAME", 22),
    ("category", "CATEGORY", 18),
    ("kind_label", "KIND", 10),
    ("entrypoint_win64", "ENTRYPOINT", 16),
    ("version_display", "VER", 8),
    ("machine_label", "ARCH", 6),
    ("classification", "CLASS", 26),
    ("relative_path", "FILE", 32),
]


def _cell(value: Any, width: int) -> str:
    text = "-" if value is None else str(value)
    if len(text) > width:
        text = text[: width - 1] + "…"
    return text.ljust(width)


def _row_values(entry: dict[str, Any]) -> dict[str, Any]:
    row = dict(entry.get("identity", {}))
    row["machine_label"] = entry.get("machine_label")
    row["classification"] = entry.get("classification")
    row["relative_path"] = entry.get("relative_path")
    return row


def render_table(report: dict[str, Any]) -> str:
    lines: list[str] = []
    header = "  ".join(title.ljust(width) for _, title, width in COLUMNS)
    lines.append(header)
    lines.append("  ".join("-" * width for _, _, width in COLUMNS))
    for entry in report.get("entries", []):
        row = _row_values(entry)
        lines.append("  ".join(_cell(row.get(field), width) for field, _, width in COLUMNS))
    lines.append("")
    lines.append(f"{report.get('aex_count', 0)} AEX; " + ", ".join(
        f"{cls}={count}" for cls, count in sorted(report.get("classification_counts", {}).items())
    ))
    return "\n".join(lines)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="List AEX plug-ins with static PiPL identity")
    parser.add_argument("--input", required=True, help="AEX file or directory to scan")
    parser.add_argument("--out", help="Also write the full JSON report under target/aex-pipl-identity")
    parser.add_argument("--json", action="store_true", help="Print JSON instead of the table")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    report = identity.build_report(Path(args.input))
    if args.out:
        identity.write_json_create_new(Path(args.out), report)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    else:
        print(render_table(report))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
