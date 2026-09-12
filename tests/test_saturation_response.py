"""Bounded HSI desaturation response; not a full color-model/AE oracle."""
import hashlib
import json
import os
from pathlib import Path
import subprocess

import pytest
from PIL import Image
from test_brightness_response import source_pixels
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def assert_desaturation(neutral, middle, gray):
    source = source_pixels()
    assert neutral == source
    for pixels in (middle, gray):
        assert len(pixels) == len(source)
        assert pixels[3::4] == source[3::4]
    assert len(set(gray[0::4])) > 1
    for i in range(0, len(source), 4):
        original, partial, final = source[i:i+3], middle[i:i+3], gray[i:i+3]
        assert 0 < max(partial) - min(partial) < max(original) - min(original)
        assert max(final) - min(final) <= 1
        assert min(original) - 1 <= min(final) <= max(final) <= max(original) + 1
        for a in range(3):
            for b in range(3):
                if original[a] < original[b]:
                    assert partial[a] <= partial[b]


def synthetic_desaturation():
    source = source_pixels()
    middle, gray = bytearray(source), bytearray(source)
    for i in range(0, len(source), 4):
        level = round(sum(source[i:i+3]) / 3)
        for c in range(3):
            gray[i+c] = level
            middle[i+c] = round((source[i+c] + level) / 2)
    return middle, gray


@pytest.mark.parametrize('fault', [
    'copy', 'early_gray', 'colored_end', 'flat', 'alpha', 'swap', 'truncated', 'neutral',
])
def test_desaturation_validator_rejects_corruption(fault):
    neutral = source_pixels()
    middle, gray = synthetic_desaturation()
    assert_desaturation(neutral, middle, gray)
    if fault == 'copy':
        middle = neutral
    elif fault == 'early_gray':
        middle = gray
    elif fault == 'colored_end':
        gray = middle
    elif fault == 'flat':
        gray = bytes((110, 110, 110, 255)) * (WIDTH * HEIGHT)
    elif fault == 'alpha':
        gray[3] = 0
    elif fault == 'swap':
        middle[0::4], middle[2::4] = middle[2::4], middle[0::4]
    elif fault == 'truncated':
        middle = middle[:-4]
    else:
        neutral = middle
    with pytest.raises(AssertionError):
        assert_desaturation(neutral, middle, gray)


@pytest.mark.parametrize('name,saturation_slot,color_slot', [
    ('BCCHLS.aex', 11, 8), ('BCCColorCorrection.aex', 13, 10),
])
def test_real_hsi_desaturation(tmp_path, name, saturation_slot, color_slot):
    folder = os.environ.get('AEXCOMPAT_TEST_SATURATION_DIR')
    if not folder:
        pytest.skip('set AEXCOMPAT_TEST_SATURATION_DIR to the local AEX directory')
    assert os.name == 'nt'
    plugin = Path(folder) / name
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run(
            [str(harness), '--headless', *map(str, args)], cwd=ROOT,
            capture_output=True,
            timeout=None if args[0] == '--inspect-experimental' else 90,
        )
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    params = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert params[6]['name'] == 'Host Layer' and params[6]['kind'] == 'layer'
    assert params[saturation_slot]['name'] == 'Saturation'
    assert params[color_slot]['name'] == 'Color Space'
    assert params[color_slot]['value'] == 2 and params[color_slot]['choices'][1] == 'HSI'
    minimum = params[saturation_slot]['minimum']
    assert minimum < -50
    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), source_pixels()).save(source)
    outputs = []
    for value in (0, -50, minimum):
        request, output = tmp_path / f'{value}.json', tmp_path / f'{value}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [{'slot': 6, 'layer': str(source)},
                            {'slot': saturation_slot, 'value': value}],
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
    assert_desaturation(*outputs)
