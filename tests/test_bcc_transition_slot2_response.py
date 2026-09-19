"""BCC slot-2 transition endpoints and midpoint; not exact AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_bcc_blur_response import source_pixels
from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


PLUGINS = (
    'BCCColorizeGlowDissolve.aex',
    'BCCColorizeGlowDissolvePrTr.aex',
    'BCCCompositeDissolve.aex',
    'BCCCompositeDissolvePrTr.aex',
    'BCCCrossMelt.aex',
    'BCCCrossMeltPrTr.aex',
    'BCCDamagedTVDissolve.aex',
    'BCCDamagedTVDissolvePrTr.aex',
    'BCCGridWipe.aex',
    'BCCGridWipePrTr.aex',
    'BCCKaleidaDissolve.aex',
    'BCCKaleidaDissolvePrTr.aex',
    'BCCLensBlurDissolve.aex',
    'BCCLensBlurDissolvePrTr.aex',
    'BCCLensDistortionWipe.aex',
    'BCCLensDistortionWipePrTr.aex',
    'BCCLensFlareRound.aex',
    'BCCLensFlareRoundPrTr.aex',
    'BCCLensFlareSpiked.aex',
    'BCCLensFlareSpikedPrTr.aex',
    'BCCLensFlash.aex',
    'BCCLensFlashPrTr.aex',
    'BCCLightWipe.aex',
    'BCCLightWipePrTr.aex',
    'BCCParticleIllusionDissolve.aex',
    'BCCParticleIllusionDissolvePrTr.aex',
    'BCCRGBDisplacementDissolve.aex',
    'BCCRGBDisplacementDissolvePrTr.aex',
    'BCCTileWipe.aex',
    'BCCTileWipePrTr.aex',
    'BCCTritoneDissolve.aex',
    'BCCTritoneDissolvePrTr.aex',
    'BCCTwister.aex',
    'BCCTwisterPrTr.aex',
    'BCCWaterWaveDissolve.aex',
    'BCCWaterWavesDissolvePrTr.aex',
    'BCCCurlDissolve.aex',
    'BCCCurlDissolvePrTr.aex',
    'BCCLensFlareDissolve.aex',
    'BCCLensFlareDissolvePrTr.aex',
    'BCCVectorBlurDissolve.aex',
    'BCCVectorBlurDissolvePrTr.aex',
)


def reveal_pixels():
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH)
                 for c in (16 + x // 2, 48 + y, 224 - x // 3, 255))


def assert_transition(start, middle, end):
    source, reveal = source_pixels(), reveal_pixels()
    assert len(start) == len(middle) == len(end) == WIDTH * HEIGHT * 4
    assert start == source
    assert end == reveal
    assert all(middle[3::4])
    assert middle != start and middle != end
    assert len({bytes(middle[offset:offset + 4])
                for offset in range(0, len(middle), 4)}) > 1
    assert any(middle[offset:offset + 3] != start[offset:offset + 3]
               and middle[offset:offset + 3] != end[offset:offset + 3]
               for offset in range(0, len(middle), 4))


@pytest.mark.parametrize('fault', ['wrong_start', 'wrong_end', 'fixed_start',
                                   'fixed_end', 'constant', 'transparent', 'truncated'])
def test_transition_cohort_validator_rejects_corruption(fault):
    source, reveal = source_pixels(), reveal_pixels()
    middle = bytearray(Image.blend(
        Image.frombytes('RGBA', (WIDTH, HEIGHT), source),
        Image.frombytes('RGBA', (WIDTH, HEIGHT), reveal), 0.5).tobytes())
    start, end = source, reveal
    assert_transition(start, middle, end)
    if fault == 'wrong_start':
        start = reveal
    elif fault == 'wrong_end':
        end = source
    elif fault == 'fixed_start':
        middle = source
    elif fault == 'fixed_end':
        middle = reveal
    elif fault == 'constant':
        middle = bytes((32, 64, 128, 255)) * WIDTH * HEIGHT
    elif fault == 'transparent':
        middle[3::4] = bytes(WIDTH * HEIGHT)
    else:
        middle = middle[:-4]
    with pytest.raises(AssertionError):
        assert_transition(start, middle, end)


@pytest.mark.parametrize('plugin_name', PLUGINS)
def test_installed_bcc_slot2_transition_tracks_manual_percent(tmp_path, plugin_name):
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
    assert parameters[2]['name'] == 'Layer to Reveal'
    assert parameters[2]['kind'] == 'layer'
    assert parameters[3]['name'] == 'Animation'
    assert parameters[3]['choices'] == ['Auto', 'Pct. Done']
    assert parameters[4]['name'] == 'Percent Done'
    assert parameters[4]['minimum'] <= 0 < 50 < 100 <= parameters[4]['maximum']

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
                {'slot': 3, 'value': 2},
                {'slot': 4, 'value': percent},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output, request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p['value'] for p in report['requested_parameters']}
        assert requested[3] == 2 and requested[4] == percent
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_transition(*outputs)
