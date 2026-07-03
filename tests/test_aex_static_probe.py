import importlib.util
import json
import struct
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


aex_static_probe = load_tool("aex_static_probe")


def minimal_pe64() -> bytes:
    data = bytearray(0x800)
    data[0:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    coff = 0x84
    struct.pack_into("<H", data, coff, 0x8664)
    struct.pack_into("<H", data, coff + 2, 1)
    struct.pack_into("<I", data, coff + 4, 0x12345678)
    struct.pack_into("<H", data, coff + 16, 0xF0)
    struct.pack_into("<H", data, coff + 18, 0x2022)
    optional = coff + 20
    struct.pack_into("<H", data, optional, 0x20B)
    struct.pack_into("<I", data, optional + 16, 0x1000)
    struct.pack_into("<H", data, optional + 68, 2)
    struct.pack_into("<II", data, optional + 112, 0x1100, 0x80)
    struct.pack_into("<II", data, optional + 120, 0x1180, 0x40)
    struct.pack_into("<II", data, optional + 128, 0x1300, 0x100)
    section = optional + 0xF0
    data[section : section + 8] = b".text\0\0\0"
    struct.pack_into("<I", data, section + 8, 0x600)
    struct.pack_into("<I", data, section + 12, 0x1000)
    struct.pack_into("<I", data, section + 16, 0x600)
    struct.pack_into("<I", data, section + 20, 0x200)
    data[0x220:0x240] = b"PiPL EffectMain PF_Cmd AE_Effect"
    export = 0x300
    struct.pack_into("<I", data, export + 12, 0x1130)
    struct.pack_into("<I", data, export + 16, 1)
    struct.pack_into("<I", data, export + 20, 1)
    struct.pack_into("<I", data, export + 24, 1)
    struct.pack_into("<I", data, export + 28, 0x1140)
    struct.pack_into("<I", data, export + 32, 0x1148)
    struct.pack_into("<I", data, export + 36, 0x1150)
    data[0x330:0x33E] = b"Synthetic.aex\0"
    struct.pack_into("<I", data, 0x340, 0x1200)
    struct.pack_into("<I", data, 0x348, 0x1160)
    struct.pack_into("<H", data, 0x350, 0)
    data[0x360:0x36B] = b"EffectMain\0"
    import_descriptor = 0x380
    struct.pack_into("<I", data, import_descriptor + 12, 0x11C0)
    data[0x3C0:0x3CD] = b"KERNEL32.dll\0"
    resource = 0x500
    struct.pack_into("<HH", data, resource + 12, 1, 0)
    struct.pack_into("<II", data, resource + 16, 0x80000080, 0x80000028)
    type_dir = resource + 0x28
    struct.pack_into("<HH", data, type_dir + 12, 0, 1)
    struct.pack_into("<II", data, type_dir + 16, 16000, 0x80000050)
    lang_dir = resource + 0x50
    struct.pack_into("<HH", data, lang_dir + 12, 0, 1)
    struct.pack_into("<II", data, lang_dir + 16, 1033, 0x70)
    struct.pack_into("<IIII", data, resource + 0x70, 0x13A0, 12, 1200, 0)
    struct.pack_into("<H", data, resource + 0x80, 4)
    data[resource + 0x82 : resource + 0x8A] = "PIPL".encode("utf-16le")
    data[0x5A0:0x5AC] = b"PIPLmetadata"
    return bytes(data)


class AexStaticProbeTests(unittest.TestCase):
    def test_parse_minimal_pe_without_loading(self):
        report = aex_static_probe.parse_pe(minimal_pe64())
        self.assertTrue(report["mz_header_present"])
        self.assertTrue(report["pe_valid"])
        self.assertEqual(report["machine_label"], "x64")
        self.assertEqual(report["subsystem_label"], "windows_gui")
        self.assertEqual(report["section_names"], [".text"])
        self.assertTrue(report["characteristics_flags"]["dll"])
        self.assertTrue(report["export_summary"]["effect_main_export_present"])
        self.assertEqual(report["export_summary"]["exported_names"], ["EffectMain"])
        self.assertEqual(report["import_summary"]["dll_names"], ["KERNEL32.dll"])
        resource = report["resource_summary"]
        self.assertTrue(resource["pipl_resource_type_present"])
        self.assertEqual(resource["pipl_resource_data_entry_count"], 1)
        self.assertEqual(resource["pipl_resource_total_size"], 12)
        self.assertEqual(
            resource["pipl_resource_entries"][0],
            {
                "type": "PIPL",
                "name": 16000,
                "language": 1033,
                "data_rva": 0x13A0,
                "size_bytes": 12,
                "codepage": 1200,
                "reserved": 0,
            },
        )

    def test_analyze_file_reports_markers_and_no_runtime_flags(self):
        target = LAB_ROOT / "target" / "test-inputs"
        target.mkdir(parents=True, exist_ok=True)
        path = target / f"{time.time_ns()}-synthetic.aex"
        path.write_bytes(minimal_pe64())
        entry = aex_static_probe.analyze_aex_file(path, root=target)
        self.assertFalse(entry["native_load_performed"])
        self.assertFalse(entry["render_performed"])
        self.assertFalse(entry["ae_invoked"])
        self.assertTrue(entry["pipl_signal_present"])
        self.assertTrue(entry["markers"]["effect_main_marker_present"])
        self.assertTrue(entry["pe"]["export_summary"]["effect_main_export_present"])
        self.assertGreaterEqual(entry["markers"]["pf_cmd_marker_count"], 1)
        self.assertEqual(entry["compatibility_class"], "classic_pf_effect_candidate")
        self.assertGreater(entry["fixture_candidate_score"], 0)

    def test_report_summarizes_fixture_candidates(self):
        target = LAB_ROOT / "target" / "test-inputs"
        target.mkdir(parents=True, exist_ok=True)
        path = target / f"{time.time_ns()}-synthetic.aex"
        path.write_bytes(minimal_pe64())
        report = aex_static_probe.build_report(path)
        self.assertEqual(report["schema_version"], 3)
        self.assertEqual(report["summary"]["aex_count"], 1)
        self.assertEqual(report["summary"]["pipl_resource_entry_count"], 1)
        self.assertEqual(report["summary"]["pipl_resource_total_size"], 12)
        self.assertEqual(report["summary"]["effect_main_export_count"], 1)
        self.assertEqual(report["fixture_candidates"][0]["relative_path"], path.name)
        self.assertEqual(report["fixture_candidates"][0]["pipl_resource_data_entry_count"], 1)
        self.assertIn("EffectMain-export", report["fixture_candidates"][0]["fixture_candidate_reasons"])

    def test_report_writer_is_create_new_under_target_root(self):
        path = LAB_ROOT / "target" / "aex-static-probe" / f"{time.time_ns()}-probe.local.json"
        payload = {"schema_version": 1, "ok": True}
        aex_static_probe.write_json_create_new(path, payload)
        with self.assertRaises(FileExistsError):
            aex_static_probe.write_json_create_new(path, payload)
        parsed = json.loads(path.read_text(encoding="utf-8"))
        self.assertTrue(parsed["ok"])
        with self.assertRaises(ValueError):
            aex_static_probe.write_json_create_new(LAB_ROOT / "target" / "outside.json", payload)


if __name__ == "__main__":
    unittest.main()
