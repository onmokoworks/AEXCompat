"""Standard-library validation for AEXCompat host trace contracts."""

from __future__ import annotations

import re
import uuid
from typing import Any


ABSOLUTE_PATH = re.compile(r"(^|[^A-Za-z])[A-Za-z]:\\")
EVENT_KINDS = {
    "session_start", "selector_dispatch", "suite_acquire", "suite_release",
    "callback_invoke", "world_descriptor", "error", "unimplemented", "session_end",
}
HOST_KINDS = {"after_effects_manual", "minihost"}
PIXEL_FORMATS = {"argb8", "argb16", "argb32f", "rgba8", "unknown"}
BASE_FIELDS = {
    "schema_version", "event_index", "event_kind", "host_kind",
    "host_version_label", "plugin_label",
}
PAYLOAD_FIELDS = {"selector", "suite", "world", "error"}
FORBIDDEN_FIELDS = {"raw_payload", "binary_payload", "pixels", "pointer"}


def _is_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


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
