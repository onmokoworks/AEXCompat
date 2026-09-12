"""Velocity Remap integer-time selection; no interpolation or AE-parity claim."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def frame_pixels(time):
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (x, y, time * 25, 255))


def assert_frame(actual, time):
    assert actual == frame_pixels(time)


@pytest.mark.parametrize('fault', ['current', 'wrong-time', 'rgb', 'alpha', 'truncated'])
def test_velocity_oracle_rejects_corruption(fault):
    actual = bytearray(frame_pixels(2))
    if fault == 'current':
        actual = frame_pixels(4)
    elif fault == 'wrong-time':
        actual = frame_pixels(0)
    elif fault == 'rgb':
        actual[(40 * WIDTH + 16) * 4] ^= 1
    elif fault == 'alpha':
        actual[(40 * WIDTH + 16) * 4 + 3] = 0
    else:
        actual = actual[:-4]
    with pytest.raises(AssertionError):
        assert_frame(actual, 2)


@pytest.mark.parametrize('binding', ['default-self', 'explicit'])
def test_real_velocity_integer_speed_and_start(tmp_path, binding):
    plugin = os.environ.get('AEXCOMPAT_TEST_VELOCITY_REMAP')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_VELOCITY_REMAP to local BCCVelocityRemap.aex')
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'
    for time in (0, 2, 4, 6, 8):
        Image.frombytes('RGBA', (WIDTH, HEIGHT), frame_pixels(time)).save(tmp_path / f'{time}.png')
    for velocity, start, expected in ((0, 0, 0), (50, 0, 2), (100, 0, 4), (200, 0, 8), (100, 2, 6)):
        name = f'{velocity}-{start}'
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        assignments = [{'slot': s, 'value': v} for s, v in ((3, velocity), (4, start), (5, 0), (11, 1))]
        if binding == 'explicit':
            assignments.append({'slot': 2, 'layer': str(tmp_path / '4.png')})
        sample_slot = 2 if binding == 'explicit' else 0
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 4, 'fps': 30, 'duration_frames': 300}, 'assignments': assignments,
            'timed_layers': [{'slot': sample_slot, 'time': t, 'time_scale': 30,
                             'image': str(tmp_path / f'{t}.png')} for t in (0, 2, 6, 8)]}), encoding='utf-8')
        run = subprocess.run([str(harness), '--headless', '--render-experimental-smart-request',
                              plugin, str(tmp_path / '4.png'), str(output), str(request)],
                             cwd=ROOT, capture_output=True, timeout=90)
        (tmp_path / f'{name}-report.json').write_bytes(run.stdout)
        (tmp_path / f'{name}-stderr.log').write_bytes(run.stderr)
        assert run.returncode == 0, run.stderr.decode('utf-8', errors='replace')
        report = json.loads(run.stdout)
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            actual = image.convert('RGBA').tobytes()
        argb = bytes(c for i in range(0, len(actual), 4) for c in (actual[i+3], *actual[i:i+3]))
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        assert_frame(actual, expected)
