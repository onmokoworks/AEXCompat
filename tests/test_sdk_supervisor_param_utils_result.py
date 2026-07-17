import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_SUPERVISOR_PARAM_UTILS_RESULT_2026-07-16.json"
SCRIPT = ROOT / "tools" / "build-sdk-supervisor.ps1"
PROPS = ROOT / "tools" / "sdk-fixtures" / "supervisor" / "supervisor-v143.props"
AEX = ROOT / "target" / "sdk-fixtures" / "supervisor" / "Supervisor.aex"


def load_result():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def test_supervisor_build_contract_keeps_sdk_read_only():
    script = SCRIPT.read_text(encoding="utf-8")
    props = PROPS.read_text(encoding="utf-8")
    assert "PlatformToolset=v143" in script
    assert "sdk_source_unchanged" in script
    assert "SDK source changed during build" in script
    assert "CustomBuild Update" in props
    assert "ExcludedFromBuild>true" in props
    assert "ResourceCompile Remove" in props
    assert "AEXCompatSupervisorPiPLRc" in props


def test_real_supervisor_artifact_and_param_utils_selftest_are_authenticated():
    result = load_result()
    fixture = result["fixture"]
    assert fixture["sdk_source_unchanged"] is True
    assert AEX.is_file()
    assert AEX.stat().st_size == fixture["artifact_size"]
    assert hashlib.sha256(AEX.read_bytes()).hexdigest() == fixture["artifact_sha256"]
    assert result["param_utils_selftest"]["exit_code"] == 0
    assert result["param_utils_selftest"]["report"]["pf_param_utils_suite3"] == "passed"


def test_supervisor_e2e_completes_lifecycle_state_ui_and_render_contracts():
    result = load_result()
    basic = result["e2e"]["l2_basic"]
    changed = result["e2e"]["user_changed_checkbox"]
    render = result["e2e"]["classic_render"]
    assert result["status"] == "passed"
    assert basic["exit_code"] == changed["exit_code"] == render["exit_code"] == 0
    assert basic["sequence_setup_error"] == basic["update_params_ui_error"] == 0
    assert basic["pf_get_current_state_calls"] >= 2
    assert basic["pf_are_states_identical_calls"] >= 1
    assert basic["suite_leases_balanced"] is True
    assert changed["user_changed_param_error"] == 0
    assert changed["update_param_ui_calls"] >= 3
    assert render["render_error"] == 0 and render["render_completed"] is True
    assert render["output_size"] == 16
    safety = result["safety_and_reporting"]
    assert safety["isolated_workers"] is True
    assert safety["suite_leases_balanced"] is True
    assert safety["guard_bytes_intact"] is True


def test_supervisor_lifecycle_precedes_conditional_ui_in_l2_and_render_paths():
    source = (ROOT / "minihost/src/l2_main.cpp").read_text(encoding="utf-8")
    l2_sequence = source.index('std::cerr << "stage:sequence_setup_begin', source.index("lifecycle_errors"))
    l2_ui = source.index("dispatch_conditional_ui_selectors", l2_sequence)
    assert l2_sequence < l2_ui
    classic = source.index("int32_t render_once(")
    classic_sequence = source.index("begin_render_lifecycle", classic)
    classic_ui = source.index("dispatch_conditional_ui_selectors", classic)
    assert classic_sequence < classic_ui
