import importlib.util
import json
import subprocess
import sys
import threading
import time
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


def test_map_corpus_orders_deterministically_by_sha(tmp_path):
    corpus = tmp_path / "corpus"
    corpus.mkdir()
    first_by_name = corpus / "a.aex"
    last_by_name = corpus / "z.aex"
    first_by_name.write_bytes(b"payload-z")
    last_by_name.write_bytes(b"payload-a")
    entries = [
        {"sha256": SWEEP.sha256_file(path), "architecture": "x64"}
        for path in (first_by_name, last_by_name)
    ]

    mapped = SWEEP.map_corpus(
        {"schema_version": 1, "entries": entries}, [corpus]
    )

    assert [item["sha256"] for item in mapped] == sorted(
        item["sha256"] for item in mapped
    )


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


def test_validate_ready_accepts_exact_optional_custom_ui_contract():
    setup = _setup_message()
    setup["custom_ui"] = {
        "events": 4,
        "comp_width": 640,
        "comp_height": 360,
        "comp_alignment": 0,
        "layer_width": 320,
        "layer_height": 180,
        "layer_alignment": 0,
        "preview_width": 160,
        "preview_height": 90,
        "preview_alignment": 0,
    }
    message = {
        "v": 1,
        "type": "session_ready",
        "worker_pid": 42,
        "setup": setup,
    }

    SWEEP.validate_ready(message, 42)
    setup["custom_ui"]["events"] = True
    with pytest.raises(SWEEP.SweepError, match="invalid session custom_ui"):
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


def test_validate_close_rejects_global_setdown_diagnostic_on_claimed_clean_close():
    message = _close_message()
    message["close"]["global_setdown_diagnostic"] = {
        "message": "cleanup failed"
    }

    with pytest.raises(SWEEP.SweepError, match="cleanup was not clean"):
        SWEEP.validate_close(message, 42, 1)


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
            "render_path": "smartfx",
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


def test_default_mode_is_unicorn_only_and_does_not_require_native_worker(tmp_path):
    unicorn = tmp_path / "unicorn-worker"

    args = SWEEP.parse_args(
        _required_cli_args(tmp_path) + ["--unicorn-worker", str(unicorn)]
    )

    assert args.backend is None
    assert SWEEP.requested_backends(args) == ["unicorn"]
    assert args.native_worker is None


def test_default_jobs_is_six_and_cli_accepts_explicit_parallelism(tmp_path):
    unicorn = tmp_path / "unicorn-worker"

    default = SWEEP.parse_args(
        _required_cli_args(tmp_path) + ["--unicorn-worker", str(unicorn)]
    )
    parallel = SWEEP.parse_args(
        _required_cli_args(tmp_path)
        + ["--unicorn-worker", str(unicorn), "--jobs", "12"]
    )

    assert default.jobs == 6
    assert parallel.jobs == 12


def test_sweep_runs_isolated_backends_concurrently_and_keeps_sha_order(
    tmp_path, monkeypatch
):
    inventory = tmp_path / "inventory.json"
    summary = tmp_path / "summary.json"
    corpus = tmp_path / "corpus"
    input_png = tmp_path / "input.png"
    worker = tmp_path / "worker"
    output = tmp_path / "report.json"
    corpus.mkdir()
    inventory.write_text('{"schema_version":1,"entries":[{}]}', encoding="utf-8")
    inventory_sha = SWEEP.sha256_file(inventory)
    summary.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "corpus": {
                    "inventory_sha256": inventory_sha,
                    "canonical_count": 1,
                    "processed": 1,
                    "remaining": 0,
                    "ordered_path_sha_identity_exact": True,
                },
            }
        ),
        encoding="utf-8",
    )
    SWEEP.Image.new("RGBA", (1, 1)).save(input_png)
    worker.write_bytes(b"worker")
    mapped = []
    for index in range(6):
        plugin = corpus / f"{index}.aex"
        plugin.write_bytes(bytes([index]))
        mapped.append(
            {
                "path": plugin,
                "name": plugin.name,
                "sha256": f"{index + 1:064x}",
                "windows_match_count": 1,
                "windows_root_categories": [],
                "windows_source_categories": [],
            }
        )
    monkeypatch.setattr(SWEEP, "map_corpus", lambda *_: mapped)
    active = 0
    maximum_active = 0
    lock = threading.Lock()

    def fake_run_backend(*_args):
        nonlocal active, maximum_active
        redactions = _args[-1]
        with lock:
            active += 1
            maximum_active = max(maximum_active, active)
        time.sleep(0.03)
        with lock:
            active -= 1
        return {
            "status": "rendered",
            "output_sha256": "a" * 64,
            "worker_stderr": SWEEP.sanitize_error_text(
                f"warning for {corpus}/private.aex", redactions
            ),
            "milestones": {
                "admission_success": True,
                "render_success": True,
                "cleanup_success": True,
            },
        }

    monkeypatch.setattr(SWEEP, "run_backend", fake_run_backend)
    args = Namespace(
        inventory=inventory,
        windows_summary=summary,
        corpus_root=[corpus],
        input_png=input_png,
        native_worker=None,
        unicorn_worker=worker,
        backend=None,
        output=output,
        baseline_report=None,
        expected_inventory_sha256=inventory_sha,
        expected_summary_sha256=SWEEP.sha256_file(summary),
        native_run_dllmain=False,
        jobs=3,
    )

    report = SWEEP.run_sweep(args)

    assert maximum_active == 3
    assert report["source"]["jobs"] == 3
    assert report["source"]["execution_model"] == SWEEP.EXECUTION_MODEL
    assert [entry["sha256"] for entry in report["entries"]] == [
        item["sha256"] for item in mapped
    ]
    assert report["summary"]["counts"] == {"unicorn:rendered": 6}
    assert report["entries"][0]["backends"]["unicorn"]["worker_stderr"] == (
        "warning for <corpus-root:0>/private.aex"
    )


