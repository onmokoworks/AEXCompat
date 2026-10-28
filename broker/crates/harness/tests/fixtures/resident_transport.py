#!/usr/bin/python3
"""Protocol double for the Mac GUI adapter; does not emulate an AEX.

Changes dimensions only when the GUI actually passes --negotiate-output 1.
Emits deep boundary samples and signed origin for end-to-end export tests.
"""
import hashlib
import json
import os
from pathlib import Path
import struct
import sys

_, route, plugin, source, target, width, height, scale, *options = sys.argv
assert route == 'session'
fmt = options[options.index('--pixel-format') + 1]
negotiated = '--negotiate-output' in options
if negotiated:
    assert options[options.index('--negotiate-output') + 1] == '1'
width, height = (3, 2) if negotiated else (int(width), int(height))
origin = (-1, 2) if negotiated else (0, 0)
pixel = {'argb8': bytes([255, 11, 22, 33]),
         'argb16': struct.pack('<4H', 32768, 1, 16384, 65535),
         'argb32f': struct.pack('<4I', 0x3f800000, 0x80000000, 0x3f000000, 0x40000000)}[fmt]

def send(message):
    data = json.dumps({'v': 1, **message}).encode()
    sys.stdout.buffer.write(struct.pack('<I', len(data)) + data)
    sys.stdout.buffer.flush()

send({'type': 'session_ready', 'worker_pid': os.getpid(), 'setup': {}})
while True:
    prefix = sys.stdin.buffer.read(4)
    if not prefix:
        break
    request = json.loads(sys.stdin.buffer.read(struct.unpack('<I', prefix)[0]))
    if request['type'] == 'probe':
        send({'type': 'session_probed', 'worker_pid': os.getpid(),
              'status': 'ok', 'guards_intact': True, 'render_error': 0})
    elif request['type'] == 'render_frame':
        data = pixel * (width * height)
        Path(target).write_bytes(data)
        frame = request['frame_index']
        send({'type': 'frame_done', 'frame_index': frame, 'status': 'ok',
              'render_error': 0, 'generation': frame + 1,
              'output': {'rect': [*origin, origin[0] + width, origin[1] + height],
                         'width': width, 'height': height, 'rowbytes': width * len(pixel),
                         'pixel_format': fmt, 'render_path': 'classic',
                         'checksum': hashlib.sha256(data).hexdigest(), 'guards_intact': True}})
    else:
        assert request['type'] == 'close'
        send({'type': 'session_closed', 'worker_pid': os.getpid(), 'setup': {},
              'close': {'session_clean': True, 'frame_setdown_error': 0,
                        'sequence_setdown_error': 0, 'global_setdown_error': 0,
                        'handles_remaining': 0, 'worlds_remaining': 0,
                        'unsupported_suite_calls': [], 'dropped_unsupported_suite_calls': 0,
                        'cleanup_error': 0, 'reserved': 0}})
        break
