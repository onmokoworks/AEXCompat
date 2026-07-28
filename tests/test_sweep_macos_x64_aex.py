import importlib.util
import json
import subprocess
import sys
from argparse import Namespace
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


def test_require_backend_rejects_swapped_worker_label():
    ready = {"setup": _setup_message()}

    with pytest.raises(SWEEP.SweepError, match="worker backend differs"):
        SWEEP.require_backend(ready, "native-x86_64-carrier", "native")


def test_source_pair_requires_summary_to_bind_exact_inventory_sha():
    inventory = {"entries": [{}, {}]}
    summary = {
        "schema_version": 1,
        "corpus": {
            "inventory_sha256": "a" * 64,
            "canonical_count": 2,
            "processed": 2,
            "remaining": 0,
            "ordered_path_sha_identity_exact": True,
        },
    }

    SWEEP.validate_source_pair(inventory, summary, "a" * 64)
    with pytest.raises(SWEEP.SweepError, match="does not bind"):
        SWEEP.validate_source_pair(inventory, summary, "b" * 64)


def _failure_diagnostic(category="callback", unsupported=None):
    return {
        "schema_version": 1,
        "stage": "admission_probe",
        "execution_backend": "unicorn-x86_64",
        "category": category,
        "selector": "RENDER",
        "error_code": None,
        "message": "bounded failure",
        "crash_reason": None,
        "suite_requests": [],
        "dropped_suite_requests": 0,
        "unsupported_suite_calls": unsupported or [],
        "dropped_unsupported_suite_calls": 0,
    }


def test_validate_probe_raises_typed_structured_admission_failure():
    message = {
        "v": 1,
        "type": "session_probed",
        "worker_pid": 42,
        "status": "error",
        "guards_intact": False,
        "render_error": -40,
        "failure": _failure_diagnostic(),
    }

    with pytest.raises(SWEEP.AdmissionFailure) as captured:
        SWEEP.validate_probe(message, 42)

    assert captured.value.diagnostic["category"] == "callback"
    assert SWEEP.classify_diagnostic(captured.value.diagnostic) == "callback"


def test_structured_unsupported_suite_call_takes_classification_priority():
    diagnostic = _failure_diagnostic(
        "callback",
        [
            {
                "name": "PF Iterate8 Suite",
                "version": 1,
                "slot": 2,
                "call_count": 1,
            }
        ],
    )

    assert SWEEP.classify_diagnostic(diagnostic) == "Suite"


def test_failure_diagnostic_rejects_unbounded_message():
    diagnostic = _failure_diagnostic()
    diagnostic["message"] = "x" * 1025

    with pytest.raises(SWEEP.SweepError, match="invalid admission failure"):
        SWEEP.validate_failure_diagnostic(diagnostic)


@pytest.mark.parametrize("field", ["suite_requests", "unsupported_suite_calls"])
def test_failure_diagnostic_rejects_more_than_64_exemplars(field):
    diagnostic = _failure_diagnostic()
    if field == "suite_requests":
        diagnostic[field] = [f"suite-{index}" for index in range(65)]
    else:
        diagnostic[field] = [
            {
                "name": "PF Iterate8 Suite",
                "version": 1,
                "slot": index,
                "call_count": 1,
            }
            for index in range(65)
        ]

    with pytest.raises(SWEEP.SweepError, match="bound|invalid admission failure"):
        SWEEP.validate_failure_diagnostic(diagnostic)


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("schema_version", True),
        ("error_code", True),
        ("dropped_suite_requests", True),
        ("dropped_unsupported_suite_calls", True),
    ],
)
def test_failure_diagnostic_rejects_boolean_integer_fields(field, value):
    diagnostic = _failure_diagnostic()
    diagnostic[field] = value

    with pytest.raises(SWEEP.SweepError, match="invalid admission failure"):
        SWEEP.validate_failure_diagnostic(diagnostic)


def test_failure_diagnostic_rejects_unbounded_suite_request():
    diagnostic = _failure_diagnostic()
    diagnostic["suite_requests"] = ["あ" * 86]

    with pytest.raises(SWEEP.SweepError, match="invalid admission failure"):
        SWEEP.validate_failure_diagnostic(diagnostic)


def test_failure_diagnostic_rejects_boolean_suite_call_integer():
    diagnostic = _failure_diagnostic(
        unsupported=[
            {
                "name": "PF Iterate8 Suite",
                "version": True,
                "slot": 2,
                "call_count": 1,
            }
        ]
    )

    with pytest.raises(SWEEP.SweepError, match="invalid unsupported suite call"):
        SWEEP.validate_failure_diagnostic(diagnostic)


def test_durable_error_sanitizes_home_and_omits_crash_snapshot():
    message = (
        f"failed at {Path.home()}/private/plugin.aex; "
        'crash_snapshot={"registers":{"rax":42}}'
    )

    sanitized = SWEEP.sanitize_error_text(message)

    assert str(Path.home()) not in sanitized
    assert "<home>/private/plugin.aex" in sanitized
    assert "rax" not in sanitized
    assert sanitized.endswith("crash_snapshot=<omitted>")
    assert len(sanitized.encode("utf-8")) <= SWEEP.MAX_DURABLE_ERROR_BYTES