def test_explicit_native_mode_still_requires_native_worker(tmp_path, capsys):
    with pytest.raises(SystemExit) as captured:
        SWEEP.parse_args(_required_cli_args(tmp_path) + ["--backend", "native"])

    assert captured.value.code == 2
    assert "--native-worker is required for native backend" in capsys.readouterr().err


def test_cli_rejects_duplicate_backend_selection(tmp_path, capsys):
    with pytest.raises(SystemExit) as captured:
        SWEEP.parse_args(
            _required_cli_args(tmp_path)
            + [
                "--backend",
                "unicorn",
                "--backend",
                "unicorn",
                "--unicorn-worker",
                str(tmp_path / "unicorn-worker"),
            ]
        )

    assert captured.value.code == 2
    assert "must not be duplicated" in capsys.readouterr().err


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


@pytest.mark.skipif(sys.platform != "darwin", reason="POSIX process-group contract is macOS-only")
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


def test_close_worker_preserves_bounded_stderr_when_structured_close_is_clean(
    monkeypatch,
):
    process = Namespace(
        pid=42,
        stdin=Namespace(close=lambda: None),
        stdout=object(),
        wait=lambda timeout: 0,
    )
    response = _close_message()
    monkeypatch.setattr(SWEEP, "write_message", lambda *_: None)
    monkeypatch.setattr(SWEEP, "read_message", lambda *_: response)
    monkeypatch.setattr(
        SWEEP,
        "read_stderr_bounded",
        lambda _: f"warning from {Path.home()}/private/plugin.aex",
    )

    result = SWEEP.close_worker(process, 1)

    assert result["worker_stderr"] == "warning from <home>/private/plugin.aex"


def test_close_worker_redacts_external_path_before_durable_truncation(monkeypatch):
    process = Namespace(
        pid=42,
        stdin=Namespace(close=lambda: None),
        stdout=object(),
        wait=lambda timeout: 0,
    )
    response = _close_message()
    private_root = "/Volumes/AEX Corpus"
    monkeypatch.setattr(SWEEP, "write_message", lambda *_: None)
    monkeypatch.setattr(SWEEP, "read_message", lambda *_: response)
    monkeypatch.setattr(
        SWEEP,
        "read_stderr_bounded",
        lambda _: "x" * 1000 + f" {private_root}/private/plugin.aex",
    )

    result = SWEEP.close_worker(process, 1, {private_root: "<corpus-root:0>"})

    assert "/Volumes" not in result["worker_stderr"]
    assert len(result["worker_stderr"].encode("utf-8")) <= SWEEP.MAX_DURABLE_ERROR_BYTES


def test_run_backend_reuses_one_isolated_worker_for_probe_and_render(tmp_path, monkeypatch):
    process = Namespace(pid=42, stdin=object(), stdout=object())
    ready = {
        "worker_pid": 42,
        "setup": {"execution_backend": "unicorn-x86_64"},
    }
    launches = []
    closed = []
    responses = [{"probe": True}, {"frame": True}]

    def fake_launch(*args):
        launches.append(args)
        return process, ready

    monkeypatch.setattr(SWEEP, "launch_ready", fake_launch)
    monkeypatch.setattr(SWEEP, "write_message", lambda *_: None)
    monkeypatch.setattr(SWEEP, "read_message", lambda *_: responses.pop(0))
    monkeypatch.setattr(SWEEP, "validate_probe", lambda *_: None)
    monkeypatch.setattr(SWEEP, "validate_frame", lambda *_: None)

    def fake_close(candidate, expected_frames, redactions):
        closed.append((candidate, expected_frames, redactions))
        return {
            "close": {
                "suite_requests": [],
                "unsupported_suite_calls": [],
            },
            "worker_stderr": "bounded warning",
        }

    monkeypatch.setattr(SWEEP, "close_worker", fake_close)
    plugin = tmp_path / "plugin.aex"
    plugin.write_bytes(b"fixture")
    run_directory = tmp_path / "run"
    run_directory.mkdir()

    result = SWEEP.run_backend(
        tmp_path / "worker",
        "unicorn-x86_64",
        {},
        plugin,
        b"\x00\x01\x02\x03",
        1,
        1,
        run_directory,
    )

    assert len(launches) == 1
    assert closed == [(process, 1, None)]
    assert result["fresh_after_probe"] is False
    assert result["worker_stderr"] == "bounded warning"
    assert result["milestones"] == {
        "admission_success": True,
        "render_success": True,
        "cleanup_success": True,
    }


