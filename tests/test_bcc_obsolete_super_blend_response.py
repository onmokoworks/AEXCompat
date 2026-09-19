"""Legacy BCC Super Blend two-layer response; not exact AE parity."""
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


def secondary_pixels(red):
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


def normal_blend_pixels(secondary, secondary_weight=0.5):
    primary = primary_pixels()
    return bytearray(round(primary[index] * (1 - secondary_weight) +
                           secondary[index] * secondary_weight)
                     for index in range(len(primary)))


def assert_super_blend_response(base, red_output, blue_output):
    primary = primary_pixels()
    red, blue = secondary_pixels(True), secondary_pixels(False)
    assert base == primary
    assert len(red_output) == len(blue_output) == WIDTH * HEIGHT * 4
    assert red_output[3::4] == blue_output[3::4] == bytes([255]) * WIDTH * HEIGHT
    for output, secondary in ((red_output, red), (blue_output, blue)):
        expected = normal_blend_pixels(secondary)
        for offset in range(0, len(output), 4):
            assert all(abs(output[offset + channel] -
                           expected[offset + channel]) <= 2
                       for channel in range(3))
    assert sum(red_output[offset:offset + 4] != blue_output[offset:offset + 4]
               for offset in range(0, len(red_output), 4)) == WIDTH * HEIGHT


@pytest.mark.parametrize('fault', [
    'primary_only', 'secondary_only', 'uniform', 'transparent',
    'swap_active', 'swap_rb', 'wrong_weight', 'additive', 'mirrored',
    'wrong_base', 'local_corruption', 'truncated',
])
def test_super_blend_validator_rejects_corruption(fault):
    primary = primary_pixels()
    red, blue = secondary_pixels(True), secondary_pixels(False)
    base = primary
    red_output = normal_blend_pixels(red)
    blue_output = normal_blend_pixels(blue)
    assert_super_blend_response(base, red_output, blue_output)
    if fault == 'primary_only':
        red_output = primary
    elif fault == 'secondary_only':
        red_output = red
    elif fault == 'uniform':
        red_output = bytes((140, 65, 40, 255)) * WIDTH * HEIGHT
    elif fault == 'transparent':
        red_output[3] = 0
    elif fault == 'swap_active':
        red_output, blue_output = blue_output, red_output
    elif fault == 'swap_rb':
        for offset in range(0, len(red_output), 4):
            red_output[offset], red_output[offset + 2] = (
                red_output[offset + 2], red_output[offset])
    elif fault == 'wrong_weight':
        red_output = normal_blend_pixels(red, 0.75)
    elif fault == 'additive':
        red_output = bytearray(min(255, primary[index] + red[index])
                               for index in range(len(primary)))
    elif fault == 'mirrored':
        mirrored = bytearray(len(red_output))
        for y in range(HEIGHT):
            for x in range(WIDTH):
                source = (y * WIDTH + x) * 4
                destination = (y * WIDTH + WIDTH - 1 - x) * 4
                mirrored[destination:destination + 4] = red_output[source:source + 4]
        red_output = mirrored
    elif fault == 'wrong_base':
        base = red
    elif fault == 'local_corruption':
        red_output[(HEIGHT // 2 * WIDTH + WIDTH // 2) * 4] = 0
    else:
        blue_output = blue_output[:-4]
    with pytest.raises(AssertionError):
        assert_super_blend_response(base, red_output, blue_output)


def test_installed_bcc_obsolete_super_blend_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCSuperBlend.aex'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    for slot, name, kind in (
            (6, 'Host Layer', 'layer'), (13, 'Source', 'layer'),
            (15, 'Opacity Layer 1', 'float'), (17, 'Apply Mode', 'integer'),
            (22, 'Source', 'layer'), (24, 'Opacity Layer 2', 'float'),
            (26, 'Apply Mode', 'integer'), (72, 'Mix with Original', 'float')):
        assert parameters[slot]['name'] == name
        assert parameters[slot]['kind'] == kind

    primary = tmp_path / 'primary.png'
    red = tmp_path / 'red.png'
    blue = tmp_path / 'blue.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), primary_pixels()).save(primary)
    Image.frombytes('RGBA', (WIDTH, HEIGHT), secondary_pixels(True)).save(red)
    Image.frombytes('RGBA', (WIDTH, HEIGHT), secondary_pixels(False)).save(blue)

    outputs = []
    for name, secondary, layer_two_on in (
            ('base', red, 0), ('red', red, 1), ('blue', blue, 1)):
        request = tmp_path / f'{name}.json'
        output = tmp_path / f'{name}-output.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(primary)},
                {'slot': 10, 'value': 2},
                {'slot': 11, 'value': 1},
                {'slot': 13, 'layer': str(primary)},
                {'slot': 15, 'value': 100},
                {'slot': 16, 'value': 0},
                {'slot': 17, 'value': 1},
                {'slot': 18, 'value': 100},
                {'slot': 20, 'value': layer_two_on},
                {'slot': 22, 'layer': str(secondary)},
                {'slot': 24, 'value': 50},
                {'slot': 25, 'value': 0},
                {'slot': 26, 'value': 1},
                {'slot': 27, 'value': 100},
                {'slot': 29, 'value': 0},
                {'slot': 38, 'value': 0},
                {'slot': 47, 'value': 0},
                {'slot': 72, 'value': 0},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, primary, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p for p in report['requested_parameters']}
        assert requested[10]['value'] == 2
        assert requested[16]['value'] == 0
        assert requested[20]['value'] == layer_two_on
        assert requested[24]['value'] == 50
        assert requested[25]['value'] == 0
        assert requested[26]['value'] == 1
        assert requested[29]['value'] == 0
        assert requested[38]['value'] == 0
        assert requested[47]['value'] == 0
        assert requested[72]['value'] == 0
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())

    assert_super_blend_response(*outputs)
