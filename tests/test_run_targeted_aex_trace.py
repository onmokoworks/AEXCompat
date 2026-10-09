import argparse
import ctypes
import errno
import hashlib
import importlib.util
import json
import os
import shutil
import stat
import subprocess
import sys
import time
from pathlib import Path

import pytest
from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "run_targeted_aex_trace", ROOT / "tools" / "run_targeted_aex_trace.py"
)
assert SPEC and SPEC.loader
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


def _sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _process_has_exited(pid: int) -> bool:
    if os.name == "nt":
        synchronize = 0x00100000
        wait_object_0 = 0
        wait_timeout = 258
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.OpenProcess.restype = ctypes.c_void_p
        kernel32.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
        kernel32.WaitForSingleObject.restype = ctypes.c_uint32
        kernel32.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
        kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
        handle = kernel32.OpenProcess(synchronize, False, pid)
        if not handle:
            error = ctypes.get_last_error()
            if error in (87, 1168):  # invalid parameter / not found
                return True
            raise OSError(error, f"OpenProcess({pid}) failed")
        try:
            result = kernel32.WaitForSingleObject(handle, 0)
        finally:
            kernel32.CloseHandle(handle)
        if result == wait_object_0:
            return True
        if result == wait_timeout:
            return False
        raise OSError(ctypes.get_last_error(), f"WaitForSingleObject({pid}) failed")

    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return True
    except PermissionError:
        return False
    except OSError as error:
        if error.errno == errno.ESRCH:
            return True
        if error.errno == errno.EPERM:
            return False
        raise

    # An orphaned descendant can remain as a zombie until PID 1 reaps it.
    # It is no longer executing even though kill(pid, 0) still succeeds.
    proc_stat = Path(f"/proc/{pid}/stat")
    if proc_stat.exists():
        try:
            state = proc_stat.read_text(encoding="ascii").rsplit(")", 1)[1].split()[0]
        except (IndexError, OSError):
            pass
        else:
            if state in {"Z", "X"}:
                return True
    return False


def _wait_for_process_exit(pid: int, timeout_seconds: float = 5) -> bool:
    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        if _process_has_exited(pid):
            return True
        time.sleep(0.05)
    return _process_has_exited(pid)


# Fixture processes record themselves with this snippet. On Windows a PID is
# not an identity: once the last handle to an exited process closes, the PID
# can name an unrelated process, and checking or killing "pid" then observes or
# terminates that process instead (issue #1763). So each fixture process
# duplicates a handle to itself into the test process while it is certainly
# alive, and the test waits on and terminates only through that handle. POSIX
# keeps the PID-based checks; CI runs this module only on Windows.
_RECORD_SELF = """
def _record_self(path, owner_pid):
    import json, os
    record = {"pid": os.getpid()}
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes
        k = ctypes.WinDLL("kernel32", use_last_error=True)
        k.OpenProcess.restype = wintypes.HANDLE
        k.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        k.GetCurrentProcess.restype = wintypes.HANDLE
        k.DuplicateHandle.restype = wintypes.BOOL
        k.DuplicateHandle.argtypes = [
            wintypes.HANDLE, wintypes.HANDLE, wintypes.HANDLE,
            ctypes.POINTER(wintypes.HANDLE), wintypes.DWORD, wintypes.BOOL, wintypes.DWORD,
        ]
        k.CloseHandle.argtypes = [wintypes.HANDLE]
        owner = k.OpenProcess(0x40, False, owner_pid)  # PROCESS_DUP_HANDLE
        if not owner:
            raise ctypes.WinError(ctypes.get_last_error())
        duplicated = wintypes.HANDLE()
        # SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION
        if not k.DuplicateHandle(
            k.GetCurrentProcess(), k.GetCurrentProcess(), owner,
            ctypes.byref(duplicated), 0x00100000 | 0x1 | 0x1000, False, 0,
        ):
            raise ctypes.WinError(ctypes.get_last_error())
        k.CloseHandle(owner)
        record["handle"] = duplicated.value
    staged = path + ".tmp"
    with open(staged, "w", encoding="ascii") as stream:
        json.dump(record, stream)
    os.replace(staged, path)
"""


def _record_self_call(path) -> str:
    """Fixture source that records the running process for this test process."""
    return f"_record_self({str(path)!r}, {os.getpid()})\n"


