"""Legacy BCC Magic Sharp edge response; not exact AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def soft_edge_pixels():
    def value(x):
        if x < 96:
            return 40
        if x >= 116:
            return 200
        return 40 + (x - 96) * 8

    return bytes(
        channel
        for _y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in (value(x), value(x), value(x), 255)
    )


def sharpened_edge_pixels():
    source = bytearray(soft_edge_pixels()[:WIDTH * 4])
    values = {
        92: 30, 93: 10, 94: 0, 95: 0, 96: 0, 97: 255, 98: 120,
        114: 140, 115: 0, 116: 255, 117: 240,
    }
    for x, value in values.items():
        source[x * 4:x * 4 + 3] = bytes((value, value, value))
    return bytes(source) * HEIGHT


def assert_magic_sharp_response(neutral, sharp):
    source = soft_edge_pixels()
    assert neutral == source
    assert len(sharp) == WIDTH * HEIGHT * 4
    assert sharp[3::4] == bytes([255]) * WIDTH * HEIGHT
    assert sharp[0::4] == sharp[1::4] == sharp[2::4]
    first_row = sharp[:WIDTH * 4]
    assert all(sharp[y * WIDTH * 4:(y + 1) * WIDTH * 4] == first_row
               for y in range(1, HEIGHT))
    source_row = list(source[:WIDTH * 4:4])
    sharp_row = list(first_row[0::4])
    changed = [x for x, pair in enumerate(zip(source_row, sharp_row))
               if pair[0] != pair[1]]
    assert 8 <= len(changed) <= 40
    assert all(88 <= x <= 124 for x in changed)
    assert any(sharp_row[x] < source_row[x] for x in changed)
    assert any(sharp_row[x] > source_row[x] for x in changed)
    assert min(sharp_row) <= 10
    assert max(sharp_row) >= 245
    assert max(abs(right - left)
               for left, right in zip(sharp_row, sharp_row[1:])) >= 200


@pytest.mark.parametrize('fault', [
    'copy', 'weak', 'nonlocal', 'color', 'transparent', 'spatial', 'shifted',
    'one_sided', 'wrong_neutral', 'truncated',
])
def test_magic_sharp_validator_rejects_corruption(fault):
    neutral = soft_edge_pixels()
    sharp = bytearray(sharpened_edge_pixels())
    assert_magic_sharp_response(neutral, sharp)
    if fault == 'copy':
        sharp = neutral
    elif fault == 'weak':
        sharp = bytearray(neutral)
        for x in range(92, 100):
            value = sharp[x * 4] + 1
            sharp[x * 4:x * 4 + 3] = bytes((value, value, value))
    elif fault == 'nonlocal':
        sharp[0:3] = bytes((0, 0, 0))
    elif fault == 'color':
        sharp[1] = 0
    elif fault == 'transparent':
        sharp[3] = 0
    elif fault == 'spatial':
        sharp[WIDTH * 4:WIDTH * 4 + 3] = bytes((0, 0, 0))
    elif fault == 'shifted':
        row = sharp[:WIDTH * 4]
        sharp = row[4:] + row[:4]
        sharp *= HEIGHT
    elif fault == 'one_sided':
        sharp = bytearray(neutral)
        for x in range(92, 100):
            sharp[x * 4:x * 4 + 3] = bytes((255, 255, 255))
    elif fault == 'wrong_neutral':
        neutral = sharp
    else:
        sharp = sharp[:-4]
    with pytest.raises(AssertionError):
        assert_magic_sharp_response(neutral, sharp)


def test_installed_bcc_obsolete_magic_sharp_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCMagicSharp.aex'
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
    assert parameters[8]['name'] == 'Sharpen Amount'
    assert parameters[8]['kind'] == 'float'
    assert parameters[10]['name'] == 'Sharpen Radius'
    assert parameters[10]['kind'] == 'float'
    assert parameters[14]['name'] == 'Fine Sharpen'
    assert parameters[14]['kind'] == 'float'
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), soft_edge_pixels()).save(source)
    outputs = []
    for name, amount in (('neutral', 0), ('sharp', 3000)):
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(source)},
                {'slot': 8, 'value': amount},
                {'slot': 9, 'value': 1},
                {'slot': 10, 'value': 4},
                {'slot': 13, 'value': 1},
                {'slot': 14, 'value': 3000},
                {'slot': 15, 'value': 0},
                {'slot': 17, 'value': 0},
                {'slot': 19, 'value': 0},
                {'slot': 35, 'value': 0},
                {'slot': 40, 'value': 0},
                {'slot': 43, 'value': 0},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[8] == amount
        assert requested[9] == 1
        assert requested[10] == 4
        assert requested[13] == 1
        assert requested[14] == 3000
        assert requested[43] == 0
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_magic_sharp_response(*outputs)
