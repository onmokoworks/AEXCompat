import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ADAPTER = ROOT / "broker/crates/broker/include/aexcompat_host_core_adapter.hpp"
ABI_HEADER = ROOT / "broker/crates/broker/include/aexcompat_host_core_abi.h"
NATIVE = ROOT / "tests/native/rust_host_core_ffi_dual_run_selftest.cpp"
DOC = ROOT / "docs/RUST_HOST_CORE_MIGRATION_2026-07-31.md"

EXPORTS = (
    "aex_host_core_session_create_v1",
    "aex_host_core_session_open_v1",
    "aex_host_core_session_begin_callback_v1",
    "aex_host_core_session_end_callback_v1",
    "aex_host_core_session_close_v1",
    "aex_host_core_session_dispose_v1",
)


class RustHostCorePhase3Tests(unittest.TestCase):
    def test_adapter_owns_absolute_loading_complete_exports_and_lifetime(self):
        adapter = ADAPTER.read_text(encoding="utf-8")
        self.assertIn('#include "aexcompat_host_core_abi.h"', adapter)
        self.assertIn("class AdapterV1", adapter)
        self.assertIn("struct ApiV1", adapter)
        self.assertIn("GetFullPathNameW", adapter)
        self.assertIn("LoadLibraryExW", adapter)
        self.assertIn("LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR", adapter)
        self.assertIn("LOAD_LIBRARY_SEARCH_SYSTEM32", adapter)
        self.assertIn("GetProcAddress", adapter)
        self.assertIn("FreeLibrary", adapter)
        self.assertIn("AdapterV1(const AdapterV1 &) = delete", adapter)
        self.assertIn("AdapterV1(AdapterV1 &&other) noexcept", adapter)
        self.assertIn("a Windows MSVC-compatible SEH compiler", adapter)
        for export in EXPORTS:
            self.assertIn(f'"{export}"', adapter)

    def test_adapter_contains_seh_and_normalizes_a_stable_fault_report(self):
        adapter = ADAPTER.read_text(encoding="utf-8")
        for marker in (
            "InvokeCreateRaw",
            "InvokeSessionRaw",
            "Invocation Create(AexHostCallContext context)",
            "AexHostOpaqueHandle created_handle{}",
            "adapter-owned storage inside the SEH frame",
            "__try",
            "__except (EXCEPTION_EXECUTE_HANDLER)",
            "GetExceptionCode",
            "session->value = 0",
            "NormalizeBoundaryFailure(AEX_HOST_SEH_FAULT, true",
            "AEX_HOST_REPORT_PHASE_BOUNDARY",
            "AEX_HOST_REPORT_OUTCOME_FAULTED",
            "AEX_HOST_SEH_FAULT",
            "report_id zero identifies an adapter-local failure",
        ):
            self.assertIn(marker, adapter)

    def test_dual_run_consumes_adapter_and_injects_an_actual_seh(self):
        native = NATIVE.read_text(encoding="utf-8")
        self.assertIn('#include "aexcompat_host_core_adapter.hpp"', native)
        self.assertNotIn('#include "aexcompat_host_core_abi.h"', native)
        self.assertNotIn("LoadLibraryW(", native)
        self.assertNotIn("GetProcAddress(", native)
        self.assertNotIn("__try", native)
        for marker in (
            "AdapterV1::Load",
            "AdapterLoadStatus::kMissingExport",
            "unloaded adapter invocation must fail closed",
            "SyntheticSehCreate",
            "RaiseException",
            "kSyntheticSehCode",
            "synthetic SEH must be normalized into stable boundary values",
            "seh_handle.value == 0",
            "seh_status.code == AEX_HOST_SEH_FAULT",
            "seh_report.phase == AEX_HOST_REPORT_PHASE_BOUNDARY",
            "seh_report.outcome == AEX_HOST_REPORT_OUTCOME_FAULTED",
            "adapter.Create",
            "adapter.Open",
            "adapter.Dispose",
        ):
            self.assertIn(marker, native)

    def test_boundary_stays_value_only_and_production_routing_is_unchanged(self):
        boundary = "\n".join(
            path.read_text(encoding="utf-8")
            for path in (ABI_HEADER, ADAPTER, NATIVE)
        )
        for forbidden in ("PF_", "AEGP_", "SPBasic", "wgpu"):
            self.assertNotIn(forbidden, boundary)
        document = DOC.read_text(encoding="utf-8")
        for marker in (
            "Phase 3",
            "Issue #623",
            "reusable C++ adapter",
            "synthetic SEH",
            "report ID zero",
            "Production worker routing remains unchanged",
            "#26 / PR #571",
            "#98",
            "#614/wgpu/GPU",
        ):
            self.assertIn(marker, document)


if __name__ == "__main__":
    unittest.main()
