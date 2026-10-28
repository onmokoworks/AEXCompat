#!/usr/bin/env python3
"""Exercise the real guest stream: response before EOF, replay identity and bounds.

Needs an original AEX plus a canvas config. All artifacts go to --output.
No plugin-specific serialization normalization is done here.
"""
import argparse
import json
import os
from pathlib import Path
import selectors
import subprocess


def read_line(process, deadline=30):
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout, selectors.EVENT_READ)
        result = bytearray()
        while not result.endswith(b'\n'):
            if not selector.select(deadline):
                raise AssertionError('no complete response before EOF/deadline')
            chunk = os.read(process.stdout.fileno(), 1)
            if not chunk:
                raise AssertionError('worker closed stdout before response')
            result.extend(chunk)
            if len(result) > 32 * 1024 * 1024:
                raise AssertionError('response bound exceeded')
        return json.loads(result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--worker', type=Path, required=True)
    parser.add_argument('--plugin', type=Path, required=True)
    parser.add_argument('--config', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    config = json.loads(args.config.read_text())
    header = args.output / 'header.json'
    header.write_text(json.dumps(config['session']))
    command = [str(args.worker.resolve()), 'ui-session', str(args.plugin.resolve()), str(header.resolve()), '--stream']
    event = config['event']
    operation = {'type': 'event', 'event': event}
    rows = []
    with (args.output / 'stream.log').open('wb') as log:
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, bufsize=0)
        try:
            for op in [operation, {'type': 'event', 'event': {**event, 'event': 10, 'key_code': 0x80000041, 'modifiers': 0x300}}]:
                process.stdin.write(json.dumps(op).encode() + b'\n')
                rows.append(read_line(process))
                assert process.poll() is None, 'response must arrive with stdin still open'
            process.stdin.close()
            assert process.wait(timeout=30) == 0
        finally:
            if process.poll() is None:
                process.kill(); process.wait()
            process.stdout.close()
    (args.output / 'stream.jsonl').write_text(''.join(json.dumps(r) + '\n' for r in rows))
    # Cheap ignored-area EVENT still executes the original selector; use it for
    # exact 256/257 count boundaries without expensive custom-control drawing.
    ignored = {'type': 'event', 'event': {**event, 'area': 1}}
    cases = {
        'limit256': (b''.join(json.dumps(ignored).encode() + b'\n' for _ in range(256)), 0, 256, b'ui_session_close:'),
        'limit257': (b''.join(json.dumps(ignored).encode() + b'\n' for _ in range(257)), 1, 256, b'limited to 256'),
        'oversize': (b'x' * 1048577 + b'\n', 1, 0, b'exceeds 1 MiB'),
        'unterminated': (json.dumps(ignored).encode(), 1, 0, b'lacks newline'),
        'bad-key': (json.dumps({'type': 'event', 'event': {**event, 'event': 10}}).encode() + b'\n', 1, 0, b'key payload'),
        'bad-field': (json.dumps({**ignored, 'unexpected': 1}).encode() + b'\n', 1, 0, b'unknown field'),
    }
    summary = {'response_before_eof': True, 'operations': len(rows), 'cases': {}}
    for name, (payload, code, count, diagnostic) in cases.items():
        request = args.output / (name + '.jsonl')
        request.write_bytes(payload)
        with request.open('rb') as inp, (args.output / (name + '.out')).open('wb') as out, (args.output / (name + '.log')).open('wb') as log:
            run = subprocess.run(command, stdin=inp, stdout=out, stderr=log, timeout=180)
        with (args.output / (name + '.out')).open('rb') as out:
            actual_count = sum(1 for _ in out)
        assert run.returncode == code and actual_count == count, (name, run.returncode, actual_count)
        assert diagnostic in (args.output / (name + '.log')).read_bytes(), name
        summary['cases'][name] = {'returncode': run.returncode, 'operations': actual_count}
    (args.output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary))


if __name__ == '__main__':
    main()
