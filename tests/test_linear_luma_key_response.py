"""Linear Luma Key threshold response; not a full AE-equivalence claim."""
import json
import os
import subprocess

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def ramp_pixels():
    return bytes(c for _y in range(HEIGHT) for x in range(WIDTH)
                 for c in (min(x, 254), min(x, 254), min(x, 254), 255))


def assert_threshold_response(pixels, boundary):
    assert len(pixels) == WIDTH * HEIGHT * 4
    source = ramp_pixels()
    for y in range(HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            opaque = x >= boundary
            assert pixels[offset + 3] == (255 if opaque else 0)
            if opaque:
                assert pixels[offset:offset + 3] == source[offset:offset + 3]


@pytest.mark.parametrize('fault', ['passthrough', 'empty', 'inverted', 'boundary',
                                   'rgb', 'row', 'truncated'])
def test_linear_luma_validator_rejects_corruption(fault):
    pixels = bytearray(ramp_pixels())
    pixels[3::4] = bytes(255 if x >= 64 else 0
                         for _y in range(HEIGHT) for x in range(WIDTH))
    assert_threshold_response(pixels, 64)
    if fault == 'passthrough':
        pixels = ramp_pixels()
    elif fault == 'empty':
        pixels[3::4] = bytes(WIDTH * HEIGHT)
    elif fault == 'inverted':
        pixels[3::4] = bytes(255 - value for value in pixels[3::4])
    elif fault == 'boundary':
        pixels[64 * 4 + 3] = 0
    elif fault == 'rgb':
        pixels[128 * 4] += 1
    elif fault == 'row':
        pixels[(WIDTH + 200) * 4 + 3] = 0
    else:
        pixels = pixels[:-4]
    with pytest.raises(AssertionError):
        assert_threshold_response(pixels, 64)


def test_installed_linear_luma_key_tracks_threshold(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_LINEAR_LUMA_KEY')
    if not plugin:
        pytest.skip('requires AEXCOMPAT_TEST_LINEAR_LUMA_KEY')
    assert os.name == 'nt'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert parameters[186]['name'] == 'View'
    assert parameters[187]['name'] == 'Channel'
    assert parameters[188]['name'] == 'Output'
    assert parameters[190]['name'] == 'Key Type'
    assert parameters[191]['name'] == 'Threshold'
    assert parameters[193]['name'] == 'Softness'
    assert parameters[199]['name'] == 'Region of Interest'
    assert parameters[204]['name'] == 'GPU Rendering'

    source = tmp_path / 'ramp.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), ramp_pixels()).save(source)
    for threshold, boundary in ((25, 64), (75, 192)):
        request = tmp_path / f'request-{threshold}.json'
        output = tmp_path / f'output-{threshold}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 186, 'value': 1},
                {'slot': 187, 'value': 1},
                {'slot': 188, 'value': 1},
                {'slot': 190, 'value': 2},
                {'slot': 191, 'value': threshold},
                {'slot': 193, 'value': 0},
                {'slot': 199, 'value': 5},
                {'slot': 204, 'value': 4},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output, request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[191] == threshold
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            assert_threshold_response(image.convert('RGBA').tobytes(), boundary)
