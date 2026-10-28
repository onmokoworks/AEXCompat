#!/usr/bin/env python3
"""Typed F54 geometry cases: actual guest execution, explicit native static mapping.

No Windows execution is implied by this runner. Output contains no pixel bytes or
absolute paths. Use --output beneath the caller's ticket scratch directory.
"""
import argparse
from dataclasses import asdict, dataclass, replace
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess


@dataclass(frozen=True)
class Case:
    name: str
    full: tuple[int, int] = (17, 11)
    depth: int = 8
    downsample_x: tuple[int, int] = (1, 1)
    downsample_y: tuple[int, int] = (1, 1)
    pixel_aspect: tuple[int, int] = (1, 1)
    negotiate: bool = False
    checkout: tuple[int, int, int, int] | str | None = None
    request: tuple[int, int, int, int] | None = None
    extent: tuple[int, int, int, int] | None = None
    padding: int = 0
    expected: str = 'ok'
    native: str = 'identity'

    def world(self):
        if isinstance(self.checkout, tuple):
            l, t, r, b = self.checkout
            return r-l, b-t
        return tuple((n*a+b-1)//b for n, (a, b) in
                     zip(self.full, (self.downsample_x, self.downsample_y)))

    def args(self):
        result = ['--pixel-format', {8:'argb8', 16:'argb16', 32:'argb32f'}[self.depth],
                  '--fixture-render-path', 'smart', '--negotiate-output', str(int(self.negotiate)),
                  '--rowbytes-pad', str(self.padding)]
        for flag, value in [('downsample-x', self.downsample_x), ('downsample-y', self.downsample_y),
                            ('pixel-aspect', self.pixel_aspect), ('checkout-rect', self.checkout),
                            ('request-rect', self.request), ('extent-hint', self.extent)]:
            if value is not None:
                result += ['--'+flag, value if isinstance(value, str) else ','.join(map(str, value))]
        return result


# Reviewed structural mapping, NOT source-string assertions or runtime evidence.
NATIVE = {
    'identity': ('comparable', 'WorldLayout; prepare_image_request; prepare_world_layout'),
    'ratios': ('comparable_with_adapter', 'RenderContextState::downsample_x/y, pixel_aspect_ratio, full_resolution_width/height; caller must supply already sampled input'),
    'negotiate': ('comparable', 'SmartOutputBounds; prepare_smart_output_bounds uses effect result_rect and negates output origin for InData'),
    'request': ('comparable', 'RenderContextState::smart_output_request / DiagnosticWorldLayout::request_rect -> worker_smart_setup'),
    'extent': ('comparable', 'DiagnosticWorldLayout::extent_hint -> worker_smart_setup; full source extent remains separate'),
    'padding': ('comparable', 'DiagnosticWorldLayout::input_row_padding/output_row_padding (0..256); output serialization packs rows'),
    'crop': ('gap', 'worker_smart_runtime::checkout_pixels returns view_world with original allocation/dimensions; request intersection changes extent only. No F54 physically cropped checkout/rebased data contract'),
    'negative_request': ('partial', 'smart_output_request supports signed layer coordinates; DiagnosticWorldLayout parser rejects negative request coordinates'),
    '4k': ('comparable', 'WorldLayout edge<=4096; SmartOutputBounds area<=16777216; broker native input 4K depth admission tests'),
    'bad_pad': ('comparable_rejection', 'parse_diagnostic_world_layout rejects padding>256'),
    'bad_checkout': ('gap', 'No native forced-checkout option to which this guest admission rejection maps'),
}


def cases():
    small = [Case('identity'), Case('downsample-ceil', full=(41,31), downsample_x=(1,2), downsample_y=(1,3), native='ratios'),
             Case('pixel-aspect', pixel_aspect=(10,11), native='ratios'),
             Case('negotiate', negotiate=True, native='negotiate'),
             Case('request-inset', negotiate=True, request=(3,2,14,9), native='request'),
             Case('request-negative-origin', negotiate=True, request=(-3,-2,14,9), native='negative_request'),
             Case('extent-inset', extent=(1,2,16,9), native='extent'),
             Case('checkout-explicit', negotiate=True, checkout=(3,2,14,9), request=(3,2,14,9), native='crop'),
             Case('checkout-auto', negotiate=True, checkout='auto', request=(3,2,14,9), native='crop'),
             Case('padding-odd', padding=3, native='padding'),
             Case('padding-max', padding=256, native='padding')]
    for depth in (8,16,32):
        for c in small:
            yield replace(c, name=f'{c.name}-{depth}', depth=depth)
        yield Case(f'4k-{depth}', full=(3840,2160), depth=depth, native='4k')
    yield Case('padding-over-limit', padding=257, expected='reject', native='bad_pad')
    yield Case('checkout-without-negotiation', checkout=(3,2,14,9), expected='reject', native='bad_checkout')
    yield Case('checkout-outside', negotiate=True, checkout=(0,0,18,11), expected='reject', native='bad_checkout')


