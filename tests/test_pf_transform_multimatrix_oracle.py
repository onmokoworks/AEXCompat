import subprocess
import struct
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools/build-pf-transform-multimatrix-oracle.ps1"
SOURCE = ROOT / "instruments/pf-transform-multimatrix-oracle/oracle.cpp"
PROBE = ROOT / "target/pf-transform-multimatrix-oracle-build/Release/pf_transform_multimatrix_oracle.aex"
RUNNER = ROOT / "tools/ae-transform-multimatrix-oracle-run.jsx"
RESOURCE = ROOT / "instruments/pf-transform-multimatrix-oracle/oracle.rc"


def _pe_sections(data):
    pe_offset = struct.unpack_from("<I", data, 0x3C)[0]
    assert data[pe_offset : pe_offset + 4] == b"PE\0\0"
    section_count = struct.unpack_from("<H", data, pe_offset + 6)[0]
    optional_size = struct.unpack_from("<H", data, pe_offset + 20)[0]
    optional_offset = pe_offset + 24
    section_offset = optional_offset + optional_size
    sections = []
    for index in range(section_count):
        offset = section_offset + index * 40
        virtual_size, virtual_address, raw_size, raw_offset = struct.unpack_from(
            "<IIII", data, offset + 8
        )
        sections.append((virtual_address, max(virtual_size, raw_size), raw_offset))
    return optional_offset, sections


def _rva_offset(rva, sections):
    for virtual_address, size, raw_offset in sections:
        if virtual_address <= rva < virtual_address + size:
            return raw_offset + rva - virtual_address
    raise AssertionError(f"RVA 0x{rva:x} is not mapped")


def _directory_entry(data, resource_offset, base_offset, entry_id):
    named, ids = struct.unpack_from("<HH", data, base_offset + 12)
    for index in range(named + ids):
        name, target = struct.unpack_from("<II", data, base_offset + 16 + index * 8)
        if name & 0x80000000:
            string_offset = resource_offset + (name & 0x7FFFFFFF)
            length = struct.unpack_from("<H", data, string_offset)[0]
            actual = data[string_offset + 2 : string_offset + 2 + length * 2].decode("utf-16le")
        else:
            actual = name
        if actual == entry_id:
            return target
    raise AssertionError(f"resource entry {entry_id} is missing")


def _pipl_payload_and_exports(path):
    data = path.read_bytes()
    optional_offset, sections = _pe_sections(data)
    assert struct.unpack_from("<H", data, optional_offset)[0] == 0x20B
    export_rva = struct.unpack_from("<I", data, optional_offset + 112)[0]
    resource_rva = struct.unpack_from("<I", data, optional_offset + 128)[0]

    export_offset = _rva_offset(export_rva, sections)
    name_count = struct.unpack_from("<I", data, export_offset + 24)[0]
    names_rva = struct.unpack_from("<I", data, export_offset + 32)[0]
    names_offset = _rva_offset(names_rva, sections)
    exports = set()
    for index in range(name_count):
        name_rva = struct.unpack_from("<I", data, names_offset + index * 4)[0]
        name_offset = _rva_offset(name_rva, sections)
        exports.add(data[name_offset : data.index(b"\0", name_offset)].decode("ascii"))

    resource_offset = _rva_offset(resource_rva, sections)
    type_target = _directory_entry(data, resource_offset, resource_offset, "PIPL")
    assert type_target & 0x80000000
    name_offset = resource_offset + (type_target & 0x7FFFFFFF)
    name_target = _directory_entry(data, resource_offset, name_offset, 16000)
    assert name_target & 0x80000000
    language_offset = resource_offset + (name_target & 0x7FFFFFFF)
    _, language_ids = struct.unpack_from("<HH", data, language_offset + 12)
    assert language_ids == 1
    data_target = struct.unpack_from("<I", data, language_offset + 20)[0]
    assert not data_target & 0x80000000
    data_entry = resource_offset + data_target
    payload_rva, payload_size = struct.unpack_from("<II", data, data_entry)
    payload_offset = _rva_offset(payload_rva, sections)
    return data[payload_offset : payload_offset + payload_size], exports


def _parse_pipl(payload):
    version, reserved, property_count = struct.unpack_from("<IHI", payload)
    offset = 10
    properties = []
    for _ in range(property_count):
        assert (offset - 10) % 4 == 0
        vendor, key, property_id, length = struct.unpack_from("<4s4sII", payload, offset)
        value_offset = offset + 16
        end = value_offset + length
        assert end <= len(payload)
        properties.append((vendor, key, property_id, length, payload[value_offset:end]))
        offset = 10 + ((end - 10 + 3) & ~3)
    return version, reserved, properties, offset


def test_multimatrix_oracle_probe_builds_with_two_motion_samples():
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
        cwd=ROOT,
        check=True,
        timeout=180,
    )
    source = SOURCE.read_text(encoding="utf-8")
    assert "std::array<PF_FloatMatrix, 2>" in source
    assert "matrices.data(), 2, TRUE" in source


def test_oracle_build_is_reproducible_for_hash_pinned_bundle():
    first = PROBE.read_bytes()
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
        cwd=ROOT,
        check=True,
        timeout=180,
    )
    assert PROBE.read_bytes() == first


def test_oracle_has_ae_load_and_render_safety_guards():
    source = SOURCE.read_text(encoding="utf-8")
    assert "PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_DEEP_COLOR_AWARE" in source
    assert "if (!out) return PF_Err_BAD_CALLBACK_PARAM;" in source
    assert "if (!err && !transforms) err = PF_Err_BAD_CALLBACK_PARAM;" in source
    assert "if (!err && !worlds) err = PF_Err_BAD_CALLBACK_PARAM;" in source
    assert "world.rowbytes < 0 || world.height < 0" in source
    assert "rowbytes > std::numeric_limits<size_t>::max() / height" in source
    assert "std::memset(output->data, 0, output_bytes);" in source


def test_generated_oracle_has_well_formed_pipl_and_effect_main_export():
    payload, exports = _pipl_payload_and_exports(PROBE)
    version, reserved, properties, final_offset = _parse_pipl(payload)

    assert "EffectMain" in exports
    assert len(payload) == 334
    assert (version, reserved, len(properties)) == (1, 0, 12)
    assert final_offset == len(payload)
    assert all(length == len(value) for _, _, _, length, value in properties)
    assert all(length % 2 == 0 for _, _, _, length, _ in properties)
    assert [key for _, key, _, _, _ in properties].count(b"eman") == 1
    assert [key for _, key, _, _, _ in properties].count(b"4668") == 1
    assert b"PF Transform Multi Matrix Oracle" in payload
    assert b"PF Transform Affine Probe" not in payload


def test_ae_runner_uses_capture_contract_for_registered_effect_name():
    runner = RUNNER.read_text(encoding="utf-8")
    assert 'env("AEXCOMPAT_AE_EFFECT")' in runner
    assert 'canAddProperty(requestedName)' in runner
    assert 'saveFrameToPng(1.0 / 30.0' in runner
