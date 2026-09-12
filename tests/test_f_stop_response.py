"""Master red exposure response, not an AE transfer-curve oracle."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_brightness_response import source_pixels
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def assert_red_exposure(neutral, brighter, darker):
    source = source_pixels()
    assert neutral == source
    for pixels, direction in ((brighter, 1), (darker, -1)):
        assert len(pixels) == len(source)
        for channel in (1, 2, 3):
            assert pixels[channel::4] == source[channel::4]
        assert all((a - b) * direction > 0 for a, b in zip(pixels[0::4], source[0::4]))
        pairs = sorted(set(zip(source[0::4], pixels[0::4])))
        assert len({a for a, _ in pairs}) == len(pairs)
        levels = [b for _, b in pairs]
        assert len(set(levels)) > 1  # Reject flat fills, not a quality threshold.
        assert all(a <= b for a, b in zip(levels, levels[1:]))


@pytest.mark.parametrize('fault', [
    'copy', 'reversed', 'green', 'blue', 'alpha', 'flat', 'nonmonotonic',
    'inconsistent', 'truncated', 'neutral',
])
def test_red_exposure_validator_rejects_corruption(fault):
    source = source_pixels()
    brighter, darker = bytearray(source), bytearray(source)
    for i in range(0, len(source), 4):
        brighter[i] += 20
        darker[i] -= 20
    neutral = source
    assert_red_exposure(neutral, brighter, darker)
    if fault == 'copy':
        brighter = source
    elif fault == 'reversed':
        brighter, darker = darker, brighter
    elif fault in ('green', 'blue', 'alpha'):
        brighter[{'green': 1, 'blue': 2, 'alpha': 3}[fault]] -= 1
    elif fault == 'flat':
        brighter[0::4] = bytes([255]) * (WIDTH * HEIGHT)
    elif fault == 'nonmonotonic':
        for i in range(0, len(source), 4):
            brighter[i] = 255 - source[i]
    elif fault == 'inconsistent':
        brighter[0] += 1
    elif fault == 'truncated':
        darker = darker[:-4]
    else:
        neutral = brighter
    with pytest.raises(AssertionError):
        assert_red_exposure(neutral, brighter, darker)


def test_real_f_stop_red_exposure(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_F_STOP')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_F_STOP to the local AEX')
    assert os.name == 'nt'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run(
            [str(harness), '--headless', *map(str, args)], cwd=ROOT,
            capture_output=True,
            timeout=None if args[0] == '--inspect-experimental' else 90,
        )
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = run('--inspect-experimental', plugin)
    slots = {p['slot']: p for p in params}
    assert slots[181]['name'] == 'Host Layer' and slots[181]['kind'] == 'layer'
    groups, paths = [], {}
    for p in params:
        if p['kind'] == 'group_start':
            groups.append(p['name'])
        elif p['kind'] == 'group_end':
            groups.pop()
        else:
            paths[p['slot']] = tuple(groups)
    for slot, name in ((189, 'Red Exposure'), (190, 'Green Exposure'), (191, 'Blue Exposure')):
        assert slots[slot]['name'] == name and slots[slot]['value'] == 0
        assert paths[slot] == ('Master',)
    assert slots[192]['name'] == 'Gang' and slots[192]['value'] == 0
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for exposure in (0, 1, -1):
        request, output = tmp_path / f'{exposure}.json', tmp_path / f'{exposure}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 181, 'layer': str(source)}, {'slot': 189, 'value': exposure},
                {'slot': 190, 'value': 0}, {'slot': 191, 'value': 0}, {'slot': 192, 'value': 0},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output, request)
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert('RGBA').tobytes()
        argb = bytearray(len(pixels))
        argb[0::4], argb[1::4] = pixels[3::4], pixels[0::4]
        argb[2::4], argb[3::4] = pixels[1::4], pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        outputs.append(pixels)
    assert_red_exposure(*outputs)
