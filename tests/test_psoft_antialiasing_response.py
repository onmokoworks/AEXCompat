"""PSOFT antialiasing response on a binary stair edge; not AE parity."""
import json
import os
import subprocess

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


BOUNDARY_LEFT = 56


def stair_pixels():
    return bytes(
        channel
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in ((255, 255, 255, 255)
                        if x < BOUNDARY_LEFT + y else (0, 0, 0, 255))
    )


def antialiased_stair_pixels():
    pixels = bytearray(stair_pixels())
    for y in range(HEIGHT):
        boundary = BOUNDARY_LEFT + y
        for x, value in ((boundary - 1, 229), (boundary, 25)):
            offset = (y * WIDTH + x) * 4
            pixels[offset:offset + 4] = bytes((value, value, value, 255))
    return bytes(pixels)


def assert_antialias_response(source, output):
    assert len(source) == len(output) == WIDTH * HEIGHT * 4
    assert output != source
    assert output[0::4] == output[1::4] == output[2::4]
    assert output[3::4] == bytes([255]) * WIDTH * HEIGHT

    changed = set()
    for y in range(HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            if output[offset:offset + 4] != source[offset:offset + 4]:
                changed.add((x, y))

    expected = {
        (x, y)
        for y in range(HEIGHT)
        for x in (BOUNDARY_LEFT + y - 1, BOUNDARY_LEFT + y)
    }
    assert changed == expected
    for y in range(HEIGHT):
        boundary = BOUNDARY_LEFT + y
        white_edge = output[(y * WIDTH + boundary - 1) * 4]
        black_edge = output[(y * WIDTH + boundary) * 4]
        # The observed row phases use two white-edge levels, and both keep the
        # originally white side well above the black side.
        assert 150 <= white_edge <= 235
        assert 20 <= black_edge <= 35
        assert white_edge > black_edge


@pytest.mark.parametrize('mutation', [
    'copy', 'constant', 'off_boundary', 'missing_edge', 'shifted', 'reversed',
    'color', 'alpha', 'truncated',
])
def test_psoft_antialiasing_validator_rejects_mutations(mutation):
    source = stair_pixels()
    output = bytearray(antialiased_stair_pixels())
    assert_antialias_response(source, output)

    if mutation == 'copy':
        output = bytearray(source)
    elif mutation == 'constant':
        output = bytearray((128, 128, 128, 255) * (WIDTH * HEIGHT))
    elif mutation == 'off_boundary':
        output[0:4] = bytes((128, 128, 128, 255))
    elif mutation == 'missing_edge':
        boundary = BOUNDARY_LEFT
        offset = (boundary - 1) * 4
        output[offset:offset + 8] = source[offset:offset + 8]
    elif mutation == 'shifted':
        boundary = BOUNDARY_LEFT
        offset = (boundary - 1) * 4
        output[offset:offset + 8] = source[offset:offset + 8]
        offset = (boundary + 1) * 4
        output[offset:offset + 4] = bytes((25, 25, 25, 255))
    elif mutation == 'reversed':
        for y in range(HEIGHT):
            boundary = BOUNDARY_LEFT + y
            white_offset = (y * WIDTH + boundary - 1) * 4
            black_offset = (y * WIDTH + boundary) * 4
            output[white_offset:white_offset + 4] = bytes((25, 25, 25, 255))
            output[black_offset:black_offset + 4] = bytes((229, 229, 229, 255))
    elif mutation == 'color':
        output[(BOUNDARY_LEFT - 1) * 4 + 1] = 10
    elif mutation == 'alpha':
        output[(BOUNDARY_LEFT - 1) * 4 + 3] = 0
    elif mutation == 'truncated':
        output = output[:-4]

    with pytest.raises(AssertionError):
        assert_antialias_response(source, output)


def test_installed_psoft_antialiasing_response(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_PSOFT_ANTIALIASING')
    if not plugin:
        pytest.skip(
            'set AEXCOMPAT_TEST_PSOFT_ANTIALIASING to installed antialiasing.aex')
    assert os.name == 'nt'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    for slot, name, kind in (
            (1, 'color threshold', 'integer'), (2, 'softness', 'integer'),
            (3, 'bias', 'integer'), (5, 'color key enable', 'integer'),
            (6, 'key color', 'color'), (7, 'key color threshold', 'integer'),
            (8, 'invert', 'integer')):
        assert parameters[slot]['name'] == name
        assert parameters[slot]['kind'] == kind

    source_pixels = stair_pixels()
    source = tmp_path / 'stair.png'
    output = tmp_path / 'antialiased.png'
    request = tmp_path / 'request.json'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels).save(source)
    assignments = ((1, 10), (2, 70), (3, 0), (5, 0), (8, 0))
    request.write_text(json.dumps({
        'schema_version': 1,
        'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
        'assignments': [
            {'slot': slot, 'value': value} for slot, value in assignments
        ],
    }), encoding='utf-8')

    report = run('--render-experimental-smart-request', plugin, source, output,
                 request)
    assert report['passed']
    assert report['worker_classification'] == 'ok'
    assert report['output_pixels_valid']
    assert output.is_file() and output.stat().st_size > 0
    requested = {p['slot']: p['value'] for p in report['requested_parameters']}
    for slot, value in assignments:
        assert requested[slot] == value
    with Image.open(output) as image:
        assert image.size == (WIDTH, HEIGHT)
        output_pixels = image.convert('RGBA').tobytes()
    assert_antialias_response(source_pixels, output_pixels)
