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


aex_worker_selftest = load_tool("aex_worker_selftest")


def make_design_packet() -> Path:
    root = LAB_ROOT / "target" / "worker-design"
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{time.time_ns()}-selftest-design.local.json"
    payload = {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_worker_sandbox_design_packet",
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
    }
    path.write_text(json.dumps(payload), encoding="utf-8")
    return path


class AexWorkerSelftestTests(unittest.TestCase):
    def test_run_selftest_spawns_worker_and_reports_no_load(self):
        output_ppm = LAB_ROOT / "target" / "worker-selftest" / f"{time.time_ns()}-identity.ppm"
        report = aex_worker_selftest.run_selftest(
            worker_path=LAB_ROOT / "tools" / "aex_no_load_worker.py",
            input_ppm=None,
            output_ppm=output_ppm,
            design_packet_path=make_design_packet(),
        )
        self.assertEqual(report["report_kind"], "aex_no_load_worker_selftest")
        self.assertTrue(report["worker_selftest_passed"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["render_performed"])
        self.assertFalse(report["ae_invoked"])
        self.assertFalse(report["ofx_route_invoked"])
        self.assertFalse(report["private_payload_copied"])
        self.assertFalse(report["aex_file_opened"])
        self.assertTrue(Path(report["input_ppm"]).exists())
        self.assertTrue(Path(report["output_ppm"]).exists())
        self.assertEqual([step["step"] for step in report["steps"]], [
            "hello",
            "inspect_environment",
            "inspect_ppm",
            "transform_ppm_identity",
            "blocked_load_aex",
            "quit",
        ])

    def test_rejects_unsafe_design_packet_and_create_new_report_output(self):
        unsafe = make_design_packet()
        payload = json.loads(unsafe.read_text(encoding="utf-8"))
        payload["native_load_performed"] = True
        bad = LAB_ROOT / "target" / "worker-design" / f"{time.time_ns()}-unsafe.local.json"
        bad.write_text(json.dumps(payload), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_worker_selftest.load_design_packet(bad)

        report = {
            "schema_version": 1,
            "report_kind": "aex_no_load_worker_selftest",
            "native_load_performed": False,
        }
        out = LAB_ROOT / "target" / "worker-selftest" / f"{time.time_ns()}-report.local.json"
        written = aex_worker_selftest.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_worker_selftest.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_worker_selftest.write_json_create_new(LAB_ROOT / "target" / "outside-selftest.json", report)


if __name__ == "__main__":
    unittest.main()
