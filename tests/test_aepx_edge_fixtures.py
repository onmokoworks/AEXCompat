import xml.etree.ElementTree as ET
import unittest
from pathlib import Path

from tools import aepx_roundtrip_validator, aepx_static_probe


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_ROOT = ROOT / "tests" / "fixtures" / "aepx"
SOURCE_ROOT = ROOT / "imports" / "aviutlas-rust-contracts" / "aviutl-rs" / "tests" / "fixtures"

EXPECTED = {
    "aepx_preservation_sentinels.aepx": (4, 4, 2),
    "aepx_writer_spike_ambiguous.aepx": (3, 2, 1),
    "aepx_writer_spike_crlf_bom.aepx": (5, 5, 3),
    "aepx_writer_spike_duplicate_id.aepx": (3, 2, 1),
    "aepx_writer_spike_multi_comp.aepx": (6, 4, 2),
    "aepx_writer_spike_preservation.aepx": (7, 7, 3),
    "aepx_writer_spike_scanner_edges.aepx": (7, 7, 3),
    "aepx_writer_spike_unicode_edges.aepx": (5, 5, 3),
}


class AepxEdgeFixtureTests(unittest.TestCase):
    def test_fixture_set_is_exact_and_byte_identical_to_frozen_sources(self):
        actual = {path.name for path in FIXTURE_ROOT.glob("*.aepx")}
        self.assertEqual(set(EXPECTED), actual)
        for name in sorted(EXPECTED):
            with self.subTest(name=name):
                self.assertEqual((SOURCE_ROOT / name).read_bytes(), (FIXTURE_ROOT / name).read_bytes())

    def test_static_probe_results_are_deterministic(self):
        for name, expected in EXPECTED.items():
            with self.subTest(name=name):
                first = aepx_static_probe.build_aepx_probe(FIXTURE_ROOT / name)
                second = aepx_static_probe.build_aepx_probe(FIXTURE_ROOT / name)
                summary = first["summary"]
                self.assertEqual("parsed", first["xml_parse_state"])
                self.assertEqual("project", first["root"]["tag"])
                self.assertEqual(expected, (summary["element_count"], summary["unique_tag_count"], summary["max_depth"]))
                self.assertEqual(first["root"], second["root"])
                self.assertEqual(first["summary"], second["summary"])
                self.assertEqual(first["top_tags"], second["top_tags"])

    def test_every_parseable_fixture_round_trips_in_memory(self):
        for name in EXPECTED:
            with self.subTest(name=name):
                source_root = ET.parse(FIXTURE_ROOT / name).getroot()
                source = aepx_roundtrip_validator.comparable_signature(
                    aepx_roundtrip_validator.structure_signature(source_root)
                )
                serialized = ET.tostring(source_root, encoding="utf-8")
                reparsed = ET.fromstring(serialized)
                roundtrip = aepx_roundtrip_validator.comparable_signature(
                    aepx_roundtrip_validator.structure_signature(reparsed)
                )
                self.assertTrue(aepx_roundtrip_validator.compare_signatures(source, roundtrip)["match"])

if __name__ == "__main__":
    unittest.main()
