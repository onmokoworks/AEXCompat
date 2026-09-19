"""BCC slot-8 transitions follow manual progress; not exact AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_bcc_blur_response import source_pixels
from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


PLUGINS = (
    'Atmospheric Glow Dissolve.aex',
    'Blur Dissolve.aex',
    'Camera Shake Dissolve.aex',
    'Channel Blur Dissolve.aex',
    'Crash Zoom Dissolve.aex',
    'Cross Zoom Dissolve.aex',
    'Depth Wipe ML.aex',
    'Directional Blur Dissolve.aex',
    'Dissolve.aex',
    'Film Glow Dissolve.aex',
    'Film Roll.aex',
    'Jump Cut Fixer ML.aex',
    'Lens Flare Dissolve.aex',
    'Light Leaks Dissolve.aex',
    'Linear Wipe.aex',
    'Mosaic Dissolve.aex',
    'Multi-Star Dissolve.aex',
    'Orbs Dissolve.aex',
    'Rack Focus Dissolve.aex',
    'Radial Wipe.aex',
    'Rays Dissolve.aex',
    'Rectangular Wipe.aex',
    'Ripple Dissolve.aex',
    'Smoke Wipe.aex',
    'Smoke and Fog Dissolve.aex',
    'Spin Blur Dissolve.aex',
    'Swish Glow.aex',
    'Swish Pan.aex',
    'Swish Prism.aex',
    'Swish Warp.aex',
    'Texture Wipe.aex',
    'Video Glitch Dissolve.aex',
    'Vignette Wipe.aex',
)


def reveal_pixels():
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (16 + x // 2, 48 + y, 224 - x // 3, 255))


def rgb_distance(left, right):
    assert len(left) == len(right)
    return sum(abs(left[offset + channel] - right[offset + channel])
               for offset in range(0, len(left), 4)
               for channel in range(3))


def assert_transition(start, middle, end):
    source, reveal = source_pixels(), reveal_pixels()
    assert len(start) == len(middle) == len(end) == WIDTH * HEIGHT * 4
    assert all(start[3::4]) and all(middle[3::4]) and all(end[3::4])
    assert len({bytes(start), bytes(middle), bytes(end)}) == 3
    assert rgb_distance(start, source) < rgb_distance(start, reveal)
    assert rgb_distance(end, reveal) < rgb_distance(end, source)
    assert len({bytes(middle[offset:offset + 4])
                for offset in range(0, len(middle), 4)}) > 1
    assert any(middle[offset:offset + 3] != source[offset:offset + 3]
               and middle[offset:offset + 3] != reveal[offset:offset + 3]
               for offset in range(0, len(middle), 4))


@pytest.mark.parametrize('fault', [
    'wrong_start', 'wrong_end', 'fixed_start', 'fixed_end', 'constant',
    'transparent', 'truncated',
])
def test_transition_cohort_validator_rejects_corruption(fault):
    source, reveal = source_pixels(), reveal_pixels()
    middle = bytearray(Image.blend(
        Image.frombytes('RGBA', (WIDTH, HEIGHT), source),
        Image.frombytes('RGBA', (WIDTH, HEIGHT), reveal), 0.5).tobytes())
    start, end = source, reveal
    assert_transition(start, middle, end)
    if fault == 'wrong_start':
        start = reveal
    elif fault == 'wrong_end':
        end = source
    elif fault == 'fixed_start':
        middle = source
    elif fault == 'fixed_end':
        middle = reveal
    elif fault == 'constant':
        middle = bytes((32, 64, 128, 255)) * WIDTH * HEIGHT
    elif fault == 'transparent':
        middle[3::4] = bytes(WIDTH * HEIGHT)
    else:
        middle = middle[:-4]
    with pytest.raises(AssertionError):
        assert_transition(start, middle, end)


@pytest.mark.parametrize('plugin_name', PLUGINS)
def test_installed_bcc_slot8_transition_tracks_manual_percent(tmp_path, plugin_name):
    directory = os.environ.get('AEXCOMPAT_TEST_BCC_TRANSITION_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_BCC_TRANSITION_DIR')
    assert os.name == 'nt'
    plugin = Path(directory) / plugin_name
    assert plugin.is_file()
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert parameters[7]['name'] == 'Animation'
    assert parameters[7]['choices'] == ['Auto', 'Manual Pct Done']
    assert parameters[8]['name'] == 'Layer to Reveal'
    assert parameters[8]['kind'] == 'layer'
    assert parameters[9]['name'] == 'Percent Done'
    assert parameters[9]['minimum'] <= 0 < 50 < 100 <= parameters[9]['maximum']

    source, reveal = tmp_path / 'source.png', tmp_path / 'reveal.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    Image.frombytes('RGBA', (WIDTH, HEIGHT), reveal_pixels()).save(reveal)
    outputs = []
    for percent in (0, 50, 100):
        request, output = tmp_path / f'{percent}.json', tmp_path / f'{percent}.png'
        assignments = [
            {'slot': 7, 'value': 2},
            {'slot': 8, 'layer': str(reveal)},
            {'slot': 9, 'value': percent},
        ]
        if plugin_name == 'Atmospheric Glow Dissolve.aex':
            assignments.append({'slot': 10, 'value': 1})
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': assignments,
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[7] == 2 and requested[9] == percent
        if plugin_name == 'Atmospheric Glow Dissolve.aex':
            assert requested[10] == 1
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_transition(*outputs)
