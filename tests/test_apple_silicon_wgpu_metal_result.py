import json
import re
from pathlib import Path, PurePosixPath


ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "analysis" / "APPLE_SILICON_WGPU_METAL_RESULT_2026-07-31.json"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
GIT_SHA = re.compile(r"^[0-9a-f]{40}$")

DEFAULT_RAW = "e3c36b52b1033c74507a419630f5918656b9b267a89c3573a65e7605ae0ad89e"
DEFAULT_RGBA = "6f24052bf442cc05899fdfe3779514c610652c6ab1d8dcba083dbf36f9ad0617"
BRIGHTNESS_RAW = "d6f672fae016974aec384a4533a02ea56c46322d3e31b8b923f0c9b4c122455a"
BRIGHTNESS_RGBA = "6677d579bc6680478e5bd53897795d726ff999ff3af13c482686fa799c55e6f8"
OLMBLUR_RAW = "3f174ac3d9c0cc8d573dbb4424982e7465a2628e07327e200a8256b59aa96693"


def _reject_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _load():
    return json.loads(
        EVIDENCE.read_text(encoding="utf-8"),
        object_pairs_hook=_reject_duplicate_keys,
    )


def _assert_zero_counts(counts):
    assert counts
    assert all(value == 0 for value in counts.values())


def test_schema_and_pinned_fixture_provenance_are_self_contained():
    result = _load()
    assert set(result) == {
        "schema_version",
        "issue",
        "observed_utc",
        "repository_head",
        "base_head",
        "host",
        "fixture",
        "reproduction",
        "normal_effect_entry",
        "wgpu_renders",
        "oracle_comparison",
        "cpu_non_regression",
        "validation",
    }
    assert result["schema_version"] == 1
    assert result["issue"] == 615
    assert GIT_SHA.fullmatch(result["repository_head"])
    assert GIT_SHA.fullmatch(result["base_head"])
    assert result["host"] == {
        "os": "macOS 15.7.2 (24G325)",
        "architecture": "arm64",
        "processor": "Apple M1 Pro",
    }

    fixture = result["fixture"]
    assert fixture["path_kind"] == "repository_relative_provenance"
    assert not Path(fixture["path"]).is_absolute()
    assert fixture["size_bytes"] == 109568
    assert fixture["sha256"] == (
        "6ad8f4e0808c1fe2cda1cf8a66e79b5f333095bb8ff28f1aa1764260730f586c"
    )
    assert fixture["input"]["path_kind"] == "repository_relative_provenance"
    assert not Path(fixture["input"]["path"]).is_absolute()
    assert fixture["input"]["sha256"] == (
        "bd112765998bb452a0ddd0532c2a58d14156be25b261b99ead372826e6ca455f"
    )
    assert (fixture["input"]["width"], fixture["input"]["height"]) == (37, 23)
    assert fixture["opencl_source"] == {
        "strings": 2,
        "bytes": 66067,
        "sha256": "989aab007e3fa9df453e39ea9aeb0a4e5b126cad49238bba6f197aae6c831937",
    }
    assert SHA256.fullmatch(fixture["precompiled_spirv"]["sha256"])
    assert SHA256.fullmatch(fixture["compiler"]["binary_sha256"])
    assert fixture["precompiled_spirv"]["entry_points"] == [
        "ProcAmp2Kernel",
        "InvertColorKernel",
    ]


