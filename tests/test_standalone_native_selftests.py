"""Execute standalone native self-tests that are part of the default build.

These binaries test production components directly, but historically nothing
launched them after CMake built them.  Keep every target visible as a separate
pytest node so a failing executable is named in CI rather than hidden behind a
single aggregate runner.
"""

import json
import os
import subprocess

import pytest

from _native_selftest import ROOT, locate, run


@pytest.mark.parametrize(
    ("executable", "report_key", "expected"),
    (
        pytest.param(
            "aex_string_table_selftest.exe",
            "aex_string_table",
            {"readonly_sections_only": True, "duplicate_fail_closed": True},
            id="aex-string-table",
        ),
        pytest.param(
            "extended_inter_memory_selftest.exe",
            "extended_inter_memory",
            {
                "zero_size": True,
                "foreign_free_rejected": True,
                "failed_alloc_clears": True,
            },
            id="extended-inter-memory",
        ),
        pytest.param(
            "worker_report_json_selftest.exe",
            "worker_report_json",
            {"nonfinite_values": "null"},
            id="worker-report-json",
        ),
        pytest.param(
            "worker_pf_bad_callback_param_selftest.exe",
            "pf_bad_callback_param",
            {},
            id="worker-pf-bad-callback-param",
        ),
        pytest.param(
            "worker_aegp_scene_model_selftest.exe",
            "scene_model",
            {
                "aligned_tokens": True,
                "cross_registry_rejected": True,
                "stale_token_reuse_rejected": True,
                "transaction_cancel_byte_invariant": True,
                "foreign_rejected": True,
            },
            id="worker-aegp-scene-model",
        ),
        pytest.param(
            "worker_suite_registry_selftest.exe",
            "suite_registry_bounds",
            {
                "maximum_input_name_bytes": 96,
                "maximum_telemetry_name_bytes": 64,
                "maximum_version": 65535,
                "maximum_timeline_events": 512,
                "fail_closed": True,
            },
            id="worker-suite-registry",
        ),
    ),
)
def test_json_standalone_selftest(executable, report_key, expected):
    report = run(executable, report_key)
    assert {key: report[key] for key in expected} == expected


@pytest.mark.parametrize(
    "executable",
    (
        pytest.param("l2_mode_execution_selftest.exe", id="l2-mode-execution"),
        pytest.param("rust_host_core_abi_selftest.exe", id="rust-host-core-abi"),
        pytest.param(
            "worker_aegp_entry_guard_selftest.exe",
            id="worker-aegp-entry-guard",
        ),
        pytest.param(
            "worker_aegp_init_runtime_selftest.exe",
            id="worker-aegp-init-runtime",
        ),
        pytest.param(
            "worker_handle_runtime_selftest.exe",
            id="worker-handle-runtime",
        ),
        pytest.param(
            "worker_openmp_policy_selftest.exe",
            id="worker-openmp-policy",
        ),
    ),
)
def test_exit_only_standalone_selftest(executable):
    completed = subprocess.run(
        [str(locate(executable))],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, {
        "executable": executable,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


def test_temporal_checkout_report_standalone_selftest():
    completed = subprocess.run(
        [str(locate("worker_temporal_checkout_report_selftest.exe"))],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, {
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }
    assert json.loads(completed.stdout) == {
        "rejected_temporal_param_checkouts": 17,
        "rejected_temporal_layer_checkouts": 17,
        "rejected_temporal_parameter_checkouts": 23,
    }


def test_rust_host_core_ffi_dual_run_selftest():
    profile = os.environ.get("AEXCOMPAT_CARGO_PROFILE", "release")
    host_core_ffi = ROOT / "broker" / "target" / profile / "aexcompat_host_core_ffi.dll"
    assert host_core_ffi.is_file(), f"missing host-core FFI DLL: {host_core_ffi}"
    completed = subprocess.run(
        [str(locate("rust_host_core_ffi_dual_run_selftest.exe")), str(host_core_ffi)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, {
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }
    assert completed.stdout.strip() == "rust_host_core_ffi_dual_run_selftest: ok"
