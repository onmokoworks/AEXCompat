"""Photo Positive channel independence, not an exact curve or AE oracle."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_brightness_response import source_pixels
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT

CASES = [('neutral', (0, 0, 0)), ('red', (20, 0, 0)),
         ('green', (0, -20, 0)), ('blue', (0, 0, 20)), ('combined', (20, -20, 20))]


def assert_balance(outputs):
    source = source_pixels()
    assert len(outputs) == 5 and outputs[0] == source
    for pixels in outputs:
        assert len(pixels) == len(source)
        assert pixels[3::4] == source[3::4]
    for channel, direction in enumerate((1, -1, 1)):
        pixels = outputs[channel + 1]
        for other in range(3):
            if other != channel:
                assert pixels[other::4] == source[other::4]
        assert all((a-b)*direction > 0 for a, b in zip(pixels[channel::4], source[channel::4]))
        pairs = sorted(set(zip(source[channel::4], pixels[channel::4])))
        assert len({a for a, _ in pairs}) == len(pairs)
        levels = [b for _, b in pairs]
        assert len(set(levels)) > 1
        assert all(a <= b for a, b in zip(levels, levels[1:]))
        assert outputs[4][channel::4] == pixels[channel::4]


@pytest.mark.parametrize('fault', ['copy', 'direction', 'leak', 'alpha', 'flat',
                                  'composition', 'truncated', 'neutral', 'nonmonotonic'])
def test_balance_validator_rejects_corruption(fault):
    source = source_pixels()
    outputs = [bytearray(source) for _ in CASES]
    for channel, delta in enumerate((10, -10, 10)):
        values = bytes(v + delta for v in source[channel::4])
        outputs[channel+1][channel::4] = values
        outputs[4][channel::4] = values
    assert_balance(outputs)
    if fault == 'copy':
        outputs[1] = source
    elif fault == 'direction':
        outputs[2][1::4] = bytes(v+10 for v in source[1::4])
    elif fault == 'leak':
        outputs[1][1] += 1
    elif fault == 'alpha':
        outputs[4][3] = 0
    elif fault == 'flat':
        outputs[1][0::4] = outputs[4][0::4] = bytes([255]) * (WIDTH*HEIGHT)
    elif fault == 'composition':
        outputs[4][0] += 1
    elif fault == 'truncated':
        outputs[3] = outputs[3][:-4]
    elif fault == 'neutral':
        outputs[0] = outputs[1]
    else:
        values = bytes(255-v for v in source[0::4])
        outputs[1][0::4] = outputs[4][0::4] = values
    with pytest.raises(AssertionError):
        assert_balance(outputs)


def test_real_color_balance(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_COLOR_BALANCE')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_COLOR_BALANCE to the local AEX')
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
    assert params[8]['value'] == 1 and params[8]['choices'][0] == 'Photo Positive'
    for slot, name in ((9, 'Red Balance'), (10, 'Green Balance'), (11, 'Blue Balance')):
        assert params[slot]['name'] == name and params[slot]['value'] == 0
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, values in CASES:
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1, 'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 6, 'layer': str(source)}] +
                           [{'slot': s, 'value': v} for s, v in zip((9, 10, 11), values)],
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
    assert_balance(outputs)
