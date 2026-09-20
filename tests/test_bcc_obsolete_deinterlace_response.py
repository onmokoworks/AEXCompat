"""Legacy BCC DeInterlace response on a deterministic combed edge."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def combed_pixels():
    pixels = bytearray()
    for y in range(HEIGHT):
        edge = 80 if y % 2 == 0 else 120
        for x in range(WIDTH):
            value = 32 if x < edge else 224
            pixels.extend((value, value, value, 255))
    return bytes(pixels)


def adjacent_row_difference(pixels):
    rowbytes = WIDTH * 4
    return sum(
        abs(pixels[y * rowbytes + x] - pixels[(y - 1) * rowbytes + x])
        for y in range(1, HEIGHT)
        for x in range(0, rowbytes, 4)
    )


def assert_deinterlace_response(source, neutral, active):
    assert len(source) == len(neutral) == len(active) == WIDTH * HEIGHT * 4
    pixel_count = WIDTH * HEIGHT
    source_difference = adjacent_row_difference(source)
    neutral_difference = adjacent_row_difference(neutral)
    active_difference = adjacent_row_difference(active)
    neutral_changed = sum(a != b for a, b in zip(neutral[0::4], source[0::4]))
    active_changed = sum(a != b for a, b in zip(active[0::4], source[0::4]))
    assert neutral_changed < pixel_count // 20
    assert abs(neutral_difference - source_difference) < source_difference // 50
    assert active != source
    assert active_changed > neutral_changed * 3
    assert active_difference < source_difference // 100
    assert len(set(active[0::4])) > 1
    assert all(active[offset] == active[offset + 1] == active[offset + 2]
               for offset in range(0, len(active), 4))
    assert set(neutral[3::4]) == set(active[3::4]) == {255}
    dark_far = [active[(y * WIDTH + x) * 4]
                for y in range(HEIGHT) for x in range(64)]
    bright_far = [active[(y * WIDTH + x) * 4]
                  for y in range(HEIGHT) for x in range(136, WIDTH)]
    assert max(dark_far) <= 40
    assert min(bright_far) >= 216
    assert all(64 <= (index % WIDTH) < 136
               for index, (before, after) in enumerate(
                   zip(source[0::4], active[0::4])) if before != after)


@pytest.mark.parametrize('fault', [
    'copy', 'constant', 'comb', 'alpha', 'color', 'neutral_damage', 'truncated',
    'gradient', 'reverse',
])
def test_deinterlace_validator_rejects_corruption(fault):
    source = combed_pixels()
    neutral = bytearray(source)
    active = bytearray(source)
    for y in range(1, HEIGHT, 2):
        active[y * WIDTH * 4:(y + 1) * WIDTH * 4] = active[(y - 1) * WIDTH * 4:y * WIDTH * 4]
    assert_deinterlace_response(source, neutral, active)
    if fault == 'copy':
        active = bytearray(source)
    elif fault == 'constant':
        active = bytearray((128, 128, 128, 255) * (WIDTH * HEIGHT))
    elif fault == 'comb':
        active = bytearray(source)
        active[0] = 33
    elif fault == 'alpha':
        active[3] = 0
    elif fault == 'color':
        active[1] = 99
    elif fault == 'neutral_damage':
        neutral[0::4] = bytes([128]) * (WIDTH * HEIGHT)
    elif fault == 'gradient':
        for y in range(HEIGHT):
            for x in range(WIDTH):
                value = 32 + x * 192 // (WIDTH - 1)
                active[(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 3] = \
                    bytes((value, value, value))
    elif fault == 'reverse':
        for y in range(HEIGHT):
            row = active[y * WIDTH * 4:(y + 1) * WIDTH * 4]
            active[y * WIDTH * 4:(y + 1) * WIDTH * 4] = b''.join(
                row[x * 4:(x + 1) * 4] for x in range(WIDTH - 1, -1, -1))
    else:
        active = active[:-4]
    with pytest.raises(AssertionError):
        assert_deinterlace_response(source, neutral, active)


def test_installed_bcc_obsolete_deinterlace_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCDeInterlace.aex'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    for slot, name, kind in (
            (6, 'Host Layer', 'layer'), (9, 'Operation', 'integer'),
            (10, 'Input Field Order', 'integer'),
            (11, 'Motion Sensitivity', 'float'),
            (12, 'Motion Filter Size', 'float'),
            (13, 'Interpolation Slant', 'float'),
            (16, 'Mix with Original', 'float')):
        assert parameters[slot]['name'] == name
        assert parameters[slot]['kind'] == kind

    source_pixels = combed_pixels()
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels).save(source)
    outputs = {}
    for name, mix in (('neutral', 100), ('active', 0)):
        request = tmp_path / f'{name}.json'
        output = tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(source)},
                {'slot': 9, 'value': 2},
                {'slot': 10, 'value': 1},
                {'slot': 11, 'value': 100},
                {'slot': 12, 'value': 8},
                {'slot': 13, 'value': 1},
                {'slot': 16, 'value': mix},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs[name] = image.convert('RGBA').tobytes()

    assert_deinterlace_response(source_pixels, outputs['neutral'], outputs['active'])
