from pathlib import Path
import tomllib


REPO_ROOT = Path(__file__).resolve().parents[1]


def test_windows_formatting_uses_repository_rust_toolchain() -> None:
    workflow = (
        REPO_ROOT / ".github" / "workflows" / "windows-clean-clone.yml"
    ).read_text(encoding="utf-8")
    toolchain = tomllib.loads(
        (REPO_ROOT / "rust-toolchain.toml").read_text(encoding="utf-8")
    )["toolchain"]

    assert toolchain["channel"]
    assert "rustfmt" in toolchain["components"]
    assert "Select-String -Path rust-toolchain.toml" in workflow
    assert "toolchain: ${{ steps.repository-rust.outputs.channel }}" in workflow
    assert "components: rustfmt" in workflow
    assert "rustfmt --edition 2024 --check" in workflow
