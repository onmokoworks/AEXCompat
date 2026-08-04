import os
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BROKER_ROOT = ROOT / "broker"
BROKER_SOURCE_ROOT = BROKER_ROOT / "crates"
MINIHOST_ROOT = ROOT / "minihost"


class NativeCodeGuardTests(unittest.TestCase):




    def test_cargo_execution_policy_is_explicit(self):
        value = os.environ.get("AEXCOMPAT_HAS_CARGO")
        if value is not None:
            self.assertIn(value, {"0", "1"})





if __name__ == "__main__":
    unittest.main()
