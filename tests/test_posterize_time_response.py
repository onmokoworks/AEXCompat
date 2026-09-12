"""Integer temporal quantization through explicit primary frame samples."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def frame(number):
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (x, y, number * 30, 255))


def assert_frame(actual, number):
    assert actual == frame(number)


@pytest.mark.parametrize('fault', ['current', 'future', 'spatial', 'alpha', 'truncated'])
def test_posterize_oracle_rejects_corruption(fault):
    actual = bytearray(frame(4))
    if fault == 'current':
        actual = frame(5)
    elif fault == 'future':
        actual = frame(6)
    elif fault == 'spatial':
        actual[0] = 1
    elif fault == 'alpha':
        actual[3] = 0
    else:
        actual = actual[:-4]
    with pytest.raises(AssertionError):
        assert_frame(actual, 4)


def test_real_posterize_integer_frame_hold(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_POSTERIZE_TIME')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_POSTERIZE_TIME to local BCCPosterizeTime.aex')
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'
    for number in range(4, 9):
        Image.frombytes('RGBA', (WIDTH, HEIGHT), frame(number)).save(tmp_path/f'{number}.png')
    for current, separation, selected in ((5, 1, 5), (5, 2, 4), (6, 2, 6), (7, 2, 6)):
        stem = f'{current}-{separation}'
        request, output = tmp_path/f'{stem}.json', tmp_path/f'{stem}-output.png'
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': current, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 4, 'value': separation}],
            'timed_layers': [{'slot': 0, 'time': t, 'time_scale': 30,
                             'image': str(tmp_path/f'{t}.png')}
                            for t in range(4, 9) if t != current]}), encoding='utf-8')
        run = subprocess.run([str(harness), '--headless', '--render-experimental-smart-request',
                              plugin, str(tmp_path/f'{current}.png'), str(output), str(request)],
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
        assert_frame(actual, selected)
