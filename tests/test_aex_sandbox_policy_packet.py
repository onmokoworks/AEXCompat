import importlib.util
import json
import sys
import time
import unittest
from pathlib import Path


LAB_ROOT = Path(__file__).resolve().parents[1]


def load_tool(name: str):
    path = LAB_ROOT / "tools" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


aex_sandbox_policy_packet = load_tool("aex_sandbox_policy_packet")


def make_dependency_matrix() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_dependency_matrix",
        "dependency_matrix_state": "dependency_matrix_ready",
        "availability_check": "not_performed",
        "dependency_rows": [
            {"dll_name": "kernel32.dll", "category": "core_windows", "candidate_count": 3},
            {"dll_name": "vcruntime140.dll", "category": "release_crt_runtime", "candidate_count": 2},
            {"dll_name": "opengl32.dll", "category": "graphics_or_gpu", "candidate_count": 1},
            {"dll_name": "ucrtbased.dll", "category": "debug_crt_runtime", "candidate_count": 1},
        ],
        "candidate_rows": [
            {
                "relative_path": "AEPluginBuild\\ScatterMap.aex",
                "review_bucket": "primary_fixture_candidate",
                "compatibility_class": "classic_pf_effect_candidate",
                "fixture_candidate_score": 95,
                "dependency_categories": ["core_windows", "release_crt_runtime", "windows_crt_api_set"],
                "dependency_risk_flags": [],
            },
            {
                "relative_path": "AEPluginBuild\\AAlphabeticalButSecond.aex",
                "review_bucket": "primary_fixture_candidate",
                "compatibility_class": "classic_pf_effect_candidate",
                "fixture_candidate_score": 95,
                "dependency_categories": ["core_windows", "release_crt_runtime", "windows_crt_api_set"],
                "dependency_risk_flags": [],
            },
            {
                "relative_path": "AEPluginBuild\\GpuThing.aex",
                "review_bucket": "dependency_or_environment_review",
                "compatibility_class": "classic_pf_effect_candidate",
                "fixture_candidate_score": 80,
                "dependency_categories": ["core_windows", "graphics_or_gpu"],
                "dependency_risk_flags": ["graphics_or_gpu_dependency"],
            },
            {
                "relative_path": "AEPluginBuild\\DebugThing.aex",
                "review_bucket": "dependency_or_environment_review",
                "compatibility_class": "classic_pf_effect_candidate",
                "fixture_candidate_score": 70,
                "dependency_categories": ["core_windows", "debug_crt_runtime"],
                "dependency_risk_flags": ["debug_runtime_dependency"],
            },
        ],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


class AexSandboxPolicyPacketTests(unittest.TestCase):
    def test_policy_packet_splits_allow_review_and_deny_candidates(self):
        packet = aex_sandbox_policy_packet.build_sandbox_policy_packet(make_dependency_matrix(), Path("deps.json"))
        self.assertEqual(packet["packet_kind"], "aex_sandbox_policy_packet")
        self.assertEqual(packet["sandbox_policy_state"], "policy_ready_no_native_load")
        self.assertEqual(packet["availability_check"], "not_performed")
        self.assertFalse(packet["native_load_enabled"])
        self.assertFalse(packet["native_load_performed"])
        self.assertFalse(packet["aex_file_opened"])

        by_path = {row["relative_path"]: row for row in packet["candidate_policy_rows"]}
        self.assertEqual(
            by_path["AEPluginBuild\\ScatterMap.aex"]["candidate_policy_state"],
            "eligible_for_manual_policy_review",
        )
        self.assertEqual(
            by_path["AEPluginBuild\\GpuThing.aex"]["candidate_policy_state"],
            "manual_dependency_review_required",
        )
        self.assertEqual(
            by_path["AEPluginBuild\\DebugThing.aex"]["candidate_policy_state"],
            "blocked_by_default_deny_dependency",
        )
        self.assertEqual(packet["primary_policy_candidate"]["relative_path"], "AEPluginBuild\\ScatterMap.aex")
        self.assertEqual(packet["primary_policy_candidate"]["source_order"], 0)

    def test_invalid_or_unsafe_source_matrix_is_rejected(self):
        matrix = make_dependency_matrix()
        matrix["dependency_matrix_state"] = "incomplete"
        with self.assertRaises(ValueError):
            aex_sandbox_policy_packet.build_sandbox_policy_packet(matrix, Path("deps.json"))

        matrix = make_dependency_matrix()
        matrix["native_load_performed"] = True
        with self.assertRaises(ValueError):
            aex_sandbox_policy_packet.build_sandbox_policy_packet(matrix, Path("deps.json"))

    def test_paths_are_confined_and_output_is_create_new(self):
        dep_root = LAB_ROOT / "target" / "dependency-matrix"
        dep_root.mkdir(parents=True, exist_ok=True)
        source = dep_root / f"{time.time_ns()}-policy-source.local.json"
        source.write_text(json.dumps(make_dependency_matrix()), encoding="utf-8")
        loaded, resolved = aex_sandbox_policy_packet.load_dependency_matrix(source)
        self.assertEqual(loaded["report_kind"], "aex_dependency_matrix")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-policy.json"
        outside.write_text(json.dumps(make_dependency_matrix()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_sandbox_policy_packet.load_dependency_matrix(outside)

        payload = aex_sandbox_policy_packet.build_sandbox_policy_packet(loaded, resolved)
        out = LAB_ROOT / "target" / "sandbox-policy" / f"{time.time_ns()}-policy.local.json"
        written = aex_sandbox_policy_packet.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_sandbox_policy_packet.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_sandbox_policy_packet.write_json_create_new(LAB_ROOT / "target" / "outside-policy.json", payload)


if __name__ == "__main__":
    unittest.main()
