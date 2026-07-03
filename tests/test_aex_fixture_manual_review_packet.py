import importlib.util
import json
import sys
import time
import unittest
from pathlib import Path


LAB_ROOT = Path(__file__).resolve().parents[1]
TOOLS_ROOT = LAB_ROOT.parent


def load_tool(name: str):
    path = LAB_ROOT / "tools" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


aex_fixture_manual_review_packet = load_tool("aex_fixture_manual_review_packet")


def make_candidate() -> dict:
    return {
        "relative_path": "AEPluginBuild\\ScatterMap.aex",
        "file_name": "ScatterMap.aex",
        "size_bytes": 201216,
        "compatibility_class": "classic_pf_effect_candidate",
        "fixture_candidate_score": 95,
        "fixture_candidate_reasons": ["pe-valid", "x64", "dll-image", "pipl-signal", "EffectMain-export"],
        "pipl_signal_present": True,
        "effect_main_export_present": True,
        "aegp_marker_count": 0,
        "import_dll_names": ["KERNEL32.dll", "VCRUNTIME140.dll"],
    }


def make_dossier() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_review_dossier",
        "candidate": make_candidate(),
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "dossier_state": "manual_review_pending",
        "load_approval_recommendation": "do_not_approve_yet_manual_review_pending",
        "risk_flags": [],
        "review_items": [
            {"id": "classic_pf_static_classification", "status": "pass"},
            {"id": "fixture_decision", "status": "pending"},
        ],
        "manual_review_questions": ["Is provenance safe?"],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_dependency_review(recommendation: str = "do_not_open_native_load_gate") -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_dependency_review_packet",
        "review_state": "dependency_review_pending_native_load_blocked",
        "native_load_recommendation": recommendation,
        "review_items": [
            {
                "dll_name": "ucrtbased.dll",
                "review_severity": "blocker",
                "review_state": "native_load_blocker",
            }
        ],
        "summary": {"native_load_blocker_count": 1, "review_severity_counts": {"blocker": 1}},
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
        "primary_review_candidate": make_candidate(),
        "approval_state": "present",
        "dependency_review_state": "present",
        "dependency_native_load_recommendation": "do_not_open_native_load_gate",
        "gate_state": "closed_dependency_review_or_invalid_approval",
        "gate_errors": [
            "dependency review recommendation blocks native load",
            "fixture decision is not an approval: hold_for_manual_review",
        ],
        "gates": [],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def write_wiztree_csv() -> Path:
    root = TOOLS_ROOT / "WizTree MCP" / "exports"
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{time.time_ns()}-aex-fixtures.csv"
    path.write_text(
        "\n".join(
            [
                "生成したソフトウェア WizTree 4.19",
                "ファイル名,サイズ,割り当て,更新日時,属性,ファイル数,フォルダー",
                f"\"{TOOLS_ROOT}\\Ae_Plugins\\AEPluginBuild\\ScatterMap.aex\",201216,204800,2026/03/27 03:34:27,32,0,0",
                f"\"{TOOLS_ROOT}\\Ae_Plugins\\AEPluginBuild\\LargeGpu.aex\",6480896,6483968,2026/04/20 16:16:52,32,0,0",
            ]
        )
        + "\n",
        encoding="utf-8",
    )
    return path