class _FixtureProcess:
    """A fixture process the test observes and stops by identity, not by PID."""

    def __init__(self, path: Path):
        record = json.loads(path.read_text(encoding="ascii"))
        self.pid = int(record["pid"])
        self._handle = None
        if os.name == "nt":
            self._kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            self._kernel32.WaitForSingleObject.restype = ctypes.c_uint32
            self._kernel32.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
            self._kernel32.TerminateProcess.restype = ctypes.c_int
            self._kernel32.TerminateProcess.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
            self._kernel32.GetProcessId.restype = ctypes.c_uint32
            self._kernel32.GetProcessId.argtypes = [ctypes.c_void_p]
            self._kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
            self._handle = int(record["handle"])
            # The handle value came from a file; make sure it is the recorded process.
            if self._kernel32.GetProcessId(self._handle) != self.pid:
                raise OSError(ctypes.get_last_error(), f"handle does not name PID {self.pid}")

    def __repr__(self) -> str:
        return f"fixture process {self.pid}"

    def wait_exit(self, timeout_seconds: float = 5) -> bool:
        if self._handle is None:
            return _wait_for_process_exit(self.pid, timeout_seconds)
        result = self._kernel32.WaitForSingleObject(self._handle, int(timeout_seconds * 1000))
        if result == 0:  # WAIT_OBJECT_0
            return True
        if result == 258:  # WAIT_TIMEOUT
            return False
        raise OSError(ctypes.get_last_error(), f"WaitForSingleObject({self.pid}) failed")

    def stop(self) -> None:
        """Terminate this fixture process if it is still running, then close it."""
        try:
            if self.wait_exit(0):
                return
            if self._handle is None:
                os.kill(self.pid, 9)
            elif not self._kernel32.TerminateProcess(self._handle, 9):
                error = ctypes.get_last_error()
                # TerminateProcess reports ACCESS_DENIED for a process that is
                # already exiting; anything else, or no exit, is a real failure.
                if not (error == 5 and self.wait_exit(5)):
                    raise OSError(error, f"TerminateProcess({self.pid}) failed")
            assert self.wait_exit(), f"{self!r} did not stop"
        finally:
            self.close()

    def close(self) -> None:
        if self._handle is not None:
            self._kernel32.CloseHandle(self._handle)
            self._handle = None


class _FixtureProcesses:
    """Load each record once, so each duplicated handle has exactly one owner."""

    def __init__(self, *paths: Path):
        self._paths = paths
        self._loaded: dict[Path, _FixtureProcess] = {}

    def get(self, path: Path) -> _FixtureProcess:
        if path not in self._loaded:
            assert path.exists(), f"fixture process did not record itself in {path.name}"
            self._loaded[path] = _FixtureProcess(path)
        return self._loaded[path]

    def stop_all(self) -> None:
        """Stop every fixture process that recorded itself, even after a failure."""
        errors = []
        for path in self._paths:
            if path not in self._loaded and not path.exists():
                continue
            try:
                self.get(path).stop()
            except Exception as error:  # keep stopping the others
                errors.append(error)
        if errors:
            raise errors[0]


def _fixture(tmp_path):
    worker = tmp_path / "worker"
    plugin = tmp_path / "plugin.aex"
    input_png = tmp_path / "input.png"
    worker.write_bytes(b"worker")
    plugin.write_bytes(b"plugin")
    input_png.write_bytes(b"png")
    manifest = tmp_path / "manifest.json"
    manifest.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "cases": [
                    {
                        "id": "argb8-a",
                        "plugin": {"path": str(plugin), "sha256": _sha(plugin)},
                        "input_png": {
                            "path": str(input_png),
                            "sha256": _sha(input_png),
                        },
                        "pixel_format": "argb8",
                        "parameters": ["Amount=25"],
                    }
                ],
            }
        ),
        encoding="utf-8",
    )
    return worker, plugin, input_png, manifest


def test_manifest_pins_plugin_and_input_and_is_bounded(tmp_path):
    _, plugin, _, manifest = _fixture(tmp_path)
    cases = RUNNER.load_manifest(manifest)
    assert cases[0]["plugin_sha256"] == _sha(plugin)

    value = json.loads(manifest.read_text())
    value["cases"][0]["plugin"]["sha256"] = "0" * 64
    manifest.write_text(json.dumps(value), encoding="utf-8")
    with pytest.raises(RUNNER.TraceRunnerError, match="SHA-256 mismatch"):
        RUNNER.load_manifest(manifest)

    value["cases"] *= RUNNER.MAX_CASES + 1
    manifest.write_text(json.dumps(value), encoding="utf-8")
    with pytest.raises(RUNNER.TraceRunnerError, match=r"1\.\.8"):
        RUNNER.load_manifest(manifest)


def test_success_report_is_portable_sha_pinned_and_deterministic(
    tmp_path, monkeypatch
):
    worker, plugin, input_png, manifest = _fixture(tmp_path)
    output = tmp_path / "report.json"

    def fake_run(command, timeout_seconds):
        Image.new("RGBA", (1, 1), (1, 2, 3, 255)).save(command[4], "PNG")
        worker_report = {
            "execution_traces": [
                {"selector": "RENDER", "note": command[2]}
            ],
            "classification": "ok",
        }
        return (
            subprocess.CompletedProcess(
                command, 0, json.dumps(worker_report).encode(), b""
            ),
            False,
            None,
        )

    monkeypatch.setattr(RUNNER, "_run_bounded_process", fake_run)
    args = argparse.Namespace(
        manifest=manifest,
        worker=worker,
        expected_worker_sha256=_sha(worker),
        output=output,
        timeout=10,
        run_parent=tmp_path,
    )
    first = RUNNER.run(args)
    first_bytes = output.read_bytes()
    second = RUNNER.run(args)

    assert first == second
    assert output.read_bytes() == first_bytes
    assert first["worker_sha256"] == _sha(worker)
    assert first["cases"][0]["plugin_sha256"] == _sha(plugin)
    assert first["cases"][0]["input_png_sha256"] == _sha(input_png)
    assert first["cases"][0]["result"]["kind"] == "trace"
    assert str(tmp_path) not in output.read_text()
    assert first["cases"][0]["result"]["report"]["execution_traces"][0][
        "note"
    ] == "<plugin>"


