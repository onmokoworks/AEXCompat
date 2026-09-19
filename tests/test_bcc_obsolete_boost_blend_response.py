"""Legacy BCC Boost Blend two-input response; not exact AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def primary_pixels():
    return bytes(
        channel
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in (20 + x * 160 // (WIDTH - 1),
                        30 + y * 140 // (HEIGHT - 1), 60, 255)
    )


def blend_pixels(red):
    return bytes(
        channel
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in ((220 if red else 20) if (x // 16 + y // 16) % 2 else
                        (150 if red else 30),
                        30 if red else 80,
                        20 if red else (220 if (x // 16 + y // 16) % 2 else 150),
                        255)
    )


def max_blend_pixels(blend):
    primary = primary_pixels()
    return bytearray(max(primary[index], blend[index])
                     for index in range(len(primary)))


def assert_boost_blend_response(bypass, red_output, blue_output):
    primary = primary_pixels()
    red, blue = blend_pixels(True), blend_pixels(False)
    assert bypass == red
    assert len(red_output) == len(blue_output) == WIDTH * HEIGHT * 4
    assert red_output[3::4] == blue_output[3::4] == bytes([255]) * WIDTH * HEIGHT

    for output, blend in ((red_output, red), (blue_output, blue)):
        expected = max_blend_pixels(blend)
        for offset in range(0, len(output), 4):
            assert all(abs(output[offset + channel] -
                           expected[offset + channel]) <= 2
                       for channel in range(3))
    assert sum(red_output[offset:offset + 4] != blue_output[offset:offset + 4]
               for offset in range(0, len(red_output), 4)) == WIDTH * HEIGHT
    assert sum(red_output[offset:offset + 4] != primary[offset:offset + 4]
               for offset in range(0, len(red_output), 4)) >= 30_000
    assert sum(blue_output[offset:offset + 4] != primary[offset:offset + 4]
               for offset in range(0, len(blue_output), 4)) == WIDTH * HEIGHT


@pytest.mark.parametrize('fault', [
    'primary_only', 'secondary_only', 'uniform', 'transparent',
    'swap_active', 'swap_rb', 'minimum_operator', 'mirrored',
    'wrong_bypass', 'blue_bypass', 'local_corruption', 'truncated',
])
def test_boost_blend_validator_rejects_corruption(fault):
    primary = primary_pixels()
    red, blue = blend_pixels(True), blend_pixels(False)
    bypass = red
    red_output, blue_output = max_blend_pixels(red), max_blend_pixels(blue)
    assert_boost_blend_response(bypass, red_output, blue_output)
    if fault == 'primary_only':
        red_output = primary
    elif fault == 'secondary_only':
        red_output = red
    elif fault == 'uniform':
        red_output = bytes((180, 100, 60, 255)) * WIDTH * HEIGHT
    elif fault == 'transparent':
        red_output[3] = 0
    elif fault == 'swap_active':
        red_output, blue_output = blue_output, red_output
    elif fault == 'swap_rb':
        for offset in range(0, len(red_output), 4):
            red_output[offset], red_output[offset + 2] = (
                red_output[offset + 2], red_output[offset])
    elif fault == 'minimum_operator':
        red_output = bytearray(min(primary[index], red[index])
                               for index in range(len(primary)))
    elif fault == 'mirrored':
        mirrored = bytearray(len(red_output))
        for y in range(HEIGHT):
            for x in range(WIDTH):
                source = (y * WIDTH + x) * 4
                destination = (y * WIDTH + WIDTH - 1 - x) * 4
                mirrored[destination:destination + 4] = red_output[source:source + 4]
        red_output = mirrored
    elif fault == 'wrong_bypass':
        bypass = primary
    elif fault == 'blue_bypass':
        bypass = blue
    elif fault == 'local_corruption':
        red_output[(HEIGHT // 2 * WIDTH + WIDTH // 2) * 4] = 0
    else:
        blue_output = blue_output[:-4]
    with pytest.raises(AssertionError):
        assert_boost_blend_response(bypass, red_output, blue_output)


def test_installed_bcc_obsolete_boost_blend_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCBoostBlend.aex'
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
    assert parameters[8]['name'] == 'Blend Layer'
    assert parameters[8]['kind'] == 'layer'
    assert parameters[9]['name'] == 'Mix Back'
    assert parameters[10]['name'] == 'Mode'
    assert parameters[13]['name'] == 'Boost Mix'

    primary = tmp_path / 'primary.png'
    red = tmp_path / 'red.png'
    blue = tmp_path / 'blue.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), primary_pixels()).save(primary)
    Image.frombytes('RGBA', (WIDTH, HEIGHT), blend_pixels(True)).save(red)
    Image.frombytes('RGBA', (WIDTH, HEIGHT), blend_pixels(False)).save(blue)

    outputs = []
    for name, blend, mix_back, boost in (
            ('bypass', red, 100, 100),
            ('red', red, 0, 100),
            ('blue', blue, 0, 100)):
        request = tmp_path / f'{name}.json'
        output = tmp_path / f'{name}-output.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(primary)},
                {'slot': 8, 'layer': str(blend)},
                {'slot': 9, 'value': mix_back},
                {'slot': 10, 'value': 1},
                {'slot': 11, 'value': 20},
                {'slot': 12, 'value': 0},
                {'slot': 13, 'value': boost},
                {'slot': 15, 'value': 1},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, primary, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p for p in report['requested_parameters']}
        assert requested[9]['value'] == mix_back
        assert requested[13]['value'] == boost
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())

    assert_boost_blend_response(*outputs)
