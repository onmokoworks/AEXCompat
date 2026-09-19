"""Legacy BCC Two Way Key keep/remove alpha response; not exact AE parity."""
import json
import os
import subprocess
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH


COLORS = ((0, 255, 0), (0, 200, 55), (55, 200, 0), (255, 0, 0))


class MissingKeepColorResponse(AssertionError):
    """The known signature where keying works but both keep colors are inert."""


def band_pixels():
    return bytes(
        channel
        for _y in range(HEIGHT)
        for x in range(WIDTH)
        for channel in (*COLORS[min(3, x * 4 // WIDTH)], 255)
    )


def expected_keep_pixels(keep_band):
    output = bytearray(band_pixels())
    for y in range(HEIGHT):
        for x in range(WIDTH):
            band = min(3, x * 4 // WIDTH)
            if band < 3 and band != keep_band:
                output[(y * WIDTH + x) * 4 + 3] = 0
    return output


def expected_key_only_pixels():
    output = bytearray(band_pixels())
    for y in range(HEIGHT):
        for x in range(WIDTH):
            if min(3, x * 4 // WIDTH) < 3:
                output[(y * WIDTH + x) * 4 + 3] = 0
    return output


@pytest.mark.xfail(
    raises=MissingKeepColorResponse,
    strict=True,
    reason='known signature: keying works but exact Keep Color is inert',
)
def test_installed_bcc_obsolete_two_way_key_response(tmp_path):
    directory = os.environ.get('AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    if not directory:
        pytest.skip('requires AEXCOMPAT_TEST_STATIC_BLUR_DIR')
    plugin = Path(directory) / 'BCCTwoWayKey.aex'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'

    def run(*args):
        result = subprocess.run([str(harness), '--headless', *map(str, args)],
                                cwd=ROOT, capture_output=True,
                                timeout=None if args[0] == '--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8', errors='replace')
        return json.loads(result.stdout)

    parameters = {p['slot']: p for p in run('--inspect-experimental', plugin)}
    for slot, name, kind in (
            (6, 'Host Layer', 'layer'), (8, 'Output', 'integer'),
            (9, 'Key Color', 'color'), (10, 'Similarity', 'float'),
            (11, 'Keep Color', 'color'), (12, 'Keep Similarity', 'float'),
            (13, 'Softness', 'float'), (19, 'Region of Interest', 'integer')):
        assert parameters[slot]['name'] == name
        assert parameters[slot]['kind'] == kind

    source = tmp_path / 'source.png'
    Image.frombytes('RGBA', (WIDTH, HEIGHT), band_pixels()).save(source)
    outputs = []
    for name, keep_color in (('keep-a', [255, 0, 200, 55]),
                             ('keep-b', [255, 55, 200, 0])):
        request = tmp_path / f'{name}.json'
        output = tmp_path / f'{name}.png'
        request.write_text(json.dumps({
            'schema_version': 1,
            'timing': {'frame': 0, 'fps': 30, 'duration_frames': 300},
            'assignments': [
                {'slot': 6, 'layer': str(source)},
                {'slot': 8, 'value': 1},
                {'slot': 9, 'color': [255, 0, 255, 0]},
                {'slot': 10, 'value': 100},
                {'slot': 11, 'color': keep_color},
                {'slot': 12, 'value': 1},
                {'slot': 13, 'value': 0},
                {'slot': 14, 'value': 0},
                {'slot': 15, 'value': 1},
                {'slot': 16, 'value': 0},
                {'slot': 17, 'value': 0},
                {'slot': 19, 'value': 5},
                {'slot': 22, 'value': 2},
            ],
        }), encoding='utf-8')
        report = run('--render-experimental-smart-request', plugin, source, output,
                     request)
        assert report['passed'] and report['output_pixels_valid']
        requested = {p['slot']: p for p in report['requested_parameters']}
        assert requested[8]['value'] == 1
        assert requested[10]['value'] == 100
        received = requested[11]['value']
        assert [received[channel] for channel in
                ('alpha', 'red', 'green', 'blue')] == keep_color
        assert requested[12]['value'] == 1
        assert requested[13]['value'] == 0
        assert requested[19]['value'] == 5
        with Image.open(output) as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())

    key_only = expected_key_only_pixels()
    if outputs[0] == outputs[1] == key_only:
        raise MissingKeepColorResponse(
            'both exact Keep Color choices produced the same key-only alpha')

    assert outputs[0] == expected_keep_pixels(1)
    assert outputs[1] == expected_keep_pixels(2)
