import ctypes
import json
import os
import struct
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
MEMBERS = [
    "get_blending_tables", "does_view_have_xform", "xform_working_to_view",
    "get_new_working_space_profile", "get_new_profile_from_icc",
    "get_new_icc_from_profile", "get_new_profile_description", "dispose_profile",
    "get_profile_approximate_gamma", "is_rgb_profile", "set_working_color_space",
    "is_ocio_used", "get_ocio_configuration_file", "get_ocio_configuration_file_path",
    "get_ocio_working_colorspace", "get_ocio_display_colorspace",
    "is_colorspace_aware_effects_enabled", "get_lut_interpolation_method",
    "get_graphics_white_luminance", "get_working_colorspace_id",
]


def source_text():
    return SOURCE.read_text(encoding="utf-8")


def worker(name):
    configured = os.environ.get(f"AEXCOMPAT_{name.upper()}_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-timed-layers" / "Release" / f"aex_{name}_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "Release" / f"aex_{name}_worker.exe",
        ROOT / "target" / "minihost-build-v18" / f"aex_{name}_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)


def test_color_settings_suite6_has_exact_typed_20_slot_abi():
    text = source_text()
    assert "struct AegpColorSettingsSuite6" in text
    assert "sizeof(AegpColorSettingsSuite6) == 20 * sizeof(void*)" in text
    for slot, member in enumerate(MEMBERS):
        assert f"offsetof(AegpColorSettingsSuite6, {member}) == {slot} * sizeof(void*)" in text
    assert 'std::strcmp(name, "PF Color Settings Suite") == 0 && version == 7' in text
    assert "*suite = &g_color_settings_suite6" in text
    assert "color_settings_validate_icc" in text
    assert "color_settings_builtin_srgb_icc" in text
    assert "color_settings_builtin_linear_icc" in text
    assert "g_working_color_space_kind" in text
    assert "color_settings_init_empty_utf16_handle" in text


def test_color_settings_headless_policy_is_fail_closed_and_alpha_preserving():
    text = source_text()
    assert "ColorProfileKind::ImportedRgb" in text
    assert "color_settings_linear_to_srgb" in text
    assert "in_place = src == dst" in text
    assert "AEGP_WorldH is an opaque handle token" in text
    assert "std::memcpy(dst_pixel, pixel.data(), sizeof(pixel))" in text
    assert "++g_invalid_color_profile_operations" in text
    assert "kMaxIccProfileBytes" in text
    assert "kWorkingLinearSrgbGuid" in text
    assert "kWorkingSrgbGuid" in text


def test_color_settings_runtime_matrix():
    executable = worker("render")
    assert executable is not None, "build aex_render_worker before running the focused runtime test"
    completed = subprocess.run(
        [str(executable), "--self-test-pf-color-settings-suite6"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    report = json.loads(completed.stdout)
    assert report["pf_color_settings_suite6"] == "passed"
    assert report["ocio_enabled"] is False
    assert report["profiles_created"] == report["profiles_disposed"]
    assert report["xform_calls"] >= 4
    assert report["invalid_operations"] >= 2


def test_color_settings_selftest_available_on_l2_and_smart_workers():
    for name in ("l2", "smart"):
        executable = worker(name)
        assert executable is not None, f"build aex_{name}_worker before running the focused runtime test"
        completed = subprocess.run(
            [str(executable), "--self-test-pf-color-settings-suite6"],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )
        assert completed.returncode == 0, completed.stderr or completed.stdout
        assert json.loads(completed.stdout)["pf_color_settings_suite6"] == "passed"


def test_generated_linear_icc_with_independent_binary_parser():
    executable = worker("render")
    assert executable is not None
    completed = subprocess.run(
        [str(executable), "--self-test-pf-color-settings-suite6"],
        cwd=ROOT, text=True, capture_output=True, timeout=30, check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    profile = bytes.fromhex(json.loads(completed.stdout)["linear_icc_hex"])

    # This parser intentionally shares no code with the host ICC validator.
    be32 = lambda offset: struct.unpack_from(">I", profile, offset)[0]
    s15_fixed16 = lambda offset: struct.unpack_from(">i", profile, offset)[0] / 65536.0
    assert be32(0) == len(profile)
    assert profile[16:20] == b"RGB "
    assert profile[36:40] == b"acsp"
    tag_count = be32(128)
    assert tag_count == 9
    table_end = 132 + tag_count * 12
    tags = {}
    spans = []
    for index in range(tag_count):
        entry = 132 + index * 12
        signature = profile[entry:entry + 4]
        offset, size = struct.unpack_from(">II", profile, entry + 4)
        assert offset >= table_end
        assert size > 0
        assert offset + size <= len(profile)
        tags[signature] = (offset, size)
        spans.append((offset, offset + size, signature))
    for previous, current in zip(sorted(spans), sorted(spans)[1:]):
        assert previous[1] <= current[0], (previous, current)

    expected_xyz = {
        b"wtpt": (0.95047, 1.0, 1.08883),
        b"rXYZ": (0.43607, 0.22249, 0.01392),
        b"gXYZ": (0.38515, 0.71687, 0.09708),
        b"bXYZ": (0.14307, 0.06061, 0.71410),
    }
    for signature, expected in expected_xyz.items():
        offset, size = tags[signature]
        assert size == 20
        assert profile[offset:offset + 4] == b"XYZ "
        actual = tuple(s15_fixed16(offset + 8 + channel * 4) for channel in range(3))
        assert actual == pytest.approx(expected, abs=1 / 65536)
    for signature in (b"rTRC", b"gTRC", b"bTRC"):
        offset, size = tags[signature]
        assert size == 20
        assert profile[offset:offset + 4] == b"curv"
        assert be32(offset + 8) == 1
        assert struct.unpack_from(">H", profile, offset + 12)[0] / 256.0 == 1.0


def test_icc_source_has_no_unaligned_or_double_swap_paths():
    text = source_text()
    section = text[
        text.index("uint32_t color_settings_read_be32"):
        text.index("ColorProfileKind color_settings_classify_icc")
    ]
    assert "reinterpret_cast<const uint32_t*>" not in section
    assert "reinterpret_cast<const float*>" not in section
    assert "color_settings_be16" not in section
    assert "color_settings_be32" not in section
    assert "constexpr std::size_t kTagCount = 9" in section


def test_generated_linear_icc_is_accepted_by_windows_wcs(tmp_path):
    executable = worker("render")
    completed = subprocess.run(
        [str(executable), "--self-test-pf-color-settings-suite6"],
        cwd=ROOT, text=True, capture_output=True, timeout=30, check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    profile_path = tmp_path / "aexcompat-linear-srgb.icc"
    profile_path.write_bytes(bytes.fromhex(json.loads(completed.stdout)["linear_icc_hex"]))

    class Profile(ctypes.Structure):
        _fields_ = [
            ("dwType", ctypes.c_uint32),
            ("pProfileData", ctypes.c_void_p),
            ("cbDataSize", ctypes.c_uint32),
        ]

    path_buffer = ctypes.create_unicode_buffer(str(profile_path))
    descriptor = Profile(
        1, ctypes.cast(path_buffer, ctypes.c_void_p), ctypes.sizeof(path_buffer)
    )
    wcs = ctypes.WinDLL("mscms.dll", use_last_error=True)
    wcs.OpenColorProfileW.argtypes = [
        ctypes.POINTER(Profile), ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint32
    ]
    wcs.OpenColorProfileW.restype = ctypes.c_void_p
    wcs.CloseColorProfile.argtypes = [ctypes.c_void_p]
    handle = wcs.OpenColorProfileW(ctypes.byref(descriptor), 1, 3, 3)
    assert handle, f"Windows WCS rejected generated ICC: {ctypes.get_last_error()}"
    assert wcs.CloseColorProfile(handle)
