from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
JSX = ROOT / "tools" / "ae-oracle-project.jsx"
RUNNER = ROOT / "tools" / "prepare-ae-oracle-project.ps1"


def test_oracle_jsx_is_fail_closed_and_prepares_exr_render_queue():
    source = JSX.read_text(encoding="utf-8")
    assert "app.project.numItems !== 0 || app.project.file !== null" in source
    assert "CloseOptions.DO_NOT_SAVE_CHANGES" in source
    assert "app.quit()" in source
    assert "bitsPerChannel = depth" in source
    assert "addProperty(effectName)" in source
    assert "renderQueue.items.add(comp)" in source
    assert "OpenEXR" in source
    assert '"Depth": "Floating Point"' in source
    assert 'status: "prepared"' in source


def test_oracle_runner_refuses_live_ae_and_uses_isolated_launch():
    source = RUNNER.read_text(encoding="utf-8")
    assert "Get-Process AfterFX,aerender,aerendercore" in source
    assert "Join-Path (Split-Path -Parent $afterEffectsPath) 'AfterFX.com'" in source
    assert "Start-Process -FilePath $scriptHostPath" in source
    assert "@('-m', '-noui', '-r', $scriptPath)" in source
    assert "$process.WaitForExit" in source
    assert "Stop-Process -Id $process.Id -Force" in source
    assert "Refusing to overwrite existing output" in source
    assert "$result.status -ne 'prepared'" in source
    assert "did not confirm full-float straight-alpha OpenEXR settings" in source
    assert "Remove-Item \"Env:$_\"" in source


def test_oracle_runner_does_not_invoke_aerender_or_render_project():
    source = RUNNER.read_text(encoding="utf-8")
    assert "Start-Process -FilePath $scriptHostPath" in source
    assert "aerender.exe" not in source
    assert "'-project'" not in source
