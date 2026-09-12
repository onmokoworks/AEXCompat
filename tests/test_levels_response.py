"""8-bit gamma-one levels mapping; not a full Levels or AE parity oracle."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT

CASES = [('neutral', (0, 255, 0, 255)), ('output_range', (0, 255, 32, 224)),
         ('input_range', (64, 192, 0, 255))]


def source_pixels():
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (x, 255-x, round(y*255/(HEIGHT-1)), 255))


def expected(value, levels):
    low, high, black, white = levels
    return black + (white-black)*min(1, max(0, (value-low)/(high-low)))


def assert_levels(outputs):
    source = source_pixels()
    assert len(outputs) == 3 and outputs[0] == source
    for pixels, (_, levels) in zip(outputs, CASES):
        assert len(pixels) == len(source)
        assert pixels[3::4] == source[3::4]
        for c in range(3):
            assert all(abs(a-expected(b, levels)) <= 1
                       for a, b in zip(pixels[c::4], source[c::4]))


@pytest.mark.parametrize('fault', ['copy', 'ranges_swapped', 'gamma', 'clip_wrap',
                                  'channel_swap', 'alpha', 'truncated', 'neutral'])
def test_levels_validator_rejects_corruption(fault):
    source = source_pixels()
    outputs = [bytearray(v if i % 4 == 3 else round(expected(v, levels))
                         for i, v in enumerate(source)) for _, levels in CASES]
    assert_levels(outputs)
    if fault == 'copy':
        outputs[1] = source
    elif fault == 'ranges_swapped':
        outputs[1], outputs[2] = outputs[2], outputs[1]
    elif fault == 'gamma':
        for i in range(0, len(source), 4):
            for c in range(3):
                outputs[1][i+c] = round(32+192*(source[i+c]/255)**0.5)
    elif fault == 'clip_wrap':
        for i in range(0, len(source), 4):
            for c in range(3):
                outputs[2][i+c] = round((source[i+c]-64)*255/128) % 256
    elif fault == 'channel_swap':
        outputs[1][0::4], outputs[1][1::4] = outputs[1][1::4], outputs[1][0::4]
    elif fault == 'alpha':
        outputs[2][3] = 0
    elif fault == 'truncated':
        outputs[1] = outputs[1][:-4]
    else:
        outputs[0] = outputs[1]
    with pytest.raises(AssertionError):
        assert_levels(outputs)


def test_real_levels_mapping(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_LEVELS')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_LEVELS to the local AEX')
    assert os.name == 'nt'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)], cwd=ROOT,
                                capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert params[6]['name'] == 'Host Layer' and params[6]['kind'] == 'layer'
    for slot, name in ((8, 'Input Black'), (9, 'Input White'), (10, 'Gamma'),
                       (11, 'Output Black'), (12, 'Output White')):
        assert params[slot]['name'] == name
    assert params[15]['value'] == 1 and params[15]['choices'][0] == 'RGB'
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, values in CASES:
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1, 'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 6, 'layer': str(source)}, {'slot': 10, 'value': 1}] +
                           [{'slot': s, 'value': v} for s, v in zip((8, 9, 11, 12), values)],
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
    assert_levels(outputs)
