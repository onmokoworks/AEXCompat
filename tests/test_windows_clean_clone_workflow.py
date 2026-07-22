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
    assert (
        "cargo check --manifest-path bridges/aviutl2/Cargo.toml --all-targets --locked"
        in workflow
    )
    assert (
        "cargo check --manifest-path bridges/aviutl2-multifilter/Cargo.toml --all-targets --locked"
        in workflow
    )
    assert "cargo test --workspace --locked" in workflow
    # The hosted runner's restricted token cannot initialize a worker, so the
    # tests that drive one detect that themselves and report a skip (issue #335).
    # The workflow must not carry a hand-maintained --skip list again: that list
    # did not follow the tests added after it was written, and main went red with
    # 32 failures nobody had opted out of.
    directives = "\n".join(
        line for line in workflow.splitlines() if not line.lstrip().startswith("#"))
    assert "--skip " not in directives
    for name in (
            "external_worker_reads_sealed_plugin_and_tree_is_cleaned_after_exit",
            "timeout_kills_worker_and_cleans_sealed_and_staged_trees"):
        assert name not in directives, (
            f"{name} is skipped by the test itself now, not by the workflow")
        guard = (ROOT / "broker/crates/broker/tests/secure_launch.rs").read_text(encoding="utf-8")
        anchor = guard.index(f"fn {name}(")
        assert "skip_without_restricted_token_launch" in guard[anchor:anchor + 400], (
            f"{name} lost its restricted-token guard; the workflow no longer skips it")
    assert "uv sync --locked" in workflow
    assert "uv run python -m pytest --collect-only -q --validate-local-artifact-manifest" in workflow
    assert "uv run python -m pytest -q" in workflow
    assert "--run-local-artifact-tests" not in workflow
