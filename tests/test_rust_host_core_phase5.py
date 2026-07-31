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
CPP_IDENTITY = ROOT / "minihost/src/worker_aegp_scene_model.hpp"
NATIVE = (
    ROOT
    / "tests/native/rust_host_core_scene_identity_dual_run_selftest.cpp"
)
DOC = ROOT / "docs/RUST_HOST_CORE_MIGRATION_2026-07-31.md"
STANDALONE_GATE = (
    ROOT / "tools" / "test-rust-host-core-scene-identity.ps1"
)


class RustHostCorePhase5Tests(unittest.TestCase):
    @staticmethod
    def _command_failure(result: subprocess.CompletedProcess) -> str:
        stdout = (result.stdout or b"").decode("utf-8", errors="replace")
        stderr = (result.stderr or b"").decode("utf-8", errors="replace")
        return f"exit={result.returncode}\nstdout:\n{stdout}\nstderr:\n{stderr}"

    def test_c_abi_freezes_only_the_pointer_free_identity_and_kind_values(self):
        header = ABI_HEADER.read_text(encoding="utf-8")
        match = re.search(
            r"typedef struct AexHostSceneIdentity \{(?P<body>.*?)"
            r"\} AexHostSceneIdentity;",
            header,
            re.DOTALL,
        )
        self.assertIsNotNone(match)
        body = " ".join(match.group("body").split())
        self.assertEqual(
            body,
            "uint64_t project_id; uint64_t object_id; "
            "uint32_t generation; uint8_t kind; uint8_t reserved[3];",
        )
        self.assertNotIn("*", body)
        for marker in (
            "static_assert(sizeof(AexHostSceneIdentity) == 24)",
            "static_assert(alignof(AexHostSceneIdentity) == 8)",
            "offsetof(AexHostSceneIdentity, project_id) == 0",
            "offsetof(AexHostSceneIdentity, object_id) == 8",
            "offsetof(AexHostSceneIdentity, generation) == 16",
            "offsetof(AexHostSceneIdentity, kind) == 20",
            "offsetof(AexHostSceneIdentity, reserved) == 21",
        ):
            self.assertIn(marker, header)
        for name, value in (
            ("NONE", 0),
            ("PROJECT", 1),
            ("ITEM", 2),
            ("COMPOSITION", 3),
            ("FOLDER", 4),
            ("FOOTAGE", 5),
            ("LAYER", 6),
            ("EFFECT", 7),
            ("STREAM", 8),
            ("KEYFRAME", 9),
            ("VALUE", 10),
        ):
            self.assertIn(
                f"AEX_HOST_SCENE_OBJECT_KIND_{name} = {value}", header
            )

    def test_rust_keeps_kind_integer_and_classifies_generation_fail_closed(self):
        scene = SCENE.read_text(encoding="utf-8")
        for marker in (
            "pub struct HostSceneIdentity",
            "pub kind: u8",
            "pub reserved: [u8; 3]",
            "size_of::<HostSceneIdentity>() == 24",
            "align_of::<HostSceneIdentity>() == 8",
            "self.project_id != candidate.project_id",
            "HostErrorCode::WrongOwner",
            "self.kind != candidate.kind",
            "HostErrorCode::WrongKind",
            "self.object_id != candidate.object_id",
            "HostErrorCode::InvalidHandle",
            "self.generation != candidate.generation",
            "HostErrorCode::StaleHandle",
            "kind: u8::MAX",
        ):
            self.assertIn(marker, scene)
        self.assertNotRegex(scene, r"enum\s+HostSceneObjectKind")
        cpp = CPP_IDENTITY.read_text(encoding="utf-8")
        self.assertIn("static_assert(sizeof(Identity) == 24)", cpp)
        self.assertIn("uint32_t generation{}", cpp)
        self.assertIn("ObjectKind kind{ObjectKind::none}", cpp)

    def test_identity_descriptor_is_exact_and_precedes_function_cast(self):
        header = ABI_HEADER.read_text(encoding="utf-8")
        descriptor = re.search(
            r"typedef struct AexHostSceneIdentityAbiDescriptorV1 "
            r"\{(?P<body>.*?)\} AexHostSceneIdentityAbiDescriptorV1;",
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
                ("uint32_t", "identity_size"),
                ("uint32_t", "identity_alignment"),
                ("uint64_t", "capabilities"),
            ],
        )
        self.assertIn(
            "static_assert(sizeof(AexHostSceneIdentityAbiDescriptorV1) == 32)",
            header,
        )

        adapter = ADAPTER.read_text(encoding="utf-8")
        load = adapter.index("static AdapterLoadStatus Load")
        descriptor_lookup = adapter.index(
            '"aex_host_core_scene_identity_abi_descriptor_v1"', load
        )
        descriptor_copy = adapter.index(
            "CopySceneIdentityDescriptor(published_scene_descriptor",
            descriptor_lookup,
        )
        compatibility = adapter.index(
            "IsCompatibleSceneIdentityDescriptor(scene_descriptor)",
            descriptor_copy,
        )
        function_cast = adapter.index(
            "Resolve<AexHostCoreSceneIdentityMatchV1Fn>", load
        )
        self.assertLess(descriptor_lookup, descriptor_copy)
        self.assertLess(descriptor_copy, compatibility)
        self.assertLess(compatibility, function_cast)
        for marker in (
            "kMissingSceneIdentityAbiDescriptor",
            "kIncompatibleSceneIdentityAbiDescriptor",
            "descriptor.identity_size == sizeof(AexHostSceneIdentity)",
            "descriptor.identity_alignment == alignof(AexHostSceneIdentity)",
            "AEXCOMPAT_HOST_CORE_SCENE_IDENTITY_CAPABILITY_MATCH_V1",
        ):
            self.assertIn(marker, adapter)

    def test_ffi_and_adapter_contain_rust_panic_and_native_seh(self):
        ffi = FFI.read_text(encoding="utf-8")
        for marker in (
            'export_name = "aex_host_core_scene_identity_abi_descriptor_v1"',
            "HostSceneIdentityAbiDescriptorV1::current()",
            "aex_host_core_scene_identity_match_v1",
            'contain_panic("ffi_scene_identity_match"',
            "read_scene_identity",
            "identity.read()",
        ):
            self.assertIn(marker, ffi)

        adapter = ADAPTER.read_text(encoding="utf-8")
        for marker in (
            "InvokeSceneIdentityRaw",
            "AexHostCoreSceneIdentityMatchV1Fn",
            "__try",
            "__except (EXCEPTION_EXECUTE_HANDLER)",
            "AEX_HOST_SEH_FAULT",
        ):
            self.assertIn(marker, adapter)

    def test_native_gate_uses_the_real_cpp_registry_and_independent_oracle(self):
        native = NATIVE.read_text(encoding="utf-8")
        for marker in (
            '#include "worker_aegp_scene_model.hpp"',
            "static_assert(sizeof(Identity) == sizeof(AexHostSceneIdentity))",
            "MatchIdentityOracle",
            "Registry registry",
            "registry.initialize_fixture",
            "registry.invalidate(original, replacement)",
            "invalidated C++ identity must classify as stale",
            "foreign C++ project identity must classify as wrong owner",
            "different C++ object must classify as invalid handle",
            "different known kind must classify as wrong kind",
            "none kind sentinel must classify as wrong kind",
            "unknown integer kind must fail closed without an enum cast",
            "zero generation must fail closed",
            "nonzero reserved identity bytes must fail closed",
            "null current identity must fail closed",
            "SyntheticSceneIdentitySeh",
        ):
            self.assertIn(marker, native)
        for forbidden in ("ObjectSnapshot payload", "PF_", "SPBasic", "wgpu"):
            self.assertNotIn(forbidden, native)

    @unittest.skipUnless(
        os.name == "nt",
        "the scene identity dual-run requires the Windows MSVC/SEH boundary",
    )
    def test_native_gate_compiles_and_runs_standalone_without_shared_cmake(
        self,
    ):
        with tempfile.TemporaryDirectory(
            prefix="aexcompat-issue628-native-"
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
                report["rust_host_core_scene_identity_dual_run"], "passed"
            )
            self.assertGreaterEqual(report["checks"], 29)
            self.assertIs(report["cpp_registry"], True)
            self.assertIs(report["balanced"], True)

    def test_standalone_gate_is_bounded_to_the_phase5_native_selftest(self):
        script = STANDALONE_GATE.read_text(encoding="utf-8")
        for marker in (
            "aexcompat-host-core-ffi",
            "rust_host_core_scene_identity_dual_run_selftest.cpp",
            "worker_aegp_scene_model.cpp",
            "/std:c++17 /O2 /DNDEBUG /EHsc /W4 /WX",
            "rust_host_core_scene_identity_dual_run",
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

    def test_document_limits_phase5_to_identity_without_routing_or_pixels(self):
        document = " ".join(DOC.read_text(encoding="utf-8").split())
        for marker in (
            "Phase 5 scene identity/generation gate (Issue #628)",
            "24-byte",
            "pointer-free",
            "independent C++ identity oracle",
            "Production worker routing remains unchanged",
            "Issue #98",
            "does not require After Effects",
            "does not compare pixels",
        ):
            self.assertIn(marker, document)


if __name__ == "__main__":
    unittest.main()
