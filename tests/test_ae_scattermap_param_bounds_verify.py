import unittest

from tools.ae_scattermap_param_bounds_verify import EXPECTED, verify


def valid_report():
    attempts = []
    for name, attempted, unchanged, minimum, maximum in EXPECTED:
        attempts.append({
            "property": name,
            "attempted": attempted,
            "before": unchanged,
            "after": unchanged,
            "error": f"Value {attempted} out of range {minimum} to {maximum}.",
        })
    return {"schema_version": 1, "app_version": "25.2x131", "attempts": attempts, "error": ""}


class AeScatterMapParamBoundsVerifyTests(unittest.TestCase):
    def test_accepts_exact_rejection_matrix(self):
        result = verify(valid_report())
        self.assertTrue(result["passed"])
        self.assertEqual(result["rejected_count"], 10)

    def test_rejects_clamping_or_mutation(self):
        report = valid_report()
        report["attempts"][0]["after"] = 0
        result = verify(report)
        self.assertFalse(result["passed"])
        self.assertIn("attempt 0 changed the property value", result["failures"])

    def test_rejects_missing_error_or_wrong_range(self):
        report = valid_report()
        report["attempts"][2]["error"] = ""
        report["attempts"][4]["error"] = "Value out of range 0 to 9999."
        result = verify(report)
        self.assertFalse(result["passed"])
        self.assertIn("attempt 2 did not report out of range", result["failures"])
        self.assertIn("attempt 4 reported the wrong valid range", result["failures"])


if __name__ == "__main__":
    unittest.main()
