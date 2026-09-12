"""Secondary-layer channel extraction; bounded ARGB8 evidence, not full AE parity."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT

CASES = ((2, 0, False), (3, 1, False), (4, 2, False), (9, 0, True))
COLOR = (173, 57, 219)


def secondary_pixels():
    # Independent channel patterns prevent inverse-red from aliasing green.
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (x, (x + 3*y) % 256, round(y*255/(HEIGHT-1)), 255))


def expected_alpha(sample, channel, inverse):
    return bytes(255-v if inverse else v for v in sample[channel::4])


def assert_response(pixels, sample, channel, inverse):
    assert len(pixels) == WIDTH*HEIGHT*4
    alpha = expected_alpha(sample, channel, inverse)
    assert max(abs(a-b) for a, b in zip(pixels[3::4], alpha)) <= 1
    assert all(tuple(pixels[i:i+3]) == COLOR
               for i in range(0, len(pixels), 4) if pixels[i+3])


@pytest.mark.parametrize('fault', ['opaque', 'transparent', 'wrong_channel',
                                  'inverse_ignored', 'rgb', 'truncated'])
def test_make_alpha_validator_rejects_corruption(fault):
    sample = secondary_pixels()
    alpha = expected_alpha(sample, 0, True)
    pixels = bytearray(c for a in alpha for c in (*COLOR, a))
    assert_response(pixels, sample, 0, True)
    if fault == 'opaque':
        pixels[3::4] = bytes([255])*(WIDTH*HEIGHT)
    elif fault == 'transparent':
        pixels[3::4] = bytes(WIDTH*HEIGHT)
    elif fault == 'wrong_channel':
        pixels[3::4] = sample[1::4]
    elif fault == 'inverse_ignored':
        pixels[3::4] = sample[0::4]
    elif fault == 'rgb':
        pixels[0] = 0
    else:
        pixels = pixels[:-4]
    with pytest.raises(AssertionError):
        assert_response(pixels, sample, 0, True)


def test_real_make_alpha(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_MAKE_ALPHA')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_MAKE_ALPHA to local BCCMakeAlpha.aex')
    assert os.name == 'nt'
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert params[6]['name'] == 'Host Layer'
    assert params[8]['name'] == 'Alpha Source Layer'
    assert params[9]['name'] == 'Alpha From Channel'
    source, secondary = tmp_path/'source.png', tmp_path/'secondary.png'
    Image.new('RGBA', (WIDTH, HEIGHT), (*COLOR, 255)).save(source)
    sample = secondary_pixels()
    Image.frombytes('RGBA', (WIDTH, HEIGHT), sample).save(secondary)
    for choice, channel, inverse in CASES:
        request, output = tmp_path/f'{choice}.json', tmp_path/f'{choice}.png'
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 6, 'layer': str(source)},
                            {'slot': 8, 'layer': str(secondary)},
                            {'slot': 9, 'value': choice}]}), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output, request)
        (tmp_path/f'{choice}-report.json').write_text(json.dumps(report), encoding='utf-8')
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert('RGBA').tobytes()
        argb = bytearray(len(pixels))
        argb[0::4], argb[1::4] = pixels[3::4], pixels[0::4]
        argb[2::4], argb[3::4] = pixels[1::4], pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        assert_response(pixels, sample, channel, inverse)
