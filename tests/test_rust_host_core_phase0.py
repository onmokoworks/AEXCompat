import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CORE = ROOT / "broker/crates/host-core/src"
DOC = ROOT / "docs/RUST_HOST_CORE_MIGRATION_2026-07-31.md"
HEADER = ROOT / "broker/crates/broker/include/aexcompat_host_core_abi.h"
NATIVE_SELFTEST = ROOT / "tests/native/rust_host_core_abi_selftest.cpp"
CMAKE = ROOT / "minihost/CMakeLists.txt"


class RustHostCorePhase0Tests(unittest.TestCase):



    def test_migration_document_keeps_native_responsibilities_and_scope(self):
        text = DOC.read_text(encoding="utf-8")
        for marker in (
            "Thin C++ adapter",
            "Windows SEH filter",
            "`catch_unwind`",
            "SDK-shaped callback",
            "raw pointers stay in C++",
            "#98",
            "#26 / PR #571",
            "#614/wgpu/GPU",
            "one Issue and one PR",
        ):
            self.assertIn(marker, text)

if __name__ == "__main__":
    unittest.main()
