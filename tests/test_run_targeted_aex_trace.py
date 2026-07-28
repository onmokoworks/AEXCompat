import argparse
import hashlib
import importlib.util
import json
import subprocess
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

    def fake_run(command, **kwargs):
        Image.new("RGBA", (1, 1), (1, 2, 3, 255)).save(command[4], "PNG")
        worker_report = {
            "execution_traces": [{"selector": "RENDER", "note": str(plugin)}],
            "classification": "ok",
        }
        return subprocess.CompletedProcess(
            command, 0, json.dumps(worker_report).encode(), b""
        )

    monkeypatch.setattr(RUNNER.subprocess, "run", fake_run)
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
    worker, plugin, _, manifest = _fixture(tmp_path)
    output = tmp_path / "report.json"
    snapshot = {
        "reason": "UC_ERR_FETCH_PROT",
        "registers": {"rip": 0},
        "module": str(plugin),
    }

    def fake_run(command, **kwargs):
        stderr = (
            f"aex_guest_error: guest execution failed at {plugin}; "
            f"crash_snapshot={json.dumps(snapshot)}\n"
        ).encode()
        return subprocess.CompletedProcess(
            command,
            1,
            json.dumps(
                {
                    "setup": {"execution_backend": "unicorn-x86_64"},
                    "output_png": str(tmp_path / "must-not-leak.png"),
                }
            ).encode(),
            stderr,
        )

    monkeypatch.setattr(RUNNER.subprocess, "run", fake_run)
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
    assert (
        result["partial_report"]["output_png"]
        == "<absolute-path>"
    )
    assert str(tmp_path) not in output.read_text()


def test_strict_json_rejects_duplicate_keys():
    with pytest.raises(RUNNER.TraceRunnerError, match="duplicate JSON key"):
        RUNNER.strict_json_bytes(b'{"a":1,"a":2}', "test")


def test_success_requires_a_trace_payload(tmp_path, monkeypatch):
    worker, _, _, manifest = _fixture(tmp_path)
    monkeypatch.setattr(
        RUNNER.subprocess,
        "run",
        lambda command, **kwargs: subprocess.CompletedProcess(
            command, 0, b'{"classification":"ok"}', b""
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

    def fake_run(command, **kwargs):
        output = Path(command[4])
        if artifact == "invalid":
            output.write_bytes(b"not a png")
        elif artifact == "directory":
            output.mkdir()
        return subprocess.CompletedProcess(
            command,
            0,
            b'{"execution_traces":[{"selector":"RENDER"}]}',
            b"",
        )

    monkeypatch.setattr(RUNNER.subprocess, "run", fake_run)
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