def digest(path):
    with path.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def framed_messages(path):
    with path.open('rb') as f:
        while header := f.read(4):
            if len(header) != 4:
                raise ValueError('truncated frame header')
            n, = struct.unpack('<I', header)
            if n > 16*1024*1024:
                raise ValueError('oversized response')
            payload = f.read(n)
            if len(payload) != n:
                raise ValueError('truncated frame')
            yield json.loads(payload)


def run(worker, aex, c, directory):
    directory.mkdir()
    ip, op = directory/'input.argb', directory/'output.argb'
    w, h = c.world()
    pixel = {8:bytes([255,71,29,103]), 16:struct.pack('<4H',32768,9000,4000,12000),
             32:struct.pack('<4f',1.,.25,.125,.375)}[c.depth]
    with ip.open('wb') as f:
        for _ in range(h):
            f.write(pixel*w)
    request = json.dumps({'v':1, 'type':'render_frame', 'frame_index':0,
                          'current_time':{'value':0,'scale':24}}).encode()
    command = [str(worker), 'session', str(aex), str(ip), str(op), *map(str,c.full), '24', *c.args()]
    env = {k:v for k,v in os.environ.items() if not k.startswith(('AEXCOMPAT_RESIDENT_', 'AEXCOMPAT_SMART_TRACE', 'OLM_FLOAT_COLOR'))}
    with (directory/'stdout.bin').open('wb') as out, (directory/'stderr.log').open('wb') as err:
        try:
            p = subprocess.run(command, input=struct.pack('<I',len(request))+request,
                               stdout=out, stderr=err, env=env, timeout=240)
            code = p.returncode
        except subprocess.TimeoutExpired:
            code = 'timeout'
    messages = list(framed_messages(directory/'stdout.bin'))
    frame = next((m for m in messages if m.get('type')=='frame_done'), None)
    setup = next((m['setup'] for m in messages if m.get('type')=='session_ready'), None)
    close = next((m['close'] for m in messages if m.get('type')=='session_closed'), None)
    actual = 'ok' if code == 0 and frame and frame['status']=='ok' else 'reject'
    result = {'exit_code':code, 'status':actual, 'frame':frame,
              'setup_errors': {k:setup[k] for k in ('global_setup_error','params_setup_error')} if setup else None,
              'session_clean':close.get('session_clean') if close else None,
              'stderr':(directory/'stderr.log').read_text(errors='replace')[-1500:]}
    if actual == 'ok':
        output = frame['output']
        result['output_sha256'] = digest(op)
        result['output_bytes'] = op.stat().st_size
        result['invariants_ok'] = (output['guards_intact'] and result['session_clean'] and
            result['output_sha256']==output['checksum'] and
            result['output_bytes']==output['width']*output['height']*len(pixel) and
            not any(result['setup_errors'].values()))
    result['expectation_met'] = actual == c.expected and (actual != 'ok' or result['invariants_ok']) and isinstance(code,int) and code >= 0
    ip.unlink()
    op.unlink(missing_ok=True)
    result['world_captures'] = []
    for dump in sorted(directory.glob('fixture-*-world.bin')):
        with dump.open('rb') as f:
            header = f.read(56)
        magic, *words = struct.unpack('<8s10iQ', header)
        if magic != b'AEXWRAW1' or dump.stat().st_size != 56+words[-1]:
            raise ValueError('invalid world capture')
        result['world_captures'].append({'stage':dump.stem,
            'width':words[0], 'height':words[1], 'pixel_bytes':words[2],
            'rowbytes':words[3], 'origin':words[4:6], 'extent':words[6:10],
            'bytes':words[10], 'sha256':digest(dump)})
        dump.unlink()
    return result


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--worker', type=Path, action='append', required=True)
    ap.add_argument('--aex', type=Path, required=True)
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--case', action='append')
    args = ap.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    rows = []
    for worker in args.worker:
        identity = {'name':worker.parent.name,'sha256':digest(worker)}
        for c in cases():
            if args.case and c.name not in args.case:
                continue
            result = run(worker.resolve(), args.aex.resolve(), c, args.output/f'{identity["name"]}-{c.name}')
            row = {'case':asdict(c),'guest_worker':identity,'guest':result,
                   'native':{'assessment':NATIVE[c.native][0], 'source_mapping':NATIVE[c.native][1],
                             'windows_execution_observed':False, 'evidence_kind':'static_review_only'}}
            rows.append(row)
            print(identity['name'],c.name,result['status'],result['expectation_met'],flush=True)
            (args.output/'matrix.json').write_text(json.dumps({'schema':'aexcompat.geometry-matrix/1',
                'aex_sha256':digest(args.aex),'rows':rows},indent=2)+'\n')
    failures = sum(not r['guest']['expectation_met'] for r in rows)
    print(f'{len(rows)} rows; {failures} expectation failures')
    return int(failures != 0)


if __name__ == '__main__':
    raise SystemExit(main())
