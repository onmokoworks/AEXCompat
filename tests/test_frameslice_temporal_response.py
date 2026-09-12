"""Two-frame nearest temporal slicing through shipping timed-primary requests."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def frame(past):
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (x, y, 71 if past else 193, 255))


def expected(mode):
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (x, y, 71 if (x < WIDTH//2 if mode == 1 else y < HEIGHT//2) else 193, 255))


@pytest.mark.parametrize('fault', ['current', 'past', 'axis', 'alpha', 'truncated'])
def test_frameslice_oracle_rejects_corruption(fault):
    wanted = expected(1)
    actual = bytearray(wanted)
    if fault == 'current':
        actual = frame(False)
    elif fault == 'past':
        actual = frame(True)
    elif fault == 'axis':
        actual = expected(2)
    elif fault == 'alpha':
        actual[3] = 0
    else:
        actual = actual[:-4]
    assert actual != wanted


def test_real_frameslice_temporal_bands(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_FRAMESLICE')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_FRAMESLICE to local FrameSlice.aex')
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'
    source, past = tmp_path/'current.png', tmp_path/'past.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), frame(False)).save(source)
    Image.frombytes('RGBA', (WIDTH, HEIGHT), frame(True)).save(past)
    for mode in (1, 2):
        request, output = tmp_path/f'{mode}.json', tmp_path/f'{mode}.png'
        request.write_text(json.dumps({'schema_version':1,
            'timing':{'frame':2,'fps':30,'duration_frames':300},
            'assignments':[{'slot':s,'value':v} for s,v in
                           ((1,2),(2,1),(3,1),(4,mode),(5,0),(7,1),(8,100))],
            'timed_layers':[{'slot':0,'time':1,'time_scale':30,'image':str(past)}]}), encoding='utf-8')
        p = subprocess.run([str(harness),'--headless','--render-experimental-smart-request',
                            plugin,str(source),str(output),str(request)], cwd=ROOT,
                           capture_output=True,timeout=90)
        (tmp_path/f'{mode}-report.json').write_bytes(p.stdout)
        assert p.returncode == 0, p.stderr.decode('utf-8',errors='replace')
        report = json.loads(p.stdout)
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert('RGBA').tobytes()
        argb = bytes(c for i in range(0,len(pixels),4)
                     for c in (pixels[i+3],*pixels[i:i+3]))
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        assert pixels == expected(mode)
