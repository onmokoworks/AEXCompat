"""Host-layer mask routes inside/outside exposure; not a color-curve oracle."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_brightness_response import source_pixels
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT

CASES = [('neutral', 0, 0, False), ('inside', 1, 0, False),
         ('outside', 0, -1, False), ('reverse', 1, 0, True)]


def mask_pixels(reverse=False):
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in ((255, 255, 255, 255) if (x < WIDTH//2) != reverse
                           else (0, 0, 0, 255)))


def assert_regions(outputs):
    source = source_pixels()
    assert len(outputs) == 4 and outputs[0] == source
    for pixels, left, direction in ((outputs[1], True, 1), (outputs[2], False, -1),
                                    (outputs[3], False, 1)):
        assert len(pixels) == len(source)
        assert pixels[3::4] == source[3::4]
        mappings = [set() for _ in range(3)]
        for i in range(0, len(source), 4):
            if (i//4 % WIDTH < WIDTH//2) == left:
                for c in range(3):
                    assert (pixels[i+c]-source[i+c])*direction > 0
                    mappings[c].add((source[i+c], pixels[i+c]))
            else:
                assert pixels[i:i+4] == source[i:i+4]
        for mapping in mappings:
            pairs = sorted(mapping)
            assert len({a for a, _ in pairs}) == len(pairs)
            levels = [b for _, b in pairs]
            assert len(set(levels)) > 1
            assert all(a <= b for a, b in zip(levels, levels[1:]))


@pytest.mark.parametrize('fault', ['copy', 'mask_ignored', 'leak', 'direction',
                                  'alpha', 'flat', 'truncated', 'neutral'])
def test_region_validator_rejects_corruption(fault):
    source = source_pixels()
    outputs = [bytearray(source) for _ in CASES]
    for output, left, delta in ((outputs[1], True, 10), (outputs[2], False, -10),
                                (outputs[3], False, 10)):
        for i in range(0, len(source), 4):
            if (i//4 % WIDTH < WIDTH//2) == left:
                for c in range(3):
                    output[i+c] += delta
    assert_regions(outputs)
    if fault == 'copy':
        outputs[1] = source
    elif fault == 'mask_ignored':
        outputs[3] = outputs[1]
    elif fault == 'leak':
        outputs[1][WIDTH//2*4] += 1
    elif fault == 'direction':
        outputs[2][WIDTH//2*4] = source[WIDTH//2*4] + 10
    elif fault == 'alpha':
        outputs[3][3] = 0
    elif fault == 'flat':
        for i in range(0, len(source), 4):
            if i//4 % WIDTH < WIDTH//2:
                outputs[1][i:i+3] = bytes([255, 255, 255])
    elif fault == 'truncated':
        outputs[1] = outputs[1][:-4]
    else:
        outputs[0] = outputs[1]
    with pytest.raises(AssertionError):
        assert_regions(outputs)


def test_real_three_way_mask_response(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_THREE_WAY')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_THREE_WAY to the local AEX')
    assert os.name == 'nt'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)], cwd=ROOT,
                                capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    for slot in (7, 18):
        assert params[slot]['kind'] == 'layer' and params[slot]['name'] == 'Host Layer'
    assert params[16]['choices'][4] == 'Host Layer'
    assert params[19]['choices'][0] == 'Luma'
    assert params[12]['name'] == params[50]['name'] == 'Exposure'
    assert params[14]['value'] == 1 and params[14]['choices'][0] == 'Render with Matte'
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, inside, outside, reverse in CASES:
        mask = tmp_path / f'{name}-mask.png'
        Image.frombytes('RGBA', (WIDTH, HEIGHT), mask_pixels(reverse)).save(mask)
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1, 'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 7, 'layer': str(source)}, {'slot': 16, 'value': 5},
                            {'slot': 18, 'layer': str(mask)}, {'slot': 19, 'value': 1},
                            {'slot': 12, 'value': inside}, {'slot': 50, 'value': outside}],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output, request)
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert('RGBA').tobytes()
        argb = bytearray(len(pixels))
        argb[0::4], argb[1::4] = pixels[3::4], pixels[0::4]
        argb[2::4], argb[3::4] = pixels[1::4], pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        outputs.append(pixels)
    assert_regions(outputs)
