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


aex_candidate_dependency_scope_packet = load_tool("aex_candidate_dependency_scope_packet")


def make_manual_review() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_manual_review_packet",
        "review_packet_state": "fixture_manual_review_packet_ready_no_load",
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "candidate": {
            "relative_path": "AEPluginBuild\\ScatterMap.aex",
            "file_name": "ScatterMap.aex",
            "import_dll_names": [
                "KERNEL32.dll",
                "VCRUNTIME140.dll",
                "api-ms-win-crt-runtime-l1-1-0.dll",
            ],
        },
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_dependency_review() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_dependency_review_packet",
        "review_state": "dependency_review_pending_native_load_blocked",
        "native_load_recommendation": "do_not_open_native_load_gate",
        "summary": {"native_load_blocker_count": 1},
        "review_items": [
            {
                "dll_name": "kernel32.dll",
                "category": "core_windows",
                "availability_status": "found_in_search_path",
                "policy_review_state": "available_for_first_sandbox_design_review",
                "review_state": "available_dependency",
                "review_severity": "informational",
                "example_candidates": ["AEPluginBuild\\ScatterMap.aex"],
            },
            {
                "dll_name": "vcruntime140.dll",
                "category": "release_crt_runtime",
                "availability_status": "found_in_search_path",
                "policy_review_state": "available_for_first_sandbox_design_review",
                "review_state": "available_dependency",
                "review_severity": "informational",
                "example_candidates": ["AEPluginBuild\\ScatterMap.aex"],
            },
            {
                "dll_name": "ucrtbased.dll",
                "category": "debug_crt_runtime",
                "availability_status": "found_in_search_path",
                "policy_review_state": "default_deny_dependency",
                "review_state": "native_load_blocker",
                "review_severity": "blocker",
                "example_candidates": ["AEPluginBuild\\DebugThing.aex"],
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


def make_dependency_preflight() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_dependency_availability_preflight",
        "preflight_state": "dependency_availability_preflight_ready_no_load",
        "dependency_rows": [
            {
                "dll_name": "kernel32.dll",
                "availability_status": "found_in_search_path",
                "policy_review_state": "available_for_first_sandbox_design_review",
                "found_paths": ["C:\\Windows\\System32\\kernel32.dll"],
                "checked_search_directory_count": 2,
            },
            {
                "dll_name": "vcruntime140.dll",
                "availability_status": "found_in_search_path",
                "policy_review_state": "available_for_first_sandbox_design_review",
                "found_paths": ["C:\\Windows\\System32\\vcruntime140.dll"],
                "checked_search_directory_count": 2,
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


def make_load_gate() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_load_gate_check",
        "primary_review_candidate": {"relative_path": "AEPluginBuild\\ScatterMap.aex"},
        "gate_state": "closed_dependency_review_or_invalid_approval",
        "dependency_native_load_recommendation": "do_not_open_native_load_gate",
        "gate_errors": ["dependency review recommendation blocks native load"],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


class AexCandidateDependencyScopePacketTests(unittest.TestCase):
    def test_candidate_scope_separates_global_blockers_from_selected_candidate(self):
        packet = aex_candidate_dependency_scope_packet.build_candidate_dependency_scope_packet(
            fixture_manual_review=make_manual_review(),
            fixture_manual_review_path=Path("manual.json"),
            dependency_review=make_dependency_review(),
            dependency_review_path=Path("dependency.json"),
            dependency_preflight=make_dependency_preflight(),
            dependency_preflight_path=Path("preflight.json"),
            load_gate=make_load_gate(),
            load_gate_path=Path("gate.json"),
        )

        self.assertEqual(packet["report_kind"], "aex_candidate_dependency_scope_packet")
        self.assertEqual(packet["candidate_dependency_scope_state"], "candidate_dependency_scope_ready_no_load")
        self.assertTrue(packet["candidate_scope_ready"])
        self.assertFalse(packet["candidate_dependency_blockers_present"])
        self.assertTrue(packet["global_dependency_blockers_present"])
        self.assertFalse(packet["global_dependency_blockers_apply_to_candidate"])
        self.assertEqual(
            packet["scoped_gate_recommendation"],
            "candidate_dependencies_clear_global_gate_still_closed",
        )
        self.assertFalse(packet["native_load_performed"])
        self.assertFalse(packet["dll_load_performed"])
        self.assertFalse(packet["aex_file_opened"])
        self.assertEqual(packet["candidate_dependency_blocker_count"], 0)
        self.assertEqual(packet["candidate_dependency_missing_or_api_set_review_count"], 0)
        self.assertFalse(packet["candidate_dependency_found_paths_exported"])
        self.assertEqual(packet["candidate_dependency_summary"]["candidate_dependency_blocker_count"], 0)
        by_dll = {row["dll_name"]: row for row in packet["candidate_dependency_rows"]}
        self.assertEqual(by_dll["kernel32.dll"]["found_paths_count"], 1)
        self.assertFalse(by_dll["kernel32.dll"]["found_paths_exported"])

    def test_candidate_dependency_blocker_is_reported_when_import_matches(self):
        manual = make_manual_review()
        manual["candidate"]["import_dll_names"].append("ucrtbased.dll")
        packet = aex_candidate_dependency_scope_packet.build_candidate_dependency_scope_packet(
            fixture_manual_review=manual,
            fixture_manual_review_path=Path("manual.json"),
            dependency_review=make_dependency_review(),
            dependency_review_path=Path("dependency.json"),
            load_gate=make_load_gate(),
            load_gate_path=Path("gate.json"),
        )
        self.assertTrue(packet["candidate_dependency_blockers_present"])
        self.assertTrue(packet["global_dependency_blockers_apply_to_candidate"])
        self.assertEqual(
            packet["scoped_gate_recommendation"],
            "keep_gate_closed_candidate_dependency_blockers_present",
        )

    def test_invalid_or_mismatched_evidence_is_rejected(self):
        manual = make_manual_review()
        manual["dll_load_performed"] = True
        with self.assertRaises(ValueError):
            aex_candidate_dependency_scope_packet.build_candidate_dependency_scope_packet(
                fixture_manual_review=manual,
                fixture_manual_review_path=Path("manual.json"),
                dependency_review=make_dependency_review(),
                dependency_review_path=Path("dependency.json"),
                load_gate=make_load_gate(),
                load_gate_path=Path("gate.json"),
            )

        gate = make_load_gate()
        gate["primary_review_candidate"]["relative_path"] = "Other.aex"
        with self.assertRaises(ValueError):
            aex_candidate_dependency_scope_packet.build_candidate_dependency_scope_packet(
                fixture_manual_review=make_manual_review(),
                fixture_manual_review_path=Path("manual.json"),
                dependency_review=make_dependency_review(),
                dependency_review_path=Path("dependency.json"),
                load_gate=gate,
                load_gate_path=Path("gate.json"),
            )

    def test_paths_are_confined_and_output_is_create_new(self):
        roots = {
            "manual": LAB_ROOT / "target" / "fixture-manual-review",
            "dependency": LAB_ROOT / "target" / "dependency-review",
            "preflight": LAB_ROOT / "target" / "dependency-preflight",
            "gate": LAB_ROOT / "target" / "load-gate",
        }
        for root in roots.values():
            root.mkdir(parents=True, exist_ok=True)
        stamp = time.time_ns()
        manual_path = roots["manual"] / f"{stamp}-manual.local.json"
        dependency_path = roots["dependency"] / f"{stamp}-dependency.local.json"
        preflight_path = roots["preflight"] / f"{stamp}-preflight.local.json"
        gate_path = roots["gate"] / f"{stamp}-gate.local.json"
        manual_path.write_text(json.dumps(make_manual_review()), encoding="utf-8")
        dependency_path.write_text(json.dumps(make_dependency_review()), encoding="utf-8")
        preflight_path.write_text(json.dumps(make_dependency_preflight()), encoding="utf-8")
        gate_path.write_text(json.dumps(make_load_gate()), encoding="utf-8")

        manual, resolved_manual = aex_candidate_dependency_scope_packet.load_fixture_manual_review(manual_path)
        dependency, resolved_dependency = aex_candidate_dependency_scope_packet.load_dependency_review(dependency_path)
        preflight, resolved_preflight = aex_candidate_dependency_scope_packet.load_dependency_preflight(preflight_path)
        gate, resolved_gate = aex_candidate_dependency_scope_packet.load_load_gate(gate_path)
        self.assertEqual(resolved_manual, manual_path.resolve())
        self.assertEqual(resolved_dependency, dependency_path.resolve())
        self.assertEqual(resolved_preflight, preflight_path.resolve())
        self.assertEqual(resolved_gate, gate_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-manual.json"
        outside.write_text(json.dumps(make_manual_review()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_candidate_dependency_scope_packet.load_fixture_manual_review(outside)

        packet = aex_candidate_dependency_scope_packet.build_candidate_dependency_scope_packet(
            fixture_manual_review=manual,
            fixture_manual_review_path=resolved_manual,
            dependency_review=dependency,
            dependency_review_path=resolved_dependency,
            dependency_preflight=preflight,
            dependency_preflight_path=resolved_preflight,
            load_gate=gate,
            load_gate_path=resolved_gate,
        )
        out = LAB_ROOT / "target" / "candidate-dependency-scope" / f"{time.time_ns()}-scope.local.json"
        written = aex_candidate_dependency_scope_packet.write_json_create_new(out, packet)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_candidate_dependency_scope_packet.write_json_create_new(out, packet)
        with self.assertRaises(ValueError):
            aex_candidate_dependency_scope_packet.write_json_create_new(
                LAB_ROOT / "target" / "outside-scope.json",
                packet,
            )


if __name__ == "__main__":
    unittest.main()
