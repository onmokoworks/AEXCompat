"""Legacy BCC spatial blur controls respond; not exact kernel or AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image, ImageFilter

from test_bcc_blur_response import source_pixels
from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


PLUGINS = (
    ('BCCFastLensBlur.aex', 9, 'Iris Scale', 'float', 20),
    ('BCCLensBlur.aex', 9, 'Iris Scale', 'float', 20),
    ('BCCRadialBlur.aex', 10, 'Blur Amount', 'float', 30),
    ('BCCSpiralBlur.aex', 10, 'Spin Angle', 'angle', 45),
)


def assert_spatial_blur(zero, active):
    source = source_pixels()
    assert zero == source
    assert len(active) == WIDTH * HEIGHT * 4
    assert all(active[3::4])
    assert active[0::4] == active[1::4] == active[2::4]
    changed = sum(active[offset:offset + 4] != source[offset:offset + 4]
                  for offset in range(0, len(active), 4))
    assert 100 < changed < WIDTH * HEIGHT // 2
    colors = {bytes(active[offset:offset + 4])
              for offset in range(0, len(active), 4)}
    assert len(colors) > 8
    assert sum(active[offset] not in (0, 255)
               for offset in range(0, len(active), 4)) > 100
    near_edge = far_field = inside_mix = outside_mix = 0
    for y in range(HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            if active[offset:offset + 4] == source[offset:offset + 4]:
                continue
            inside = 64 <= x <= 191 and 36 <= y <= 107
            if inside:
                distance = min(x - 64, 191 - x, y - 36, 107 - y)
            else:
                dx = 0 if 64 <= x <= 191 else min(abs(x - 64), abs(x - 191))
                dy = 0 if 36 <= y <= 107 else min(abs(y - 36), abs(y - 107))
                distance = (dx * dx + dy * dy) ** 0.5
            near_edge += distance <= 24
            far_field += distance > 32
            if 0 < active[offset] < 255:
                if inside:
                    inside_mix += 1
                else:
                    outside_mix += 1
    assert near_edge >= changed * 95 // 100
    assert far_field < WIDTH * HEIGHT // 100
    assert inside_mix > 100 and outside_mix > 100
    for x, y in ((0, 0), (WIDTH - 1, 0), (0, HEIGHT - 1),
                 (WIDTH - 1, HEIGHT - 1)):
        offset = (y * WIDTH + x) * 4
        assert active[offset:offset + 4] == bytes((0, 0, 0, 255))


@pytest.mark.parametrize('fault', [
    'copy', 'transparent', 'color', 'truncated', 'constant', 'corner',
    'remote_gradient', 'wrong_zero',
])
def test_spatial_blur_validator_rejects_corruption(fault):
    zero = source_pixels()
    active = bytearray(Image.frombytes('RGBA', (WIDTH, HEIGHT), zero).filter(
        ImageFilter.GaussianBlur(4)).tobytes())
    assert_spatial_blur(zero, active)
    if fault == 'copy':
        active = zero
    elif fault == 'transparent':
        active[3] = 0
    elif fault == 'color':
        active[0] = 1
    elif fault == 'truncated':
        active = active[:-4]
    elif fault == 'constant':
        active = bytes((64, 64, 64, 255)) * WIDTH * HEIGHT
    elif fault == 'corner':
        active[0:4] = bytes((1, 1, 1, 255))
    elif fault == 'remote_gradient':
        for y in range(16):
            for x in range(16, WIDTH - 16):
                offset = (y * WIDTH + x) * 4
                value = 16 + x % 64
                active[offset:offset + 4] = bytes((value, value, value, 255))
    else:
        zero = active
    with pytest.raises(AssertionError):
        assert_spatial_blur(zero, active)


@pytest.mark.parametrize('plugin_name,slot,label,kind,amount', PLUGINS)
def test_installed_bcc_obsolete_spatial_blur_response(
        tmp_path, plugin_name, slot, label, kind, amount):
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
    assert parameters[slot]['kind'] == kind

    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for value in (0, amount):
        request, output = tmp_path / f'{value}.json', tmp_path / f'{value}.png'
        control = ({'slot': slot, 'components': [value]} if kind == 'angle'
                   else {'slot': slot, 'value': value})
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 6, 'layer': str(source)}, control],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p for p in report['requested_parameters']}
        if kind == 'angle':
            assert requested[slot]['value'][0] == value
        else:
            assert requested[slot]['value'] == value
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_spatial_blur(*outputs)
