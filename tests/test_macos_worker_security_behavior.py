"""Executable macOS worker security and credential-free packaging checks.

These tests intentionally invoke the product's Rust and shell entry points.  A
marker left in source code must not be enough to keep a security claim green.
"""

import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys

import pytest


ROOT = Path(__file__).resolve().parents[1]
HARNESS_MANIFEST = ROOT / "broker/Cargo.toml"
GUEST_MANIFEST = ROOT / "guest/Cargo.toml"


pytestmark = pytest.mark.skipif(sys.platform != "darwin", reason="macOS behavior")


def run(command: list[str], *, environment: dict[str, str] | None = None):
    return subprocess.run(
        command,
        cwd=ROOT,
        env=environment,
        text=True,
        capture_output=True,
        timeout=300,
        check=False,
    )


def require_success(result: subprocess.CompletedProcess[str]):
    assert result.returncode == 0, result.stdout + result.stderr


def test_native_carrier_requires_both_explicit_trust_opt_ins():
    result = run(
        [
            "cargo",
            "test",
            "--manifest-path",
            str(HARNESS_MANIFEST),
            "-p",
            "aexcompat-harness",
            "macos::tests::native_carrier_requires_explicit_trusted_plugin_acknowledgement",
            "--",
            "--exact",
            "--test-threads=1",
        ]
    )
    require_success(result)
    assert "1 passed" in result.stdout + result.stderr


def test_pe_admission_rejects_malformed_and_out_of_bounds_images():
    result = run(
        [
            "cargo",
            "test",
            "--manifest-path",
            str(GUEST_MANIFEST),
            "-p",
            "aex-guest-worker",
            "pe::tests::",
            "--",
            "--test-threads=1",
        ]
    )
    require_success(result)
    combined = result.stdout + result.stderr
    assert "empty_and_oversized_inputs_fail_before_parse" in combined
    assert "entry_rva_must_be_inside_an_executable_section" in combined
    assert "tls_callbacks_are_bounded_inside_executable_sections" in combined


def test_entitlements_are_data_contracts_not_source_markers():
    arm = plistlib.loads((ROOT / "tools/macos/arm64-unicorn.entitlements").read_bytes())
    native = plistlib.loads(
        (ROOT / "tools/macos/x86_64-native-carrier.entitlements").read_bytes()
    )
    assert arm == {
        "com.apple.security.cs.allow-jit": True,
        "com.apple.security.cs.allow-unsigned-executable-memory": True,
    }
    assert native == {"com.apple.security.cs.allow-unsigned-executable-memory": True}
    forbidden = {
        "com.apple.security.get-task-allow",
        "com.apple.security.cs.disable-library-validation",
        "com.apple.security.cs.disable-executable-page-protection",
        "com.apple.security.cs.allow-dyld-environment-variables",
    }
    assert forbidden.isdisjoint(arm)
    assert forbidden.isdisjoint(native)


def test_adhoc_arm64_worker_packages_verifies_and_rejects_signature_mutation(tmp_path):
    require_success(
        run(
            [
                "cargo",
                "build",
                "--release",
                "--manifest-path",
                str(GUEST_MANIFEST),
                "-p",
                "aex-guest-worker",
            ]
        )
    )
    built_worker = ROOT / "guest/target/release/aex-guest-worker"
    worker = tmp_path / "aex-guest-worker"
    shutil.copy2(built_worker, worker)
    environment = os.environ.copy()
    environment.update(
        {
            "AEXCOMPAT_CODESIGN_IDENTITY": "-",
            "AEXCOMPAT_INCLUDE_NATIVE_CARRIER": "0",
        }
    )
    require_success(
        run(
            [str(ROOT / "tools/sign-macos-aex-carriers.sh"), str(worker)],
            environment=environment,
        )
    )

    package = tmp_path / "aexcompat-local.dmg"
    package_environment = environment | {"AEXCOMPAT_DISTRIBUTION_TIER": "local-adhoc"}
    require_success(
        run(
            [
                str(ROOT / "tools/package-macos-aex-carriers.sh"),
                str(worker),
                str(tmp_path / "unused-native-worker"),
                str(package),
            ],
            environment=package_environment,
        )
    )
    require_success(
        run(
            [str(ROOT / "tools/verify-macos-aex-carrier-package.sh"), str(package)],
            environment=environment,
        )
    )

    payload = bytearray(worker.read_bytes())
    payload[len(payload) // 2] ^= 1
    worker.write_bytes(payload)
    rejected = run(
        [str(ROOT / "tools/verify-macos-aex-carriers.sh"), str(worker)],
        environment=environment,
    )
    assert rejected.returncode != 0
    assert "code object is not signed at all" in rejected.stderr or "invalid" in rejected.stderr
