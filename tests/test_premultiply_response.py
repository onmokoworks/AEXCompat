"""Observed ARGB8 premultiply arithmetic, not independent AE-equivalence evidence."""
import hashlib
import json
import os
import subprocess
import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT

CASES = (((64, 96, 128), (200, 40, 80)), ((173, 57, 219), (31, 203, 91)))


def source_pixels(color):
    return bytes(c for y in range(HEIGHT) for a in range(WIDTH) for c in (*color, a))


def expected(color, matte):
    return bytes(c for y in range(HEIGHT) for a in range(WIDTH)
                 for c in (*[0 if a == 0 else color[k] if a == 255 else
                             (color[k]*a+matte[k]*(256-a))//256 for k in range(3)], a))


def assert_premultiply(pixels, color, matte):
    assert pixels == expected(color, matte)


@pytest.mark.parametrize('fault', ['copy', 'double', 'alpha', 'zero', 'opaque',
                                  'swap', 'denominator255', 'truncated'])
def test_premultiply_validator_rejects_corruption(fault):
    color, matte = CASES[0]
    pixels = bytearray(expected(color, matte))
    assert_premultiply(pixels, color, matte)
    if fault == 'copy':
        pixels = source_pixels(color)
    elif fault == 'double':
        for i in range(0, len(pixels), 4):
            for c in range(3):
                pixels[i+c] = pixels[i+c]*pixels[i+3]//255
    elif fault == 'alpha':
        pixels[3::4] = bytes([255])*(WIDTH*HEIGHT)
    elif fault == 'zero':
        pixels[:3] = bytes(matte)
    elif fault == 'opaque':
        pixels[255*4] -= 1
    elif fault == 'swap':
        pixels = expected(color, matte[::-1])
    elif fault == 'denominator255':
        pixels = bytes(c for y in range(HEIGHT) for a in range(WIDTH)
                       for c in (*[0 if a == 0 else
                                   (color[k]*a+matte[k]*(255-a))//255 for k in range(3)], a))
    else:
        pixels = pixels[:-4]
    with pytest.raises(AssertionError):
        assert_premultiply(pixels, color, matte)


def test_real_premultiply(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_PREMULTIPLY')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_PREMULTIPLY to local BCCPremultiply.aex')
    assert os.name == 'nt'
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)], cwd=ROOT,
                                capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert params[2]['name'] == 'Premult with Color' and params[2]['kind'] == 'color'
    for index, (color, matte) in enumerate(CASES):
        source, request, output = [tmp_path/f'{index}-{name}' for name in ('source.png', 'request.json', 'output.png')]
        Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels(color)).save(source)
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 2, 'color': [255, *matte]}]}), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output, request)
        (tmp_path/f'{index}-report.json').write_text(json.dumps(report), encoding='utf-8')
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert('RGBA').tobytes()
        argb = bytearray(len(pixels))
        argb[0::4], argb[1::4] = pixels[3::4], pixels[0::4]
        argb[2::4], argb[3::4] = pixels[1::4], pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        assert_premultiply(pixels, color, matte)
