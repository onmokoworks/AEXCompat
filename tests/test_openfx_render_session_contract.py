import importlib.util
import json
import sys
from pathlib import Path

import jsonschema


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "tools" / "openfx_render_session_contract.py"
SPEC = importlib.util.spec_from_file_location("openfx_render_session_contract", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def packet(status: str = "rendered") -> dict:
    pixels = bytes(range(32))
    return MODULE.build_bridge_packet(
        plugin_relative_path="plugins/example.aex",
        plugin_sha256="a" * 64,
        worker_sha256="b" * 64,
        width=3,
        height=2,
        rowbytes=16,
        pixels=pixels,
        response_status=status,
    )


def test_positive_packet_is_schema_and_semantically_valid():
    value = packet()
    schema = json.loads(MODULE.SCHEMA_PATH.read_text(encoding="utf-8"))
    jsonschema.Draft202012Validator(schema).validate(value)
    assert MODULE.validate_bridge_packet(value) == []


def test_padded_stride_hash_and_identity_are_checked():
    value = packet()
    value["frame_exchange"]["request"]["input"]["sha256"] = "c" * 64
    errors = MODULE.validate_bridge_packet(value)
    assert any("sha256 does not match" in error for error in errors)

    value = packet()
    value["frame_exchange"]["response"]["identity"]["worker_sha256"] = "c" * 64
    errors = MODULE.validate_bridge_packet(value)
    assert "render response worker identity does not match session_open" in errors


def test_absolute_or_traversal_plugin_paths_are_rejected():
    for path in ("C:/private/example.aex", "../example.aex", "\\\\server\\example.aex"):
        value = packet()
        value["session_open"]["plugin"]["relative_path"] = path
        errors = MODULE.validate_bridge_packet(value)
        assert any("relative_path" in error for error in errors), path


def test_failed_status_is_explicit_and_cannot_be_marked_healthy():
    value = packet("timeout")
    assert MODULE.validate_bridge_packet(value) == []

    value["close"]["status"] = "closed"
    errors = MODULE.validate_bridge_packet(value)
    assert "a non-rendered response cannot close as healthy" in errors

    value = packet("timeout")
    value["contract_state"] = "ready"
    errors = MODULE.validate_bridge_packet(value)
    assert "failed response requires contract_state=rejected" in errors


def test_rendered_status_requires_output_and_identity():
    value = packet()
    value["frame_exchange"]["response"].pop("output")
    errors = MODULE.validate_bridge_packet(value)
    assert any("output" in error for error in errors)
    assert any("identity" in error for error in errors)


def test_geometry_rejects_non_tight_session_open_stride():
    value = packet()
    value["session_open"]["geometry"]["rowbytes"] = 16
    errors = MODULE.validate_bridge_packet(value)
    assert "session_open.geometry.rowbytes must be tight RGBA8 rowbytes" in errors


def test_transport_bound_rejects_an_oversized_padded_frame():
    value = packet()
    value["frame_exchange"]["request"]["input"]["rowbytes"] = 65536
    value["frame_exchange"]["request"]["input"]["height"] = 4096
    value["frame_exchange"]["request"]["input"]["data_base64"] = "AA=="
    errors = MODULE.validate_bridge_packet(value)
    assert any("transport bound" in error for error in errors)
