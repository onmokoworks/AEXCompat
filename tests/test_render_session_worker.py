"""Behavioral self-tests for the worker render session frame loop.

Drives ``aex_render_worker.exe --render-session-v1`` directly over the
protocol transport (docs/RENDER_SESSION_PROTOCOL_2026-07-19.md): two
inherited anonymous pipes carrying length-prefixed JSON and one inherited
anonymous file mapping with copy-through pixel slots. Expectations are
self-computed (checksums over transferred bytes, exit codes from the
protocol), so a fresh build on any machine satisfies them.
"""
import ctypes
import ctypes.wintypes as wintypes
import hashlib
import json
import os
import struct
import subprocess
import threading
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "target" / "minihost-build" / "aex_render_worker.exe"
AEX = ROOT / "target" / "pf-sampling-probe-build" / "Release" / "pf_sampling_probe.aex"

HEADER_BYTES = 4096
SLOT_ALIGNMENT = 4096
HEADER_MAGIC = 0x53584541  # "AEXS"
PROTOCOL_VERSION = 1

MAGIC_OFFSET = 0
VERSION_OFFSET = 4
DEPTH_CODE_OFFSET = 8
MAX_WIDTH_OFFSET = 12
MAX_HEIGHT_OFFSET = 16
LAYER_SLOT_COUNT_OFFSET = 20
INPUT_GENERATION_OFFSET = 24
OUTPUT_GENERATION_OFFSET = 28

EXIT_PROTOCOL_VIOLATION = 23
EXIT_INVARIANT_FAILURE = 24

WIDTH = 32
HEIGHT = 16
TIME_SCALE = 30

pytestmark = pytest.mark.skipif(os.name != "nt", reason="session transport is Windows-only")


def _align(value):
    return (value + SLOT_ALIGNMENT - 1) // SLOT_ALIGNMENT * SLOT_ALIGNMENT


