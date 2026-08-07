import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_GLATOR_RUNTIME_POLICY_INSPECT_RESULT_2026-07-18.json"


def test_glator_inspect_uses_exact_purpose_bound_runtime_modules():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    assert result["result"] == "passed"
    assert result["inspect"] == {
        "classification": "ok",
        "runtime_module_policy_applied": True,
        "module_audit_status": "passed",
        "unknown_count": 0,
        "policy_modules": ["nvgpucomp64.dll", "nvoglv64.dll"],
        "global_setup_error": 0,
        "params_setup_error": 0,
        "global_setdown_error": 0,
        "parameter_count": 1,
        "parameter_name": "GLator",
    }
    assert {module["backend"] for module in result["authorized_runtime_modules"]} == {"opengl"}
    assert result["fail_closed_control"] == {
        "omitted_module": "nvgpucomp64.dll",
        "worker_exit_code": 14,
        "first_failure_stage": "global_setup",
        "unknown_count": 1,
        "authorized_policy_modules": ["nvoglv64.dll"],
        "passed": False,
    }
    assert result["privacy"] == {"local_paths_exported": False, "private_stderr_exported": False}