def test_cleanup_failure_is_not_misclassified_as_suite_gap():
    message = "resident cleanup was not clean; suite_requests=['AEGP Compute Cache v1']"

    assert SWEEP.classify_failure(message) == "cleanup"


def test_signal_evidence_is_classified_as_crash():
    assert SWEEP.classify_failure("signal=SIGSEGV(11)") == "crash"


@pytest.mark.parametrize(
    ("message", "expected"),
    [
        ("unsupported Win64 import: ucrtbase.dll!ceilf", "import"),
        ("native AVX state sync point capacity exceeded", "emulation"),
    ],
)
def test_pre_ready_guest_errors_keep_actionable_failure_class(message, expected):
    assert SWEEP.classify_failure(message) == expected


@pytest.mark.skipif(sys.platform != "darwin", reason="POSIX process-group contract is macOS-only")
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


def _metric_entry(identity, result):
    return {"sha256": identity, "name": f"{identity}.aex", "backends": {"unicorn": result}}


def _rendered(checksum="a" * 64):
    return {
        "status": "rendered",
        "output_sha256": checksum,
        "milestones": {
            "admission_success": True,
            "render_success": True,
            "cleanup_success": True,
        },
    }


def _failed(failure_class="Suite", selector="SMART_RENDER", admission=True):
    return {
        "status": "failed",
        "failure_class": failure_class,
        "diagnostic": {"selector": selector},
        "milestones": {
            "admission_success": admission,
            "render_success": False,
            "cleanup_success": False,
        },
    }


def test_compatibility_summary_uses_fixed_entry_denominator_and_exact_rates():
    entries = [
        _metric_entry("1" * 64, _rendered()),
        _metric_entry("2" * 64, _failed()),
        _metric_entry("3" * 64, _failed("import", "GLOBAL_SETUP", False)),
    ]

    summary = SWEEP.summarize_compatibility(entries, ["unicorn"])

    assert summary["denominator"] == 3
    assert summary["by_backend"]["unicorn"] == {
        "denominator": 3,
        "admission_success": 2,
        "admission_rate": {"numerator": 2, "denominator": 3},
        "render_success": 1,
        "render_rate": {"numerator": 1, "denominator": 3},
        "cleanup_success": 1,
        "cleanup_rate": {"numerator": 1, "denominator": 3},
    }
    assert summary["blockers"] == [
        {
            "backend": "unicorn",
            "failure_class": "Suite",
            "selector": "SMART_RENDER",
            "count": 1,
        },
        {
            "backend": "unicorn",
            "failure_class": "import",
            "selector": "GLOBAL_SETUP",
            "count": 1,
        },
    ]


def _report(entries, worker_sha="f" * 64):
    return {
        "schema_version": SWEEP.SCHEMA_VERSION,
        "source": {
            "windows_inventory_sha256": "a" * 64,
            "windows_summary_sha256": "b" * 64,
            "input_png_sha256": "c" * 64,
            "input_dimensions": [64, 64],
            "backends": ["unicorn"],
            "jobs": 4,
            "execution_model": SWEEP.EXECUTION_MODEL,
            "unicorn_worker_sha256": worker_sha,
        },
        "entries": entries,
    }


def test_baseline_comparison_reports_gain_without_regression():
    identity = "1" * 64
    baseline = _report([_metric_entry(identity, _failed())])
    current = _report([_metric_entry(identity, _rendered())])

    comparison = SWEEP.compare_baseline(current, baseline)

    assert comparison["regression"] is False
    assert comparison["by_backend"]["unicorn"]["gained_render"] == [identity]


