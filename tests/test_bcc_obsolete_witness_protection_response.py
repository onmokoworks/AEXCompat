"""Legacy BCC Witness Protection localized mosaic response; not exact AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def pattern_pixels():
    return bytes(
        channel
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in (
            (x * 29 + y * 17) % 256,
            (x * 11 + y * 31) % 256,
            (x * 43 + y * 7) % 256,
            255,
        )
    )


def witness_pixels(center=(WIDTH / 2, HEIGHT / 2), radius=64,
                   color=(128, 128, 128), square=False):
    source = pattern_pixels()
    output = bytearray(source)
    center_x, center_y = center
    for y in range(HEIGHT):
        for x in range(WIDTH):
            delta_x, delta_y = x - center_x, y - center_y
            selected = (max(abs(delta_x), abs(delta_y)) <= radius if square
                        else delta_x ** 2 + delta_y ** 2 <= radius ** 2)
            if selected:
                offset = (y * WIDTH + x) * 4
                output[offset:offset + 3] = bytes(color)
    return output


def assert_witness_protection_response(neutral, active):
    source = pattern_pixels()
    assert neutral == source
    assert len(active) == WIDTH * HEIGHT * 4
    assert active[3::4] == bytes([255]) * WIDTH * HEIGHT

    changed = []
    core = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            distance_squared = ((x - WIDTH / 2) ** 2 +
                                (y - HEIGHT / 2) ** 2)
            if active[offset:offset + 4] != source[offset:offset + 4]:
                changed.append((x, y))
            if distance_squared <= 40 ** 2:
                core.append(active[offset:offset + 3])
            elif distance_squared >= 66 ** 2:
                assert active[offset:offset + 4] == source[offset:offset + 4]

    assert 12_000 <= len(changed) <= 14_000
    bounds = (min(x for x, _ in changed), min(y for _, y in changed),
              max(x for x, _ in changed), max(y for _, y in changed))
    assert 62 <= bounds[0] <= 66 and 6 <= bounds[1] <= 10
    assert 189 <= bounds[2] <= 193 and 133 <= bounds[3] <= 137
    for channel in range(3):
        values = [pixel[channel] for pixel in core]
        assert max(values) - min(values) <= 2
        assert 110 <= sum(values) / len(values) <= 145
    assert all(max(pixel) - min(pixel) <= 2 for pixel in core)


@pytest.mark.parametrize('fault', [
    'copy', 'global_flatten', 'uniform', 'small_mask', 'square_mask',
    'wrong_center', 'colored_core', 'transparent', 'outside_damage',
    'mirrored', 'wrong_neutral', 'truncated',
])
def test_witness_protection_validator_rejects_corruption(fault):
    neutral = pattern_pixels()
    active = witness_pixels()
    assert_witness_protection_response(neutral, active)
    if fault == 'copy':
        active = neutral
    elif fault == 'global_flatten':
        active = bytes((128, 128, 128, 255)) * WIDTH * HEIGHT
    elif fault == 'uniform':
        active = bytes((24, 24, 24, 255)) * WIDTH * HEIGHT
    elif fault == 'small_mask':
        active = witness_pixels(radius=20)
    elif fault == 'square_mask':
        active = witness_pixels(square=True)
    elif fault == 'wrong_center':
        active = witness_pixels(center=(80, HEIGHT / 2))
    elif fault == 'colored_core':
        active = witness_pixels(color=(96, 128, 160))
    elif fault == 'transparent':
        active[3] = 0
    elif fault == 'outside_damage':
        active[0:3] = bytes((255, 255, 255))
    elif fault == 'mirrored':
        mirrored = bytearray(len(active))
        for y in range(HEIGHT):
            for x in range(WIDTH):
                source_offset = (y * WIDTH + x) * 4
                destination = (y * WIDTH + WIDTH - 1 - x) * 4
                mirrored[destination:destination + 4] = active[source_offset:source_offset + 4]
        active = mirrored
    elif fault == 'wrong_neutral':
        neutral = active
    else:
        active = active[:-4]
    with pytest.raises(AssertionError):
        assert_witness_protection_response(neutral, active)


def test_installed_bcc_obsolete_witness_protection_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCWitnessProtection.aex'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert parameters[2]['name'] == 'Effect Method'
    assert parameters[2]['kind'] == 'integer'
    assert parameters[13]['name'] == 'Shape'
    assert parameters[15]['name'] == 'Center'
    assert parameters[15]['kind'] == 'point'
    assert parameters[18]['name'] == 'Region Radius'
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), pattern_pixels()).save(source)
    outputs = []
    for name, mix in (('neutral', 100), ('mosaic', 0)):
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 2, 'value': 2},
                {'slot': 4, 'value': 80},
                {'slot': 10, 'value': 100},
                {'slot': 11, 'value': mix},
                {'slot': 13, 'value': 1},
                {'slot': 15, 'components': [50, 50]},
                {'slot': 18, 'value': 25},
                {'slot': 20, 'value': 0},
                {'slot': 21, 'value': 0},
                {'slot': 22, 'value': 1},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p for p in report['requested_parameters']}
        assert requested[2]['value'] == 2
        assert requested[4]['value'] == 80
        assert requested[10]['value'] == 100
        assert requested[11]['value'] == mix
        assert requested[13]['value'] == 1
        assert requested[15]['value'][:2] == [50, 50]
        assert requested[18]['value'] == 25
        assert requested[20]['value'] == 0
        assert requested[21]['value'] == 0
        assert requested[22]['value'] == 1
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())

    assert_witness_protection_response(*outputs)
