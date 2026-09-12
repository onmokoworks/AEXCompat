"""Blur identity and edge broadening; no exact-kernel or AE parity claim."""
import json
import os
import subprocess
import pytest
from PIL import Image
from test_render_fixture_semantic_response import ROOT, WIDTH, HEIGHT


def source_pixels():
    return bytes(c for y in range(HEIGHT) for x in range(WIDTH) for c in
        ((255,255,255,255) if 64<=x<192 and 36<=y<108 else (0,0,0,255)))


def edge_width(raw):
    assert len(raw)==WIDTH*HEIGHT*4
    assert raw[3::4]==bytes([255])*WIDTH*HEIGHT
    assert raw[0::4]==raw[1::4]==raw[2::4]
    row=raw[72*WIDTH*4:73*WIDTH*4:4]
    assert row[40]==0 and row[128]==255 and row[216]==0
    assert all(a<=b for a,b in zip(row[40:88],row[41:89]))
    assert all(a>=b for a,b in zip(row[168:216],row[169:217]))
    gray=[x for x,v in enumerate(row) if 0<v<255]
    assert gray and all(56<=x<=72 or 184<=x<=200 for x in gray)
    assert any(x<64 for x in gray) and any(x>=64 and x<72 for x in gray)
    assert any(x<192 and x>=184 for x in gray) and any(x>=192 for x in gray)
    return len(gray)


def assert_blur_response(zero,small,large):
    assert zero==source_pixels()
    assert edge_width(large)>edge_width(small)>0


def synthetic(width):
    raw=bytearray(source_pixels())
    for y in range(HEIGHT):
        for x in range(64-width,64+width):
            v=round(255*(x-(64-width)+1)/(2*width+1))
            for pos,val in ((x,v),(255-x,v)):
                i=(y*WIDTH+pos)*4
                raw[i:i+3]=bytes([val])*3
    return raw


@pytest.mark.parametrize('fault',['copy','reversed','alpha','color','nonmonotonic','shift','truncated'])
def test_blur_validator_rejects_corruption(fault):
    zero,small,large=source_pixels(),synthetic(1),synthetic(4)
    assert_blur_response(zero,small,large)
    if fault=='copy': large=small
    elif fault=='reversed': small,large=large,small
    elif fault=='alpha': large[3]=0
    elif fault=='color': large[0]=1
    elif fault=='nonmonotonic':
        i=(72*WIDTH+63)*4
        large[i:i+3]=bytes([255])*3
    elif fault=='shift': large=large[80:]+large[:80]
    else: large=large[:-4]
    with pytest.raises(AssertionError): assert_blur_response(zero,small,large)


def test_installed_bcc_blur_radius_response(tmp_path):
    plugin=os.environ.get('AEXCOMPAT_TEST_BCC_BLUR')
    if not plugin: pytest.skip('requires AEXCOMPAT_TEST_BCC_BLUR')
    assert os.name=='nt'
    harness=ROOT/'broker/target/release/aexcompat-harness.exe'
    def run(*args):
        r=subprocess.run([str(harness),'--headless',*map(str,args)],cwd=ROOT,
            capture_output=True,timeout=None if args[0]=='--inspect-experimental' else 90)
        assert r.returncode==0,r.stderr.decode('utf-8',errors='replace')
        return json.loads(r.stdout)
    params=run('--inspect-experimental',plugin)
    for slot,name in ((4,'Horizontal Blur'),(5,'Vertical Blur')):
        p=next(p for p in params if p['slot']==slot)
        assert p['name']==name and p['minimum']<=0<20<=p['maximum']
    src=tmp_path/'source.png'
    Image.frombytes('RGBA',(WIDTH,HEIGHT),source_pixels()).save(src)
    outputs=[]
    for radius in (0,2,20):
        req=tmp_path/f'{radius}.json';out=tmp_path/f'{radius}.png'
        req.write_text(json.dumps({'schema_version':1,'timing':{'frame':0,'fps':30,'duration_frames':300},
            'assignments':[{'slot':4,'value':radius},{'slot':5,'value':radius}]}),encoding='utf-8')
        report=run('--render-experimental-smart-request',plugin,src,out,req)
        assert report['passed'] and report['output_pixels_valid']
        image=Image.open(out).convert('RGBA')
        assert image.size==(WIDTH,HEIGHT)
        outputs.append(image.tobytes())
    assert_blur_response(*outputs)
