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


def test_each_cargo_test_surface_is_planned_once_across_both_partitions() -> None:
    runner = load_runner()
    independent, native = runner.partitions(metadata(runner))
    calls = []

    def record(*arguments, **options):
        calls.append((arguments, options))

    runner.cargo_test = record
    runner.run_independent(independent)
    independent_calls = list(calls)
    calls.clear()
    runner.SILENT_SKIP_PREREQUISITES = ()
    runner.run_native(native)
    native_calls = list(calls)

    assert independent_calls[:4] == [
        (
            ("--workspace", "--exclude", runner.BROKER, "--exclude", runner.HARNESS),
            {},
        ),
        (
            (
                "-p",
                runner.BROKER,
                "--lib",
                "--",
                "--skip",
                runner.NATIVE_LIB_TEST,
            ),
            {},
        ),
        (("-p", runner.BROKER, "--doc"), {}),
        (("-p", runner.BROKER, "--bins"), {}),
    ]
    assert independent_calls[4] == (
        ("-p", runner.BROKER)
        + tuple(
            argument
            for target in sorted(runner.INDEPENDENT_BROKER_TARGETS)
            for argument in ("--test", target)
        ),
        {},
    )
    assert independent_calls[5] == (
        ("-p", runner.HARNESS, "--bin", runner.HARNESS)
        + tuple(
            argument
            for target in sorted(runner.INDEPENDENT_HARNESS_TARGETS)
            for argument in ("--test", target)
        ),
        {},
    )
    assert native_calls == [
        (
            ("-p", runner.BROKER, "--lib", runner.NATIVE_LIB_TEST),
            {"reject_skip": True},
        ),
        (
            ("-p", runner.BROKER)
            + tuple(
                argument
                for target in sorted(runner.NATIVE_BROKER_TARGETS)
                for argument in ("--test", target)
            ),
            {"reject_skip": True},
        ),
        (
            ("-p", runner.HARNESS)
            + tuple(
                argument
                for target in sorted(runner.NATIVE_HARNESS_TARGETS)
                for argument in ("--test", target)
            ),
            {"reject_skip": True},
        ),
    ]
