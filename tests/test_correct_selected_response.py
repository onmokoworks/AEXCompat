"""Exact RGB selection with HSI desaturation; not general selection/AE parity."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def source_pixels():
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in ((200, 40, 40, 255) if x < WIDTH//2 else (40, 40, 200, 255)))


def assert_selected(neutral, red, blue):
    source = source_pixels()
    assert neutral == source
    for pixels, selected_left in ((red, True), (blue, False)):
        assert len(pixels) == len(source)
        for i in range(0, len(source), 4):
            selected = (i//4 % WIDTH < WIDTH//2) == selected_left
            if selected:
                assert pixels[i+3] == 255
                assert all(abs(c - 280/3) <= 1 for c in pixels[i:i+3])
            else:
                assert pixels[i:i+4] == source[i:i+4]


@pytest.mark.parametrize('fault', ['copy', 'swapped', 'whole_gray', 'alpha',
                                  'wrong_gray', 'leak', 'truncated', 'neutral'])
def test_selected_validator_rejects_corruption(fault):
    neutral = source_pixels()
    red, blue = bytearray(neutral), bytearray(neutral)
    for i in range(0, len(neutral), 4):
        output = red if i//4 % WIDTH < WIDTH//2 else blue
        output[i:i+3] = bytes([93, 93, 93])
    assert_selected(neutral, red, blue)
    if fault == 'copy':
        red = neutral
    elif fault == 'swapped':
        red, blue = blue, red
    elif fault == 'whole_gray':
        red = bytes((93, 93, 93, 255)) * (WIDTH*HEIGHT)
    elif fault == 'alpha':
        red[3] = 0
    elif fault == 'wrong_gray':
        red[0:3] = bytes([120, 120, 120])
    elif fault == 'leak':
        red[(WIDTH//2)*4] += 1
    elif fault == 'truncated':
        blue = blue[:-4]
    else:
        neutral = red
    with pytest.raises(AssertionError):
        assert_selected(neutral, red, blue)


def test_real_correct_selected(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_CORRECT_SELECTED')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_CORRECT_SELECTED to the local AEX')
    assert os.name == 'nt'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)], cwd=ROOT,
                                capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    for slot, name in ((6, 'Host Layer'), (9, 'Color Matching'), (11, 'Color'),
                       (12, 'Color Range'), (13, 'Blend'), (22, 'Saturation')):
        assert params[slot]['name'] == name
    assert params[9]['choices'][0] == 'RGB'
    assert params[19]['choices'][1] == 'HSI' and params[19]['value'] == 2
    minimum = params[22]['minimum']
    assert minimum < -50
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, color, saturation in (('neutral', [255, 200, 40, 40], 0),
                                     ('red', [255, 200, 40, 40], minimum),
                                     ('blue', [255, 40, 40, 200], minimum)):
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1, 'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 6, 'layer': str(source)}, {'slot': 9, 'value': 1},
                            {'slot': 11, 'color': color}, {'slot': 12, 'value': 10},
                            {'slot': 13, 'value': 0}, {'slot': 22, 'value': saturation}],
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
    assert_selected(*outputs)