@pytest.mark.parametrize("temp_inside_home", [False, True])
def test_failure_extracts_structured_crash_and_redacts_paths(
    tmp_path, monkeypatch, temp_inside_home
):
    # Simulate both locations without writing anything in the real user's home.
    home = tmp_path if temp_inside_home else tmp_path / "different-home"
    monkeypatch.setattr(RUNNER.Path, "home", lambda: home)
    worker, _, _, manifest = _fixture(tmp_path)
    output = tmp_path / "report.json"
    def fake_run(command, timeout_seconds):
        snapshot = {
            "reason": "UC_ERR_FETCH_PROT",
            "registers": {"rip": 0},
            "module": command[2],
        }
        stderr = (
            f"aex_guest_error: guest execution failed at {command[2]}; "
            f"crash_snapshot={json.dumps(snapshot)}\n"
        ).encode()
        return (
            subprocess.CompletedProcess(
                command,
                1,
                json.dumps(
                    {
                        "setup": {"execution_backend": "unicorn-x86_64"},
                        "output_png": str(tmp_path / "must-not-leak.png"),
                    }
                ).encode(),
                stderr,
            ),
            False,
            None,
        )

    monkeypatch.setattr(RUNNER, "_run_bounded_process", fake_run)
    report = RUNNER.run(
        argparse.Namespace(
            manifest=manifest,
            worker=worker,
            expected_worker_sha256=_sha(worker),
            output=output,
            timeout=10,
            run_parent=tmp_path,
        )
    )
    result = report["cases"][0]["result"]

    assert result["kind"] == "worker_error"
    assert result["crash_snapshot"]["reason"] == "UC_ERR_FETCH_PROT"
    assert result["crash_snapshot"]["registers"]["rip"] == 0
    assert result["crash_snapshot"]["module"] == "<plugin>"
    assert result["partial_report"]["setup"]["execution_backend"] == "unicorn-x86_64"
    expected = "<home>" if temp_inside_home else "<absolute-path>"
    assert result["partial_report"]["output_png"].startswith(expected)
    assert str(tmp_path) not in output.read_text()


@pytest.mark.parametrize(
    "private_path",
    [
        r"D:\outside-home\output.png",
        "D:/outside-home/output.png",
        r"D:\outside home\private image.png",
        r"\\server\private share\output.png",
        r"\\?\D:\outside home\output.png",
        "/private/outside-home/output.png",
        "/private/outside home/output.png",
        r"D:\private\alice;secret\output.png",
        "/private/alice;secret/output.png",
        r"D:\another user's files\output.png",
    ],
)
def test_unknown_absolute_paths_are_redacted_in_nested_reports(private_path):
    payload = {
        "reason": "UC_ERR_FETCH_PROT",
        "registers": {"rip": 0, "rax": 42},
        "module": "effect.dll",
        "nested": [{"output_png": private_path}],
    }
    sanitized = RUNNER._sanitize(payload, {})
    assert sanitized == {
        **payload,
        "nested": [{"output_png": "<absolute-path>"}],
    }
    assert private_path not in json.dumps(sanitized)


@pytest.mark.parametrize(
    "private_path",
    [
        r"D:\private\output.png",
        r"\\server\share\output.png",
        r"\\?\D:\private\output.png",
        "D:/private/output.png",
        "/private/output.png",
    ],
)
def test_embedded_absolute_paths_preserve_error_reason(private_path):
    assert RUNNER._redact_text(f"failed at {private_path}; reason=EIO", {}) == (
        "failed at <absolute-path>; reason=EIO"
    )


@pytest.mark.parametrize(
    "private_path",
    [
        r"D:\private folder\secret image.png",
        r"\\server\private share\secret.png",
        "/private folder/secret image.png",
        r"D:\another user's files\image.png",
        r"D:\private\alice;secret\output.png",
    ],
)
@pytest.mark.parametrize("prefix", ["", "failed at "])
def test_unquoted_spaced_paths_preserve_explicit_diagnostic_suffix(
    private_path, prefix
):
    suffix = ": permission denied; reason=EIO"
    assert RUNNER._redact_text(f"{prefix}{private_path}{suffix}", {}) == (
        f"{prefix}<absolute-path>{suffix}"
    )


@pytest.mark.parametrize("quote", ['"', "'"])
@pytest.mark.parametrize(
    "private_path",
    [
        r"D:\private folder\image.png",
        r"\\server\private share\image.png",
        "/private folder/image.png",
        r"D:\private\alice;secret\image.png",
    ],
)
def test_quoted_paths_with_spaces_preserve_error_reason(private_path, quote):
    assert RUNNER._redact_text(
        f"failed at {quote}{private_path}{quote}; reason=EIO", {}
    ) == f"failed at {quote}<absolute-path>{quote}; reason=EIO"


