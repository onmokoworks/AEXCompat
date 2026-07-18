"""Standard-library validation for AEXCompat host trace contracts."""

from __future__ import annotations

import re
import uuid
from typing import Any


ABSOLUTE_PATH = re.compile(r"(^|[^A-Za-z])[A-Za-z]:\\")
EVENT_KINDS = {
    "session_start", "selector_dispatch", "suite_acquire", "suite_release",
    "callback_invoke", "world_descriptor", "known_function_invoke",
    "error", "unimplemented", "session_end",
}
HOST_KINDS = {"after_effects_manual", "minihost", "native_observation"}
PIXEL_FORMATS = {"argb8", "argb16", "argb32f", "rgba8", "unknown"}
MODULE_RVA = re.compile(r"^0x[0-9a-f]+$")
# A module-relative offset is bounded by the plug-in image size, always well
# under 4 GiB. A 64-bit absolute (ASLR'd) code address lowercases to the same
# hex shape but is far larger, so bound the magnitude to reject it: a genuine
# absolute address like 0x7ffabc001c40 must not slip through as an "RVA".
MODULE_RVA_LIMIT = 0x1_0000_0000
KNOWN_FUNCTION_PHASES = {"enter", "leave"}
BASE_FIELDS = {
    "schema_version", "event_index", "event_kind", "host_kind",
    "host_version_label", "plugin_label",
}
PAYLOAD_FIELDS = {"selector", "suite", "world", "error", "known_function"}
FORBIDDEN_FIELDS = {"raw_payload", "binary_payload", "pixels", "pointer"}


