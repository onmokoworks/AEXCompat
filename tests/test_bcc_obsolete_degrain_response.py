"""Legacy BCC DeGrain temporal-sample response; not exact AE parity."""
import json
import os
import statistics
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def clean_edge_pixels():
    return bytes(
        channel
        for _y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in ((72, 72, 72, 255) if x < WIDTH // 2
                        else (184, 184, 184, 255))
    )


def noisy_edge_pixels(frame):
    """Give each temporal sample independent, approximately zero-mean grain."""
    state = 0xD3E60000 ^ (frame * 0x9E3779B9)
    pixels = bytearray()
    for _y in range(HEIGHT):
        for x in range(WIDTH):
            state = (1664525 * state + 1013904223) & 0xffffffff
            noise = ((state >> 24) * 41 // 256) - 20
            value = (72 if x < WIDTH // 2 else 184) + noise
            pixels.extend((value, value, value, 255))
    return bytes(pixels)


def region_values(raw, left):
    start, end = ((16, WIDTH // 2 - 24) if left
                  else (WIDTH // 2 + 24, WIDTH - 16))
    return [raw[(y * WIDTH + x) * 4]
            for y in range(16, HEIGHT - 16)
            for x in range(start, end)]


def mean_squared_error(actual, expected):
    return statistics.mean(
        (actual[offset] - expected[offset]) ** 2
        for offset in range(0, len(expected), 4)
    )


def assert_degrain_response(source, neutral, active):
    clean = clean_edge_pixels()
    assert len(source) == len(neutral) == len(active) == len(clean)
    assert neutral == source
    assert active != source
    assert active[3::4] == bytes([255]) * WIDTH * HEIGHT
    assert active[0::4] == active[1::4] == active[2::4]

    source_regions = region_values(source, True), region_values(source, False)
    active_regions = region_values(active, True), region_values(active, False)
    source_variance = tuple(statistics.pvariance(values) for values in source_regions)
    active_variance = tuple(statistics.pvariance(values) for values in active_regions)
    source_mse = mean_squared_error(source, clean)
    active_mse = mean_squared_error(active, clean)
    active_means = tuple(statistics.mean(values) for values in active_regions)
    metrics = {
        'source_mse': source_mse,
        'active_mse': active_mse,
        'source_variance': source_variance,
        'active_variance': active_variance,
        'active_means': active_means,
    }
    assert active_mse < source_mse * 0.8, metrics
    assert active_variance[0] < source_variance[0] * 0.8, metrics
    assert active_variance[1] < source_variance[1] * 0.8, metrics
    assert abs(active_means[0] - 72) <= 3, metrics
    assert abs(active_means[1] - 184) <= 3, metrics
    assert active_means[1] - active_means[0] >= 100, metrics


def test_installed_bcc_obsolete_degrain_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCDegrain.aex'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    for slot, name, kind in (
            (6, 'Host Layer', 'layer'), (10, 'Setup', 'integer'),
            (11, 'Lock Sample', 'integer'), (13, 'Sample Layer', 'layer'),
            (14, 'Sample Size', 'float'), (18, 'Filter Strength', 'float'),
            (19, 'HiPass Filter', 'float'), (20, 'Threshold', 'float'),
            (21, 'Edge Padding', 'float'), (22, 'Mix with Original', 'float')):
        assert parameters[slot]['name'] == name
        assert parameters[slot]['kind'] == kind

    frames = {}
    for frame in range(5):
        path = tmp_path / f'frame-{frame}.png'
        Image.frombytes('RGBA', (WIDTH, HEIGHT), noisy_edge_pixels(frame)).save(path)
        frames[frame] = path

    outputs = {}
    reports = {}
    for label, mix in (('neutral', 100), ('active', 0)):
        request = tmp_path / f'{label}.json'
        output = tmp_path / f'{label}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 2, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(frames[2])},
                {'slot': 10, 'value': 1},
                {'slot': 11, 'value': 0},
                {'slot': 13, 'layer': str(frames[2])},
                {'slot': 14, 'value': 32},
                {'slot': 18, 'value': 40},
                {'slot': 19, 'value': 10},
                {'slot': 20, 'value': 3},
                {'slot': 21, 'value': 64},
                {'slot': 22, 'value': mix},
            ],
            'timed_layers': [
                {'slot': 13, 'time': frame, 'time_scale': 30,
                 'image': str(path)}
                for frame, path in frames.items()
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, frames[2], output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        assert report['current_time'] == 2 and report['time_scale'] == 30
        requested = {
            parameter['slot']: parameter['value']
            for parameter in report['requested_parameters']
        }
        assert requested[10] == 1 and requested[11] == 0
        assert requested[14] == 32 and requested[18] == 40
        assert requested[19] == 10 and requested[20] == 3
        assert requested[21] == 64 and requested[22] == mix
        reports[label] = report
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs[label] = image.convert('RGBA').tobytes()

    source = noisy_edge_pixels(2)
    neutral, active = outputs['neutral'], outputs['active']
    assert reports['neutral']['worker_classification'] == 'ok'
    assert reports['active']['worker_classification'] == 'ok'
    if neutral == source and active == source:
        pytest.xfail(
            'known Degrain signature: valid temporal Sample Layer checkout and '
            'decoded outputs, but Normal setup at Mix 0 remains byte-identical')

    assert_degrain_response(source, neutral, active)
    pytest.fail('Degrain output changed; validate and promote this expected failure')
