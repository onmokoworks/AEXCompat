"""Spatial time-map selection with explicit primary samples, no AE parity claim."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def frame(time):
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (x, y, time * 30, 255))


def expected(amount, reverse):
    def time(x):
        if amount == 0:
            return 5
        later = (x >= WIDTH//2) != reverse
        if amount < 0:
            later = not later
        return 6 if later else 4
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (x, y, time(x) * 30, 255))


def assert_map(actual, amount, reverse):
    assert actual == expected(amount, reverse)


@pytest.mark.parametrize('fault', ['current', 'reversed', 'spatial', 'alpha', 'truncated'])
def test_time_map_oracle_rejects_corruption(fault):
    actual = bytearray(expected(2, False))
    if fault == 'current':
        actual = frame(5)
    elif fault == 'reversed':
        actual = expected(2, True)
    elif fault == 'spatial':
        actual[0] = 1
    elif fault == 'alpha':
        actual[3] = 0
    else:
        actual = actual[:-4]
    with pytest.raises(AssertionError):
        assert_map(actual, 2, False)


def test_real_time_displacement_map_and_sign(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_TIME_DISPLACEMENT')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_TIME_DISPLACEMENT to local BCCTimeDisplacement.aex')
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'
    for time in range(3, 8):
        Image.frombytes('RGBA', (WIDTH, HEIGHT), frame(time)).save(tmp_path/f'{time}.png')
    for reverse in (False, True):
        mask = bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                     for c in ((255,255,255,255) if (x >= WIDTH//2) != reverse else (0,0,0,255)))
        Image.frombytes('RGBA', (WIDTH, HEIGHT), mask).save(tmp_path/f'map-{reverse}.png')
    for amount, reverse in ((0, False), (2, False), (2, True), (-2, False), (-2, True)):
        stem = f'{amount}-{reverse}'
        request, output = tmp_path/f'{stem}.json', tmp_path/f'{stem}-output.png'
        assignments = [{'slot': s, 'value': v} for s, v in
            ((5,0),(7,0),(8,255),(9,1),(11,0),(12,4),(13,amount),(14,127.5),(15,0),(16,0),(17,3),(18,1),(20,1))]
        assignments.append({'slot':3, 'layer':str(tmp_path/f'map-{reverse}.png')})
        request.write_text(json.dumps({'schema_version':1,
            'timing':{'frame':5,'fps':30,'duration_frames':300}, 'assignments':assignments,
            'timed_layers':[{'slot':0,'time':t,'time_scale':30,'image':str(tmp_path/f'{t}.png')}
                           for t in (3,4,6,7)]}), encoding='utf-8')
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
        assert_map(actual, amount, reverse)
