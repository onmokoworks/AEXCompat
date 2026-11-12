"""Qualify independent cases against the unchanged single-request CLI.

External AEX files remain outside the repository. All generated inputs, outputs,
and raw reports go into a new run directory. The summary records hashes and
wall-clock samples, not a native AE conformance claim. No AE/GUI is launched.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import struct
import subprocess
import time
import zlib


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def png(path, seed, width=97, height=61):
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    data = b''.join(b'\0' + bytes(
        value for x in range(width) for value in (
            (x * 13 + y * 3 + seed * 17) % 256,
            (x * 7 + y * 19 + seed * 41) % 256,
            (x * 23 + y * 11 + seed * 71) % 256, 255)) for y in range(height))
    path.write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0))
                     + chunk(b'IDAT', zlib.compress(data)) + chunk(b'IEND', b''))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--originals', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--repetitions', type=int, default=3)
    parser.add_argument('--cases-per-batch', type=int, choices=(2, 6), default=6,
                        help='2 selects an edited/default smoke pair; 6 retains the full sequence')
    parser.add_argument('--downsample', type=int, choices=(1, 2, 4), default=1)
    parser.add_argument('--only', help='optional single plug-in name for a supplemental context check')
    args = parser.parse_args()
    if not 1 <= args.repetitions <= 10:
        parser.error('repetitions must be in 1..10')
    repo = Path(__file__).resolve().parents[1]
    exe = repo / 'broker/target/release/aexcompat-harness.exe'
    worker = repo / 'target/minihost-build/aex_worker.exe'
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    env = dict(os.environ, AEXCOMPAT_REPOSITORY_ROOT=str(repo))
    expected = {'ColorKeep', 'DistanceGradation', 'OLMBlur', 'OLMColorKey', 'OLMDirectionalBlur',
                'OLMKiraKira', 'OLMRadialBlur', 'OLMSmoother', 'OLMSmoother2', 'OLMToonDilate'}
    plugins = {p.stem: p.resolve() for p in args.originals.glob('*.aex')}
    if set(plugins) != expected:
        raise ValueError('qualification requires exactly the ten named OLM originals')
    if args.only and args.only not in expected:
        raise ValueError('--only must name one of the ten OLM originals')
    summary = {'schema': 'aexcompat.independent_case_qualification', 'schema_version': 1,
               'host': platform.platform(), 'native_ae': False, 'dimensions': [97, 61],
               'repetitions': args.repetitions, 'cases_per_batch': args.cases_per_batch,
               'downsample': args.downsample, 'only': args.only,
               'harness_sha256': sha(exe), 'worker_sha256': sha(worker),
               'timing_scope': 'CLI wall time including discovery, input/output, launch and close; warm filesystem; ' +
                   ('batch first (single repetition)' if args.repetitions == 1 else 'alternating order'),
               'rows': []}

    def run(argv, log):
        started = time.perf_counter()
        process = subprocess.Popen([str(exe), '--headless', *map(str, argv)], cwd=repo,
                                   env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            stdout, stderr = process.communicate(timeout=120)
        except BaseException:
            process.kill()  # only this owned harness; its Job Object contains its workers
            process.wait()
            raise
        elapsed = time.perf_counter() - started
        log.with_suffix('.json').write_bytes(stdout)
        log.with_suffix('.stderr').write_bytes(stderr)
        if process.returncode:
            raise RuntimeError(f'{log}: exit={process.returncode}; inspect saved reports')
        return json.loads(stdout), elapsed

    for name, plugin in sorted(plugins.items()):
        if args.only and name != args.only:
            continue
        folder = out / name
        folder.mkdir()
        parameters, _ = run(['--inspect-experimental', plugin], folder / 'inspection')
        editable = next(p for p in parameters if p['kind'] in ('integer', 'float') and p['maximum'] > p['minimum'])
        edit = min(editable['maximum'], max(editable['minimum'], editable['value']) + 1)
        if editable['kind'] == 'integer':
            edit = int(edit)
        inputs = [folder / f'input{i}.png' for i in range(3)]
        for i, path in enumerate(inputs):
            png(path, i)
        requests = []
        for i in range(3):
            request = folder / f'request{i}.json'
            document = {'schema_version': 1,
                'timing': {'frame': i % 2, 'fps': 30, 'duration_frames': 300},
                'assignments': [{'slot': editable['slot'], 'value': edit}] if i == 2 else []}
            if args.downsample != 1:
                document['host_context'] = {'mask_scene': {'masks': []}, 'spatial': {
                    'downsample_x': {'numerator': 1, 'denominator': args.downsample},
                    'downsample_y': {'numerator': 1, 'denominator': args.downsample},
                    'pixel_aspect_ratio': {'numerator': 1, 'denominator': 1},
                    'full_resolution_width': 97 * args.downsample,
                    'full_resolution_height': 61 * args.downsample}}
            request.write_text(json.dumps(document), encoding='utf-8')
            requests.append(request)
        # A / B / A / edited C / B / A: repeated A observes both pixel and edit leakage.
        indices = [2, 0] if args.cases_per_batch == 2 else [0, 1, 0, 2, 1, 0]
        routes = ['classic8', 'classic16'] if name == 'OLMSmoother' else ['smart8', 'smart16', 'smart32']
        for route in routes:
            command = {'classic8': '--render-experimental-request',
                       'classic16': '--render-experimental-request-16',
                       'smart8': '--render-experimental-smart-request',
                       'smart16': '--render-experimental-smart-request-16',
                       'smart32': '--render-experimental-smart-request-32-cpu'}[route]
            samples = []
            for repeat in range(args.repetitions):
                run_dir = folder / f'{route}-{repeat}'
                run_dir.mkdir()
                order = indices if repeat % 2 == 0 else list(reversed(indices))
                cases = [{'id': f'{i}-{source}', 'input': str(inputs[source]), 'request': str(requests[source]),
                          'output': str(run_dir / f'batch{i}.png')} for i, source in enumerate(order)]
                manifest = run_dir / 'manifest.json'
                manifest.write_text(json.dumps({'schema': 'aexcompat.independent_case_batch', 'schema_version': 1,
                    'plugin': str(plugin), 'route': route, 'isolation': 'fresh_sequence',
                    'frame_deadline_ms': 120000, 'cases': cases}), encoding='utf-8')

                def singles():
                    seconds = 0
                    for i, case in enumerate(cases):
                        report, elapsed = run([command, plugin, case['input'], run_dir / f'single{i}.png',
                                               case['request']], run_dir / f'single{i}-report')
                        assert report['passed'], report
                        seconds += elapsed
                    return seconds

                if repeat % 2:
                    single_seconds = singles()
                batch, batch_seconds = run(['--render-request-batch', manifest], run_dir / 'batch-report')
                if repeat % 2 == 0:
                    single_seconds = singles()
                assert batch['passed'] and batch['complete'], run_dir
                pids = {case['worker_pid'] for case in batch['cases']}
                assert len(pids) == 1 and None not in pids, pids
                # A finished CLI must have collected the resident worker. A PID may
                # no longer be openable after exit; either result verifies cleanup.
                kernel = ctypes.WinDLL('kernel32', use_last_error=True)
                kernel.OpenProcess.restype = ctypes.c_void_p
                kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
                kernel.CloseHandle.argtypes = [ctypes.c_void_p]
                handle = kernel.OpenProcess(0x100000, False, next(iter(pids)))
                if handle:
                    try:
                        assert kernel.WaitForSingleObject(handle, 0) == 0, 'resident process still active'
                    finally:
                        kernel.CloseHandle(handle)
                records = []
                for i, case in enumerate(cases):
                    extensions = ['.png'] + (['.rgba16le'] if route.endswith('16') else
                                             ['.rgba32f-le'] if route.endswith('32') else [])
                    hashes = {}
                    for extension in extensions:
                        resident = Path(case['output']).with_suffix(extension)
                        single = (run_dir / f'single{i}.png').with_suffix(extension)
                        assert resident.read_bytes() == single.read_bytes(), (name, route, i, extension)
                        hashes[extension] = sha(resident)
                    records.append({'source': order[i], 'artifacts_sha256': hashes})
                samples.append({'batch_s': batch_seconds, 'single_total_s': single_seconds,
                                'worker_pid': next(iter(pids)), 'worker_exited': True, 'cases': records})
            batch_median = statistics.median(s['batch_s'] for s in samples)
            single_median = statistics.median(s['single_total_s'] for s in samples)
            row = {'plugin': name, 'plugin_sha256': sha(plugin), 'route': route,
                   'edited_slot': editable['slot'], 'edited_value': edit,
                   'batch_median_s': batch_median, 'single_median_s': single_median,
                   'speedup': single_median / batch_median, 'byte_identical': True, 'samples': samples}
            summary['rows'].append(row)
            (out / 'measurement.json').write_text(json.dumps(summary, indent=2), encoding='utf-8')
            comparisons = len(indices) * args.repetitions
            print(f'{name} {route}: exact {comparisons}/{comparisons}, {row["speedup"]:.2f}x', flush=True)


if __name__ == '__main__':
    main()
