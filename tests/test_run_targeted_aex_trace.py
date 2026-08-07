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
    return False


def _wait_for_process_exit(pid: int, timeout_seconds: float = 5) -> bool:
    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        if _process_has_exited(pid):
            return True
        time.sleep(0.05)
    return _process_has_exited(pid)


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


def test_failure_extracts_structured_crash_and_redacts_paths(tmp_path, monkeypatch):
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
    assert result["partial_report"]["output_png"].startswith("<home>")
    assert str(tmp_path) not in output.read_text()


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
    child_pid_path = tmp_path / "descendant.pid"
    child_code = (
        "import time\n"
        "time.sleep(30)\n"
    )
    parent_code = (
        "import pathlib,subprocess,sys,time\n"
        f"child=subprocess.Popen([sys.executable, '-c', {child_code!r}])\n"
        f"pathlib.Path({str(child_pid_path)!r}).write_text(str(child.pid))\n"
        "time.sleep(30)\n"
    )
    completed, timed_out, reason = RUNNER._run_bounded_process(
        [sys.executable, "-c", parent_code], 2
    )
    assert timed_out
    assert reason == "timeout"
    assert completed.returncode is not None
    assert child_pid_path.exists(), "parent did not report the descendant PID"
    child_pid = int(child_pid_path.read_text())
    assert _wait_for_process_exit(child_pid), f"descendant PID {child_pid} survived"


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
