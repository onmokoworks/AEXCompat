"""Legacy BCC DV Fixer chroma response; not exact AE parity."""
import json
import os
import statistics
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def chroma_blocks_pixels():
    colors = ((220, 64, 64, 255), (64, 64, 220, 255))
    return bytes(
        channel
        for _y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in colors[(x // 2) % 2]
    )


def fixed_chroma_pixels():
    left = [158, 157, 155, 152, 148, 145, 143]
    right = [141, 139, 136, 132, 129, 127, 126]
    row = bytearray()
    for x in range(WIDTH):
        red = left[x] if x < len(left) else (
            right[x - (WIDTH - len(right))] if x >= WIDTH - len(right) else 142)
        row.extend((red, 64, 284 - red, 255))
    return bytes(row) * HEIGHT


def channel_variation(raw, channel):
    row = raw[channel:WIDTH * 4:4]
    return sum(abs(right - left) for left, right in zip(row, row[1:]))


def assert_dv_fixer_response(neutral, fixed):
    source = chroma_blocks_pixels()
    assert neutral == source
    assert len(fixed) == WIDTH * HEIGHT * 4
    assert fixed[3::4] == bytes([255]) * WIDTH * HEIGHT
    assert fixed[1::4] == bytes([64]) * WIDTH * HEIGHT
    first_row = fixed[:WIDTH * 4]
    assert all(fixed[y * WIDTH * 4:(y + 1) * WIDTH * 4] == first_row
               for y in range(1, HEIGHT))
    source_red_variation = channel_variation(source, 0)
    source_blue_variation = channel_variation(source, 2)
    red_variation = channel_variation(fixed, 0)
    blue_variation = channel_variation(fixed, 2)
    assert 0 < red_variation <= source_red_variation // 100
    assert 0 < blue_variation <= source_blue_variation // 100
    assert abs(statistics.mean(fixed[0::4]) - 142) <= 1
    assert abs(statistics.mean(fixed[2::4]) - 142) <= 1
    colors = set(zip(fixed[0::4], fixed[1::4], fixed[2::4]))
    assert len(colors) >= 10
    assert all(280 <= red + blue <= 288 for red, _green, blue in colors)
    assert fixed[0] > fixed[2]
    assert fixed[-4] < fixed[-2]
    changed = sum(fixed[offset:offset + 3] != source[offset:offset + 3]
                  for offset in range(0, len(source), 4))
    assert changed >= WIDTH * HEIGHT * 95 // 100


@pytest.mark.parametrize('fault', [
    'copy', 'uniform', 'high_variation', 'gray', 'dark', 'hue_flip',
    'transparent', 'spatial', 'wrong_green', 'wrong_neutral', 'truncated',
])
def test_dv_fixer_validator_rejects_corruption(fault):
    neutral = chroma_blocks_pixels()
    fixed = bytearray(fixed_chroma_pixels())
    assert_dv_fixer_response(neutral, fixed)
    if fault == 'copy':
        fixed = neutral
    elif fault == 'uniform':
        fixed = bytes((142, 64, 142, 255)) * WIDTH * HEIGHT
    elif fault == 'high_variation':
        fixed = neutral
    elif fault == 'gray':
        fixed[1::4] = bytes([142]) * WIDTH * HEIGHT
    elif fault == 'dark':
        for offset in range(0, len(fixed), 4):
            fixed[offset] -= 30
            fixed[offset + 2] -= 30
    elif fault == 'hue_flip':
        for offset in range(0, len(fixed), 4):
            fixed[offset], fixed[offset + 2] = fixed[offset + 2], fixed[offset]
    elif fault == 'transparent':
        fixed[3] = 0
    elif fault == 'spatial':
        fixed[WIDTH * 4:WIDTH * 4 + 3] = bytes((142, 64, 142))
    elif fault == 'wrong_green':
        fixed[1] = 0
    elif fault == 'wrong_neutral':
        neutral = fixed
    else:
        fixed = fixed[:-4]
    with pytest.raises(AssertionError):
        assert_dv_fixer_response(neutral, fixed)


def test_installed_bcc_obsolete_dv_fixer_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCDVFixer.aex'
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
    assert parameters[9]['name'] == 'Threshold'
    assert parameters[9]['kind'] == 'float'
    assert parameters[11]['name'] == 'Iterations'
    assert parameters[11]['kind'] == 'float'
    assert parameters[14]['name'] == 'Mix with Original'
    assert parameters[14]['kind'] == 'float'
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), chroma_blocks_pixels()).save(source)
    outputs = []
    for name, mix in (('neutral', 100), ('fixed', 0)):
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(source)},
                {'slot': 8, 'value': 0},
                {'slot': 9, 'value': 0},
                {'slot': 10, 'value': 50},
                {'slot': 11, 'value': 20},
                {'slot': 12, 'value': 1},
                {'slot': 13, 'value': 100},
                {'slot': 14, 'value': mix},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[8] == 0
        assert requested[9] == 0
        assert requested[11] == 20
        assert requested[13] == 100
        assert requested[14] == mix
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_dv_fixer_response(*outputs)
