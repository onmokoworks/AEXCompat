"""Legacy BCC Lens Transition zoom response; not exact AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_bcc_blur_response import source_pixels
from test_bcc_transition_slot2_response import assert_transition, reveal_pixels
from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def plain_midpoint():
    return Image.blend(
        Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()),
        Image.frombytes('RGBA', (WIDTH, HEIGHT), reveal_pixels()),
        0.5,
    ).tobytes()


def synthetic_zoom_midpoint():
    image = Image.new('RGBA', (WIDTH, HEIGHT), (0, 0, 0, 255))
    reveal = Image.frombytes('RGBA', (WIDTH, HEIGHT), reveal_pixels())
    image.paste(reveal.resize((160, 90), Image.Resampling.BILINEAR), (48, 27))
    return image.tobytes()


def assert_lens_transition(start, middle, end):
    assert_transition(start, middle, end)
    plain = plain_midpoint()
    significant = sum(
        max(abs(middle[offset + channel] - plain[offset + channel])
            for channel in range(3)) >= 8
        for offset in range(0, len(middle), 4)
    )
    assert significant >= WIDTH * HEIGHT * 9 // 10
    border, center = [], []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            pixel = middle[offset:offset + 3]
            if x < 32 or x >= 224 or y < 18 or y >= 126:
                border.append(pixel)
            if 64 <= x < 192 and 36 <= y < 108:
                center.append(pixel)
    assert sum(max(pixel) <= 8 for pixel in border) >= len(border) * 95 // 100
    assert sum(any(pixel) for pixel in center) >= len(center) * 95 // 100


@pytest.mark.parametrize('fault', [
    'crossfade_only', 'crossfade_noise', 'color_transform', 'fixed', 'transparent',
])
def test_lens_transition_validator_rejects_corruption(fault):
    source, reveal = source_pixels(), reveal_pixels()
    middle = bytearray(synthetic_zoom_midpoint())
    assert_lens_transition(source, middle, reveal)
    if fault == 'crossfade_only':
        middle = plain_midpoint()
    elif fault == 'crossfade_noise':
        middle = bytearray(plain_midpoint())
        middle[0] ^= 1
    elif fault == 'color_transform':
        middle = bytearray(plain_midpoint())
        for offset in range(0, len(middle), 4):
            for channel in range(3):
                middle[offset + channel] = max(0, middle[offset + channel] - 1)
    elif fault == 'fixed':
        middle = source
    else:
        middle[3] = 0
    with pytest.raises(AssertionError):
        assert_lens_transition(source, middle, reveal)


@pytest.mark.parametrize('plugin_name', (
    'BCCLensTransition.aex',
    'BCCLensTransitionPrTr.aex',
))
def test_installed_bcc_obsolete_lens_transition_zoom_response(
        tmp_path, plugin_name):
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
    assert parameters[3]['name'] == 'Transition Type'
    assert parameters[3]['choices'][1] == 'Zoom In'
    assert parameters[8]['name'] == 'Layer to Reveal'
    assert parameters[8]['kind'] == 'layer'
    assert parameters[9]['name'] == 'Animation'
    assert parameters[9]['choices'] == ['Auto', 'Pct. Done']
    assert parameters[10]['name'] == 'Percent Done'
    assert parameters[10]['minimum'] <= 0 < 50 < 100 <= parameters[10]['maximum']

    source, reveal = tmp_path / 'source.png', tmp_path / 'reveal.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    Image.frombytes('RGBA', (WIDTH, HEIGHT), reveal_pixels()).save(reveal)
    outputs = []
    for percent in (0, 50, 100):
        request, output = tmp_path / f'{percent}.json', tmp_path / f'{percent}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 3, 'value': 2},
                {'slot': 8, 'layer': str(reveal)},
                {'slot': 9, 'value': 2},
                {'slot': 10, 'value': percent},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[3] == 2 and requested[9] == 2 and requested[10] == percent
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_lens_transition(*outputs)
