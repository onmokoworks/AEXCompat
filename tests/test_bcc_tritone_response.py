"""BCC Tritone core response, not an exact interpolation formula or AE oracle."""

import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


CASES = (
    ("tritone", 1, 0),
    ("duotone", 0, 0),
    ("mix100", 1, 100),
)

COLORS = (
    (8, [255, 255, 0, 0]),
    (10, [255, 0, 255, 0]),
    (11, [255, 0, 0, 255]),
)


def source_pixels():
    return bytes(
        component
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for value in ((x + 37 * y) % 256,)
        for component in (value, value, value, 255)
    )


def tritone_pixel(value):
    if value <= 128:
        green = 255 * value // 128
        return 255 - green, green, 0, 255
    blue = 255 * (value - 128) // 127
    return 0, 255 - blue, blue, 255


def duotone_pixel(value):
    return 255 - value, 0, value, 255


def apply_mapping(mapper):
    source = source_pixels()
    return bytes(
        component
        for value in source[0::4]
        for component in mapper(value)
    )


def synthetic_tritone():
    return apply_mapping(tritone_pixel)


def synthetic_duotone():
    return apply_mapping(duotone_pixel)


def mapping_by_input(source, output):
    mapping = {}
    for offset in range(0, len(source), 4):
        assert source[offset] == source[offset + 1] == source[offset + 2]
        value = source[offset]
        rgba = tuple(output[offset : offset + 4])
        if value in mapping:
            assert mapping[value] == rgba
        else:
            mapping[value] = rgba
    assert sorted(mapping) == list(range(256))
    return [mapping[value] for value in range(256)]


def assert_monotonic(values, *, increasing):
    pairs = zip(values, values[1:])
    if increasing:
        assert all(left <= right for left, right in pairs)
    else:
        assert all(left >= right for left, right in pairs)


def assert_tritone_response(tritone, duotone, mix100):
    source = source_pixels()
    assert len(tritone) == len(duotone) == len(mix100) == len(source)
    assert tritone != source and duotone != source and tritone != duotone
    assert mix100 == source

    mappings = []
    for output in (tritone, duotone):
        assert output[3::4] == source[3::4]
        mapping = mapping_by_input(source, output)
        assert len(set(mapping)) == 256
        mappings.append(mapping)

    tri, duo = mappings
    assert tri[0] == (255, 0, 0, 255)
    assert tri[128] == (0, 255, 0, 255)
    assert tri[-1][0] == 0 and tri[-1][1] <= 1 and tri[-1][2] >= 253
    assert all(pixel[2] == 0 for pixel in tri[:129])
    assert all(pixel[0] == 0 for pixel in tri[128:])
    assert_monotonic([pixel[0] for pixel in tri[:129]], increasing=False)
    assert_monotonic([pixel[1] for pixel in tri[:129]], increasing=True)
    assert_monotonic([pixel[1] for pixel in tri[128:]], increasing=False)
    assert_monotonic([pixel[2] for pixel in tri[128:]], increasing=True)

    assert duo[0] == (255, 0, 0, 255)
    assert duo[-1][0] == 0 and duo[-1][2] >= 254
    assert all(pixel[1] == 0 for pixel in duo)
    assert duo[128][0] > 0 and duo[128][2] > 0
    assert_monotonic([pixel[0] for pixel in duo], increasing=False)
    assert_monotonic([pixel[2] for pixel in duo], increasing=True)


