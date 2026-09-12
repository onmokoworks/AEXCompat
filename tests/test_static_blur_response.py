"""Static blur amount response, not exact kernels or AE equivalence."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image
from test_bcc_blur_response import source_pixels, synthetic
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def assert_static_blur(zero, active):
    assert zero == source_pixels()
    assert len(active) == WIDTH * HEIGHT * 4
    assert active[3::4] == bytes([255]) * WIDTH * HEIGHT
    assert active[0::4] == active[1::4] == active[2::4]
    row = active[72 * WIDTH * 4:73 * WIDTH * 4:4]
    assert row[0] == row[-1] == 0 and row[128] == 255
    assert all(a <= b for a, b in zip(row[:128], row[1:129]))
    assert all(a >= b for a, b in zip(row[128:], row[129:]))
    for lo, hi in ((32, 64), (64, 96), (160, 192), (192, 224)):
        assert any(0 < value < 255 for value in row[lo:hi])


@pytest.mark.parametrize('fault', ['copy', 'transparent', 'color', 'truncated', 'reversed', 'wrong_zero'])
def test_static_blur_validator_rejects_corruption(fault):
    zero, active = source_pixels(), synthetic(4)
    assert_static_blur(zero, active)
    if fault == 'copy': active = zero
    elif fault == 'transparent': active[3] = 0
    elif fault == 'color': active[0] = 1
    elif fault == 'truncated': active = active[:-4]
    elif fault == 'reversed':
        offset = (72 * WIDTH + 63) * 4
        active[offset:offset + 3] = bytes([255]) * 3
    else: zero = active
    with pytest.raises(AssertionError):
        assert_static_blur(zero, active)


@pytest.mark.parametrize('name,controls,amount', [
    ('BCCGaussianBlur', [(9, 'Horizontal Blur'), (10, 'Vertical Blur')], 2),
    ('BCCFastBlur', [(10, 'Horizontal Blur'), (11, 'Vertical Blur')], 10),
    ('BCCDirectionalBlur', [(8, 'Blur Amount')], 10),
])
def test_real_static_blur_amount_response(tmp_path, name, controls, amount):
    folder = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not folder:
        pytest.skip('set AEXCOMPAT_TEST_STATIC_BLUR_DIR to the approved fixture folder')
    assert os.name == 'nt'
    plugin = Path(folder) / (name + '.aex')
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)], cwd=ROOT,
            capture_output=True, timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert params[6]['name'] == 'Host Layer' and params[6]['kind'] == 'layer'
    for slot, label in controls:
        assert params[slot]['name'] == label
        assert params[slot]['minimum'] <= 0 < amount <= params[slot]['maximum']
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for value in (0, amount):
        request, output = tmp_path / f'{value}.json', tmp_path / f'{value}.png'
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 6, 'layer': str(source)}] +
                [{'slot': slot, 'value': value} for slot, _ in controls]}), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output, request)
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_static_blur(*outputs)
