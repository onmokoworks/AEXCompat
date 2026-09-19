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


def assert_directional_blur_response(default, blurred, source):
    assert len(default) == len(blurred) == len(source) == WIDTH * HEIGHT * 4
    assert default == source
    assert blurred != source
    assert min(blurred[0::4]) >= 254 and max(blurred[0::4]) == 255
    assert max(blurred[2::4]) - min(blurred[2::4]) > 64
    assert edge_energy(blurred) < edge_energy(source) * 0.75


@pytest.mark.parametrize('fault', ['noop', 'constant', 'transparent', 'truncated'])
def test_directional_blur_check_rejects_invalid_response(fault):
    source = argb(structured_rgba())
    blurred = bytearray(source)
    for y in range(HEIGHT):
        for x in range(1, WIDTH):
            offset = (y * WIDTH + x) * 4
            blurred[offset + 1] = (source[offset + 1] + source[offset - 3]) // 2
    candidate = {
        'noop': source,
        'constant': bytes([255, 0, 0, 0]) * (WIDTH * HEIGHT),
        'transparent': bytes(len(source)),
        'truncated': bytes(blurred[:-4]),
    }[fault]
    with pytest.raises(AssertionError):
        assert_directional_blur_response(source, candidate, source)


def test_installed_olmdirectionalblur_fixture_strength_response(tmp_path):
    plugin_value = os.environ.get('AEXCOMPAT_TEST_OLM_DIRECTIONAL_BLUR')
    if not plugin_value:
        pytest.skip('real AEX execution requires AEXCOMPAT_TEST_OLM_DIRECTIONAL_BLUR')
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
    strength = next(p for p in parameters if p['slot'] == 5)
    assert strength['name'] == 'Blur Strength'
    assert strength['kind'] == 'integer' and strength['value'] == 0
    source_rgba = structured_rgba()
    source = argb(source_rgba)
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_rgba).save(tmp_path / 'input.png')
    outputs = []
    for value in (0, 20):
        edited = copy.deepcopy(parameters)
        next(p for p in edited if p['slot'] == strength['slot'])['value'] = value
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
        assert read_artifact(destination / 'checkpoints/input', checkpoints['input']) == source
        output = read_artifact(destination / 'checkpoints/output', checkpoints['output'])
        assert output == read_artifact(destination / 'final', report['final_artifact'])
        outputs.append(output)
    assert_directional_blur_response(*outputs, source)


def changed_pixel_count(first, second):
    assert len(first) == len(second) == WIDTH * HEIGHT * 4
    return sum(first[offset:offset + 4] != second[offset:offset + 4]
               for offset in range(0, len(first), 4))


def radial_marker_argb():
    return bytes(component for y in range(HEIGHT) for x in range(WIDTH)
                 for component in ((255, 240, 80, 200)
                                   if 48 <= x < 64 and 64 <= y < 80
                                   else (255, 0, 0, 0)))


def assert_radial_blur_response(default, blurred, source):
    assert len(default) == len(blurred) == len(source) == WIDTH * HEIGHT * 4
    assert default == source
    assert changed_pixel_count(default, blurred) > 16 * 16
    assert min(blurred[0::4]) >= 254 and max(blurred[0::4]) == 255
    background_outputs = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            if not (48 <= x < 64 and 64 <= y < 80):
                offset = (y * WIDTH + x) * 4
                assert source[offset + 1:offset + 4] == bytes(3)
                background_outputs.append(blurred[offset + 1:offset + 4])
    # Identical black input pixels split into both affected and unaffected
    # outputs according to their neighborhood. A spatially independent color
    # transform must map all of those identical inputs to one value.
    assert any(pixel != bytes(3) for pixel in background_outputs)
    assert any(pixel == bytes(3) for pixel in background_outputs)


@pytest.mark.parametrize(
    'fault', ['noop', 'constant', 'transparent', 'color_transform', 'truncated'])
