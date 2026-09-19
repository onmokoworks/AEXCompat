"""Legacy BCC Safe Colors saturation clipping; not exact AE parity."""
import colorsys
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def hue_pixels():
    row = bytearray()
    for x in range(WIDTH):
        red, green, blue = colorsys.hsv_to_rgb(x / WIDTH, 1, 1)
        row.extend((round(red * 255), round(green * 255), round(blue * 255), 255))
    return bytes(row) * HEIGHT


def clipped_hue_pixels():
    row = bytearray()
    for x in range(WIDTH):
        red, green, blue = colorsys.hsv_to_rgb(x / WIDTH, 1, 1)
        source = tuple(round(value * 255) for value in (red, green, blue))
        luma = 0.2126 * source[0] + 0.7152 * source[1] + 0.0722 * source[2]
        row.extend((*[round(luma + 0.5 * (value - luma)) for value in source],
                    255))
    return bytes(row) * HEIGHT


def assert_safe_colors_response(neutral, clipped):
    source = hue_pixels()
    assert neutral == source
    assert len(clipped) == WIDTH * HEIGHT * 4
    assert clipped[3::4] == bytes([255]) * WIDTH * HEIGHT
    first_row = clipped[:WIDTH * 4]
    assert all(clipped[y * WIDTH * 4:(y + 1) * WIDTH * 4] == first_row
               for y in range(1, HEIGHT))
    colors = {bytes(clipped[offset:offset + 3])
              for offset in range(0, WIDTH * 4, 4)}
    assert len(colors) >= 200
    changed = 0
    source_luma = []
    clipped_luma = []
    for offset in range(0, len(source), 4):
        original = source[offset:offset + 3]
        result = clipped[offset:offset + 3]
        changed += original != result
        chroma = max(result) - min(result)
        assert 120 <= chroma <= 135
        source_luma.append(0.2126 * original[0] + 0.7152 * original[1]
                           + 0.0722 * original[2])
        clipped_luma.append(0.2126 * result[0] + 0.7152 * result[1]
                            + 0.0722 * result[2])
        original_max = {index for index, value in enumerate(original)
                        if value == max(original)}
        result_max = {index for index, value in enumerate(result)
                      if value == max(result)}
        original_min = {index for index, value in enumerate(original)
                        if value == min(original)}
        result_min = {index for index, value in enumerate(result)
                      if value == min(result)}
        assert original_max & result_max
        assert original_min & result_min
    assert changed >= WIDTH * HEIGHT * 95 // 100
    assert abs(sum(source_luma) - sum(clipped_luma)) / len(source_luma) <= 1
    assert (sum(abs(before - after)
                for before, after in zip(source_luma, clipped_luma))
            / len(source_luma)) <= 15


@pytest.mark.parametrize('fault', [
    'copy', 'grayscale', 'uniform', 'oversaturated', 'darkened', 'hue_shift',
    'transparent', 'spatial', 'wrong_neutral', 'truncated',
])
def test_safe_colors_validator_rejects_corruption(fault):
    neutral, clipped = hue_pixels(), bytearray(clipped_hue_pixels())
    assert_safe_colors_response(neutral, clipped)
    if fault == 'copy':
        clipped = neutral
    elif fault == 'grayscale':
        for offset in range(0, len(clipped), 4):
            value = sum(clipped[offset:offset + 3]) // 3
            clipped[offset:offset + 3] = bytes((value, value, value))
    elif fault == 'uniform':
        clipped = bytes((255, 128, 128, 255)) * WIDTH * HEIGHT
    elif fault == 'oversaturated':
        clipped = neutral
    elif fault == 'darkened':
        clipped = bytearray(neutral)
        for offset in range(0, len(clipped), 4):
            clipped[offset:offset + 3] = bytes(
                value // 2 for value in clipped[offset:offset + 3])
    elif fault == 'hue_shift':
        shift = 32 * 4
        row = clipped[:WIDTH * 4]
        shifted = row[shift:] + row[:shift]
        clipped = shifted * HEIGHT
    elif fault == 'transparent':
        clipped[3] = 0
    elif fault == 'spatial':
        offset = WIDTH * 4
        clipped[offset:offset + 3] = bytes((128, 255, 128))
    elif fault == 'wrong_neutral':
        neutral = clipped
    else:
        clipped = clipped[:-4]
    with pytest.raises(AssertionError):
        assert_safe_colors_response(neutral, clipped)


def test_installed_bcc_obsolete_safe_colors_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCSafeColors.aex'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert parameters[6]['name'] == 'Host Layer'
    assert parameters[6]['kind'] == 'layer'
    assert parameters[8]['name'] == 'Saturation Soft Clip'
    assert parameters[8]['kind'] == 'float'
    assert parameters[9]['name'] == 'Saturation Hard Clip'
    assert parameters[9]['kind'] == 'float'
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), hue_pixels()).save(source)
    outputs = []
    for name, soft, hard in (('neutral', 100, 100), ('clipped', 0, 50)):
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(source)},
                {'slot': 8, 'value': soft},
                {'slot': 9, 'value': hard},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[8] == soft and requested[9] == hard
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_safe_colors_response(*outputs)
