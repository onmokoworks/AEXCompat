"""Pixel-level fixture regression with explicitly opted-in real OLM coverage.

Set the test-specific AEXCOMPAT_TEST_* variable to the installed AEX path to
run a Windows CLI case. Build the Release harness and native worker from this
checkout first. No AE process, license operation, or installed-file mutation is
performed.
"""
import copy
import hashlib
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
WIDTH, HEIGHT = 256, 144


def structured_rgba():
    return bytes(component for y in range(HEIGHT) for x in range(WIDTH)
                 for component in (240 if ((x // 8) ^ (y // 8)) & 1 else 16,
                                   x, y * 255 // (HEIGHT - 1), 255))


def argb(rgba):
    result = bytearray(len(rgba))
    result[0::4], result[1::4] = rgba[3::4], rgba[0::4]
    result[2::4], result[3::4] = rgba[1::4], rgba[2::4]
    return bytes(result)


def edge_energy(raw):
    return sum(abs(raw[(y * WIDTH + x) * 4 + 1] - raw[(y * WIDTH + x - 1) * 4 + 1])
               for y in range(HEIGHT) for x in range(1, WIDTH)) / ((WIDTH - 1) * HEIGHT)


def assert_blur_response(low, high):
    assert len(low) == len(high) == WIDTH * HEIGHT * 4
    assert set(low[0::4]) == set(high[0::4]) == {255}
    assert low != high, "parameter edit had no pixel effect"
    assert edge_energy(high) < edge_energy(low) * 0.5
    # Preserve the nonconstant gradient: a constant black result is not blur.
    assert max(high[2::4]) - min(high[2::4]) > 64


@pytest.mark.parametrize("fault", ["noop", "constant", "truncated", "transparent"])
def test_semantic_check_rejects_invalid_response(fault):
    low = argb(structured_rgba())
    high = {"noop": low, "constant": bytes([255, 0, 0, 0]) * (WIDTH * HEIGHT),
            "truncated": low[:-4], "transparent": bytes(len(low))}[fault]
    with pytest.raises(AssertionError):
        assert_blur_response(low, high)


def test_semantic_check_accepts_smooth_gradient():
    low = argb(structured_rgba())
    high = bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (255, 128, x, y * 255 // (HEIGHT - 1)))
    assert_blur_response(low, high)


def read_artifact(directory, metadata):
    assert metadata['channel_order'] == 'ARGB'
    assert metadata['pixel_format'] == 'argb8'
    assert (metadata['width'], metadata['height']) == (WIDTH, HEIGHT)
    assert metadata['data_file'] == 'output.bin'
    raw = (directory / 'output.bin').read_bytes()
    assert len(raw) == metadata['data_size_bytes'] == WIDTH * HEIGHT * 4
    assert hashlib.sha256(raw).hexdigest() == metadata['data_sha256']
    return raw


def test_installed_olmblur_fixture_parameter_response(tmp_path):
    plugin_value = os.environ.get('AEXCOMPAT_TEST_OLM_BLUR')
    if not plugin_value:
        pytest.skip('real AEX execution requires AEXCOMPAT_TEST_OLM_BLUR')
    assert os.name == 'nt', 'opted-in native test requires Windows'
    plugin = Path(plugin_value).resolve()
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'
    assert plugin.is_file() and harness.is_file()
    assert (ROOT / 'target/minihost-build/aex_worker.exe').is_file()

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True, check=False)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = run('--inspect-experimental', plugin)
    amount = next(p for p in parameters if p['name'] == 'Blur Amount')
    assert amount['kind'] == 'float' and amount['minimum'] <= 1 < 20 <= amount['maximum']
    source = structured_rgba()
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source).save(tmp_path / 'input.png')
    outputs = []
    for value in (1, 20):
        edited = copy.deepcopy(parameters)
        next(p for p in edited if p['slot'] == amount['slot'])['value'] = value
        fixture = dict(schema='aexcompat.render_fixture', schema_version=1,
                       primary_layer='input.png', parameters=edited,
                       pixel_format='argb8', render_path='smart', premultiplication='straight',
                       timing=dict(current_time=0, time_step=1, total_time=300, time_scale=30),
                       final_artifact='raw', checkpoints=[
                           dict(id='input', stage='smart-input'),
                           dict(id='output', stage='smart-output')])
        fixture_path = tmp_path / f'fixture-{value}.json'
        fixture_path.write_text(json.dumps(fixture), encoding='utf-8')
        destination = tmp_path / f'render-{value}'
        report = run('--render-fixture', plugin, fixture_path, destination)
        checkpoints = report['checkpoints']
        assert read_artifact(destination / 'checkpoints/input', checkpoints['input']) == argb(source)
        output = read_artifact(destination / 'checkpoints/output', checkpoints['output'])
        assert output == read_artifact(destination / 'final', report['final_artifact'])
        outputs.append(output)
    assert_blur_response(*outputs)


def assert_color_keep_response(default, selected, source):
    assert len(default) == len(selected) == len(source) == WIDTH * HEIGHT * 4
    assert set(default[0::4]) == {0}
    for y in range(HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            if x < WIDTH // 2:
                assert selected[offset:offset + 4] == source[offset:offset + 4]
            else:
                assert selected[offset] == 0


def two_color_argb():
    return (bytes([255, 32, 64, 128]) * (WIDTH // 2)
            + bytes([255, 190, 40, 80]) * (WIDTH // 2)) * HEIGHT


@pytest.mark.parametrize('fault', ['empty', 'noop', 'wrong_color', 'truncated'])
def test_color_keep_check_rejects_invalid_response(fault):
    source = two_color_argb()
    selected = bytearray(source)
    for y in range(HEIGHT):
        for x in range(WIDTH // 2, WIDTH):
            selected[(y * WIDTH + x) * 4] = 0
    wrong = bytearray(selected)
    wrong[1] ^= 1
    candidate = {'empty': bytes(len(source)), 'noop': source,
                 'wrong_color': bytes(wrong), 'truncated': bytes(selected[:-4])}[fault]
    with pytest.raises(AssertionError):
        assert_color_keep_response(bytes(len(source)), candidate, source)


def test_installed_colorkeep_fixture_color_response(tmp_path):
    plugin_value = os.environ.get('AEXCOMPAT_TEST_COLOR_KEEP')
    if not plugin_value:
        pytest.skip('real AEX execution requires AEXCOMPAT_TEST_COLOR_KEEP')
    assert os.name == 'nt', 'opted-in native test requires Windows'
    plugin = Path(plugin_value).resolve()
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'
    assert plugin.is_file() and harness.is_file()
    assert (ROOT / 'target/minihost-build/aex_worker.exe').is_file()

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True, check=False)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = run('--inspect-experimental', plugin)
    count = next(p for p in parameters if p['name'] == 'Enabled Color Num')
    color = next(p for p in parameters if p['kind'] == 'color')
    assert count['value'] == 1 and color['color'] == [255, 0, 0, 0]
    source = two_color_argb()
    rgba = bytes(c for i in range(0, len(source), 4)
                 for c in (*source[i + 1:i + 4], source[i]))
    Image.frombytes('RGBA', (WIDTH, HEIGHT), rgba).save(tmp_path / 'input.png')
    outputs = []
    for variant, value in [('default', [255, 0, 0, 0]), ('selected', [255, 32, 64, 128])]:
        edited = copy.deepcopy(parameters)
        next(p for p in edited if p['slot'] == color['slot'])['color'] = value
        fixture = dict(schema='aexcompat.render_fixture', schema_version=1,
                       primary_layer='input.png', parameters=edited,
                       pixel_format='argb8', render_path='smart', premultiplication='straight',
                       timing=dict(current_time=0, time_step=1, total_time=300, time_scale=30),
                       final_artifact='raw', checkpoints=[
                           dict(id='input', stage='smart-input'),
                           dict(id='output', stage='smart-output')])
        fixture_path = tmp_path / f'{variant}.json'
        fixture_path.write_text(json.dumps(fixture), encoding='utf-8')
        destination = tmp_path / variant
        report = run('--render-fixture', plugin, fixture_path, destination)
        checkpoints = report['checkpoints']
        assert read_artifact(destination / 'checkpoints/input', checkpoints['input']) == source
        output = read_artifact(destination / 'checkpoints/output', checkpoints['output'])
        assert output == read_artifact(destination / 'final', report['final_artifact'])
        outputs.append(output)
    assert_color_keep_response(*outputs, source)


def assert_color_key_response(default, keyed, source):
    assert len(default) == len(keyed) == len(source) == WIDTH * HEIGHT * 4
    assert default == source
    for y in range(HEIGHT):
        for x in range(WIDTH):
            offset = (y * WIDTH + x) * 4
            if x < WIDTH // 2:
                assert keyed[offset] == 0
                assert keyed[offset + 1:offset + 4] == source[offset + 1:offset + 4]
            else:
                assert keyed[offset:offset + 4] == source[offset:offset + 4]


@pytest.mark.parametrize('fault', ['noop', 'all_transparent', 'wrong_color', 'truncated'])
def test_color_key_check_rejects_invalid_response(fault):
    source = two_color_argb()
    keyed = bytearray(source)
    for y in range(HEIGHT):
        for x in range(WIDTH // 2):
            keyed[(y * WIDTH + x) * 4] = 0
    wrong = bytearray(keyed)
    wrong[1] ^= 1
    candidate = {
        'noop': source,
        'all_transparent': bytes(len(source)),
        'wrong_color': bytes(wrong),
        'truncated': bytes(keyed[:-4]),
    }[fault]
    with pytest.raises(AssertionError):
        assert_color_key_response(source, candidate, source)


def test_installed_olmcolorkey_fixture_color_response(tmp_path):
    plugin_value = os.environ.get('AEXCOMPAT_TEST_OLM_COLOR_KEY')
    if not plugin_value:
        pytest.skip('real AEX execution requires AEXCOMPAT_TEST_OLM_COLOR_KEY')
    assert os.name == 'nt', 'opted-in native test requires Windows'
    plugin = Path(plugin_value).resolve()
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'
    assert plugin.is_file() and harness.is_file()
    assert (ROOT / 'target/minihost-build/aex_worker.exe').is_file()

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True, check=False)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = run('--inspect-experimental', plugin)
    use_color = next(p for p in parameters if p['name'] == 'Use Color 1')
    color = next(p for p in parameters if p['name'] == 'Color 1')
    assert use_color['value'] == 0 and color['kind'] == 'color'
    source = two_color_argb()
    rgba = bytes(c for i in range(0, len(source), 4)
                 for c in (*source[i + 1:i + 4], source[i]))
    Image.frombytes('RGBA', (WIDTH, HEIGHT), rgba).save(tmp_path / 'input.png')
    outputs = []
    for variant, enabled in [('default', 0), ('keyed', 1)]:
        edited = copy.deepcopy(parameters)
        next(p for p in edited if p['slot'] == use_color['slot'])['value'] = enabled
        next(p for p in edited if p['slot'] == color['slot'])['color'] = [255, 32, 64, 128]
        fixture = dict(schema='aexcompat.render_fixture', schema_version=1,
                       primary_layer='input.png', parameters=edited,
                       pixel_format='argb8', render_path='smart', premultiplication='straight',
                       timing=dict(current_time=0, time_step=1, total_time=300, time_scale=30),
                       final_artifact='raw', checkpoints=[
                           dict(id='input', stage='smart-input'),
                           dict(id='output', stage='smart-output')])
        fixture_path = tmp_path / f'{variant}.json'
        fixture_path.write_text(json.dumps(fixture), encoding='utf-8')
        destination = tmp_path / variant
        report = run('--render-fixture', plugin, fixture_path, destination)
        checkpoints = report['checkpoints']
        assert read_artifact(destination / 'checkpoints/input', checkpoints['input']) == source
        output = read_artifact(destination / 'checkpoints/output', checkpoints['output'])
        assert output == read_artifact(destination / 'final', report['final_artifact'])
        outputs.append(output)
    assert_color_key_response(*outputs, source)