def test_success_unknown_path_is_redacted_in_persisted_report(tmp_path, monkeypatch):
    worker, _, _, manifest = _fixture(tmp_path)
    output = tmp_path / "report.json"
    private_path = r"D:\another user's files\unknown.png"

    def fake_run(command, timeout_seconds):
        Image.new("RGBA", (1, 1), (1, 2, 3, 255)).save(command[4], "PNG")
        return (
            subprocess.CompletedProcess(
                command, 0,
                json.dumps({"execution_traces": [{"selector": "RENDER"}],
                            "output_png": private_path}).encode(), b""
            ), False, None,
        )

    monkeypatch.setattr(RUNNER, "_run_bounded_process", fake_run)
    report = RUNNER.run(_main_args(worker, manifest, output, tmp_path))
    assert report["cases"][0]["result"]["report"]["output_png"] == "<absolute-path>"
    assert json.loads(output.read_text(encoding="utf-8")) == report


def test_known_path_tokens_and_non_path_diagnostics_are_preserved():
    replacements = {r"D:\private\plugin.aex": "<plugin>"}
    assert RUNNER._redact_text(r"D:\private\plugin.aex", replacements) == "<plugin>"
    for value in ["effect.dll", "UC_ERR_FETCH_PROT", "RENDER", "relative/output.png"]:
        assert RUNNER._redact_text(value, {}) == value


def test_structured_diagnostic_fields_preserve_path_prefixed_reason():
    value = {
        "message": r"D:\private\file.png: permission denied; reason=EIO",
        "reason": "/private/file.png: permission denied; reason=EIO",
        "registers": {"rip": 0},
        "module": r"D:\private\alice;secret\module.dll",
    }
    assert RUNNER._sanitize(value, {}) == {
        "message": "<absolute-path>: permission denied; reason=EIO",
        "reason": "<absolute-path>: permission denied; reason=EIO",
        "registers": {"rip": 0},
        "module": "<absolute-path>",
    }


def test_strict_json_rejects_duplicate_keys():
    with pytest.raises(RUNNER.TraceRunnerError, match="duplicate JSON key"):
        RUNNER.strict_json_bytes(b'{"a":1,"a":2}', "test")


@pytest.mark.parametrize("stream_name", ["stdout", "stderr"])
def test_bounded_capture_stops_oversized_stream(stream_name, monkeypatch):
    monkeypatch.setattr(RUNNER, "MAX_CAPTURE_BYTES", 1024)
    monkeypatch.setattr(RUNNER, "MAX_COMBINED_CAPTURE_BYTES", 1024)
    code = (
        "import os\n"
        f"fd = {1 if stream_name == 'stdout' else 2}\n"
        "while True:\n"
        " os.write(fd, b'x' * 4096)\n"
    )
    completed, timed_out, reason = RUNNER._run_bounded_process(
        [sys.executable, "-c", code], 5
    )
    assert not timed_out
    assert reason == "capture_limit"
    assert len(completed.stdout) <= 1024
    assert len(completed.stderr) <= 1024
    assert len(completed.stdout) + len(completed.stderr) <= 1024


def test_bounded_capture_enforces_combined_limit(monkeypatch):
    monkeypatch.setattr(RUNNER, "MAX_CAPTURE_BYTES", 1024)
    monkeypatch.setattr(RUNNER, "MAX_COMBINED_CAPTURE_BYTES", 1200)
    code = (
        "import os\n"
        "os.write(1, b'o' * 700)\n"
        "os.write(2, b'e' * 700)\n"
    )
    completed, timed_out, reason = RUNNER._run_bounded_process(
        [sys.executable, "-c", code], 5
    )
    assert not timed_out
    assert reason == "capture_limit"
    assert len(completed.stdout) <= 1024
    assert len(completed.stderr) <= 1024
    assert len(completed.stdout) + len(completed.stderr) == 1200


def test_bounded_capture_timeout_cleans_up_descendant(tmp_path):
    child_record = tmp_path / "descendant.json"
    child_code = (
        _RECORD_SELF
        + "import time\n"
        + _record_self_call(child_record)
        + "time.sleep(30)\n"
    )
    parent_code = (
        "import subprocess,sys,time\n"
        f"child=subprocess.Popen([sys.executable, '-c', {child_code!r}])\n"
        "time.sleep(30)\n"
    )
    processes = _FixtureProcesses(child_record)
    try:
        completed, timed_out, reason = RUNNER._run_bounded_process(
            [sys.executable, "-c", parent_code], 2
        )
        assert timed_out
        assert reason == "timeout"
        assert completed.returncode is not None
        child = processes.get(child_record)
        assert child.wait_exit(), f"descendant {child!r} survived"
    finally:
        processes.stop_all()


