import importlib.util
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "run-broker-rust-tests.py"


def load_runner():
    spec = importlib.util.spec_from_file_location("rust_ci_partition", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def metadata(runner, *, extra_broker_target: str | None = None):
    broker_targets = runner.NATIVE_BROKER_TARGETS | runner.INDEPENDENT_BROKER_TARGETS
    if extra_broker_target:
        broker_targets.add(extra_broker_target)
    return {
        "packages": [
            {
                "name": runner.BROKER,
                "targets": [
                    {"name": target, "kind": ["test"]}
                    for target in sorted(broker_targets)
                ],
            },
            {
                "name": runner.HARNESS,
                "targets": [
                    {"name": "artifact_cli", "kind": ["test"]},
                    {"name": "render_fixture_cli", "kind": ["test"]},
                ],
            },
        ]
    }


def test_partitions_are_disjoint_complete_and_fail_closed_on_stale_native_target() -> (
    None
):
    runner = load_runner()
    independent, native = runner.partitions(metadata(runner))
    assert independent[runner.BROKER] == runner.INDEPENDENT_BROKER_TARGETS
    assert native[runner.BROKER] == runner.NATIVE_BROKER_TARGETS
    assert independent[runner.HARNESS] == {"render_fixture_cli"}
    assert native[runner.HARNESS] == {"artifact_cli"}

    with pytest.raises(SystemExit, match="unclassified"):
        runner.partitions(metadata(runner, extra_broker_target="new_target"))

    current_metadata = metadata(runner)
    runner.NATIVE_BROKER_TARGETS = runner.NATIVE_BROKER_TARGETS | {"removed_target"}
    with pytest.raises(SystemExit, match="disappeared"):
        runner.partitions(current_metadata)


def test_only_the_existing_issue_900_visual_audio_skip_is_allowlisted() -> None:
    runner = load_runner()
    assert (
        runner.unexpected_skip_lines(
            "skipping image+audio+layer session test: run "
            "tools/build-pf-visual-audio-probe.ps1 "
            "-Target pf_visual_audio_layer_sidecar_probe first\n"
            "skipping smart timed-multilayer: build the smart worker and the probe\n"
        )
        == []
    )
    regressions = [
        "skipping audio-only session: build pf-visual-audio-probe first",
        "skipping audio-only cluster: build pf-visual-audio-probe first",
        "skipping image+audio: run tools/build-pf-visual-audio-probe.ps1 first",
        "skipping classic session render: build aex_worker.exe first",
    ]
    for regression in regressions:
        assert runner.unexpected_skip_lines(regression) == [regression]


def test_each_nextest_surface_is_planned_once_across_both_partitions() -> None:
    runner = load_runner()
    independent, native = runner.partitions(metadata(runner))
    calls = []

    def record(filterset, **options):
        calls.append((filterset, options))

    runner.nextest_run = record
    runner.run_independent(independent)
    independent_calls = list(calls)
    calls.clear()
    runner.SILENT_SKIP_PREREQUISITES = ()
    runner.run_native(native)
    native_calls = list(calls)

    assert independent_calls == [(runner.independent_filter(independent), {})]
    assert native_calls == [(runner.native_filter(native), {"reject_skip": True})]

    independent_plan = independent_calls[0][0]
    native_plan = native_calls[0][0]
    for target in (
        runner.INDEPENDENT_BROKER_TARGETS | runner.INDEPENDENT_HARNESS_TARGETS
    ):
        assert f"binary(={target})" in independent_plan
        assert f"binary(={target})" not in native_plan
    for target in runner.NATIVE_BROKER_TARGETS | runner.NATIVE_HARNESS_TARGETS:
        assert f"binary(={target})" in native_plan
        assert f"binary(={target})" not in independent_plan
    assert f"test(={runner.NATIVE_LIB_TEST})" in independent_plan
    assert f"test(={runner.NATIVE_LIB_TEST})" in native_plan


def test_nextest_command_uses_only_the_prebuilt_archive(tmp_path, monkeypatch) -> None:
    runner = load_runner()
    archive = tmp_path / "tests.tar.zst"
    archive.write_bytes(b"archive")
    runner.NEXTEST_ARCHIVE = archive
    calls = []

    class Result:
        returncode = 0
        stdout = "ok"
        stderr = ""

    monkeypatch.setattr(
        runner.subprocess,
        "run",
        lambda *args, **kwargs: calls.append((args, kwargs)) or Result(),
    )
    runner.nextest_run("package(=example)", reject_skip=True)

    command = calls[0][0][0]
    assert command[:3] == ["cargo", "nextest", "run"]
    assert "--archive-file" in command
    assert "--workspace-remap" in command
    assert "--success-output" in command
    assert "immediate" in command
    assert "test" not in command[:3]


def test_archive_partition_validation_rejects_overlap_and_missing_tests() -> None:
    runner = load_runner()
    independent, native = runner.partitions(metadata(runner))
    all_tests = {("suite", "independent"), ("suite", "native")}
    selections = iter(
        (
            all_tests,
            {("suite", "independent")},
            {("suite", "native")},
        )
    )
    runner.listed_tests = lambda _: next(selections)
    runner.validate_archive_partitions(independent, native)

    invalid_selections = (
        # Overlap without a missing test.
        (all_tests, all_tests, {("suite", "native")}),
        # Missing without overlap.
        (all_tests, {("suite", "independent")}, set()),
        # A selected test not present in the archive inventory.
        (
            all_tests,
            {("suite", "independent"), ("suite", "unexpected")},
            {("suite", "native")},
        ),
    )
    for invalid in invalid_selections:
        selections = iter(invalid)
        runner.listed_tests = lambda _: next(selections)
        with pytest.raises(SystemExit, match="not complete and disjoint"):
            runner.validate_archive_partitions(independent, native)
