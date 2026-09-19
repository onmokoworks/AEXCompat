"""Legacy BCC Flutter Cut timed response; not a complete AE timing oracle."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_bcc_blur_response import source_pixels
from test_bcc_transition_slot2_response import reveal_pixels
from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


FRAMES = (0, 146, 148, 150, 152, 154, 299)
REVEAL_STATES = (False, True, False, True, False, True, True)


def assert_flutter_timeline(outputs):
    source, reveal = source_pixels(), reveal_pixels()
    assert len(outputs) == len(FRAMES)
    assert all(output in (source, reveal) for output in outputs)
    assert tuple(output == reveal for output in outputs) == REVEAL_STATES


@pytest.mark.parametrize('fault', [
    'source_only', 'reveal_only', 'monotonic', 'blend', 'alpha', 'truncated',
    'missing_frame', 'wrong_order',
])
def test_obsolete_flutter_validator_rejects_corruption(fault):
    source, reveal = source_pixels(), reveal_pixels()
    outputs = [reveal if state else source for state in REVEAL_STATES]
    assert_flutter_timeline(outputs)
    if fault == 'source_only':
        outputs = [source] * len(FRAMES)
    elif fault == 'reveal_only':
        outputs = [reveal] * len(FRAMES)
    elif fault == 'monotonic':
        outputs = [source] * 3 + [reveal] * 4
    elif fault == 'blend':
        outputs[1] = bytes((a + b) // 2 for a, b in zip(source, reveal))
    elif fault in ('alpha', 'truncated'):
        corrupt = bytearray(outputs[1])
        if fault == 'alpha':
            corrupt[3] = 0
        else:
            corrupt = corrupt[:-4]
        outputs[1] = bytes(corrupt)
    elif fault == 'missing_frame':
        outputs.pop(1)
    else:
        outputs[1], outputs[2] = outputs[2], outputs[1]
    with pytest.raises(AssertionError):
        assert_flutter_timeline(outputs)


@pytest.mark.parametrize('plugin_name', (
    'BCCFlutterCut.aex',
    'BCCFlutterCutPrTr.aex',
))
def test_installed_bcc_obsolete_flutter_cut_timeline(tmp_path, plugin_name):
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
    assert parameters[2]['name'] == 'Incoming Clip'
    assert parameters[2]['kind'] == 'layer'
    for slot, name in ((4, 'Outgoing Start Frames'), (5, 'Outgoing End Frames'),
                       (10, 'Incoming Start Frames'), (11, 'Incoming End Frames')):
        assert parameters[slot]['name'] == name
        assert parameters[slot]['value'] == 2

    source, reveal = tmp_path / 'source.png', tmp_path / 'reveal.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    Image.frombytes('RGBA', (WIDTH, HEIGHT), reveal_pixels()).save(reveal)
    outputs = []
    for frame in FRAMES:
        request, output = tmp_path / f'{frame}.json', tmp_path / f'{frame}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': frame, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 2, 'layer': str(reveal)}],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_flutter_timeline(outputs)