def test_normal_effect_entry_preserves_selector_suite_and_opencl_abi_contracts():
    result = _load()
    effect = result["normal_effect_entry"]
    assert effect["normal_effect_entry_exercised"] is True
    assert effect["execution_backend"] == "unicorn-x86_64"
    assert effect["render_mode"] == "smart-opencl"
    assert effect["requested_backend"] == "wgpu-metal"
    assert effect["runtime_backend"] == "wgpu-metal"
    assert effect["plugin_framework"] == "opencl"
    assert effect["device_index"] == 0
    assert effect["suite_requests"] == [
        "PF Handle Suite v2",
        "PF GPU Device Suite v1",
        "PF World Suite v2",
    ]

    selectors = effect["selector_lifecycle"]
    assert [(item["selector"], item["command"]) for item in selectors] == [
        ("GPU_DEVICE_SETUP", 32),
        ("SMART_PRE_RENDER", 23),
        ("SMART_RENDER_GPU", 31),
        ("GPU_DEVICE_SETDOWN", 33),
    ]
    assert all(item["attempted"] and item["completed"] for item in selectors)
    assert all(item["error"] == 0 for item in selectors)
    assert effect["gpu_render_possible"] is True
    assert effect["runtime_started"] is True
    assert effect["transport_prepared"] is True
    assert effect["transport_finished"] is True
    assert effect["runtime_ended"] is True
    assert effect["guards_intact"] is True
    assert effect["unsupported_suite_calls"] == 0

    suite = effect["device_suite"]
    assert suite["allocations_created"] == suite["allocations_freed"] == 3
    assert suite["worlds_created"] == suite["worlds_disposed"] == 1
    assert suite["upload_bytes"] == suite["download_bytes"] == 13616
    assert suite["cleanup_balanced"] is True
    assert suite["transport_active"] is False
    _assert_zero_counts(
        {
            key: suite[key]
            for key in (
                "invalid_operations",
                "exclusive_access_depth",
                "live_host_allocations",
                "live_gpu_worlds",
                "live_borrowed_gpu_worlds",
                "live_device_allocations",
                "live_bytes",
            )
        }
    )

    opencl = effect["opencl_abi_facade"]
    assert opencl["native_opencl_backend_used"] is False
    assert opencl["api_calls"] == {
        "clCreateProgramWithSource": 1,
        "clBuildProgram": 1,
        "clCreateKernel": 2,
        "clSetKernelArg": 18,
        "clEnqueueNDRangeKernel": 2,
        "clReleaseKernel": 2,
    }
    assert opencl["source_strings"] == 2
    assert opencl["source_bytes"] == 66067
    assert opencl["programs_built"] == 1
    assert opencl["kernels_created"] == opencl["kernels_released"] == 2
    assert opencl["kernel_arguments"] == {
        "total": 18,
        "buffer": 4,
        "scalar": 14,
        "scalar_bytes": 56,
    }
    assert opencl["kernel_dispatches"] == 2
    assert opencl["dispatched_work_items"] == 3072
    assert opencl["cleanup_balanced"] is True
    _assert_zero_counts(
        {
            key: opencl[key]
            for key in (
                "errors",
                "live_buffers",
                "live_programs",
                "live_kernels",
                "native_contexts",
                "native_command_queues",
                "native_buffers",
                "native_programs",
                "native_kernels",
                "native_release_errors",
            )
        }
    )


def test_wgpu_metal_artifact_dispatch_geometry_and_resources_are_bounded():
    result = _load()
    fixture = result["fixture"]
    wgpu = result["normal_effect_entry"]["wgpu"]
    assert wgpu["executor_available"] is True
    assert wgpu["backend_operations_attempted"] == 3
    assert wgpu["adapter"]["index"] == 0
    assert wgpu["adapter"]["name"] == "Apple M1 Pro"
    assert wgpu["adapter"]["backend"] == "Metal"
    assert wgpu["adapter"]["device_type"] == "IntegratedGpu"

    assert len(wgpu["artifacts"]) == 1
    artifact = wgpu["artifacts"][0]
    assert artifact["toolchain_mode"] == "precompiled-pinned"
    assert artifact["source_sha256"] == fixture["opencl_source"]["sha256"]
    assert artifact["raw_spirv_sha256"] == fixture["precompiled_spirv"]["sha256"]
    assert artifact["compiler_sha256"] == fixture["compiler"]["binary_sha256"]
    assert artifact["normalized_options"] == fixture["compiler"]["normalized_options"]

    dispatches = wgpu["dispatches"]
    assert [item["kernel"] for item in dispatches] == [
        "InvertColorKernel",
        "ProcAmp2Kernel",
    ]
    assert len(dispatches) == 2
    for dispatch in dispatches:
        assert dispatch["work_dim"] == 2
        assert dispatch["global"] == [48, 32, 1]
        assert dispatch["local"] == [16, 16, 1]
        assert dispatch["groups"] == [3, 2, 1]
        assert dispatch["storage_bindings"] == 2
        assert dispatch["uniform_bindings"] == 1
        assert dispatch["upload_bytes"] > 0
        assert dispatch["download_bytes"] > 0
    assert wgpu["upload_bytes"] == sum(item["upload_bytes"] for item in dispatches)
    assert wgpu["download_bytes"] == sum(item["download_bytes"] for item in dispatches)
    assert all(value > 0 for value in wgpu["created_resources"].values())
    _assert_zero_counts(wgpu["live_resources"])
    assert wgpu["cleanup_balanced"] is True
    assert result["normal_effect_entry"]["cleanup_complete"] is True


