from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "render-ae-oracle-project.jsx"


def test_ae_render_fallback_is_bound_to_the_prepared_project_and_output():
    source = SCRIPT.read_text(encoding="utf-8")
    assert 'requiredEnv("AEXCOMPAT_AE_ORACLE_PROJECT")' in source
    assert 'requiredEnv("AEXCOMPAT_AE_ORACLE_OUTPUT")' in source
    assert 'requiredEnv("AEXCOMPAT_AE_ORACLE_RENDER_RESULT")' in source
    assert "renderQueue.numItems !== 1" in source
    assert "outputModule.file.fsName !== outputFile.fsName" in source
    assert "app.project.renderQueue.render()" in source
    assert 'getFiles(outputFile.name + "*")' in source
    assert "sequenceFiles.length === 1" in source
    assert "errorText === null ? renderedFile.fsName : errorText" in source
    assert "CloseOptions.DO_NOT_SAVE_CHANGES" in source
