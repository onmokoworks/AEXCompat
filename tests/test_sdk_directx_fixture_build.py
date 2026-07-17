from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "build-sdk-invert-directx.ps1"


def test_directx_fixture_script_is_non_mutating_and_reproducible():
    source = SCRIPT.read_text(encoding="utf-8")
    assert "SDK_Invert_ProcAmp_Kernel.chlsl" in source
    assert "GF_DEVICE_TARGET_HLSL=1" in source
    assert "-T cs_6_5" in source
    assert "-enable-16bit-types" in source
    assert '"ProcAmp2Kernel", "InvertColorKernel"' in source
    assert 'Join-Path $OutputDirectory "DirectX_Assets"' in source
    assert "/DHAS_HLSL=1" in source
    assert "Get-FileHash" in source
    assert "Set-Content -LiteralPath $EvidencePath" in source
    assert "Set-Content -LiteralPath $kernelSource" not in source