def test_baseline_comparison_marks_render_loss_and_output_change_as_regression():
    lost_identity = "1" * 64
    changed_identity = "2" * 64
    baseline = _report(
        [
            _metric_entry(lost_identity, _rendered("a" * 64)),
            _metric_entry(changed_identity, _rendered("b" * 64)),
        ]
    )
    current = _report(
        [
            _metric_entry(lost_identity, _failed()),
            _metric_entry(changed_identity, _rendered("c" * 64)),
        ]
    )

    comparison = SWEEP.compare_baseline(current, baseline)

    assert comparison["regression"] is True
    assert comparison["by_backend"]["unicorn"]["lost_render"] == [lost_identity]
    assert comparison["by_backend"]["unicorn"]["output_changed"] == [changed_identity]


@pytest.mark.parametrize("difference", ["input", "identity"])
def test_baseline_comparison_fails_closed_on_condition_or_identity_drift(difference):
    identity = "1" * 64
    baseline = _report([_metric_entry(identity, _rendered())])
    current = _report([_metric_entry(identity, _rendered())])
    if difference == "input":
        current["source"]["input_dimensions"] = [32, 32]
    else:
        current["entries"][0]["sha256"] = "2" * 64

    with pytest.raises(SWEEP.SweepError, match="conditions differ|identity order differs"):
        SWEEP.compare_baseline(current, baseline)


def test_baseline_comparison_allows_worker_change_and_records_both_identities():
    identity = "1" * 64
    baseline = _report([_metric_entry(identity, _failed())], worker_sha="a" * 64)
    current = _report([_metric_entry(identity, _rendered())], worker_sha="b" * 64)

    comparison = SWEEP.compare_baseline(current, baseline)

    assert comparison["baseline_workers"] == {"unicorn_worker_sha256": "a" * 64}
    assert comparison["current_workers"] == {"unicorn_worker_sha256": "b" * 64}
    assert comparison["regression"] is False


def test_baseline_comparison_rejects_parallelism_drift():
    identity = "1" * 64
    baseline = _report([_metric_entry(identity, _rendered())])
    current = _report([_metric_entry(identity, _rendered())])
    current["source"]["jobs"] = 8

    with pytest.raises(SWEEP.SweepError, match="execution conditions differ"):
        SWEEP.compare_baseline(current, baseline)


def test_baseline_comparison_rejects_execution_model_drift():
    identity = "1" * 64
    baseline = _report([_metric_entry(identity, _rendered())])
    current = _report([_metric_entry(identity, _rendered())])
    baseline["source"]["execution_model"] = "fresh-worker-after-probe-v1"

    with pytest.raises(SWEEP.SweepError, match="execution conditions differ"):
        SWEEP.compare_baseline(current, baseline)


def test_baseline_comparison_rejects_missing_execution_model():
    identity = "1" * 64
    baseline = _report([_metric_entry(identity, _rendered())])
    current = _report([_metric_entry(identity, _rendered())])
    del baseline["source"]["execution_model"]
    del current["source"]["execution_model"]

    with pytest.raises(SWEEP.SweepError, match="execution_model is invalid"):
        SWEEP.compare_baseline(current, baseline)


@pytest.mark.parametrize("value", [None, True, 0, 33, "4"])
def test_baseline_comparison_rejects_missing_or_invalid_jobs(value):
    identity = "1" * 64
    baseline = _report([_metric_entry(identity, _rendered())])
    current = _report([_metric_entry(identity, _rendered())])
    if value is None:
        del baseline["source"]["jobs"]
        del current["source"]["jobs"]
    else:
        baseline["source"]["jobs"] = value
        current["source"]["jobs"] = value

    with pytest.raises(SWEEP.SweepError, match="report jobs is invalid"):
        SWEEP.compare_baseline(current, baseline)


def test_baseline_comparison_rejects_missing_worker_identity():
    identity = "1" * 64
    baseline = _report([_metric_entry(identity, _rendered())])
    del baseline["source"]["unicorn_worker_sha256"]
    current = _report([_metric_entry(identity, _rendered())])

    with pytest.raises(SWEEP.SweepError, match="unicorn_worker_sha256 is invalid"):
        SWEEP.compare_baseline(current, baseline)


@pytest.mark.parametrize("malformation", ["identity", "milestone", "output"])
def test_baseline_comparison_rejects_malformed_success_contract(malformation):
    identity = "1" * 64
    baseline = _report([_metric_entry(identity, _rendered())])
    current = _report([_metric_entry(identity, _rendered())])
    result = baseline["entries"][0]["backends"]["unicorn"]
    if malformation == "identity":
        baseline["entries"][0]["sha256"] = "not-a-sha"
    elif malformation == "milestone":
        result["milestones"]["cleanup_success"] = "yes"
    else:
        result["output_sha256"] = "not-a-sha"

    with pytest.raises(
        SWEEP.SweepError, match="identity is invalid|milestones are invalid|output SHA-256 is invalid"
    ):
        SWEEP.compare_baseline(current, baseline)
