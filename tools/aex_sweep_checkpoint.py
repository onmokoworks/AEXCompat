#!/usr/bin/env python3
"""Join a static AEX inventory with one discover_sweep worker report.

The worker report intentionally contains only scan-root-relative paths.  This
tool resolves those paths against the explicitly supplied scan root, rechecks
the SHA, and writes a local checkpoint without using a basename as identity.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from inventory_installed_aex import RootSpec, canonical, inspect_file  # noqa: E402


def classify_failure(bucket: str) -> str | None:
    if bucket == "loaded":
        return None
    if bucket == "exit_12_aegp_candidate":
        return "aegp_candidate"
    if bucket == "exit_12_unknown_no_effect_entrypoint":
        return "entrypoint/ABI"
    if bucket.startswith("module_audit"):
        return "module_audit"
    if bucket == "exit_20":
        return "selector/ABI"
    if bucket.startswith("timeout"):
        return "timeout"
    return bucket


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--file-list", type=Path, required=True)
    parser.add_argument("--worker-report", type=Path, required=True)
    parser.add_argument("--scan-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    inventory = json.loads(args.file_list.read_text(encoding="utf-8"))
    worker = json.loads(args.worker_report.read_text(encoding="utf-8"))
    records_by_path = {str(canonical(Path(item["canonical_path"]))).casefold(): item for item in inventory["entries"]}
    roots = {str(item["path"]).casefold(): RootSpec(Path(item["path"]), item["category"], set(item.get("discovery", []))) for item in inventory["roots"]}
    entries = []
    for result in worker.get("plugins", []):
        relative = Path(result["plugin_relative_path"])
        path = canonical(args.scan_root / relative)
        static_record = records_by_path.get(str(path).casefold())
        if static_record is None:
            raise SystemExit(f"worker path absent from inventory: {path}")
        root = roots.get(str(static_record["root"]).casefold(), RootSpec(Path(static_record["root"]), static_record["root_category"], set(static_record.get("root_discovery", []))))
        static = inspect_file(path, root, str(static_record.get("relative_to_root", relative)))
        bucket = str(result.get("bucket", "unknown"))
        worker_sha = result.get("plugin_sha256")
        entry = {
            "canonical_path": str(path),
            "relative_path": str(relative),
            "root_category": static["root_category"],
            "sha256": static.get("sha256"),
            "worker_sha256": worker_sha,
            "sha_match": static.get("sha256") == worker_sha,
            "architecture": static.get("architecture"),
            "valid_pe": static.get("valid_pe"),
            "pipl_present": static.get("pipl_present", False),
            "entrypoint_symbol_hints": static.get("entrypoint_symbol_hints", []),
            "static_parsed": bool(static.get("valid_pe")),
            "worker_attempted": True,
            "worker_bucket": bucket,
            "entrypoint_success": bucket == "loaded",
            "descriptor_success": bucket == "loaded",
            "minimal_render_attempted": False,
            "minimal_render_success": False,
            "render_status": "not_attempted_discovery_only",
            "failure_class": classify_failure(bucket),
            "worker_elapsed_ms": result.get("elapsed_ms"),
            "worker_error": result.get("error"),
        }
        entries.append(entry)

    failures = Counter(entry["failure_class"] for entry in entries if entry["failure_class"])
    summary = {
        "discovered": len(entries),
        "hashed": sum(bool(entry["sha256"]) for entry in entries),
        "static_parsed": sum(entry["static_parsed"] for entry in entries),
        "worker_attempted": sum(entry["worker_attempted"] for entry in entries),
        "entrypoint_success": sum(entry["entrypoint_success"] for entry in entries),
        "descriptor_success": sum(entry["descriptor_success"] for entry in entries),
        "minimal_render_attempted": sum(entry["minimal_render_attempted"] for entry in entries),
        "minimal_render_success": sum(entry["minimal_render_success"] for entry in entries),
        "sha_mismatch": sum(not entry["sha_match"] for entry in entries),
        "failure_class_counts": dict(sorted(failures.items())),
    }
    output = args.output
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({
        "schema_version": 1,
        "mode": "static_plus_isolated_worker_checkpoint",
        "scan_root": str(canonical(args.scan_root)),
        "summary": summary,
        "entries": entries,
    }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(output), "summary": summary}, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
