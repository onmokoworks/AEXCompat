"""Bounded installed-shader consistency, not AE parity or backend attestation.

GPU-off Diffusion currently differs from the embedded GPU algorithm. Keep that
comparison visibly unresolved rather than treating its observed output as a fix.
"""
import json
import hashlib
import math
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
WIDTH, HEIGHT = 32, 24


def pattern():
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in ((x * 7 + y * 3) % 256, (y * 9 + x * 2) % 256,
                           (x * 5 + y * 11) % 256, 255))


def expected(mode, radius=3):
    source = pattern()

    def sample(x, y):
        i = (max(0, min(HEIGHT - 1, y)) * WIDTH + max(0, min(WIDTH - 1, x))) * 4
        return source[i:i + 4]

    output = bytearray()
    for y in range(HEIGHT):
        for x in range(WIDTH):
            if mode == 3:
                pixels = [sample(x + dx, y + dy) for dy in range(-radius, radius + 1)
                          for dx in range(-radius, radius + 1)]
                value = [sum(p[c] for p in pixels) // len(pixels) for c in range(4)]
            elif mode == 4:
                center = sample(x, y)
                neighbors = [sample(x - 1, y), sample(x + 1, y),
                             sample(x, y - 1), sample(x, y + 1)]
                value = [int(center[c] * .5 + sum(p[c] for p in neighbors) * .125)
                         for c in range(4)]
            elif mode == 5:
                dx = int(math.sin(y / HEIGHT * 37) * 12)
                dy = int(math.cos(x / WIDTH * 31) * 8)
                value = [sample(x + dx, y)[0], sample(x, y + dy)[1],
                         sample(x - dx, y - dy)[2], sample(x, y)[3]]
            else:
                raise ValueError(mode)
            output.extend(value)
    return bytes(output)


def assert_pixels(actual, reference):
    assert len(actual) == len(reference) == WIDTH * HEIGHT * 4
    assert actual[3::4] == reference[3::4]
    assert actual == reference


@pytest.mark.parametrize('mode', [3, 4, 5])
@pytest.mark.parametrize('fault', ['passthrough', 'rgb', 'alpha', 'short'])
def test_spatial_oracle_rejects_corruption(mode, fault):
    reference = expected(mode)
    actual = bytearray(reference)
    if fault == 'passthrough':
        actual = pattern()
    elif fault == 'rgb':
        actual[0] ^= 1
    elif fault == 'alpha':
        actual[3] = 0
    else:
        actual = actual[:-4]
    with pytest.raises(AssertionError):
        assert_pixels(actual, reference)


@pytest.mark.parametrize('mode,gpu', [(3, 0), (3, 1), (4, 0), (4, 1), (5, 0), (5, 1)])
def test_real_aegpulab_spatial_shader_consistency(tmp_path, mode, gpu):
    plugin = os.environ.get('AEXCOMPAT_TEST_AEGPULAB')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_AEGPULAB to installed AeGpuLab.aex')
    source, output, request = (tmp_path / name for name in ('input.png', 'output.png', 'request.json'))
    Image.frombytes('RGBA', (WIDTH, HEIGHT), pattern()).save(source)
    request.write_text(json.dumps({'schema_version': 1,
        'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
        'assignments': [{'slot': s, 'value': v} for s, v in
                        ((1, mode), (2, gpu), (3, 1), (4, 3), (5, 1))]}), encoding='utf-8')
    run = subprocess.run([str(ROOT / 'broker/target/release/aexcompat-harness.exe'),
        '--headless', '--render-experimental-smart-request', plugin, str(source),
        str(output), str(request)], cwd=ROOT, capture_output=True, timeout=90)
    (tmp_path / 'report.json').write_bytes(run.stdout)
    (tmp_path / 'stderr.log').write_bytes(run.stderr)
    assert run.returncode == 0, run.stderr.decode('utf-8', errors='replace')
    report = json.loads(run.stdout)
    assert report['passed'] and report['output_pixels_valid']
    with Image.open(output) as im:
        im.load()
        assert im.size == (WIDTH, HEIGHT)
        actual = im.convert('RGBA').tobytes()
    assert actual != pattern() and actual[3::4] == pattern()[3::4]
    argb = bytes(c for i in range(0, len(actual), 4)
                 for c in (actual[i + 3], *actual[i:i + 3]))
    assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
    if mode == 4 and gpu == 0 and actual == expected(3, radius=1):
        pytest.xfail('Unresolved CPU/GPU Diffusion algorithm difference; not a host-fix verdict')
    assert_pixels(actual, expected(mode))
