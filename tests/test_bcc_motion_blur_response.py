"""BCC Motion Blur temporal response; not an AE pixel-equivalence claim."""
import json
import os
import subprocess

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def moving_box(left):
    pixels = bytearray(bytes((0, 0, 0, 255)) * WIDTH * HEIGHT)
    for y in range(40, 104):
        for x in range(left, left + 64):
            offset = (y * WIDTH + x) * 4
            pixels[offset:offset + 3] = bytes((255, 255, 255))
    return bytes(pixels)


def assert_motion_response(source, zero, medium, strong, static):
    assert (len(source) == len(zero) == len(medium) == len(strong) == len(static)
            == WIDTH * HEIGHT * 4)
    assert zero == source
    assert static == bytes(len(static))
    changed = []
    for pixels in (medium, strong):
        assert pixels[3::4] == bytes([255]) * WIDTH * HEIGHT
        assert pixels[0::4] == pixels[1::4] == pixels[2::4]
        positions = [index for index in range(WIDTH * HEIGHT)
                     if pixels[index * 4:index * 4 + 3]
                     != source[index * 4:index * 4 + 3]]
        assert positions
        assert all(32 <= index % WIDTH < 160 and 24 <= index // WIDTH < 120
                   for index in positions)
        assert any(pixels[index * 4:index * 4 + 3] != bytes(3)
                   for index in positions)
        changed.append(len(positions))
    assert changed[1] > changed[0] > 0
    assert strong != static


@pytest.mark.parametrize('fault', ['zero_changed', 'fixed', 'reversed', 'alpha',
                                   'color', 'far_field', 'timed_ignored', 'truncated'])
def test_motion_validator_rejects_corruption(fault):
    source = moving_box(64)
    medium, strong = bytearray(source), bytearray(source)
    for pixels, width in ((medium, 2), (strong, 8)):
        for y in range(40, 104):
            for x in range(64 - width, 64):
                offset = (y * WIDTH + x) * 4
                pixels[offset:offset + 3] = bytes((96, 96, 96))
    zero = source
    static = bytes(WIDTH * HEIGHT * 4)
    assert_motion_response(source, zero, medium, strong, static)
    if fault == 'zero_changed':
        zero = medium
    elif fault == 'fixed':
        strong = medium
    elif fault == 'reversed':
        medium, strong = strong, medium
    elif fault == 'alpha':
        strong[3] = 0
    elif fault == 'color':
        strong[0] = 1
    elif fault == 'far_field':
        strong[0:3] = bytes((96, 96, 96))
    elif fault == 'timed_ignored':
        static = strong
    else:
        strong = strong[:-4]
    with pytest.raises(AssertionError):
        assert_motion_response(source, zero, medium, strong, static)


def test_installed_bcc_motion_blur_responds_to_timed_source(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_BCC_MOTION_BLUR')
    if not plugin:
        pytest.skip('requires AEXCOMPAT_TEST_BCC_MOTION_BLUR')
    assert os.name == 'nt'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert parameters[6]['name'] == 'Host Layer' and parameters[6]['kind'] == 'layer'
    assert parameters[8]['name'] == 'Source Layer' and parameters[8]['kind'] == 'layer'
    assert parameters[10]['name'] == 'Motion Blur Amt'
    assert parameters[10]['minimum'] <= 0 < 2 < 8 <= parameters[10]['maximum']

    frames = {}
    for frame, left in ((4, 48), (5, 64), (6, 80)):
        path = tmp_path / f'frame-{frame}.png'
        Image.frombytes('RGBA', (WIDTH, HEIGHT), moving_box(left)).save(path)
        frames[frame] = path

    outputs = []
    for label, amount, past, future in (
        ('zero', 0, 4, 6),
        ('medium', 2, 4, 6),
        ('strong', 8, 4, 6),
        ('static-control', 8, 5, 5),
    ):
        request = tmp_path / f'request-{label}.json'
        output = tmp_path / f'output-{label}.png'
        timed_layers = [
            {'slot': slot, 'time': time, 'time_scale': 30,
             'image': str(frames[image_frame])}
            for slot in (0, 8) for time, image_frame in ((4, past), (6, future))
        ]
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 5, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(frames[5])},
                {'slot': 8, 'layer': str(frames[5])},
                {'slot': 9, 'value': 1},
                {'slot': 10, 'value': amount},
                {'slot': 13, 'value': 1},
                {'slot': 29, 'value': 1},
                {'slot': 30, 'value': 100},
            ],
            'timed_layers': timed_layers,
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, frames[5], output, request)
        assert report['passed'] and report['output_pixels_valid']
        assert report['current_time'] == 5 and report['time_scale'] == 30
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[10] == amount
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_motion_response(moving_box(64), *outputs)
