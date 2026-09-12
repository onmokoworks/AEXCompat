"""Observed opaque Normal RGB blending, not all-mode or AE-equivalence proof."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT

MIXES = ((0, 0, 0), (100, 100, 100), (100, 0, 0), (0, 100, 0), (25, 50, 75))
WEIGHTS = {0: 0, 25: 63, 50: 127, 75: 191, 100: 256}


def inputs():
    a = bytes(c for y in range(HEIGHT) for x in range(WIDTH)
              for c in (x, (3*x+y)%256, (x+5*y)%256, 255))
    b = bytes(c for y in range(HEIGHT) for x in range(WIDTH)
              for c in ((7*x+11*y)%256, y, (13*x+3*y)%256, 255))
    return a, b


def expected(a, b, mix):
    return bytes(255 if i%4 == 3 else
                 (v*(256-WEIGHTS[mix[i%4]])+b[i]*WEIGHTS[mix[i%4]])//256
                 for i, v in enumerate(a))


def assert_blend(raw, a, b, mix):
    assert raw == expected(a, b, mix)


@pytest.mark.parametrize('fault', ['copy', 'secondary', 'swap_layers', 'swap_channels',
                                  'ideal_linear', 'alpha', 'truncated'])
def test_rgb_blend_validator_rejects_corruption(fault):
    a, b = inputs()
    mix = (25, 50, 75)
    raw = bytearray(expected(a, b, mix))
    assert_blend(raw, a, b, mix)
    if fault == 'copy': raw = a
    elif fault == 'secondary': raw = b
    elif fault == 'swap_layers': raw = expected(b, a, mix)
    elif fault == 'swap_channels': raw = expected(a, b, mix[::-1])
    elif fault == 'ideal_linear':
        raw = bytes(255 if i%4 == 3 else (v*(100-mix[i%4])+b[i]*mix[i%4])//100
                    for i,v in enumerate(a))
    elif fault == 'alpha': raw[3] = 0
    else: raw = raw[:-4]
    with pytest.raises(AssertionError): assert_blend(raw, a, b, mix)


def test_real_rgb_blend(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_RGB_BLEND')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_RGB_BLEND to local BCCRGBBlend.aex')
    assert os.name == 'nt'
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)], cwd=ROOT,
                                capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    for slot, name in ((6,'Host Layer'),(8,'Mix Layer'),(24,'Lock Mix'),
                       (25,'Red Mix'),(26,'Green Mix'),(27,'Blue Mix')):
        assert params[slot]['name'] == name
    a, b = inputs()
    source, secondary = tmp_path/'input-a.png', tmp_path/'input-b.png'
    for path, raw in ((source,a),(secondary,b)):
        Image.frombytes('RGBA', (WIDTH,HEIGHT), raw).save(path)
    for index, mix in enumerate(MIXES):
        request, output = tmp_path/f'{index}.json', tmp_path/f'{index}.png'
        assignments = [{'slot':6,'layer':str(source)}, {'slot':8,'layer':str(secondary)}, {'slot':24,'value':0}]
        assignments += [{'slot':25+c,'value':v} for c,v in enumerate(mix)]
        request.write_text(json.dumps({'schema_version':1,
            'timing':{'frame':0,'fps':30,'duration_frames':300},
            'assignments':assignments}), encoding='utf-8')
        report = run('--render-experimental-smart-request',plugin,source,output,request)
        (tmp_path/f'{index}-report.json').write_text(json.dumps(report), encoding='utf-8')
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as im:
            assert im.size == (WIDTH,HEIGHT)
            raw = im.convert('RGBA').tobytes()
        argb = bytes(c for i in range(0,len(raw),4) for c in (raw[i+3],*raw[i:i+3]))
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        assert_blend(raw, a, b, mix)
