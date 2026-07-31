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
IMPLEMENTATION_HEAD = "c6a4181daf1af49372b115444b8982039f4bd418"
BASE_HEAD = "93cfbb7e9f25f296636444cc54d1a97624e12bfb"
PROVENANCE_SHA256 = "0372c252499d01cf530a2f8fd0365ca21355dd55eca758ae031d078d74522765"
LIVE_RESOURCE_SCOPE = (
    "Counts cover AEXCompat-owned dispatch-scoped wgpu handles only, not session "
    "handles or wgpu/Metal driver allocations. Zero means synchronous dispatch "
    "exited without retaining tracked dispatch handles; backend caches and deferred "
    "destruction are excluded."
)


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
        "distribution",
        "validation",
    }
    assert result["schema_version"] == 2
    assert result["issue"] == 615
    assert GIT_SHA.fullmatch(result["repository_head"])
    assert GIT_SHA.fullmatch(result["base_head"])
    assert result["repository_head"] == IMPLEMENTATION_HEAD
    assert result["base_head"] == BASE_HEAD
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
    assert fixture["precompiled_spirv"]["provenance_sha256"] == PROVENANCE_SHA256
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
    assert artifact["provenance_sha256"] == PROVENANCE_SHA256
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
    resource_keys = {
        "buffers",
        "staging_buffers",
        "shader_modules",
        "bind_group_layouts",
        "pipeline_layouts",
        "pipelines",
        "bind_groups",
        "command_buffers",
    }
    assert set(wgpu["created_resources"]) == resource_keys
    assert set(wgpu["live_resources"]) == resource_keys
    assert all(value > 0 for value in wgpu["created_resources"].values())
    _assert_zero_counts(wgpu["live_resources"])
    assert wgpu["live_resource_scope"] == LIVE_RESOURCE_SCOPE
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

    windows = result["oracle_comparison"]["windows"]
    capture = windows["capture"]
    assert capture["host"] == "R5900X"
    assert capture["os"] == "Windows"
    assert capture["transport"] == "Tailscale SSH alias r5900x"
    assert capture["worker"] == {
        "path": "target/minihost-build/aex_smart_worker.exe",
        "sha256": "3abdb1611d1a9b0595a29120644d10421b4eaded9b3dd6a0c97029104f496bbe",
    }
    assert capture["protocol"] == "--smart-session32-opencl-v1"
    assert capture["fixture"]["sha256"] == result["fixture"]["sha256"]
    assert capture["input"]["png_sha256"] == result["fixture"]["input"]["sha256"]
    assert capture["manifest"]["path_kind"] == "host_local_provenance"
    assert SHA256.fullmatch(capture["manifest"]["sha256"])
    assert capture["slot_normalization"] == {
        "physical_layout": "rgba32f",
        "canonical_layout": "argb32f",
        "operation": "reorder each float32 pixel [R,G,B,A] to [A,R,G,B]",
    }
    assert capture["default"]["payload"] == "v2|"
    assert capture["brightness_25"]["payload"] == "v2|brightness@1:f64=25"
    assert capture["brightness_25"]["requested_parameter"] == {
        "id": "brightness",
        "slot": 1,
        "kind": "float",
        "value": 25.0,
    }
    assert (
        capture["default"]["physical_rgba32f_sha256"]
        != windows["default"]["raw_argb32f_sha256"]
    )
    assert (
        capture["brightness_25"]["physical_rgba32f_sha256"]
        != windows["brightness_25"]["raw_argb32f_sha256"]
    )
    for case in ("default", "brightness_25"):
        assert windows[case]["capture_byte_exact"] is True
        assert capture[case]["render_error"] == 0
        assert capture[case]["opencl_context_used"] is True
        assert capture[case]["upload_bytes"] == capture[case]["download_bytes"] == 13616
        assert capture[case]["gpu_memory_lifetimes_balanced"] is True
        assert capture[case]["live_gpu_allocation_count"] == 0
        assert capture[case]["guard_bytes_intact"] is True
        assert SHA256.fullmatch(capture[case]["physical_rgba32f_sha256"])
        assert SHA256.fullmatch(capture[case]["report_sha256"])

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
    assert reproduction["command_scope"] == (
        "host-local invocation records, not clean-checkout reproduction"
    )
    assert reproduction["host_local_artifacts"] == [
        {
            "path": result["fixture"]["path"],
            "sha256": result["fixture"]["sha256"],
        },
        {
            "path": result["fixture"]["input"]["path"],
            "sha256": result["fixture"]["input"]["sha256"],
        },
        {
            "path": result["fixture"]["precompiled_spirv"]["path"],
            "sha256": result["fixture"]["precompiled_spirv"]["sha256"],
        },
    ]
    assert reproduction["prepare_output_directory"] == (
        "mkdir -p target/issue615-acceptance"
    )
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
    assert (
        environment["AEXCOMPAT_WGPU_PRECOMPILED_PROVENANCE_SHA256"]
        == result["fixture"]["precompiled_spirv"]["provenance_sha256"]
        == PROVENANCE_SHA256
    )
    builds = reproduction["build_commands"]
    default_build = builds["default_worker"]
    experimental_build = builds["wgpu_metal_experimental_worker"]
    assert "--no-default-features" in default_build
    assert "--features wgpu-metal-experimental" not in default_build
    assert "--target-dir target/issue615-default-worker" in default_build
    assert "--features wgpu-metal-experimental" in experimental_build
    assert "--target-dir target/issue615-wgpu-worker" in experimental_build

    commands = reproduction["commands"]
    input_path = result["fixture"]["input"]["path"]
    assert all(input_path in command for command in commands.values())
    assert "--render-backend wgpu-metal" in commands["wgpu_default"]
    assert "AEXCOMPAT_WGPU_PRECOMPILED_SPIRV=" in commands["wgpu_default"]
    assert (
        f"AEXCOMPAT_WGPU_PRECOMPILED_PROVENANCE_SHA256={PROVENANCE_SHA256}"
        in commands["wgpu_default"]
    )
    assert "target/issue615-wgpu-worker/release/aex-guest-worker" in commands[
        "wgpu_default"
    ]
    assert "Brightness=25" in commands["wgpu_brightness_25"]
    assert "--render-backend opencl" in commands["apple_opencl_default"]
    assert "target/issue615-default-worker/release/aex-guest-worker" in commands[
        "apple_opencl_default"
    ]
    assert "Brightness=25" in commands["apple_opencl_brightness_25"]
    assert f'"{cpu["fixture"]}"' in commands["olmblur_cpu"]
    assert "--render-backend cpu" in commands["olmblur_cpu"]

    distribution = result["distribution"]
    assert distribution["publication_status"] == "local-only"
    assert distribution["required_feature"] == "wgpu-metal-experimental"
    assert distribution["default_enabled"] is False
    assert distribution["shipping_status"] == "non-shipping"
    assert distribution["product_distribution"] == "hold"
    assert "does not establish" in distribution["hold_reason"]
    dependency_graph = distribution["default_dependency_graph"]
    assert dependency_graph["status"] == "pass"
    assert dependency_graph["excluded_packages"] == [
        "aex-clspv",
        "aex-wgpu-compute",
        "codespan-reporting",
        "spirv",
    ]
    feature_off = distribution["feature_off_wgpu_request"]
    assert feature_off == {
        "status": "pass",
        "result": "explicitly-unsupported",
        "exit_code": 1,
        "fallback_used": False,
        "runtime_active": False,
        "output_created": False,
        "cleanup_complete": True,
    }

    validation = result["validation"]
    assert validation["guest_worker_tests"] == {
        "default_feature_off": {
            "status": "pass",
            "library_passed_tests": 181,
            "cli_passed_tests": 9,
            "total_passed_tests": 190,
        },
        "wgpu_metal_experimental": {
            "status": "pass",
            "library_passed_tests": 187,
            "cli_passed_tests": 9,
            "total_passed_tests": 196,
        },
    }
    assert validation["guest_worker_release_build"] == {
        "default_feature_off": "pass",
        "wgpu_metal_experimental": "pass",
    }
    assert validation["guest_workspace_serial"] == {
        "status": "pass",
        "passed_tests": 233,
        "ignored_tests": 1,
    }
    assert validation["external_clspv_fixture"] == {
        "status": "pass",
        "passed_tests": 1,
    }
    assert validation["python_focused_evidence_contract"] == {
        "status": "pass",
        "passed_tests": 5,
        "failed_tests": 0,
    }
    assert validation["python_full_suite"]["status"] == (
        "known-baseline-or-platform-failures"
    )
    assert validation["python_full_suite"]["issue_615_focused_failures"] == 0
    assert validation["python_full_suite"]["origin_main_baseline"] == {
        "head": BASE_HEAD,
        "passed_tests": 1965,
        "skipped_tests": 200,
        "failed_tests": 68,
        "passed_subtests": 391,
    }
    assert validation["normal_effect_entry_acceptance"] == "pass"
    assert validation["windows_opencl_default_and_brightness_oracle_recapture"] == "pass"
    assert validation["apple_opencl_non_regression"] == "pass"
    assert validation["cpu_olmblur_non_regression"] == "pass"
    assert validation["default_dependency_graph"] == "pass"
    assert validation["macos_x64_native_carrier_check"] == "pass"
    assert validation["windows_x64_bounded_crate_checks"] == {
        "aex-clspv": "pass",
        "aex-wgpu-compute": "pass",
        "full_worker": "not-run: known external Windows CRT header block",
    }
    assert validation["abi_generator_check"] == "pass"
    assert validation["latest_head_review"] == {
        "status": "pending",
        "unresolved_p1": None,
        "unresolved_p2": None,
    }
    assert validation["ci"] == "ignored under the repository billing policy"