@pytest.mark.skipif(os.name != "nt", reason="Windows Job descendant containment")
@pytest.mark.parametrize("inherit_pipes", [False, True])
@pytest.mark.parametrize("finish", ["exit", "timeout", "capture_limit"])
def test_bounded_capture_cleans_descendants(tmp_path, monkeypatch, inherit_pipes, finish):
    ready = tmp_path / "child.json"
    grandchild_ready = tmp_path / "grandchild.json"
    marker = tmp_path / "escaped.txt"
    grandchild_code = (
        _RECORD_SELF
        + "import pathlib,time\n"
        + _record_self_call(grandchild_ready)
        + "time.sleep(10)\n"
        + f"pathlib.Path({str(marker)!r}).write_text('escaped')\n"
    )
    child_code = (
        _RECORD_SELF
        + "import pathlib,subprocess,sys,time\n"
        + f"child=subprocess.Popen([sys.executable, '-c', {grandchild_code!r}])\n"
        + f"ready=pathlib.Path({str(grandchild_ready)!r})\n"
        + "deadline=time.monotonic()+5\n"
        + "while not ready.exists() and time.monotonic()<deadline: time.sleep(.01)\n"
        + "assert ready.exists()\n"
        + _record_self_call(ready)
        + "time.sleep(30)\n"
    )
    streams = "" if inherit_pipes else ", stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL"
    ending = {
        "exit": "sys.stdout.buffer.write(b'parent done')\n",
        "timeout": "time.sleep(30)\n",
        "capture_limit": "while True: os.write(1, b'x'*65536)\n",
    }[finish]
    parent_code = (
        "import os,pathlib,subprocess,sys,time\n"
        f"subprocess.Popen([sys.executable, '-c', {child_code!r}]{streams})\n"
        f"ready=pathlib.Path({str(ready)!r})\n"
        "deadline=time.monotonic()+5\n"
        "while not ready.exists() and time.monotonic()<deadline: time.sleep(.01)\n"
        "assert ready.exists()\n"
        + ending
    )
    monkeypatch.setattr(RUNNER, "MAX_CAPTURE_BYTES", 1024)
    monkeypatch.setattr(RUNNER, "MAX_COMBINED_CAPTURE_BYTES", 1024)
    # A failing baseline must not leave either fixture descendant running.
    processes = _FixtureProcesses(ready, grandchild_ready)
    started = time.monotonic()
    try:
        completed, timed_out, reason = RUNNER._run_bounded_process(
            [sys.executable, "-c", parent_code], 2 if finish == "timeout" else 15
        )
        assert completed.returncode is not None
        assert timed_out == (finish == "timeout")
        assert reason == (None if finish == "exit" else finish)
        if finish == "exit":
            assert completed.returncode == 0
            assert completed.stdout == b"parent done"
        assert len(completed.stdout) + len(completed.stderr) <= 1024
        assert time.monotonic() - started < 5, "inherited pipe delayed cleanup"
        for path in (ready, grandchild_ready):
            descendant = processes.get(path)
            assert descendant.wait_exit(2), f"descendant {descendant!r} survived"
        assert not marker.exists()
    finally:
        processes.stop_all()


@pytest.mark.skipif(os.name != "nt", reason="Windows suspended launch failure")
@pytest.mark.parametrize("stage", ["assignment", "resume", "capture_thread"])
def test_bounded_capture_launch_failure_cleans_owned_process(tmp_path, monkeypatch, stage):
    marker = tmp_path / "executed.txt"
    launched = []
    popen = subprocess.Popen

    def record_popen(*args, **kwargs):
        process = popen(*args, **kwargs)
        launched.append(process)
        return process

    def fail(*args, **kwargs):
        raise KeyboardInterrupt("injected startup interruption")

    monkeypatch.setattr(RUNNER.subprocess, "Popen", record_popen)
    if stage == "assignment":
        monkeypatch.setattr(RUNNER._WindowsCaptureJob, "assign_and_resume", fail)
    elif stage == "resume":
        monkeypatch.setattr(RUNNER._WindowsCaptureJob, "_resume_primary_thread", fail)
    else:
        monkeypatch.setattr(RUNNER.threading.Thread, "start", fail)
    code = f"from pathlib import Path; import time; time.sleep(2); Path({str(marker)!r}).write_text('executed')"
    with pytest.raises(KeyboardInterrupt, match="injected startup"):
        RUNNER._run_bounded_process([sys.executable, "-c", code], 5)
    assert len(launched) == 1
    assert launched[0].poll() is not None
    assert launched[0].stdout.closed and launched[0].stderr.closed
    assert not marker.exists()


@pytest.mark.skipif(os.name not in {"nt", "posix"}, reason="process tree cleanup")
def test_bounded_capture_partial_reader_start_failure_after_parent_exit(tmp_path, monkeypatch):
    record = tmp_path / "descendant.json"
    marker = tmp_path / "escaped.txt"
    child_code = (
        _RECORD_SELF
        + "import pathlib,time\n"
        + _record_self_call(record)
        + "time.sleep(10)\n"
        + f"pathlib.Path({str(marker)!r}).write_text('escaped')\n"
    )
    parent_code = (
        "import pathlib,subprocess,sys,time\n"
        f"child=subprocess.Popen([sys.executable, '-c', {child_code!r}])\n"
        f"record=pathlib.Path({str(record)!r})\n"
        "deadline=time.monotonic()+5\n"
        "while not record.exists() and time.monotonic()<deadline: time.sleep(.01)\n"
        "assert record.exists()\n"
    )
    launched = []
    popen = subprocess.Popen
    start = RUNNER.threading.Thread.start
    calls = 0

    def record_popen(*args, **kwargs):
        process = popen(*args, **kwargs)
        launched.append(process)
        return process

    def fail_second_start(thread):
        nonlocal calls
        calls += 1
        if calls == 2:
            assert launched[0].wait(timeout=5) == 0
            raise KeyboardInterrupt("injected second reader interruption")
        return start(thread)

    monkeypatch.setattr(RUNNER.subprocess, "Popen", record_popen)
    monkeypatch.setattr(RUNNER.threading.Thread, "start", fail_second_start)
    processes = _FixtureProcesses(record)
    started = time.monotonic()
    try:
        with pytest.raises(KeyboardInterrupt, match="second reader"):
            RUNNER._run_bounded_process([sys.executable, "-c", parent_code], 15)
        assert time.monotonic() - started < 5
        assert launched[0].stdout.closed and launched[0].stderr.closed
        descendant = processes.get(record)
        assert descendant.wait_exit(2), f"descendant {descendant!r} survived"
        assert not marker.exists()
    finally:
        processes.stop_all()


