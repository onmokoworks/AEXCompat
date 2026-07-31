import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKSPACE = ROOT / "broker/Cargo.toml"
CRATE = ROOT / "broker/crates/host-core-ffi"
CORE = ROOT / "broker/crates/broker/src/host_core"
HEADER = ROOT / "broker/crates/broker/include/aexcompat_host_core_abi.h"
NATIVE = ROOT / "tests/native/rust_host_core_ffi_dual_run_selftest.cpp"
CMAKE = ROOT / "minihost/CMakeLists.txt"
DOC = ROOT / "docs/RUST_HOST_CORE_MIGRATION_2026-07-31.md"

EXPORTS = (
    "aex_host_core_session_create_v1",
    "aex_host_core_session_open_v1",
    "aex_host_core_session_begin_callback_v1",
    "aex_host_core_session_end_callback_v1",
    "aex_host_core_session_close_v1",
    "aex_host_core_session_dispose_v1",
)


class RustHostCorePhase1Tests(unittest.TestCase):
    def test_workspace_builds_a_dedicated_cdylib_adapter(self):
        workspace = WORKSPACE.read_text(encoding="utf-8")
        manifest = (CRATE / "Cargo.toml").read_text(encoding="utf-8")
        library = (CRATE / "src/lib.rs").read_text(encoding="utf-8")
        self.assertIn('"crates/host-core-ffi"', workspace)
        self.assertIn('crate-type = ["cdylib", "rlib"]', manifest)
        self.assertIn("aexcompat-broker", manifest)
        for export in EXPORTS:
            self.assertIn(f'pub unsafe extern "C" fn {export}', library)
            self.assertIn(export, HEADER.read_text(encoding="utf-8"))
        self.assertGreaterEqual(library.count("#[unsafe(no_mangle)]"), len(EXPORTS))
        self.assertIn("contain_panic", library)
        self.assertIn("HandleRegistry<SessionRecord>", library)

    def test_report_and_session_values_are_frozen_on_both_sides(self):
        report = (CORE / "report.rs").read_text(encoding="utf-8")
        session = (CORE / "session.rs").read_text(encoding="utf-8")
        header = HEADER.read_text(encoding="utf-8")
        for marker in (
            "pub struct HostReportSnapshot",
            "size_of::<HostReportSnapshot>() == 72",
            "offset_of!(HostReportSnapshot, error_code) == 20",
            "offset_of!(HostReportSnapshot, report_id) == 32",
            "offset_of!(HostReportSnapshot, callbacks_completed) == 64",
        ):
            self.assertIn(marker, report)
        for marker in (
            "Created = 1",
            "Open = 2",
            "InCallback = 3",
            "Closed = 4",
            "Faulted = 5",
        ):
            self.assertIn(marker, session)
        for marker in (
            "AEXCOMPAT_HOST_CORE_CALL __cdecl",
            "sizeof(AexHostReportSnapshot) == 72",
            "offsetof(AexHostReportSnapshot, error_code) == 20",
            "offsetof(AexHostReportSnapshot, report_id) == 32",
            "offsetof(AexHostReportSnapshot, callbacks_completed) == 64",
            "AEX_HOST_REPORT_PHASE_SESSION = 3",
            "AEX_HOST_REPORT_OUTCOME_FAULTED = 3",
            "AEX_HOST_HANDLE_KIND_SESSION = 4",
            "AEX_HOST_SESSION_STATE_FAULTED = 5",
        ):
            self.assertIn(marker, header)

    def test_native_test_is_a_dynamic_seh_contained_dual_run(self):
        native = NATIVE.read_text(encoding="utf-8")
        for export in EXPORTS:
            self.assertIn(f'"{export}"', native)
        for marker in (
            "NativeSessionOracle",
            "LoadLibraryW",
            "GetProcAddress",
            "__try",
            "EXCEPTION_EXECUTE_HANDLER",
            "create/bad-version",
            "create/bad-size",
            "open/invalid-handle",
            "open/foreign-thread",
            "dispose/wrong-owner",
            "open/stale",
            "callbacks_attempted",
            "callbacks_completed",
        ):
            self.assertIn(marker, native)
        cmake = CMAKE.read_text(encoding="utf-8")
        for marker in (
            "add_executable(rust_host_core_ffi_dual_run_selftest",
            "../tests/native/rust_host_core_ffi_dual_run_selftest.cpp",
            "../broker/crates/broker/include",
        ):
            self.assertIn(marker, cmake)

    def test_sdk_shapes_and_production_routing_remain_outside_rust(self):
        adapter_text = "\n".join(
            path.read_text(encoding="utf-8")
            for path in (
                CRATE / "src/lib.rs",
                HEADER,
            )
        )
        for sdk_marker in ("PF_", "AEGP_", "SPBasic", "PF_Err"):
            self.assertNotIn(sdk_marker, adapter_text)
        document = DOC.read_text(encoding="utf-8")
        for marker in (
            "Issue #619",
            "Phase 1",
            "dynamic loader",
            "normalized lifecycle",
            "production worker",
            "routing remains unchanged",
            "#26 / PR #571",
            "#98",
            "#614/wgpu/GPU",
        ):
            self.assertIn(marker, document)


if __name__ == "__main__":
    unittest.main()
