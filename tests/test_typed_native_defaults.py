"""Installed-AEX regression for native defaults versus explicit typed edits."""
import hashlib
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image


def test_signal_native_default_survives_empty_typed_request(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_SIGNAL')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_SIGNAL to installed signal.aex')
    root = Path(__file__).resolve().parents[1]
    harness = root / 'broker/target/release/aexcompat-harness.exe'

    def run(*args, timeout=90):
        return subprocess.run([str(harness), '--headless', *map(str, args)],
                              cwd=root, capture_output=True, timeout=timeout)

    inspected = run('--inspect-experimental', plugin, timeout=None)
    assert inspected.returncode == 0, inspected.stderr
    parameters = {p['slot']: p for p in json.loads(inspected.stdout)}
    seed = parameters[1]
    assert seed['name'] == 'Random seed'
    assert seed['value'] == 0 and seed['minimum'] == 1
    source = tmp_path / 'input.png'
    Image.new('RGBA', (256, 144), (32, 64, 128, 255)).save(source)
    for label, assignments in [('native', []), ('invalid', [{'slot': 1, 'value': 0}])]:
        request, output = tmp_path / f'{label}.json', tmp_path / f'{label}.png'
        request.write_text(json.dumps({'schema_version': 1, 'assignments': assignments,
                                      'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300}}),
                           encoding='utf-8')
        result = run('--render-experimental-request', plugin, source, output, request)
        if label == 'invalid':
            assert result.returncode != 0
            assert b'out of range' in result.stderr
            assert not output.exists()
            continue
        assert result.returncode == 0, result.stderr
        report = json.loads(result.stdout)
        assert report['passed'] and report.get('output_pixels_valid') is not False
        with Image.open(output) as image:
            assert image.size == (256, 144)
            raw = image.convert('RGBA').tobytes()
        assert all(a == 255 for a in raw[3::4])
        assert raw != bytes((32, 64, 128, 255)) * (256 * 144)
        argb = bytes(c for i in range(0, len(raw), 4) for c in (raw[i+3], *raw[i:i+3]))
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