@pytest.mark.parametrize("exit_code", [0, 7])
def test_bounded_capture_preserves_normal_success_and_failure(exit_code):
    code = (
        "import sys\n"
        "sys.stdout.buffer.write(b'{\"execution_traces\":[{}]}')\n"
        "sys.stderr.buffer.write(b'bounded diagnostic')\n"
        f"raise SystemExit({exit_code})\n"
    )
    completed, timed_out, reason = RUNNER._run_bounded_process(
        [sys.executable, "-c", code], 5
    )
    assert completed.returncode == exit_code
    assert completed.stdout == b'{"execution_traces":[{}]}'
    assert completed.stderr == b"bounded diagnostic"
    assert not timed_out
    assert reason is None


def test_success_requires_a_trace_payload(tmp_path, monkeypatch):
    worker, _, _, manifest = _fixture(tmp_path)
    monkeypatch.setattr(
        RUNNER,
        "_run_bounded_process",
        lambda command, timeout_seconds: (
            subprocess.CompletedProcess(
                command, 0, b'{"classification":"ok"}', b""
            ),
            False,
            None,
        ),
    )
    with pytest.raises(RUNNER.TraceRunnerError, match="no execution_traces"):
        RUNNER.run(
            argparse.Namespace(
                manifest=manifest,
                worker=worker,
                expected_worker_sha256=_sha(worker),
                output=tmp_path / "report.json",
                timeout=10,
                run_parent=tmp_path,
            )
        )


@pytest.mark.parametrize("artifact", ["missing", "invalid", "directory"])
def test_success_requires_a_valid_regular_output_png(
    tmp_path, monkeypatch, artifact
):
    worker, _, _, manifest = _fixture(tmp_path)

    def fake_run(command, timeout_seconds):
        output = Path(command[4])
        if artifact == "invalid":
            output.write_bytes(b"not a png")
        elif artifact == "directory":
            output.mkdir()
        return (
            subprocess.CompletedProcess(
                command,
                0,
                b'{"execution_traces":[{"selector":"RENDER"}]}',
                b"",
            ),
            False,
            None,
        )

    monkeypatch.setattr(RUNNER, "_run_bounded_process", fake_run)
    with pytest.raises(
        RUNNER.TraceRunnerError,
        match="missing or unreadable|bounded regular PNG|valid readable PNG",
    ):
        RUNNER.run(
            argparse.Namespace(
                manifest=manifest,
                worker=worker,
                expected_worker_sha256=_sha(worker),
                output=tmp_path / "report.json",
                timeout=10,
                run_parent=tmp_path,
            )
        )


@pytest.mark.parametrize("protected_name", ["manifest", "worker", "plugin", "input"])
@pytest.mark.parametrize("alias_kind", ["direct", "hardlink", "symlink"])
def test_output_cannot_alias_a_pinned_input_before_worker_launch(
    tmp_path, monkeypatch, protected_name, alias_kind
):
    worker, plugin, input_png, manifest = _fixture(tmp_path)
    protected = {
        "manifest": manifest,
        "worker": worker,
        "plugin": plugin,
        "input": input_png,
    }[protected_name]
    before = protected.read_bytes()
    if alias_kind == "direct":
        output = protected
    elif alias_kind == "hardlink":
        output = tmp_path / f"{protected_name}-report-hardlink.json"
        output.hardlink_to(protected)
    else:
        output = tmp_path / f"{protected_name}-report-symlink.json"
        output.symlink_to(protected)
    launched = False

    def must_not_launch(*args, **kwargs):
        nonlocal launched
        launched = True
        raise AssertionError("worker must not launch for an aliased output")

    monkeypatch.setattr(RUNNER, "_run_bounded_process", must_not_launch)
    with pytest.raises(RUNNER.TraceRunnerError, match="output aliases protected"):
        RUNNER.run(
            argparse.Namespace(
                manifest=manifest,
                worker=worker,
                expected_worker_sha256=_sha(worker),
                output=output,
                timeout=10,
                run_parent=tmp_path,
            )
        )

    assert not launched
    assert protected.read_bytes() == before


