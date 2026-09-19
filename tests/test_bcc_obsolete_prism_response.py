"""Legacy BCC Prism spectral response; not exact AE parity."""
import json
import math
import os
import statistics
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def bar_pixels():
    return bytes(
        channel
        for _y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in ((230, 230, 230, 255) if (x // 16) % 2
                        else (20, 20, 20, 255))
    )


def prism_pixels():
    pixels = bytearray()
    for y in range(HEIGHT):
        position = y / (HEIGHT - 1)
        red = round(125 + 100 * math.sin(16 * math.pi * position)
                    + 20 * (0.5 - position))
        green = round(125 + 100 * math.sin(8 * math.pi * position)
                      + 10 * (0.5 - position))
        blue = round(125 + 80 * math.sin(4 * math.pi * position)
                     - 80 * (0.5 - position))
        color = tuple(max(0, min(255, value)) for value in (red, green, blue))
        pixels.extend(bytes((*color, 255)) * WIDTH)
    return bytes(pixels)


def assert_prism_response(neutral, prism):
    source = bar_pixels()
    assert neutral == source
    assert len(prism) == WIDTH * HEIGHT * 4
    assert prism[3::4] == bytes([255]) * WIDTH * HEIGHT
    for y in range(HEIGHT):
        row = prism[y * WIDTH * 4:(y + 1) * WIDTH * 4]
        for channel in range(3):
            values = row[channel::4]
            assert max(values) - min(values) <= 1
    profiles = [
        [prism[(y * WIDTH + WIDTH // 2) * 4 + channel]
         for y in range(HEIGHT)]
        for channel in range(3)
    ]
    for profile in profiles:
        assert max(profile) - min(profile) >= 180
        assert abs(statistics.mean(profile) - 125) <= 3
    pair_differences = [
        sum(left != right for left, right in zip(profiles[first], profiles[second]))
        for first, second in ((0, 1), (1, 2), (0, 2))
    ]
    assert min(pair_differences) >= HEIGHT * 3 // 4

    def crossings(profile):
        return sum((left < 125) != (right < 125)
                   for left, right in zip(profile, profile[1:]))

    crossing_counts = [crossings(profile) for profile in profiles]
    assert crossing_counts[0] >= 12
    assert 5 <= crossing_counts[1] <= 10
    assert 3 <= crossing_counts[2] <= 7
    assert crossing_counts[0] > crossing_counts[1] > crossing_counts[2]
    assert prism[0] > prism[1] > prism[2]
    assert prism[-4] < prism[-3] < prism[-2]
    colored = sum(not (prism[offset] == prism[offset + 1] == prism[offset + 2])
                  for offset in range(0, len(prism), 4))
    assert colored >= WIDTH * HEIGHT * 3 // 4


@pytest.mark.parametrize('fault', [
    'copy', 'gray', 'uniform', 'row_damage', 'transparent', 'swap_rb',
    'reverse', 'low_dynamic', 'wrong_mean', 'wrong_neutral', 'truncated',
])
def test_prism_validator_rejects_corruption(fault):
    neutral = bar_pixels()
    prism = bytearray(prism_pixels())
    assert_prism_response(neutral, prism)
    if fault == 'copy':
        prism = neutral
    elif fault == 'gray':
        for offset in range(0, len(prism), 4):
            value = prism[offset]
            prism[offset:offset + 3] = bytes((value, value, value))
    elif fault == 'uniform':
        prism = bytes((135, 125, 85, 255)) * WIDTH * HEIGHT
    elif fault == 'row_damage':
        prism[4:7] = bytes((0, 0, 0))
    elif fault == 'transparent':
        prism[3] = 0
    elif fault == 'swap_rb':
        for offset in range(0, len(prism), 4):
            prism[offset], prism[offset + 2] = prism[offset + 2], prism[offset]
    elif fault == 'reverse':
        row_size = WIDTH * 4
        rows = [prism[y * row_size:(y + 1) * row_size]
                for y in range(HEIGHT)]
        prism = b''.join(reversed(rows))
    elif fault == 'low_dynamic':
        for offset in range(0, len(prism), 4):
            for channel in range(3):
                prism[offset + channel] = 90 + prism[offset + channel] // 4
    elif fault == 'wrong_mean':
        for offset in range(0, len(prism), 4):
            for channel in range(3):
                prism[offset + channel] = min(255, prism[offset + channel] + 30)
    elif fault == 'wrong_neutral':
        neutral = prism
    else:
        prism = prism[:-4]
    with pytest.raises(AssertionError):
        assert_prism_response(neutral, prism)


def test_installed_bcc_obsolete_prism_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCPrism.aex'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert parameters[3]['name'] == 'Starting'
    assert parameters[3]['kind'] == 'point'
    assert parameters[4]['name'] == 'Ending'
    assert parameters[4]['kind'] == 'point'
    assert parameters[10]['name'] == 'Prism Amount'
    assert parameters[10]['kind'] == 'float'
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), bar_pixels()).save(source)
    outputs = []
    for name, mix in (('neutral', 100), ('prism', 0)):
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 2, 'value': 100},
                {'slot': 3, 'components': [30, 50]},
                {'slot': 4, 'components': [70, 50]},
                {'slot': 5, 'value': 0.5},
                {'slot': 6, 'value': 2},
                {'slot': 8, 'components': [0]},
                {'slot': 9, 'components': [0]},
                {'slot': 10, 'value': 100},
                {'slot': 16, 'value': 3},
                {'slot': 17, 'value': mix},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p for p in report['requested_parameters']}
        assert requested[3]['value'][:2] == [30, 50]
        assert requested[4]['value'][:2] == [70, 50]
        assert requested[5]['value'] == 0.5
        assert requested[6]['value'] == 2
        assert requested[10]['value'] == 100
        assert requested[17]['value'] == mix
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_prism_response(*outputs)
