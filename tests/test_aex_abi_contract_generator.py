import importlib.util
import json
import subprocess
import sys
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "generate-aex-abi-contract.py"
OBSERVATION = ROOT / "analysis" / "AE_ABI_LAYOUT_OBSERVATION_2026-07-13.json"


def load_generator():
    spec = importlib.util.spec_from_file_location("aex_abi_generator", SCRIPT)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_committed_outputs_are_deterministic():
    subprocess.run(
        [sys.executable, str(SCRIPT), "--check"],
        cwd=ROOT,
        check=True,
        timeout=30,
    )


def test_cpp_and_rust_share_observed_constants():
    cpp = (ROOT / "minihost" / "src" / "generated" / "aex_abi_contract.hpp").read_text()
    rust = (ROOT / "guest" / "crates" / "aex-abi" / "src" / "generated.rs").read_text()
    for name, value in (
        ("SCHEMA_VERSION", 1),
        ("PF_IN_DATA_SIZE", 408),
        ("PF_OUT_DATA_SIZE", 408),
        ("PF_UTIL_CALLBACKS_SIZE", 552),
        ("INTER_ADD_PARAM_OFFSET", 16),
    ):
        assert f"{name} = {value}" in cpp
        assert f"{name}: usize = {value}" in rust
    input_offsets = (
        "INTER_CHECKOUT_PARAM_OFFSET, INTER_CHECKIN_PARAM_OFFSET, "
        "INTER_ADD_PARAM_OFFSET"
    )
    utility_offsets = (
        "UTILS_BEGIN_SAMPLING_OFFSET, UTILS_SUBPIXEL_SAMPLE_OFFSET, "
        "UTILS_AREA_SAMPLE_OFFSET"
    )
    for output in (cpp, rust):
        assert input_offsets in output
        assert utility_offsets in output


def test_duplicate_keys_are_rejected(tmp_path):
    path = tmp_path / "duplicate.json"
    path.write_text(
        '{"schema_version":1,"schema_version":1}',
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match="duplicate JSON key"):
        load_generator().load_contract(path)


def test_missing_contract_is_rejected(tmp_path):
    with pytest.raises(ValueError, match="No such file"):
        load_generator().load_contract(tmp_path / "missing.json")


def test_malformed_contract_is_rejected(tmp_path):
    path = tmp_path / "malformed.json"
    path.write_text('{"schema_version":', encoding="utf-8")
    with pytest.raises(ValueError):
        load_generator().load_contract(path)


def test_missing_required_field_is_rejected(tmp_path):
    data = json.loads(OBSERVATION.read_text(encoding="utf-8"))
    del data["fields"]["inter.add_param"]
    path = tmp_path / "missing-field.json"
    path.write_text(json.dumps(data), encoding="utf-8")
    with pytest.raises(ValueError, match="missing required fields: inter.add_param"):
        load_generator().load_contract(path)


@pytest.mark.parametrize(
    ("mutation", "message"),
    [
        (
            lambda data: data.update(schema_version=True),
            "schema_version must be 1",
        ),
        (lambda data: data.update(architecture="arm64-macos"), "architecture"),
        (lambda data: data.update(pointer_size=4), "pointer_size"),
        (
            lambda data: data.update(native_aex_loaded=True),
            "native_aex_loaded must be false",
        ),
        (
            lambda data: data.update(selector_dispatched=True),
            "selector_dispatched must be false",
        ),
        (
            lambda data: data.update(sdk_boundary="runtime_observation"),
            "sdk_boundary must be instrument_observation",
        ),
        (
            lambda data: data["fields"].update(
                {"in.bad": {"offset": data["pf_in_data_size"], "size": 8}}
            ),
            "exceeds pf_in_data_size",
        ),
        (
            lambda data: data["fields"].update(
                {"in.bad": {"offset": -1, "size": 8}}
            ),
            "non-negative",
        ),
        (
            lambda data: data["fields"]["utils.subpixel_sample"].update(
                offset=data["fields"]["utils.begin_sampling"]["offset"]
            ),
            "UTILITY_CALLBACK_OFFSETS contains duplicate offsets",
        ),
        (
            lambda data: data["fields"].update(
                {"unknown_family.field": {"offset": 0, "size": 8}}
            ),
            "no container size mapping for field family",
        ),
        (
            lambda data: data["fields"]["batch_sampling.get_func16"].update(
                offset=data["pf_batch_sampling_suite1_size"]
            ),
            "exceeds pf_batch_sampling_suite1_size",
        ),
    ],
)
def test_invalid_contracts_fail_closed(tmp_path, mutation, message):
    data = json.loads(OBSERVATION.read_text(encoding="utf-8"))
    mutation(data)
    path = tmp_path / "invalid.json"
    path.write_text(json.dumps(data), encoding="utf-8")
    with pytest.raises(ValueError, match=message):
        load_generator().load_contract(path)


def test_probe_covers_every_current_bootstrap_callback():
    probe = (ROOT / "instruments" / "abi-layout-probe" / "main.cpp").read_text()
    names = (
        "inter.checkout_param", "inter.checkin_param", "inter.add_param",
        "inter.abort", "inter.progress", "inter.register_ui",
        "inter.checkout_layer_audio", "inter.checkin_layer_audio",
        "inter.get_audio_data", "utils.begin_sampling",
        "utils.subpixel_sample", "utils.area_sample", "utils.end_sampling",
        "utils.blend", "utils.convolve", "utils.copy", "utils.fill",
        "utils.premultiply", "utils.premultiply_color", "utils.fill16",
        "utils.premultiply_color16", "utils.iterate", "utils.new_world",
        "utils.dispose_world", "utils.transform_world", "utils.ansi_ceil",
        "utils.ansi_fabs", "utils.ansi_pow", "utils.ansi_sin",
        "utils.ansi_sprintf", "utils.ansi_strcpy", "utils.get_platform_data",
        "utils.get_pixel_data8", "utils.get_pixel_data16",
        "utils.host_new_handle", "utils.host_lock_handle",
        "utils.host_unlock_handle", "utils.host_dispose_handle",
        "utils.host_get_handle_size", "utils.host_resize_handle",
    )
    for name in names:
        assert f'"{name}"' in probe
