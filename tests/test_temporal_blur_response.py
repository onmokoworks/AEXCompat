"""Flat two-frame temporal means, independent of Gaussian/general blur behavior."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def pixels(blue):
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (x, y, blue, 255))


def assert_mean(actual, blue):
    assert actual == pixels(blue)


@pytest.mark.parametrize('fault', ['current_copy', 'neighbor_copy', 'direction', 'spatial', 'alpha', 'truncated'])
def test_temporal_mean_rejects_corruption(fault):
    actual = bytearray(pixels(70))
    if fault == 'current_copy':
        actual = pixels(100)
    elif fault == 'neighbor_copy':
        actual = pixels(40)
    elif fault == 'direction':
        actual = pixels(160)
    elif fault == 'spatial':
        actual[0] = 1
    elif fault == 'alpha':
        actual[3] = 0
    else:
        actual = actual[:-4]
    with pytest.raises(AssertionError):
        assert_mean(actual, 70)


def test_real_temporal_blur_flat_means(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_TEMPORAL_BLUR')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_TEMPORAL_BLUR to local BCCTemporalBlur.aex')
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'
    for time, blue in ((3, 10), (4, 40), (5, 100), (6, 220), (7, 250)):
        Image.frombytes('RGBA', (WIDTH, HEIGHT), pixels(blue)).save(tmp_path/f'{time}.png')
    for amount, direction, blue in ((0, 3, 100), (1, 3, 100), (2, 3, 70), (2, 2, 160)):
        stem = f'{amount}-{direction}'
        request, output = tmp_path/f'{stem}.json', tmp_path/f'{stem}-output.png'
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 5, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': s, 'value': v} for s, v in
                ((3, amount), (4, 1), (5, 1), (6, 0), (7, direction), (8, 2), (9, 0),
                 (10, 0), (11, 0), (12, 1), (16, 100), (18, 0))],
            'timed_layers': [{'slot': 0, 'time': t, 'time_scale': 30,
                             'image': str(tmp_path/f'{t}.png')} for t in (3, 4, 6, 7)]}), encoding='utf-8')
        run = subprocess.run([str(harness), '--headless', '--render-experimental-smart-request',
                              plugin, str(tmp_path/'5.png'), str(output), str(request)],
                             cwd=ROOT, capture_output=True, timeout=90)
        (tmp_path/f'{stem}-report.json').write_bytes(run.stdout)
        assert run.returncode == 0, run.stderr.decode('utf-8', errors='replace')
        report = json.loads(run.stdout)
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            actual = image.convert('RGBA').tobytes()
        argb = bytes(c for i in range(0, len(actual), 4)
                     for c in (actual[i+3], *actual[i:i+3]))
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        assert_mean(actual, blue)
