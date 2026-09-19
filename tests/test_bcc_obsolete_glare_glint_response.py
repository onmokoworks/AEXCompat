"""Legacy BCC glare/glint ray response; not exact kernel or AE parity."""
import json
import math
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image, ImageChops, ImageDraw, ImageFilter

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


PLUGINS = (
    ('BCCGlare.aex', 'Glare', 25, 'Glare Ray Counts', ((16, 'Glare Ring On', 0),)),
    ('BCCGlint.aex', 'Glint', 19, 'Glint Counts', ()),
)


def emitter_pixels():
    return bytes(
        channel
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in (
            (255, 255, 255, 255)
            if 112 <= x < 144 and 56 <= y < 88 else (0, 0, 0, 255)
        )
    )


def ray_metrics(raw):
    assert len(raw) == WIDTH * HEIGHT * 4
    assert raw[3::4] == bytes([255]) * WIDTH * HEIGHT
    lit = []
    axis_levels = []
    diagonal_levels = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            level = max(raw[offset:offset + 3])
            if level:
                lit.append((x, y, bytes(raw[offset:offset + 3])))
            dx, dy = x - 128, y - 72
            radius = (dx * dx + dy * dy) ** 0.5
            if not 30 < radius <= 60:
                continue
            if abs(dx) <= 1 or abs(dy) <= 1:
                axis_levels.append(level)
            if abs(abs(dx) - abs(dy)) <= 1:
                diagonal_levels.append(level)
    return {
        'lit': len(lit),
        'colors': len({color for _, _, color in lit}),
        'bounds': (
            min(x for x, _, _ in lit), min(y for _, y, _ in lit),
            max(x for x, _, _ in lit), max(y for _, y, _ in lit),
        ),
        'axis_mean': sum(axis_levels) / len(axis_levels),
        'diagonal_mean': sum(diagonal_levels) / len(diagonal_levels),
    }


def directional_mean(raw, offset_degrees):
    levels = []
    for radius in range(24, 41):
        for quarter in range(4):
            angle = math.radians(offset_degrees + quarter * 90)
            x = round(128 + math.cos(angle) * radius)
            y = round(72 + math.sin(angle) * radius)
            pixel = (y * WIDTH + x) * 4
            levels.append(max(raw[pixel:pixel + 3]))
    return sum(levels) / len(levels)


def dominant_direction(raw):
    return max(range(90), key=lambda angle: directional_mean(raw, angle))


def assert_ray_response(neutral, axis, rotated):
    source = emitter_pixels()
    assert neutral == source
    axis_metrics = ray_metrics(axis)
    rotated_metrics = ray_metrics(rotated)
    for metrics in (axis_metrics, rotated_metrics):
        assert metrics['lit'] >= 5000
        assert metrics['colors'] >= 8
        left, top, right, bottom = metrics['bounds']
        assert right - left >= 95 and bottom - top >= 95
    assert axis_metrics['axis_mean'] >= axis_metrics['diagonal_mean'] * 1.5
    changed = sum(
        axis[offset:offset + 4] != rotated[offset:offset + 4]
        for offset in range(0, len(axis), 4)
    )
    assert changed >= 4000
    axis_direction = dominant_direction(axis)
    rotated_direction = dominant_direction(rotated)
    direction_shift = abs(axis_direction - rotated_direction)
    assert min(direction_shift, 90 - direction_shift) >= 20


def synthetic_rays(diagonal):
    source = Image.frombytes('RGBA', (WIDTH, HEIGHT), emitter_pixels())
    mask = Image.new('L', (WIDTH, HEIGHT), 0)
    draw = ImageDraw.Draw(mask)
    if diagonal:
        draw.line((56, 0, 199, 143), fill=255, width=5)
        draw.line((199, 0, 56, 143), fill=255, width=5)
    else:
        draw.line((0, 72, WIDTH - 1, 72), fill=255, width=5)
        draw.line((128, 0, 128, HEIGHT - 1), fill=255, width=5)
    mask = ImageChops.lighter(mask.filter(ImageFilter.GaussianBlur(4)),
                              source.convert('L'))
    alpha = Image.new('L', (WIDTH, HEIGHT), 255)
    return Image.merge('RGBA', (mask, mask, mask, alpha)).tobytes()


@pytest.mark.parametrize('fault', [
    'copy', 'angle_ignored', 'same_direction_dimmed', 'isotropic_blur',
    'transparent', 'localized', 'uniform', 'truncated', 'wrong_neutral',
])
def test_glare_glint_validator_rejects_corruption(fault):
    neutral = emitter_pixels()
    axis, rotated = bytearray(synthetic_rays(False)), bytearray(synthetic_rays(True))
    assert_ray_response(neutral, axis, rotated)
    if fault == 'copy':
        axis = neutral
    elif fault == 'angle_ignored':
        rotated = axis
    elif fault == 'same_direction_dimmed':
        rotated = bytearray(axis)
        for offset in range(0, len(rotated), 4):
            for channel in range(3):
                rotated[offset + channel] //= 2
    elif fault == 'isotropic_blur':
        source = Image.frombytes('RGBA', (WIDTH, HEIGHT), neutral)
        axis = ImageChops.lighter(
            source, source.filter(ImageFilter.GaussianBlur(20))).tobytes()
    elif fault == 'transparent':
        axis[3] = 0
    elif fault == 'localized':
        axis = neutral
    elif fault == 'uniform':
        axis = bytes((64, 64, 64, 255)) * WIDTH * HEIGHT
    elif fault == 'truncated':
        rotated = rotated[:-4]
    else:
        neutral = axis
    with pytest.raises(AssertionError):
        assert_ray_response(neutral, axis, rotated)


@pytest.mark.parametrize(
    'plugin_name,prefix,mix_slot,count_label,extra', PLUGINS)
def test_installed_bcc_obsolete_glare_glint_response(
        tmp_path, plugin_name, prefix, mix_slot, count_label, extra):
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
    expected = (
        (2, f'{prefix} Threshhold', 'float'),
        (3, f'{prefix} Brightness', 'float'),
        (4, f'{prefix} Scale', 'float'),
        (6, f'{prefix} Angle', 'angle'),
        (11, count_label, 'float'),
        (mix_slot, 'Mix With Original', 'float'),
    )
    for slot, label, kind in expected:
        assert parameters[slot]['name'] == label
        assert parameters[slot]['kind'] == kind
    for slot, label, _ in extra:
        assert parameters[slot]['name'] == label

    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), emitter_pixels()).save(source)
    outputs = []
    for mix, angle in ((100, 0), (0, 0), (0, 45)):
        request = tmp_path / f'{mix}-{angle}.json'
        output = tmp_path / f'{mix}-{angle}.png'
        assignments = [
            {'slot': 2, 'value': 0},
            {'slot': 3, 'value': 300},
            {'slot': 4, 'value': 3},
            {'slot': 6, 'components': [angle]},
            {'slot': 11, 'value': 4},
            {'slot': mix_slot, 'value': mix},
            *({'slot': slot, 'value': value} for slot, _, value in extra),
        ]
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': assignments,
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p for p in report['requested_parameters']}
        assert requested[mix_slot]['value'] == mix
        assert requested[6]['value'][0] == angle
        assert requested[11]['value'] == 4
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_ray_response(*outputs)
