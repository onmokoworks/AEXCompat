import json
import math
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from tools.aex_parameter_value_gate import validate_assignments


ROOT = Path(__file__).resolve().parents[1]
TOOL = ROOT / "tools" / "aex_parameter_value_gate.py"
PARAMETERS = [
    {"name": "Scatter Amount", "type": 1, "valid_min": 0, "valid_max": 500},
    {"name": "Direction", "type": 7, "valid_min": 1, "valid_max": 3},
    {"name": "Random Seed", "type": 1, "valid_min": 0, "valid_max": 10000},
    {"name": "Mix with Original", "type": 10, "valid_min": 0, "valid_max": 100},
    {"name": "Invert Map", "type": 4, "valid_min": 0, "valid_max": 1},
]


class ParameterValueGateTests(unittest.TestCase):
    def test_accepts_observed_valid_boundaries(self):
        report = validate_assignments(PARAMETERS, {
            "Scatter Amount": 500,
            "Direction": 1,
            "Random Seed": 10000,
            "Mix with Original": 0.0,
            "Invert Map": 1,
        })
        self.assertTrue(report["accepted"])
        self.assertTrue(report["native_dispatch_permitted"])

    def test_rejects_the_ten_production_ae_out_of_range_cases(self):
        cases = {
            "Scatter Amount": (-1, 501),
            "Direction": (0, 4),
            "Random Seed": (-1, 10001),
            "Mix with Original": (-0.1, 100.1),
            "Invert Map": (-1, 2),
        }
        for name, values in cases.items():
            for value in values:
                with self.subTest(name=name, value=value):
                    report = validate_assignments(PARAMETERS, {name: value})
                    self.assertFalse(report["accepted"])
                    self.assertFalse(report["native_dispatch_permitted"])
                    self.assertEqual(report["errors"][0]["code"], "parameter_out_of_range")

    def test_fails_closed_for_ambiguous_values_and_schema(self):
        for value in (True, "5", math.nan, math.inf, 1.5):
            with self.subTest(value=value):
                report = validate_assignments(PARAMETERS, {"Scatter Amount": value})
                self.assertFalse(report["native_dispatch_permitted"])
        self.assertFalse(validate_assignments(PARAMETERS, {"missing": 1})["accepted"])
        self.assertFalse(validate_assignments({}, {})["accepted"])

    def test_cli_is_create_new_and_uses_rejection_exit_code(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            request = root / "request.json"
            output = root / "report.json"
            request.write_text(json.dumps({
                "parameters": PARAMETERS,
                "assignments": {"Direction": 4},
            }), encoding="utf-8")
            first = subprocess.run(
                [sys.executable, str(TOOL), str(request), str(output)],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(first.returncode, 3)
            self.assertFalse(json.loads(output.read_text(encoding="utf-8"))["native_dispatch_permitted"])
            second = subprocess.run(
                [sys.executable, str(TOOL), str(request), str(output)],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(second.returncode, 4)
            self.assertEqual(second.stderr.strip(), "output already exists")


if __name__ == "__main__":
    unittest.main()
