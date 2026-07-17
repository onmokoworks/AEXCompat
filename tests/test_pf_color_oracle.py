import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments/pf-color-oracle/pf_color_oracle.cpp"
RC = ROOT / "instruments/pf-color-oracle/pf_color_oracle.rc"
BUILD = ROOT / "tools/build-pf-color-oracle.ps1"
RUNNER = ROOT / "tools/ae-color-oracle-run.jsx"
ATTEMPT = ROOT / "analysis/PF_COLOR_AE_ORACLE_ATTEMPT_2026-07-16.json"


def test_pf_color_oracle_builds():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(BUILD)],
                   cwd=ROOT, check=True, timeout=180)
    assert (ROOT / "target/pf-color-oracle-build/Release/pf_color_oracle.aex").is_file()


def test_probe_covers_suites_legacy_vectors_and_safe_bits():
    text = SOURCE.read_text(encoding="utf-8")
    for marker in ("kPFColorCallbacksSuite", "kPFColorCallbacks16Suite",
                   "kPFColorCallbacksFloatSuite", "in->utils->colorCB", "RGBtoHLS",
                   "HLStoRGB", "RGBtoYIQ", "YIQtoRGB", "Luminance", "Hue",
                   "Lightness", "Saturation", '"negative"', '"over_one"', '"nan"',
                   '"infinity"', "hex_bits", "write_atomic"):
        assert marker in text
    for marker in ("checked_world_bytes", "callbacks_complete", "\\\"release\\\":{" ,
                   "PF_OutFlag2_SUPPORTS_THREADED_RENDERING", "achromatic_minus_1lsb",
                   "achromatic_plus_1lsb", '"halfway"', "positive_zero",
                   "negative_zero", "positive_subnormal", "negative_subnormal",
                   "min_normal", "nextafter_half_down", "nextafter_half_up",
                   "max_finite", "manual_inverse", "manual_gray_half", "manual_chroma"):
        assert marker in text
    assert "if (!out) return PF_Err_BAD_CALLBACK_PARAM" in text
    assert "leases.suite8 = !a8;" in text
    assert "leases.suite16 = !a16;" in text
    assert "leases.suite_float = !af;" in text
    for marker in ("struct ColorSuiteLeases", "~ColorSuiteLeases() noexcept",
                   "leases.release_all()", "release_float_attempted",
                   "release16_attempted", "release8_attempted",
                   "catch (const std::bad_alloc&)", "PF_Err_OUT_OF_MEMORY",
                   "catch (...)", "PF_Err_INTERNAL_STRUCT_DAMAGED"):
        assert marker in text
    release_body = text[text.index("void release_all() noexcept"):text.index("~ColorSuiteLeases")]
    assert release_body.index("if (suite_float)") < release_body.index("if (suite16)")
    assert release_body.index("if (suite16)") < release_body.index("if (suite8)")
    assert "PF Color Oracle Probe" in RC.read_text(encoding="utf-8")


def test_runner_and_attempt_do_not_claim_uncaptured_values():
    runner = RUNNER.read_text(encoding="utf-8")
    assert "bitsPerChannel" in runner and "oracle_not_captured" in runner
    for marker in ("canAddProperty", "can_add_property", "effect.name", "effect.matchName",
                   "depth_results", "native.remove()"):
        assert marker in runner
    attempt = json.loads(ATTEMPT.read_text(encoding="utf-8"))
    assert attempt["status"] == "oracle_not_captured"
    assert attempt["captured_values"] is None
