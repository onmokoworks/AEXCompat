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
    "rust_host_core_scene_owner_relation_dual_run_selftest.cpp"
)
DOC = ROOT / "docs/RUST_HOST_CORE_MIGRATION_2026-07-31.md"
STANDALONE_GATE = (
    ROOT / "tools/test-rust-host-core-scene-owner-relation.ps1"
)


class RustHostCorePhase6Tests(unittest.TestCase):
    @staticmethod
    def _command_failure(result: subprocess.CompletedProcess) -> str:
        stdout = (result.stdout or b"").decode("utf-8", errors="replace")
        stderr = (result.stderr or b"").decode("utf-8", errors="replace")
        return f"exit={result.returncode}\nstdout:\n{stdout}\nstderr:\n{stderr}"

    def test_c_abi_freezes_only_two_phase5_identities(self):
        header = ABI_HEADER.read_text(encoding="utf-8")
        relation = re.search(
            r"typedef struct AexHostSceneOwnerRelation \{(?P<body>.*?)"
            r"\} AexHostSceneOwnerRelation;",
            header,
            re.DOTALL,
        )
        self.assertIsNotNone(relation)
        body = " ".join(relation.group("body").split())
        self.assertEqual(
            body,
            "AexHostSceneIdentity object; AexHostSceneIdentity owner;",
        )
        self.assertNotIn("*", body)
        for marker in (
            "static_assert(sizeof(AexHostSceneOwnerRelation) == 48)",
            "static_assert(alignof(AexHostSceneOwnerRelation) == 8)",
            "offsetof(AexHostSceneOwnerRelation, object) == 0",
            "offsetof(AexHostSceneOwnerRelation, owner) == 24",
        ):
            self.assertIn(marker, header)

    def test_rust_classifies_owner_edges_without_taking_registry_ownership(self):
        scene = SCENE.read_text(encoding="utf-8")
        for marker in (
            "pub struct HostSceneOwnerRelation",
            "pub object: HostSceneIdentity",
            "pub owner: HostSceneIdentity",
            "self.owner.is_zero_sentinel()",
            "self.object.project_id != self.owner.project_id",
            "validate_scene_self_owner",
            "self.object.match_candidate(&candidate.object)",
            "self.owner.match_candidate(&candidate.owner)",
            "HostErrorCode::WrongOwner",
            "HostErrorCode::StaleHandle",
        ):
            self.assertIn(marker, scene)
        for forbidden in (
            "related_item",
            "parent_layer",
            "legacy_handle",
            "StreamState",
            "KeyframeState",
        ):
            self.assertNotIn(forbidden, scene)

    def test_owner_descriptor_precedes_function_cast(self):
        header = ABI_HEADER.read_text(encoding="utf-8")
        descriptor = re.search(
            r"typedef struct AexHostSceneOwnerRelationAbiDescriptorV1 "
            r"\{(?P<body>.*?)"
            r"\} AexHostSceneOwnerRelationAbiDescriptorV1;",
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
                ("uint32_t", "relation_size"),
                ("uint32_t", "relation_alignment"),
                ("uint64_t", "capabilities"),
            ],
        )

        adapter = ADAPTER.read_text(encoding="utf-8")
        load = adapter.index("static AdapterLoadStatus Load")
        descriptor_lookup = adapter.index(
            '"aex_host_core_scene_owner_relation_abi_descriptor_v1"',
            load,
        )
        descriptor_copy = adapter.index(
            "CopySceneOwnerRelationDescriptor(",
            descriptor_lookup,
        )
        compatibility = adapter.index(
            "IsCompatibleSceneOwnerRelationDescriptor(",
            descriptor_copy,
        )
        function_cast = adapter.index(
            "Resolve<AexHostCoreSceneOwnerRelationMatchV1Fn>",
            compatibility,
        )
        self.assertLess(descriptor_lookup, descriptor_copy)
        self.assertLess(descriptor_copy, compatibility)
        self.assertLess(compatibility, function_cast)
        for marker in (
            "kMissingSceneOwnerRelationAbiDescriptor",
            "kIncompatibleSceneOwnerRelationAbiDescriptor",
            "descriptor.relation_size == sizeof(AexHostSceneOwnerRelation)",
            "AEXCOMPAT_HOST_CORE_SCENE_OWNER_RELATION_CAPABILITY_MATCH_V1",
        ):
            self.assertIn(marker, adapter)

    def test_ffi_and_adapter_contain_panic_and_seh(self):
        ffi = FFI.read_text(encoding="utf-8")
        for marker in (
            'export_name = "aex_host_core_scene_owner_relation_abi_descriptor_v1"',
            "HostSceneOwnerRelationAbiDescriptorV1::current()",
            "aex_host_core_scene_owner_relation_match_v1",
            'contain_panic("ffi_scene_owner_relation_match"',
            "read_scene_owner_relation",
            "relation.read()",
        ):
            self.assertIn(marker, ffi)
        adapter = ADAPTER.read_text(encoding="utf-8")
        for marker in (
            "InvokeSceneOwnerRelationRaw",
            "AexHostCoreSceneOwnerRelationMatchV1Fn",
            "__try",
            "__except (EXCEPTION_EXECUTE_HANDLER)",
            "AEX_HOST_SEH_FAULT",
        ):
            self.assertIn(marker, adapter)

    def test_native_gate_uses_real_registry_values_and_independent_oracle(self):
        native = NATIVE.read_text(encoding="utf-8")
        for marker in (
            '#include "worker_aegp_scene_model.hpp"',
            "MatchOwnerRelationOracle",
            "Registry registry",
            "registry.initialize_fixture",
            "registry.snapshot(project_a, project_snapshot)",
            "registry.invalidate(comp_snapshot.identity, replacement_comp)",
            "current_layer_snapshot.owner == replacement_comp",
            "invalidated C++ owner generation must classify as stale",
            "different same-project owner must classify as wrong owner",
            "foreign C++ owner must classify as wrong owner",
            "cross-project edge must classify as wrong owner",
            "wrong known owner kind must classify as wrong kind",
            "unknown owner kind must fail closed",
            "self-owner edge must fail closed",
            "SyntheticOwnerRelationSeh",
        ):
            self.assertIn(marker, native)
        for forbidden in ("PF_", "SPBasic", "wgpu", "RenderSession"):
            self.assertNotIn(forbidden, native)

    @unittest.skipUnless(
        os.name == "nt",
        "the owner relation dual-run requires the Windows MSVC/SEH boundary",
    )
    def test_native_gate_compiles_and_runs_standalone(self):
        with tempfile.TemporaryDirectory(
            prefix="aexcompat-issue630-native-"
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
                report["rust_host_core_scene_owner_relation_dual_run"],
                "passed",
            )
            self.assertGreaterEqual(report["checks"], 25)
            self.assertIs(report["cpp_registry"], True)
            self.assertIs(report["balanced"], True)

    def test_standalone_gate_is_bounded_to_issue630(self):
        script = STANDALONE_GATE.read_text(encoding="utf-8")
        for marker in (
            "aexcompat-host-core-ffi",
            "rust_host_core_scene_owner_relation_dual_run_selftest.cpp",
            "worker_aegp_scene_model.cpp",
            "/std:c++17 /O2 /DNDEBUG /EHsc /W4 /WX",
            "rust_host_core_scene_owner_relation_dual_run",
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

    def test_document_limits_phase6_to_the_owner_edge(self):
        document = " ".join(DOC.read_text(encoding="utf-8").split())
        for marker in (
            "Phase 6 scene object owner-edge gate (Issue #630)",
            "48-byte",
            "object",
            "owner",
            "C++ registry remains authoritative",
            "Production worker routing remains unchanged",
            "Issue #98",
            "does not require After Effects",
            "does not compare pixels",
        ):
            self.assertIn(marker, document)


if __name__ == "__main__":
    unittest.main()