def test_durable_error_sanitizes_known_paths_outside_home():
    message = (
        "failed plugin /Volumes/AEX Corpus/OLMBlur.aex; "
        "output /private/tmp/aex-sweep-runs/0001/output.argb8"
    )

    sanitized = SWEEP.sanitize_error_text(
        message,
        {
            "/Volumes/AEX Corpus": "<corpus-root:0>",
            "/private/tmp/aex-sweep-runs": "<run-root>",
        },
    )

    assert "/Volumes/AEX Corpus" not in sanitized
    assert "/private/tmp/aex-sweep-runs" not in sanitized
    assert "<corpus-root:0>/OLMBlur.aex" in sanitized
    assert "<run-root>/0001/output.argb8" in sanitized


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


def _required_cli_args(tmp_path):
    return [
        "--inventory",
        str(tmp_path / "inventory.json"),
        "--windows-summary",
        str(tmp_path / "summary.json"),
        "--corpus-root",
        str(tmp_path / "corpus"),
        "--input-png",
        str(tmp_path / "input.png"),
        "--output",
        str(tmp_path / "report.json"),
        "--expected-inventory-sha256",
        "0" * 64,
        "--expected-summary-sha256",
        "1" * 64,
    ]


def test_unicorn_only_cli_does_not_require_native_worker(tmp_path):
    unicorn = tmp_path / "unicorn-worker"

    args = SWEEP.parse_args(
        _required_cli_args(tmp_path)
        + ["--backend", "unicorn", "--unicorn-worker", str(unicorn)]
    )

    assert args.backend == ["unicorn"]
    assert args.unicorn_worker == unicorn
    assert args.native_worker is None


def test_default_both_mode_rejects_missing_native_worker(tmp_path, capsys):
    with pytest.raises(SystemExit) as captured:
        SWEEP.parse_args(
            _required_cli_args(tmp_path)
            + ["--unicorn-worker", str(tmp_path / "unicorn-worker")]
        )

    assert captured.value.code == 2
    assert "--native-worker is required for native backend" in capsys.readouterr().err


def test_unicorn_only_resolves_and_hashes_only_selected_worker(tmp_path, monkeypatch):
    unicorn = tmp_path / "unicorn-worker"
    unicorn.write_bytes(b"unicorn")
    missing_native = tmp_path / "missing-native-worker"
    args = Namespace(
        backend=["unicorn"],
        native_worker=missing_native,
        unicorn_worker=unicorn,
        native_run_dllmain=False,
    )
    hashed = []
    original_sha256_file = SWEEP.sha256_file

    def record_sha256(path):
        hashed.append(path)
        return original_sha256_file(path)

    workers = SWEEP.resolve_workers(args)
    monkeypatch.setattr(SWEEP, "sha256_file", record_sha256)
    identity = SWEEP.source_worker_identity(workers, args.native_run_dllmain)

    assert set(workers) == {"unicorn"}
    assert identity == {"unicorn_worker_sha256": original_sha256_file(unicorn)}
    assert hashed == [unicorn.resolve()]


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


def test_spawn_worker_applies_explicit_environment_without_mutating_parent(
    tmp_path, monkeypatch
):
    captured = {}
    sentinel = object()

    def fake_popen(*arguments, **options):
        captured["arguments"] = arguments
        captured["options"] = options
        return sentinel

    monkeypatch.setenv("AEXCOMPAT_NATIVE_RUN_DLLMAIN", "ambient")
    monkeypatch.setattr(SWEEP.subprocess, "Popen", fake_popen)

    without_opt_in = SWEEP.spawn_worker(
        Path("/worker"),
        tmp_path / "plugin.aex",
        tmp_path / "input.argb8",
        tmp_path / "output.argb8",
        1,
        1,
    )
    assert without_opt_in is sentinel
    assert "AEXCOMPAT_NATIVE_RUN_DLLMAIN" not in captured["options"]["env"]

    process = SWEEP.spawn_worker(
        Path("/worker"),
        tmp_path / "plugin.aex",
        tmp_path / "input.argb8",
        tmp_path / "output.argb8",
        1,
        1,
        {"AEXCOMPAT_NATIVE_RUN_DLLMAIN": "1"},
    )

    assert process is sentinel
    assert captured["options"]["env"]["AEXCOMPAT_NATIVE_RUN_DLLMAIN"] == "1"
    assert SWEEP.os.environ["AEXCOMPAT_NATIVE_RUN_DLLMAIN"] == "ambient"


def test_cleanup_failure_is_not_misclassified_as_suite_gap():
    message = "resident cleanup was not clean; suite_requests=['AEGP Compute Cache v1']"

    assert SWEEP.classify_failure(message) == "cleanup"


def test_signal_evidence_is_classified_as_crash():
    assert SWEEP.classify_failure("signal=SIGSEGV(11)") == "crash"


def test_runner_initiated_termination_is_not_classified_as_guest_crash():
    process = subprocess.Popen(
        [sys.executable, "-c", "import time; time.sleep(30)"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )

    evidence = SWEEP.terminate_worker(process)

    assert "terminated_by_runner=SIGTERM" in evidence
    assert SWEEP.classify_failure(evidence) != "crash"
