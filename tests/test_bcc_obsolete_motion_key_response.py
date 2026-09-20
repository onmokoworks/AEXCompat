"""Strict expected failure for legacy BCC Motion Key temporal replacement."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


BOX_TOP = 66
BOX_SIZE = 12
BOX_LEFTS = {frame: 102 + frame * 4 for frame in range(11)}
OBJECT = (245, 20, 220, 255)


def background_pixels():
    pixels = bytearray()
    for y in range(HEIGHT):
        for x in range(WIDTH):
            pixels.extend((
                32 + (x * 5 + y * 3) % 160,
                48 + (x * 2 + y * 7) % 144,
                64 + (x * 7 + y * 5) % 128,
                255,
            ))
    return bytes(pixels)


def motion_frame(frame):
    pixels = bytearray(background_pixels())
    left = BOX_LEFTS[frame]
    for y in range(BOX_TOP, BOX_TOP + BOX_SIZE):
        for x in range(left, left + BOX_SIZE):
            offset = (y * WIDTH + x) * 4
            pixels[offset:offset + 4] = bytes(OBJECT)
    return bytes(pixels)


def region_error(actual, expected, inside):
    total = 0
    count = 0
    for y in range(HEIGHT):
        for x in range(WIDTH):
            selected = (BOX_LEFTS[5] <= x < BOX_LEFTS[5] + BOX_SIZE
                        and BOX_TOP <= y < BOX_TOP + BOX_SIZE)
            if selected != inside:
                continue
            offset = (y * WIDTH + x) * 4
            total += sum(abs(actual[offset + channel] - expected[offset + channel])
                         for channel in range(3))
            count += 1
    return total, count


def test_installed_bcc_obsolete_motion_key_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCMotionKey.aex'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 180)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    for slot, name, kind in (
            (6, 'Host Layer', 'layer'), (9, 'Mode', 'integer'),
            (10, 'Area Selection', 'integer'),
            (12, 'Area Position/Offset', 'point'), (13, 'Area Scale', 'float'),
            (16, 'First Replacement Frame', 'float'),
            (17, 'Last Replacement Frame', 'float'),
            (18, 'Replacement Range', 'float'),
            (21, 'Tracking Mode', 'integer'), (28, 'Resolution', 'integer'),
            (32, 'Mix with Original', 'float')):
        assert parameters[slot]['name'] == name
        assert parameters[slot]['kind'] == kind

    frames = {}
    for frame in range(11):
        path = tmp_path / f'frame-{frame}.png'
        Image.frombytes('RGBA', (WIDTH, HEIGHT), motion_frame(frame)).save(path)
        frames[frame] = path

    outputs = {}
    for label, mode, mix in (
            ('neutral', 1, 100),
            ('remove', 1, 0),
            ('mask', 3, 0)):
        request = tmp_path / f'{label}.json'
        output = tmp_path / f'{label}.png'
        timed_layers = [
            {'slot': slot, 'time': frame, 'time_scale': 30,
             'image': str(frames[frame])}
            for slot in (0,) for frame in range(11)
        ]
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 5, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(frames[5])},
                {'slot': 9, 'value': mode},
                {'slot': 10, 'value': 2},
                {'slot': 12, 'components': [50, 50]},
                {'slot': 13, 'value': 1},
                {'slot': 14, 'value': 0},
                {'slot': 15, 'components': [0]},
                {'slot': 16, 'value': 0},
                {'slot': 17, 'value': 10},
                {'slot': 18, 'value': 10},
                {'slot': 19, 'value': 0},
                {'slot': 20, 'value': 0},
                {'slot': 21, 'value': 2},
                {'slot': 25, 'value': 2500},
                {'slot': 26, 'value': 20},
                {'slot': 27, 'value': 0},
                {'slot': 28, 'value': 1},
                {'slot': 29, 'value': 100},
                {'slot': 30, 'value': 0},
                {'slot': 32, 'value': mix},
            ],
            'timed_layers': timed_layers,
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, frames[5], output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        assert report['current_time'] == 5 and report['time_scale'] == 30
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[9] == mode and requested[10] == 2
        assert requested[12][:2] == [50, 50] and requested[13] == 1
        assert requested[16] == 0 and requested[17] == 10 and requested[18] == 10
        assert requested[21] == 2 and requested[28] == 1
        assert requested[32] == mix
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs[label] = image.convert('RGBA').tobytes()

    source = motion_frame(5)
    background = background_pixels()
    neutral, remove, mask = outputs['neutral'], outputs['remove'], outputs['mask']
    source_inside_error, _ = region_error(source, background, True)
    remove_inside_error, _ = region_error(remove, background, True)
    outside_error, outside_count = region_error(remove, source, False)
    metrics = {
        'source_inside_error': source_inside_error,
        'remove_inside_error': remove_inside_error,
        'outside_error': outside_error,
        'outside_count': outside_count,
        'remove_matches_source': remove == source,
        'mask_matches_source': mask == source,
    }
    if neutral == source and remove == source and mask == source:
        pytest.xfail(
            'known Motion Key signature: valid transport and temporal checkouts, '
            'but Remove and Show Mask are both byte-identical to the input')

    assert neutral == source
    assert remove != source, metrics
    assert mask != source, metrics
    assert remove_inside_error < source_inside_error * 3 // 4, metrics
    assert outside_error < outside_count * 3, metrics
    assert set(remove[3::4]) == {255}
    pytest.fail('Motion Key output changed; validate and promote this expected failure')
