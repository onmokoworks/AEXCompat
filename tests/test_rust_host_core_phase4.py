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
    def test_descriptor_is_the_exact_pointer_free_minimum(self):
        header = ABI_HEADER.read_text(encoding="utf-8")
        match = re.search(
            r"typedef struct AexHostCoreAbiDescriptorV1 \{(?P<body>.*?)"
            r"\} AexHostCoreAbiDescriptorV1;",
            header,
            re.DOTALL,
        )
        self.assertIsNotNone(match)
        body = match.group("body")
        fields = re.findall(
            r"^\s*(uint(?:32|64)_t)\s+([a-z0-9_]+);$",
            body,
            re.MULTILINE,
        )
        self.assertEqual(
            fields,
            [
                ("uint64_t", "magic"),
                ("uint32_t", "abi_version"),
                ("uint32_t", "struct_size"),
                ("uint32_t", "call_context_size"),
                ("uint32_t", "call_context_alignment"),
                ("uint32_t", "call_status_size"),
                ("uint32_t", "call_status_alignment"),
                ("uint32_t", "opaque_handle_size"),
                ("uint32_t", "opaque_handle_alignment"),
                ("uint32_t", "report_snapshot_size"),
                ("uint32_t", "report_snapshot_alignment"),
                ("uint64_t", "capabilities"),
            ],
        )
        for forbidden in (
            "*",
            "PF_",
            "AEGP_",
            "SPBasic",
            "schema",
            "calling_convention",
            "function_table",
            "session_state",
        ):
            self.assertNotIn(forbidden, body)
        for marker in (
            "AEXCOMPAT_HOST_CORE_ABI_DESCRIPTOR_MAGIC",
            "AEXCOMPAT_HOST_CORE_CAPABILITY_SESSION_LIFECYCLE_V1",
            "extern const AexHostCoreAbiDescriptorV1 "
            "aex_host_core_abi_descriptor_v1;",
            "static_assert(sizeof(AexHostCoreAbiDescriptorV1) == 56)",
            "static_assert(alignof(AexHostCoreAbiDescriptorV1) == 8)",
            "offsetof(AexHostCoreAbiDescriptorV1, capabilities) == 48",
        ):
            self.assertIn(marker, header)

    def test_rust_exports_the_compiled_descriptor_as_data(self):
        boundary = BOUNDARY.read_text(encoding="utf-8")
        ffi = FFI.read_text(encoding="utf-8")
        for marker in (
            "pub const HOST_CORE_ABI_DESCRIPTOR_MAGIC",
            "pub const HOST_CORE_CAPABILITY_SESSION_LIFECYCLE_V1",
            "#[repr(C)]",
            "pub struct HostCoreAbiDescriptorV1",
            "pub const fn current() -> Self",
            "size_of::<HostCoreAbiDescriptorV1>() == 56",
            "offset_of!(HostCoreAbiDescriptorV1, capabilities) == 48",
            "abi_descriptor_identifies_the_exact_shared_value_layouts",
        ):
            self.assertIn(marker, boundary)
        self.assertIn(
            '#[unsafe(export_name = "aex_host_core_abi_descriptor_v1")]',
            ffi,
        )
        self.assertIn(
            "pub static AEX_HOST_CORE_ABI_DESCRIPTOR_V1: "
            "HostCoreAbiDescriptorV1",
            ffi,
        )
        self.assertIn(
            "HostCoreAbiDescriptorV1::current()",
            ffi,
        )

    def test_loader_validates_descriptor_before_function_casts(self):
        adapter = ADAPTER.read_text(encoding="utf-8")
        load_start = adapter.index("static AdapterLoadStatus Load")
        descriptor_lookup = adapter.index(
            '"aex_host_core_abi_descriptor_v1"', load_start
        )
        descriptor_copy = adapter.index(
            "CopyDescriptor(published_descriptor", descriptor_lookup
        )
        compatibility_check = adapter.index(
            "IsCompatibleDescriptor(descriptor)", descriptor_copy
        )
        first_function_cast = adapter.index(
            "Resolve<AexHostCoreSessionCreateV1Fn>", load_start
        )
        self.assertLess(descriptor_lookup, descriptor_copy)
        self.assertLess(descriptor_copy, compatibility_check)
        self.assertLess(compatibility_check, first_function_cast)
        for marker in (
            "kMissingAbiDescriptor",
            "kIncompatibleAbiDescriptor",
            "ERROR_BAD_FORMAT",
            "static bool CopyDescriptor",
            "__try",
            "__except (EXCEPTION_EXECUTE_HANDLER)",
            "descriptor.capabilities ==",
            "AEXCOMPAT_HOST_CORE_CAPABILITY_SESSION_LIFECYCLE_V1",
            "ValidateApi(api)",
            "AdapterLoadStatus::kMissingExport",
        ):
            self.assertIn(marker, adapter)

    def test_native_gate_rejects_each_identity_mismatch_and_keeps_regressions(self):
        abi_selftest = ABI_SELFTEST.read_text(encoding="utf-8")
        dual_run = DUAL_RUN.read_text(encoding="utf-8")
        for marker in (
            "constexpr AexHostCoreAbiDescriptorV1 kExpectedDescriptor",
            "static_assert(kExpectedDescriptor.magic ==",
            "static_assert(kExpectedDescriptor.capabilities ==",
            "offsetof(AexHostCoreAbiDescriptorV1, capabilities) == 48",
        ):
            self.assertIn(marker, abi_selftest)
        for marker in (
            "adapter must copy the exact compatible ABI descriptor",
            "wrong descriptor magic must fail closed",
            "wrong descriptor ABI version must fail closed",
            "wrong descriptor size must fail closed",
            "wrong context size must fail closed",
            "wrong context alignment must fail closed",
            "wrong status size must fail closed",
            "wrong status alignment must fail closed",
            "wrong handle size must fail closed",
            "wrong handle alignment must fail closed",
            "wrong report size must fail closed",
            "wrong report alignment must fail closed",
            "missing capability must fail closed",
            "unknown capability must fail closed",
            "AdapterLoadStatus::kMissingAbiDescriptor",
            "AdapterLoadStatus::kMissingExport",
            "SyntheticSehCreate",
            "open/foreign-thread",
            "dispose/wrong-owner",
            "open/stale",
        ):
            self.assertIn(marker, dual_run)

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
