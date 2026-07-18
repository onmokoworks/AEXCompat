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


# At most 8 hex digits, matching the JSON schemas exactly so a resolved plan can
# never produce a trace RVA the schema rejects; 8 digits caps the value below the
# 4 GiB module-relative bound.
MODULE_RVA = re.compile(r"^0x[0-9a-f]{1,8}$")
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
# Allowed interpretations. Struct reads may be float; register scalars may not
# (a float arg lives in XMM, and a pointer is not a representable role).
READ_INTERPRETS = {"int", "uint", "float", "bool"}
REGISTER_INTERPRETS = {"int", "uint", "bool"}
RETURN_INTERPRETS = {"int", "uint"}
# Allowed keys per object, mirroring the schema's additionalProperties:false so a
# misspelled key (e.g. "readz") fails closed instead of being silently ignored.
SPEC_KEYS = {"schema_version", "spec_kind", "module_label", "hooks"}
HOOK_KEYS = {"symbol", "module_rva", "arg_structs", "scalar_args", "reads", "return_as", "return_width"}
ARG_STRUCT_KEYS = {"index", "struct", "extent"}
READ_KEYS = {"name", "as", "phase"}
SCALAR_ARG_KEYS = {"index", "name", "as", "width", "phase"}


class ResolutionError(ValueError):
    """A hook spec could not be resolved against an offset map."""


def _rva_to_int(rva: Any) -> int:
    if not isinstance(rva, str) or not MODULE_RVA.match(rva):
        raise ResolutionError(f"module_rva must match ^0x[0-9a-f]{{1,8}}$ (module-relative), got {rva!r}")
    return int(rva, 16)


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


def _require_fields(entry: Any, required: tuple[str, ...], where: str) -> None:
    """Raise ResolutionError (not KeyError/TypeError) if entry is not a dict with
    all required keys, so a malformed hand-authored spec fails closed via main."""

    if not isinstance(entry, dict):
        raise ResolutionError(f"{where} must be an object")
    missing = [key for key in required if key not in entry]
    if missing:
        raise ResolutionError(f"{where} is missing required field(s) {missing}")


def _reject_unknown(entry: dict[str, Any], allowed: set[str], where: str) -> None:
    extra = sorted(set(entry) - allowed)
    if extra:
        raise ResolutionError(f"{where} has unknown field(s) {extra}")


def resolve_hook(hook: dict[str, Any], offset_map: dict[str, dict[str, int]]) -> dict[str, Any]:
    """Resolve one hook into a per-phase read plan.

    Raises :class:`ResolutionError` with an explicit message on any gap so a
    missing field or an XMM-only float argument fails loudly rather than
    silently producing nothing.
    """

    if not isinstance(hook, dict):
        raise ResolutionError("hook must be an object")
    symbol = hook.get("symbol")
    if not isinstance(symbol, str) or not symbol:
        raise ResolutionError("hook.symbol is required")
    _reject_unknown(hook, HOOK_KEYS, f"hook {symbol!r}")
    module_rva = hook.get("module_rva")
    rva_int = _rva_to_int(module_rva)

    # struct namespace -> (arg_index, extent bytes). The extent bounds every read
    # from that pointer so a bad offset cannot walk outside the struct.
    struct_args: dict[str, tuple[int, int]] = {}
    for entry in hook.get("arg_structs", []):
        _require_fields(entry, ("index", "struct", "extent"), f"{symbol}: arg_structs entry")
        _reject_unknown(entry, ARG_STRUCT_KEYS, f"{symbol}: arg_structs entry")
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
        _require_fields(entry, ("name", "as"), f"{symbol}: read entry")
        _reject_unknown(entry, READ_KEYS, f"{symbol}: read entry")
        name = entry["name"]
        interpret = entry["as"]
        phase = entry.get("phase", "enter")
        if phase not in plan:
            raise ResolutionError(f"{symbol}: read {name!r} has unsupported phase {phase!r} (allowed: {list(PHASES)})")
        prefix = name.split(".", 1)[0]
        if prefix not in struct_args:
            raise ResolutionError(
                f"{symbol}: read {name!r} has no arg_structs mapping for struct {prefix!r}"
            )
        if name not in offset_map:
            raise ResolutionError(f"{symbol}: read {name!r} is absent from the offset map")
        if interpret not in READ_INTERPRETS:
            raise ResolutionError(
                f"{symbol}: read {name!r} has unsupported interpretation {interpret!r} "
                f"(allowed: {sorted(READ_INTERPRETS)})"
            )
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
        _require_fields(entry, ("index", "name", "as", "width"), f"{symbol}: scalar_args entry")
        _reject_unknown(entry, SCALAR_ARG_KEYS, f"{symbol}: scalar_args entry")
        name = entry["name"]
        interpret = entry["as"]
        phase = entry.get("phase", "enter")
        if phase not in plan:
            raise ResolutionError(f"{symbol}: scalar arg {name!r} has unsupported phase {phase!r} (allowed: {list(PHASES)})")
        index = entry["index"]
        width = entry.get("width")
        if interpret == "float":
            # x64 passes float/double in XMM registers, not the integer arg
            # slots, so a float scalar arg cannot be read from args[index].
            raise ResolutionError(
                f"{symbol}: scalar arg {name!r} is float; float args live in XMM, "
                "read the value through a struct field instead"
            )
        if interpret not in REGISTER_INTERPRETS:
            raise ResolutionError(
                f"{symbol}: scalar arg {name!r} has unsupported interpretation {interpret!r} "
                f"(allowed: {sorted(REGISTER_INTERPRETS)})"
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
        return_as = hook["return_as"]
        if return_as not in RETURN_INTERPRETS:
            raise ResolutionError(
                f"{symbol}: return_as {return_as!r} must be one of {sorted(RETURN_INTERPRETS)}"
            )
        return_width = hook.get("return_width", 4)
        if return_width not in REGISTER_WIDTHS:
            raise ResolutionError(
                f"{symbol}: return_width must be one of {sorted(REGISTER_WIDTHS)}"
            )
        resolved["return"] = {"interpret": return_as, "width": return_width}
    return resolved


def resolve_spec(spec: dict[str, Any], offset_map: Any) -> dict[str, Any]:
    if not isinstance(spec, dict):
        raise ResolutionError("spec must be an object")
    _reject_unknown(spec, SPEC_KEYS, "spec")
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
