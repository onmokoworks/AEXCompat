import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_resolve_ofx_contract_is_machine_checkable_and_fail_closed():
    contract = json.loads(
        (ROOT / "contracts" / "resolve_ofx_surface.json").read_text(
            encoding="utf-8"
        )
    )
    schema = json.loads(
        (ROOT / "contracts" / "resolve_ofx_surface.schema.json").read_text(
            encoding="utf-8"
        )
    )
    assert schema["properties"]["schema_version"]["const"] == 1
    assert contract["schema_version"] == 1
    assert contract["provenance"]["vendored_headers"] is False
    assert contract["package"] == {
        "bundle_suffix": ".ofx.bundle",
        "binary_suffix": ".ofx",
        "windows_architecture": "Win64",
    }
    assert contract["control_render"]["state"] == "verified_local_fixture"
    assert contract["control_render"]["render_claim"] == "builtin_rgba8_control"
    assert contract["control_render"]["aex_render_claim"] == (
        "blocked_missing_aex_rendersession"
    )
    assert contract["control_render"]["source_rowbytes"] == 20
    assert contract["control_render"]["output_rowbytes"] == 24
    assert contract["control_render"]["parameter"] == {"name": "strength", "time": 7.0, "value": 0.25}
    assert contract["control_render"]["pixel_diff"] == 12
    assert contract["aex_render_gate"]["state"] == "blocked"
    assert contract["aex_render_gate"]["success_status"] == "not_claimed"
    assert contract["aex_render_gate"]["blocked_status"] == "kOfxStatErrUnsupported"
    assert len(contract["identity"]["source_sha256"]) == 64
    assert len(contract["identity"]["binary_sha256"]) == 64
    assert contract["host_evidence"]["standard_external_path_entries"] == 0
    assert contract["host_evidence"]["real_resolve_smoke"] == (
        "blocked_external_path_empty"
    )


def test_resolve_ofx_source_contains_real_exports_and_no_identity_success():
    source = (ROOT / "bridges" / "resolve-ofx" / "src" / "resolve_ofx_plugin.cpp").read_text(
        encoding="utf-8"
    )
    header = (ROOT / "bridges" / "resolve-ofx" / "include" / "resolve_ofx_abi.h").read_text(
        encoding="utf-8"
    )
    assert "OfxGetNumberOfPlugins" in source
    assert "OfxGetPlugin" in source
    assert "OfxSetHost" in source
    assert "OfxPluginMain" in source
    assert "kOfxActionCreateInstance" in source
    assert "kOfxActionDestroyInstance" in source
    assert "kOfxImageEffectActionRender" in source
    assert "clipGetImage" in source
    assert "kOfxImagePropRowBytes" in source
    assert "kOfxStatErrUnsupported" in source
    assert "paramGetValueAtTime" in source
    assert "strength" in source
    assert "return kOfxStatErrUnsupported;" in source
    assert "OpenFX" in header
    assert "BSD-3-Clause" in header
    assert '#include "ofx' not in header
