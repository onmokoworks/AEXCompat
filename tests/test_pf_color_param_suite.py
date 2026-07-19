import os
import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.L2_MAIN
PARAM_SUITES = ROOT / "minihost" / "src" / "worker_pf_param_suites.cpp"
SELFTEST_SOURCE = ROOT / "minihost" / "src" / "worker_pf_color_selftests.cpp"


def _worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target/minihost-build-v18/Release/aex_render_worker.exe",
        ROOT / "target/minihost-build-v18/aex_render_worker.exe",
    ]
    return next((candidate for candidate in candidates if candidate and candidate.is_file()), None)


def test_color_param_suite_is_exact_typed_frozen_v1_abi():
    # The catalog entry stays in l2_main; the ABI struct and asserts live in
    # the worker-runtime owner set.
    assert ('{"PF ColorParamSuite", 1, &g_color_param_suite1}'
            in SOURCE.read_text(encoding="utf-8"))
    text = source_owners.worker_text()
    assert "struct PfColorParamSuite1" in text
    assert "sizeof(PfColorParamSuite1) == 1 * sizeof(void*)" in text
    assert "offsetof(PfColorParamSuite1, PF_GetFloatingPointColorFromColorDef) ==" in text
    assert ('release_suite("PF ColorParamSuite", 1)'
            in SELFTEST_SOURCE.read_text(encoding="utf-8"))


def test_color_param_contract_is_stateful_depth_aware_and_fail_closed():
    text = SOURCE.read_text(encoding="utf-8") + PARAM_SUITES.read_text(encoding="utf-8")
    # The production callback contract stays in l2_main.
    for marker in (
        "current_float_color",
        "default_float_color",
        "value == found->current_color",
        "value == found->default_color",
        "return kPfInvalidIndex",
        "return kPfUnrecognizedParamType",
        "effect_ref != &g_effect || !definition || !output",
    ):
        assert marker in text
    assert "PfColorParamPixelFloat result{(*resolved)[0], (*resolved)[1]," in text
    # The self-test body lives in its owner translation unit.
    selftest = SELFTEST_SOURCE.read_text(encoding="utf-8")
    for marker in (
        "out.red == 4097.0f / 32768.0f",
        "out.red == 1.5f",
        "std::memcmp(&out, &sentinel, sizeof(out)) == 0",
    ):
        assert marker in selftest
        assert marker not in text


def test_pf_color_param_suite_native_self_test():
    executable = _worker()
    assert executable is not None, "build aex_render_worker before running the focused runtime test"
    completed = subprocess.run(
        [str(executable), "--self-test-pf-color-param-suite"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert completed.stdout.strip() == '{"pf_color_param_suite":"passed"}'
