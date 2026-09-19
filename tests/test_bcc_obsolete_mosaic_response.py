"""Legacy BCC Mosaic pixel-block response; not exact AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image, ImageFilter

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def pattern_pixels():
    return bytes(
        channel
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in (x, y, (x * 3 + y * 5) % 256, 255)
    )


def mosaic_pixels(block_x=4, block_y=2):
    source = pattern_pixels()
    output = bytearray(len(source))
    for y in range(HEIGHT):
        for x in range(WIDTH):
            destination = (y * WIDTH + x) * 4
            sample = ((y // block_y * block_y) * WIDTH
                      + x // block_x * block_x) * 4
            output[destination:destination + 4] = source[sample:sample + 4]
    return output


def assert_mosaic_response(neutral, active):
    source = pattern_pixels()
    assert neutral == source
    assert len(active) == WIDTH * HEIGHT * 4
    assert active[3::4] == bytes([255]) * WIDTH * HEIGHT
    for y in range(HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            assert abs(active[offset] - x) <= 4
            assert abs(active[offset + 1] - y) <= 2
    colors = {bytes(active[offset:offset + 3])
              for offset in range(0, len(active), 4)}
    assert 100 <= len(colors) <= WIDTH * HEIGHT // 4
    horizontal = sum(
        active[(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 3]
        == active[(y * WIDTH + x + 1) * 4:(y * WIDTH + x + 1) * 4 + 3]
        for y in range(HEIGHT) for x in range(WIDTH - 1)
    )
    vertical = sum(
        active[(y * WIDTH + x) * 4:(y * WIDTH + x) * 4 + 3]
        == active[((y + 1) * WIDTH + x) * 4:((y + 1) * WIDTH + x) * 4 + 3]
        for y in range(HEIGHT - 1) for x in range(WIDTH)
    )
    assert horizontal >= HEIGHT * (WIDTH - 1) * 2 // 3
    assert vertical >= (HEIGHT - 1) * WIDTH // 2


@pytest.mark.parametrize('fault', [
    'copy', 'blur', 'uniform', 'transparent', 'horizontal_only',
    'vertical_only', 'rgb_inverted', 'mirrored', 'wrong_neutral', 'truncated',
])
def test_mosaic_validator_rejects_corruption(fault):
    neutral, active = pattern_pixels(), mosaic_pixels()
    assert_mosaic_response(neutral, active)
    if fault == 'copy':
        active = neutral
    elif fault == 'blur':
        active = Image.frombytes('RGBA', (WIDTH, HEIGHT), neutral).filter(
            ImageFilter.GaussianBlur(4)).tobytes()
    elif fault == 'uniform':
        active = bytes((64, 64, 64, 255)) * WIDTH * HEIGHT
    elif fault == 'transparent':
        active[3] = 0
    elif fault == 'horizontal_only':
        active = mosaic_pixels(4, 1)
    elif fault == 'vertical_only':
        active = mosaic_pixels(1, 2)
    elif fault == 'rgb_inverted':
        for offset in range(0, len(active), 4):
            for channel in range(3):
                active[offset + channel] = 255 - active[offset + channel]
    elif fault == 'mirrored':
        mirrored = bytearray(len(active))
        for y in range(HEIGHT):
            for x in range(WIDTH):
                source = (y * WIDTH + x) * 4
                destination = (y * WIDTH + WIDTH - 1 - x) * 4
                mirrored[destination:destination + 4] = active[source:source + 4]
        active = mirrored
    elif fault == 'wrong_neutral':
        neutral = active
    else:
        active = active[:-4]
    with pytest.raises(AssertionError):
        assert_mosaic_response(neutral, active)


def test_installed_bcc_obsolete_mosaic_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCMosaic.aex'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert parameters[6]['name'] == 'Host Layer'
    assert parameters[6]['kind'] == 'layer'
    assert parameters[9]['name'] == 'Pixelate X'
    assert parameters[9]['kind'] == 'float'
    assert parameters[10]['name'] == 'Pixelate Y'
    assert parameters[10]['kind'] == 'float'
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), pattern_pixels()).save(source)
    outputs = []
    for amount in (0, 25):
        request, output = tmp_path / f'{amount}.json', tmp_path / f'{amount}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(source)},
                {'slot': 9, 'value': amount},
                {'slot': 10, 'value': amount},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[9] == amount and requested[10] == amount
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_mosaic_response(*outputs)
