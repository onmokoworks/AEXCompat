"""Legacy Luma Key hard-threshold behavior on opaque gray, not full AE parity."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT

CASES = (('darker100', 2, 100), ('brighter100', 1, 100), ('darker160', 2, 160))


def source_pixels():
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH) for c in (x, x, x, 255))


def assert_key(pixels, kind, threshold):
    source = source_pixels()
    assert len(pixels) == len(source)
    for y in range(HEIGHT):
        for x in range(WIDTH):
            i = (y*WIDTH+x)*4
            opaque = x >= threshold if kind == 2 else x < threshold
            assert pixels[i+3] == (255 if opaque else 0)
            if opaque:
                assert pixels[i:i+3] == source[i:i+3]
    # RGB under zero alpha is intentionally unconstrained.


@pytest.mark.parametrize('fault', ['passthrough', 'empty', 'inverted', 'threshold',
                                  'boundary', 'rgb', 'row', 'truncated'])
def test_key_validator_rejects_corruption(fault):
    pixels = bytearray(source_pixels())
    pixels[3::4] = bytes(255 if x >= 100 else 0 for y in range(HEIGHT) for x in range(WIDTH))
    assert_key(pixels, 2, 100)
    if fault == 'passthrough':
        pixels = source_pixels()
    elif fault == 'empty':
        pixels[3::4] = bytes(WIDTH*HEIGHT)
    elif fault == 'inverted':
        pixels[3::4] = bytes(255-v for v in pixels[3::4])
    elif fault == 'threshold':
        pixels[3::4] = bytes(255 if x >= 160 else 0 for y in range(HEIGHT) for x in range(WIDTH))
    elif fault == 'boundary':
        pixels[100*4+3] = 0
    elif fault == 'rgb':
        pixels[150*4] += 1
    elif fault == 'row':
        pixels[WIDTH*4+200*4+3] = 0
    else:
        pixels = pixels[:-4]
    with pytest.raises(AssertionError):
        assert_key(pixels, 2, 100)


def test_real_obsolete_luma(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_OBSOLETE_LUMA')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_OBSOLETE_LUMA to the local legacy AEX')
    assert os.name == 'nt'
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)], cwd=ROOT,
                                capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert params[6]['name'] == 'Host Layer' and params[6]['kind'] == 'layer'
    assert params[10]['choices'][:2] == ['Key Out Brighter', 'Key Out Darker']
    assert params[11]['name'] == 'Threshold' and params[13]['name'] == 'Softness'
    assert params[19]['choices'][4] == 'All'
    source = tmp_path/'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    for name, kind, threshold in CASES:
        request, output = tmp_path/f'{name}.json', tmp_path/f'{name}.png'
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 6, 'layer': str(source)}]+[
                {'slot': s, 'value': v} for s, v in ((10, kind), (11, threshold), (13, 0), (19, 5))]}),
            encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output, request)
        (tmp_path/f'{name}-report.json').write_text(json.dumps(report), encoding='utf-8')
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert('RGBA').tobytes()
        argb = bytearray(len(pixels))
        argb[0::4], argb[1::4] = pixels[3::4], pixels[0::4]
        argb[2::4], argb[3::4] = pixels[1::4], pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        assert_key(pixels, kind, threshold)
