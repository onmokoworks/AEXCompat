"""Explicit white-map horizontal morphology, not null-map or AE parity."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def source_pixels():
    image = Image.new('RGBA', (WIDTH, HEIGHT), (83, 127, 191, 255))
    image.paste((173, 61, 107, 255), (WIDTH//2, 0, WIDTH, HEIGHT))
    image.putpixel((64, 48), (255, 255, 255, 255))
    image.putpixel((192, 96), (0, 0, 0, 255))
    return image.tobytes()


def horizontal_extrema(source, amount):
    assert len(source) == WIDTH*HEIGHT*4 and amount in (-1, 1)
    result = bytearray(source)
    operation = max if amount > 0 else min
    for y in range(HEIGHT):
        for x in range(WIDTH):
            for channel in range(3):
                result[(y*WIDTH+x)*4+channel] = operation(
                    source[(y*WIDTH+min(WIDTH-1, max(0, x+dx)))*4+channel]
                    for dx in (-1, 0, 1))
    return bytes(result)


@pytest.mark.parametrize('fault', ['bypass', 'sign', 'background', 'alpha', 'truncated'])
def test_minimax_oracle_rejects_corruption(fault):
    source = source_pixels()
    expected = horizontal_extrema(source, 1)
    pixels = bytearray(expected)
    if fault == 'bypass':
        pixels = source
    elif fault == 'sign':
        pixels = horizontal_extrema(source, -1)
    elif fault == 'background':
        pixels[0] ^= 1
    elif fault == 'alpha':
        pixels[3] = 0
    else:
        pixels = pixels[:-4]
    assert pixels != expected


def test_real_minimax_white_map(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_MINIMAX_MAP')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_MINIMAX_MAP to local MinimaxMap.aex')
    assert os.name == 'nt'
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        p = subprocess.run([str(harness), '--headless', *map(str, args)], cwd=ROOT,
                           capture_output=True, timeout=None if args[0] == '--inspect-experimental' else 90)
        assert p.returncode == 0, p.stderr.decode('utf-8', errors='replace')
        return json.loads(p.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert params[1]['name'] == 'Amount'
    assert params[4]['choices'][0] == 'Horizontal'
    assert params[5]['choices'][0] == 'Color'
    assert params[8]['name'] == 'Radius Map' and params[8]['kind'] == 'layer'
    source = source_pixels()
    input_path, map_path = tmp_path/'input.png', tmp_path/'map.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source).save(input_path)
    Image.new('RGBA', (WIDTH, HEIGHT), (255, 255, 255, 255)).save(map_path)
    for amount in (-1, 1):
        request, output = tmp_path/f'{amount}.json', tmp_path/f'{amount}.png'
        assignments = [{'slot':s, 'value':v} for s, v in
                       ((1, amount), (2, 1), (3, 1), (4, 1), (5, 1), (6, 1), (7, 100))]
        assignments.append({'slot':8, 'layer':str(map_path)})
        request.write_text(json.dumps({'schema_version':1,
            'timing':{'frame':0, 'fps':30, 'duration_frames':300},
            'assignments':assignments}), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, input_path, output, request)
        (tmp_path/f'{amount}-report.json').write_text(json.dumps(report), encoding='utf-8')
        assert report['passed'] and report.get('output_pixels_valid') is not False
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert('RGBA').tobytes()
        argb = bytearray(len(pixels))
        argb[0::4], argb[1::4] = pixels[3::4], pixels[0::4]
        argb[2::4], argb[3::4] = pixels[1::4], pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        assert pixels == horizontal_extrema(source, amount)
