"""Bounded timesmear relationships, not a temporal blend or AE-parity oracle."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def source_pixels(time=4):
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in ((x+30*(4-time)) % 256, (y+20*(4-time)) % 256, time*25, 255))


def split_pixels(current, effect):
    assert len(current) == len(effect) == WIDTH * HEIGHT * 4
    return b''.join(current[y*WIDTH*4:(y*WIDTH+WIDTH//2)*4] +
                    effect[(y*WIDTH+WIDTH//2)*4:(y+1)*WIDTH*4] for y in range(HEIGHT))


def assert_split(actual, current, effect):
    for left, right in ((0, WIDTH//2), (WIDTH//2, WIDTH)):
        assert any(current[(y*WIDTH+x)*4:(y*WIDTH+x+1)*4] !=
                   effect[(y*WIDTH+x)*4:(y*WIDTH+x+1)*4]
                   for y in range(HEIGHT) for x in range(left, right))
    assert actual == split_pixels(current, effect)


@pytest.mark.parametrize('fault', ['current', 'effect', 'reversed', 'boundary', 'alpha', 'truncated'])
def test_timesmear_map_oracle_rejects_corruption(fault):
    current, effect = source_pixels(4), source_pixels(1)
    actual = bytearray(split_pixels(current, effect))
    if fault == 'current':
        actual = current
    elif fault == 'effect':
        actual = effect
    elif fault == 'reversed':
        actual = split_pixels(effect, current)
    elif fault == 'boundary':
        actual[(40*WIDTH+WIDTH//2)*4] ^= 1
    elif fault == 'alpha':
        actual[(40*WIDTH+16)*4+3] = 0
    else:
        actual = actual[:-4]
    with pytest.raises(AssertionError):
        assert_split(actual, current, effect)


def test_real_timesmear_history_bypass_and_map(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_TIMESMEAR')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_TIMESMEAR to local D_timesmear.aex')
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'
    current = source_pixels()
    for t in range(5):
        Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels(t)).save(tmp_path/f'{t}.png')
    Image.new('RGBA', (WIDTH, HEIGHT), (255,255,255,255)).save(tmp_path/'white.png')
    mask = bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in ((0,0,0,255) if x < WIDTH//2 else (255,255,255,255)))
    Image.frombytes('RGBA', (WIDTH, HEIGHT), mask).save(tmp_path/'split.png')
    outputs = {}
    for name, same, amount, mix, mask_name in (
        ('same',True,100,0,None), ('full',False,100,0,None),
        ('amount-zero',False,0,0,None), ('original-full',False,100,100,None),
        ('white',False,100,0,'white'), ('split',False,100,0,'split')):
        values = ((1,amount),(2,2),(3,3),(4,1),(5,360),(6,0),(8,mix),(17,1),
                  (18,0),(21,100 if mask_name else 0),(22,0),(23,0))
        assignments = [{'slot':s,'value':v} for s,v in values]
        if mask_name:
            assignments.append({'slot':20,'layer':str(tmp_path/f'{mask_name}.png')})
        request, output = tmp_path/f'{name}.json', tmp_path/f'{name}-output.png'
        request.write_text(json.dumps({'schema_version':1,
            'timing':{'frame':4,'fps':30,'duration_frames':300},'assignments':assignments,
            'timed_layers':[{'slot':0,'time':t,'time_scale':30,
                            'image':str(tmp_path/f'{4 if same else t}.png')} for t in range(4)]}), encoding='utf-8')
        env = dict(os.environ)
        env.pop('AEXCOMPAT_EXTENDED_DIAG', None)
        run = subprocess.run([str(harness),'--headless','--render-experimental-smart-request',
                              plugin,str(tmp_path/'4.png'),str(output),str(request)],
                             cwd=ROOT,env=env,capture_output=True,timeout=90)
        (tmp_path/f'{name}-report.json').write_bytes(run.stdout)
        assert run.returncode == 0, run.stderr.decode('utf-8', errors='replace')
        report = json.loads(run.stdout)
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as im:
            assert im.size == (WIDTH, HEIGHT)
            actual = im.convert('RGBA').tobytes()
        argb = bytes(c for i in range(0,len(actual),4) for c in (actual[i+3],*actual[i:i+3]))
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        assert any(actual[3::4])
        outputs[name] = actual
    assert outputs['full'] != outputs['same']
    assert outputs['amount-zero'] == outputs['original-full'] == current
    assert outputs['white'] == outputs['full']
    assert_split(outputs['split'], current, outputs['full'])
