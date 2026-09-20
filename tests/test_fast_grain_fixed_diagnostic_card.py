"""Classify the installed Fast Grain fixed diagnostic card; not render success."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


CARD_SHA256 = 'b15318f0673ca5cbd7b8365f81cc8391c1eb472865d263114cd1c24cf1347c10'


def source_pixels(alternate=False):
    if not alternate:
        return bytes((128, 128, 128, 255)) * (WIDTH * HEIGHT)
    colors = ((220, 20, 40, 255), (20, 200, 80, 255), (30, 70, 220, 255))
    return bytes(
        channel
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in colors[(x // 16 + y // 12) % len(colors)]
    )


def diagnostic_card_fixture():
    output = bytearray((0, 0, 255, 0) * (WIDTH * HEIGHT))
    for index in range(0, WIDTH * HEIGHT, 2):
        output[index * 4:index * 4 + 4] = bytes((0, 153, 255, 255))
    white_points = ([(x, 70) for x in range(92, 165)]
                    + [(x, 71) for x in range(92, 102)]
                    + [(92, 74), (164, 74)])
    for x, y in white_points:
        offset = (y * WIDTH + x) * 4
        output[offset:offset + 4] = bytes((255, 255, 255, 255))
    return output


def assert_fixed_diagnostic_card(source_a, source_b, output_a, output_b,
                                 expected_sha256=None):
    expected_length = WIDTH * HEIGHT * 4
    assert (len(source_a) == len(source_b) == len(output_a)
            == len(output_b) == expected_length)
    assert source_a != source_b
    assert output_a == output_b
    assert output_a != source_a and output_b != source_b
    if expected_sha256 is not None:
        assert hashlib.sha256(output_a).hexdigest() == expected_sha256

    pixels = [tuple(output_a[offset:offset + 4])
              for offset in range(0, len(output_a), 4)]
    assert {pixel[3] for pixel in pixels} == {0, 255}
    assert all(pixel[2] == 255 for pixel in pixels)
    assert all(pixel[0] == 0 or pixel[0] == pixel[1] for pixel in pixels)
    white_points = [
        (index % WIDTH, index // WIDTH)
        for index, pixel in enumerate(pixels)
        if pixel == (255, 255, 255, 255)
    ]
    assert len(white_points) == 85
    assert (min(x for x, _ in white_points), min(y for _, y in white_points),
            max(x for x, _ in white_points), max(y for _, y in white_points)) == (
                92, 70, 164, 74)


@pytest.mark.parametrize('mutation', [
    'passthrough', 'different_outputs', 'card_damage', 'opaque', 'truncated',
])
def test_fast_grain_card_validator_rejects_mutations(mutation):
    source_a = source_pixels()
    source_b = source_pixels(True)
    output_a = diagnostic_card_fixture()
    output_b = bytearray(output_a)
    assert_fixed_diagnostic_card(source_a, source_b, output_a, output_b)

    if mutation == 'passthrough':
        output_a = bytearray(source_a)
        output_b = bytearray(source_a)
    elif mutation == 'different_outputs':
        output_b[0] = 1
    elif mutation == 'card_damage':
        offset = (70 * WIDTH + 92) * 4
        output_a[offset:offset + 4] = output_b[offset:offset + 4] = bytes(
            (0, 153, 255, 255))
    elif mutation == 'opaque':
        output_a[3::4] = output_b[3::4] = bytes([255]) * (WIDTH * HEIGHT)
    elif mutation == 'truncated':
        output_a = output_a[:-4]
        output_b = output_b[:-4]

    with pytest.raises(AssertionError):
        assert_fixed_diagnostic_card(source_a, source_b, output_a, output_b)


def test_installed_fast_grain_returns_fixed_diagnostic_card(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_FAST_GRAIN')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_FAST_GRAIN to installed Fast Grain.aex')
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
            (1, 'Intensity', 'float'), (2, 'Size', 'float'),
            (3, 'Grain Color', 'float'), (4, 'Frame Rate', 'float'),
            (8, 'Grain Response', 'arbitrary_data'),
            (13, 'Blend Mode', 'integer'), (14, 'Blend Opacity', 'float')):
        assert parameters[slot]['name'] == name
        assert parameters[slot]['kind'] == kind

    sources = (source_pixels(), source_pixels(True))
    outputs = {}
    for source_index, pixels in enumerate(sources):
        for intensity in (0, 100):
            label = f'{source_index}-{intensity}'
            source = tmp_path / f'source-{label}.png'
            output = tmp_path / f'output-{label}.png'
            request = tmp_path / f'request-{label}.json'
            Image.frombytes('RGBA', (WIDTH, HEIGHT), pixels).save(source)
            assignments = (
                (1, intensity), (2, 1.25), (3, 0), (4, 24),
                (9, 1), (10, 0), (11, 0), (13, 3), (14, 100),
            )
            request.write_text(json.dumps({
                'schema_version': 1,
                'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
                'assignments': [
                    {'slot': slot, 'value': value} for slot, value in assignments
                ],
            }), encoding='utf-8')
            report = run('--render-experimental-smart-request', plugin, source,
                         output, request)
            assert report['passed']
            assert report['worker_classification'] == 'ok'
            assert report['output_pixels_valid']
            requested = {
                p['slot']: p['value'] for p in report['requested_parameters']
            }
            for slot, value in assignments:
                assert requested[slot] == value
            with Image.open(output) as image:
                assert image.size == (WIDTH, HEIGHT)
                outputs[(source_index, intensity)] = image.convert(
                    'RGBA').tobytes()

    assert len(set(outputs.values())) == 1
    assert_fixed_diagnostic_card(
        sources[0], sources[1], outputs[(0, 0)], outputs[(1, 100)], CARD_SHA256)