class SessionTransport:
    """Broker-side half of the session transport, built with ctypes."""

    def __init__(self):
        kernel32 = ctypes.windll.kernel32
        self.kernel32 = kernel32
        self.input_offset = HEADER_BYTES
        self.output_offset = HEADER_BYTES + _align(WIDTH * HEIGHT * 4)
        self.section_bytes = self.output_offset + _align(WIDTH * HEIGHT * 4)

        class SECURITY_ATTRIBUTES(ctypes.Structure):
            _fields_ = [
                ("nLength", wintypes.DWORD),
                ("lpSecurityDescriptor", wintypes.LPVOID),
                ("bInheritHandle", wintypes.BOOL),
            ]

        inheritable = SECURITY_ATTRIBUTES(ctypes.sizeof(SECURITY_ATTRIBUTES), None, True)

        request_read = wintypes.HANDLE()
        request_write = wintypes.HANDLE()
        assert kernel32.CreatePipe(
            ctypes.byref(request_read), ctypes.byref(request_write),
            ctypes.byref(inheritable), 0)
        response_read = wintypes.HANDLE()
        response_write = wintypes.HANDLE()
        assert kernel32.CreatePipe(
            ctypes.byref(response_read), ctypes.byref(response_write),
            ctypes.byref(inheritable), 0)
        # Only the child ends stay inheritable, mirroring windows_process.rs.
        HANDLE_FLAG_INHERIT = 1
        assert kernel32.SetHandleInformation(request_write, HANDLE_FLAG_INHERIT, 0)
        assert kernel32.SetHandleInformation(response_read, HANDLE_FLAG_INHERIT, 0)
        self.request_read = request_read.value
        self.request_write = request_write.value
        self.response_read = response_read.value
        self.response_write = response_write.value

        PAGE_READWRITE = 0x04
        kernel32.CreateFileMappingW.restype = wintypes.HANDLE
        section = kernel32.CreateFileMappingW(
            wintypes.HANDLE(-1), ctypes.byref(inheritable), PAGE_READWRITE,
            0, self.section_bytes, None)
        assert section, ctypes.get_last_error()
        self.section = section
        FILE_MAP_ALL_ACCESS = 0x000F001F
        kernel32.MapViewOfFile.restype = wintypes.LPVOID
        view = kernel32.MapViewOfFile(
            wintypes.HANDLE(section), FILE_MAP_ALL_ACCESS, 0, 0, 0)
        assert view
        self.view = (ctypes.c_ubyte * self.section_bytes).from_address(view)

        self.write_header(MAGIC_OFFSET, HEADER_MAGIC)
        self.write_header(VERSION_OFFSET, PROTOCOL_VERSION)
        self.write_header(DEPTH_CODE_OFFSET, 8)
        self.write_header(MAX_WIDTH_OFFSET, WIDTH)
        self.write_header(MAX_HEIGHT_OFFSET, HEIGHT)
        self.write_header(LAYER_SLOT_COUNT_OFFSET, 0)

    def write_header(self, offset, value):
        struct.pack_into("<I", self.view, offset, value)

    def read_header(self, offset):
        return struct.unpack_from("<I", self.view, offset)[0]

    def write_input(self, pattern_byte, generation):
        pixels = bytes(
            (pattern_byte + index) % 256 for index in range(WIDTH * HEIGHT * 4))
        self.view[self.input_offset:self.input_offset + len(pixels)] = pixels
        self.write_header(INPUT_GENERATION_OFFSET, generation)

    def output_bytes(self, count):
        return bytes(self.view[self.output_offset:self.output_offset + count])

    def environment(self):
        return {
            **os.environ,
            "AEXCOMPAT_RENDER_SESSION_REQUEST_HANDLE": str(self.request_read),
            "AEXCOMPAT_RENDER_SESSION_RESPONSE_HANDLE": str(self.response_write),
            "AEXCOMPAT_RENDER_SESSION_SECTION_HANDLE": str(self.section),
        }

    def send(self, payload):
        data = json.dumps(payload).encode() if isinstance(payload, dict) else payload
        self.send_raw(struct.pack("<I", len(data)) + data)

    def send_raw(self, frame):
        written = wintypes.DWORD()
        assert self.kernel32.WriteFile(
            wintypes.HANDLE(self.request_write), frame, len(frame),
            ctypes.byref(written), None)
        assert written.value == len(frame)

    def _read_exact(self, count):
        collected = b""
        while len(collected) < count:
            buffer = ctypes.create_string_buffer(count - len(collected))
            read = wintypes.DWORD()
            ok = self.kernel32.ReadFile(
                wintypes.HANDLE(self.response_read), buffer, len(buffer),
                ctypes.byref(read), None)
            if not ok or read.value == 0:
                return None
            collected += buffer.raw[:read.value]
        return collected

    def receive(self, timeout=30):
        result = {}

        def reader():
            prefix = self._read_exact(4)
            if prefix is None:
                result["message"] = None
                return
            (length,) = struct.unpack("<I", prefix)
            body = self._read_exact(length)
            result["message"] = None if body is None else json.loads(body)

        thread = threading.Thread(target=reader, daemon=True)
        thread.start()
        thread.join(timeout)
        assert not thread.is_alive(), "timed out waiting for a worker response"
        return result["message"]

    def close_child_ends(self):
        # After spawn the parent must drop the child-side ends so a worker
        # exit turns into pipe EOF instead of a hang.
        self.kernel32.CloseHandle(wintypes.HANDLE(self.request_read))
        self.kernel32.CloseHandle(wintypes.HANDLE(self.response_write))
        self.request_read = None
        self.response_write = None


def _require_artifacts():
    if not WORKER.is_file():
        pytest.skip("aex_render_worker.exe is not built; run the minihost build")
    if not AEX.is_file():
        pytest.skip("pf_sampling_probe.aex is not built")


