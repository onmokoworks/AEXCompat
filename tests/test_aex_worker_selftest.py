import importlib.util
import json
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock


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
    def test_main_removes_owned_ppm_when_report_publish_fails(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            worker_root = root / "worker"
            worker_root.mkdir()
            output_ppm = worker_root / "owned.ppm"
            report_path = worker_root / "report.json"
            report_path.write_text("existing report", encoding="utf-8")

            def fake_run_selftest(**kwargs):
                self.assertEqual(kwargs["output_ppm"], output_ppm)
                output_ppm.write_bytes(b"created by this run")
                return {"output_ppm": str(output_ppm)}

            with (
                mock.patch.object(aex_worker_selftest, "WORKER_SELFTEST_ROOT", worker_root),
                mock.patch.object(aex_worker_selftest, "run_selftest", side_effect=fake_run_selftest),
                mock.patch.object(
                    aex_worker_selftest.sys,
                    "argv",
                    [
                        "aex_worker_selftest.py", "--worker", str(LAB_ROOT / "tools" / "aex_no_load_worker.py"),
                        "--output-ppm", str(output_ppm), "--out", str(report_path),
                    ],
                ),
            ):
                with self.assertRaises(FileExistsError):
                    aex_worker_selftest.main()

            self.assertFalse(output_ppm.exists())
            self.assertEqual(report_path.read_text(encoding="utf-8"), "existing report")

    def test_main_never_removes_unowned_reported_ppm(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            worker_root = root / "worker"
            worker_root.mkdir()
            requested_output = worker_root / "requested.ppm"
            unowned_output = worker_root / "unowned.ppm"
            unowned_output.write_bytes(b"pre-existing artifact")
            report_path = worker_root / "report.json"

            with (
                mock.patch.object(aex_worker_selftest, "WORKER_SELFTEST_ROOT", worker_root),
                mock.patch.object(
                    aex_worker_selftest,
                    "run_selftest",
                    return_value={"output_ppm": str(unowned_output)},
                ),
                mock.patch.object(
                    aex_worker_selftest.sys,
                    "argv",
                    [
                        "aex_worker_selftest.py", "--worker", str(LAB_ROOT / "tools" / "aex_no_load_worker.py"),
                        "--output-ppm", str(requested_output), "--out", str(report_path),
                    ],
                ),
            ):
                with self.assertRaisesRegex(AssertionError, "does not match"):
                    aex_worker_selftest.main()

            self.assertTrue(unowned_output.exists())
            self.assertFalse(requested_output.exists())
            self.assertFalse(report_path.exists())

    def test_main_keeps_owned_ppm_and_report_after_successful_publish(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            worker_root = root / "worker"
            worker_root.mkdir()
            output_ppm = worker_root / "owned.ppm"
            report_path = worker_root / "report.json"

            def fake_run_selftest(**kwargs):
                self.assertEqual(kwargs["output_ppm"], output_ppm)
                output_ppm.write_bytes(b"created by this run")
                return {"output_ppm": str(output_ppm)}

            with (
                mock.patch.object(aex_worker_selftest, "WORKER_SELFTEST_ROOT", worker_root),
                mock.patch.object(aex_worker_selftest, "run_selftest", side_effect=fake_run_selftest),
                mock.patch.object(
                    aex_worker_selftest.sys,
                    "argv",
                    [
                        "aex_worker_selftest.py", "--worker", str(LAB_ROOT / "tools" / "aex_no_load_worker.py"),
                        "--output-ppm", str(output_ppm), "--out", str(report_path),
                    ],
                ),
            ):
                self.assertEqual(aex_worker_selftest.main(), 0)

            self.assertTrue(output_ppm.exists())
            self.assertEqual(json.loads(report_path.read_text(encoding="utf-8"))["output_ppm"], str(output_ppm))

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
