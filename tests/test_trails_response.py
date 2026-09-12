"""BCC Trails explicit Input Layer history, bounded disjoint-patch oracle."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def patches(times):
    def pixel(x, y):
        for time in times:
            if 16+(time-3)*80 <= x < 48+(time-3)*80 and 40 <= y < 104:
                return x, y, time*30, 255
        return 0, 0, 0, 0
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH) for c in pixel(x, y))


def assert_trails(actual, count, mode):
    times = list(range(5-count, 5)) + ([5] if mode == 1 else [])
    assert actual == patches(times)


@pytest.mark.parametrize('fault', ['current', 'missing', 'source', 'rgb', 'alpha', 'truncated'])
def test_trails_oracle_rejects_corruption(fault):
    actual = bytearray(patches([3, 4]))
    if fault == 'current':
        actual = patches([5])
    elif fault == 'missing':
        actual = patches([4])
    elif fault == 'source':
        actual = patches([3, 4, 5])
    elif fault == 'rgb':
        actual[(40*WIDTH+16)*4] ^= 1
    elif fault == 'alpha':
        actual[(40*WIDTH+16)*4+3] = 0
    else:
        actual = actual[:-4]
    with pytest.raises(AssertionError):
        assert_trails(actual, 2, 3)


def test_real_trails_count_and_source_exclusion(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_TRAILS')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_TRAILS to local BCCTrails.aex')
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'
    for time in (3, 4, 5):
        Image.frombytes('RGBA', (WIDTH, HEIGHT), patches([time])).save(tmp_path/f'{time}.png')
    for count, mode in ((0, 1), (1, 1), (2, 1), (1, 3), (2, 3)):
        stem = f'{count}-{mode}'
        request, output = tmp_path/f'{stem}.json', tmp_path/f'{stem}-output.png'
        assignments = [{'slot': s, 'value': v} for s, v in
            ((17,1),(18,count),(19,1),(20,100),(21,0),(22,100),(23,1),(26,100),
             (37,100),(117,1),(118,100),(120,1),(121,mode),(123,100))]
        assignments.append({'slot': 2, 'layer': str(tmp_path/'5.png')})
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 5, 'fps': 30, 'duration_frames': 300}, 'assignments': assignments,
            'timed_layers': [{'slot': 2, 'time': t, 'time_scale': 30, 'image': str(tmp_path/f'{t}.png')}
                             for t in (3, 4)]}), encoding='utf-8')
        run = subprocess.run([str(harness),'--headless','--render-experimental-smart-request',
                              plugin,str(tmp_path/'5.png'),str(output),str(request)],
                             cwd=ROOT,capture_output=True,timeout=90)
        (tmp_path/f'{stem}-report.json').write_bytes(run.stdout)
        assert run.returncode == 0, run.stderr.decode('utf-8', errors='replace')
        report = json.loads(run.stdout)
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            actual = image.convert('RGBA').tobytes()
        argb = bytes(c for i in range(0,len(actual),4) for c in (actual[i+3],*actual[i:i+3]))
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        assert_trails(actual, count, mode)
