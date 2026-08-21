#!/usr/bin/env python3
"""Run the broker workspace's disjoint native-independent/native test partitions."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "broker" / "Cargo.toml"
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


def cargo_test(*arguments: str, reject_skip: bool = False) -> None:
    command = [
        "cargo",
        "test",
        "--manifest-path",
        str(MANIFEST),
        "--locked",
        *arguments,
    ]
    if reject_skip:
        command.extend(["--", "--nocapture"])
    print("+", subprocess.list2cmdline(command), flush=True)
    result = subprocess.run(
        command,
        cwd=ROOT,
        capture_output=reject_skip,
        text=reject_skip,
        encoding="utf-8" if reject_skip else None,
        errors="replace" if reject_skip else None,
    )
    if reject_skip:
        sys.stdout.write(result.stdout)
        sys.stderr.write(result.stderr)
        unexpected = unexpected_skip_lines(result.stdout + result.stderr)
        if unexpected:
            raise SystemExit(
                f"native Rust test attempted an unexpected artifact skip: {unexpected}"
            )
    if result.returncode:
        raise SystemExit(result.returncode)


def unexpected_skip_lines(output: str) -> list[str]:
    skip_lines = [line for line in output.lower().splitlines() if "skipping" in line]
    return [
        line for line in skip_lines if line.strip() not in KNOWN_ISSUE_900_SKIP_LINES
    ]


def run_independent(independent: dict[str, set[str]]) -> None:
    cargo_test("--workspace", "--exclude", BROKER, "--exclude", HARNESS)
    cargo_test("-p", BROKER, "--lib", "--", "--skip", NATIVE_LIB_TEST)
    cargo_test("-p", BROKER, "--doc")
    cargo_test("-p", BROKER, "--bins")
    broker_targets = [
        argument
        for target in sorted(independent[BROKER])
        for argument in ("--test", target)
    ]
    cargo_test("-p", BROKER, *broker_targets)
    harness_targets = [
        argument
        for target in sorted(independent[HARNESS])
        for argument in ("--test", target)
    ]
    cargo_test("-p", HARNESS, "--bin", HARNESS, *harness_targets)


def run_native(native: dict[str, set[str]]) -> None:
    missing = [
        relative
        for relative in SILENT_SKIP_PREREQUISITES
        if not (ROOT / relative).is_file()
    ]
    if missing:
        raise SystemExit(f"native Rust test prerequisites are missing: {missing}")
    cargo_test("-p", BROKER, "--lib", NATIVE_LIB_TEST, reject_skip=True)
    broker_targets = [
        argument for target in sorted(native[BROKER]) for argument in ("--test", target)
    ]
    cargo_test("-p", BROKER, *broker_targets, reject_skip=True)
    harness_targets = [
        argument
        for target in sorted(native[HARNESS])
        for argument in ("--test", target)
    ]
    cargo_test("-p", HARNESS, *harness_targets, reject_skip=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("partition", choices=("independent", "native"))
    args = parser.parse_args()
    independent, native = partitions(cargo_metadata())
    if args.partition == "independent":
        run_independent(independent)
    else:
        run_native(native)
    return 0


if __name__ == "__main__":
    sys.exit(main())
