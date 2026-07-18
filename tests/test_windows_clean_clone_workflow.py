from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_windows_clean_clone_runs_canonical_source_reproducible_gates():
    workflow = (ROOT / ".github/workflows/windows-clean-clone.yml").read_text(
        encoding="utf-8"
    )
    assert "runs-on: windows-latest" in workflow
    assert "cargo check --workspace --locked" in workflow
    assert "cargo test --workspace --locked" in workflow
    assert "python -m pip install -r requirements-dev.txt" in workflow
    assert "python -m pytest --collect-only -q --validate-local-artifact-manifest" in workflow
    assert "python -m pytest -q" in workflow
    assert "--run-local-artifact-tests" not in workflow
