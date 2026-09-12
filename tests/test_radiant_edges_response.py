"""Conditional Radiant Edges geometry response, not full AE equivalence."""
import json
import os
import subprocess

import pytest
from PIL import Image

from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def assert_edge_response(raw, left):
    assert len(raw) == WIDTH * HEIGHT * 4
    alpha = raw[3::4]
    assert 0 < sum(a > 0 for a in alpha) < WIDTH * HEIGHT
    # Default rays leave a transparent interior, with a narrow edge fringe.
    row = alpha[72 * WIDTH:73 * WIDTH]
    transitions = [x for x in range(1, WIDTH) if bool(row[x]) != bool(row[x-1])]
    assert len(transitions) == 2
    assert left <= transitions[0] <= left + 6
    assert left + 122 <= transitions[1] <= left + 128
    assert row[0] > 0 and row[left + 64] == 0 and row[-1] > 0
    assert any(raw[i] > 0 for i in range(0, len(raw), 4) if raw[i+3])


@pytest.mark.parametrize('fault', ['empty', 'opaque', 'fixed', 'rgb_empty', 'truncated'])
def test_edge_validator_rejects_corruption(fault):
    raw = bytearray(bytes((200, 20, 0, 255)) * (WIDTH * HEIGHT))
    for y in range(36, 108):
        for x in range(91, 213):
            raw[(y*WIDTH+x)*4+3] = 0
    assert_edge_response(raw, 88)
    if fault == 'empty': raw[3::4] = bytes(WIDTH * HEIGHT)
    elif fault == 'opaque': raw[3::4] = bytes([255]) * (WIDTH * HEIGHT)
    elif fault == 'fixed':
        with pytest.raises(AssertionError):
            assert_edge_response(raw, 64)
        return
    elif fault == 'rgb_empty': raw[0::4] = bytes(WIDTH * HEIGHT)
    else: raw = raw[:-4]
    with pytest.raises(AssertionError):
        assert_edge_response(raw, 88)


def test_installed_radiant_edges_tracks_source_boundary(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_RADIANT_EDGES')
    if not plugin:
        pytest.skip('requires AEXCOMPAT_TEST_RADIANT_EDGES')
    assert os.name == 'nt'
    request = tmp_path / 'request.json'
    request.write_text(json.dumps({'schema_version': 1, 'assignments': [],
        'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300}}), encoding='utf-8')
    outputs = []
    for left in (64, 88):
        source = bytes(c for y in range(HEIGHT) for x in range(WIDTH)
            for c in ((255, 255, 255, 255) if left <= x < left+128 and 36 <= y < 108
                      else (0, 0, 0, 255)))
        src = tmp_path / f'input-{left}.png'
        out = tmp_path / f'output-{left}.png'
        Image.frombytes('RGBA', (WIDTH, HEIGHT), source).save(src)
        run = subprocess.run([str(ROOT/'broker/target/release/aexcompat-harness.exe'),
            '--headless', '--render-experimental-smart-request', plugin,
            str(src), str(out), str(request)], cwd=ROOT, capture_output=True, timeout=90)
        assert run.returncode == 0, run.stderr.decode('utf-8', errors='replace')
        report = json.loads(run.stdout)
        assert report['passed'] and report['output_pixels_valid']
        image = Image.open(out).convert('RGBA')
        assert image.size == (WIDTH, HEIGHT)
        pixels = image.tobytes()
        assert pixels != source
        assert_edge_response(pixels, left)
        outputs.append(pixels)
    assert outputs[0] != outputs[1]
