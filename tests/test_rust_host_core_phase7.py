import json
import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCENE = ROOT / "broker/crates/host-core/src/scene.rs"
FFI = ROOT / "broker/crates/host-core-ffi/src/lib.rs"
ABI_HEADER = ROOT / "broker/crates/broker/include/aexcompat_host_core_abi.h"
ADAPTER = ROOT / "broker/crates/broker/include/aexcompat_host_core_adapter.hpp"
NATIVE = (
    ROOT
    / "tests/native/"
    "rust_host_core_scene_topology_snapshot_dual_run_selftest.cpp"
)
DOC = ROOT / "docs/RUST_HOST_CORE_MIGRATION_2026-07-31.md"
STANDALONE_GATE = (
    ROOT / "tools/test-rust-host-core-scene-topology-snapshot.ps1"
)


class RustHostCorePhase7Tests(unittest.TestCase):
    @staticmethod
    def _command_failure(result: subprocess.CompletedProcess) -> str:
        stdout = (result.stdout or b"").decode("utf-8", errors="replace")
        stderr = (result.stderr or b"").decode("utf-8", errors="replace")
        return f"exit={result.returncode}\nstdout:\n{stdout}\nstderr:\n{stderr}"

    def test_c_abi_is_fixed_capacity_and_pointer_free(self):
        header = ABI_HEADER.read_text(encoding="utf-8")
        snapshot = re.search(
            r"typedef struct AexHostSceneTopologySnapshot \{(?P<body>.*?)"
            r"\} AexHostSceneTopologySnapshot;",
            header,
            re.DOTALL,
        )
        self.assertIsNotNone(snapshot)
        body = " ".join(snapshot.group("body").split())
        self.assertNotIn("*", body)
        self.assertIn("uint64_t project_id;", body)
        self.assertIn("uint32_t entry_count;", body)
        self.assertIn(
            "entries[AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY];",
            body,
        )
        for marker in (
            "#define AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY 16u",
            "static_assert(sizeof(AexHostSceneTopologyEntry) == 56)",
            "static_assert(sizeof(AexHostSceneTopologySnapshot) == 912)",
            "static_assert(sizeof(AexHostSceneTopologySummary) == 32)",
        ):
            self.assertIn(marker, header)

    def test_rust_owns_whole_snapshot_validation_and_canonical_summary(self):
        scene = SCENE.read_text(encoding="utf-8")
        for marker in (
            "pub struct HostSceneTopologySnapshot",
            "pub fn calculate_summary",
            "HostErrorCode::CapacityExceeded",
            "validate_scene_topology_duplicate_identity",
            "validate_scene_topology_missing_owner",
            "validate_scene_topology_cycle",
            "validate_scene_topology_local_index_collision",
            "sort_unstable_by_key",
            "mix_topology_identity",
            "pub struct HostSceneTopologySummary",
        ):
            self.assertIn(marker, scene)
        for forbidden in (
            "StreamState",
            "KeyframeState",
            "World",
            "parameter_value",
            "RenderSession",
        ):
            self.assertNotIn(forbidden, scene)

    def test_topology_descriptor_is_checked_before_function_cast(self):
        header = ABI_HEADER.read_text(encoding="utf-8")
        descriptor = re.search(
            r"typedef struct AexHostSceneTopologyAbiDescriptorV1 "
            r"\{(?P<body>.*?)"
            r"\} AexHostSceneTopologyAbiDescriptorV1;",
            header,
            re.DOTALL,
        )
        self.assertIsNotNone(descriptor)
        fields = re.findall(
            r"^\s*(uint(?:32|64)_t)\s+([a-z0-9_]+);$",
            descriptor.group("body"),
            re.MULTILINE,
        )
        self.assertEqual(
            fields,
            [
                ("uint64_t", "magic"),
                ("uint32_t", "abi_version"),
                ("uint32_t", "struct_size"),
                ("uint32_t", "entry_size"),
                ("uint32_t", "entry_alignment"),
                ("uint32_t", "snapshot_size"),
                ("uint32_t", "snapshot_alignment"),
                ("uint32_t", "summary_size"),
                ("uint32_t", "summary_alignment"),
                ("uint32_t", "capacity"),
                ("uint32_t", "reserved"),
                ("uint64_t", "capabilities"),
            ],
        )

        adapter = ADAPTER.read_text(encoding="utf-8")
        load = adapter.index("static AdapterLoadStatus Load")
        lookup = adapter.index(
            '"aex_host_core_scene_topology_abi_descriptor_v1"', load
        )
        copied = adapter.index("CopySceneTopologyDescriptor(", lookup)
        compatible = adapter.index(
            "IsCompatibleSceneTopologyDescriptor(", copied
        )
        function_cast = adapter.index(
            "Resolve<AexHostCoreSceneTopologySummarizeV1Fn>", compatible
        )
        self.assertLess(lookup, copied)
        self.assertLess(copied, compatible)
        self.assertLess(compatible, function_cast)

    def test_ffi_and_adapter_keep_panic_and_seh_boundaries(self):
        ffi = FFI.read_text(encoding="utf-8")
        for marker in (
            'export_name = "aex_host_core_scene_topology_abi_descriptor_v1"',
            "HostSceneTopologyAbiDescriptorV1::current()",
            "aex_host_core_scene_topology_summarize_v1",
            'contain_panic("ffi_scene_topology_summarize"',
            "HostSceneTopologySummary::zeroed()",
        ):
            self.assertIn(marker, ffi)
        adapter = ADAPTER.read_text(encoding="utf-8")
        for marker in (
            "InvokeSceneTopologyRaw",
            "AexHostCoreSceneTopologySummarizeV1Fn",
            "__try",
            "__except (EXCEPTION_EXECUTE_HANDLER)",
            "AEX_HOST_SEH_FAULT",
        ):
            self.assertIn(marker, adapter)

    def test_native_gate_uses_registry_transitions_and_independent_oracle(self):
        native = NATIVE.read_text(encoding="utf-8")
        for marker in (
            '#include "worker_aegp_scene_model.hpp"',
            "SummarizeOracle",
            "Registry registry",
            "registry.initialize_fixture",
            "BuildSnapshot(registry",
            "registry.invalidate(comp, replacement_comp)",
            "registry.update_local_index(layer2, 7)",
            "!registry.update_local_index(missing, 3)",
            "duplicate identity must fail closed",
            "missing owner must fail closed",
            "ownership cycle must fail closed",
            "cross-project owner must fail closed",
            "duplicate sibling local index must fail closed",
            "capacity overflow must fail without truncation",
            "SyntheticTopologySeh",
        ):
            self.assertIn(marker, native)
        for forbidden in ("PF_", "SPBasic", "wgpu", "RenderSession"):
            self.assertNotIn(forbidden, native)

    @unittest.skipUnless(
        os.name == "nt",
        "the topology dual-run requires the Windows MSVC/SEH boundary",
    )
    def test_native_gate_compiles_and_runs_standalone(self):
        with tempfile.TemporaryDirectory(
            prefix="aexcompat-issue632-native-"
        ) as temporary:
            result = subprocess.run(
                [
                    "powershell.exe",
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    str(STANDALONE_GATE),
                    "-BuildDirectory",
                    temporary,
                ],
                cwd=ROOT,
                capture_output=True,
                timeout=240,
            )
            self.assertEqual(
                result.returncode, 0, self._command_failure(result)
            )
            reports = [
                json.loads(line)
                for line in result.stdout.decode(
                    "utf-8", errors="replace"
                ).splitlines()
                if line.startswith("{")
            ]
            self.assertEqual(len(reports), 1, self._command_failure(result))
            report = reports[0]
            self.assertEqual(
                report[
                    "rust_host_core_scene_topology_snapshot_dual_run"
                ],
                "passed",
            )
            self.assertGreaterEqual(report["checks"], 35)
            self.assertIs(report["cpp_registry"], True)
            self.assertIs(report["canonical"], True)
            self.assertIs(report["overflow_fail_closed"], True)

    def test_standalone_gate_is_bounded_to_issue632(self):
        script = STANDALONE_GATE.read_text(encoding="utf-8")
        for marker in (
            "aexcompat-host-core-ffi",
            "rust_host_core_scene_topology_snapshot_dual_run_selftest.cpp",
            "worker_aegp_scene_model.cpp",
            "/std:c++17 /O2 /DNDEBUG /EHsc /W4 /WX",
            "rust_host_core_scene_topology_snapshot_dual_run",
            "ConvertFrom-Json",
        ):
            self.assertIn(marker, script)
        for forbidden in (
            "minihost\\CMakeLists.txt",
            "l2_main.cpp",
            "extended_inter_memory",
            "wgpu",
            "render-session",
        ):
            self.assertNotIn(forbidden, script)

    def test_document_limits_phase7_to_topology_state_calculation(self):
        document = " ".join(DOC.read_text(encoding="utf-8").split())
        for marker in (
            "Phase 7 bounded scene topology state gate (Issue #632)",
            "fixed-capacity",
            "local_index",
            "enumeration-order-independent",
            "C++ Registry remains authoritative",
            "Production worker routing remains unchanged",
            "does not require After Effects",
            "does not compare pixels",
        ):
            self.assertIn(marker, document)


if __name__ == "__main__":
    unittest.main()
