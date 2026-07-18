import importlib.util
import json
import sys
import tempfile
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


aepx_static_probe = load_tool("aepx_static_probe")


def write_synthetic_aepx() -> Path:
    root = LAB_ROOT / "target" / "test-inputs"
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{time.time_ns()}-synthetic.aepx"
    path.write_text(
        """<?xml version="1.0" encoding="UTF-8"?>
<AfterEffectsProject xmlns="http://www.adobe.com/products/aftereffects" majorVersion="1" minorVersion="0">
  <head bdata="00010203"/>
  <Folder>
    <string>Do not export this literal text</string>
    <Item bdata="0a0b"/>
  </Folder>
</AfterEffectsProject>
""",
        encoding="utf-8",
    )
    return path


class AepxStaticProbeTests(unittest.TestCase):
    def test_probe_parses_aepx_without_exporting_text_payloads(self):
        report = aepx_static_probe.build_aepx_probe(write_synthetic_aepx())
        self.assertEqual(report["report_kind"], "aepx_static_probe")
        self.assertEqual(report["probe_state"], "aepx_static_probe_ready_no_write")
        self.assertEqual(report["xml_parse_state"], "parsed")
        self.assertEqual(report["root"]["tag"], "AfterEffectsProject")
        self.assertEqual(report["root"]["namespace"], "http://www.adobe.com/products/aftereffects")
        self.assertEqual(report["summary"]["element_count"], 5)
        self.assertEqual(report["summary"]["bdata_attribute_count"], 2)
        self.assertEqual(report["summary"]["bdata_total_decoded_bytes_if_hex"], 6)
        self.assertFalse(report["aepx_file_modified"])
        self.assertFalse(report["ae_invoked"])
        self.assertFalse(report["aex_file_opened"])
        serialized = json.dumps(report)
        self.assertNotIn("Do not export this literal text", serialized)

    def test_invalid_suffix_or_xml_is_rejected(self):
        bad_suffix = LAB_ROOT / "target" / "test-inputs" / f"{time.time_ns()}-bad.txt"
        bad_suffix.parent.mkdir(parents=True, exist_ok=True)
        bad_suffix.write_text("<x/>", encoding="utf-8")
        with self.assertRaises(ValueError):
            aepx_static_probe.build_aepx_probe(bad_suffix)

        bad_xml = LAB_ROOT / "target" / "test-inputs" / f"{time.time_ns()}-bad.aepx"
        bad_xml.write_text("<AfterEffectsProject>", encoding="utf-8")
        with self.assertRaises(ValueError):
            aepx_static_probe.build_aepx_probe(bad_xml)

    def test_paths_are_confined_and_output_is_create_new(self):
        source = write_synthetic_aepx()
        resolved = aepx_static_probe.validate_aepx_input_path(source)
        self.assertEqual(resolved, source.resolve())

        outside_root = next(
            root for root in (Path(tempfile.gettempdir()), Path.home())
            if not root.resolve().is_relative_to(LAB_ROOT.parent.resolve())
        )
        with tempfile.TemporaryDirectory(
            prefix="aexcompat-outside-", dir=outside_root
        ) as directory:
            outside = Path(directory) / f"{time.time_ns()}-outside.aepx"
            outside.write_text("<x/>", encoding="utf-8")
            with self.assertRaises(ValueError):
                aepx_static_probe.validate_aepx_input_path(outside)

        payload = aepx_static_probe.build_aepx_probe(source)
        out = LAB_ROOT / "target" / "aepx-static-probe" / f"{time.time_ns()}-aepx.local.json"
        written = aepx_static_probe.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aepx_static_probe.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aepx_static_probe.write_json_create_new(LAB_ROOT / "target" / "outside-aepx.json", payload)


if __name__ == "__main__":
    unittest.main()
