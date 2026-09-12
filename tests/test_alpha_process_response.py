"""Blur-disabled alpha levels and two mix controls; not full effect/AE parity."""
import hashlib
import json
import os
import subprocess
import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT

CASES = (('neutral',0,255,1,100,0), ('levels',64,192,1,100,0),
         ('half',64,192,1,50,0), ('original-half',64,192,1,100,50),
         ('multiply-full',64,192,4,100,0), ('multiply-zero',64,192,4,0,0))


def source_pixels():
    return bytes(c for y in range(HEIGHT) for a in range(WIDTH) for c in (173,57,219,a))


def alpha_model(name, a):
    levels = 255*max(0,min(1,(a-64)/128))
    if name == 'neutral':
        return a
    if name == 'original-half':
        return (a+levels)/2
    if name == 'multiply-full':
        return a*levels/255
    return levels


def assert_alpha_process(outputs):
    assert set(outputs) == {c[0] for c in CASES}
    for name,pixels in outputs.items():
        assert len(pixels) == WIDTH*HEIGHT*4
        for y in range(HEIGHT):
            for a in range(WIDTH):
                i = (y*WIDTH+a)*4
                assert abs(pixels[i+3]-alpha_model(name,a)) <= 1
                if pixels[i+3]:
                    assert pixels[i:i+3] == bytes((173,57,219))
    assert outputs['levels'] == outputs['half'] == outputs['multiply-zero']
    assert outputs['neutral'][3::4] == source_pixels()[3::4]


@pytest.mark.parametrize('fault', ['copy','original_ignored','apply_ignored',
                                  'wrong_endpoint','alpha','rgb','truncated','normal_mix'])
def test_alpha_validator_rejects_corruption(fault):
    outputs = {c[0]:bytearray(v for y in range(HEIGHT) for a in range(WIDTH)
                              for v in (173,57,219,round(alpha_model(c[0],a)))) for c in CASES}
    assert_alpha_process(outputs)
    if fault == 'copy':
        outputs['levels'] = source_pixels()
    elif fault == 'original_ignored':
        outputs['original-half'] = outputs['levels']
    elif fault == 'apply_ignored':
        outputs['multiply-full'] = outputs['multiply-zero']
    elif fault == 'wrong_endpoint':
        outputs['multiply-zero'] = source_pixels()
    elif fault == 'alpha':
        outputs['levels'][100*4+3] += 10
    elif fault == 'rgb':
        outputs['levels'][100*4] -= 1
    elif fault == 'truncated':
        outputs['levels'] = outputs['levels'][:-4]
    else:
        outputs['half'] = outputs['original-half']
    with pytest.raises(AssertionError):
        assert_alpha_process(outputs)


def test_real_alpha_process(tmp_path):
    plugin = os.environ.get('AEXCOMPAT_TEST_ALPHA_PROCESS')
    if not plugin:
        pytest.skip('set AEXCOMPAT_TEST_ALPHA_PROCESS to the local AEX')
    assert os.name == 'nt'
    harness = ROOT/'broker/target/release/aexcompat-harness.exe'
    def run(*args):
        result = subprocess.run([str(harness),'--headless',*map(str,args)],cwd=ROOT,
                                capture_output=True,
                                timeout=None if args[0]=='--inspect-experimental' else 90)
        assert result.returncode == 0, result.stderr.decode('utf-8',errors='replace')
        return json.loads(result.stdout)
    params = {p['slot']:p for p in run('--inspect-experimental',plugin)}
    for slot,name in ((6,'Host Layer'),(11,'Black Level'),(12,'White Level'),
                      (15,'Apply Mix'),(19,'Mix with Original')):
        assert params[slot]['name'] == name
    assert params[14]['choices'][3] == 'Multiply'
    source = tmp_path/'source.png'
    Image.frombytes('RGBA',(WIDTH,HEIGHT),source_pixels()).save(source)
    outputs = {}
    for name,black,white,mode,mix,original in CASES:
        request,output = tmp_path/f'{name}.json',tmp_path/f'{name}.png'
        assignments = [{'slot':6,'layer':str(source)}]+[{'slot':s,'value':v} for s,v in
            ((8,0),(9,0),(10,0),(11,black),(12,white),(14,mode),(15,mix),(19,original))]
        request.write_text(json.dumps({'schema_version':1,'timing':{'frame':0,'fps':30,
            'duration_frames':300},'assignments':assignments}),encoding='utf-8')
        report = run('--render-experimental-smart-request',plugin,source,output,request)
        (tmp_path/f'{name}-report.json').write_text(json.dumps(report),encoding='utf-8')
        assert report['passed'] and report['output_pixels_valid']
        with Image.open(output) as image:
            assert image.size == (WIDTH,HEIGHT)
            pixels = image.convert('RGBA').tobytes()
        argb = bytearray(len(pixels))
        argb[0::4],argb[1::4] = pixels[3::4],pixels[0::4]
        argb[2::4],argb[3::4] = pixels[1::4],pixels[2::4]
        assert hashlib.sha256(argb).hexdigest() == report['output_sha256']
        outputs[name] = pixels
    assert_alpha_process(outputs)
