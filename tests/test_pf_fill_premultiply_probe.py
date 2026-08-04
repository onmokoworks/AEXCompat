from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-fill-premultiply-probe" / "pf_fill_premultiply_probe.cpp"
BUILD = ROOT / "tools" / "build-pf-fill-premultiply-probe.ps1"
RESOURCE = ROOT / "instruments" / "pf-fill-premultiply-probe" / "pf_fill_premultiply_probe.rc"










def test_build_script_is_self_contained_and_reproducible():
    script = BUILD.read_text(encoding="utf-8")
    assert "AE_EffectCBSuites.h" in script
    assert "vcvars64.bat" in script
    assert "cl /nologo /c /std:c++17" in script
    assert "rc /nologo" in script
    assert "link /nologo /DLL" in script
    assert "Get-FileHash" in script and "SHA256" in script

