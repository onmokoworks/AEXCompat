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


aex_fixture_manifest = load_tool("aex_fixture_manifest")


def make_entry(relative_path: str, compatibility_class: str, score: int, aegp_count: int = 0) -> dict:
    has_effect_main = compatibility_class != "aegp_or_helper_candidate"
    return {
        "relative_path": relative_path,
        "file_name": Path(relative_path).name,
        "size_bytes": 200000 + score,
        "mtime_utc": "2026-06-05T00:00:00+00:00",
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "pipl_signal_present": True,
        "compatibility_class": compatibility_class,
        "fixture_candidate_score": score,
        "fixture_candidate_reasons": ["pe-valid", "x64", "pipl-signal"],
        "markers": {
            "effect_main_marker_present": has_effect_main,
            "ae_plugin_marker_count": aegp_count,
        },
        "pe": {
            "machine_label": "x64",
            "characteristics_flags": {"dll": True},
            "export_summary": {"effect_main_export_present": has_effect_main},
            "import_summary": {"dll_names": ["KERNEL32.dll"]},
            "resource_summary": {"type_details": [{"type": "PIPL", "entry_count": 1}]},
        },
    }


def make_report() -> dict:
    classic = make_entry("AEPluginBuild\\ScatterMap.aex", "classic_pf_effect_candidate", 95)
    mixed = make_entry(
        "AEPluginBuild\\MaskOffset.aex",
        "classic_pf_effect_with_aegp_markers",
        75,
        aegp_count=17,
    )
    helper = make_entry("AEPluginBuild\\ExEditRemoteAEGP.aex", "aegp_or_helper_candidate", 40, aegp_count=53)
    return {
        "schema_version": 2,
        "publication_status": "local-only",
        "report_kind": "aex_static_probe",
        "input_root": "D:\\Projects\\01_Project\\04_Tools\\Ae_Plugins",
        "summary": {"aex_count": 3},
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "fixture_candidates": [{"relative_path": classic["relative_path"]}],
        "entries": [classic, mixed, helper],
    }


class AexFixtureManifestTests(unittest.TestCase):
    def test_build_manifest_selects_review_and_hold_candidates(self):
        payload = aex_fixture_manifest.build_manifest_payload(make_report(), Path("source.json"))
        self.assertEqual(payload["manifest_kind"], "aex_fixture_review_manifest")
        self.assertFalse(payload["native_load_performed"])
        self.assertEqual(len(payload["selected_candidates"]), 1)
        self.assertEqual(payload["selected_candidates"][0]["relative_path"], "AEPluginBuild\\ScatterMap.aex")
        self.assertEqual(payload["selected_candidates"][0]["review_status"], "static_review_candidate")
        self.assertEqual(len(payload["hold_candidates"]), 2)
        self.assertTrue(any(candidate["aegp_marker_count"] for candidate in payload["hold_candidates"]))
        self.assertIn("load_aex_dll", payload["safety_gate"]["blocked_actions"])

    def test_rejects_unsafe_or_old_source_report(self):
        unsafe = make_report()
        unsafe["native_load_performed"] = True
        with self.assertRaises(ValueError):
            aex_fixture_manifest.build_manifest_payload(unsafe, Path("source.json"))

        old = make_report()
        old["schema_version"] = 1
        with self.assertRaises(ValueError):
            aex_fixture_manifest.build_manifest_payload(old, Path("source.json"))

        entry_unsafe = make_report()
        entry_unsafe["entries"][0]["private_payload_copied"] = True
        with self.assertRaises(ValueError):
            aex_fixture_manifest.build_manifest_payload(entry_unsafe, Path("source.json"))

    def test_source_report_must_be_under_static_probe_root(self):
        source_root = LAB_ROOT / "target" / "aex-static-probe"
        source_root.mkdir(parents=True, exist_ok=True)
        source = source_root / f"{time.time_ns()}-fixture-source.json"
        source.write_text(json.dumps(make_report()), encoding="utf-8")
        loaded, resolved = aex_fixture_manifest.load_source_report(source)
        self.assertEqual(loaded["report_kind"], "aex_static_probe")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-report.json"
        outside.write_text(json.dumps(make_report()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_fixture_manifest.load_source_report(outside)

    def test_manifest_writer_is_create_new_under_manifest_root(self):
        manifest = aex_fixture_manifest.build_manifest_payload(make_report(), Path("source.json"))
        path = LAB_ROOT / "target" / "fixture-review" / f"{time.time_ns()}-manifest.local.json"
        written = aex_fixture_manifest.write_json_create_new(path, manifest)
        self.assertEqual(written, path.resolve())
        with self.assertRaises(FileExistsError):
            aex_fixture_manifest.write_json_create_new(path, manifest)
        with self.assertRaises(ValueError):
            aex_fixture_manifest.write_json_create_new(LAB_ROOT / "target" / "outside-manifest.json", manifest)


if __name__ == "__main__":
    unittest.main()
