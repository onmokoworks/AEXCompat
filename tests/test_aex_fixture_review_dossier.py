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


aex_fixture_review_dossier = load_tool("aex_fixture_review_dossier")


def candidate(**overrides):
    base = {
        "relative_path": "AEPluginBuild\\ScatterMap.aex",
        "file_name": "ScatterMap.aex",
        "size_bytes": 201216,
        "compatibility_class": "classic_pf_effect_candidate",
        "fixture_candidate_score": 95,
        "fixture_candidate_reasons": ["pe-valid", "x64", "dll-image", "pipl-signal", "EffectMain-export"],
        "machine_label": "x64",
        "pipl_signal_present": True,
        "effect_main_export_present": True,
        "effect_main_marker_present": True,
        "aegp_marker_count": 0,
        "resource_types": ["#16", "PIPL"],
        "pipl_resource_data_entry_count": 1,
        "pipl_resource_total_size": 12,
        "pipl_resource_entries": [
            {"type": "PIPL", "name": 16000, "language": 1033, "data_rva": 0x1300, "size_bytes": 12, "codepage": 1200}
        ],
        "import_dll_names": ["KERNEL32.dll", "VCRUNTIME140.dll"],
        "review_status": "static_review_candidate",
    }
    base.update(overrides)
    return base


def make_manifest() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "manifest_kind": "aex_fixture_review_manifest",
        "selected_candidates": [candidate()],
        "hold_candidates": [
            candidate(
                relative_path="AEPluginBuild\\DebugGpu.aex",
                file_name="DebugGpu.aex",
                compatibility_class="classic_pf_effect_with_aegp_markers",
                aegp_marker_count=4,
                import_dll_names=["KERNEL32.dll", "ucrtbased.dll", "OPENGL32.dll"],
                review_status="hold_for_later_review",
            )
        ],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
    }


def make_decision(relative_path: str = "AEPluginBuild\\ScatterMap.aex", decision: str = "hold") -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "manifest_kind": "aex_fixture_decision_manifest",
        "candidate_relative_path": relative_path,
        "decision": decision,
        "decision_state": "hold_for_manual_review" if decision == "hold" else "rejected_for_load_gate",
        "approval_state": "not_approved_for_load_gate",
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
    }


class AexFixtureReviewDossierTests(unittest.TestCase):
    def test_selected_candidate_dossier_stays_pending_and_no_load(self):
        dossier = aex_fixture_review_dossier.build_dossier(
            make_manifest(),
            Path("manifest.json"),
            make_decision(),
            Path("decision.json"),
        )
        self.assertEqual(dossier["report_kind"], "aex_fixture_review_dossier")
        self.assertEqual(dossier["dossier_state"], "manual_review_pending")
        self.assertEqual(dossier["load_approval_recommendation"], "do_not_approve_yet_manual_review_pending")
        self.assertEqual(dossier["risk_flags"], [])
        self.assertFalse(dossier["native_load_performed"])
        self.assertFalse(dossier["aex_file_opened"])

        by_id = {item["id"]: item for item in dossier["review_items"]}
        self.assertEqual(by_id["classic_pf_static_classification"]["status"], "pass")
        self.assertEqual(by_id["fixture_decision"]["status"], "pending")
        self.assertEqual(by_id["no_runtime_action"]["status"], "pass")

    def test_hold_candidate_risk_flags_are_reported(self):
        dossier = aex_fixture_review_dossier.build_dossier(
            make_manifest(),
            Path("manifest.json"),
            make_decision("AEPluginBuild\\DebugGpu.aex"),
            Path("decision.json"),
        )
        self.assertIn("not_classic_pf_effect_candidate", dossier["risk_flags"])
        self.assertIn("aegp_markers_present", dossier["risk_flags"])
        self.assertIn("debug_runtime_imports_present", dossier["risk_flags"])
        self.assertIn("graphics_or_gpu_imports_present", dossier["risk_flags"])
        self.assertIn("not_in_selected_candidate_bucket", dossier["risk_flags"])
        self.assertEqual(
            dossier["load_approval_recommendation"],
            "do_not_approve_yet_static_risks_or_review_items_present",
        )

    def test_candidate_must_match_decision(self):
        with self.assertRaises(ValueError):
            aex_fixture_review_dossier.build_dossier(
                make_manifest(),
                Path("manifest.json"),
                make_decision(),
                Path("decision.json"),
                candidate_relative_path="AEPluginBuild\\DebugGpu.aex",
            )

    def test_paths_are_confined_and_output_is_create_new(self):
        manifest_root = LAB_ROOT / "target" / "fixture-review"
        decision_root = LAB_ROOT / "target" / "fixture-approval"
        manifest_root.mkdir(parents=True, exist_ok=True)
        decision_root.mkdir(parents=True, exist_ok=True)
        manifest_path = manifest_root / f"{time.time_ns()}-dossier-source.local.json"
        decision_path = decision_root / f"{time.time_ns()}-dossier-decision.local.json"
        manifest_path.write_text(json.dumps(make_manifest()), encoding="utf-8")
        decision_path.write_text(json.dumps(make_decision()), encoding="utf-8")

        manifest, resolved_manifest = aex_fixture_review_dossier.load_manifest(manifest_path)
        decision, resolved_decision = aex_fixture_review_dossier.load_decision(decision_path)
        self.assertEqual(resolved_manifest, manifest_path.resolve())
        self.assertEqual(resolved_decision, decision_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-dossier.json"
        outside.write_text(json.dumps(make_manifest()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_fixture_review_dossier.load_manifest(outside)

        payload = aex_fixture_review_dossier.build_dossier(manifest, resolved_manifest, decision, resolved_decision)
        out = LAB_ROOT / "target" / "fixture-dossier" / f"{time.time_ns()}-dossier.local.json"
        written = aex_fixture_review_dossier.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_fixture_review_dossier.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_fixture_review_dossier.write_json_create_new(LAB_ROOT / "target" / "outside-dossier.json", payload)


if __name__ == "__main__":
    unittest.main()
