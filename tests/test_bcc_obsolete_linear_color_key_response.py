"""Legacy BCC Linear Color Key alpha response; not exact AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


COLORS = ((0, 255, 0), (255, 0, 0), (0, 0, 255))


def band_pixels():
    return bytes(
        channel
        for _y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in (*COLORS[min(2, x * 3 // WIDTH)], 255)
    )


def keyed_pixels(keyed_band):
    output = bytearray(band_pixels())
    for y in range(HEIGHT):
        for x in range(WIDTH):
            if min(2, x * 3 // WIDTH) == keyed_band:
                output[(y * WIDTH + x) * 4 + 3] = 0
    return output


def assert_linear_color_key_response(green_output, blue_output):
    assert len(green_output) == len(blue_output) == WIDTH * HEIGHT * 4
    assert green_output == keyed_pixels(0)
    assert blue_output == keyed_pixels(2)
    assert all(green_output[offset:offset + 3] ==
               blue_output[offset:offset + 3]
               for offset in range(0, len(green_output), 4))


@pytest.mark.parametrize('fault', [
    'copy', 'all_transparent', 'all_opaque', 'green_keys_red',
    'blue_keys_red', 'same_matte', 'invert_green', 'swap_outputs',
    'rgb_damage', 'green_local_alpha', 'blue_local_alpha', 'truncated',
])
def test_linear_color_key_validator_rejects_corruption(fault):
    source = band_pixels()
    green_output, blue_output = keyed_pixels(0), keyed_pixels(2)
    assert_linear_color_key_response(green_output, blue_output)
    if fault == 'copy':
        green_output = source
    elif fault == 'all_transparent':
        green_output[3::4] = bytes([0]) * WIDTH * HEIGHT
    elif fault == 'all_opaque':
        green_output[3::4] = bytes([255]) * WIDTH * HEIGHT
    elif fault == 'green_keys_red':
        green_output = keyed_pixels(1)
    elif fault == 'blue_keys_red':
        blue_output = keyed_pixels(1)
    elif fault == 'same_matte':
        blue_output = green_output
    elif fault == 'invert_green':
        green_output[3::4] = bytes(255 - value for value in green_output[3::4])
    elif fault == 'swap_outputs':
        green_output, blue_output = blue_output, green_output
    elif fault == 'rgb_damage':
        green_output[0] = 1
    elif fault == 'green_local_alpha':
        green_output[(HEIGHT // 2 * WIDTH + WIDTH // 2) * 4 + 3] = 0
    elif fault == 'blue_local_alpha':
        blue_output[(HEIGHT // 2 * WIDTH + WIDTH // 2) * 4 + 3] = 0
    else:
        blue_output = blue_output[:-4]
    with pytest.raises(AssertionError):
        assert_linear_color_key_response(green_output, blue_output)


def test_installed_bcc_obsolete_linear_color_key_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCLinearColorKey.aex'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    for slot, name, kind in (
            (6, 'Host Layer', 'layer'), (8, 'Output', 'integer'),
            (9, 'Color Matching', 'integer'), (10, 'Key Color', 'color'),
            (11, 'Similarity', 'float'), (15, 'Softness', 'float'),
            (22, 'Region of Interest', 'integer')):
        assert parameters[slot]['name'] == name
        assert parameters[slot]['kind'] == kind

    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), band_pixels()).save(source)
    outputs = []
    for name, color in (('green', [255, 0, 255, 0]),
                        ('blue', [255, 0, 0, 255])):
        request = tmp_path / f'{name}.json'
        output = tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(source)},
                {'slot': 8, 'value': 1},
                {'slot': 9, 'value': 1},
                {'slot': 10, 'color': color},
                {'slot': 11, 'value': 40},
                {'slot': 15, 'value': 0},
                {'slot': 17, 'value': 0},
                {'slot': 18, 'value': 1},
                {'slot': 19, 'value': 0},
                {'slot': 20, 'value': 0},
                {'slot': 22, 'value': 5},
                {'slot': 25, 'value': 2},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p for p in report['requested_parameters']}
        assert requested[8]['value'] == 1
        assert requested[9]['value'] == 1
        received_color = requested[10]['value']
        assert [received_color[channel]
                for channel in ('alpha', 'red', 'green', 'blue')] == color
        assert requested[11]['value'] == 40
        assert requested[15]['value'] == 0
        assert requested[22]['value'] == 5
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())

    assert_linear_color_key_response(*outputs)
