import hashlib
import json
import os
import pathlib
import subprocess

import pytest


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "build-sdk-backwards.ps1"
OUTPUT = ROOT / "target" / "sdk-fixtures" / "sdk-backwards" / "SDK_Backwards.aex"
RESULT = ROOT / "target" / "sdk-fixtures" / "sdk-backwards" / "build-result.json"
BUILD_OUTPUT = (
    ROOT / "target" / "sdk-fixtures" / "sdk-backwards" / "ci-build-output.txt"
)


def verify_sdk_backwards_build(output: pathlib.Path, manifest: pathlib.Path) -> str:
    assert output.is_file()
    result = json.loads(manifest.read_text(encoding="utf-8-sig"))
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    assert result["status"] == "built"
    assert result["platform_toolset"] == "v143"
    assert result["configuration"] == "Release|x64"
    assert result["artifact_sha256"] == digest
    assert result["artifact_size"] == output.stat().st_size
    assert result["sdk_source_unchanged"] is True
    return digest


def verify_build_receipt(receipt: str | bytes, digest: str) -> None:
    expected = "SDK_Backwards.aex SHA-256: " + digest
    if isinstance(receipt, str):
        assert expected in receipt
    else:
        assert (
            expected.encode("ascii") in receipt
            or expected.encode("utf-16le") in receipt
        )


def test_official_sdk_backwards_builds_unchanged_and_records_hash():
    completed = None
    if os.environ.get("AEXCOMPAT_SDK_BACKWARDS_PREBUILT") != "1":
        completed = subprocess.run(
            [
                "powershell",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                str(SCRIPT),
            ],
            cwd=ROOT,
            text=True,
            encoding="utf-8",
            errors="replace",
            capture_output=True,
            timeout=300,
        )
        assert completed.returncode == 0, completed.stdout + completed.stderr

    digest = verify_sdk_backwards_build(OUTPUT, RESULT)
    receipt = BUILD_OUTPUT.read_bytes() if completed is None else completed.stdout
    verify_build_receipt(receipt, digest)


@pytest.mark.parametrize(
    ("field", "invalid"),
    (
        ("status", "failed"),
        ("platform_toolset", "v142"),
        ("configuration", "Debug|x64"),
        ("artifact_sha256", "0" * 64),
        ("artifact_size", 999),
        ("sdk_source_unchanged", False),
    ),
)
def test_sdk_backwards_verifier_rejects_contract_mutations(tmp_path, field, invalid):
    output = tmp_path / "SDK_Backwards.aex"
    output.write_bytes(b"fixture bytes")
    result = {
        "status": "built",
        "platform_toolset": "v143",
        "configuration": "Release|x64",
        "artifact_sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
        "artifact_size": output.stat().st_size,
        "sdk_source_unchanged": True,
    }
    result[field] = invalid
    manifest = tmp_path / "build-result.json"
    manifest.write_text(json.dumps(result), encoding="utf-8")

    with pytest.raises(AssertionError):
        verify_sdk_backwards_build(output, manifest)


def test_prebuilt_mode_verifies_without_rebuilding(tmp_path, monkeypatch):
    output = tmp_path / "SDK_Backwards.aex"
    output.write_bytes(b"producer artifact")
    manifest = tmp_path / "build-result.json"
    manifest.write_text(
        json.dumps(
            {
                "status": "built",
                "platform_toolset": "v143",
                "configuration": "Release|x64",
                "artifact_sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
                "artifact_size": output.stat().st_size,
                "sdk_source_unchanged": True,
            }
        ),
        encoding="utf-8",
    )
    build_output = tmp_path / "ci-build-output.txt"
    build_output.write_text(
        "SDK_Backwards.aex SHA-256: "
        + hashlib.sha256(output.read_bytes()).hexdigest()
        + "\n",
        encoding="utf-8",
    )
    monkeypatch.setenv("AEXCOMPAT_SDK_BACKWARDS_PREBUILT", "1")
    monkeypatch.setattr("test_sdk_backwards_fixture_build.OUTPUT", output)
    monkeypatch.setattr("test_sdk_backwards_fixture_build.RESULT", manifest)
    monkeypatch.setattr("test_sdk_backwards_fixture_build.BUILD_OUTPUT", build_output)

    def unexpected_build(*_args, **_kwargs):
        raise AssertionError("prebuilt verification must not rebuild")

    monkeypatch.setattr(subprocess, "run", unexpected_build)
    test_official_sdk_backwards_builds_unchanged_and_records_hash()


@pytest.mark.parametrize("encoding", ("ascii", "utf-16le"))
def test_build_receipt_requires_the_actual_digest(encoding):
    digest = "1" * 64
    receipt = ("SDK_Backwards.aex SHA-256: " + digest).encode(encoding)
    verify_build_receipt(receipt, digest)
    with pytest.raises(AssertionError):
        verify_build_receipt(receipt, "2" * 64)
