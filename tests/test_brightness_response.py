"""Brightness direction and selected-channel response, not a transfer-curve oracle."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def source_pixels():
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (64 + x // 4, 80 + y // 3, 96 + (x + y) // 6, 255))


def assert_brightness(neutral, brighter, darker, red):
    source = source_pixels()
    assert neutral == source
    for pixels in (brighter, darker, red):
        assert len(pixels) == len(source)
        assert pixels[3::4] == source[3::4]
    for channel in range(3):
        assert all(a > b for a, b in zip(brighter[channel::4], source[channel::4]))
        assert all(a < b for a, b in zip(darker[channel::4], source[channel::4]))
        for pixels in (brighter, darker):
            pairs = sorted(set(zip(source[channel::4], pixels[channel::4])))
            assert len({a for a, _ in pairs}) == len(pairs)
            levels = [b for _, b in pairs]
            # Reject flat fills without assuming a vendor-specific curve or
            # a quantization-dependent minimum number of retained levels.
            assert len(set(levels)) > 1
            assert all(a <= b for a, b in zip(levels, levels[1:]))
    assert red[0::4] == brighter[0::4]
    assert red[1::4] == source[1::4]
    assert red[2::4] == source[2::4]


@pytest.mark.parametrize('fault', [
    'copy', 'reversed', 'green_leak', 'blue_leak', 'red_ignored',
    'alpha', 'truncated', 'neutral_changed', 'white_fill', 'black_fill',
])
def test_brightness_validator_rejects_corruption(fault):
    source = source_pixels()
    brighter, darker, red = bytearray(source), bytearray(source), bytearray(source)
    for i in range(0, len(source), 4):
        for channel in range(3):
            brighter[i + channel] += 10
            darker[i + channel] -= 10
        red[i] += 10
    neutral = source
    assert_brightness(neutral, brighter, darker, red)
    if fault == 'copy':
        brighter = source
    elif fault == 'reversed':
        brighter, darker = darker, brighter
    elif fault == 'green_leak':
        red[1] += 1
    elif fault == 'blue_leak':
        red[2] += 1
    elif fault == 'red_ignored':
        red = source
    elif fault == 'alpha':
        darker[3] = 0
    elif fault == 'truncated':
        brighter = brighter[:-4]
    elif fault == 'white_fill':
        brighter = bytes((255, 255, 255, 255)) * (WIDTH * HEIGHT)
        red[0::4] = brighter[0::4]
    elif fault == 'black_fill':
        darker = bytes((0, 0, 0, 255)) * (WIDTH * HEIGHT)
    else:
        neutral = brighter
    with pytest.raises(AssertionError):
        assert_brightness(neutral, brighter, darker, red)


def test_real_brightness_response(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_BRIGHTNESS_CONTRAST')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_BRIGHTNESS_CONTRAST to the local AEX')
    assert os.name == 'nt'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run(
            [str(harness), '--headless', *map(str, args)], cwd=ROOT,
            capture_output=True,
            timeout=None if args[0] == '--inspect-experimental' else 90,
        )
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert params[6]['kind'] == 'layer' and params[6]['name'] == 'Host Layer'
    assert params[8]['name'] == 'Brightness' and params[8]['value'] == 0
    assert params[9]['name'] == 'Contrast' and params[9]['value'] == 0
    assert params[10]['choices'][:2] == ['RGB', 'Red']
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, brightness, channel in (
        ('neutral', 0, 1), ('brighter', 25, 1), ('darker', -25, 1), ('red', 25, 2),
    ):
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(source)}, {'slot': 8, 'value': brightness},
                {'slot': 9, 'value': 0}, {'slot': 10, 'value': channel},
            ],
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
    assert_brightness(*outputs)
