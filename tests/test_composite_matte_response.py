"""Opt-in Composite matte response, not AE equivalence or full effect coverage."""
import copy
import json
import os
import subprocess

import pytest
from PIL import Image

from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT, argb, read_artifact


def assert_matte_response(raw, source, alpha):
    assert len(raw) == len(source) == WIDTH * HEIGHT * 4
    assert len(alpha) == WIDTH * HEIGHT
    assert raw[0::4] == alpha
    for i, a in enumerate(alpha):
        if a:
            assert raw[i*4+1:i*4+4] == source[i*4+1:i*4+4]


@pytest.mark.parametrize('fault', ['empty', 'opaque', 'shifted', 'rgb', 'truncated'])
def test_matte_validator_rejects_corruption(fault):
    source = bytes(c for y in range(HEIGHT) for x in range(WIDTH) for c in (255, x, x, x))
    alpha = bytes(255 if (x//32+y//24)%2 else 0 for y in range(HEIGHT) for x in range(WIDTH))
    raw = bytearray(source)
    raw[0::4] = alpha
    assert_matte_response(raw, source, alpha)
    if fault == 'empty': raw[0::4] = bytes(len(alpha))
    elif fault == 'opaque': raw[0::4] = bytes([255])*len(alpha)
    elif fault == 'shifted': raw[0::4] = alpha[1:]+alpha[:1]
    elif fault == 'rgb': raw[32*4+1] ^= 1
    else: raw = raw[:-4]
    with pytest.raises(AssertionError):
        assert_matte_response(raw, source, alpha)


def test_installed_composite_uses_spatial_matte(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_COMPOSITE')
    if not plugin:
        pytest.skip('real AEX requires AEXCOMPAT_TEST_COMPOSITE')
    assert os.name == 'nt'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        r = subprocess.run([str(harness), '--headless', *map(str, args)], cwd=ROOT, capture_output=True)
        assert r.returncode == 0, r.stderr.decode('utf-8', errors='replace')
        return json.loads(r.stdout)

    parameters = run('--inspect-experimental', plugin)
    parameters = [p for p in parameters if not (p['kind']=='arbitrary_data' and not p.get('debug_summary'))]
    source = bytes(c for y in range(HEIGHT) for x in range(WIDTH) for c in (x, x, x, 255))
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source).save(tmp_path/'source.png')
    for masked in (False, True):
        alpha = bytes(255 if not masked or (x//32+y//24)%2 else 0 for y in range(HEIGHT) for x in range(WIDTH))
        matte = Image.frombytes('RGBA', (WIDTH, HEIGHT), source)
        matte.putalpha(Image.frombytes('L', (WIDTH, HEIGHT), alpha))
        matte.save(tmp_path/'matte.png')
        edited = copy.deepcopy(parameters)
        for slot, name, path in [(181, 'Host Layer', 'source.png'), (329, 'Input', 'matte.png')]:
            item = next(p for p in edited if p['slot']==slot)
            assert item['kind']=='layer' and item['name']==name
            item['layer_path'] = path
        fixture = dict(schema='aexcompat.render_fixture', schema_version=1,
            primary_layer='source.png', parameters=edited, pixel_format='argb8',
            render_path='smart', premultiplication='straight',
            timing=dict(current_time=0,time_step=1,total_time=300,time_scale=30),
            final_artifact='raw', checkpoints=[dict(id='input',stage='smart-input'),dict(id='output',stage='smart-output')])
        path = tmp_path/f'{masked}.json'
        path.write_text(json.dumps(fixture), encoding='utf-8')
        dest = tmp_path/str(masked)
        report = run('--render-fixture', plugin, path, dest)
        assert read_artifact(dest/'checkpoints/input',report['checkpoints']['input']) == argb(source)
        raw = read_artifact(dest/'final',report['final_artifact'])
        assert raw == read_artifact(dest/'checkpoints/output',report['checkpoints']['output'])
        assert_matte_response(raw, argb(source), alpha)