def test_radial_blur_check_rejects_invalid_response(fault):
    source = radial_marker_argb()
    blurred = bytearray(source)
    for offset in range(0, len(blurred), 4):
        blurred[offset + 1] = (blurred[offset + 1] + 128) // 2
    color_transform = bytearray(source)
    for offset in range(0, len(color_transform), 4):
        color_transform[offset + 1] = 255 - color_transform[offset + 1]
    candidate = {
        'noop': source,
        'constant': bytes([255, 0, 0, 0]) * (WIDTH * HEIGHT),
        'transparent': bytes(len(source)),
        'color_transform': bytes(color_transform),
        'truncated': bytes(blurred[:-4]),
    }[fault]
    with pytest.raises(AssertionError):
        assert_radial_blur_response(source, candidate, source)


def test_installed_olmradialblur_fixture_strength_response(tmp_path):
    plugin_value = os.environ.get('AEXCOMPAT_TEST_OLM_RADIAL_BLUR')
    if not plugin_value:
        pytest.skip('real AEX execution requires AEXCOMPAT_TEST_OLM_RADIAL_BLUR')
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
    blur_type = next(p for p in parameters if p['slot'] == 1)
    strength = next(p for p in parameters if p['slot'] == 4)
    assert blur_type['name'] == 'Blur Type' and blur_type['value'] == 1
    assert strength['name'] == 'Strength'
    assert strength['kind'] == 'integer' and strength['value'] == 0
    source = radial_marker_argb()
    source_rgba = bytes(c for i in range(0, len(source), 4)
                        for c in (*source[i + 1:i + 4], source[i]))
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_rgba).save(tmp_path / 'input.png')
    outputs = []
    for value in (0, 20):
        edited = copy.deepcopy(parameters)
        next(p for p in edited if p['slot'] == strength['slot'])['value'] = value
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
        assert read_artifact(destination / 'checkpoints/input', checkpoints['input']) == source
        output = read_artifact(destination / 'checkpoints/output', checkpoints['output'])
        assert output == read_artifact(destination / 'final', report['final_artifact'])
        outputs.append(output)
    assert_radial_blur_response(*outputs, source)


def smoother_stair_argb():
    return bytes(component for y in range(HEIGHT) for x in range(WIDTH)
                 for component in ((255, 0, 0, 0) if x < y + 56
                                   else (255, 255, 255, 255)))


def assert_smoother_response(smoothed, source):
    assert len(smoothed) == len(source) == WIDTH * HEIGHT * 4
    changed = changed_pixel_count(source, smoothed)
    assert HEIGHT <= changed <= HEIGHT * 2
    assert min(smoothed[0::4]) >= 254 and max(smoothed[0::4]) == 255
    black_outputs, white_outputs = [], []
    for offset in range(0, len(source), 4):
        source_rgb = source[offset + 1:offset + 4]
        output_rgb = smoothed[offset + 1:offset + 4]
        if source_rgb == bytes(3):
            black_outputs.append(output_rgb)
        else:
            assert source_rgb == bytes([255, 255, 255])
            white_outputs.append(output_rgb)
        if output_rgb != source_rgb:
            pixel = offset // 4
            y, x = divmod(pixel, WIDTH)
            assert x in (y + 55, y + 56)
    assert any(pixel == bytes(3) for pixel in black_outputs)
    assert any(pixel != bytes(3) for pixel in black_outputs)
    assert any(pixel == bytes([255, 255, 255]) for pixel in white_outputs)
    assert any(pixel != bytes([255, 255, 255]) for pixel in white_outputs)
    assert any(0 < component < 255 for component in smoothed[1::4])


@pytest.mark.parametrize(
    'fault', ['noop', 'constant', 'transparent', 'global_gray', 'sparse', 'truncated'])