def test_preexisting_output_symlink_replaces_link_not_unrelated_victim(
    tmp_path, monkeypatch
):
    worker, _, _, manifest = _fixture(tmp_path)
    victim = tmp_path / "unrelated-victim.txt"
    victim.write_bytes(b"do not overwrite")
    report_path = tmp_path / "report.json"
    report_path.symlink_to(victim)

    def fake_run(command, timeout_seconds):
        Image.new("RGBA", (1, 1), (1, 2, 3, 255)).save(command[4], "PNG")
        return (
            subprocess.CompletedProcess(
                command,
                0,
                b'{"execution_traces":[{"selector":"RENDER"}]}',
                b"",
            ),
            False,
            None,
        )

    monkeypatch.setattr(RUNNER, "_run_bounded_process", fake_run)
    report = RUNNER.run(
        argparse.Namespace(
            manifest=manifest,
            worker=worker,
            expected_worker_sha256=_sha(worker),
            output=report_path,
            timeout=10,
            run_parent=tmp_path,
        )
    )

    assert victim.read_bytes() == b"do not overwrite"
    assert not report_path.is_symlink()
    assert json.loads(report_path.read_text(encoding="utf-8")) == report


def test_parent_symlink_swap_cannot_redirect_report_replace(
    tmp_path, monkeypatch
):
    worker, _, _, manifest = _fixture(tmp_path)
    original_parent = tmp_path / "original"
    unrelated_parent = tmp_path / "unrelated"
    original_parent.mkdir()
    unrelated_parent.mkdir()
    parent_link = tmp_path / "report-parent"
    parent_link.symlink_to(original_parent, target_is_directory=True)
    requested_report = parent_link / "report.json"

    def fake_run(command, timeout_seconds):
        Image.new("RGBA", (1, 1), (1, 2, 3, 255)).save(command[4], "PNG")
        parent_link.unlink()
        parent_link.symlink_to(unrelated_parent, target_is_directory=True)
        return (
            subprocess.CompletedProcess(
                command,
                0,
                b'{"execution_traces":[{"selector":"RENDER"}]}',
                b"",
            ),
            False,
            None,
        )

    monkeypatch.setattr(RUNNER, "_run_bounded_process", fake_run)
    report = RUNNER.run(
        argparse.Namespace(
            manifest=manifest,
            worker=worker,
            expected_worker_sha256=_sha(worker),
            output=requested_report,
            timeout=10,
            run_parent=tmp_path,
        )
    )

    canonical_report = original_parent / "report.json"
    assert json.loads(canonical_report.read_text(encoding="utf-8")) == report
    assert not (unrelated_parent / "report.json").exists()


def _main_args(worker, manifest, output, tmp_path):
    return argparse.Namespace(
        manifest=manifest,
        worker=worker,
        expected_worker_sha256=_sha(worker),
        output=output,
        timeout=10,
        run_parent=tmp_path,
    )


def test_main_returns_zero_when_every_case_traces(
    tmp_path, monkeypatch, capsys
):
    worker, _, _, manifest = _fixture(tmp_path)
    output = tmp_path / "report.json"

    def fake_run(command, timeout_seconds):
        Image.new("RGBA", (1, 1), (1, 2, 3, 255)).save(command[4], "PNG")
        return (
            subprocess.CompletedProcess(
                command,
                0,
                b'{"execution_traces":[{"selector":"RENDER"}]}',
                b"",
            ),
            False,
            None,
        )

    monkeypatch.setattr(RUNNER, "_run_bounded_process", fake_run)
    monkeypatch.setattr(
        RUNNER, "parse_args", lambda: _main_args(
            worker, manifest, output, tmp_path
        )
    )

    assert RUNNER.main() == 0
    assert json.loads(capsys.readouterr().out) == {
        "case_count": 1,
        "result_kinds": ["trace"],
    }
    assert json.loads(output.read_text(encoding="utf-8"))["cases"][0][
        "result"
    ]["kind"] == "trace"


@pytest.mark.parametrize("failure_kind", ["timeout", "worker_error"])
def test_main_returns_nonzero_but_keeps_mixed_failure_report(
    tmp_path, monkeypatch, capsys, failure_kind
):
    worker, _, _, manifest = _fixture(tmp_path)
    manifest_value = json.loads(manifest.read_text(encoding="utf-8"))
    second = dict(manifest_value["cases"][0])
    second["id"] = "argb8-b"
    manifest_value["cases"].append(second)
    manifest.write_text(json.dumps(manifest_value), encoding="utf-8")
    output = tmp_path / "report.json"
    calls = 0

    def fake_run(command, timeout_seconds):
        nonlocal calls
        calls += 1
        if calls == 1:
            Image.new("RGBA", (1, 1), (1, 2, 3, 255)).save(
                command[4], "PNG"
            )
            return (
                subprocess.CompletedProcess(
                    command,
                    0,
                    b'{"execution_traces":[{"selector":"RENDER"}]}',
                    b"",
                ),
                False,
                None,
            )
        if failure_kind == "timeout":
            return (
                subprocess.CompletedProcess(command, -9, b"", b""),
                True,
                "timeout",
            )
        return (
            subprocess.CompletedProcess(
                command, 1, b"", b"aex_guest_error: failed"
            ),
            False,
            None,
        )

    monkeypatch.setattr(RUNNER, "_run_bounded_process", fake_run)
    monkeypatch.setattr(
        RUNNER, "parse_args", lambda: _main_args(
            worker, manifest, output, tmp_path
        )
    )

    assert RUNNER.main() == 1
    summary = json.loads(capsys.readouterr().out)
    assert summary == {
        "case_count": 2,
        "result_kinds": ["trace", failure_kind],
    }
    persisted = json.loads(output.read_text(encoding="utf-8"))
    assert [
        case["result"]["kind"] for case in persisted["cases"]
    ] == ["trace", failure_kind]


