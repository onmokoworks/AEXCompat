"""Displacement Dissolve manual response with its built-in map; not AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_bcc_blur_response import source_pixels
from test_bcc_transition_slot2_response import assert_transition, reveal_pixels
from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


def test_real_displacement_dissolve_manual_response(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_DISPLACEMENT_DISSOLVE')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_DISPLACEMENT_DISSOLVE to the local AEX')
    assert os.name == 'nt'
    plugin = Path(plugin)
    assert plugin.is_file()
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert parameters[8]['name'] == 'Animation'
    assert parameters[8]['choices'] == ['Auto', 'Manual Pct Done']
    assert parameters[9]['name'] == 'Layer to Reveal'
    assert parameters[9]['kind'] == 'layer'
    assert parameters[10]['name'] == 'Percent Done'
    assert parameters[10]['minimum'] <= 0 < 50 < 100 <= parameters[10]['maximum']
    assert parameters[12]['name'] == 'Map Type'
    assert parameters[12]['choices'] == ['Layer', 'Displacement Map']
    assert parameters[12]['value'] == 2

    source, reveal = tmp_path / 'source.png', tmp_path / 'reveal.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    Image.frombytes('RGBA', (WIDTH, HEIGHT), reveal_pixels()).save(reveal)
    outputs = []
    for percent in (0, 50, 100):
        request, output = tmp_path / f'{percent}.json', tmp_path / f'{percent}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 8, 'value': 2},
                {'slot': 9, 'layer': str(reveal)},
                {'slot': 10, 'value': percent},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[8] == 2 and requested[10] == percent
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_transition(*outputs)
