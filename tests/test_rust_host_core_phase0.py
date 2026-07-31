import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CORE = ROOT / "broker/crates/broker/src/host_core"
DOC = ROOT / "docs/RUST_HOST_CORE_MIGRATION_2026-07-31.md"
HEADER = ROOT / "broker/crates/broker/include/aexcompat_host_core_abi.h"
NATIVE_SELFTEST = ROOT / "tests/native/rust_host_core_abi_selftest.cpp"


class RustHostCorePhase0Tests(unittest.TestCase):
    def test_value_abi_and_containment_anchors_exist(self):
        boundary = (CORE / "boundary.rs").read_text(encoding="utf-8")
        for marker in (
            "#[repr(C)]",
            "#[repr(transparent)]",
            "catch_unwind",
            "HostCallContext",
            "HostCallStatus",
            "HostOpaqueHandle",
            "size_of::<HostCallContext>() == 24",
            "offset_of!(HostCallStatus, report_id) == 16",
        ):
            self.assertIn(marker, boundary)

    def test_c_and_rust_value_abi_contracts_match(self):
        boundary = (CORE / "boundary.rs").read_text(encoding="utf-8")
        errors = (CORE / "error.rs").read_text(encoding="utf-8")
        header = HEADER.read_text(encoding="utf-8")
        native = NATIVE_SELFTEST.read_text(encoding="utf-8")
        for marker in (
            "AEXCOMPAT_HOST_CORE_ABI_VERSION 1u",
            "sizeof(AexHostCallContext) == 24",
            "offsetof(AexHostCallContext, caller_thread_token) == 16",
            "sizeof(AexHostCallStatus) == 24",
            "offsetof(AexHostCallStatus, report_id) == 16",
            "sizeof(AexHostOpaqueHandle) == 8",
        ):
            self.assertIn(marker, header)
            if marker.startswith(("sizeof", "offsetof")):
                self.assertIn(marker, native)
        for rust_name, c_name, value in (
            ("Ok", "AEX_HOST_OK", 0),
            ("InvalidArgument", "AEX_HOST_INVALID_ARGUMENT", 1),
            ("InvalidState", "AEX_HOST_INVALID_STATE", 2),
            ("WrongThread", "AEX_HOST_WRONG_THREAD", 3),
            ("InvalidHandle", "AEX_HOST_INVALID_HANDLE", 4),
            ("WrongOwner", "AEX_HOST_WRONG_OWNER", 5),
            ("WrongKind", "AEX_HOST_WRONG_KIND", 6),
            ("StaleHandle", "AEX_HOST_STALE_HANDLE", 7),
            ("Panic", "AEX_HOST_PANIC", 8),
            ("SehFault", "AEX_HOST_SEH_FAULT", 9),
            ("CapacityExceeded", "AEX_HOST_CAPACITY_EXCEEDED", 10),
        ):
            self.assertIn(f"{rust_name} = {value}", errors)
            self.assertIn(f"{c_name} = {value}", header)
        self.assertIn("size_of::<HostCallContext>() == 24", boundary)
        self.assertNotIn("*", "\n".join(
            line for line in header.splitlines()
            if line.startswith("  ") and not line.startswith("  AEX_")
        ))
        for sdk_marker in ("PF_", "AEGP_", "SPBasic"):
            self.assertNotIn(sdk_marker, header)

    def test_ownership_thread_and_value_only_report_are_explicit(self):
        handle = (CORE / "handle.rs").read_text(encoding="utf-8")
        session = (CORE / "session.rs").read_text(encoding="utf-8")
        report = (CORE / "report.rs").read_text(encoding="utf-8")
        for marker in ("WrongOwner", "WrongKind", "StaleHandle", "generation"):
            self.assertIn(marker, handle)
        for marker in ("WrongThread", "origin_thread", "InCallback", "Faulted"):
            self.assertIn(marker, session)
        for forbidden in ("PathBuf", "*mut ", "*const ", "Vec<u8>"):
            self.assertNotIn(forbidden, report)
        self.assertIn("Value-only diagnostic contract", report)

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