def test_worker_launch_uses_verified_private_staged_sources(
    tmp_path, monkeypatch
):
    worker, plugin, input_png, manifest = _fixture(tmp_path)
    original_bytes = {
        "worker": worker.read_bytes(),
        "plugin": plugin.read_bytes(),
        "input": input_png.read_bytes(),
    }
    output = tmp_path / "report.json"

    def fake_run(command, timeout_seconds):
        worker.write_bytes(b"replaced worker")
        plugin.write_bytes(b"replaced plugin")
        input_png.write_bytes(b"replaced input")

        staged_worker = Path(command[0])
        staged_plugin = Path(command[2])
        staged_input = Path(command[3])
        assert staged_worker != worker
        assert staged_plugin != plugin
        assert staged_input != input_png
        assert staged_worker.read_bytes() == original_bytes["worker"]
        assert staged_plugin.read_bytes() == original_bytes["plugin"]
        assert staged_input.read_bytes() == original_bytes["input"]
        expected_worker_mode = 0o444 if os.name == "nt" else 0o500
        expected_data_mode = 0o444 if os.name == "nt" else 0o400
        assert stat.S_IMODE(staged_worker.stat().st_mode) == expected_worker_mode
        assert stat.S_IMODE(staged_plugin.stat().st_mode) == expected_data_mode
        assert stat.S_IMODE(staged_input.stat().st_mode) == expected_data_mode
        Image.new("RGBA", (1, 1), (1, 2, 3, 255)).save(command[4], "PNG")
        return (
            subprocess.CompletedProcess(
                command,
                0,
                b'{"execution_traces":[{"selector":"RENDER"}]}',
                b"",
            ),
            False,
            None,
        )

    monkeypatch.setattr(RUNNER, "_run_bounded_process", fake_run)
    report = RUNNER.run(
        _main_args(worker, manifest, output, tmp_path)
    )

    assert report["worker_sha256"] == hashlib.sha256(
        original_bytes["worker"]
    ).hexdigest()
    assert report["cases"][0]["plugin_sha256"] == hashlib.sha256(
        original_bytes["plugin"]
    ).hexdigest()
    assert report["cases"][0]["input_png_sha256"] == hashlib.sha256(
        original_bytes["input"]
    ).hexdigest()
    assert str(tmp_path) not in output.read_text(encoding="utf-8")


def test_source_mutation_during_staging_fails_before_worker_launch(
    tmp_path, monkeypatch
):
    worker, plugin, _, manifest = _fixture(tmp_path)
    output = tmp_path / "report.json"
    real_copyfile = shutil.copyfile
    launched = False

    def racing_copyfile(source, destination):
        if Path(source) == plugin:
            plugin.write_bytes(b"plugin changed after initial pin")
        return real_copyfile(source, destination)

    def must_not_launch(*args, **kwargs):
        nonlocal launched
        launched = True
        raise AssertionError("worker must not launch after staged SHA mismatch")

    monkeypatch.setattr(RUNNER.shutil, "copyfile", racing_copyfile)
    monkeypatch.setattr(RUNNER, "_run_bounded_process", must_not_launch)
    with pytest.raises(
        RUNNER.TraceRunnerError, match="staged plugin .* SHA-256 mismatch"
    ):
        RUNNER.run(
            _main_args(worker, manifest, output, tmp_path)
        )

    assert not launched
    assert not output.exists()


def test_late_output_symlink_is_replaced_without_touching_target(
    tmp_path, monkeypatch
):
    worker, plugin, _, manifest = _fixture(tmp_path)
    report_path = tmp_path / "report.json"
    plugin_before = plugin.read_bytes()

    def fake_run(command, timeout_seconds):
        Image.new("RGBA", (1, 1), (1, 2, 3, 255)).save(command[4], "PNG")
        report_path.symlink_to(plugin)
        return (
            subprocess.CompletedProcess(
                command,
                0,
                b'{"execution_traces":[{"selector":"RENDER"}]}',
                b"",
            ),
            False,
            None,
        )

    monkeypatch.setattr(RUNNER, "_run_bounded_process", fake_run)
    report = RUNNER.run(
        argparse.Namespace(
            manifest=manifest,
            worker=worker,
            expected_worker_sha256=_sha(worker),
            output=report_path,
            timeout=10,
            run_parent=tmp_path,
        )
    )

    assert plugin.read_bytes() == plugin_before
    assert not report_path.is_symlink()
    assert json.loads(report_path.read_text(encoding="utf-8")) == report
