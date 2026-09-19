"""Legacy BCC Spill Remover response; not exact AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


COLORS = ((80, 200, 80, 255), (200, 80, 80, 255),
          (80, 80, 200, 255), (128, 128, 128, 255))


def band_pixels():
    return bytes(
        channel
        for _y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in COLORS[min(3, x * 4 // WIDTH)]
    )


def removed_spill_pixels():
    colors = ((80, 80, 80, 255), *COLORS[1:])
    return bytes(
        channel
        for _y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in colors[min(3, x * 4 // WIDTH)]
    )


def assert_spill_remover_response(neutral, active):
    source = band_pixels()
    assert neutral == source
    assert len(active) == WIDTH * HEIGHT * 4
    assert active[3::4] == bytes([255]) * WIDTH * HEIGHT
    first_row = active[:WIDTH * 4]
    assert all(active[y * WIDTH * 4:(y + 1) * WIDTH * 4] == first_row
               for y in range(1, HEIGHT))
    changed = 0
    for offset in range(0, len(source), 4):
        x = (offset // 4) % WIDTH
        original = source[offset:offset + 4]
        result = active[offset:offset + 4]
        if x < WIDTH // 4:
            assert result[0] == original[0]
            assert result[2] == original[2]
            assert 70 <= result[1] <= max(result[0], result[2])
            changed += result != original
        else:
            assert result == original
    assert changed == WIDTH * HEIGHT // 4


@pytest.mark.parametrize('fault', [
    'copy', 'uniform', 'green_remains', 'overremove', 'red_channel',
    'blue_channel', 'non_green', 'transparent', 'spatial', 'wrong_neutral',
    'truncated',
])
def test_spill_remover_validator_rejects_corruption(fault):
    neutral = band_pixels()
    active = bytearray(removed_spill_pixels())
    assert_spill_remover_response(neutral, active)
    if fault == 'copy':
        active = neutral
    elif fault == 'uniform':
        active = bytes((80, 80, 80, 255)) * WIDTH * HEIGHT
    elif fault == 'green_remains':
        active[1] = 200
    elif fault == 'overremove':
        active[1] = 0
    elif fault == 'red_channel':
        active[0] = 0
    elif fault == 'blue_channel':
        active[2] = 0
    elif fault == 'non_green':
        active[WIDTH] = 0
    elif fault == 'transparent':
        active[3] = 0
    elif fault == 'spatial':
        active[WIDTH * 4:WIDTH * 4 + 4] = COLORS[0]
    elif fault == 'wrong_neutral':
        neutral = active
    else:
        active = active[:-4]
    with pytest.raises(AssertionError):
        assert_spill_remover_response(neutral, active)


def test_installed_bcc_obsolete_spill_remover_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCSpillRemover.aex'
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
    assert parameters[10]['name'] == 'Screen Type'
    assert parameters[10]['kind'] == 'integer'
    assert parameters[10]['choices'][:3] == ['Green', 'Blue', 'Red']
    assert parameters[14]['name'] == 'Spill Ratio'
    assert parameters[14]['kind'] == 'float'
    assert parameters[17]['name'] == 'Amount'
    assert parameters[17]['kind'] == 'float'
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), band_pixels()).save(source)
    outputs = []
    for name, amount in (('neutral', 0), ('active', 100)):
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(source)},
                {'slot': 10, 'value': 1},
                {'slot': 14, 'value': 50},
                {'slot': 17, 'value': amount},
                {'slot': 26, 'value': 0},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[10] == 1
        assert requested[14] == 50
        assert requested[17] == amount
        assert requested[26] == 0
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_spill_remover_response(*outputs)
