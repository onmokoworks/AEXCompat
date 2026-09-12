"""Target tint response on a gray ramp, not a complete color-match oracle."""
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


def assert_match(neutral, red, blue):
    source = source_pixels()
    assert neutral == source
    for pixels, selected in ((red, 0), (blue, 2)):
        assert len(pixels) == len(source) and pixels[3::4] == source[3::4]
        other = 2 if selected == 0 else 0
        assert pixels[other::4] == pixels[1::4]
        assert all(a > b for a, b in zip(pixels[selected::4], pixels[1::4]))
        for channel in range(3):
            pairs = sorted(set(zip(source[channel::4], pixels[channel::4])))
            assert len({a for a, _ in pairs}) == len(pairs)
            levels = [b for _, b in pairs]
            assert len(set(levels)) > 1
            assert all(a <= b for a, b in zip(levels, levels[1:]))
    assert red[0::4] == blue[2::4] and red[2::4] == blue[0::4]
    assert red[1::4] == blue[1::4]


@pytest.mark.parametrize('fault', ['copy', 'swap', 'flat', 'alpha', 'asymmetry',
                                  'other_channel', 'truncated', 'neutral'])
def test_match_validator_rejects_corruption(fault):
    neutral = source_pixels()
    red, blue = bytearray(neutral), bytearray(neutral)
    red[0::4] = bytes(v+14 for v in neutral[0::4])
    blue[2::4] = red[0::4]
    assert_match(neutral, red, blue)
    if fault == 'copy':
        red = neutral
    elif fault == 'swap':
        red, blue = blue, red
    elif fault == 'flat':
        red = bytes((140, 126, 126, 255)) * (WIDTH*HEIGHT)
        blue = bytes((126, 126, 140, 255)) * (WIDTH*HEIGHT)
    elif fault == 'alpha':
        red[3] = 0
    elif fault == 'asymmetry':
        red[0::4] = bytes(v+1 for v in red[0::4])
    elif fault == 'other_channel':
        red[2] += 1
    elif fault == 'truncated':
        blue = blue[:-4]
    else:
        neutral = red
    with pytest.raises(AssertionError):
        assert_match(neutral, red, blue)


def test_real_color_match(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_COLOR_MATCH')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_COLOR_MATCH to the local AEX')
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
    for slot, name, base in ((13, 'Hilight Target', 205), (18, 'Midtone Target', 128),
                             (23, 'Shadow Target', 51)):
        assert params[slot]['name'] == name and params[slot]['color'] == [255, base, base, base]
        assert params[slot-1]['color'] == params[slot]['color']
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, channel in (('neutral', None), ('red', 0), ('blue', 2)):
        assignments = [{'slot': 6, 'layer': str(source)}]
        for slot, base in ((13, 205), (18, 128), (23, 51)):
            color = [base]*3
            if channel is not None:
                color[channel] += 30
            assignments.append({'slot': slot, 'color': [255, *color]})
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': assignments}), encoding='utf-8')
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
    assert_match(*outputs)
