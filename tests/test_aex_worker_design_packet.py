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


aex_worker_design_packet = load_tool("aex_worker_design_packet")


def make_candidate(relative_path: str = "AEPluginBuild\\ScatterMap.aex") -> dict:
    return {
        "relative_path": relative_path,
        "file_name": Path(relative_path).name,
        "size_bytes": 201216,
        "compatibility_class": "classic_pf_effect_candidate",
        "fixture_candidate_score": 95,
        "machine_label": "x64",
        "pipl_signal_present": True,
        "effect_main_export_present": True,
        "effect_main_marker_present": True,
        "aegp_marker_count": 0,
        "resource_types": ["PIPL", "#16"],
        "import_dll_names": ["KERNEL32.dll"],
        "review_status": "static_review_candidate",
    }


def make_manifest() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "manifest_kind": "aex_fixture_review_manifest",
        "source_report": "target/aex-static-probe/source.json",
        "source_summary": {"aex_count": 1},
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "safety_gate": {
            "state": "review_manifest_only",
            "blocked_actions": [
                "load_aex_dll",
                "call_EffectMain",
                "render_with_aex",
                "route_through_ofx",
            ],
        },
        "selected_candidates": [make_candidate()],
        "hold_candidates": [],
    }


class AexWorkerDesignPacketTests(unittest.TestCase):
    def test_build_packet_preserves_no_load_boundary(self):
        payload = aex_worker_design_packet.build_packet_payload(make_manifest(), Path("manifest.json"))
        self.assertEqual(payload["packet_kind"], "aex_worker_sandbox_design_packet")
        self.assertEqual(payload["design_state"], "no_load_worker_boundary_only")
        self.assertFalse(payload["native_load_performed"])
        self.assertFalse(payload["render_performed"])
        self.assertFalse(payload["ae_invoked"])
        self.assertFalse(payload["ofx_route_invoked"])
        self.assertFalse(payload["private_payload_copied"])
        self.assertEqual(payload["primary_review_candidate"]["relative_path"], "AEPluginBuild\\ScatterMap.aex")
        self.assertEqual(payload["primary_review_candidate"]["approval_state"], "not_approved_for_load")
        self.assertIn("load_aex_dll", payload["blocked_actions"])
        self.assertEqual(payload["ofx_position"]["state"], "deferred")
        self.assertIn("inspect_ppm", json.dumps(payload["ipc_protocol"]))

    def test_rejects_unsafe_or_bad_manifest(self):
        unsafe = make_manifest()
        unsafe["native_load_performed"] = True
        with self.assertRaises(ValueError):
            aex_worker_design_packet.build_packet_payload(unsafe, Path("manifest.json"))

        bad_candidate = make_manifest()
        bad_candidate["selected_candidates"][0]["aegp_marker_count"] = 4
        with self.assertRaises(ValueError):
            aex_worker_design_packet.build_packet_payload(bad_candidate, Path("manifest.json"))

        missing_block = make_manifest()
        missing_block["safety_gate"]["blocked_actions"] = ["call_EffectMain"]
        with self.assertRaises(ValueError):
            aex_worker_design_packet.build_packet_payload(missing_block, Path("manifest.json"))

    def test_manifest_must_be_under_fixture_review_root(self):
        manifest_root = LAB_ROOT / "target" / "fixture-review"
        manifest_root.mkdir(parents=True, exist_ok=True)
        source = manifest_root / f"{time.time_ns()}-worker-source.json"
        source.write_text(json.dumps(make_manifest()), encoding="utf-8")
        loaded, resolved = aex_worker_design_packet.load_manifest(source)
        self.assertEqual(loaded["manifest_kind"], "aex_fixture_review_manifest")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-manifest.json"
        outside.write_text(json.dumps(make_manifest()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_worker_design_packet.load_manifest(outside)

    def test_packet_writer_is_create_new_under_packet_root(self):
        packet = aex_worker_design_packet.build_packet_payload(make_manifest(), Path("manifest.json"))
        path = LAB_ROOT / "target" / "worker-design" / f"{time.time_ns()}-packet.local.json"
        written = aex_worker_design_packet.write_json_create_new(path, packet)
        self.assertEqual(written, path.resolve())
        with self.assertRaises(FileExistsError):
            aex_worker_design_packet.write_json_create_new(path, packet)
        with self.assertRaises(ValueError):
            aex_worker_design_packet.write_json_create_new(LAB_ROOT / "target" / "outside-packet.json", packet)


if __name__ == "__main__":
    unittest.main()
