"""BCC Burnt Film manual transition response; not exact AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_bcc_blur_response import source_pixels
from test_bcc_transition_slot2_response import assert_transition, reveal_pixels
from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


@pytest.mark.parametrize('plugin_name', (
    'BCCBurntFilm.aex',
    'BCCBurntFilmPrTr.aex',
))
def test_installed_bcc_burnt_film_tracks_manual_percent(tmp_path, plugin_name):
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
    assert parameters[2]['name'] == 'Animation'
    assert parameters[2]['choices'] == ['Manual', 'Auto', 'Pct. Done']
    assert parameters[3]['name'] == 'Percent Done'
    assert parameters[3]['minimum'] <= 0 < 50 < 100 <= parameters[3]['maximum']
    assert parameters[5]['name'] == 'Layer to Reveal'
    assert parameters[5]['kind'] == 'layer'
    assert sum(p['kind'] == 'layer' for p in parameters.values()) == 5

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
                {'slot': 2, 'value': 3},
                {'slot': 3, 'value': percent},
                {'slot': 5, 'layer': str(reveal)},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[2] == 3 and requested[3] == percent
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_transition(*outputs)
