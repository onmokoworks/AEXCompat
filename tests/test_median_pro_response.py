"""Bounded MedianPro impulse removal, not weighted/map/AE parity."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def inputs():
    image = Image.new('RGBA', (WIDTH, HEIGHT), (83, 127, 191, 255))
    image.paste((173, 61, 107, 255), (WIDTH//2, 0, WIDTH, HEIGHT))
    clean = image.tobytes()
    for x, y, value in ((32, 32, 0), (96, 64, 255), (160, 96, 0), (224, 112, 255)):
        image.putpixel((x, y), (value, value, value, 255))
    return image.tobytes(), clean


def assert_response(pixels, original, clean, mix):
    assert mix in (0, 100)
    assert len(pixels) == len(original) == len(clean) == WIDTH*HEIGHT*4
    # Observed endpoint convention: 100 is processed, despite the UI label.
    assert pixels == (clean if mix == 100 else original)


@pytest.mark.parametrize('fault', ['bypass', 'background', 'alpha', 'truncated', 'wrong_endpoint'])
def test_median_pro_oracle_rejects_corruption(fault):
    original, clean = inputs()
    assert_response(clean, original, clean, 100)
    assert_response(original, original, clean, 0)
    pixels = bytearray(clean)
    mix = 100
    if fault == 'bypass':
        pixels = original
    elif fault == 'background':
        pixels[0] ^= 1
    elif fault == 'alpha':
        pixels[3] = 0
    elif fault == 'truncated':
        pixels = pixels[:-4]
    else:
        mix = 0
    with pytest.raises(AssertionError):
        assert_response(pixels, original, clean, mix)


def test_real_median_pro(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_MEDIAN_PRO')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_MEDIAN_PRO to local MedianPro.aex')
    assert os.name == 'nt'
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    for slot, name in ((1, 'Filter Type'), (2, 'Radius'), (3, 'Edge Preserve'),
                       (4, 'Iterations'), (5, 'Mix with Original')):
        assert params[slot]['name'] == name
    assert params[1]['choices'][0] == 'Median'
    original, clean = inputs()
    source = tmp_path/'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), original).save(source)
    for mix in (0, 100):
        request, output = tmp_path/f'{mix}.json', tmp_path/f'{mix}.png'
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': s, 'value': v} for s, v in
                            ((1, 1), (2, 1), (3, 0), (4, 1), (5, mix))]}), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output, request)
        (tmp_path/f'{mix}-report.json').write_text(json.dumps(report), encoding='utf-8')
        assert report['passed'] and report.get('output_pixels_valid') is not False
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert('RGBA').tobytes()
        argb = bytearray(len(pixels))
        argb[0::4], argb[1::4] = pixels[3::4], pixels[0::4]
        argb[2::4], argb[3::4] = pixels[1::4], pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        assert_response(pixels, original, clean, mix)
