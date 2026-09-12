"""Three tonal-band saturation response; exact selection curves remain unverified."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT

COLORS = ((48, 24, 24, 255), (144, 120, 120, 255), (240, 216, 216, 255))


def mirror(pixels):
    assert len(pixels) == WIDTH*HEIGHT*4
    return Image.frombytes('RGBA', (WIDTH, HEIGHT), pixels).transpose(
        Image.Transpose.FLIP_LEFT_RIGHT).tobytes()


def assert_mirrored(forward, reversed_outputs):
    assert len(forward) == len(reversed_outputs) == 4
    assert_selective(*forward)
    for original, reversed_output in zip(forward, reversed_outputs):
        assert reversed_output == mirror(original)


def band_pixels(colors):
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in colors[min(x*3//WIDTH, 2)])


def assert_selective(neutral, shadows, midtones, highlights):
    source = band_pixels(COLORS)
    assert neutral == source
    for pixels, selected in ((shadows, 0), (midtones, 1), (highlights, 2)):
        assert len(pixels) == len(source) and pixels[3::4] == source[3::4]
        bands = [set() for _ in range(3)]
        for y in range(HEIGHT):
            for x in range(WIDTH):
                i = (y*WIDTH+x)*4
                bands[min(x*3//WIDTH, 2)].add(tuple(pixels[i:i+3]))
        assert all(len(b) == 1 for b in bands)
        colors = [next(iter(b)) for b in bands]
        spreads = []
        for original, output in zip(COLORS, colors):
            assert min(original[:3]) <= min(output) <= max(output) <= max(original[:3])
            assert output[0] >= output[1] == output[2]
            spreads.append(max(output)-min(output))
        assert spreads[selected] < 24
        assert all(spreads[selected] < spread for i, spread in enumerate(spreads)
                   if i != selected)


@pytest.mark.parametrize('fault', ['copy', 'swap', 'all_gray', 'flat', 'alpha',
                                  'truncated', 'neutral', 'spatial', 'hue_shift'])
def test_selective_validator_rejects_corruption(fault):
    neutral = band_pixels(COLORS)
    outputs = []
    for selected in range(3):
        colors = list(COLORS)
        base = sum(colors[selected][:3])//3
        colors[selected] = (base, base, base, 255)
        outputs.append(bytearray(band_pixels(colors)))
    assert_selective(neutral, *outputs)
    if fault == 'copy':
        outputs[0] = neutral
    elif fault == 'swap':
        outputs[0], outputs[2] = outputs[2], outputs[0]
    elif fault == 'all_gray':
        outputs[0] = band_pixels([(sum(c[:3])//3,)*3+(255,) for c in COLORS])
    elif fault == 'flat':
        outputs[0] = bytes((128, 128, 128, 255))*(WIDTH*HEIGHT)
    elif fault == 'alpha':
        outputs[0][3] = 0
    elif fault == 'truncated':
        outputs[1] = outputs[1][:-4]
    elif fault == 'neutral':
        neutral = outputs[0]
    elif fault == 'spatial':
        outputs[0][0] += 1
    else:
        outputs[0] = band_pixels(((32, 31, 30, 255), COLORS[1], COLORS[2]))
    with pytest.raises(AssertionError):
        assert_selective(neutral, *outputs)


@pytest.mark.parametrize('fault', ['fixed_position', 'copy', 'one_pixel'])
def test_mirrored_validator_rejects_corruption(fault):
    forward = [band_pixels(COLORS)]
    for selected in range(3):
        colors = list(COLORS)
        base = sum(colors[selected][:3])//3
        colors[selected] = (base, base, base, 255)
        forward.append(band_pixels(colors))
    reversed_outputs = [mirror(p) for p in forward]
    assert_mirrored(forward, reversed_outputs)
    if fault == 'fixed_position':
        reversed_outputs[1] = forward[1]
    elif fault == 'copy':
        reversed_outputs[1] = reversed_outputs[0]
    else:
        corrupted = bytearray(reversed_outputs[1])
        corrupted[0] ^= 1
        reversed_outputs[1] = corrupted
    with pytest.raises(AssertionError):
        assert_mirrored(forward, reversed_outputs)


def test_real_selective_saturation(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_SELECTIVE_SATURATION')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_SELECTIVE_SATURATION to the local AEX')
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
    for slot, group in ((188, 'Shadows'), (193, 'Midtones'), (198, 'Highlights')):
        assert params[slot]['name'] == 'Saturation'
        assert params[slot-1]['name'] == group and params[slot-1]['kind'] == 'group_start'
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), band_pixels(COLORS)).save(source)
    outputs = []
    for name, selected in (('neutral', None), ('shadows', 188), ('midtones', 193),
                           ('highlights', 198)):
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        assignments = [{'slot': 181, 'layer': str(source)}]
        assignments.extend({'slot': slot, 'value': -100 if slot == selected else 0}
                           for slot in (188, 193, 198))
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
    assert_selective(*outputs)
    reverse_source = tmp_path / 'reverse-source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), mirror(band_pixels(COLORS))).save(reverse_source)
    reversed_outputs = []
    for name, selected in (('neutral', None), ('shadows', 188), ('midtones', 193),
                           ('highlights', 198)):
        request, output = tmp_path / f'reverse-{name}.json', tmp_path / f'reverse-{name}.png'
        assignments = [{'slot': 181, 'layer': str(reverse_source)}]
        assignments.extend({'slot': slot, 'value': -100 if slot == selected else 0}
                           for slot in (188, 193, 198))
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': assignments}), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, reverse_source, output, request)
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert('RGBA').tobytes()
        argb = bytearray(len(pixels))
        argb[0::4], argb[1::4] = pixels[3::4], pixels[0::4]
        argb[2::4], argb[3::4] = pixels[1::4], pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        reversed_outputs.append(pixels)
    assert_mirrored(outputs, reversed_outputs)
