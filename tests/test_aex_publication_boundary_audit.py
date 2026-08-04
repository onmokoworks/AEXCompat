import importlib.util
import json
import os
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


aex_publication_boundary_audit = load_tool("aex_publication_boundary_audit")


def make_safety_audit() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_no_load_safety_chain_audit",
        "audit_passed": True,
        "audit_state": "no_load_chain_verified",
        "artifact_count": 2,
        "artifacts": [
            {"label": "static_report", "path": "D:\\local\\static.json"},
            {"label": "ofx_noop_mock", "path": "D:\\local\\ofx.json"},
        ],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


class AexPublicationBoundaryAuditTests(unittest.TestCase):
    def test_clean_safety_audit_is_still_not_publishable(self):
        report = aex_publication_boundary_audit.build_publication_report(make_safety_audit(), Path("audit.json"))
        self.assertEqual(report["report_kind"], "aex_publication_boundary_audit")
        self.assertEqual(report["boundary_state"], "local_only_not_publishable")
        self.assertFalse(report["publishable_now"])
        self.assertTrue(report["redaction_required"])
        self.assertEqual(report["local_path_reference_count"], 2)
        self.assertIn("absolute local paths", report["redacted_local_summary"]["redacted_fields"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["ofx_route_invoked"])

    def test_invalid_safety_audit_blocks_publication(self):
        audit = make_safety_audit()
        audit["audit_passed"] = False
        audit["native_load_performed"] = True
        report = aex_publication_boundary_audit.build_publication_report(audit, Path("audit.json"))
        self.assertEqual(report["boundary_state"], "invalid_evidence_not_publishable")
        self.assertFalse(report["publishable_now"])
        self.assertIn("safety audit must have audit_passed=true", report["evidence_errors"])
        self.assertIn("safety audit native_load_performed must be false", report["evidence_errors"])

    def test_paths_are_confined_and_output_is_create_new(self):
        audit_root = LAB_ROOT / "target" / "safety-audit"
        audit_root.mkdir(parents=True, exist_ok=True)
        source = audit_root / f"{time.time_ns()}-{os.getpid()}-publication-source.json"
        source.write_text(json.dumps(make_safety_audit()), encoding="utf-8")
        loaded, resolved = aex_publication_boundary_audit.load_safety_audit(source)
        self.assertEqual(loaded["report_kind"], "aex_no_load_safety_chain_audit")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-publication.json"
        outside.write_text(json.dumps(make_safety_audit()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_publication_boundary_audit.load_safety_audit(outside)

        payload = aex_publication_boundary_audit.build_publication_report(loaded, resolved)
        out = LAB_ROOT / "target" / "publication-boundary" / f"{time.time_ns()}-{os.getpid()}-publication.local.json"
        written = aex_publication_boundary_audit.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_publication_boundary_audit.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_publication_boundary_audit.write_json_create_new(LAB_ROOT / "target" / "outside-publication.json", payload)


if __name__ == "__main__":
    unittest.main()
