import os
import re
import subprocess
from pathlib import Path
import source_owners

ROOT = Path(__file__).resolve().parents[1]
SOURCES = source_owners.contract_files("pf_color_suite")
def source_text():
    return "\n".join(path.read_text(encoding="utf-8") for path in SOURCES)

def worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [Path(configured) if configured else None,
                  ROOT / "target/minihost-build-v18/Release/aex_render_worker.exe",
                  ROOT / "target/minihost-build-v18/aex_render_worker.exe"]
    return next((path for path in candidates if path and path.is_file()), None)

def test_color_suites_are_typed_frozen_v1_abis():
    text = source_text()
    for suite in ("PfColorCallbacks8", "PfColorCallbacks16", "PfColorCallbacksFloat"):
        assert f"sizeof({suite}) == 8 * sizeof(void*)" in text
    for slot, member in enumerate(("RGBtoHLS", "HLStoRGB", "RGBtoYIQ", "YIQtoRGB",
                                   "Luminance", "Hue", "Lightness", "Saturation")):
        assert f"PF_COLOR_OFFSET_ASSERT(PfColorCallbacks8, {member}, {slot})" in text
    for name, instance in (("PF Color Suite", "g_color_suite8"),
                           ("PF Color16 Suite", "g_color_suite16"),
                           ("PF ColorFloat Suite", "g_color_suite_float")):
        assert f'{{"{name}", 1, &{instance}}}' in text

def test_legacy_block_ends_where_platform_data_begins():
    text = source_text()
    assert "constexpr std::size_t kUtilsColorCallbacks = 368" in text
    assert "constexpr std::size_t kUtilsGetPlatformData = 432" in text
    assert "kUtilsColorCallbacks + sizeof(PfColorCallbacks8) == kUtilsGetPlatformData" in text
    assert re.search(r"memcpy\(utils\.data\(\) \+ kUtilsColorCallbacks, &g_color_suite8,\s*"
                     r"sizeof\(g_color_suite8\)\)", text)
    assert "write(utils, kUtilsGetPlatformData, &get_platform_data)" in text

def test_color_contract_is_fail_closed_hdr_capable_and_alpha_preserving():
    text = source_text()
    assert "if (!finite3(c.r, c.g, c.b)) return kPfErrBadCallbackParam" in text
    assert "PfFixed result[3]" in text and "std::memcpy(out, result, sizeof(result))" in text
    traits = text[text.index("template <> struct ColorPixelTraits<PfPixelFloat>"):
                  text.index("template <class Pixel> int32_t __cdecl color_rgb_to_hls")]
    assert "std::min" not in traits and "std::max" not in traits
    assert "p.red = static_cast<float>(c.r)" in traits
    assert "round8.alpha == 91" in text and "round16.alpha == 4321" in text
    assert "yiq[0] > 65536" in text
    assert "color_to_fixed(hls.h * 360.0)" in text
    assert "color_from_fixed(in[0]) / 360.0" in text
    assert "Which == 1 ? 255.0" in text
    assert "hue16 == 85" in text
    assert "unobserved AE tie behavior is not asserted" in text

def test_pf_color_suite_native_self_test():
    executable = worker()
    assert executable is not None, "build aex_render_worker before running the focused runtime test"
    completed = subprocess.run([str(executable), "--self-test-pf-color-suite"], cwd=ROOT,
                               text=True, capture_output=True, timeout=30, check=False)
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert completed.stdout.strip() == '{"pf_color_suite":"passed"}'
