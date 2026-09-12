"""Explicitly opted-in BCCLinearLumaKey native threshold/auxiliary-layer test.

Build Release harness/worker, then set AEXCOMPAT_TEST_BCC_LUMA to the installed
AEX path. No AE application or license operation is performed. This validates
selected settings, not AE equivalence or all BCC effects.
"""
import copy
import json
import os
import subprocess

import pytest
from PIL import Image

from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT, argb, read_artifact


def assert_keyed_ramp(raw, threshold):
    assert len(raw) == WIDTH * HEIGHT * 4
    for y in range(HEIGHT):
        for x in range(WIDTH):
            i = (y * WIDTH + x) * 4
            assert raw[i] == (255 if x >= threshold else 0)
            if x >= threshold:
                assert raw[i + 1:i + 4] == bytes([x, x, x])


@pytest.mark.parametrize('fault', ['empty', 'noop', 'shifted', 'wrong_rgb', 'truncated'])
def test_luma_validator_rejects_corruption(fault):
    threshold = 64
    valid = bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                  for c in (255 if x >= threshold else 0, x, x, x))
    assert_keyed_ramp(valid, threshold)
    broken = bytearray(valid)
    if fault == 'empty':
        broken[0::4] = bytes(WIDTH * HEIGHT)
    elif fault == 'noop':
        broken[0::4] = bytes([255]) * (WIDTH * HEIGHT)
    elif fault == 'shifted':
        broken[threshold * 4] = 0
    elif fault == 'wrong_rgb':
        broken[threshold * 4 + 1] ^= 1
    else:
        broken = broken[:-4]
    with pytest.raises(AssertionError):
        assert_keyed_ramp(broken, threshold)


def test_installed_bcc_luma_threshold_with_auxiliary_layer(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_BCC_LUMA')
    if not plugin:
        pytest.skip('real AEX execution requires AEXCOMPAT_TEST_BCC_LUMA')
    assert os.name == 'nt'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'
    assert harness.is_file() and (ROOT / 'target/minihost-build/aex_worker.exe').is_file()

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = run('--inspect-experimental', plugin)
    source = bytes(c for y in range(HEIGHT) for x in range(WIDTH) for c in (x, x, x, 255))
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source).save(tmp_path / 'ramp.png')
    for threshold in (64, 192):
        edited = copy.deepcopy(parameters)
        for name, value in [('Make Key From', 1), ('Output', 1), ('Key Type', 2),
                            ('Threshold', threshold), ('Softness', 0), ('Region of Interest', 5)]:
            parameter = next(p for p in edited if p['name'] == name)
            assert parameter['minimum'] <= value <= parameter['maximum']
            parameter['value'] = value
        layer = next(p for p in edited if p['name'] == 'Host Layer')
        assert layer['kind'] == 'layer'
        layer['layer_path'] = 'ramp.png'
        fixture = dict(schema='aexcompat.render_fixture', schema_version=1,
                       primary_layer='ramp.png', parameters=edited,
                       pixel_format='argb8', render_path='smart', premultiplication='straight',
                       timing=dict(current_time=0, time_step=1, total_time=300, time_scale=30),
                       final_artifact='raw', checkpoints=[
                           dict(id='input', stage='smart-input'),
                           dict(id='output', stage='smart-output')])
        path = tmp_path / f'{threshold}.json'
        path.write_text(json.dumps(fixture), encoding='utf-8')
        destination = tmp_path / str(threshold)
        report = run('--render-fixture', plugin, path, destination)
        checkpoints = report['checkpoints']
        assert read_artifact(destination / 'checkpoints/input', checkpoints['input']) == argb(source)
        raw = read_artifact(destination / 'checkpoints/output', checkpoints['output'])
        assert raw == read_artifact(destination / 'final', report['final_artifact'])
        assert_keyed_ramp(raw, threshold)