def test_smoother_check_rejects_invalid_response(fault):
    source = smoother_stair_argb()
    global_gray = bytearray(source)
    for offset in range(0, len(global_gray), 4):
        if global_gray[offset + 1] == 255:
            global_gray[offset + 1:offset + 4] = bytes([128, 128, 128])
    sparse = bytearray(source)
    sparse[(56 * 4) + 1:(56 * 4) + 4] = bytes([128, 128, 128])
    candidate = {
        'noop': source,
        'constant': bytes([255, 0, 0, 0]) * (WIDTH * HEIGHT),
        'transparent': bytes(len(source)),
        'global_gray': bytes(global_gray),
        'sparse': bytes(sparse),
        'truncated': source[:-4],
    }[fault]
    with pytest.raises(AssertionError):
        assert_smoother_response(candidate, source)


def test_installed_olmsmoother_fixture_default_response(tmp_path):
    plugin_value = os.environ.get('AEXCOMPAT_TEST_OLM_SMOOTHER')
    if not plugin_value:
        pytest.skip('real AEX execution requires AEXCOMPAT_TEST_OLM_SMOOTHER')
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
    smooth_range = next(p for p in parameters if p['name'] == 'Do Smooth Range')
    assert smooth_range['kind'] == 'integer' and smooth_range['value'] == 6
    source = smoother_stair_argb()
    source_rgba = bytes(c for i in range(0, len(source), 4)
                        for c in (*source[i + 1:i + 4], source[i]))
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_rgba).save(tmp_path / 'input.png')
    fixture = dict(schema='aexcompat.render_fixture', schema_version=1,
                   primary_layer='input.png', parameters=parameters,
                   pixel_format='argb8', render_path='classic', premultiplication='straight',
                   timing=dict(current_time=0, time_step=1, total_time=300, time_scale=30),
                   final_artifact='raw', checkpoints=[
                       dict(id='input', stage='classic-input'),
                       dict(id='output', stage='classic-output')])
    fixture_path = tmp_path / 'fixture.json'
    fixture_path.write_text(json.dumps(fixture), encoding='utf-8')
    destination = tmp_path / 'render'
    report = run('--render-fixture', plugin, fixture_path, destination)
    checkpoints = report['checkpoints']
    assert read_artifact(destination / 'checkpoints/input', checkpoints['input']) == source
    output = read_artifact(destination / 'checkpoints/output', checkpoints['output'])
    assert output == read_artifact(destination / 'final', report['final_artifact'])
    assert_smoother_response(output, source)


def test_installed_olmsmoother2_fixture_default_response(tmp_path):
    plugin_value = os.environ.get('AEXCOMPAT_TEST_OLM_SMOOTHER2')
    if not plugin_value:
        pytest.skip('real AEX execution requires AEXCOMPAT_TEST_OLM_SMOOTHER2')
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
    smoothness = next(p for p in parameters if p['name'] == 'Smoothness')
    smooth_range = next(p for p in parameters if p['name'] == 'Smooth Range')
    smoother_version = next(p for p in parameters if p['name'] == 'Smoother Version')
    assert smoothness['kind'] == 'integer' and smoothness['value'] == 100
    assert smooth_range['kind'] == 'integer' and smooth_range['value'] == 2
    assert smoother_version['choices'] == ['v1', 'v2'] and smoother_version['value'] == 2
    source = smoother_stair_argb()
    source_rgba = bytes(c for i in range(0, len(source), 4)
                        for c in (*source[i + 1:i + 4], source[i]))
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_rgba).save(tmp_path / 'input.png')
    fixture = dict(schema='aexcompat.render_fixture', schema_version=1,
                   primary_layer='input.png', parameters=parameters,
                   pixel_format='argb8', render_path='smart', premultiplication='straight',
                   timing=dict(current_time=0, time_step=1, total_time=300, time_scale=30),
                   final_artifact='raw', checkpoints=[
                       dict(id='input', stage='smart-input'),
                       dict(id='output', stage='smart-output')])
    fixture_path = tmp_path / 'fixture.json'
    fixture_path.write_text(json.dumps(fixture), encoding='utf-8')
    destination = tmp_path / 'render'
    report = run('--render-fixture', plugin, fixture_path, destination)
    checkpoints = report['checkpoints']
    assert read_artifact(destination / 'checkpoints/input', checkpoints['input']) == source
    output = read_artifact(destination / 'checkpoints/output', checkpoints['output'])
    assert output == read_artifact(destination / 'final', report['final_artifact'])
    assert_smoother_response(output, source)


