"""BCC multi-layer slot-2 transitions track manual progress; not AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_bcc_blur_response import source_pixels
from test_bcc_transition_slot2_response import assert_transition, reveal_pixels
from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


PLUGINS = (
    ('BCCBlobsWipe.aex', ('Auto', 'Pct. Done'), 2),
    ('BCCBlobsWipePrTr.aex', ('Auto', 'Pct. Done'), 2),
    ('BCCCheckerWipe.aex', ('Auto', 'Pct. Done'), 2),
    ('BCCCheckerWipePrTr.aex', ('Auto', 'Pct. Done'), 2),
    ('BCCCrissCrossWipe.aex', ('Manual', 'Auto', 'Pct. Done'), 3),
    ('BCCCrissCrossWipePrTr.aex', ('Manual', 'Auto', 'Pct. Done'), 3),
    ('BCCMultiStretchWipe.aex', ('Manual', 'Auto', 'Pct. Done'), 3),
    ('BCCMultiStretchWipePrTr.aex', ('Manual', 'Auto', 'Pct. Done'), 3),
    ('BCCMultiStripeWipe.aex', ('Manual', 'Auto', 'Pct. Done'), 3),
    ('BCCMultiStripeWipePrTr.aex', ('Manual', 'Auto', 'Pct. Done'), 3),
    ('BCCRibbonWipe.aex', ('Auto', 'Pct. Done'), 2),
    ('BCCRibbonWipePrTr.aex', ('Auto', 'Pct. Done'), 2),
    ('BCCRingsWipe.aex', ('Auto', 'Pct. Done'), 2),
    ('BCCRingsWipePrTr.aex', ('Auto', 'Pct. Done'), 2),
)


@pytest.mark.parametrize('plugin_name,animation_choices,manual_value', PLUGINS)
def test_installed_bcc_slot2_multilayer_transition_tracks_manual_percent(
        tmp_path, plugin_name, animation_choices, manual_value):
    directory = os.environ.get('AEXCOMPAT_TEST_BCC_TRANSITION_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_BCC_TRANSITION_DIR')
    assert os.name == 'nt'
    plugin = Path(directory) / plugin_name
    assert plugin.is_file()
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert parameters[2]['name'] == 'Background'
    assert parameters[2]['kind'] == 'layer'
    assert parameters[3]['name'] == 'Animation'
    assert parameters[3]['choices'] == list(animation_choices)
    assert parameters[4]['name'] == 'Percent Done'
    assert parameters[4]['minimum'] <= 0 < 50 < 100 <= parameters[4]['maximum']
    assert sum(p['kind'] == 'layer' for p in parameters.values()) > 1

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
                {'slot': 2, 'layer': str(reveal)},
                {'slot': 3, 'value': manual_value},
                {'slot': 4, 'value': percent},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[3] == manual_value and requested[4] == percent
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_transition(*outputs)
