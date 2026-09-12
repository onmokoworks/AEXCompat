"""Low-chroma hue roundtrip invariants, not an exact color-rotation oracle."""
import hashlib
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def source_pixels():
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in ((112+4*(x%8), 112+4*((x//8)%8), 112+4*(y%8), 255)
                           if y < 128 else (64+x//2,)*3+(255,)))


def assert_roundtrip(forward, restored):
    source = source_pixels()
    assert len(forward) == len(source)
    assert max(abs(a-b) for a, b in zip(forward[:128*WIDTH*4], source[:128*WIDTH*4])) > 3
    for pixels in (forward, restored):
        assert len(pixels) == len(source) and pixels[3::4] == source[3::4]
        for i in range(0, len(pixels), 4):
            assert all(0 < pixels[i+c] < 255 for c in range(3))
            assert abs(sum(w*(pixels[i+c]-source[i+c])
                           for c, w in enumerate((.299, .587, .114)))) <= 1.000001
        assert max(abs(a-b) for a, b in zip(pixels[128*WIDTH*4:], source[128*WIDTH*4:])) <= 1
    assert max(abs(a-b) for a, b in zip(restored, source)) <= 3


@pytest.mark.parametrize('fault', ['copy', 'alpha', 'luma', 'gray', 'roundtrip', 'truncated',
                                  'inverse_ignored', 'tiny_change'])
def test_grade_validator_rejects_corruption(fault):
    source = source_pixels()
    forward, restored = bytearray(source), bytearray(source)
    for i in range(0, 128*WIDTH*4, 4):
        forward[i] += 10
        forward[i+1] -= 5
    assert_roundtrip(forward, restored)
    if fault == 'copy':
        forward = source
    elif fault == 'alpha':
        restored[3] = 0
    elif fault == 'luma':
        forward[1] += 10
    elif fault == 'gray':
        i = 128*WIDTH*4
        forward[i] += 10
        forward[i+1] -= 5
    elif fault == 'roundtrip':
        restored[0] += 10
        restored[1] -= 5
    elif fault == 'inverse_ignored':
        restored = forward
    elif fault == 'tiny_change':
        forward = bytearray(source)
        forward[0] += 1
        restored = forward
    else:
        restored = restored[:-4]
    with pytest.raises(AssertionError):
        assert_roundtrip(forward, restored)


def test_real_grade_roundtrip(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_GRADE')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_GRADE to the local AEX')
    assert os.name == 'nt'
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)], cwd=ROOT,
                                capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert params[181]['name'] == 'Host Layer' and params[181]['kind'] == 'layer'
    assert params[255]['name'] == 'Master' and params[255]['kind'] == 'group_start'
    assert params[256]['name'] == 'Hue'
    source = tmp_path/'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for name, angle in (('forward', 120), ('roundtrip', 240)):
        request, output = tmp_path/f'{name}.json', tmp_path/f'{name}.png'
        request.write_text(json.dumps({'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 181, 'layer': str(source)},
                            {'slot': 256, 'value': angle}]}), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output, request)
        (tmp_path/f'{name}-report.json').write_text(json.dumps(report), encoding='utf-8')
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.convert('RGBA').tobytes()
        argb = bytearray(len(pixels))
        argb[0::4], argb[1::4] = pixels[3::4], pixels[0::4]
        argb[2::4], argb[3::4] = pixels[1::4], pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        outputs.append(pixels)
        source = output
    assert_roundtrip(*outputs)
