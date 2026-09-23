"""Behavioral self-tests for the worker render session frame loop.

Drives ``aex_worker.exe --kind classic --render-session-v1`` directly over the
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
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"
AEX = ROOT / "target" / "pf-sampling-probe-build" / "Release" / "pf_sampling_probe.aex"
PARAMETER_ECHO_AEX = (
    ROOT / "target" / "pf-parameter-echo-probe-build" / "Release"
    / "pf_parameter_echo_probe.aex")

HEADER_BYTES = 4096
SLOT_ALIGNMENT = 4096
HEADER_MAGIC = 0x53584541  # "AEXS"
PROTOCOL_VERSION = 1
# Session header layout version (distinct from the message `v`): 3 since layer
# pixels left the section for inherited per-layer file handles (#268), so the
# section is header + input + output only. The real worker's
# static_header_matches requires it, so this stand-in broker (which carries no
# layers) must stamp 3 or the worker fail-closes.
SESSION_HEADER_VERSION = 3

MAGIC_OFFSET = 0
VERSION_OFFSET = 4
DEPTH_CODE_OFFSET = 8
MAX_WIDTH_OFFSET = 12
MAX_HEIGHT_OFFSET = 16
LAYER_SLOT_COUNT_OFFSET = 20
INPUT_GENERATION_OFFSET = 24
OUTPUT_GENERATION_OFFSET = 28
FRAME_WIDTH_OFFSET = 32
FRAME_HEIGHT_OFFSET = 36

EXIT_PROTOCOL_VIOLATION = 23
EXIT_INVARIANT_FAILURE = 24

WIDTH = 32
HEIGHT = 16
TIME_SCALE = 30

pytestmark = pytest.mark.skipif(os.name != "nt", reason="session transport is Windows-only")


def _align(value):
    return (value + SLOT_ALIGNMENT - 1) // SLOT_ALIGNMENT * SLOT_ALIGNMENT


class SessionTransport:
    """Broker-side half of the session transport, built with ctypes.

    ``depth_code``/``output_pixel_bytes`` parameterize the output slot for the
    deeper session commands (16 -> 8 bytes/px, 32 -> 16 bytes/px); the input
    slot is RGBA8 at every depth, like the one-shot raw transport.
    """

    def __init__(self, depth_code=8, output_pixel_bytes=4):
        kernel32 = ctypes.windll.kernel32
        self.kernel32 = kernel32
        self.depth_code = depth_code
        self.output_pixel_bytes = output_pixel_bytes
        self.process = None
        self.input_offset = HEADER_BYTES
        self.output_offset = HEADER_BYTES + _align(WIDTH * HEIGHT * 4)
        self.section_bytes = self.output_offset + _align(
            WIDTH * HEIGHT * output_pixel_bytes)

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
        self.view_address = view
        self.view = (ctypes.c_ubyte * self.section_bytes).from_address(view)

        self.write_header(MAGIC_OFFSET, HEADER_MAGIC)
        self.write_header(VERSION_OFFSET, SESSION_HEADER_VERSION)
        self.write_header(DEPTH_CODE_OFFSET, depth_code)
        self.write_header(MAX_WIDTH_OFFSET, WIDTH)
        self.write_header(MAX_HEIGHT_OFFSET, HEIGHT)
        self.write_header(LAYER_SLOT_COUNT_OFFSET, 0)
        self.write_header(INPUT_GENERATION_OFFSET, 0)
        self.write_header(OUTPUT_GENERATION_OFFSET, 0)
        self.write_header(FRAME_WIDTH_OFFSET, WIDTH)
        self.write_header(FRAME_HEIGHT_OFFSET, HEIGHT)

    def attach_process(self, process):
        self.process = process

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
        while True:
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
            message = result["message"]
            if not (isinstance(message, dict) and
                    message.get("type") == "frame_done" and
                    message.get("status") == "resize_needed"):
                return message
            self._grow_output_capacity(message["width"], message["height"])

    def _grow_output_capacity(self, width, height):
        assert self.process is not None, "worker process is not attached"
        assert width > 0 and height > 0
        assert width > WIDTH or height > HEIGHT
        grown_section_bytes = self.output_offset + _align(
            width * height * self.output_pixel_bytes)

        PAGE_READWRITE = 0x04
        self.kernel32.CreateFileMappingW.restype = wintypes.HANDLE
        grown_section = self.kernel32.CreateFileMappingW(
            wintypes.HANDLE(-1), None, PAGE_READWRITE,
            (grown_section_bytes >> 32) & 0xFFFFFFFF,
            grown_section_bytes & 0xFFFFFFFF, None)
        assert grown_section, ctypes.get_last_error()
        grown_section_value = getattr(grown_section, "value", grown_section)

        FILE_MAP_ALL_ACCESS = 0x000F001F
        self.kernel32.MapViewOfFile.restype = wintypes.LPVOID
        grown_view_address = self.kernel32.MapViewOfFile(
            wintypes.HANDLE(grown_section_value), FILE_MAP_ALL_ACCESS, 0, 0, 0)
        if not grown_view_address:
            self.kernel32.CloseHandle(wintypes.HANDLE(grown_section_value))
            raise AssertionError(ctypes.get_last_error())
        grown_view = (ctypes.c_ubyte * grown_section_bytes).from_address(
            grown_view_address)

        # The worker has already copied this frame's input and rendered pixels
        # into private buffers. It only needs the broker-owned static header and
        # the last completed generation when adopting the new section.
        last_generation = self.read_header(OUTPUT_GENERATION_OFFSET)
        for offset, value in (
            (MAGIC_OFFSET, HEADER_MAGIC),
            (VERSION_OFFSET, SESSION_HEADER_VERSION),
            (DEPTH_CODE_OFFSET, self.depth_code),
            (MAX_WIDTH_OFFSET, WIDTH),
            (MAX_HEIGHT_OFFSET, HEIGHT),
            (LAYER_SLOT_COUNT_OFFSET, 0),
            (INPUT_GENERATION_OFFSET, last_generation),
            (OUTPUT_GENERATION_OFFSET, last_generation),
            (FRAME_WIDTH_OFFSET, WIDTH),
            (FRAME_HEIGHT_OFFSET, HEIGHT),
        ):
            struct.pack_into("<I", grown_view, offset, value)

        process_handle = getattr(self.process, "_handle", None)
        process_handle = getattr(process_handle, "value", process_handle)
        assert process_handle, "worker process handle is unavailable"
        self.kernel32.GetCurrentProcess.restype = wintypes.HANDLE
        self.kernel32.DuplicateHandle.argtypes = [
            wintypes.HANDLE, wintypes.HANDLE, wintypes.HANDLE,
            ctypes.POINTER(wintypes.HANDLE), wintypes.DWORD, wintypes.BOOL,
            wintypes.DWORD,
        ]
        self.kernel32.DuplicateHandle.restype = wintypes.BOOL
        duplicated = wintypes.HANDLE()
        DUPLICATE_SAME_ACCESS = 0x00000002
        assert self.kernel32.DuplicateHandle(
            self.kernel32.GetCurrentProcess(),
            wintypes.HANDLE(grown_section_value),
            wintypes.HANDLE(process_handle),
            ctypes.byref(duplicated), 0, False, DUPLICATE_SAME_ACCESS), \
            ctypes.get_last_error()
        duplicated_value = duplicated.value
        assert duplicated_value

        self.send({
            "v": 1,
            "type": "grow",
            "section_handle": str(duplicated_value),
            "output_capacity_width": width,
            "output_capacity_height": height,
        })

        self.kernel32.UnmapViewOfFile.argtypes = [wintypes.LPVOID]
        self.kernel32.UnmapViewOfFile.restype = wintypes.BOOL
        assert self.kernel32.UnmapViewOfFile(self.view_address)
        self.kernel32.CloseHandle(wintypes.HANDLE(
            getattr(self.section, "value", self.section)))
        self.section = grown_section
        self.view_address = grown_view_address
        self.view = grown_view
        self.section_bytes = grown_section_bytes

    def close_child_ends(self):
        # After spawn the parent must drop the child-side ends so a worker
        # exit turns into pipe EOF instead of a hang.
        self.kernel32.CloseHandle(wintypes.HANDLE(self.request_read))
        self.kernel32.CloseHandle(wintypes.HANDLE(self.response_write))
        self.request_read = None
        self.response_write = None


def _require_artifacts():
    if not WORKER.is_file():
        pytest.skip("aex_worker.exe is not built; run the minihost build")
    if not AEX.is_file():
        pytest.skip("pf_sampling_probe.aex is not built")


def probe_variant(marker, probe=None):
    """A copy of a probe whose file name carries a depth-advertisement marker.

    The probes read their own module path and change what they advertise when
    they see ``-shallow``, ``-floatonly`` or ``-rewrite`` in their file name,
    so the variant is a property
    of the plug-in the run loaded. The earlier spelling of this was an
    environment variable, which every other test that spawns the same probe
    inherited - and after a session report began describing the slot rather
    than the plug-in's world, those tests stayed green while silently
    measuring a narrowed run.

    Copied rather than written to a temp file so a failed run leaves the exact
    bytes that ran, next to the original so the build directory holds every
    variant of a probe together.

    One marker per probe, and one test per (probe, marker): the copy truncates
    in place, and pytest runs with ``-n auto --dist worksteal``, which makes no
    module affinity guarantee - two tests sharing a variant path could have one
    worker rewriting the `.aex` while another has it mapped.
    """
    probe = AEX if probe is None else probe
    variant = probe.with_name(f"{probe.stem}-{marker}{probe.suffix}")
    # Copied every time rather than when an mtime comparison says it is stale:
    # anything that touches the copy without changing its bytes (a restore, a
    # glob that resets timestamps) would make it permanently "fresh", and the
    # test would then run an old plug-in and still pass. A vacuous green is
    # worse than the copy.
    variant.write_bytes(probe.read_bytes())
    return variant


def _spawn(transport, aex=None, payload="v5|", command="--render-session-v1",
           variant=None):
    aex = AEX if aex is None else aex
    if variant is not None:
        aex = probe_variant(variant, aex)
    aex_sha = hashlib.sha256(aex.read_bytes()).hexdigest()
    process = subprocess.Popen(
        [str(WORKER), "--kind", "classic", command, str(aex), aex_sha, payload,
         str(WIDTH), str(HEIGHT), "1", "300", str(TIME_SCALE)],
        cwd=ROOT, env=transport.environment(), close_fds=False,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    transport.attach_process(process)
    transport.close_child_ends()
    return process


def _finish(process, timeout=60):
    stdout, stderr = process.communicate(timeout=timeout)
    return process.returncode, stdout, stderr


def render_frame_message(frame_index, time_value, scale=TIME_SCALE):
    return {"v": 1, "type": "render_frame", "frame_index": frame_index,
            "current_time": {"value": time_value, "scale": scale}}


def test_classic_session_narrows_a_plug_in_that_does_not_advertise_the_depth():
    """The classic twin of the smart narrowing case. What the caller receives
    is the slot, at the session's depth; the plug-in rendered into a narrower
    world that the frame loop widened into it. The final report has to describe
    the slot too - taking its row length from the slot and its pixel size from
    the plug-in's world reports the written half of every row as undefined
    padding, which reads as a plug-in that short-wrote its rows.
    """
    _require_artifacts()
    transport = SessionTransport(depth_code=16, output_pixel_bytes=8)
    process = _spawn(transport, command="--render-session16-v1",
                     variant="shallow")
    try:
        transport.write_input(11, 1)
        transport.send(render_frame_message(0, 0))
        done = transport.receive()
        assert done is not None, "worker closed the response pipe early"
        assert done["status"] == "ok", json.dumps(done)
        output = done["output"]
        assert output["pixel_format"] == "argb16"
        assert output["rowbytes"] == WIDTH * 8
        assert output["packed_bytes"] == WIDTH * HEIGHT * 8
        transport.send({"v": 1, "type": "close"})
        code, stdout, stderr = _finish(process)
        assert code == 0, (code, stderr[-800:])
        report = json.loads(stdout.strip())
        assert report["pixel_format"] == "argb16"
        assert report["rowbytes"] == WIDTH * 8
        assert report["bytes_written_per_row"] == WIDTH * 8
        assert report["undefined_tail_bytes_per_row"] == 0
        assert report["advertised_depth_supported"] is False
        assert report["depth_supported"] is True
        assert report["dispatch_pixel_bytes"] == 4
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_classic_session_dispatches_a_float_only_plug_in_at_float32():
    """A plug-in that advertises FLOAT_COLOR_AWARE and not DEEP_COLOR_AWARE is
    handed float32 worlds in a 16-bpc session and its frame is narrowed into
    the 16-bit slot. After Effects was measured to render such an effect above
    8-bit precision in a 16-bpc project
    (docs/DEPTH_FALLBACK_OBSERVATION_2026-09-17.md), so dropping it to 8 bits
    would be the wrong answer, not merely a coarser one.
    """
    _require_artifacts()
    transport = SessionTransport(depth_code=16, output_pixel_bytes=8)
    process = _spawn(transport, command="--render-session16-v1",
                     variant="floatonly")
    try:
        transport.write_input(11, 1)
        transport.send(render_frame_message(0, 0))
        done = transport.receive()
        assert done is not None, "worker closed the response pipe early"
        assert done["status"] == "ok", json.dumps(done)
        output = done["output"]
        assert output["pixel_format"] == "argb16"
        assert output["rowbytes"] == WIDTH * 8
        assert output["packed_bytes"] == WIDTH * HEIGHT * 8
        transport.send({"v": 1, "type": "close"})
        code, stdout, stderr = _finish(process)
        assert code == 0, (code, stderr[-800:])
        report = json.loads(stdout.strip())
        assert report["pixel_format"] == "argb16"
        assert report["rowbytes"] == WIDTH * 8
        assert report["undefined_tail_bytes_per_row"] == 0
        assert report["advertised_depth_supported"] is False
        assert report["depth_supported"] is True
        assert report["dispatch_pixel_bytes"] == 16
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_session_renders_frames_with_persistent_sequence_and_packed_extents():
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
            # The worker reports how many bytes it packed; the reader derives
            # the same extent from the reported dimensions (issue #690).
            assert output["packed_bytes"] == len(slot)
            checksums.append(hashlib.sha256(slot).hexdigest())
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


def _require_parameter_echo():
    if not PARAMETER_ECHO_AEX.is_file():
        pytest.skip("pf_parameter_echo_probe.aex is not built")


def _echo_frame_bytes(value):
    # The parameter echo probe fills every pixel with (red=value,
    # green=255-value, blue=128, alpha=255); the output slot carries RGBA.
    return bytes((value, 255 - value, 128, 255)) * (WIDTH * HEIGHT)


def render_frame_v2_message(frame_index, time_value, parameters,
                            scale=TIME_SCALE):
    return {"v": 2, "type": "render_frame", "frame_index": frame_index,
            "current_time": {"value": time_value, "scale": scale},
            "parameters": parameters}


def test_v2_parameters_replace_the_launch_assignments_for_one_frame():
    """Protocol §4.2.1: a v:2 frame renders with the message payload, and the
    next v:1 frame falls back to the launch payload (no sticky state)."""
    _require_artifacts()
    _require_parameter_echo()
    transport = SessionTransport()
    process = _spawn(transport, aex=PARAMETER_ECHO_AEX,
                     payload="v2|param_1@1:f64=32")
    try:
        expectations = [
            (render_frame_message(0, 0), 32),
            (render_frame_v2_message(1, 1, "v2|param_1@1:f64=200"), 200),
            (render_frame_message(2, 2), 32),
        ]
        for frame_index, (message, value) in enumerate(expectations):
            transport.write_input(frame_index * 7, frame_index + 1)
            transport.send(message)
            done = transport.receive()
            assert done is not None, "worker closed the response pipe early"
            assert done["status"] == "ok", done
            expected = _echo_frame_bytes(value)
            assert transport.output_bytes(len(expected)) == expected
            assert done["output"]["packed_bytes"] == len(expected)
        transport.send({"v": 1, "type": "close"})
        code, _, stderr = _finish(process)
        assert code == 0, (code, stderr[-500:])
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_v2_render_frame_without_parameters_is_a_protocol_violation():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        transport.write_input(17, 1)
        message = render_frame_message(0, 0)
        message["v"] = 2
        transport.send(message)
        assert transport.receive() is None
        code, _, _ = _finish(process)
        assert code == EXIT_PROTOCOL_VIOLATION
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_v2_close_is_a_protocol_violation():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        transport.send({"v": 2, "type": "close"})
        assert transport.receive() is None
        code, _, _ = _finish(process)
        assert code == EXIT_PROTOCOL_VIOLATION
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_v2_malformed_parameters_payload_fails_the_session_closed():
    """A payload the broker's pre-send validation would reject is a protocol
    violation, not a frame-local error (protocol §4.2.1)."""
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        transport.write_input(23, 1)
        transport.send(render_frame_v2_message(0, 0, "v9|param_1@1:f64=1"))
        assert transport.receive() is None
        code, _, _ = _finish(process)
        assert code == EXIT_PROTOCOL_VIOLATION
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_v2_non_ascii_parameters_payload_fails_the_session_closed():
    _require_artifacts()
    transport = SessionTransport()
    process = _spawn(transport)
    try:
        transport.write_input(29, 1)
        transport.send(render_frame_v2_message(0, 0, "v2|param_é@1:f64=1"))
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
        [str(WORKER), "--kind", "classic", "--render-session-v1", str(AEX), aex_sha, "v5|",
         str(WIDTH), str(HEIGHT), "1", "300", str(TIME_SCALE)],
        cwd=ROOT, capture_output=True, text=True, timeout=60)
    assert result.returncode == EXIT_PROTOCOL_VIOLATION


def test_session_launch_rejects_time_scale_above_int32():
    _require_artifacts()
    aex_sha = hashlib.sha256(AEX.read_bytes()).hexdigest()
    # The per-frame protocol carries scales as signed 32-bit, so a launch
    # scale above INT32_MAX could never be matched by any frame.
    result = subprocess.run(
        [str(WORKER), "--kind", "classic", "--render-session-v1", str(AEX), aex_sha, "v5|",
         str(WIDTH), str(HEIGHT), "1", "300", str(2**31)],
        cwd=ROOT, capture_output=True, text=True, timeout=60)
    assert result.returncode == 3


def _visual_audio_probe(target):
    """Resolve a pf-visual-audio-probe artifact across the layouts its build can
    produce. tools/build-pf-visual-audio-probe.ps1 uses a private multi-config
    tree and takes -Configuration Debug|Release, so both config subdirectories
    are searched. The instruments-build candidates cover a hand-run configure of
    `instruments` (single-config Ninja, or multi-config); CI does not build this
    probe at all, since its Ninja configure of `instruments` deliberately runs
    without AE_SDK_ROOT and the probe lives inside that guard. Returns None when
    the probe is unbuilt."""
    private = ROOT / "target" / "pf-visual-audio-probe-build" / "pf-visual-audio-probe"
    shared = ROOT / "target" / "instruments-build" / "pf-visual-audio-probe"
    for candidate in (
            private / "Release" / (target + ".aex"),
            private / "Debug" / (target + ".aex"),
            shared / (target + ".aex"),
            shared / "Release" / (target + ".aex")):
        if candidate.is_file():
            return candidate
    return None


SIDECAR_AEX = _visual_audio_probe("pf_visual_audio_sidecar_probe")
# pf_visual_audio_sidecar_probe checks out samples 4..9 at 44100 and returns
# PF_Err_NONE only when the window it reads back is exactly
# [0.25, 0, 0, 0, 0, 0.5625] followed by one silence sample past the end. A
# sidecar the worker never loaded therefore fails the render outright, which is
# what makes this a real test of the trailer rather than of the report shape.
SIDECAR_SAMPLES = [0.0, 0.0, 0.0, 0.0, 0.25, 0.0, 0.0, 0.0, 0.0, 0.5625]


def _sidecar_path(tmp_path):
    path = tmp_path / "session-audio.f32"
    path.write_bytes(struct.pack("<%df" % len(SIDECAR_SAMPLES), *SIDECAR_SAMPLES))
    return path


def test_session_audio_trailer_feeds_the_plug_in_the_checked_out_window(tmp_path):
    """The classic session carries an audio source through `session-audio:v1|`.

    The deleted one-shot spent three bare argv slots on the sample count, rate,
    and path under its own command word; a session cannot, because its tail is
    shared with the other optional trailers, so the values ride one marked
    argument peeled like the rest (issue #339). The name used to claim
    equivalence with that transport, which #365 removed.
    """
    if not WORKER.is_file():
        pytest.skip("aex_worker.exe is not built; run the minihost build")
    if SIDECAR_AEX is None:
        pytest.skip("pf_visual_audio_sidecar_probe.aex is not built; run "
                    "tools/build-pf-visual-audio-probe.ps1")
    sidecar = _sidecar_path(tmp_path)
    trailer = "session-audio:v1|%d|44100|%s" % (len(SIDECAR_SAMPLES), sidecar)
    transport = SessionTransport()
    aex_sha = hashlib.sha256(SIDECAR_AEX.read_bytes()).hexdigest()
    process = subprocess.Popen(
        [str(WORKER), "--kind", "classic", "--render-session-v1", str(SIDECAR_AEX), aex_sha, "v5|",
         str(WIDTH), str(HEIGHT), "1", "300", str(TIME_SCALE), trailer],
        cwd=ROOT, env=transport.environment(), close_fds=False,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    transport.close_child_ends()
    try:
        transport.write_input(11, 1)
        transport.send(render_frame_message(0, 0))
        done = transport.receive()
        assert done is not None, "worker closed the response pipe early"
        assert done["status"] == "ok", done
        # The probe returns INTERNAL_STRUCT_DAMAGED (512) unless every sample it
        # read back matched, so a zero render error is the audio assertion.
        assert done["render_error"] == 0, done
        transport.send({"v": 1, "type": "close"})
        code, stdout, stderr = _finish(process)
        assert code == 0, (code, stderr[-500:])
        report = json.loads(stdout.strip())
        assert report["audio_usage_advertised"] is True
        assert report["audio_source_available"] is True
        assert report["audio_checkout_allowed"] is True
        assert report["audio_checkout_calls"] == 1
        assert report["audio_checkin_calls"] == 1
        assert report["audio_get_data_calls"] == 1
        assert report["invalid_audio_operations"] == 0
        assert report["audio_lifetimes_balanced"] is True
        assert report["last_audio_checkout_start_time"] == 4
        assert report["last_audio_checkout_duration"] == 6
        assert report["last_audio_checkout_time_scale"] == 44100
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)


def test_session_without_the_audio_trailer_leaves_the_plug_in_no_source(tmp_path):
    """Without the trailer the same plug-in must fail, not silently render.

    This is the negative half of the pair: it fails if the worker ever invents
    an audio source the broker did not hand it, and it is what would have
    caught the trailer being dropped on the session route.
    """
    if not WORKER.is_file():
        pytest.skip("aex_worker.exe is not built; run the minihost build")
    if SIDECAR_AEX is None:
        pytest.skip("pf_visual_audio_sidecar_probe.aex is not built; run "
                    "tools/build-pf-visual-audio-probe.ps1")
    transport = SessionTransport()
    process = _spawn(transport, aex=SIDECAR_AEX)
    try:
        transport.write_input(11, 1)
        transport.send(render_frame_message(0, 0))
        done = transport.receive()
        assert done is not None, "worker closed the response pipe early"
        assert done["render_error"] != 0, done
        transport.send({"v": 1, "type": "close"})
        code, stdout, stderr = _finish(process)
        report = json.loads(stdout.strip()) if stdout.strip() else {}
        assert report.get("audio_source_available") is False, report
    finally:
        if process.poll() is None:
            process.kill()
            process.communicate(timeout=30)
