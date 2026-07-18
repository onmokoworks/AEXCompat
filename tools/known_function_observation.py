#!/usr/bin/env python3
"""Resolve known-function hook specs into Frida read plans and format traces.

This is reverse-engineering / observation tooling. It turns a declarative hook
set (known module-relative RVAs plus the struct fields to expand) into a
concrete read plan that a thin Frida script executes, and it formats the
resulting ``send`` messages into ``known_function_invoke`` trace events.

The output carries no integrity or provenance: it uses ``host_kind =
native_observation`` and must never be promoted into After Effects equivalence
evidence. See ``docs/KNOWN_FUNCTION_OBSERVATION_2026-07-19.md``.

Nothing here imports ``frida``; the resolution and formatting logic is
machine-portable and unit tested without Frida, the SDK, or a worker build.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

try:
    from tools.trace_contract_validator import validate_event
except ModuleNotFoundError:  # invoked as a script from tools/
    from trace_contract_validator import validate_event


MODULE_RVA = re.compile(r"^0x[0-9a-f]+$")
# Module-relative offsets are bounded by the plug-in image size (well under
# 4 GiB); a 64-bit absolute address lowercases to the same shape but is larger.
MODULE_RVA_LIMIT = 0x1_0000_0000
# Interpretation -> byte widths the thin Frida reader can honour.
INT_SIZES = {1, 2, 4, 8}
FLOAT_SIZES = {4, 8}
BOOL_SIZES = {1, 2, 4, 8}
REGISTER_WIDTHS = {4, 8}
# Bound the declarable argument slot. Win64 passes the first 4 integer/pointer
# args in registers and the rest on the stack; Frida's args[] exposes both, but
# we cap the index so a spec cannot name an implausible slot and read garbage.
MAX_ARG_SLOT = 32
# A struct extent (bytes) is bounded well under any pointer magnitude; it comes
# from the abi-layout-probe *_size fields.
MAX_STRUCT_EXTENT = 0x10_0000
PHASES = ("enter", "leave")


class ResolutionError(ValueError):
    """A hook spec could not be resolved against an offset map."""


def _rva_to_int(rva: Any) -> int:
    if not isinstance(rva, str) or not MODULE_RVA.match(rva):
        raise ResolutionError(f"module_rva must match ^0x[0-9a-f]+$, got {rva!r}")
    value = int(rva, 16)
    if value >= MODULE_RVA_LIMIT:
        raise ResolutionError(f"module_rva {rva} exceeds the module-relative bound (looks absolute)")
    return value


def _check_interpret(interpret: str, size: int, where: str) -> None:
    if interpret in {"int", "uint"} and size not in INT_SIZES:
        raise ResolutionError(f"{where}: {interpret} needs size in {sorted(INT_SIZES)}, got {size}")
    if interpret == "float" and size not in FLOAT_SIZES:
        raise ResolutionError(f"{where}: float needs size in {sorted(FLOAT_SIZES)}, got {size}")
    if interpret == "bool" and size not in BOOL_SIZES:
        raise ResolutionError(f"{where}: bool needs size in {sorted(BOOL_SIZES)}, got {size}")


def load_offset_map(source: Any) -> dict[str, dict[str, int]]:
    """Return the ``fields`` table from an abi-layout-probe offset map.

    ``source`` may be a path, a parsed offset-map dict, or the fields table
    itself. Each entry maps a dotted field name to ``{"offset", "size"}``.
    """

    if isinstance(source, (str, Path)):
        data = json.loads(Path(source).read_text(encoding="utf-8"))
    else:
        data = source
    if isinstance(data, dict) and "fields" in data:
        fields = data["fields"]
    else:
        fields = data
    if not isinstance(fields, dict):
        raise ResolutionError("offset map must expose a 'fields' object")
    resolved: dict[str, dict[str, int]] = {}
    for name, entry in fields.items():
        if not isinstance(entry, dict) or "offset" not in entry or "size" not in entry:
            raise ResolutionError(f"offset map field {name!r} must have offset and size")
        offset, size = entry["offset"], entry["size"]
        if not _is_plain_int(offset) or offset < 0:
            raise ResolutionError(f"offset map field {name!r} has a non-negative-integer offset requirement")
        if not _is_plain_int(size) or size <= 0:
            raise ResolutionError(f"offset map field {name!r} has a positive-integer size requirement")
        resolved[name] = {"offset": int(offset), "size": int(size)}
    return resolved


def _is_plain_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def resolve_hook(hook: dict[str, Any], offset_map: dict[str, dict[str, int]]) -> dict[str, Any]:
    """Resolve one hook into a per-phase read plan.

    Raises :class:`ResolutionError` with an explicit message on any gap so a
    missing field or an XMM-only float argument fails loudly rather than
    silently producing nothing.
    """

    symbol = hook.get("symbol")
    if not isinstance(symbol, str) or not symbol:
        raise ResolutionError("hook.symbol is required")
    module_rva = hook.get("module_rva")
    rva_int = _rva_to_int(module_rva)

    # struct namespace -> (arg_index, extent bytes). The extent bounds every read
    # from that pointer so a bad offset cannot walk outside the struct.
    struct_args: dict[str, tuple[int, int]] = {}
    for entry in hook.get("arg_structs", []):
        index = entry["index"]
        extent = entry.get("extent")
        struct = entry["struct"]
        if not _is_plain_int(index) or not 0 <= index <= MAX_ARG_SLOT:
            raise ResolutionError(f"{symbol}: arg_structs index for {struct!r} must be 0..{MAX_ARG_SLOT}")
        if not _is_plain_int(extent) or not 0 < extent <= MAX_STRUCT_EXTENT:
            raise ResolutionError(
                f"{symbol}: arg_structs {struct!r} needs an extent in 1..{MAX_STRUCT_EXTENT} "
                "(the struct's declared byte size from abi-layout-probe)"
            )
        struct_args[struct] = (index, extent)

    plan: dict[str, list[dict[str, Any]]] = {"enter": [], "leave": []}

    for entry in hook.get("reads", []):
        name = entry["name"]
        interpret = entry["as"]
        phase = entry.get("phase", "enter")
        prefix = name.split(".", 1)[0]
        if prefix not in struct_args:
            raise ResolutionError(
                f"{symbol}: read {name!r} has no arg_structs mapping for struct {prefix!r}"
            )
        if name not in offset_map:
            raise ResolutionError(f"{symbol}: read {name!r} is absent from the offset map")
        arg_index, extent = struct_args[prefix]
        offset = offset_map[name]["offset"]
        size = offset_map[name]["size"]
        # load_offset_map already guarantees offset >= 0 and size > 0; bound the
        # field within the declared struct extent so a read can never leave it.
        if offset + size > extent:
            raise ResolutionError(
                f"{symbol}: read {name!r} (offset {offset}, size {size}) exceeds "
                f"struct {prefix!r} extent {extent}"
            )
        _check_interpret(interpret, size, f"{symbol} read {name}")
        plan[phase].append({
            "name": name,
            "source": "struct",
            "arg_index": arg_index,
            "offset": offset,
            "size": size,
            "extent": extent,
            "interpret": interpret,
        })

    for entry in hook.get("scalar_args", []):
        name = entry["name"]
        interpret = entry["as"]
        phase = entry.get("phase", "enter")
        index = entry["index"]
        width = entry.get("width")
        if interpret == "float":
            # x64 passes float/double in XMM registers, not the integer arg
            # slots, so a float scalar arg cannot be read from args[index].
            raise ResolutionError(
                f"{symbol}: scalar arg {name!r} is float; float args live in XMM, "
                "read the value through a struct field instead"
            )
        if not _is_plain_int(index) or not 0 <= index <= MAX_ARG_SLOT:
            raise ResolutionError(f"{symbol}: scalar arg {name!r} index must be 0..{MAX_ARG_SLOT}")
        if width not in REGISTER_WIDTHS:
            raise ResolutionError(
                f"{symbol}: scalar arg {name!r} needs an explicit width in {sorted(REGISTER_WIDTHS)}"
            )
        plan[phase].append({
            "name": name,
            "source": "register",
            "arg_index": index,
            "width": width,
            "interpret": interpret,
        })

    resolved: dict[str, Any] = {
        "symbol": symbol,
        "module_rva": module_rva,
        "module_rva_int": rva_int,
        "enter_reads": plan["enter"],
        "leave_reads": plan["leave"],
        "return": None,
    }
    if "return_as" in hook:
        resolved["return"] = {"interpret": hook["return_as"]}
    return resolved


def resolve_spec(spec: dict[str, Any], offset_map: Any) -> dict[str, Any]:
    if spec.get("spec_kind") != "known_function_hook_set":
        raise ResolutionError("spec_kind must be known_function_hook_set")
    module_label = spec.get("module_label")
    if not isinstance(module_label, str) or not module_label:
        raise ResolutionError("module_label is required")
    fields = load_offset_map(offset_map)
    hooks = spec.get("hooks")
    if not isinstance(hooks, list) or not hooks:
        raise ResolutionError("hooks must be a non-empty array")
    plans = [resolve_hook(hook, fields) for hook in hooks]
    return {
        "schema_version": 1,
        "plan_kind": "known_function_read_plan",
        "module_label": module_label,
        "hooks": plans,
    }


def build_event(
    message: dict[str, Any],
    *,
    module_label: str,
    plugin_label: str,
    host_version_label: str,
    event_index: int,
) -> dict[str, Any]:
    """Format one Frida ``send`` message into a validated trace event.

    ``message`` is produced by the thin Frida reader and already carries
    interpreted scalars (never raw bytes or pointers). The formatted event is
    validated against the trace contract; a redaction or shape violation raises
    ``ValueError`` so bad observations fail closed instead of being written.
    """

    if message.get("type") != "known_function":
        raise ValueError("message.type must be 'known_function'")
    phase = message.get("phase")
    known: dict[str, Any] = {
        "symbol": message.get("symbol"),
        "module_label": module_label,
        "module_rva": message.get("module_rva"),
        "phase": phase,
    }
    if phase == "leave" and "return_value" in message:
        known["return_value"] = message["return_value"]
    fields = message.get("fields")
    if fields:
        known["fields"] = [{"name": f["name"], "value": f["value"]} for f in fields]

    event = {
        "schema_version": 1,
        "event_index": event_index,
        "event_kind": "known_function_invoke",
        "host_kind": "native_observation",
        "host_version_label": host_version_label,
        "plugin_label": plugin_label,
        "known_function": known,
    }
    errors = validate_event(event)
    if errors:
        raise ValueError("; ".join(errors))
    return event


def session_boundary_event(
    kind: str,
    *,
    plugin_label: str,
    host_version_label: str,
    event_index: int,
) -> dict[str, Any]:
    event = {
        "schema_version": 1,
        "event_index": event_index,
        "event_kind": kind,
        "host_kind": "native_observation",
        "host_version_label": host_version_label,
        "plugin_label": plugin_label,
    }
    errors = validate_event(event)
    if errors:
        raise ValueError("; ".join(errors))
    return event


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--spec", required=True, type=Path, help="hook set JSON")
    parser.add_argument("--offset-map", required=True, type=Path, help="abi-layout-probe offset map JSON")
    parser.add_argument("--out", type=Path, help="write resolved read plan JSON here")
    args = parser.parse_args(argv)
    try:
        spec = json.loads(args.spec.read_text(encoding="utf-8"))
        plan = resolve_spec(spec, args.offset_map)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"known_function_observation: {type(exc).__name__}: {exc}", file=sys.stderr)
        return 2
    text = json.dumps(plan, indent=2, ensure_ascii=False, sort_keys=True) + "\n"
    if args.out:
        args.out.write_text(text, encoding="utf-8")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
