import importlib.util
import json
import sys
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "sweep_macos_x64_aex", ROOT / "tools" / "sweep_macos_x64_aex.py"
)
assert SPEC and SPEC.loader
SWEEP = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SWEEP)


def test_load_json_strict_rejects_duplicate_keys(tmp_path):
    source = tmp_path / "duplicate.json"
    source.write_text('{"schema_version":1,"schema_version":1}', encoding="utf-8")

    with pytest.raises(SWEEP.SweepError, match="duplicate JSON key"):
        SWEEP.load_json_strict(source)


def test_map_corpus_uses_sha_not_basename(tmp_path):
    corpus = tmp_path / "corpus"
    corpus.mkdir()
    plugin = corpus / "renamed.aex"
    plugin.write_bytes(b"same binary")
    digest = SWEEP.sha256_file(plugin)
    inventory = {
        "schema_version": 1,
        "entries": [
            {
                "path": "C:/different/original.aex",
                "sha256": digest,
                "architecture": "x64",
                "root_category": "after-effects",
                "source_category": "installed",
            }
        ],
    }

    mapped = SWEEP.map_corpus(inventory, [corpus])

    assert mapped[0]["name"] == "renamed.aex"
    assert mapped[0]["sha256"] == digest
    assert mapped[0]["windows_match_count"] == 1


def test_map_corpus_fails_closed_for_unmapped_binary(tmp_path):
    corpus = tmp_path / "corpus"
    corpus.mkdir()
    (corpus / "unknown.aex").write_bytes(b"unknown")
    inventory = {
        "schema_version": 1,
        "entries": [
            {
                "sha256": "0" * 64,
                "architecture": "x64",
            }
        ],
    }

    with pytest.raises(SWEEP.SweepError, match="absent from Windows inventory"):
        SWEEP.map_corpus(inventory, [corpus])


def _close_message():
    return {
        "v": 1,
        "type": "session_closed",
        "worker_pid": 42,
        "setup": {},
        "close": {
            "schema_version": 1,
            "execution_backend": "native-x64",
            "frames_rendered": 1,
            "frame_setdown_error": 0,
            "sequence_setdown_error": 0,
            "global_setdown_error": 0,
            "suite_requests": [],
            "unsupported_suite_calls": [],
            "dropped_unsupported_suite_calls": 0,
            "session_clean": True,
        },
    }


def _setup_message():
    return {
        "schema_version": 1,
        "execution_backend": "unicorn-x86_64",
        "global_setup_error": 0,
        "params_setup_error": 0,
        "advertised_num_params": 1,
        "out_flags": 0,
        "out_flags2": 0,
        "parameters": [],
        "suite_requests": [],
        "unsupported_suite_calls": [],
        "dropped_unsupported_suite_calls": 0,
    }


def test_validate_ready_rejects_nonzero_setup_error():
    setup = _setup_message()
    setup["global_setup_error"] = -1
    message = {
        "v": 1,
        "type": "session_ready",
        "worker_pid": 42,
        "setup": setup,
    }

    with pytest.raises(SWEEP.SweepError, match="invalid session setup"):
        SWEEP.validate_ready(message, 42)


def test_validate_close_accepts_complete_clean_contract():
    SWEEP.validate_close(_close_message(), 42, 1)


def test_validate_close_rejects_unknown_nested_field():
    message = _close_message()
    message["close"]["unexpected"] = True

    with pytest.raises(SWEEP.SweepError, match="session close report keys differ"):
        SWEEP.validate_close(message, 42, 1)


def test_validate_frame_binds_report_to_output_checksum():
    output = b"\xff\x01\x02\x03"
    checksum = SWEEP.hashlib.sha256(output).hexdigest()
    message = {
        "v": 1,
        "type": "frame_done",
        "frame_index": 0,
        "status": "ok",
        "output": {
            "width": 1,
            "height": 1,
            "rowbytes": 4,
            "pixel_format": "argb8",
            "checksum": checksum,
            "guards_intact": True,
        },
        "render_error": 0,
        "generation": 1,
    }

    SWEEP.validate_frame(message, 1, 1, checksum)
    with pytest.raises(SWEEP.SweepError, match="frame invariants failed"):
        SWEEP.validate_frame(message, 1, 1, "0" * 64)


def test_report_json_has_no_duplicate_keys(tmp_path):
    report = tmp_path / "report.json"
    report.write_text(json.dumps({"schema_version": 1}), encoding="utf-8")

    assert SWEEP.load_json_strict(report)["schema_version"] == 1


def test_spawn_worker_uses_an_isolated_process_group(tmp_path):
    process = SWEEP.spawn_worker(
        Path(sys.executable),
        tmp_path / "plugin.aex",
        tmp_path / "input.argb8",
        tmp_path / "output.argb8",
        1,
        1,
    )
    try:
        assert SWEEP.os.getpgid(process.pid) == process.pid
    finally:
        SWEEP.terminate_worker(process)


def test_cleanup_failure_is_not_misclassified_as_suite_gap():
    message = "resident cleanup was not clean; suite_requests=['AEGP Compute Cache v1']"

    assert SWEEP.classify_failure(message) == "cleanup"
