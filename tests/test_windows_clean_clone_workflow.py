from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_windows_clean_clone_runs_canonical_source_reproducible_gates():
    workflow = (ROOT / ".github/workflows/windows-clean-clone.yml").read_text(
        encoding="utf-8"
    )
    assert "runs-on: windows-latest" in workflow
    assert "components: rustfmt" in workflow
    assert """      - name: Check Rust formatting
        working-directory: broker
        run: cargo fmt --all --check
""" in workflow
    assert "cargo check --workspace --locked" in workflow
    assert "cargo check --manifest-path bridges/aviutl2/Cargo.toml --locked" in workflow
    assert "cargo test --workspace --locked" in workflow
    assert "--skip external_worker_reads_sealed_plugin_and_tree_is_cleaned_after_exit" in workflow
    assert "--skip timeout_kills_worker_and_cleans_sealed_and_staged_trees" in workflow
    assert "uv sync --locked" in workflow
    assert "uv run python -m pytest --collect-only -q --validate-local-artifact-manifest" in workflow
    assert "uv run python -m pytest -q" in workflow
    assert "--run-local-artifact-tests" not in workflow
