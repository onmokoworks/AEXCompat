"""Composite CPU graded matte response; not default-GPU or full AE parity."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT

COLOR = (173, 57, 219)
CASES = ((1, 3, False), (2, 0, False), (3, 1, False), (2, 0, True))


def matte_pixels():
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (x, (x+3*y) % 256, round(y*255/(HEIGHT-1)), (3*x+y) % 256))


def assert_matte_response(pixels, matte, channel, inverse):
    assert len(pixels) == len(matte) == WIDTH*HEIGHT*4
    expected = bytes(255-v if inverse else v for v in matte[channel::4])
    assert pixels[3::4] == expected
    assert all(tuple(pixels[i:i+3]) == COLOR
               for i in range(0, len(pixels), 4) if pixels[i+3])


@pytest.mark.parametrize('fault', ['opaque', 'empty', 'channel', 'inverse', 'rgb', 'truncated'])
def test_composite_matte_oracle_rejects_corruption(fault):
    matte = matte_pixels()
    pixels = bytearray(c for v in matte[0::4] for c in (*COLOR, 255-v))
    assert_matte_response(pixels, matte, 0, True)
    if fault == 'opaque':
        pixels[3::4] = bytes([255])*(WIDTH*HEIGHT)
    elif fault == 'empty':
        pixels[3::4] = bytes(WIDTH*HEIGHT)
    elif fault == 'channel':
        pixels[3::4] = bytes(255-v for v in matte[1::4])
    elif fault == 'inverse':
        pixels[3::4] = matte[0::4]
    elif fault == 'rgb':
        pixels[0] -= 1
    else:
        pixels = pixels[:-4]
    with pytest.raises(AssertionError):
        assert_matte_response(pixels, matte, 0, True)


def test_real_composite_cpu_matte(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_COMPOSITE')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_COMPOSITE to local Composite.aex')
    assert os.name == 'nt'
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert params[181]['name'] == 'Host Layer'
    assert params[329]['kind'] == 'layer'
    assert params[330]['choices'][:4] == ['Alpha Channel', 'Red Channel', 'Green Channel', 'Blue Channel']
    assert params[331]['name'] == 'Invert'
    assert params[402]['name'] == 'GPU Rendering'
    assert params[402]['choices'][3] == 'Disabled'
    source, secondary = tmp_path/'source.png', tmp_path/'matte.png'
    Image.new('RGBA', (WIDTH, HEIGHT), (*COLOR, 255)).save(source)
    matte = matte_pixels()
    Image.frombytes('RGBA', (WIDTH, HEIGHT), matte).save(secondary)
    for index, (choice, channel, inverse) in enumerate(CASES):
        request, output = tmp_path/f'{index}.json', tmp_path/f'{index}.png'
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 181, 'layer': str(source)},
                            {'slot': 329, 'layer': str(secondary)},
                            {'slot': 330, 'value': choice},
                            {'slot': 331, 'value': int(inverse)},
                            {'slot': 402, 'value': 4}]}), encoding='utf-8')
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
        assert_matte_response(pixels, matte, channel, inverse)
