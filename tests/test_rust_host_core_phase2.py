import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKSPACE = ROOT / "broker/Cargo.toml"
CORE_CRATE = ROOT / "broker/crates/host-core"
FFI_CRATE = ROOT / "broker/crates/host-core-ffi"
BROKER_CRATE = ROOT / "broker/crates/broker"
BROKER_REEXPORT_TEST = BROKER_CRATE / "tests/host_core_reexports.rs"
DOC = ROOT / "docs/RUST_HOST_CORE_MIGRATION_2026-07-31.md"

CORE_MODULES = ("boundary", "error", "handle", "report", "session")


class RustHostCorePhase2Tests(unittest.TestCase):
    def test_core_crate_has_a_dependency_minimal_normal_graph(self):
        workspace = WORKSPACE.read_text(encoding="utf-8")
        core_manifest = tomllib.loads(
            (CORE_CRATE / "Cargo.toml").read_text(encoding="utf-8")
        )
        self.assertIn('"crates/host-core"', workspace)
        self.assertEqual(set(core_manifest["dependencies"]), {"serde"})
        self.assertEqual(set(core_manifest["dev-dependencies"]), {"serde_json"})
        manifest_text = (CORE_CRATE / "Cargo.toml").read_text(encoding="utf-8")
        for forbidden in (
            "aexcompat-broker",
            "windows",
            "image",
            "tracing",
            "sha2",
            "wgpu",
        ):
            self.assertNotIn(forbidden, manifest_text)

    def test_ffi_depends_directly_on_core_instead_of_broker(self):
        ffi_manifest = tomllib.loads(
            (FFI_CRATE / "Cargo.toml").read_text(encoding="utf-8")
        )
        self.assertEqual(set(ffi_manifest["dependencies"]), {"aexcompat-host-core"})
        ffi_source = (FFI_CRATE / "src/lib.rs").read_text(encoding="utf-8")
        self.assertIn("use aexcompat_host_core::boundary", ffi_source)
        self.assertNotIn("aexcompat_broker", ffi_source)

    def test_broker_preserves_public_paths_with_true_reexports(self):
        broker_manifest = tomllib.loads(
            (BROKER_CRATE / "Cargo.toml").read_text(encoding="utf-8")
        )
        self.assertIn("aexcompat-host-core", broker_manifest["dependencies"])
        module = (BROKER_CRATE / "src/host_core/mod.rs").read_text(encoding="utf-8")
        self.assertIn(
            "pub use aexcompat_host_core::{boundary, error, handle, report, session};",
            module,
        )
        for name in CORE_MODULES:
            self.assertTrue((CORE_CRATE / f"src/{name}.rs").is_file())
            self.assertFalse((BROKER_CRATE / f"src/host_core/{name}.rs").exists())
        compatibility = BROKER_REEXPORT_TEST.read_text(encoding="utf-8")
        self.assertIn("aexcompat_broker::host_core", compatibility)
        self.assertIn("aexcompat_host_core::boundary::HostCallContext", compatibility)

    def test_extraction_keeps_semantic_and_worker_scopes_out(self):
        library = (CORE_CRATE / "src/lib.rs").read_text(encoding="utf-8")
        for marker in CORE_MODULES:
            self.assertIn(f"pub mod {marker};", library)
        for excluded_module in ("parameter", "descriptor_manifest", "approved_artifact"):
            self.assertNotIn(f"pub mod {excluded_module};", library)
        all_core_source = "\n".join(
            path.read_text(encoding="utf-8")
            for path in sorted((CORE_CRATE / "src").glob("*.rs"))
        )
        for forbidden in ("PF_", "AEGP_", "SPBasic", "windows_sys", "wgpu"):
            self.assertNotIn(forbidden, all_core_source)
        document = DOC.read_text(encoding="utf-8")
        for marker in (
            "Phase 2",
            "Issue #621",
            "dependency-minimal",
            "compatibility",
            "re-export",
            "Production worker routing remains unchanged",
            "#26 / PR #571",
            "#98",
        ):
            self.assertIn(marker, document)


if __name__ == "__main__":
    unittest.main()
