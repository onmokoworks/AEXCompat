"""Green-cast direction and mix bounds, not an exact fluorescence/AE oracle."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def source_pixels():
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (64+x//2, 84+x//2, 64+x//2, 255))


def assert_fluorescent(neutral, middle, full, half_mix, original):
    source = source_pixels()
    assert neutral == original == source
    for pixels in (middle, full, half_mix):
        assert len(pixels) == len(source) and pixels[3::4] == source[3::4]
        for channel in range(3):
            pairs = sorted(set(zip(source[channel::4], pixels[channel::4])))
            assert len({a for a, _ in pairs}) == len(pairs)
            levels = [b for _, b in pairs]
            assert len(set(levels)) > 1
            assert all(a <= b for a, b in zip(levels, levels[1:]))
    for i in range(0, len(source), 4):
        def green(p):
            return 2*p[i+1]-p[i]-p[i+2]
        assert green(full) < green(middle) < green(source)
        assert green(full) < green(half_mix) < green(source)
    for a, b, m in zip(source, full, half_mix):
        assert min(a, b) <= m <= max(a, b)


@pytest.mark.parametrize('fault', ['copy', 'swap', 'flat', 'alpha', 'truncated',
                                  'neutral', 'original', 'mix_endpoint', 'mix_outside'])
def test_fluorescent_validator_rejects_corruption(fault):
    neutral = source_pixels()
    middle, full, half_mix = [bytearray(neutral) for _ in range(3)]
    middle[1::4] = half_mix[1::4] = bytes(v-10 for v in neutral[1::4])
    full[1::4] = bytes(v-20 for v in neutral[1::4])
    original = neutral
    assert_fluorescent(neutral, middle, full, half_mix, original)
    if fault == 'copy':
        middle = neutral
    elif fault == 'swap':
        middle, full = full, middle
    elif fault == 'flat':
        full = bytes((128, 108, 128, 255))*(WIDTH*HEIGHT)
    elif fault == 'alpha':
        full[3] = 0
    elif fault == 'truncated':
        middle = middle[:-4]
    elif fault == 'neutral':
        neutral = full
    elif fault == 'original':
        original = full
    elif fault == 'mix_endpoint':
        half_mix = full
    else:
        # A red offset keeps intermediate green bias but escapes the channel bounds.
        half_mix[0::4] = bytes(v+1 for v in neutral[0::4])
    with pytest.raises(AssertionError):
        assert_fluorescent(neutral, middle, full, half_mix, original)


def test_real_fluorescent(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_FLUORESCENT')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_FLUORESCENT to the local AEX')
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
    assert params[186]['name'] == 'Temperature'
    assert params[187]['name'] == 'Mix with Original'
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, temperature, mix in (('neutral', 0, 0), ('middle', 10, 0), ('full', 20, 0),
                                    ('half_mix', 20, 50), ('original', 20, 100)):
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 181, 'layer': str(source)},
                            {'slot': 186, 'value': temperature},
                            {'slot': 187, 'value': mix}]}), encoding='utf-8')
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
    assert_fluorescent(*outputs)
