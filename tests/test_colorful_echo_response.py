"""Exact disjoint temporal echoes; no claim about overlapping blend semantics."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def patch(index):
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in ((x, y, 71 + index * 40, 255)
                           if 16 + index * 80 <= x < 48 + index * 80 and 40 <= y < 104
                           else (0, 0, 0, 0)))


def expected(source_opacity):
    result = bytearray(WIDTH * HEIGHT * 4)
    for index in ((1, 2) if source_opacity == 0 else (0, 1, 2)):
        pixels = patch(index)
        for offset in range(0, len(pixels), 4):
            if pixels[offset + 3]:
                result[offset:offset + 4] = pixels[offset:offset + 4]
    return bytes(result)


def assert_pixels(actual, opacity):
    assert actual == expected(opacity)


@pytest.mark.parametrize('fault', ['missing_echo', 'current_leak', 'spatial', 'alpha', 'truncated'])
def test_echo_oracle_rejects_corruption(fault):
    actual = bytearray(expected(0))
    if fault == 'missing_echo':
        actual = patch(1)
    elif fault == 'current_leak':
        actual = expected(100)
    elif fault == 'spatial':
        offset = (40 * WIDTH + 96) * 4
        actual[offset] ^= 1
    elif fault == 'alpha':
        actual[(40 * WIDTH + 96) * 4 + 3] = 0
    else:
        actual = actual[:-4]
    with pytest.raises(AssertionError):
        assert_pixels(actual, 0)


def test_real_colorful_echo_two_history_frames(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_COLORFUL_ECHO')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_COLORFUL_ECHO to local ONMK_ColorfulEcho.aex')
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'
    for index in range(3):
        Image.frombytes('RGBA', (WIDTH, HEIGHT), patch(index)).save(tmp_path/f'{index}.png')
    for opacity in (0, 100):
        request, output = tmp_path/f'{opacity}.json', tmp_path/f'{opacity}-output.png'
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 60, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': s, 'value': v} for s, v in
                            ((1, 2), (2, 1), (3, 100), (6, 0), (7, 1), (8, 2), (9, opacity))],
            'timed_layers': [{'slot': 0, 'time': 60-i, 'time_scale': 30,
                             'image': str(tmp_path/f'{i}.png')} for i in (1, 2)]}), encoding='utf-8')
        run = subprocess.run([str(harness), '--headless', '--render-experimental-smart-request',
                              plugin, str(tmp_path/'0.png'), str(output), str(request)],
                             cwd=ROOT, capture_output=True, timeout=90)
        (tmp_path/f'{opacity}-report.json').write_bytes(run.stdout)
        assert run.returncode == 0, run.stderr.decode('utf-8', errors='replace')
        report = json.loads(run.stdout)
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            actual = image.convert('RGBA').tobytes()
        argb = bytes(c for i in range(0, len(actual), 4)
                     for c in (actual[i+3], *actual[i:i+3]))
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        assert_pixels(actual, opacity)
