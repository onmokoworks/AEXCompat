from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-fill-premultiply-probe" / "pf_fill_premultiply_probe.cpp"
BUILD = ROOT / "tools" / "build-pf-fill-premultiply-probe.ps1"
RESOURCE = ROOT / "instruments" / "pf-fill-premultiply-probe" / "pf_fill_premultiply_probe.rc"


def test_probe_acquires_frozen_suite_v2_during_classic_render():
    source = SOURCE.read_text(encoding="utf-8")
    assert "case PF_Cmd_RENDER:" in source
    assert "AcquireSuite(" in source
    assert "kPFFillMatteSuite" in source
    assert "kPFFillMatteSuiteVersion2" in source
    assert "ReleaseSuite(" in source


def test_probe_exposes_all_premultiply_entry_points_and_directions():
    source = SOURCE.read_text(encoding="utf-8")
    for call in (
        "suite->premultiply(",
        "suite->premultiply_color(",
        "suite->premultiply_color16(",
        "suite->premultiply_color_float(",
    ):
        assert call in source
    assert '"forward|reverse"' in source
    assert '"in-place|separate source/destination"' in source


def test_fixed_vectors_cover_alpha_rounding_and_color_alpha_difference():
    source = SOURCE.read_text(encoding="utf-8")
    assert "0.0, 1.0 / 255.0, 0.5, 128.0 / 255.0, 1.0" in source
    assert "0.501" in source and "0.499" in source
    assert "{64, 204, 102, 153}" in source
    assert "PF_MAX_CHAN16" in source


def test_probe_is_deep_and_float_aware_and_has_pipl():
    source = SOURCE.read_text(encoding="utf-8")
    resource = RESOURCE.read_text(encoding="utf-8")
    assert "PF_OutFlag_DEEP_COLOR_AWARE" in source
    assert "PF_OutFlag2_FLOAT_COLOR_AWARE" in source
    assert "EffectMain" in resource
    assert "AEXCompat PF Fill Premultiply" in resource


def test_build_script_is_self_contained_and_reproducible():
    script = BUILD.read_text(encoding="utf-8")
    assert "AE_EffectCBSuites.h" in script
    assert "vcvars64.bat" in script
    assert "cl /nologo /c /std:c++17" in script
    assert "rc /nologo" in script
    assert "link /nologo /DLL" in script
    assert "Get-FileHash" in script and "SHA256" in script

