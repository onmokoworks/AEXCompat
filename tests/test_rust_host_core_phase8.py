import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CORE_ROOT = ROOT / "broker/crates/host-core/src/lib.rs"
SCENE = ROOT / "broker/crates/host-core/src/scene.rs"
FFI = ROOT / "broker/crates/host-core-ffi/src/lib.rs"
ABI_HEADER = ROOT / "broker/crates/broker/include/aexcompat_host_core_abi.h"
ADAPTER = ROOT / "broker/crates/broker/include/aexcompat_host_core_adapter.hpp"
PHASE7_NATIVE = (
    ROOT
    / "tests/native/"
    "rust_host_core_scene_topology_snapshot_dual_run_selftest.cpp"
)
NATIVE = (
    ROOT
    / "tests/native/"
    "rust_host_core_scene_topology_owned_dual_run_selftest.cpp"
)
DOC = ROOT / "docs/RUST_HOST_CORE_MIGRATION_2026-07-31.md"
STANDALONE_GATE = (
    ROOT / "tools/test-rust-host-core-scene-topology-owned.ps1"
)


class RustHostCorePhase8Tests(unittest.TestCase):
    @staticmethod
    def _command_failure(result: subprocess.CompletedProcess) -> str:
        stdout = (result.stdout or b"").decode("utf-8", errors="replace")
        stderr = (result.stderr or b"").decode("utf-8", errors="replace")
        return f"exit={result.returncode}\nstdout:\n{stdout}\nstderr:\n{stderr}"

    def test_rust_core_owns_a_canonical_detached_snapshot(self):
        core_root = CORE_ROOT.read_text(encoding="utf-8")
        self.assertIn("Immutable", core_root)
        self.assertIn("Rust-owned state", core_root)
        scene = SCENE.read_text(encoding="utf-8")
        for marker in (
            "pub struct HostOwnedSceneTopologySnapshot",
            "pub fn from_snapshot",
            "snapshot.calculate_summary()?",
            "copy_from_slice",
            "sort_unstable_by_key",
            "pub fn entry(&self, index: u32)",
            "pub const fn summary",
            "query_owned_scene_topology_index",
        ):
            self.assertIn(marker, scene)
        self.assertNotIn("Box::into_raw", scene)
        self.assertNotIn("Box::from_raw", scene)

    def test_existing_opaque_handle_has_a_separate_scene_registry(self):
        ffi = FFI.read_text(encoding="utf-8")
        for marker in (
            "scene_snapshots: HandleRegistry<SceneSnapshotRecord>",
            "sessions: HandleRegistry<SessionRecord>",
            ".insert(owner, HandleKind::Scene, record)",
            ".get(handle, owner, HandleKind::Scene)?",
            ".remove(handle, owner, HandleKind::Scene)?",
            "caller_thread_token: u64",
            "origin_thread: ThreadId",
            "thread::current().id()",
            "HostErrorCode::WrongThread",
        ):
            self.assertIn(marker, ffi)
        self.assertNotIn("Box::into_raw", ffi)
        self.assertNotIn("Box::from_raw", ffi)

    def test_abi_reuses_pointer_free_values_and_gates_all_exports(self):
        header = ABI_HEADER.read_text(encoding="utf-8")
        for marker in (
            "AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPABILITY_OWNED_SNAPSHOT_V1",
            "AexHostCoreSceneTopologySnapshotCreateV1Fn",
            "AexHostCoreSceneTopologySnapshotQueryV1Fn",
            "AexHostCoreSceneTopologySnapshotSummaryV1Fn",
            "AexHostCoreSceneTopologySnapshotDestroyV1Fn",
            "aex_host_core_scene_topology_snapshot_create_v1",
            "aex_host_core_scene_topology_snapshot_query_v1",
            "aex_host_core_scene_topology_snapshot_summary_v1",
            "aex_host_core_scene_topology_snapshot_destroy_v1",
        ):
            self.assertIn(marker, header)

        adapter = ADAPTER.read_text(encoding="utf-8")
        load = adapter.index("static AdapterLoadStatus Load")
        descriptor = adapter.index(
            '"aex_host_core_scene_topology_abi_descriptor_v1"', load
        )
        compatible = adapter.index(
            "IsCompatibleSceneTopologyDescriptor(", descriptor
        )
        create_cast = adapter.index(
            "Resolve<AexHostCoreSceneTopologySnapshotCreateV1Fn>",
            compatible,
        )
        self.assertLess(descriptor, compatible)
        self.assertLess(compatible, create_cast)
        self.assertIn(
            "AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPABILITY_OWNED_SNAPSHOT_V1",
            adapter,
        )

    def test_all_rust_outputs_zero_and_panics_are_contained(self):
        ffi = FFI.read_text(encoding="utf-8")
        for marker in (
            'contain_panic("ffi_scene_topology_snapshot_create"',
            'contain_panic("ffi_scene_topology_snapshot_query"',
            'contain_panic("ffi_scene_topology_snapshot_summary"',
            'contain_panic("ffi_scene_topology_snapshot_destroy"',
            "handle.write(HostOpaqueHandle(0))",
            "entry.write(HostSceneTopologyEntry::zeroed())",
            "summary.write(HostSceneTopologySummary::zeroed())",
        ):
            self.assertIn(marker, ffi)

    def test_cpp_adapter_contains_seh_and_zeroes_value_outputs(self):
        adapter = ADAPTER.read_text(encoding="utf-8")
        for marker in (
            "InvokeSceneTopologySnapshotCreateRaw",
            "InvokeSceneTopologySnapshotQueryRaw",
            "InvokeSceneTopologySnapshotSummaryRaw",
            "InvokeSceneTopologySnapshotDestroyRaw",
            "AexHostOpaqueHandle{}",
            "AexHostSceneTopologyEntry{}",
            "AexHostSceneTopologySummary{}",
            "__except (EXCEPTION_EXECUTE_HANDLER)",
            "AEX_HOST_SEH_FAULT",
        ):
            self.assertIn(marker, adapter)

    def test_native_gate_proves_real_registry_state_ownership(self):
        native = NATIVE.read_text(encoding="utf-8")
        for marker in (
            '#include "worker_aegp_scene_model.hpp"',
            "Registry registry",
            "registry.initialize_fixture",
            "BuildSnapshot(registry, false, baseline)",
            "BuildSnapshot(registry, true, reversed)",
            "CanonicalizeOracle",
            "std::reverse(baseline.entries",
            "baseline = {};",
            "canonical queries must survive caller buffer reorder",
            "canonical queries must survive caller buffer overwrite",
            "actual foreign thread destroy must fail closed",
            "session handle must be rejected by scene query without destruction",
            "scene handle must be rejected by session namespace without destruction",
            "query after destroy must be stale and zeroed",
            "double destroy must be stale",
            "capacity overflow must not truncate or publish a handle",
            "SyntheticCreateSeh",
            "SyntheticQuerySeh",
            "SyntheticSummarySeh",
            "SyntheticDestroySeh",
        ):
            self.assertIn(marker, native)
        for forbidden in (
            "registry.invalidate",
            "registry.update_local_index",
            "PF_",
            "wgpu",
            "RenderSession",
        ):
            self.assertNotIn(forbidden, native)

    @unittest.skipUnless(
        os.name == "nt",
        "the owned topology dual-run requires Windows MSVC/SEH",
    )
    def test_native_gate_compiles_and_runs_standalone(self):
        temporary_root = os.environ.get("AEXCOMPAT_NATIVE_TEMP_ROOT")
        with tempfile.TemporaryDirectory(
            prefix="aexcompat-issue634-native-", dir=temporary_root
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
                report["rust_host_core_scene_topology_owned_dual_run"],
                "passed",
            )
            self.assertGreaterEqual(report["checks"], 25)
            self.assertIs(report["cpp_registry"], True)
            self.assertIs(report["rust_owned"], True)
            self.assertIs(report["buffer_detached"], True)
            self.assertIs(report["thread_bound"], True)
            self.assertIs(report["namespace_fail_closed"], True)

    def test_runner_and_document_keep_phase8_bounded(self):
        script = STANDALONE_GATE.read_text(encoding="utf-8")
        for marker in (
            "aexcompat-host-core-ffi",
            "rust_host_core_scene_topology_owned_dual_run_selftest.cpp",
            "worker_aegp_scene_model.cpp",
            "/std:c++17 /O2 /DNDEBUG /EHsc /W4 /WX",
            "rust_host_core_scene_topology_owned_dual_run",
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

        document = " ".join(DOC.read_text(encoding="utf-8").split())
        for marker in (
            "Phase 8 Rust-owned immutable scene topology lifecycle (Issue #634)",
            "canonical entry query",
            "caller buffer",
            "owner ID",
            "logical caller-thread token",
            "actual origin thread",
            "Production worker routing remains unchanged",
            "does not require After Effects",
            "does not compare pixels",
        ):
            self.assertIn(marker, document)

        phase7 = PHASE7_NATIVE.read_text(encoding="utf-8")
        self.assertIn("unknown topology capability must fail closed", phase7)


if __name__ == "__main__":
    unittest.main()
