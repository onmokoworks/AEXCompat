"""Legacy BCC Smooth Tone denoise response; not exact AE parity."""
import json
import os
import statistics
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def noisy_edge_pixels():
    state = 0x31415926
    pixels = bytearray()
    for _y in range(HEIGHT):
        for x in range(WIDTH):
            state = (1664525 * state + 1013904223) & 0xffffffff
            noise = ((state >> 24) * 41 // 256) - 20
            value = (80 if x < WIDTH // 2 else 180) + noise
            pixels.extend((value, value, value, 255))
    return bytes(pixels)


def region_values(raw, left):
    start, end = ((16, WIDTH // 2 - 16) if left
                  else (WIDTH // 2 + 16, WIDTH - 16))
    return [raw[(y * WIDTH + x) * 4]
            for y in range(16, HEIGHT - 16)
            for x in range(start, end)]


def smooth_edge_pixels():
    pixels = bytearray()
    for _y in range(HEIGHT):
        for x in range(WIDTH):
            if x < 120:
                value = 80
            elif x > 136:
                value = 180
            else:
                value = round(80 + 100 * (x - 120) / 16)
            pixels.extend((value, value, value, 255))
    return bytes(pixels)


def assert_smooth_tone_response(neutral, smooth):
    source = noisy_edge_pixels()
    assert neutral == source
    assert len(smooth) == WIDTH * HEIGHT * 4
    assert smooth[3::4] == bytes([255]) * WIDTH * HEIGHT
    assert smooth[0::4] == smooth[1::4] == smooth[2::4]
    left = region_values(smooth, True)
    right = region_values(smooth, False)
    assert abs(statistics.mean(left) - 80) <= 2
    assert abs(statistics.mean(right) - 180) <= 2
    assert statistics.pvariance(left) <= 1
    assert statistics.pvariance(right) <= 1
    for y in range(16, HEIGHT - 16):
        row = list(smooth[y * WIDTH * 4:(y + 1) * WIDTH * 4:4])
        left_edge = statistics.mean(row[116:124])
        right_edge = statistics.mean(row[132:140])
        assert right_edge - left_edge >= 75
        gradients = [abs(right_value - left_value)
                     for left_value, right_value in zip(row, row[1:])]
        assert 5 <= max(gradients) <= 12
        assert statistics.mean(row[:32]) < statistics.mean(row[-32:]) - 95
    changed = sum(smooth[offset] != source[offset]
                  for offset in range(0, len(source), 4))
    assert changed >= WIDTH * HEIGHT * 9 // 10


@pytest.mark.parametrize('fault', [
    'copy', 'uniform', 'global_ramp', 'hard_edge', 'wrong_means', 'color',
    'transparent', 'spatial', 'boundary_rows', 'reversed', 'wrong_neutral',
    'truncated',
])
def test_smooth_tone_validator_rejects_corruption(fault):
    neutral = noisy_edge_pixels()
    smooth = bytearray(smooth_edge_pixels())
    assert_smooth_tone_response(neutral, smooth)
    if fault == 'copy':
        smooth = neutral
    elif fault == 'uniform':
        smooth = bytes((128, 128, 128, 255)) * WIDTH * HEIGHT
    elif fault == 'global_ramp':
        row = bytearray()
        for x in range(WIDTH):
            value = round(80 + 100 * x / (WIDTH - 1))
            row.extend((value, value, value, 255))
        smooth = bytes(row) * HEIGHT
    elif fault == 'hard_edge':
        smooth = bytes(
            channel
            for _y in range(HEIGHT)
            for x in range(WIDTH)
            for channel in ((80, 80, 80, 255) if x < WIDTH // 2
                            else (180, 180, 180, 255))
        )
    elif fault == 'wrong_means':
        for offset in range(0, len(smooth), 4):
            for channel in range(3):
                smooth[offset + channel] = min(255, smooth[offset + channel] + 20)
    elif fault == 'color':
        smooth[1] = 0
    elif fault == 'transparent':
        smooth[3] = 0
    elif fault == 'spatial':
        y = 20
        for x in range(16, WIDTH // 2 - 16):
            offset = (y * WIDTH + x) * 4
            smooth[offset:offset + 3] = bytes((120, 120, 120))
    elif fault == 'boundary_rows':
        for y in range(16, HEIGHT - 16):
            if y == HEIGHT // 2:
                continue
            for x in range(112, 144):
                offset = (y * WIDTH + x) * 4
                smooth[offset:offset + 3] = bytes((0, 0, 0))
    elif fault == 'reversed':
        row_size = WIDTH * 4
        for y in range(HEIGHT):
            row = [smooth[(y * WIDTH + x) * 4:(y * WIDTH + x + 1) * 4]
                   for x in range(WIDTH)]
            smooth[y * row_size:(y + 1) * row_size] = b''.join(reversed(row))
    elif fault == 'wrong_neutral':
        neutral = smooth
    else:
        smooth = smooth[:-4]
    with pytest.raises(AssertionError):
        assert_smooth_tone_response(neutral, smooth)


def test_installed_bcc_obsolete_smooth_tone_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCSmoothTone.aex'
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
    assert parameters[10]['name'] == 'Radius X'
    assert parameters[10]['kind'] == 'float'
    assert parameters[13]['name'] == 'Maximum Deviation'
    assert parameters[13]['kind'] == 'float'
    assert parameters[17]['name'] == 'Mix with Original'
    assert parameters[17]['kind'] == 'float'
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), noisy_edge_pixels()).save(source)
    outputs = []
    for name, mix in (('neutral', 100), ('smooth', 0)):
        request, output = tmp_path / f'{name}.json', tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(source)},
                {'slot': 8, 'value': 3},
                {'slot': 9, 'value': 1},
                {'slot': 10, 'value': 8},
                {'slot': 11, 'value': 8},
                {'slot': 12, 'value': 5},
                {'slot': 13, 'value': 100},
                {'slot': 14, 'value': 5},
                {'slot': 15, 'value': 1},
                {'slot': 16, 'value': 100},
                {'slot': 17, 'value': mix},
                {'slot': 18, 'value': 1},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[8] == 3
        assert requested[10] == 8 and requested[11] == 8
        assert requested[13] == 100
        assert requested[14] == 5
        assert requested[17] == mix
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_smooth_tone_response(*outputs)
