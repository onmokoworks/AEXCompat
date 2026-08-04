import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CORE = ROOT / "broker/crates/broker/src/host_core"
FIXTURES = ROOT / "broker/crates/broker/src/fixture_profiles"


class HostCoreBoundaryTests(unittest.TestCase):



    def test_design_does_not_claim_one_fixture_proves_general_support(self):
        text = (ROOT / "docs/HOST_CORE_BOUNDARY_2026-07-13.md").read_text(encoding="utf-8")
        for marker in ("not the product architecture", "second owner-authored AEX",
                       "cannot be called general solely because ScatterMap passes"):
            self.assertIn(marker, text)


if __name__ == "__main__":
    unittest.main()
