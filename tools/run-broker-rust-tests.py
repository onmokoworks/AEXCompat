#!/usr/bin/env python3
"""Run the broker workspace's disjoint native-independent/native test partitions."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "broker" / "Cargo.toml"
NEXTEST_ARCHIVE = Path(
    os.environ.get(
        "AEXCOMPAT_NEXTEST_ARCHIVE",
        ROOT / "broker" / "target" / "nextest-archive.tar.zst",
    )
)
BROKER = "aexcompat-broker"
HARNESS = "aexcompat-harness"
NATIVE_BROKER_TARGETS = {
    "parameter_animation",
    "render_session_wrapper",
    "resident_session_live",
    "smart_cpu_sealed_render",
}
NATIVE_HARNESS_TARGETS = {"artifact_cli"}
INDEPENDENT_BROKER_TARGETS = {
    "cli_operation_aliases",
    "cuda_compute_probe",
    "descriptor_manifest",
    "gpu_platform_collector",
    "opencl_icd_adapter_binding",
    "opencl_icd_collector",
    "opencl_runtime_probe",
    "pnp_opencl_runtime_collector",
    "render_session",
    "render_session_shm_spike",
    "runtime_module_identity",
    "runtime_module_policy",
    "session_dependency_manifest",
    "trusted_worker_stage",
}
INDEPENDENT_HARNESS_TARGETS = {"render_fixture_cli"}
NATIVE_LIB_TEST = "image_render::tests::worker_callback_addr_denial_round_trips_through_broker_diagnostics"
SILENT_SKIP_PREREQUISITES = (
    # One binary serves every route since #1495, so naming it once covers what
    # the discovery and classic executables used to cover separately.
    "target/minihost-build/aex_worker.exe",
    "target/pf-layer-param-probe-build/Release/pf_layer_param_probe.aex",
    "target/pf-param-utils-animation-probe-build/Release/pf_param_utils_animation_probe.aex",
)
KNOWN_ISSUE_900_SKIP_LINES = {
    "skipping image+audio+layer session test: run tools/build-pf-visual-audio-probe.ps1 -target pf_visual_audio_layer_sidecar_probe first",
    "skipping smart timed-multilayer: build the smart worker and the probe",
}


def cargo_metadata() -> dict[str, object]:
    output = subprocess.run(
        [
            "cargo",
            "metadata",
            "--manifest-path",
            str(MANIFEST),
            "--no-deps",
            "--format-version",
            "1",
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    ).stdout
    return json.loads(output)


def integration_targets(metadata: dict[str, object], package_name: str) -> set[str]:
    package = next(
        package for package in metadata["packages"] if package["name"] == package_name
    )
    return {
        target["name"] for target in package["targets"] if target["kind"] == ["test"]
    }


def partitions(
    metadata: dict[str, object],
) -> tuple[dict[str, set[str]], dict[str, set[str]]]:
    all_targets = {
        BROKER: integration_targets(metadata, BROKER),
        HARNESS: integration_targets(metadata, HARNESS),
    }
    native = {BROKER: NATIVE_BROKER_TARGETS, HARNESS: NATIVE_HARNESS_TARGETS}
    independent = {
        BROKER: INDEPENDENT_BROKER_TARGETS,
        HARNESS: INDEPENDENT_HARNESS_TARGETS,
    }
    for package in all_targets:
        missing = (independent[package] | native[package]) - all_targets[package]
        unclassified = all_targets[package] - (independent[package] | native[package])
        if missing:
            raise SystemExit(
                f"classified Rust test targets disappeared from {package}: {sorted(missing)}"
            )
        if unclassified:
            raise SystemExit(
                f"unclassified Rust test targets in {package}: {sorted(unclassified)}"
            )
        if independent[package] & native[package]:
            raise SystemExit(f"overlapping Rust test partitions for {package}")
        if independent[package] | native[package] != all_targets[package]:
            raise SystemExit(f"incomplete Rust test partitions for {package}")
    return independent, native


def nextest_run(filterset: str, *, reject_skip: bool = False) -> None:
    if not NEXTEST_ARCHIVE.is_file():
        raise SystemExit(f"nextest archive is missing: {NEXTEST_ARCHIVE}")
    command = [
        "cargo",
        "nextest",
        "run",
        "--archive-file",
        str(NEXTEST_ARCHIVE),
        "--workspace-remap",
        str(MANIFEST.parent),
    ]
    if reject_skip:
        command.extend(["--success-output", "immediate"])
    command.extend(
        [
            "--failure-output",
            "immediate",
            "-E",
            filterset,
        ]
    )
    print("+", subprocess.list2cmdline(command), flush=True)
    result = subprocess.run(
        command,
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    sys.stdout.write(result.stdout)
    sys.stderr.write(result.stderr)
    if reject_skip:
        unexpected = unexpected_skip_lines(result.stdout + result.stderr)
        if unexpected:
            raise SystemExit(
                f"native Rust test attempted an unexpected artifact skip: {unexpected}"
            )
    if result.returncode:
        raise SystemExit(result.returncode)


def listed_tests(filterset: str) -> set[tuple[str, str]]:
    if not NEXTEST_ARCHIVE.is_file():
        raise SystemExit(f"nextest archive is missing: {NEXTEST_ARCHIVE}")
    command = [
        "cargo",
        "nextest",
        "list",
        "--archive-file",
        str(NEXTEST_ARCHIVE),
        "--workspace-remap",
        str(MANIFEST.parent),
        "--message-format",
        "json",
        "-E",
        filterset,
    ]
    result = subprocess.run(
        command,
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    payload = json.loads(result.stdout)
    return {
        (suite["binary-id"], name)
        for suite in payload["rust-suites"].values()
        for name, testcase in suite["testcases"].items()
        if testcase["filter-match"]["status"] == "matches"
    }


def unexpected_skip_lines(output: str) -> list[str]:
    skip_lines = [line for line in output.lower().splitlines() if "skipping" in line]
    return [
        line for line in skip_lines if line.strip() not in KNOWN_ISSUE_900_SKIP_LINES
    ]


def exact_union(predicate: str, values: set[str]) -> str:
    if not values:
        return "none()"
    return " + ".join(f"{predicate}(={value})" for value in sorted(values))


def independent_filter(independent: dict[str, set[str]]) -> str:
    broker = f"package(={BROKER})"
    harness = f"package(={HARNESS})"
    return " + ".join(
        (
            f"(not {broker} & not {harness})",
            f"({broker} & kind(=lib) - test(={NATIVE_LIB_TEST}))",
            f"({broker} & kind(=bin))",
            f"({broker} & kind(=test) & ({exact_union('binary', independent[BROKER])}))",
            f"({harness} & kind(=bin))",
            f"({harness} & kind(=test) & ({exact_union('binary', independent[HARNESS])}))",
        )
    )


def native_filter(native: dict[str, set[str]]) -> str:
    broker = f"package(={BROKER})"
    harness = f"package(={HARNESS})"
    return " + ".join(
        (
            f"({broker} & kind(=lib) & test(={NATIVE_LIB_TEST}))",
            f"({broker} & kind(=test) & ({exact_union('binary', native[BROKER])}))",
            f"({harness} & kind(=test) & ({exact_union('binary', native[HARNESS])}))",
        )
    )


def validate_archive_partitions(
    independent: dict[str, set[str]], native: dict[str, set[str]]
) -> None:
    all_tests = listed_tests("all()")
    independent_tests = listed_tests(independent_filter(independent))
    native_tests = listed_tests(native_filter(native))
    overlap = independent_tests & native_tests
    missing = all_tests - (independent_tests | native_tests)
    unexpected = (independent_tests | native_tests) - all_tests
    if not all_tests or overlap or missing or unexpected:
        raise SystemExit(
            "nextest archive partitions are not complete and disjoint: "
            f"all={len(all_tests)} independent={len(independent_tests)} "
            f"native={len(native_tests)} overlap={len(overlap)} "
            f"missing={len(missing)} unexpected={len(unexpected)}"
        )
    print(
        "nextest partition validation: "
        f"all={len(all_tests)} independent={len(independent_tests)} "
        f"native={len(native_tests)}"
    )


def run_independent(independent: dict[str, set[str]]) -> None:
    nextest_run(independent_filter(independent))


def run_native(native: dict[str, set[str]]) -> None:
    missing = [
        relative
        for relative in SILENT_SKIP_PREREQUISITES
        if not (ROOT / relative).is_file()
    ]
    if missing:
        raise SystemExit(f"native Rust test prerequisites are missing: {missing}")
    nextest_run(native_filter(native), reject_skip=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("partition", choices=("validate", "independent", "native"))
    args = parser.parse_args()
    independent, native = partitions(cargo_metadata())
    if args.partition == "validate":
        validate_archive_partitions(independent, native)
    elif args.partition == "independent":
        run_independent(independent)
    else:
        run_native(native)
    return 0


if __name__ == "__main__":
    sys.exit(main())
