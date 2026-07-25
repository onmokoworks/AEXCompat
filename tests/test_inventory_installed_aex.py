import importlib.util
import struct
import sys
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]


def load_tool():
    path = ROOT / "tools" / "inventory_installed_aex.py"
    spec = importlib.util.spec_from_file_location("inventory_installed_aex", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


inventory = load_tool()


def minimal_pe_with_export(optional_magic: int) -> bytes:
    is_pe32_plus = optional_magic == 0x20B
    optional_size = 0xF0 if is_pe32_plus else 0xE0
    data_directory_offset = 112 if is_pe32_plus else 96
    machine = 0x8664 if is_pe32_plus else 0x14C

    data = bytearray(0x600)
    data[0:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    coff = 0x84
    struct.pack_into("<HHIIIHH", data, coff, machine, 1, 0, 0, 0, optional_size, 0)
    optional = coff + 20
    struct.pack_into("<H", data, optional, optional_magic)
    struct.pack_into("<II", data, optional + data_directory_offset, 0x1000, 0x80)

    section = optional + optional_size
    data[section:section + 8] = b".rdata\0\0"
    struct.pack_into("<IIII", data, section + 8, 0x200, 0x1000, 0x200, 0x200)

    export = 0x200
    struct.pack_into("<I", data, export + 24, 1)
    struct.pack_into("<I", data, export + 32, 0x1050)
    struct.pack_into("<I", data, 0x250, 0x1060)
    data[0x260:0x26B] = b"EffectMain\0"
    return bytes(data)


@pytest.mark.parametrize(
    ("optional_magic", "optional_header", "architecture"),
    ((0x20B, "PE32+", "x64"), (0x10B, "PE32", "x86")),
)
def test_export_directory_offset_matches_optional_header_format(
    optional_magic: int,
    optional_header: str,
    architecture: str,
):
    result = inventory.pe_static_info(minimal_pe_with_export(optional_magic))

    assert result["valid_pe"] is True
    assert result["optional_header"] == optional_header
    assert result["architecture"] == architecture
    assert result["export_names"] == ["EffectMain"]
    assert result["export_name_count"] == 1
    assert result["entrypoint_symbol_hints"] == ["EffectMain"]
