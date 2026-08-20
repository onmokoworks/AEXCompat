import hashlib
from pathlib import Path
import tomllib


REPO_ROOT = Path(__file__).resolve().parents[1]
CANONICAL_MPL_2_SHA256 = (
    "3f3d9e0024b1921b067d6f7f88deb4a60cbe7a78e76c64e3f1d7fc3b779b9d04"
)

EXPLICIT_MPL_MANIFESTS = {
    "bridges/aviutl2-multifilter/Cargo.toml",
    "bridges/ymm4-native/Cargo.toml",
    "guest/crates/aex-apple-opencl/Cargo.toml",
    "guest/crates/aex-clspv/Cargo.toml",
    "guest/crates/aex-guest-worker/Cargo.toml",
    "guest/crates/aex-unicorn-buffer/Cargo.toml",
    "guest/crates/aex-wgpu-compute/Cargo.toml",
    "instruments/pf-wgpu-dx12-probe/runtime/Cargo.toml",
}
WORKSPACE_MPL_MANIFESTS = {
    "broker/Cargo.toml",
    "guest/Cargo.toml",
}
INHERITED_MPL_MANIFESTS = {
    "broker/crates/broker/Cargo.toml",
    "broker/crates/dummy-workers/Cargo.toml",
    "broker/crates/harness/Cargo.toml",
    "broker/crates/host-core-ffi/Cargo.toml",
    "broker/crates/host-core/Cargo.toml",
    "guest/crates/aex-abi/Cargo.toml",
}


def _manifest(relative_path: str) -> dict:
    return tomllib.loads((REPO_ROOT / relative_path).read_text(encoding="utf-8"))


def test_root_license_is_canonical_mpl_2_text() -> None:
    license_bytes = (REPO_ROOT / "LICENSE").read_bytes()

    assert hashlib.sha256(license_bytes).hexdigest() == CANONICAL_MPL_2_SHA256
    assert b"Mozilla Public License Version 2.0" in license_bytes
    assert b"Exhibit B - \"Incompatible With Secondary Licenses\" Notice" in license_bytes


def test_all_owned_cargo_packages_resolve_to_mpl_2() -> None:
    tracked_manifests = {
        path.relative_to(REPO_ROOT).as_posix()
        for path in REPO_ROOT.rglob("Cargo.toml")
        if "target" not in path.parts
    }
    expected_manifests = (
        EXPLICIT_MPL_MANIFESTS
        | WORKSPACE_MPL_MANIFESTS
        | INHERITED_MPL_MANIFESTS
    )
    assert tracked_manifests == expected_manifests

    for relative_path in EXPLICIT_MPL_MANIFESTS:
        assert _manifest(relative_path)["package"]["license"] == "MPL-2.0"

    for relative_path in WORKSPACE_MPL_MANIFESTS:
        assert _manifest(relative_path)["workspace"]["package"]["license"] == "MPL-2.0"

    for relative_path in INHERITED_MPL_MANIFESTS:
        assert _manifest(relative_path)["package"]["license"] == {"workspace": True}


def test_public_docs_state_source_and_combined_binary_boundaries() -> None:
    readme = (REPO_ROOT / "README.md").read_text(encoding="utf-8")
    audit = (REPO_ROOT / "docs" / "PUBLIC_RELEASE_AUDIT.md").read_text(
        encoding="utf-8"
    )
    contributing = (REPO_ROOT / "CONTRIBUTING.md").read_text(encoding="utf-8")

    for required in (
        "Mozilla Public License 2.0",
        "Incompatible With Secondary Licenses",
        "imports/aviutlas-rust-contracts/",
        "private/commercial AEX corpus",
        "Unicorn Engine",
        "section 3.3",
        "additionally distributed under GPL-2.0",
        "corresponding source/build information",
    ):
        assert required in readme

    assert "No `LICENSE` file is added" not in audit
    assert "No root license grant found" not in audit
    assert "MPL-2.0" in audit
    assert "provenance-identified copies" in audit
    assert "GPLv2 Unicorn" in audit

    assert "AEXCompat-authored source and documentation are contributed under MPL-2.0" in contributing
    assert "private/commercial corpus files" in contributing
    assert "GPL-2.0 distribution conditions" in contributing
