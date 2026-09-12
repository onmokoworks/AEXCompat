"""Conditional emission response; trajectories and AE equivalence are unverified."""
import json
import os
import subprocess

import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def assert_emission_response(zero_time, emitted, zero_rate):
    for raw in (zero_time, emitted, zero_rate):
        assert len(raw) == WIDTH * HEIGHT * 4
    assert not any(zero_time[3::4])
    assert not any(zero_rate[3::4])
    visible = [i for i, a in enumerate(emitted[3::4]) if a]
    assert 1 < len(visible) < WIDTH * HEIGHT // 4
    assert any(any(emitted[i*4:i*4+3]) for i in visible)


@pytest.mark.parametrize('fault', ['empty', 'fixed', 'rate_ignored', 'opaque', 'black', 'truncated'])
def test_emission_validator_rejects_corruption(fault):
    empty = bytes(WIDTH * HEIGHT * 4)
    emitted = bytearray(empty)
    emitted[400:412] = bytes((255,255,255,255)) * 3
    cases = [empty, emitted, empty]
    assert_emission_response(*cases)
    if fault == 'empty': cases[1] = empty
    elif fault == 'fixed': cases[0] = emitted
    elif fault == 'rate_ignored': cases[2] = emitted
    elif fault == 'opaque': cases[1] = bytes((255,255,255,255))*WIDTH*HEIGHT
    elif fault == 'black':
        for channel in range(3): emitted[channel::4] = bytes(WIDTH*HEIGHT)
    else: cases[1] = emitted[:-4]
    with pytest.raises(AssertionError):
        assert_emission_response(*cases)


def test_installed_particle_emitter_time_and_birthrate(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_PARTICLE_EMITTER')
    if not plugin:
        pytest.skip('requires AEXCOMPAT_TEST_PARTICLE_EMITTER')
    assert os.name == 'nt'
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'
    def run(*args):
        result = subprocess.run([str(harness),'--headless',*map(str,args)],
            cwd=ROOT,capture_output=True,
            timeout=None if args[0]=='--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8',errors='replace')
        return json.loads(result.stdout)
    inspection = run('--inspect-experimental',plugin)
    birthrate = next(p for p in inspection if p['slot']==15)
    assert birthrate['name']=='Birthrate' and birthrate['kind']=='float'
    assert birthrate['minimum'] <= 0 < 100 <= birthrate['maximum']
    source = bytes(c for y in range(HEIGHT) for x in range(WIDTH) for c in
        ((255,255,255,255) if 64<=x<192 and 36<=y<108 else (0,0,0,255)))
    src = tmp_path/'input.png'
    Image.frombytes('RGBA',(WIDTH,HEIGHT),source).save(src)
    outputs = []
    for frame, rate in ((0,100),(30,100),(30,0)):
        req = tmp_path/f'request-{frame}-{rate}.json'
        req.write_text(json.dumps({'schema_version':1,
            'timing':{'frame':frame,'fps':30,'duration_frames':300},
            'assignments':[{'slot':15,'value':rate}]}),encoding='utf-8')
        out = tmp_path/f'output-{frame}-{rate}.png'
        report = run('--render-experimental-smart-request',plugin,src,out,req)
        assert report['passed'] and report['output_pixels_valid']
        assert report['current_time']==frame and report['time_scale']==30
        assert next(p for p in report['requested_parameters'] if p['slot']==15)['value']==rate
        image = Image.open(out).convert('RGBA')
        assert image.size==(WIDTH,HEIGHT)
        outputs.append(image.tobytes())
    assert_emission_response(*outputs)
