"""Legacy BCC Edge Cleaner alpha response; not exact AE parity."""
import json
import os
import statistics
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def jagged_alpha_pixels():
    return bytes(
        channel
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in (200, 80, 40, 255 if x >= 96 + (y % 4) * 4 else 0)
    )


def cleaned_alpha_pixels(smooth=True, midpoint_offset=0):
    pixels = bytearray()
    for y in range(HEIGHT):
        midpoint = (96 + round(12 * y / (HEIGHT - 1)) if smooth
                    else 96 + (y % 4) * 4)
        midpoint += midpoint_offset
        for x in range(WIDTH):
            alpha = max(0, min(255, round(255 * (x - midpoint + 26) / 52)))
            pixels.extend((200, 80, 40, alpha))
    return bytes(pixels)


def assert_edge_cleaner_response(neutral, cleaned):
    source_alpha = jagged_alpha_pixels()[3::4]
    neutral_alpha = neutral[3::4]
    assert neutral_alpha == source_alpha
    assert len(cleaned) == WIDTH * HEIGHT * 4
    cleaned_alpha = cleaned[3::4]
    partial = sum(0 < alpha < 255 for alpha in cleaned_alpha)
    assert partial >= 7000
    assert len(set(cleaned_alpha)) >= 50
    changed = [index for index, pair in enumerate(zip(source_alpha, cleaned_alpha))
               if pair[0] != pair[1]]
    assert changed
    assert min(index % WIDTH for index in changed) >= 60
    assert max(index % WIDTH for index in changed) <= 145
    row_changes = [
        sum(source_alpha[y * WIDTH + x] != cleaned_alpha[y * WIDTH + x]
            for x in range(WIDTH))
        for y in range(HEIGHT)
    ]
    assert min(row_changes) >= 40
    assert max(row_changes) <= 70
    assert abs(sum(cleaned_alpha) - sum(source_alpha)) <= WIDTH * HEIGHT * 255 // 50

    def midpoints(alpha):
        return [next(x for x in range(WIDTH)
                     if alpha[y * WIDTH + x] >= 128)
                for y in range(HEIGHT)]

    source_midpoints = midpoints(source_alpha)
    cleaned_midpoints = midpoints(cleaned_alpha)
    assert len(set(cleaned_midpoints)) >= 8
    assert statistics.pstdev(cleaned_midpoints) < statistics.pstdev(source_midpoints)
    source_steps = [abs(right - left)
                    for left, right in zip(source_midpoints, source_midpoints[1:])]
    cleaned_steps = [abs(right - left)
                     for left, right in zip(cleaned_midpoints, cleaned_midpoints[1:])]
    assert statistics.mean(cleaned_steps) < statistics.mean(source_steps)
    assert statistics.mean(cleaned_steps) <= 1
    assert max(cleaned_steps) <= 2


@pytest.mark.parametrize('fault', [
    'copy', 'uniform', 'nonlocal', 'opaque_loss', 'transparent_loss',
    'row_damage', 'jagged_midpoint', 'reordered', 'mass_shift', 'wrong_neutral',
    'truncated',
])
def test_edge_cleaner_validator_rejects_corruption(fault):
    neutral = jagged_alpha_pixels()
    cleaned = bytearray(cleaned_alpha_pixels())
    assert_edge_cleaner_response(neutral, cleaned)
    if fault == 'copy':
        cleaned = neutral
    elif fault == 'uniform':
        cleaned = bytes((200, 80, 40, 128)) * WIDTH * HEIGHT
    elif fault == 'nonlocal':
        cleaned[3] = 1
    elif fault == 'opaque_loss':
        cleaned[-1] = 0
    elif fault == 'transparent_loss':
        cleaned[3] = 255
    elif fault == 'row_damage':
        cleaned[3:WIDTH * 4:4] = bytes([128]) * WIDTH
    elif fault == 'jagged_midpoint':
        cleaned = cleaned_alpha_pixels(smooth=False)
    elif fault == 'reordered':
        row_size = WIDTH * 4
        rows = [cleaned[y * row_size:(y + 1) * row_size]
                for y in range(HEIGHT)]
        order = []
        for low in range((HEIGHT + 1) // 2):
            order.append(low)
            high = HEIGHT - 1 - low
            if high != low:
                order.append(high)
        cleaned = b''.join(rows[index] for index in order)
    elif fault == 'mass_shift':
        cleaned = cleaned_alpha_pixels(midpoint_offset=8)
    elif fault == 'wrong_neutral':
        neutral = cleaned
    else:
        cleaned = cleaned[:-4]
    with pytest.raises(AssertionError):
        assert_edge_cleaner_response(neutral, cleaned)


def test_installed_bcc_obsolete_edge_cleaner_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCEdgeCleaner.aex'
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
    assert parameters[8]['name'] == 'Cleaning Radius'
    assert parameters[8]['kind'] == 'float'
    assert parameters[12]['name'] == 'Strength'
    assert parameters[12]['kind'] == 'float'
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), jagged_alpha_pixels()).save(source)
    outputs = []
    for name, radius in (('neutral', 0), ('cleaned', 20)):
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(source)},
                {'slot': 8, 'value': radius},
                {'slot': 10, 'value': 0},
                {'slot': 11, 'value': 0},
                {'slot': 12, 'value': 100},
                {'slot': 13, 'value': 90},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[8] == radius
        assert requested[10] == 0
        assert requested[11] == 0
        assert requested[12] == 100
        assert requested[13] == 90
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_edge_cleaner_response(*outputs)
