"""Bounded manual Flutter Cut response; not an AE timing/parity oracle."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image

from test_bcc_blur_response import source_pixels
from test_blur_dissolve_response import reveal_pixels
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def assert_flutter(outputs):
    """Require exact cut inputs, both endpoints and a reveal-to-source reversal."""
    source, reveal = source_pixels(), reveal_pixels()
    assert len(outputs) == 5
    assert outputs[0] == source
    assert outputs[-1] == reveal
    assert all(output in (source, reveal) for output in outputs)
    states = [output == reveal for output in outputs]
    assert any(left and not right for left, right in zip(states, states[1:]))


@pytest.mark.parametrize('fault', [
    'copy', 'reveal_only', 'monotonic', 'blend', 'alpha', 'channel_swap',
    'truncated', 'missing_frame', 'wrong_end',
])
def test_flutter_validator_rejects_corruption(fault):
    source, reveal = source_pixels(), reveal_pixels()
    outputs = [source, reveal, source, reveal, reveal]
    assert_flutter(outputs)
    if fault == 'copy':
        outputs = [source] * 5
    elif fault == 'reveal_only':
        outputs = [reveal] * 5
    elif fault == 'monotonic':
        outputs = [source, source, reveal, reveal, reveal]
    elif fault == 'blend':
        outputs[1] = bytes((a + b) // 2 for a, b in zip(source, reveal))
    elif fault in ('alpha', 'channel_swap'):
        corrupt = bytearray(reveal)
        if fault == 'alpha':
            corrupt[3] = 0
        else:
            corrupt[0::4], corrupt[2::4] = corrupt[2::4], corrupt[0::4]
        outputs[1] = bytes(corrupt)
    elif fault == 'truncated':
        outputs[1] = reveal[:-4]
    elif fault == 'missing_frame':
        outputs.pop(1)
    else:
        outputs[-1] = source
    with pytest.raises(AssertionError):
        assert_flutter(outputs)


def test_real_flutter_cut_manual_response(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_FLUTTER_CUT')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_FLUTTER_CUT to the local AEX')
    assert os.name == 'nt'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run(
            [str(harness), '--headless', *map(str, args)], cwd=ROOT,
            capture_output=True,
            timeout=None if args[0] == '--inspect-experimental' else 90,
        )
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {param['slot']: param for param in run('--inspect-experimental', plugin)}
    assert params[7]['choices'] == ['Auto', 'Manual Pct Done']
    assert params[8]['name'] == 'Amount'
    assert params[9]['name'] == 'Layer to Reveal' and params[9]['kind'] == 'layer'
    source, reveal = tmp_path / 'source.png', tmp_path / 'reveal.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    Image.frombytes('RGBA', (WIDTH, HEIGHT), reveal_pixels()).save(reveal)
    outputs = []
    # The sparse sequence demonstrated a reversal with the installed defaults.
    # No assertion is made about every intermediate amount or the AE schedule.
    for amount in (0, 20, 30, 40, 100):
        request, output = tmp_path / f'{amount}.json', tmp_path / f'{amount}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 7, 'value': 2}, {'slot': 8, 'value': amount},
                {'slot': 9, 'layer': str(reveal)},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output, request)
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert('RGBA').tobytes()
        argb = bytearray(len(pixels))
        argb[0::4], argb[1::4] = pixels[3::4], pixels[0::4]
        argb[2::4], argb[3::4] = pixels[1::4], pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        outputs.append(pixels)
    assert_flutter(outputs)
