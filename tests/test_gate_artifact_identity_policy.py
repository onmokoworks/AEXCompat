from __future__ import annotations

import hashlib
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def _write(root: Path, relative: str, payload: bytes) -> Path:
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(payload)
    return path


def _run_copied_gate(
    tmp_path: Path, script_name: str, prelude: str = ""
) -> subprocess.CompletedProcess[str]:
    tools = tmp_path / "tools"
    tools.mkdir()
    shutil.copy2(ROOT / "tools" / script_name, tools / script_name)
    script = str(tools / script_name).replace("'", "''")
    return subprocess.run(
        [
            "powershell.exe",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            f"Import-Module Microsoft.PowerShell.Utility; {prelude} & '{script}' -Verbose",
        ],
        cwd=tmp_path,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )


def test_glator_gate_records_rebuilt_artifacts_before_driver_validation(
    tmp_path: Path,
) -> None:
    artifacts = {
        "broker/target/release/aexcompat-harness.exe": b"rebuilt harness",
        "target/minihost-build/aex_worker.exe": b"rebuilt discovery worker",
        "target/sdk-fixtures/glator/GLator.aex": b"rebuilt GLator fixture",
    }
    for relative, payload in artifacts.items():
        _write(tmp_path, relative, payload)

    no_driver_modules = (
        "function global:Get-ChildItem { "
        "param($Path, [switch]$Recurse, [string]$Filter, $ErrorAction); @() };"
    )
    result = _run_copied_gate(
        tmp_path, "run-glator-runtime-policy-inspect-gate.ps1", no_driver_modules
    )
    output = result.stdout + result.stderr

    assert result.returncode != 0
    assert "Expected exactly one authenticated nvoglv64.dll" in output
    for payload in artifacts.values():
        assert hashlib.sha256(payload).hexdigest() in output


def test_maskoffset_gate_records_rebuilt_artifacts_before_approval_validation(
    tmp_path: Path,
) -> None:
    artifacts = {
        "broker/target/release/broker.exe": b"rebuilt broker",
        "target/minihost-build/aex_worker.exe": b"rebuilt SmartFX worker",
        "target/render-requests/maskoffset-color-fill-20260713-001.json": b"rebuilt request",
    }
    for relative, payload in artifacts.items():
        _write(tmp_path, relative, payload)
    _write(
        tmp_path,
        "target/smart-allowlist/maskoffset.active.local.json",
        b'{"entries":[]}',
    )

    result = _run_copied_gate(tmp_path, "run-maskoffset-smartfx-render-gate.ps1")
    output = result.stdout + result.stderr

    assert result.returncode != 0
    assert "Exactly one MaskOffset approval is required" in output
    for payload in artifacts.values():
        assert hashlib.sha256(payload).hexdigest() in output
