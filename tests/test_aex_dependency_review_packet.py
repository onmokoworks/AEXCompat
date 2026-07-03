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


aex_dependency_review_packet = load_tool("aex_dependency_review_packet")


def make_preflight() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_dependency_availability_preflight",
        "preflight_state": "dependency_availability_preflight_ready_no_load",
        "availability_check": "filesystem_exists_only_no_load",
        "summary": {"found_count": 3},
        "dependency_rows": [
            {
                "dll_name": "kernel32.dll",
                "category": "core_windows",
                "candidate_count": 3,
                "availability_status": "found_in_search_path",
                "policy_review_state": "available_for_first_sandbox_design_review",
                "example_candidates": ["AEPluginBuild\\ScatterMap.aex"],
            },
            {
                "dll_name": "ucrtbased.dll",
                "category": "debug_crt_runtime",
                "candidate_count": 1,
                "availability_status": "found_in_search_path",
                "policy_review_state": "default_deny_dependency",
                "example_candidates": ["AEPluginBuild\\Debug.aex"],
            },
            {
                "dll_name": "opengl32.dll",
                "category": "graphics_or_gpu",
                "candidate_count": 2,
                "availability_status": "found_in_search_path",
                "policy_review_state": "manual_review_required",
                "example_candidates": ["AEPluginBuild\\Gpu.aex"],
            },
            {
                "dll_name": "api-ms-win-crt-time-l1-1-0.dll",
                "category": "windows_crt_api_set",
                "candidate_count": 1,
                "availability_status": "api_set_virtual_or_not_found_review",
                "policy_review_state": "api_set_resolution_review",
                "example_candidates": ["AEPluginBuild\\ApiSet.aex"],
            },
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


class AexDependencyReviewPacketTests(unittest.TestCase):
    def test_review_packet_keeps_native_load_blocked_for_default_deny_and_reviews(self):
        packet = aex_dependency_review_packet.build_dependency_review_packet(make_preflight(), Path("preflight.json"))
        self.assertEqual(packet["packet_kind"], "aex_dependency_review_packet")
        self.assertEqual(packet["review_state"], "dependency_review_pending_native_load_blocked")
        self.assertEqual(packet["native_load_recommendation"], "do_not_open_native_load_gate")
        self.assertFalse(packet["native_load_performed"])
        self.assertFalse(packet["dll_load_performed"])
        self.assertFalse(packet["aex_file_opened"])

        by_dll = {item["dll_name"]: item for item in packet["review_items"]}
        self.assertEqual(by_dll["kernel32.dll"]["review_state"], "available_dependency")
        self.assertEqual(by_dll["ucrtbased.dll"]["review_state"], "native_load_blocker")
        self.assertEqual(by_dll["ucrtbased.dll"]["review_severity"], "blocker")
        self.assertEqual(by_dll["opengl32.dll"]["review_state"], "manual_policy_review_required")
        self.assertEqual(
            by_dll["api-ms-win-crt-time-l1-1-0.dll"]["review_state"],
            "api_set_resolution_review_required",
        )

        summary = packet["summary"]
        self.assertEqual(summary["available_dependency_count"], 1)
        self.assertEqual(summary["native_load_blocker_count"], 1)
        self.assertEqual(summary["manual_policy_review_required_count"], 1)
        self.assertEqual(summary["api_set_resolution_review_required_count"], 1)

    def test_invalid_or_unsafe_preflight_is_rejected(self):
        preflight = make_preflight()
        preflight["dll_load_performed"] = True
        with self.assertRaises(ValueError):
            aex_dependency_review_packet.build_dependency_review_packet(preflight, Path("preflight.json"))

        preflight = make_preflight()
        preflight["preflight_state"] = "incomplete"
        with self.assertRaises(ValueError):
            aex_dependency_review_packet.build_dependency_review_packet(preflight, Path("preflight.json"))

    def test_paths_are_confined_and_output_is_create_new(self):
        preflight_root = LAB_ROOT / "target" / "dependency-preflight"
        preflight_root.mkdir(parents=True, exist_ok=True)
        source = preflight_root / f"{time.time_ns()}-review-source.local.json"
        source.write_text(json.dumps(make_preflight()), encoding="utf-8")
        loaded, resolved = aex_dependency_review_packet.load_preflight(source)
        self.assertEqual(loaded["report_kind"], "aex_dependency_availability_preflight")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-review.json"
        outside.write_text(json.dumps(make_preflight()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_dependency_review_packet.load_preflight(outside)

        payload = aex_dependency_review_packet.build_dependency_review_packet(loaded, resolved)
        out = LAB_ROOT / "target" / "dependency-review" / f"{time.time_ns()}-dependency-review.local.json"
        written = aex_dependency_review_packet.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_dependency_review_packet.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_dependency_review_packet.write_json_create_new(
                LAB_ROOT / "target" / "outside-dependency-review.json",
                payload,
            )


if __name__ == "__main__":
    unittest.main()
