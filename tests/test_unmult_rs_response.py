"""Black-key normalization and white-key endpoints, not full white-key/AE parity."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


PALETTE = ((0, 0, 0), (255, 255, 255), (128, 128, 128), (255, 0, 0),
           (0, 255, 0), (0, 0, 255), (64, 128, 192), (192, 128, 64))


def source_pixels():
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (*PALETTE[(x//32+y//18) % len(PALETTE)], 255))


def black_key(source):
    assert len(source) == WIDTH*HEIGHT*4
    result = bytearray()
    for i in range(0, len(source), 4):
        rgb = source[i:i+3]
        assert source[i+3] == 255  # This oracle does not cover incoming alpha.
        alpha = max(rgb)
        result.extend((*(round(c*255/alpha) if alpha else 0 for c in rgb), alpha))
    return bytes(result)


@pytest.mark.parametrize('fault', ['bypass', 'alpha', 'color', 'swap', 'truncated'])
def test_unmult_oracle_rejects_corruption(fault):
    source = source_pixels()
    expected = black_key(source)
    actual = bytearray(expected)
    if fault == 'bypass':
        actual = source
    elif fault == 'alpha':
        actual[3::4] = bytes([255])*(WIDTH*HEIGHT)
    elif fault == 'color':
        actual[64*4] = 128
    elif fault == 'swap':
        actual[0::4], actual[2::4] = actual[2::4], actual[0::4]
    else:
        actual = actual[:-4]
    assert actual != expected


def test_real_unmult_black_key_and_white_endpoints(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_UNMULT_RS')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_UNMULT_RS to local unmult-rs.aex')
    assert os.name == 'nt'
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        p = subprocess.run([str(harness), '--headless', *map(str, args)], cwd=ROOT,
                           capture_output=True, timeout=None if args[0] == '--inspect-experimental' else 90)
        assert p.returncode == 0, p.stderr.decode('utf-8', errors='replace')
        return json.loads(p.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert [params[s]['name'] for s in range(1, 5)] == [
        'White Key', 'Alpha Gamma', 'Binary Mask', 'Threshold']
    source = source_pixels()
    input_path = tmp_path/'input.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source).save(input_path)
    for white in (0, 1):
        request, output = tmp_path/f'{white}.json', tmp_path/f'{white}.png'
        request.write_text(json.dumps({'schema_version':1,
            'timing':{'frame':0, 'fps':30, 'duration_frames':300},
            'assignments':[{'slot':s, 'value':v} for s, v in
                           ((1,white), (2,1), (3,0), (4,0))]}), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, input_path, output, request)
        (tmp_path/f'{white}-report.json').write_text(json.dumps(report), encoding='utf-8')
        assert report['passed'] and report.get('output_pixels_valid') is not False
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert('RGBA').tobytes()
        argb = bytearray(len(pixels))
        argb[0::4], argb[1::4] = pixels[3::4], pixels[0::4]
        argb[2::4], argb[3::4] = pixels[1::4], pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        if not white:
            assert pixels == black_key(source)
        else:
            # Mixed white-key colors remain unresolved; don't freeze anomalous
            # primary-color transparency as the intended behavior.
            for i in range(0, len(source), 4):
                if source[i:i+3] == bytes(3):
                    assert pixels[i:i+4] == bytes((0, 0, 0, 255))
                elif source[i:i+3] == bytes((255, 255, 255)):
                    assert pixels[i+3] == 0
