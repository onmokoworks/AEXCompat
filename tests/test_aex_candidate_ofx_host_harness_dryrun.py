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


aex_candidate_ofx_host_harness_dryrun = load_tool("aex_candidate_ofx_host_harness_dryrun")


SAFETY_FALSE = {
    "native_load_enabled": False,
    "native_load_performed": False,
    "dll_load_performed": False,
    "render_performed": False,
    "ae_invoked": False,
    "ofx_route_invoked": False,
    "private_payload_copied": False,
    "aex_file_opened": False,
    "aex_file_hashed": False,
    "aex_file_copied": False,
    "aepx_file_modified": False,
    "aep_binary_modified": False,
    "ae_project_write_performed": False,
    "ofx_plugin_built": False,
    "ofx_describe_performed": False,
    "ofx_render_performed": False,
    "aex_render_performed": False,
    "render_validation_performed": False,
    "pipl_payload_parsed": False,
    "parameter_schema_emitted": False,
    "redacted_schema_emitted": False,
    "real_pipl_payload_parser_enabled": False,
    "real_pipl_payload_parsed": False,
    "resource_payload_opened": False,
    "resource_payload_extracted": False,
    "raw_payload_serialized": False,
}

BLOCKED = [
    "accept_aex_path",
    "open_aex_file",
    "hash_aex_file",
    "copy_selected_aex_fixture",
    "load_aex_dll",
    "load_dependency_dll",
    "call_EffectMain",
    "dispatch_PF_Cmd",
    "start_after_effects",
    "render_with_aex",
    "route_through_real_ofx",
    "build_ofx_binary",
    "instantiate_ofx_runtime",
    "ofx_describe_from_aex",
    "ofx_render_with_aex",
    "parse_real_pipl_payload",
    "emit_parameter_schema",
    "emit_redacted_schema",
]


def bridge_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_bridge_packet",
        "bridge_state": "candidate_ofx_bridge_ready_no_load_route_closed",
        "bridge_ready": True,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "bridge_allowed_route": "no_op_identity_only",
        "mock_route_ready": True,
        "real_route_open": False,
        "real_ofx_route_ready": False,
        "ofx_runtime_invoked": False,
        "aex_runtime_invoked": False,
        "ofx_describe_ready": False,
        "ofx_render_ready": False,
        "render_equivalence_claim_ready": False,
        "absolute_ppm_paths_exported": False,
        "absolute_aex_paths_exported": False,
        "ofx_bridge_path_payload_exported": False,
        "image_surface": {
            "state": "candidate_mock_output_available_relative_path_only",
            "operation": "invert",
            "input_ppm_relative": "target/ppm-fixtures/in.ppm",
            "output_ppm_relative": "target/candidate-image-compat-mock/out.ppm",
            "input_ppm_absolute_path_exported": False,
            "output_ppm_absolute_path_exported": False,
        },
        "blocked_actions": BLOCKED,
        **SAFETY_FALSE,
    }


def write_bridge(name: str, payload: dict | None = None) -> Path:
    root = LAB_ROOT / "target" / "candidate-ofx-bridge"
    root.mkdir(parents=True, exist_ok=True)
    path = root / name
    path.write_text(json.dumps(payload or bridge_payload()), encoding="utf-8")
    return path


class AexCandidateOfxHostHarnessDryrunTests(unittest.TestCase):
    def test_builds_no_execution_harness_dryrun(self):
        stamp = time.time_ns()
        bridge_path = LAB_ROOT / "target" / "candidate-ofx-bridge" / f"ae-candidate-ofx-bridge-{stamp}.local.json"
        bridge_path.parent.mkdir(parents=True, exist_ok=True)
        packet = aex_candidate_ofx_host_harness_dryrun.build_harness_dryrun(
            bridge=bridge_payload(),
            bridge_path=bridge_path,
        )

        self.assertEqual(packet["report_kind"], "aex_candidate_ofx_host_harness_dryrun")
        self.assertEqual(packet["harness_dryrun_state"], "candidate_ofx_host_harness_dryrun_ready_route_closed")
        self.assertTrue(packet["harness_dryrun_ready"])
        self.assertTrue(packet["dry_run_only"])
        self.assertFalse(packet["would_execute"])
        self.assertFalse(packet["execution_performed"])
        self.assertEqual(packet["planned_case_count"], 2)
        self.assertEqual(packet["planned_noop_describe_case_count"], 1)
        self.assertEqual(packet["planned_noop_render_case_count"], 1)
        self.assertEqual(packet["planned_real_describe_case_count"], 0)
        self.assertEqual(packet["planned_real_render_case_count"], 0)
        self.assertFalse(packet["real_route_open"])
        self.assertFalse(packet["ofx_runtime_invoked"])
        self.assertFalse(packet["ofx_describe_performed"])
        self.assertFalse(packet["ofx_render_performed"])
        self.assertFalse(packet["aex_file_opened"])
        self.assertFalse(packet["pipl_payload_parsed"])
        self.assertTrue(packet["requires_future_runtime_approval"])
        self.assertIn("instantiate_ofx_runtime", packet["blocked_actions"])

    def test_rejects_open_bridge_or_runtime_invocation(self):
        bridge = bridge_payload()
        bridge["real_route_open"] = True
        with self.assertRaises(ValueError):
            aex_candidate_ofx_host_harness_dryrun.build_harness_dryrun(
                bridge=bridge,
                bridge_path=Path("target/candidate-ofx-bridge/bridge.json"),
            )

        bridge = bridge_payload()
        bridge["ofx_runtime_invoked"] = True
        with self.assertRaises(ValueError):
            aex_candidate_ofx_host_harness_dryrun.build_harness_dryrun(
                bridge=bridge,
                bridge_path=Path("target/candidate-ofx-bridge/bridge.json"),
            )

    def test_loads_bridge_and_writes_create_new_under_root(self):
        stamp = time.time_ns()
        bridge_path = write_bridge(f"ae-candidate-ofx-bridge-{stamp}.local.json")
        bridge, resolved_bridge = aex_candidate_ofx_host_harness_dryrun.load_bridge(
            Path("target") / "candidate-ofx-bridge" / bridge_path.name
        )
        packet = aex_candidate_ofx_host_harness_dryrun.build_harness_dryrun(
            bridge=bridge,
            bridge_path=resolved_bridge,
        )
        out = LAB_ROOT / "target" / "candidate-ofx-host-harness-dryrun" / f"ae-candidate-ofx-host-harness-dryrun-{stamp}.local.json"
        written = aex_candidate_ofx_host_harness_dryrun.write_json_create_new(out, packet)
        self.assertEqual(written, out)
        with self.assertRaises(FileExistsError):
            aex_candidate_ofx_host_harness_dryrun.write_json_create_new(out, packet)
        with self.assertRaises(ValueError):
            aex_candidate_ofx_host_harness_dryrun.write_json_create_new(
                LAB_ROOT / "target" / "outside-candidate-ofx-host-harness-dryrun.json", packet
            )
        with self.assertRaises(ValueError):
            aex_candidate_ofx_host_harness_dryrun.load_bridge(
                LAB_ROOT / "target" / "outside-candidate-ofx-bridge.json"
            )


if __name__ == "__main__":
    unittest.main()
