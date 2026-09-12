"""Average sample-layer transport and Normal opacity; other modes unverified."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT

CASES = (('a-full', 'a', 100), ('b-full', 'b', 100), ('a-half', 'a', 50), ('a-zero', 'a', 0))
COLORS = {'a': ((200, 40, 40, 255), (40, 40, 200, 255)),
          'b': ((40, 200, 40, 255), (200, 200, 40, 255))}


def source_pixels():
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (64+x//2,)*3+(255,))


def sample_pixels(name):
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in COLORS[name][x//(WIDTH//2)])


def expected(sample, opacity):
    source = source_pixels()
    mean = [(a+b)/2 for a, b in zip(*COLORS[sample])]
    return [v*(1-opacity/100)+mean[i%4]*opacity/100 for i, v in enumerate(source)]


def assert_link(outputs):
    assert len(outputs) == len(CASES)
    for pixels, (_, sample, opacity) in zip(outputs, CASES):
        source = source_pixels()
        assert len(pixels) == len(source) and pixels[3::4] == source[3::4]
        if opacity == 0:
            assert pixels == source
        else:
            assert max(abs(a-b) for a, b in zip(pixels, expected(sample, opacity))) <= 1


@pytest.mark.parametrize('fault', ['copy', 'stale_sample', 'first_pixel_sample',
                                  'wrong_opacity', 'alpha', 'truncated', 'zero', 'flat_half'])
def test_link_validator_rejects_corruption(fault):
    outputs = [bytearray(round(v) for v in expected(sample, opacity))
               for _, sample, opacity in CASES]
    assert_link(outputs)
    if fault == 'copy':
        outputs[0] = source_pixels()
    elif fault == 'stale_sample':
        outputs[1] = outputs[0]
    elif fault == 'first_pixel_sample':
        outputs[0] = bytes(COLORS['a'][0])*(WIDTH*HEIGHT)
    elif fault == 'wrong_opacity':
        outputs[2] = bytearray(round(v) for v in expected('a', 25))
    elif fault == 'alpha':
        outputs[0][3] = 0
    elif fault == 'truncated':
        outputs[0] = outputs[0][:-4]
    elif fault == 'zero':
        outputs[3] = outputs[0]
    else:
        outputs[2] = bytes(outputs[2][:4])*(WIDTH*HEIGHT)
    with pytest.raises(AssertionError):
        assert_link(outputs)


def test_real_color_link(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_COLOR_LINK')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_COLOR_LINK to the local AEX')
    assert os.name == 'nt'
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)], cwd=ROOT,
                                capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    for slot, name in ((191, 'Host Layer'), (199, 'Sample Layer')):
        assert params[slot]['name'] == name and params[slot]['kind'] == 'layer'
    assert params[200]['choices'][0] == 'Average'
    assert params[224]['choices'][0] == 'Normal' and params[223]['name'] == 'Opacity'
    source = tmp_path/'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    for sample in COLORS:
        Image.frombytes('RGBA', (WIDTH, HEIGHT), sample_pixels(sample)).save(tmp_path/f'{sample}.png')
    outputs = []
    for name, sample, opacity in CASES:
        request, output = tmp_path/f'{name}.json', tmp_path/f'{name}.png'
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 191, 'layer': str(source)},
                            {'slot': 199, 'layer': str(tmp_path/f'{sample}.png')},
                            {'slot': 200, 'value': 1}, {'slot': 224, 'value': 1},
                            {'slot': 223, 'value': opacity}]}), encoding='utf-8')
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
        outputs.append(pixels)
    assert_link(outputs)