def _is_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def _is_number(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def _check_stem(value: Any, field: str, errors: list[str]) -> None:
    _check_string(value, field, errors)
    if isinstance(value, str) and any(c in value for c in "\\/:"):
        errors.append(f"{field} must be a filename stem")


def _check_string(value: Any, field: str, errors: list[str], *, nonempty: bool = True) -> None:
    if not isinstance(value, str) or (nonempty and not value):
        errors.append(f"{field} must be a{' non-empty' if nonempty else ''} string")
    elif ABSOLUTE_PATH.search(value):
        errors.append(f"{field} contains a local absolute path")


def validate_event(event: Any) -> list[str]:
    errors: list[str] = []
    if not isinstance(event, dict):
        return ["event must be an object"]

    unknown = set(event) - BASE_FIELDS - PAYLOAD_FIELDS
    for field in sorted(unknown):
        label = "forbidden" if field in FORBIDDEN_FIELDS else "unknown"
        errors.append(f"{label} field: {field}")
    for field in BASE_FIELDS:
        if field not in event:
            errors.append(f"missing field: {field}")

    if event.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    if not _is_int(event.get("event_index")) or event.get("event_index", -1) < 0:
        errors.append("event_index must be a non-negative integer")
    kind = event.get("event_kind")
    if kind not in EVENT_KINDS:
        errors.append("event_kind is unsupported")
    if event.get("host_kind") not in HOST_KINDS:
        errors.append("host_kind is unsupported")
    _check_string(event.get("host_version_label"), "host_version_label", errors)
    _check_string(event.get("plugin_label"), "plugin_label", errors)
    if isinstance(event.get("plugin_label"), str) and any(c in event["plugin_label"] for c in "\\/:"):
        errors.append("plugin_label must be a filename stem")

    if kind == "selector_dispatch":
        _check_string(event.get("selector"), "selector", errors)
    if kind in {"suite_acquire", "suite_release"}:
        suite = event.get("suite")
        if not isinstance(suite, dict) or set(suite) != {"name", "version", "granted"}:
            errors.append("suite must contain exactly name, version, and granted")
        else:
            _check_string(suite["name"], "suite.name", errors)
            if not _is_int(suite["version"]) or suite["version"] < 0:
                errors.append("suite.version must be a non-negative integer")
            if not isinstance(suite["granted"], bool):
                errors.append("suite.granted must be boolean")
    if kind == "world_descriptor":
        world = event.get("world")
        expected = {"width", "height", "rowbytes", "pixel_format"}
        if not isinstance(world, dict) or set(world) != expected:
            errors.append("world descriptor fields are invalid")
        else:
            for field in ("width", "height", "rowbytes"):
                if not _is_int(world[field]) or world[field] < 0:
                    errors.append(f"world.{field} must be a non-negative integer")
            if world["pixel_format"] not in PIXEL_FORMATS:
                errors.append("world.pixel_format is unsupported")
    if kind == "known_function_invoke":
        known = event.get("known_function")
        allowed = {"symbol", "module_label", "module_rva", "phase", "return_value", "fields"}
        required = {"symbol", "module_label", "module_rva", "phase"}
        if not isinstance(known, dict):
            errors.append("known_function must be an object")
        else:
            for field in sorted(set(known) - allowed):
                errors.append(f"known_function.{field} is not allowed")
            for field in sorted(required - set(known)):
                errors.append(f"known_function.{field} is required")
            _check_stem(known.get("symbol"), "known_function.symbol", errors)
            _check_stem(known.get("module_label"), "known_function.module_label", errors)
            rva = known.get("module_rva")
            if not isinstance(rva, str) or not MODULE_RVA.match(rva):
                errors.append("known_function.module_rva must be a lowercase-hex module offset")
            elif int(rva, 16) >= MODULE_RVA_LIMIT:
                errors.append("known_function.module_rva exceeds the module-relative bound (looks absolute)")
            if known.get("phase") not in KNOWN_FUNCTION_PHASES:
                errors.append("known_function.phase must be enter or leave")
            if "return_value" in known and not _is_number(known["return_value"]):
                errors.append("known_function.return_value must be a numeric scalar")
            if "fields" in known:
                fields = known["fields"]
                if not isinstance(fields, list):
                    errors.append("known_function.fields must be an array")
                else:
                    for index, entry in enumerate(fields):
                        label = f"known_function.fields[{index}]"
                        if not isinstance(entry, dict) or set(entry) != {"name", "value"}:
                            errors.append(f"{label} must contain exactly name and value")
                            continue
                        _check_string(entry["name"], f"{label}.name", errors)
                        value = entry["value"]
                        if not _is_number(value) and not isinstance(value, bool):
                            errors.append(f"{label}.value must be a numeric or boolean scalar")
    if kind in {"error", "unimplemented"}:
        detail = event.get("error")
        if not isinstance(detail, dict) or set(detail) != {"code_label", "message"}:
            errors.append("error must contain exactly code_label and message")
        else:
            _check_string(detail["code_label"], "error.code_label", errors)
            _check_string(detail["message"], "error.message", errors, nonempty=False)
    return errors


def validate_session(session: Any) -> list[str]:
    if not isinstance(session, dict):
        return ["session must be an object"]
    errors: list[str] = []
    expected = {"schema_version", "session_id", "event_count", "trace_complete", "events"}
    if set(session) != expected:
        errors.append("session fields are invalid")
    if session.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    try:
        uuid.UUID(session.get("session_id", ""))
    except (ValueError, TypeError, AttributeError):
        errors.append("session_id must be a UUID")
    events = session.get("events")
    if not isinstance(events, list):
        return errors + ["events must be an array"]
    for index, event in enumerate(events):
        errors.extend(f"events[{index}]: {error}" for error in validate_event(event))
    if session.get("event_count") != len(events):
        errors.append("event_count does not match events")
    if [event.get("event_index") for event in events if isinstance(event, dict)] != list(range(len(events))):
        errors.append("event indexes are not contiguous from zero")
    complete = bool(events) and events[0].get("event_kind") == "session_start" and events[-1].get("event_kind") == "session_end"
    if session.get("trace_complete") != complete:
        errors.append("trace_complete does not match boundary events")
    return errors
