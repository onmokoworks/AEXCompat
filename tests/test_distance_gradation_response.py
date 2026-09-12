"""Opt-in DistanceGradation regression, not an AE-equivalence assertion.

Set AEXCOMPAT_TEST_DISTANCE_GRADATION to an installed AEX path after building
the Release harness and worker. No AE application is launched.
"""
import copy
import json
import os
import subprocess

import pytest
from PIL import Image

from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT, argb, read_artifact


def assert_distance_response(red, green):
    assert len(red) == len(green) == WIDTH * HEIGHT * 4
    assert red[0::4] == green[0::4]
    assert red[1::4] == green[2::4]
    assert set(red[2::4]) == set(red[3::4]) == {0}
    assert set(green[1::4]) == set(green[3::4]) == {0}
    for y in range(HEIGHT):
        for x in range(WIDTH):
            i = (y * WIDTH + x) * 4
            if not (64 <= x < 192 and 36 <= y < 108):
                assert red[i] == 0
            if red[i]:
                assert red[i + 1] == 255
    # Default inside gradation fades toward the interior, symmetrically.
    alpha = lambda x: red[(72 * WIDTH + x) * 4]
    assert alpha(64) > alpha(70) > alpha(80) > alpha(100) == 0
    assert all(alpha(x) == alpha(255 - x) for x in range(64, 128))
    vertical_alpha = lambda y: red[(y * WIDTH + 128) * 4]
    assert vertical_alpha(36) > vertical_alpha(42) > vertical_alpha(52) > vertical_alpha(71)
    assert all(vertical_alpha(y) == vertical_alpha(143 - y) for y in range(36, 72))


@pytest.mark.parametrize('fault', ['empty', 'solid', 'truncated', 'same_color', 'single_row'])
def test_distance_response_rejects_faults(fault):
    red = bytearray(WIDTH * HEIGHT * 4)
    green = bytearray(len(red))
    for y in range(36, 108):
        for x in range(64, 192):
            i = (y * WIDTH + x) * 4
            alpha = max(0, 248 - 8 * min(x - 64, 191 - x, y - 36, 107 - y))
            red[i:i + 4] = bytes([alpha, 255, 0, 0])
            green[i:i + 4] = bytes([alpha, 0, 255, 0])
    assert_distance_response(red, green)
    if fault == 'empty':
        red, green = bytes(len(red)), bytes(len(green))
    elif fault == 'solid':
        red[0::4] = green[0::4] = bytes([255]) * (WIDTH * HEIGHT)
    elif fault == 'truncated':
        green = green[:-4]
    elif fault == 'single_row':
        for y in range(HEIGHT):
            if y != 72:
                start = y * WIDTH * 4
                red[start:start + WIDTH * 4] = green[start:start + WIDTH * 4] = bytes(WIDTH * 4)
    else:
        green = red
    with pytest.raises(AssertionError):
        assert_distance_response(red, green)


def test_installed_distance_gradation_response(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_DISTANCE_GRADATION')
    if not plugin:
        pytest.skip('real AEX execution requires AEXCOMPAT_TEST_DISTANCE_GRADATION')
    assert os.name == 'nt'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'
    assert harness.is_file() and (ROOT / 'target/minihost-build/aex_worker.exe').is_file()

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = run('--inspect-experimental', plugin)
    source = bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                   for c in ((255, 255, 255, 255) if 64 <= x < 192 and 36 <= y < 108
                             else (0, 0, 0, 0)))
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source).save(tmp_path / 'input.png')
    outputs = []
    for variant, color in [('red', [255, 255, 0, 0]), ('green', [255, 0, 255, 0])]:
        edited = copy.deepcopy(parameters)
        parameter = next(p for p in edited if p['name'] == 'Gradation Color')
        assert parameter['kind'] == 'color'
        parameter['color'] = color
        fixture = dict(schema='aexcompat.render_fixture', schema_version=1,
                       primary_layer='input.png', parameters=edited,
                       pixel_format='argb8', render_path='smart', premultiplication='straight',
                       timing=dict(current_time=0, time_step=1, total_time=300, time_scale=30),
                       final_artifact='raw', checkpoints=[
                           dict(id='input', stage='smart-input'),
                           dict(id='output', stage='smart-output')])
        path = tmp_path / f'{variant}.json'
        path.write_text(json.dumps(fixture), encoding='utf-8')
        destination = tmp_path / variant
        report = run('--render-fixture', plugin, path, destination)
        checkpoints = report['checkpoints']
        assert read_artifact(destination / 'checkpoints/input', checkpoints['input']) == argb(source)
        output = read_artifact(destination / 'checkpoints/output', checkpoints['output'])
        assert output == read_artifact(destination / 'final', report['final_artifact'])
        outputs.append(output)
    assert_distance_response(*outputs)
