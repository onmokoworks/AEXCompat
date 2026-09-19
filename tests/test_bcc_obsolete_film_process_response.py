"""Legacy BCC Film Process brightness response; not exact AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


PLUGINS = ('BCCFastFilmProcess.aex', 'BCCFilmProcess.aex')


def gradient_pixels():
    return bytes(
        channel
        for _y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in (x, x, x, 255)
    )


def assert_brightness_response(dark, neutral, bright):
    source = gradient_pixels()
    assert neutral == source
    for raw in (dark, bright):
        assert len(raw) == WIDTH * HEIGHT * 4
        assert raw[3::4] == bytes([255]) * WIDTH * HEIGHT
        assert raw[0::4] == raw[1::4] == raw[2::4]
        first_row = raw[:WIDTH * 4]
        assert all(raw[y * WIDTH * 4:(y + 1) * WIDTH * 4] == first_row
                   for y in range(1, HEIGHT))
        row = raw[:WIDTH * 4:4]
        assert all(left <= right for left, right in zip(row, row[1:]))
        assert len(set(row)) >= 64
    assert all(dark[offset] <= neutral[offset] <= bright[offset]
               for offset in range(0, len(neutral), 4))
    strict_dark = sum(dark[offset] < neutral[offset]
                      for offset in range(0, len(neutral), 4))
    strict_bright = sum(neutral[offset] < bright[offset]
                        for offset in range(0, len(neutral), 4))
    assert strict_dark >= WIDTH * HEIGHT * 3 // 4
    assert strict_bright >= WIDTH * HEIGHT * 3 // 4
    pixel_count = WIDTH * HEIGHT
    assert sum(neutral[0::4]) / pixel_count - sum(dark[0::4]) / pixel_count >= 40
    assert sum(bright[0::4]) / pixel_count - sum(neutral[0::4]) / pixel_count >= 40


def synthetic_brightness(offset):
    return bytes(
        channel
        for _y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in (
            max(0, min(255, x + offset)),
            max(0, min(255, x + offset)),
            max(0, min(255, x + offset)),
            255,
        )
    )


@pytest.mark.parametrize('fault', [
    'copy_dark', 'copy_bright', 'transparent', 'color', 'nonmonotonic',
    'spatial', 'reversed', 'wrong_neutral', 'truncated',
])
def test_film_process_brightness_validator_rejects_corruption(fault):
    dark = bytearray(synthetic_brightness(-64))
    neutral = gradient_pixels()
    bright = bytearray(synthetic_brightness(64))
    assert_brightness_response(dark, neutral, bright)
    if fault == 'copy_dark':
        dark = neutral
    elif fault == 'copy_bright':
        bright = neutral
    elif fault == 'transparent':
        dark[3] = 0
    elif fault == 'color':
        bright[1] = 0
    elif fault == 'nonmonotonic':
        offset = 128 * 4
        bright[offset:offset + 3] = bytes((0, 0, 0))
    elif fault == 'spatial':
        offset = WIDTH * 4
        bright[offset:offset + 3] = bytes((1, 1, 1))
    elif fault == 'reversed':
        dark, bright = bright, dark
    elif fault == 'wrong_neutral':
        neutral = bright
    else:
        bright = bright[:-4]
    with pytest.raises(AssertionError):
        assert_brightness_response(dark, neutral, bright)


@pytest.mark.parametrize('plugin_name', PLUGINS)
def test_installed_bcc_obsolete_film_process_brightness_response(
        tmp_path, plugin_name):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / plugin_name
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
    assert parameters[14]['name'] == 'Brightness'
    assert parameters[14]['kind'] == 'float'
    assert parameters[14]['minimum'] <= -50 < 0 < 50 <= parameters[14]['maximum']

    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), gradient_pixels()).save(source)
    outputs = []
    for amount in (-50, 0, 50):
        request, output = tmp_path / f'{amount}.json', tmp_path / f'{amount}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(source)},
                {'slot': 14, 'value': amount},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[14] == amount
        with Image.open(output) as image:
            outputs.append(image.convert('RGBA').tobytes())
    assert_brightness_response(*outputs)
