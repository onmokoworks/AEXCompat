import os
import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.L2_MAIN
PF_STATE_RUNTIME = ROOT / "minihost" / "src" / "worker_pf_state_runtime.cpp"
SELFTEST_SOURCE = ROOT / "minihost" / "src" / "worker_parameter_selftests.cpp"


def _worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target/minihost-build-v18/Release/aex_render_worker.exe",
        ROOT / "target/minihost-build-v18/aex_render_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)


def test_parameter_selftests_are_a_true_translation_unit():
    worker = SOURCE.read_text(encoding="utf-8")
    implementation = SELFTEST_SOURCE.read_text(encoding="utf-8")
    for name in ("verify_pf_param_utils_suite3",
                 "verify_parameter_animation_transport"):
        marker = f"bool {name}()"
        assert marker in implementation
        assert marker not in worker


def test_param_utils_suite3_has_the_frozen_typed_nine_slot_abi():
    # The ABI struct/table live in the worker-runtime owner set; the catalog
    # entry keeps resolving through l2_main, which the owner set includes.
    source = source_owners.worker_text()
    assert '{"PF Param Utils Suite", 3, &g_param_utils_suite}' in source
    assert "struct ParamUtilsSuite3" in source
    assert "sizeof(ParamUtilsSuite3) == 9 * sizeof(void*)" in source
    assert "offsetof(ParamUtilsSuite3, PF_UpdateParamUI) == 0 * sizeof(void*)" in source
    assert "offsetof(ParamUtilsSuite3, PF_GetCurrentState) == 1 * sizeof(void*)" in source
    assert "offsetof(ParamUtilsSuite3, PF_AreStatesIdentical) == 2 * sizeof(void*)" in source
    assert "offsetof(ParamUtilsSuite3, PF_KeyIndexToTime) == 8 * sizeof(void*)" in source
    initializer = source.split("ParamUtilsSuite3 g_param_utils_suite{", 1)[1].split("};", 1)[0]
    assert initializer.count("&") == 9
    assert "unsupported" not in initializer


def test_param_utils_suite3_integrates_state_and_constant_keyframe_models():
    source = "\n".join(path.read_text(encoding="utf-8") for path in
                       (SOURCE, PF_STATE_RUNTIME, SELFTEST_SOURCE))
    for marker in (
        "canonical_param_state_snapshot",
        "g_pf_state_registry.emplace",
        "constexpr uint32_t kMutableUiFlags",
        "target + kParamName",
            "update_param_ui(g_hooks.effect, 1, local.data()) == 0",
        "effect_ref != &g_effect",
        "g_pf_state_registry.find(first_token)",
        "*count = -1",
        "*found = 0",
        "return kPfInvalidIndex",
        "g_update_params_ui_active && !g_user_changed_param_active",
        "get_current_param_state(nullptr, 1, nullptr, nullptr, &changed)",
        "std::memcmp(&changed, &sentinel, sizeof(changed)) == 0",
    ):
        assert marker in source


def test_param_utils_suite3_native_self_test():
    executable = _worker()
    assert executable is not None, "build aex_render_worker before running the focused runtime test"
    completed = subprocess.run(
        [str(executable), "--self-test-pf-param-utils-suite"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert completed.stdout.strip() == '{"pf_param_utils_suite3":"passed"}'
