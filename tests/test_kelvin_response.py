"""Bounded temperature-direction regression, not an exact color-science oracle."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def source_pixels():
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (64+x//2, 64+x//2, 64+x//2, 255))


def assert_kelvin(neutral, lower, higher):
    source = source_pixels()
    assert neutral == source
    for pixels, sign in ((lower, 1), (higher, -1)):
        assert len(pixels) == len(source)
        assert pixels[3::4] == source[3::4]
        assert all(sign*(r-b) > 0 for r, b in zip(pixels[0::4], pixels[2::4]))
        for channel in range(3):
            pairs = sorted(set(zip(source[channel::4], pixels[channel::4])))
            assert len({a for a, _ in pairs}) == len(pairs)
            levels = [b for _, b in pairs]
            assert len(set(levels)) > 1
            assert all(a <= b for a, b in zip(levels, levels[1:]))


@pytest.mark.parametrize('fault', ['copy', 'swap', 'flat', 'alpha', 'truncated',
                                  'neutral', 'nonmonotonic', 'spatial'])
def test_kelvin_validator_rejects_corruption(fault):
    neutral = source_pixels()
    lower, higher = bytearray(neutral), bytearray(neutral)
    lower[0::4] = bytes(v+20 for v in neutral[0::4])
    higher[2::4] = lower[0::4]
    assert_kelvin(neutral, lower, higher)
    if fault == 'copy':
        lower = neutral
    elif fault == 'swap':
        lower, higher = higher, lower
    elif fault == 'flat':
        lower = bytes((140, 120, 120, 255))*(WIDTH*HEIGHT)
    elif fault == 'alpha':
        lower[3] = 0
    elif fault == 'truncated':
        higher = higher[:-4]
    elif fault == 'neutral':
        neutral = lower
    elif fault == 'nonmonotonic':
        # Keep warm chromaticity and a single-valued map, reverse luminance.
        lower = bytes(c for i in range(0, len(neutral), 4)
                      for c in (255-neutral[i]+20, 255-neutral[i], 255-neutral[i], 255))
    else:
        lower[0] += 1  # Identical input levels must not depend on position.
    with pytest.raises(AssertionError):
        assert_kelvin(neutral, lower, higher)


def test_real_kelvin(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_KELVIN')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_KELVIN to the local AEX')
    assert os.name == 'nt'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)], cwd=ROOT,
                                capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert params[181]['name'] == 'Host Layer' and params[181]['kind'] == 'layer'
    for slot, name in ((188, 'Destination Kelvin'), (189, 'Source Kelvin')):
        assert params[slot]['name'] == name
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, destination in (('neutral', 6500), ('lower', 3200), ('higher', 10000)):
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 181, 'layer': str(source)},
                            {'slot': 188, 'value': destination},
                            {'slot': 189, 'value': 6500}]}), encoding='utf-8')
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
    assert_kelvin(*outputs)