@pytest.mark.parametrize(
    "fault",
    (
        "copy",
        "lost_midpoint",
        "channel_swap",
        "flat",
        "nonmonotonic",
        "x_only_gradient",
        "alpha",
        "mix_changed",
        "truncated",
    ),
)
def test_tritone_validator_rejects_corruption(fault):
    tritone = bytearray(synthetic_tritone())
    duotone = bytearray(synthetic_duotone())
    mix100 = bytearray(source_pixels())
    assert_tritone_response(tritone, duotone, mix100)

    if fault == "copy":
        tritone = mix100
    elif fault == "lost_midpoint":
        tritone = duotone
    elif fault == "channel_swap":
        tritone[0::4], tritone[2::4] = tritone[2::4], tritone[0::4]
    elif fault == "flat":
        tritone = tritone[:4] * (WIDTH * HEIGHT)
    elif fault == "nonmonotonic":
        source = source_pixels()
        for offset in range(0, len(tritone), 4):
            if source[offset] == 64:
                tritone[offset] = 200
    elif fault == "x_only_gradient":
        tritone = bytearray(
            component
            for _y in range(HEIGHT)
            for x in range(WIDTH)
            for component in tritone_pixel(x)
        )
    elif fault == "alpha":
        tritone[3] = 0
    elif fault == "mix_changed":
        mix100 = tritone
    else:
        duotone = duotone[:-4]

    with pytest.raises(AssertionError):
        assert_tritone_response(tritone, duotone, mix100)


def test_real_bcc_tritone_response(tmp_path):
    plugin = os.environ.get("AEXCOMPAT_TEST_BCC_TRITONE")
    if not plugin:
        pytest.skip("set AEXCOMPAT_TEST_BCC_TRITONE to the installed BCCTritone.aex")
    assert os.name == "nt"
    harness = ROOT / "broker/target/release/aexcompat-harness.exe"

    def run(*args):
        result = subprocess.run(
            [str(harness), "--headless", *map(str, args)],
            cwd=ROOT,
            capture_output=True,
            timeout=None if args[0] == "--inspect-experimental" else 90,
        )
        assert result.returncode == 0, result.stderr.decode("utf-8", errors="replace")
        return json.loads(result.stdout)

    params = {parameter["slot"]: parameter for parameter in run("--inspect-experimental", plugin)}
    assert params[6]["name"] == "Host Layer" and params[6]["kind"] == "layer"
    for slot, name in ((8, "Black Color"), (10, "Midpoint Color"), (11, "White Color")):
        assert params[slot]["name"] == name and params[slot]["kind"] == "color"
    assert params[9]["name"] == "Use Midpoint Color"
    assert params[9]["minimum"] == 0 and params[9]["maximum"] == 1
    assert params[12]["name"] == "Midpoint"
    assert params[12]["minimum"] == 0 and params[12]["maximum"] == 255
    assert params[13]["name"] == "Input Channel" and params[13]["choices"][0] == "Luma"
    assert params[14]["name"] == "Output Channels" and params[14]["choices"][0] == "RGB"
    assert params[15]["name"] == "Repeats"
    assert params[16]["name"] == "Repeat Mode"
    assert params[17]["name"] == "Mix with Original"
    assert params[17]["minimum"] == 0 and params[17]["maximum"] == 100
    assert params[19]["name"] == "PixelChooser" and params[19]["choices"][0] == "Off"

    source = tmp_path / "source.png"
    Image.frombytes("RGBA", (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, use_midpoint, mix in CASES:
        request = tmp_path / f"{name}.json"
        output = tmp_path / f"{name}.png"
        assignments = [{"slot": 6, "layer": str(source)}]
        assignments.extend({"slot": slot, "color": color} for slot, color in COLORS)
        assignments.extend(
            {"slot": slot, "value": value}
            for slot, value in (
                (9, use_midpoint),
                (12, 128),
                (13, 1),
                (14, 1),
                (15, 1),
                (16, 1),
                (17, mix),
                (19, 1),
            )
        )
        request.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "timing": {"frame": 0, "fps": 30, "duration_frames": 300},
                    "assignments": assignments,
                }
            ),
            encoding="utf-8",
        )
        report = run("--render-experimental-smart-request", plugin, source, output, request)
        assert report["passed"] and report["output_pixels_valid"]
        assert report["render_path"] == "smartfx"
        assert report["worker_classification"] == "ok"
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert("RGBA").tobytes()
        argb = bytearray(len(pixels))
        argb[0::4], argb[1::4] = pixels[3::4], pixels[0::4]
        argb[2::4], argb[3::4] = pixels[1::4], pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report["output_sha256"]
        outputs.append(pixels)
    assert_tritone_response(*outputs)
