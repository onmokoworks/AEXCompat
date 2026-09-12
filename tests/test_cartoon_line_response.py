"""Conditional CartoonLight line-width response; not AE equivalence."""
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def contour(raw):
    assert len(raw) == WIDTH * HEIGHT * 4
    alpha = raw[3::4]
    row = alpha[72*WIDTH:73*WIDTH]
    edges = [x for x in range(1, WIDTH) if bool(row[x]) != bool(row[x-1])]
    assert len(edges) == 2
    assert 60 <= edges[0] <= 80 and 176 <= edges[1] <= 196
    assert row[0] and row[-1] and not row[128]
    assert any(raw[i] for i in range(0, len(raw), 4) if raw[i+3])
    return edges, sum(a > 0 for a in alpha)


def assert_line_response(thin, thick):
    a, va = contour(thin)
    b, vb = contour(thick)
    assert b[0] > a[0] and b[1] < a[1]
    assert vb > va


def synthetic(left, right):
    raw = bytearray(bytes((200, 20, 0, 255)) * WIDTH * HEIGHT)
    for y in range(36, 108):
        for x in range(left, right):
            raw[(y*WIDTH+x)*4+3] = 0
    return raw


@pytest.mark.parametrize('fault', ['fixed', 'reversed', 'empty', 'opaque', 'truncated'])
def test_line_validator_rejects_corruption(fault):
    thin, thick = synthetic(66, 189), synthetic(70, 185)
    assert_line_response(thin, thick)
    if fault == 'fixed': thick = thin
    elif fault == 'reversed': thin, thick = thick, thin
    elif fault == 'empty': thick[3::4] = bytes(WIDTH*HEIGHT)
    elif fault == 'opaque': thick[3::4] = bytes([255])*WIDTH*HEIGHT
    else: thick = thick[:-4]
    with pytest.raises(AssertionError):
        assert_line_response(thin, thick)


def test_installed_cartoon_light_responds_to_line_width(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_CARTOON_LIGHT')
    if not plugin:
        pytest.skip('requires AEXCOMPAT_TEST_CARTOON_LIGHT')
    assert os.name == 'nt'
    source = bytes(c for y in range(HEIGHT) for x in range(WIDTH) for c in
        ((255,255,255,255) if 64 <= x < 192 and 36 <= y < 108 else (0,0,0,255)))
    src = tmp_path/'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source).save(src)
    outputs = []
    for value in (20, 80):
        request = tmp_path/f'request-{value}.json'
        request.write_text(json.dumps({'schema_version':1,
            'timing':{'frame':0,'fps':30,'duration_frames':300},
            'assignments':[{'slot':3,'value':value}]}), encoding='utf-8')
        out = tmp_path/f'output-{value}.png'
        run = subprocess.run([str(ROOT/'broker/target/release/aexcompat-harness.exe'),
            '--headless','--render-experimental-smart-request',plugin,str(src),str(out),str(request)],
            cwd=ROOT, capture_output=True, timeout=90)
        assert run.returncode == 0, run.stderr.decode('utf-8', errors='replace')
        report = json.loads(run.stdout)
        assert report['passed'] and report['output_pixels_valid']
        assert next(p for p in report['requested_parameters'] if p['slot']==3)['value'] == value
        image = Image.open(out).convert('RGBA')
        assert image.size == (WIDTH, HEIGHT)
        pixels = image.tobytes()
        assert pixels != source
        outputs.append(pixels)
    assert_line_response(*outputs)