def _spawn(transport):
    aex_sha = hashlib.sha256(AEX.read_bytes()).hexdigest()
    process = subprocess.Popen(
        [str(WORKER), "--render-session-v1", str(AEX), aex_sha, "v5|",
         str(WIDTH), str(HEIGHT), "1", "300", str(TIME_SCALE)],
        cwd=ROOT, env=transport.environment(), close_fds=False,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    transport.close_child_ends()
    return process


def _finish(process, timeout=60):
    stdout, stderr = process.communicate(timeout=timeout)
    return process.returncode, stdout, stderr


def render_frame_message(frame_index, time_value, scale=TIME_SCALE):
    return {"v": 1, "type": "render_frame", "frame_index": frame_index,
            "current_time": {"value": time_value, "scale": scale}}


def test_session_renders_frames_with_persistent_sequence_and_slot_checksums():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        checksums = []
        for frame_index, pattern in ((0, 11), (1, 173)):
            transport.write_input(pattern, frame_index + 1)
            transport.send(render_frame_message(frame_index, frame_index))
            done = transport.receive()
            assert done is not None, "worker closed the response pipe early"
            assert done["type"] == "frame_done"
            assert done["frame_index"] == frame_index
            assert done["status"] == "ok", done
            assert done["render_error"] == 0
            assert done["generation"] == frame_index + 1
            output = done["output"]
            assert output["width"] == WIDTH
            assert output["height"] == HEIGHT
            assert output["pixel_format"] == "argb8"
            assert output["guards_intact"] is True
            assert transport.read_header(OUTPUT_GENERATION_OFFSET) == frame_index + 1
            slot = transport.output_bytes(WIDTH * HEIGHT * 4)
            assert hashlib.sha256(slot).hexdigest() == output["checksum"]
            checksums.append(output["checksum"])
        # Different input patterns must produce different transferred outputs:
        # the slot transport is live, not an artifact of one frame.
        assert checksums[0] != checksums[1]
        transport.send({"v": 1, "type": "close"})
        code, stdout, stderr = _finish(process)
        assert code == 0, (code, stderr[-500:])
        report = json.loads(stdout.strip())
        assert isinstance(report, dict)
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_session_survives_a_frame_local_time_scale_mismatch():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        transport.write_input(31, 1)
        transport.send(render_frame_message(0, 0, scale=TIME_SCALE + 1))
        done = transport.receive()
        assert done["status"] == "error"
        assert done["render_error"] == -40
        assert "output" not in done and "generation" not in done
        # The same frame index succeeds afterwards: the error was frame-local.
        transport.send(render_frame_message(0, 0))
        done = transport.receive()
        assert done["status"] == "ok", done
        transport.send({"v": 1, "type": "close"})
        code, _, stderr = _finish(process)
        assert code == 0, (code, stderr[-500:])
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_session_rejects_frames_outside_the_declared_timeline():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        transport.write_input(63, 1)
        transport.send(render_frame_message(0, -1))
        done = transport.receive()
        assert done["status"] == "error"
        assert done["render_error"] == -46
        transport.send(render_frame_message(0, 0))
        assert transport.receive()["status"] == "ok"
        transport.send({"v": 1, "type": "close"})
        code, _, stderr = _finish(process)
        assert code == 0, (code, stderr[-500:])
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_session_close_after_frame_local_error_exits_cleanly():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        transport.write_input(21, 1)
        transport.send(render_frame_message(0, 0, scale=TIME_SCALE + 1))
        assert transport.receive()["status"] == "error"
        # The error was already reported via frame_done and stopping is the
        # broker's decision, so a clean close is a successful session.
        transport.send({"v": 1, "type": "close"})
        code, _, stderr = _finish(process)
        assert code == 0, (code, stderr[-500:])
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_session_fails_closed_on_stale_input_generation():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        transport.write_input(57, 1)
        transport.send(render_frame_message(0, 0))
        assert transport.receive()["status"] == "ok"
        # Request frame 1 without advancing input_generation: stale slot.
        transport.send(render_frame_message(1, 1))
        done = transport.receive()
        assert done["status"] == "error"
        assert done["render_error"] == -41
        code, _, _ = _finish(process)
        assert code == EXIT_INVARIANT_FAILURE
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_session_rejects_unknown_message_fields_fail_closed():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        transport.write_input(99, 1)
        message = render_frame_message(0, 0)
        message["parameters"] = {"slot": 1}
        transport.send(message)
        # Protocol violation: no response is defined; the worker terminates.
        assert transport.receive() is None
        code, _, _ = _finish(process)
        assert code == EXIT_PROTOCOL_VIOLATION
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_session_treats_malformed_framing_as_protocol_violation():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        transport.write_input(45, 1)
        transport.send(render_frame_message(0, 0))
        assert transport.receive()["status"] == "ok"
        # A zero-length prefix is invalid framing, not a close signal: the
        # worker must exit through the protocol-violation path, not exit 0.
        transport.send_raw(struct.pack("<I", 0))
        assert transport.receive() is None
        code, _, _ = _finish(process)
        assert code == EXIT_PROTOCOL_VIOLATION
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_session_launch_without_channels_fails_closed():
    _require_artifacts()
    aex_sha = hashlib.sha256(AEX.read_bytes()).hexdigest()
    result = subprocess.run(
        [str(WORKER), "--render-session-v1", str(AEX), aex_sha, "v5|",
         str(WIDTH), str(HEIGHT), "1", "300", str(TIME_SCALE)],
        cwd=ROOT, capture_output=True, text=True, timeout=60)
    assert result.returncode == EXIT_PROTOCOL_VIOLATION


def test_session_launch_rejects_time_scale_above_int32():
    _require_artifacts()
    aex_sha = hashlib.sha256(AEX.read_bytes()).hexdigest()
    # The per-frame protocol carries scales as signed 32-bit, so a launch
    # scale above INT32_MAX could never be matched by any frame.
    result = subprocess.run(
        [str(WORKER), "--render-session-v1", str(AEX), aex_sha, "v5|",
         str(WIDTH), str(HEIGHT), "1", "300", str(2**31)],
        cwd=ROOT, capture_output=True, text=True, timeout=60)
    assert result.returncode == 3
