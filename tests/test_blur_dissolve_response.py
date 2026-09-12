"""Manual transition endpoints and spatial midpoint response, not AE parity."""
import json
import os
import subprocess

import pytest
from PIL import Image, ImageFilter
from test_bcc_blur_response import source_pixels
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def reveal_pixels():
    return bytes((16, 48, 224, 255)) * (WIDTH * HEIGHT)


def assert_transition(start, middle, end):
    assert start == source_pixels()
    assert end == reveal_pixels()
    assert len(middle) == WIDTH * HEIGHT * 4
    assert middle[3::4] == bytes([255]) * (WIDTH * HEIGHT)
    row = [middle[(72 * WIDTH + x) * 4:(72 * WIDTH + x + 1) * 4] for x in range(WIDTH)]
    assert all(abs(a-b) <= 1 for a,b in zip(row[0], (8,24,112,255)))
    assert all(abs(a-b) <= 1 for a,b in zip(row[128], (135,151,239,255)))
    for channel, low, high in ((0,8,135),(1,24,151),(2,112,239)):
        assert all(a[channel] <= b[channel] for a,b in zip(row[48:80],row[49:81]))
        assert all(a[channel] >= b[channel] for a,b in zip(row[176:208],row[177:209]))
        for x in (60,63,64,68,187,191,192,195):
            assert low < row[x][channel] < high


def synthetic_midpoint(blur=True):
    source = Image.frombytes('RGBA', (WIDTH,HEIGHT), source_pixels())
    if blur:
        source = source.filter(ImageFilter.GaussianBlur(8))
    reveal = Image.frombytes('RGBA', (WIDTH,HEIGHT), reveal_pixels())
    return bytearray(Image.blend(source,reveal,0.5).tobytes())


@pytest.mark.parametrize('fault',['copy','crossfade_only','alpha','channel_swap','wrong_end','truncated'])
def test_transition_validator_rejects_corruption(fault):
    start,middle,end = source_pixels(),synthetic_midpoint(),reveal_pixels()
    assert_transition(start,middle,end)
    if fault == 'copy': middle = start
    elif fault == 'crossfade_only': middle = synthetic_midpoint(False)
    elif fault == 'alpha': middle[3] = 0
    elif fault == 'channel_swap': middle[0::4],middle[2::4] = middle[2::4],middle[0::4]
    elif fault == 'wrong_end': end = start
    else: middle = middle[:-4]
    with pytest.raises(AssertionError):
        assert_transition(start,middle,end)


def test_real_blur_dissolve_manual_response(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_BLUR_DISSOLVE')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_BLUR_DISSOLVE to the approved AEX')
    assert os.name == 'nt'
    harness = ROOT / 'broker/target/release/aexcompat-harness.exe'
    def run(*args):
        result = subprocess.run([str(harness),'--headless',*map(str,args)],cwd=ROOT,
            capture_output=True,timeout=None if args[0]=='--inspect-experimental' else 90)
        assert result.returncode == 0,result.stderr.decode('utf-8',errors='replace')
        return json.loads(result.stdout)
    params = {p['slot']:p for p in run('--inspect-experimental',plugin)}
    assert params[7]['choices'] == ['Auto','Manual Pct Done']
    assert params[8]['name'] == 'Layer to Reveal' and params[8]['kind'] == 'layer'
    assert params[9]['name'] == 'Percent Done'
    source,reveal = tmp_path/'source.png',tmp_path/'reveal.png'
    Image.frombytes('RGBA',(WIDTH,HEIGHT),source_pixels()).save(source)
    Image.frombytes('RGBA',(WIDTH,HEIGHT),reveal_pixels()).save(reveal)
    outputs = []
    for percent in (0,50,100):
        request,output = tmp_path/f'{percent}.json',tmp_path/f'{percent}.png'
        request.write_text(json.dumps({'schema_version':1,'timing':{'frame':0,'fps':30,'duration_frames':300},
            'assignments':[{'slot':7,'value':2},{'slot':8,'layer':str(reveal)},{'slot':9,'value':percent}]}),encoding='utf-8')
        report = run('--render-experimental-smart-request',plugin,source,output,request)
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH,HEIGHT)
            outputs.append(image.convert('RGBA').tobytes())
    assert_transition(*outputs)
