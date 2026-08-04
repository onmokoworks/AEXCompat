import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BOUNDARY = ROOT / "broker/crates/host-core/src/boundary.rs"
FFI = ROOT / "broker/crates/host-core-ffi/src/lib.rs"
ABI_HEADER = ROOT / "broker/crates/broker/include/aexcompat_host_core_abi.h"
ADAPTER = ROOT / "broker/crates/broker/include/aexcompat_host_core_adapter.hpp"
ABI_SELFTEST = ROOT / "tests/native/rust_host_core_abi_selftest.cpp"
DUAL_RUN = ROOT / "tests/native/rust_host_core_ffi_dual_run_selftest.cpp"
DOC = ROOT / "docs/RUST_HOST_CORE_MIGRATION_2026-07-31.md"


class RustHostCorePhase4Tests(unittest.TestCase):




    def test_phase4_document_stays_outside_semantics_and_routing(self):
        document = " ".join(DOC.read_text(encoding="utf-8").split())
        for marker in (
            "Phase 4 pre-cast ABI identity gate (Issue #626)",
            "pointer-free ABI descriptor",
            "before it resolves or casts any of the six function exports",
            "does not negotiate report meaning",
            "does not",
            "route a production worker",
            "#26 / PR #571",
            "#98",
            "#614/wgpu/GPU",
        ):
            self.assertIn(marker, document)


if __name__ == "__main__":
    unittest.main()
