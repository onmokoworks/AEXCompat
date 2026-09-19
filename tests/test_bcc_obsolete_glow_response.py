"""Legacy BCC glow contribution response; not exact kernel or AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image, ImageChops, ImageFilter

from test_bcc_blur_response import source_pixels
from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


PLUGINS = (
    ('BCCFastFilmGlow.aex', 10, 'Glow Intensity', 62),
    ('BCCFilmGlow.aex', 10, 'Glow Intensity', 62),
    ('BCCRoughGlow.aex', 18, 'Glow Opacity Scale', 100),
)


def assert_glow_response(zero, active):
    source = source_pixels()
    assert zero == source
    assert len(active) == WIDTH * HEIGHT * 4
    assert active[3::4] == bytes([255]) * WIDTH * HEIGHT
    outside_lit = near_lit = outside_mixed = changed = 0
    outside_colors = set()
    for y in range(HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            pixel = active[offset:offset + 4]
            expected = source[offset:offset + 4]
            inside = 64 <= x < 192 and 36 <= y < 108
            if inside:
                assert pixel == bytes((255, 255, 255, 255))
                continue
            changed += pixel != expected
            level = max(pixel[:3])
            if not level:
                continue
            outside_lit += 1
            outside_colors.add(bytes(pixel[:3]))
            outside_mixed += level < 255
            dx = 0 if 64 <= x < 192 else min(abs(x - 64), abs(x - 191))
            dy = 0 if 36 <= y < 108 else min(abs(y - 36), abs(y - 107))
            distance = (dx * dx + dy * dy) ** 0.5
            near_lit += distance <= 24
            assert distance <= 32
    assert changed >= 1000
    assert outside_lit >= 1000
    assert outside_mixed >= 500
    assert len(outside_colors) >= 8
    assert near_lit >= outside_lit * 95 // 100
    for x, y in ((0, 0), (WIDTH - 1, 0), (0, HEIGHT - 1),
                 (WIDTH - 1, HEIGHT - 1)):
        offset = (y * WIDTH + x) * 4
        assert active[offset:offset + 4] == bytes((0, 0, 0, 255))


def synthetic_glow():
    source = Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels())
    blurred = source.convert('L').filter(ImageFilter.GaussianBlur(6)).convert('RGBA')
    blurred.putalpha(255)
    return ImageChops.lighter(source, blurred).tobytes()


@pytest.mark.parametrize('fault', [
    'copy', 'transparent', 'darken_inside', 'remote', 'uniform',
    'constant_ring', 'truncated',
])
def test_glow_validator_rejects_corruption(fault):
    zero, active = source_pixels(), bytearray(synthetic_glow())
    assert_glow_response(zero, active)
    if fault == 'copy':
        active = zero
    elif fault == 'transparent':
        active[3] = 0
    elif fault == 'darken_inside':
        offset = (36 * WIDTH + 64) * 4
        active[offset:offset + 3] = bytes((254, 254, 254))
    elif fault == 'remote':
        active[0:4] = bytes((1, 1, 1, 255))
    elif fault == 'uniform':
        active = bytes((64, 64, 64, 255)) * WIDTH * HEIGHT
    elif fault == 'constant_ring':
        active = bytearray(source_pixels())
        for y in range(28, 116):
            for x in range(56, 200):
                if 64 <= x < 192 and 36 <= y < 108:
                    continue
                offset = (y * WIDTH + x) * 4
                active[offset:offset + 4] = bytes((64, 64, 64, 255))
    else:
        active = active[:-4]
    with pytest.raises(AssertionError):
        assert_glow_response(zero, active)


@pytest.mark.parametrize('plugin_name,slot,label,amount', PLUGINS)
def test_installed_bcc_obsolete_glow_response(
        tmp_path, plugin_name, slot, label, amount):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
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
    assert parameters[6]['name'] == 'Host Layer'
    assert parameters[6]['kind'] == 'layer'
    assert parameters[slot]['name'] == label
    assert parameters[slot]['kind'] == 'float'
    assert parameters[slot]['minimum'] <= 0 < amount <= parameters[slot]['maximum']

    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for value in (0, amount):
        request, output = tmp_path / f'{value}.json', tmp_path / f'{value}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(source)},
                {'slot': slot, 'value': value},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[slot] == value
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())

    assert_glow_response(*outputs)
