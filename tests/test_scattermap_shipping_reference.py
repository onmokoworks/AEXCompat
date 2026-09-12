"""Opaque ARGB8 reference matrix; does not claim deep/float or all-setting parity."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT
from tools.scattermap_reference_oracle import gradient, generated_luma_map, render_case


# Decoded ARGB observations from the preserved AE 25.2 scalar/map captures.
CASES = [
    ('default', {}, None, '19cea826f356e0d94bc29ff10cb9e7f5a770fe5b288cb3d190a58372353102d9'),
    ('identity', {'amount': 0}, None, '863d238f52f81aba4017c198af4d748cb57fe369e6216fdbacf45fd94037ecf7'),
    ('horizontal', {'direction': 1}, None, '10a2a95a0ae27ca5fe3a6f6f92eeddfe611885fa72afa0902a24e8bea5d2198f'),
    ('vertical', {'direction': 2}, None, '6d6198506967e18f619e57cf79e65c52c8f8c65c0ef89710344af2f1045e091c'),
    ('amount_max', {'amount': 500}, None, '8e535435c74a9521d816a3b836db578a2ae942efbd80a55447b97610dc26b794'),
    ('seed_max', {'seed': 10000}, None, 'e31ba13264e801de7ccce4d6863215e54c0dc0c7ff4a918e45ee75bc59e817ec'),
    ('mix_zero', {'mix': 0}, None, '863d238f52f81aba4017c198af4d748cb57fe369e6216fdbacf45fd94037ecf7'),
    ('connected_map', {}, (5, 3, False), 'a38568761441c209940f81a8c2792dad50566c66eda1463bdcf071cca614891b'),
    ('inverted_map', {}, (11, 7, True), '3bc0c5172b880a8a83cec24177b78721e9f0619d5330f6a26aaa02b9cc057a08'),
]


def rgba(argb):
    return bytes(c for i in range(0, len(argb), 4) for c in (*argb[i+1:i+4], argb[i]))


def assert_reference(actual, expected, recorded):
    assert actual == expected
    assert hashlib.sha256(actual).hexdigest() == recorded


@pytest.mark.parametrize('fault', ['input', 'wrong-direction', 'rgb', 'alpha', 'truncated'])
def test_reference_rejects_corruption(fault):
    expected = render_case()
    actual = bytearray(expected)
    if fault == 'input':
        actual = gradient(16, 12)
    elif fault == 'wrong-direction':
        actual = render_case(direction=1)
    elif fault == 'rgb':
        actual[5] ^= 1
    elif fault == 'alpha':
        actual[4] = 0
    else:
        actual = actual[:-4]
    with pytest.raises(AssertionError):
        assert_reference(actual, expected, CASES[0][3])


@pytest.mark.parametrize('name,params,map_spec,recorded', CASES, ids=[c[0] for c in CASES])
def test_real_scattermap_shipping_reference(tmp_path, name, params, map_spec, recorded):
    plugin = os.environ.get('AEXCOMPAT_TEST_SCATTERMAP')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_SCATTERMAP to local ScatterMap.aex')
    width, height = (11, 7) if map_spec else (16, 12)
    source = gradient(width, height)
    Image.frombytes('RGBA', (width, height), rgba(source)).save(tmp_path / 'input.png')
    values = {'amount': 5, 'direction': 3, 'seed': 0, 'mix': 100}
    values.update(params)
    assignments = [{'slot': s, 'value': values[k]} for s, k in
                   ((1, 'amount'), (2, 'direction'), (3, 'seed'), (5, 'mix'))]
    oracle_params = dict(params)
    if map_spec:
        mw, mh, invert = map_spec
        pixels = bytes(c for y in range(mh) for x in range(mw)
                       for c in (((x+y)*255//(mw+mh-2),)*3 + (255,)))
        map_path = tmp_path / 'map.png'
        Image.frombytes('RGBA', (mw, mh), pixels).save(map_path)
        assignments.extend([{'slot': 6, 'layer': str(map_path)}, {'slot': 7, 'value': int(invert)}])
        oracle_params['luma_map'] = generated_luma_map(width, height, mw, mh, invert)
    expected = render_case(width, height, **oracle_params)
    assert hashlib.sha256(expected).hexdigest() == recorded
    request, output = tmp_path / 'request.json', tmp_path / 'output.png'
    request.write_text(json.dumps({'schema_version': 1,
        'timing': {'frame': 0, 'fps': 24, 'duration_frames': 1},
        'assignments': assignments}), encoding='utf-8')
    run = subprocess.run([str(ROOT / 'broker/target/release/aexcompat-harness.exe'),
        '--headless', '--render-experimental-smart-request', plugin,
        str(tmp_path / 'input.png'), str(output), str(request)],
        cwd=ROOT, capture_output=True, timeout=90)
    (tmp_path / 'report.json').write_bytes(run.stdout)
    (tmp_path / 'stderr.log').write_bytes(run.stderr)
    assert run.returncode == 0, run.stderr.decode('utf-8', errors='replace')
    report = json.loads(run.stdout)
    assert report['passed'] and report['output_pixels_valid']
    with Image.open(output) as image:
        assert image.size == (width, height)
        actual_rgba = image.convert('RGBA').tobytes()
    actual = bytes(c for i in range(0, len(actual_rgba), 4)
                   for c in (actual_rgba[i+3], *actual_rgba[i:i+3]))
    assert hashlib.sha256(actual).hexdigest() == report['output_sha256']
    assert_reference(actual, expected, recorded)
