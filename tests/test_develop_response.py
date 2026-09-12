"""Develop exposure with Auto-Equalize off; bounded gain, not full AE parity."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_brightness_response import source_pixels
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def assert_exposure(neutral, brighter, darker):
    source = source_pixels()
    assert neutral == source
    for pixels, gain in ((brighter, 2), (darker, 0.5)):
        assert len(pixels) == len(source)
        assert pixels[3::4] == source[3::4]
        for channel in range(3):
            assert all(abs(a - min(255, b * gain)) <= 1
                       for a, b in zip(pixels[channel::4], source[channel::4]))


def gained(gain):
    source = source_pixels()
    return bytearray(value if i % 4 == 3 else min(255, round(value * gain))
                     for i, value in enumerate(source))


@pytest.mark.parametrize('fault', [
    'copy', 'reverse', 'wrong_gain', 'channel_swap', 'red_only',
    'white', 'black', 'alpha', 'truncated', 'neutral',
])
def test_exposure_validator_rejects_corruption(fault):
    neutral, brighter, darker = source_pixels(), gained(2), gained(0.5)
    assert_exposure(neutral, brighter, darker)
    if fault == 'copy':
        brighter = neutral
    elif fault == 'reverse':
        brighter, darker = darker, brighter
    elif fault == 'wrong_gain':
        brighter = gained(1.5)
    elif fault == 'channel_swap':
        brighter[0::4], brighter[2::4] = brighter[2::4], brighter[0::4]
    elif fault == 'red_only':
        brighter[1::4], brighter[2::4] = neutral[1::4], neutral[2::4]
    elif fault == 'white':
        brighter = bytes((255, 255, 255, 255)) * (WIDTH * HEIGHT)
    elif fault == 'black':
        darker = bytes((0, 0, 0, 255)) * (WIDTH * HEIGHT)
    elif fault == 'alpha':
        darker[3] = 0
    elif fault == 'truncated':
        darker = darker[:-4]
    else:
        neutral = brighter
    with pytest.raises(AssertionError):
        assert_exposure(neutral, brighter, darker)


def test_real_develop_exposure(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_DEVELOP')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_DEVELOP to the local AEX')
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
    assert params[181]['name'] == 'Host Layer' and params[181]['kind'] == 'layer'
    assert params[189]['name'] == 'Auto-Equalize'
    assert params[192]['name'] == 'Exposure' and params[192]['value'] == 0
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for exposure in (0, 1, -1):
        request, output = tmp_path / f'{exposure}.json', tmp_path / f'{exposure}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 181, 'layer': str(source)}, {'slot': 189, 'value': 0},
                {'slot': 192, 'value': exposure},
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
    assert_exposure(*outputs)