def test_wgpu_is_byte_exact_and_parameter_sensitive_against_both_oracles():
    result = _load()
    renders = result["wgpu_renders"]
    default = renders["default"]
    changed = renders["brightness_25"]
    assert default["raw_argb32f_sha256"] == DEFAULT_RAW
    assert default["decoded_public_rgba8_sha256"] == DEFAULT_RGBA
    assert changed["raw_argb32f_sha256"] == BRIGHTNESS_RAW
    assert changed["decoded_public_rgba8_sha256"] == BRIGHTNESS_RGBA
    assert default["raw_pixel_bytes"] == changed["raw_pixel_bytes"] == 13616
    assert default["exit_code"] == default["render_error"] == 0
    assert changed["exit_code"] == changed["render_error"] == 0
    assert changed["parameter"] == {
        "slot": 1,
        "name": "Brightness",
        "value": 25.0,
        "kernel_value": 0.25,
    }
    assert changed["differs_from_default"] is True
    assert default["raw_argb32f_sha256"] != changed["raw_argb32f_sha256"]
    assert default["decoded_public_rgba8_sha256"] != changed["decoded_public_rgba8_sha256"]
    assert default["windows_byte_exact"] is True
    assert default["apple_opencl_byte_exact"] is True
    assert changed["windows_byte_exact"] is True
    assert changed["apple_opencl_byte_exact"] is True
    assert default["cleanup_complete"] is True
    assert changed["cleanup_complete"] is True

    for oracle in result["oracle_comparison"].values():
        assert oracle["default"]["raw_argb32f_sha256"] == DEFAULT_RAW
        assert oracle["default"]["decoded_public_rgba8_sha256"] == DEFAULT_RGBA
        assert oracle["brightness_25"]["raw_argb32f_sha256"] == BRIGHTNESS_RAW
        assert oracle["brightness_25"]["decoded_public_rgba8_sha256"] == BRIGHTNESS_RGBA

    apple = result["oracle_comparison"]["apple_opencl"]
    assert apple["requested_backend"] == "opencl"
    assert apple["runtime_backend"] == "apple-opencl"
    assert apple["plugin_framework"] == "opencl"
    assert apple["platform"] == "Apple"
    assert apple["device"] == "Apple M1 Pro"
    assert apple["default"]["wgpu_byte_exact"] is True
    assert apple["brightness_25"]["wgpu_byte_exact"] is True
    assert apple["cleanup_complete"] is True


def test_cpu_regression_commands_and_validation_status_are_explicit():
    result = _load()
    cpu = result["cpu_non_regression"]
    assert cpu["fixture_path_kind"] == "external_local_provenance"
    assert PurePosixPath(cpu["fixture"]).is_absolute()
    assert cpu["fixture"].endswith("/OLM as/plugins_2025/OLMBlur.aex")
    assert SHA256.fullmatch(cpu["fixture_sha256"])
    assert cpu["input_path"] == result["fixture"]["input"]["path"]
    assert cpu["input_sha256"] == result["fixture"]["input"]["sha256"]
    assert cpu["exit_code"] == cpu["render_error"] == 0
    assert cpu["render_mode"] == "smart-cpu"
    assert cpu["pixel_format"] == "argb8"
    assert cpu["raw_pixel_bytes"] == 3404
    assert cpu["raw_argb8_sha256"] == OLMBLUR_RAW
    assert cpu["gpu_runtime_started"] is False
    assert cpu["gpu_setup_attempted"] is False
    assert cpu["gpu_setdown_attempted"] is False
    assert cpu["guards_intact"] is True
    assert cpu["cleanup_complete"] is True

    reproduction = result["reproduction"]
    assert reproduction["working_directory"] == "repository root"
    environment = reproduction["wgpu_environment"]
    assert environment["AEXCOMPAT_WGPU_PRECOMPILED_SPIRV"].startswith("$PWD/")
    assert (
        environment["AEXCOMPAT_WGPU_PRECOMPILED_SOURCE_SHA256"]
        == result["fixture"]["opencl_source"]["sha256"]
    )
    assert (
        environment["AEXCOMPAT_WGPU_PRECOMPILED_SPIRV_SHA256"]
        == result["fixture"]["precompiled_spirv"]["sha256"]
    )
    assert (
        environment["AEXCOMPAT_WGPU_PRECOMPILED_COMPILER_SHA256"]
        == result["fixture"]["compiler"]["binary_sha256"]
    )
    commands = reproduction["commands"]
    input_path = result["fixture"]["input"]["path"]
    assert all(input_path in command for command in commands.values())
    assert "--render-backend wgpu-metal" in commands["wgpu_default"]
    assert "AEXCOMPAT_WGPU_PRECOMPILED_SPIRV=" in commands["wgpu_default"]
    assert "Brightness=25" in commands["wgpu_brightness_25"]
    assert "--render-backend opencl" in commands["apple_opencl_default"]
    assert "Brightness=25" in commands["apple_opencl_brightness_25"]
    assert f'"{cpu["fixture"]}"' in commands["olmblur_cpu"]
    assert "--render-backend cpu" in commands["olmblur_cpu"]

    validation = result["validation"]
    assert validation["guest_worker_tests"] == {
        "status": "pass",
        "passed_tests": 183,
    }
    assert validation["guest_worker_release_build"] == "pass"
    assert validation["remaining_workspace_validation"] == "pending"
    assert validation["latest_head_review"] == "pending"
    assert validation["ci"] == "ignored under the repository billing policy"
