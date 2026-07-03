#!/usr/bin/env python3
"""Pathless JSONL native-loader broker for no-load selftests.

The broker intentionally accepts no AEX path and exposes no native load action.
It exists only to prove the future loader boundary can start closed and report
its safety state before any path-bearing protocol is introduced.
"""

from __future__ import annotations

import json
import platform
import struct
import sys
from typing import Any, TextIO


SCHEMA_VERSION = 1
BROKER_KIND = "aex_pathless_native_loader_broker"

ALLOWED_MESSAGES = [
    "hello",
    "inspect_environment",
    "quit",
]

BLOCKED_MESSAGES = {
    "accept_aex_path",
    "open_aex_file",
    "hash_aex_file",
    "copy_selected_aex_fixture",
    "load_aex_dll",
    "load_dependency_dll",
    "call_EffectMain",
    "call_effect_main",
    "dispatch_PF_Cmd",
    "render_frame",
    "render_with_aex",
    "ofx_describe",
    "route_through_ofx",
}


def safety_state() -> dict[str, Any]:
    return {
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "accepts_aex_path": False,
        "accepted_aex_path": None,
    }


def error_response(code: str, message: str, request_id: Any = None) -> dict[str, Any]:
    return {
        "type": "error",
        "request_id": request_id,
        "code": code,
        "message": message,
        "safety_state": safety_state(),
    }


def with_request_id(response: dict[str, Any], request_id: Any) -> dict[str, Any]:
    if request_id is not None:
        response["request_id"] = request_id
    return response


def handle_message(message: dict[str, Any]) -> dict[str, Any]:
    if not isinstance(message, dict):
        return error_response("invalid_message", "message must be an object")
    request_id = message.get("id")
    message_type = message.get("type")
    if not isinstance(message_type, str):
        return error_response("invalid_message", "message type must be a string", request_id)
    if message_type in BLOCKED_MESSAGES:
        return error_response(
            "blocked_action",
            f"{message_type} is blocked by the pathless native-loader broker",
            request_id,
        )
    if message_type == "hello":
        return with_request_id(
            {
                "type": "hello_ack",
                "schema_version": SCHEMA_VERSION,
                "broker_kind": BROKER_KIND,
                "allowed_messages": ALLOWED_MESSAGES,
                "blocked_messages": sorted(BLOCKED_MESSAGES),
                "safety_state": safety_state(),
            },
            request_id,
        )
    if message_type == "inspect_environment":
        return with_request_id(
            {
                "type": "environment_report",
                "schema_version": SCHEMA_VERSION,
                "process_bitness": struct.calcsize("P") * 8,
                "platform": platform.system(),
                "native_load_enabled": False,
                "accepts_aex_path": False,
                "accepted_aex_path": None,
                "safety_state": safety_state(),
            },
            request_id,
        )
    if message_type == "quit":
        return with_request_id({"type": "quit_ack", "safety_state": safety_state()}, request_id)
    return error_response("unknown_message", f"unknown message type: {message_type}", request_id)


def run_jsonl_loop(stdin: TextIO = sys.stdin, stdout: TextIO = sys.stdout) -> int:
    for line in stdin:
        if not line.strip():
            continue
        try:
            message = json.loads(line)
            response = handle_message(message)
        except Exception as exc:
            response = error_response("invalid_json", str(exc))
        stdout.write(json.dumps(response, ensure_ascii=False, separators=(",", ":")) + "\n")
        stdout.flush()
        if response.get("type") == "quit_ack":
            return 0
    return 0


def main() -> int:
    return run_jsonl_loop()


if __name__ == "__main__":
    raise SystemExit(main())