def toon_dilate_source_argb():
    return bytes(component for y in range(HEIGHT) for x in range(WIDTH)
                 for component in ((255, 32, 64, 128)
                                   if 96 <= x < 160 and 56 <= y < 88
                                   else (0, 0, 0, 0)))


def assert_toon_dilate_response(dilated, source):
    assert len(dilated) == len(source) == WIDTH * HEIGHT * 4
    changed = [offset for offset in range(0, len(source), 4)
               if source[offset:offset + 4] != dilated[offset:offset + 4]]
    expected = {(y * WIDTH + x) * 4
                for y in range(54, 90) for x in range(94, 162)
                if not (56 <= y < 88 and 96 <= x < 160)}
    assert set(changed) == expected
    opaque = [offset for offset in range(0, len(source), 4) if source[offset] == 255]
    assert all(dilated[offset:offset + 4] == source[offset:offset + 4]
               for offset in opaque)
    assert all(source[offset:offset + 4] == bytes([0, 0, 0, 0])
               and dilated[offset:offset + 4] == bytes([255, 32, 64, 128])
               for offset in changed)
    assert any(source[offset] == 0 and dilated[offset:offset + 4] == bytes([0, 0, 0, 0])
               for offset in range(0, len(source), 4))


@pytest.mark.parametrize('fault', ['noop', 'alpha_fill', 'opaque_overwrite', 'global_fill'])
def test_toon_dilate_check_rejects_invalid_response(fault):
    source = toon_dilate_source_argb()
    candidate = bytearray(source)
    if fault == 'alpha_fill':
        candidate[0] = 255
        candidate[1:4] = bytes([32, 64, 128])
    elif fault == 'opaque_overwrite':
        offset = (56 * WIDTH + 96) * 4
        candidate[offset + 1:offset + 4] = bytes([255, 255, 255])
    elif fault == 'global_fill':
        for offset in range(0, len(candidate), 4):
            if candidate[offset] == 0:
                candidate[offset + 1:offset + 4] = bytes([32, 64, 128])
    with pytest.raises(AssertionError):
        assert_toon_dilate_response(source if fault == 'noop' else bytes(candidate), source)


def test_installed_olmtoon_dilate_fixture_default_response(tmp_path):
    plugin_value = os.environ.get('AEXCOMPAT_TEST_OLM_TOON_DILATE')
    if not plugin_value:
        pytest.skip('real AEX execution requires AEXCOMPAT_TEST_OLM_TOON_DILATE')
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
    radius = next(p for p in parameters if p['name'] == 'Search Radius')
    assert radius['kind'] == 'float' and radius['value'] == 2
    source = toon_dilate_source_argb()
    source_rgba = bytes(c for i in range(0, len(source), 4)
                        for c in (*source[i + 1:i + 4], source[i]))
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_rgba).save(tmp_path / 'input.png')
    fixture = dict(schema='aexcompat.render_fixture', schema_version=1,
                   primary_layer='input.png', parameters=parameters,
                   pixel_format='argb8', render_path='smart', premultiplication='straight',
                   timing=dict(current_time=0, time_step=1, total_time=300, time_scale=30),
                   final_artifact='raw', checkpoints=[
                       dict(id='input', stage='smart-input'),
                       dict(id='output', stage='smart-output')])
    fixture_path = tmp_path / 'fixture.json'
    fixture_path.write_text(json.dumps(fixture), encoding='utf-8')
    destination = tmp_path / 'render'
    report = run('--render-fixture', plugin, fixture_path, destination)
    checkpoints = report['checkpoints']
    assert read_artifact(destination / 'checkpoints/input', checkpoints['input']) == source
    output = read_artifact(destination / 'checkpoints/output', checkpoints['output'])
    assert output == read_artifact(destination / 'final', report['final_artifact'])
    assert_toon_dilate_response(output, source)
