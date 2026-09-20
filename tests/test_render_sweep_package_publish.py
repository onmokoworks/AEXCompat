import os
from pathlib import Path
import shutil
import subprocess

import pytest


ROOT = Path(__file__).resolve().parents[1]
PUBLISH_HELPER = ROOT / "tools" / "publish-render-sweep-package.ps1"
PUBLISH_HARNESS = """
param(
    [string]$Helper,
    [string]$Cli,
    [string]$Worker,
    [string]$OutputDirectory,
    [switch]$CreateDestinationBeforeFinalMove
)
$ErrorActionPreference = 'Stop'
. $Helper
$beforeFinalMove = $null
if ($CreateDestinationBeforeFinalMove) {
    $beforeFinalMove = {
        param($TargetOutput)
        New-Item -ItemType Directory -Path $TargetOutput | Out-Null
        Set-Content -LiteralPath (Join-Path $TargetOutput 'racer.txt') -Value 'racer'
    }
}
Publish-RenderSweepPackage `
    -Cli $Cli `
    -Worker $Worker `
    -OutputDirectory $OutputDirectory `
    -BeforeFinalMove $beforeFinalMove | Out-Null
""".strip()


def run_publish(
    tmp_path: Path,
    cli: Path,
    worker: Path,
    output_directory: str,
    *,
    create_destination_before_final_move: bool = False,
) -> subprocess.CompletedProcess[str]:
    harness = tmp_path / "publish.ps1"
    harness.write_text(PUBLISH_HARNESS, encoding="utf-8")
    argv = [
        "pwsh",
        "-NoProfile",
        "-File",
        str(harness),
        "-Helper",
        str(PUBLISH_HELPER),
        "-Cli",
        str(cli),
        "-Worker",
        str(worker),
        "-OutputDirectory",
        output_directory,
    ]
    if create_destination_before_final_move:
        argv.append("-CreateDestinationBeforeFinalMove")
    return subprocess.run(
        argv,
        capture_output=True,
        text=True,
        timeout=15,
        check=False,
    )


@pytest.mark.skipif(
    os.name != "nt" or shutil.which("pwsh") is None,
    reason="render-sweep package publication is a Windows PowerShell workflow",
)
def test_late_staging_copy_failure_leaves_existing_package_unchanged(tmp_path: Path):
    output = tmp_path / "render-sweep-package"
    old_worker = output / "target" / "minihost-build" / "aex_worker.exe"
    old_worker.parent.mkdir(parents=True)
    old_cli = output / "aexcompat-render-sweep.exe"
    old_cli.write_bytes(b"old-cli")
    old_worker.write_bytes(b"old-worker")

    new_cli = tmp_path / "new-cli.exe"
    new_cli.write_bytes(b"new-cli")
    missing_worker = tmp_path / "missing-worker.exe"
    result = run_publish(tmp_path, new_cli, missing_worker, str(output))

    assert result.returncode != 0
    assert old_cli.read_bytes() == b"old-cli"
    assert old_worker.read_bytes() == b"old-worker"
    generated_prefixes = (
        f".{output.name}.staging-",
        f".{output.name}.previous-",
    )
    assert not any(
        child.name.startswith(generated_prefixes) for child in tmp_path.iterdir()
    )


@pytest.mark.skipif(
    os.name != "nt" or shutil.which("pwsh") is None,
    reason="render-sweep package publication is a Windows PowerShell workflow",
)
def test_trailing_separator_replaces_the_complete_package(tmp_path: Path):
    output = tmp_path / "render-sweep-package"
    old_worker = output / "target" / "minihost-build" / "aex_worker.exe"
    old_worker.parent.mkdir(parents=True)
    old_cli = output / "aexcompat-render-sweep.exe"
    old_cli.write_bytes(b"old-cli")
    old_worker.write_bytes(b"old-worker")

    new_cli = tmp_path / "new-cli.exe"
    new_worker = tmp_path / "new-worker.exe"
    new_cli.write_bytes(b"new-cli")
    new_worker.write_bytes(b"new-worker")

    result = run_publish(tmp_path, new_cli, new_worker, f"{output}{os.sep}")

    assert result.returncode == 0, result.stderr
    assert old_cli.read_bytes() == b"new-cli"
    assert old_worker.read_bytes() == b"new-worker"
    generated_prefixes = (
        f".{output.name}.staging-",
        f".{output.name}.previous-",
    )
    assert not any(
        child.name.startswith(generated_prefixes) for child in tmp_path.iterdir()
    )


@pytest.mark.skipif(
    os.name != "nt" or shutil.which("pwsh") is None,
    reason="render-sweep package publication is a Windows PowerShell workflow",
)
def test_destination_created_during_commit_cannot_absorb_staging(tmp_path: Path):
    output = tmp_path / "render-sweep-package"
    old_worker = output / "target" / "minihost-build" / "aex_worker.exe"
    old_worker.parent.mkdir(parents=True)
    old_cli = output / "aexcompat-render-sweep.exe"
    old_cli.write_bytes(b"old-cli")
    old_worker.write_bytes(b"old-worker")
    new_cli = tmp_path / "new-cli.exe"
    new_worker = tmp_path / "new-worker.exe"
    new_cli.write_bytes(b"new-cli")
    new_worker.write_bytes(b"new-worker")

    result = run_publish(
        tmp_path,
        new_cli,
        new_worker,
        str(output),
        create_destination_before_final_move=True,
    )

    assert result.returncode != 0
    assert (output / "racer.txt").is_file()
    assert not (output / f".{output.name}.staging-").exists()
    assert not (output / "aexcompat-render-sweep.exe").exists()
    assert not any(
        child.name.startswith(f".{output.name}.staging-")
        for child in tmp_path.iterdir()
    )
    backups = [
        child
        for child in tmp_path.iterdir()
        if child.name.startswith(f".{output.name}.previous-")
    ]
    assert len(backups) == 1
    assert (backups[0] / "aexcompat-render-sweep.exe").read_bytes() == b"old-cli"
    assert (
        backups[0] / "target" / "minihost-build" / "aex_worker.exe"
    ).read_bytes() == b"old-worker"
