"""Legacy BCC Primatte Studio factory-state keying response; not AE parity."""
import base64
import json
import os
import statistics
import subprocess
import xml.etree.ElementTree as ET
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


FOREGROUND = (220, 32, 24, 255)
# `BG Color -KF only` in the installed Default Green Screen factory preset.
BACKGROUND = (36, 96, 57, 255)
LEFT = WIDTH // 3
RIGHT = WIDTH * 2 // 3
TOP = HEIGHT // 4
BOTTOM = HEIGHT * 3 // 4


def green_screen_pixels():
    return bytes(
        channel
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in (FOREGROUND if LEFT <= x < RIGHT and TOP <= y < BOTTOM
                        else BACKGROUND)
    )


def region_gray(raw, foreground):
    values = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            inside = LEFT <= x < RIGHT and TOP <= y < BOTTOM
            if inside != foreground:
                continue
            offset = (y * WIDTH + x) * 4
            values.append(raw[offset])
    return values


def matte_pixels(background=56, foreground=255):
    pixels = bytearray()
    for y in range(HEIGHT):
        for x in range(WIDTH):
            inside = LEFT <= x < RIGHT and TOP <= y < BOTTOM
            value = foreground if inside else background
            pixels.extend((value, value, value, 255))
    return bytes(pixels)


def assert_primatte_response(source, source_view, matte):
    assert len(source) == len(source_view) == len(matte) == WIDTH * HEIGHT * 4
    assert source_view == source
    assert matte != source
    assert matte[0::4] == matte[1::4] == matte[2::4]
    assert matte[3::4] == bytes([255]) * WIDTH * HEIGHT
    foreground = region_gray(matte, True)
    background = region_gray(matte, False)
    metrics = {
        'foreground_mean': statistics.mean(foreground),
        'background_mean': statistics.mean(background),
        'foreground_min': min(foreground),
        'background_max': max(background),
    }
    assert metrics['foreground_mean'] >= 240, metrics
    assert metrics['foreground_min'] >= 220, metrics
    assert metrics['background_mean'] <= 64, metrics
    assert metrics['background_max'] <= 80, metrics
    assert metrics['foreground_mean'] - metrics['background_mean'] >= 180, metrics


def primatte_blob(preset):
    root = ET.parse(preset).getroot()
    for parameter in root.findall('./paramlist/param'):
        if parameter.findtext('name') != 'Primatte Data':
            continue
        data = parameter.findtext('./custom/primattedata/data')
        assert data
        decoded = base64.b64decode(data, validate=True)
        assert len(decoded) == 2388
        return decoded
    raise AssertionError('Primatte Data is missing from the factory preset')


def test_installed_bcc_obsolete_primatte_studio_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCPrimatteStudio.aex'
    preset = (Path(os.environ['PROGRAMDATA']) / 'BorisFX/Continuum/2026.5/Presets'
              / 'BCC Obsolete/BCC Primatte Studio/Default Green Screen.bsp')
    if not preset.is_file():
        pytest.skip('requires installed BCC Primatte Studio factory presets')
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True, timeout=90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    inspected = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    assert inspected[9]['name'] == 'View Options'
    assert inspected[9]['choices'][:3] == [
        'Final Composite', 'Matte Status', 'Final Matte']
    assert inspected[10]['name'] == 'Primatte Data'
    assert inspected[10]['kind'] == 'arbitrary_data'

    source = tmp_path / 'green-screen.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), green_screen_pixels()).save(source)
    state = list(primatte_blob(preset))

    outputs = {}
    reports = {}
    for label, view in (('source', 8), ('matte', 3)):
        sidecar = tmp_path / f'{label}.json'
        output = tmp_path / f'{label}.png'
        animations = []
        for slot, value in ((9, view), (11, 0), (13, 1), (14, 3)):
            animations.append({
                'slot': slot,
                'keys': [{
                    'time': {'value': 0, 'scale': 30},
                    'interpolation': 'hold',
                    'value': {'type': 'scalar', 'value': value},
                }],
            })
        animations.append({
            'slot': 10,
            'keys': [{
                'time': {'value': 0, 'scale': 30},
                'interpolation': 'hold',
                'value': {'type': 'arbitrary', 'value': state},
            }],
        })
        sidecar.write_text(json.dumps({
            'schema_version': 1,
            'parameters': animations,
        }), encoding='utf-8')
        report = run('--render-experimental-session-animation', plugin, source,
                     output, 0, 300, 30, sidecar)
        assert report['passed']
        assert report['worker_classification'] == 'ok'
        assert output.is_file() and output.stat().st_size > 0
        reports[label] = report
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs[label] = image.convert('RGBA').tobytes()

    assert_primatte_response(green_screen_pixels(), outputs['source'], outputs['matte'])
    assert reports['matte']['current_time'] == 0
    assert reports['matte']['time_scale'] == 30


@pytest.mark.parametrize('mutation', [
    'copy', 'white', 'black', 'reverse', 'weak_background', 'dim_foreground',
    'color', 'alpha', 'wrong_source', 'truncated',
])
def test_primatte_validator_rejects_mutations(mutation):
    source = green_screen_pixels()
    source_view = source
    matte = matte_pixels()
    assert_primatte_response(source, source_view, matte)

    if mutation == 'copy':
        matte = source
    elif mutation == 'white':
        matte = matte_pixels(255, 255)
    elif mutation == 'black':
        matte = matte_pixels(0, 0)
    elif mutation == 'reverse':
        matte = matte_pixels(255, 56)
    elif mutation == 'weak_background':
        matte = matte_pixels(90, 255)
    elif mutation == 'dim_foreground':
        matte = matte_pixels(56, 210)
    elif mutation == 'color':
        changed = bytearray(matte)
        changed[1] = 12
        matte = bytes(changed)
    elif mutation == 'alpha':
        changed = bytearray(matte)
        changed[3] = 0
        matte = bytes(changed)
    elif mutation == 'wrong_source':
        source_view = matte
    elif mutation == 'truncated':
        matte = matte[:-4]

    with pytest.raises(AssertionError):
        assert_primatte_response(source, source_view, matte)
