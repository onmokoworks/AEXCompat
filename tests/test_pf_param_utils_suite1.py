import os
import re
import subprocess
from pathlib import Path

import pytest
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.L2_MAIN
STATE_SOURCE = ROOT / "minihost/src/worker_pf_state_runtime.cpp"
SELFTEST_SOURCE = ROOT / "minihost/src/worker_parameter_selftests.cpp"
SDK_ROOT = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
SDK = Path(SDK_ROOT) / "Examples" / "Headers" / "AE_EffectSuitesOld.h" if SDK_ROOT else None


def _sdk_header() -> Path:
    if SDK is None or not SDK.is_file():
        pytest.skip("set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root")
    return SDK


def _worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target/minihost-build-v18/Release/aex_render_worker.exe",
        ROOT / "target/minihost-build-v18/aex_render_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)


def test_sdk_freezes_param_utils_suite1_at_acquisition_version_2_with_ten_slots():
    sdk = _sdk_header().read_text(encoding="utf-8", errors="replace")
    assert re.search(r"#define\s+kPFParamUtilsSuiteVersion1\s+2\b", sdk)
    table = sdk.split("typedef struct PF_ParamUtilsSuite1 {", 1)[1].split(
        "} PF_ParamUtilsSuite1;", 1
    )[0]
    slots = re.findall(r"\(\*PF_(\w+)\)\(", table)
    assert slots == [
        "UpdateParamUI", "GetCurrentStateObsolete", "HasParamChangedObsolete",
        "HaveInputsChangedOverTimeSpanObsolete", "IsIdenticalCheckout",
        "FindKeyframeTime", "GetKeyframeCount", "CheckoutKeyframe",
        "CheckinKeyframe", "KeyIndexToTime",
    ]


def test_suite1_has_a_distinct_typed_old_abi_and_all_ten_contract_slots():
    source = "\n".join((source_owners.worker_text(),
                        STATE_SOURCE.read_text(encoding="utf-8"),
                        SELFTEST_SOURCE.read_text(encoding="utf-8")))
    assert '{"PF Param Utils Suite", 2, &g_param_utils_suite1}' in source
    assert "struct ParamUtilsSuite1" in source
    assert "sizeof(ParamUtilsSuite1) == 10 * sizeof(void*)" in source
    assert "acquired_v1 != acquired" in source
    initializer = source.split("ParamUtilsSuite1 g_param_utils_suite1{", 1)[1].split("};", 1)[0]
    expected = [
        "update_param_ui", "get_current_param_state_obsolete",
        "has_param_changed_obsolete", "have_inputs_changed_over_time_span_obsolete",
        "is_identical_param_checkout", "find_param_keyframe_time",
        "get_param_keyframe_count", "checkout_param_keyframe",
        "checkin_param_keyframe", "param_key_index_to_time",
    ]
    assert re.findall(r"&(\w+)", initializer) == expected
    assert "*changed = 1;" in source
    assert "valid_obsolete_param_state(owner, state)" in source
    assert "found->second.owner == owner" in source


def test_suite1_and_suite3_native_contracts_pass_together():
    executable = _worker()
    assert executable is not None, "build aex_render_worker before running the focused runtime test"
    completed = subprocess.run(
        [str(executable), "--self-test-pf-param-utils-suite"], cwd=ROOT,
        text=True, capture_output=True, timeout=30, check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert completed.stdout.strip() == '{"pf_param_utils_suite3":"passed"}'
