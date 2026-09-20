"""Legacy BCC Beauty Studio smoothing response; not exact AE parity."""
import json
import os
import statistics
import subprocess
from pathlib import Path

import pytest
from PIL import Image, ImageFilter

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def noisy_edge_pixels():
    state = 0x27182818
    pixels = bytearray()
    for _y in range(HEIGHT):
        for x in range(WIDTH):
            state = (1664525 * state + 1013904223) & 0xffffffff
            noise = ((state >> 24) * 49 // 256) - 24
            value = (72 if x < WIDTH // 2 else 184) + noise
            pixels.extend((value, value, value, 255))
    return bytes(pixels)


def region_values(raw, left):
    start, end = ((16, WIDTH // 2 - 24) if left
                  else (WIDTH // 2 + 24, WIDTH - 16))
    return [raw[(y * WIDTH + x) * 4]
            for y in range(16, HEIGHT - 16)
            for x in range(start, end)]


def column_mean(raw, x):
    return statistics.mean(raw[(y * WIDTH + x) * 4]
                           for y in range(HEIGHT))


def row_edge_levels(raw):
    return [(
        statistics.mean(raw[(y * WIDTH + x) * 4] for x in range(WIDTH // 2 - 8,
                                                                  WIDTH // 2)),
        statistics.mean(raw[(y * WIDTH + x) * 4] for x in range(WIDTH // 2,
                                                                  WIDTH // 2 + 8)),
    ) for y in range(HEIGHT)]


def assert_beauty_studio_response(source, neutral, active):
    assert len(source) == len(neutral) == len(active) == WIDTH * HEIGHT * 4
    source_left, source_right = region_values(source, True), region_values(source, False)
    active_left, active_right = region_values(active, True), region_values(active, False)
    source_variance = statistics.pvariance(source_left), statistics.pvariance(source_right)
    active_variance = statistics.pvariance(active_left), statistics.pvariance(active_right)
    active_means = statistics.mean(active_left), statistics.mean(active_right)
    changed = sum(a != b for a, b in zip(source[0::4], active[0::4]))
    edge_levels = row_edge_levels(active)
    metrics = {
        'changed': changed,
        'source_variance': source_variance,
        'active_variance': active_variance,
        'active_means': active_means,
        'edge_jump': column_mean(active, WIDTH // 2) - column_mean(active, WIDTH // 2 - 1),
        'minimum_row_edge_contrast': min(right - left for left, right in edge_levels),
    }
    assert neutral == source
    assert active != source, metrics
    assert changed > WIDTH * HEIGHT * 3 // 4, metrics
    assert set(active[3::4]) == {255}
    assert active[0::4] == active[1::4] == active[2::4]
    assert active_variance[0] < source_variance[0] // 2, metrics
    assert active_variance[1] < source_variance[1] // 2, metrics
    assert abs(active_means[0] - 72) <= 4, metrics
    assert abs(active_means[1] - 184) <= 4, metrics
    assert active_means[1] - active_means[0] > 100, metrics
    assert metrics['edge_jump'] > 64, metrics
    assert all(56 <= left <= 112 and 144 <= right <= 208
               and right - left > 64 for left, right in edge_levels), metrics


@pytest.mark.parametrize('fault', [
    'copy', 'constant', 'global_blur', 'one_region', 'mean_shift', 'alpha',
    'color', 'neutral_damage', 'boundary_rows', 'reverse', 'truncated',
])
def test_beauty_studio_validator_rejects_corruption(fault):
    source = noisy_edge_pixels()
    neutral = bytearray(source)
    active = bytearray()
    for _y in range(HEIGHT):
        active.extend(bytes((72, 72, 72, 255)) * (WIDTH // 2))
        active.extend(bytes((184, 184, 184, 255)) * (WIDTH // 2))
    assert_beauty_studio_response(source, neutral, active)
    if fault == 'copy':
        active = bytearray(source)
    elif fault == 'constant':
        active = bytearray((128, 128, 128, 255) * (WIDTH * HEIGHT))
    elif fault == 'global_blur':
        image = Image.frombytes('RGBA', (WIDTH, HEIGHT), source)
        active = bytearray(image.filter(ImageFilter.GaussianBlur(8)).tobytes())
    elif fault == 'one_region':
        for y in range(HEIGHT):
            start = (y * WIDTH + WIDTH // 2) * 4
            end = (y + 1) * WIDTH * 4
            active[start:end] = source[start:end]
    elif fault == 'mean_shift':
        for y in range(HEIGHT):
            for x in range(WIDTH // 2):
                offset = (y * WIDTH + x) * 4
                active[offset:offset + 3] = bytes((88, 88, 88))
    elif fault == 'alpha':
        active[3] = 0
    elif fault == 'color':
        active[1] = 0
    elif fault == 'neutral_damage':
        neutral[0] = 0
    elif fault == 'boundary_rows':
        for y in range(32, 80):
            start = (y * WIDTH + WIDTH // 2 - 24) * 4
            end = (y * WIDTH + WIDTH // 2 + 24) * 4
            active[start:end] = bytes((0, 0, 0, 255)) * 48
    elif fault == 'reverse':
        for y in range(HEIGHT):
            row = active[y * WIDTH * 4:(y + 1) * WIDTH * 4]
            active[y * WIDTH * 4:(y + 1) * WIDTH * 4] = b''.join(
                row[x * 4:(x + 1) * 4] for x in range(WIDTH - 1, -1, -1))
    else:
        active = active[:-4]
    with pytest.raises(AssertionError):
        assert_beauty_studio_response(source, neutral, active)


def test_installed_bcc_obsolete_beauty_studio_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCBeautyStudio.aex'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    for slot, name, kind in (
            (6, 'Host Layer', 'layer'), (9, 'Master Amount', 'float'),
            (10, 'Smooth Smallest Details', 'float'),
            (14, 'Smooth Largest Details', 'float'),
            (15, 'Detail Levels', 'integer'),
            (16, 'Preserve Contrast', 'float'),
            (17, 'Detail Scale', 'float'),
            (43, 'Mix with Original', 'float'),
            (45, 'PixelChooser', 'integer')):
        assert parameters[slot]['name'] == name
        assert parameters[slot]['kind'] == kind
    assert parameters[45]['minimum'] == 1
    assert parameters[45]['maximum'] == 4
    assert parameters[45]['value'] == 2
    assert parameters[45]['choices'][:2] == ['Off', 'On']

    source_pixels = noisy_edge_pixels()
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels).save(source)
    outputs = {}
    for name, mix in (('neutral', 100), ('smooth', 0)):
        request = tmp_path / f'{name}.json'
        output = tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(source)},
                {'slot': 9, 'value': 300},
                {'slot': 10, 'value': 100},
                {'slot': 11, 'value': 100},
                {'slot': 12, 'value': 100},
                {'slot': 13, 'value': 100},
                {'slot': 14, 'value': 100},
                {'slot': 15, 'value': 5},
                {'slot': 16, 'value': 90},
                {'slot': 17, 'value': 100},
                {'slot': 19, 'value': 0},
                {'slot': 27, 'value': 0},
                {'slot': 35, 'value': 0},
                {'slot': 43, 'value': mix},
                {'slot': 45, 'value': 1},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[9] == 300 and requested[15] == 5
        assert requested[10] == requested[14] == 100
        assert requested[16] == 90
        assert requested[43] == mix
        assert requested[45] == 1
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs[name] = image.convert('RGBA').tobytes()

    assert_beauty_studio_response(source_pixels, outputs['neutral'], outputs['smooth'])