class AexFixtureManualReviewPacketTests(unittest.TestCase):
    def test_packet_summarizes_hold_dependency_and_gate_without_opening_aex(self):
        packet = aex_fixture_manual_review_packet.build_manual_review_packet(
            fixture_dossier=make_dossier(),
            fixture_dossier_path=Path("dossier.json"),
            dependency_review=make_dependency_review(),
            dependency_review_path=Path("dependency.json"),
            load_gate=make_load_gate(),
            load_gate_path=Path("gate.json"),
            wiztree_csv_path=write_wiztree_csv(),
        )

        self.assertEqual(packet["report_kind"], "aex_fixture_manual_review_packet")
        self.assertEqual(packet["review_packet_state"], "fixture_manual_review_packet_ready_no_load")
        self.assertTrue(packet["manual_review_ready"])
        self.assertFalse(packet["approval_ready"])
        self.assertFalse(packet["native_load_performed"])
        self.assertFalse(packet["aex_file_opened"])
        self.assertEqual(packet["recommended_next_decision"], "keep_hold_pending_manual_review")
        blocker_ids = {item["id"] for item in packet["approval_blockers"]}
        self.assertIn("fixture_not_approved", blocker_ids)
        self.assertIn("dependency_review_blocks_native_load", blocker_ids)
        self.assertIn("load_gate_closed", blocker_ids)
        self.assertEqual(packet["wiztree_aex_inventory"]["inventory_state"], "wiztree_csv_read_metadata_only")
        self.assertEqual(packet["wiztree_aex_inventory"]["aex_file_count"], 2)
        self.assertEqual(packet["wiztree_aex_inventory"]["candidate_match_count"], 1)
        self.assertTrue(packet["wiztree_aex_inventory"]["candidate_size_match"])

    def test_invalid_or_mismatched_evidence_is_rejected(self):
        dossier = make_dossier()
        dossier["aex_file_opened"] = True
        with self.assertRaises(ValueError):
            aex_fixture_manual_review_packet.build_manual_review_packet(
                fixture_dossier=dossier,
                fixture_dossier_path=Path("dossier.json"),
                dependency_review=make_dependency_review(),
                dependency_review_path=Path("dependency.json"),
                load_gate=make_load_gate(),
                load_gate_path=Path("gate.json"),
            )

        gate = make_load_gate()
        gate["primary_review_candidate"]["relative_path"] = "Other.aex"
        with self.assertRaises(ValueError):
            aex_fixture_manual_review_packet.build_manual_review_packet(
                fixture_dossier=make_dossier(),
                fixture_dossier_path=Path("dossier.json"),
                dependency_review=make_dependency_review(),
                dependency_review_path=Path("dependency.json"),
                load_gate=gate,
                load_gate_path=Path("gate.json"),
            )

    def test_paths_are_confined_and_output_is_create_new(self):
        roots = {
            "dossier": LAB_ROOT / "target" / "fixture-dossier",
            "dependency": LAB_ROOT / "target" / "dependency-review",
            "gate": LAB_ROOT / "target" / "load-gate",
        }
        for root in roots.values():
            root.mkdir(parents=True, exist_ok=True)
        stamp = time.time_ns()
        dossier_path = roots["dossier"] / f"{stamp}-dossier.local.json"
        dependency_path = roots["dependency"] / f"{stamp}-dependency.local.json"
        gate_path = roots["gate"] / f"{stamp}-gate.local.json"
        dossier_path.write_text(json.dumps(make_dossier()), encoding="utf-8")
        dependency_path.write_text(json.dumps(make_dependency_review()), encoding="utf-8")
        gate_path.write_text(json.dumps(make_load_gate()), encoding="utf-8")

        dossier, resolved_dossier = aex_fixture_manual_review_packet.load_fixture_dossier(dossier_path)
        dependency, resolved_dependency = aex_fixture_manual_review_packet.load_dependency_review(dependency_path)
        gate, resolved_gate = aex_fixture_manual_review_packet.load_load_gate(gate_path)
        self.assertEqual(resolved_dossier, dossier_path.resolve())
        self.assertEqual(resolved_dependency, dependency_path.resolve())
        self.assertEqual(resolved_gate, gate_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-dossier.json"
        outside.write_text(json.dumps(make_dossier()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_fixture_manual_review_packet.load_fixture_dossier(outside)

        packet = aex_fixture_manual_review_packet.build_manual_review_packet(
            fixture_dossier=dossier,
            fixture_dossier_path=resolved_dossier,
            dependency_review=dependency,
            dependency_review_path=resolved_dependency,
            load_gate=gate,
            load_gate_path=resolved_gate,
        )
        out = LAB_ROOT / "target" / "fixture-manual-review" / f"{time.time_ns()}-manual-review.local.json"
        written = aex_fixture_manual_review_packet.write_json_create_new(out, packet)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_fixture_manual_review_packet.write_json_create_new(out, packet)
        with self.assertRaises(ValueError):
            aex_fixture_manual_review_packet.write_json_create_new(
                LAB_ROOT / "target" / "outside-manual-review.json",
                packet,
            )

        csv_path = write_wiztree_csv()
        loaded_csv = aex_fixture_manual_review_packet.validate_wiztree_csv(csv_path)
        self.assertEqual(loaded_csv, csv_path.resolve())


if __name__ == "__main__":
    unittest.main()
