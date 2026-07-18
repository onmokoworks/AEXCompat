import hashlib
import re
import shutil
import subprocess
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "refresh-worker-trust.ps1"
POWERSHELL = shutil.which("powershell") or shutil.which("pwsh")

pytestmark = pytest.mark.skipif(
    POWERSHELL is None,
    reason="requires a PowerShell executable (powershell or pwsh)",
)

WORKERS = {
    "aex_l2_worker": ("L2_WORKER_TRUST", "generated_l2_worker_trust.rs"),
    "aex_render_worker": ("RENDER_WORKER_TRUST", "generated_render_worker_trust.rs"),
    "aex_smart_worker": ("SMART_WORKER_TRUST", "generated_smart_worker_trust.rs"),
}


def run_refresh(build_root, trust_root):
    return subprocess.run(
        [
            POWERSHELL, "-NoProfile", "-ExecutionPolicy", "Bypass",
            "-File", str(SCRIPT),
            "-SkipBuild", "-BuildRoot", str(build_root), "-TrustRoot", str(trust_root),
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=180,
    )


def parse_trust(source):
    encoded = bytes(int(value, 16) for value in re.findall(r"0x([0-9a-f]{2})", source))
    size = int(re.search(r"expected_size: ([0-9_]+)", source).group(1).replace("_", ""))
    return encoded, size


def test_skip_build_generates_hash_and_size_for_all_workers(tmp_path):
    build_root = tmp_path / "build"
    trust_root = tmp_path / "trust"
    build_root.mkdir()
    trust_root.mkdir()
    payloads = {}
    for index, name in enumerate(WORKERS):
        payload = bytes([0x41 + index]) * (100_000 + index)
        (build_root / f"{name}.exe").write_bytes(payload)
        payloads[name] = payload

    result = run_refresh(build_root, trust_root)
    assert result.returncode == 0, result.stderr

    for name, (constant, file_name) in WORKERS.items():
        source = (trust_root / file_name).read_text(encoding="utf-8")
        encoded, size = parse_trust(source)
        assert f"const {constant}: WorkerTrust" in source
        assert len(encoded) == 32
        assert encoded.hex() == hashlib.sha256(payloads[name]).hexdigest()
        assert size == len(payloads[name])

    for file_name in ("generated_render_worker_trust.rs", "generated_smart_worker_trust.rs"):
        header = (trust_root / file_name).read_text(encoding="utf-8").splitlines()[0]
        assert "refresh-worker-trust.ps1" in header
        assert "refresh-sdk-grabba-evidence" not in header


def test_missing_worker_leaves_every_trust_file_unchanged(tmp_path):
    build_root = tmp_path / "build"
    trust_root = tmp_path / "trust"
    build_root.mkdir()
    trust_root.mkdir()
    # The last worker in generation order is absent, so a per-worker
    # verify-then-write loop would already have written the first two files.
    names = list(WORKERS)
    for name in names[:-1]:
        (build_root / f"{name}.exe").write_bytes(b"present" * 32)
    sentinel = "// pre-existing trust sentinel\n"
    for _, file_name in WORKERS.values():
        (trust_root / file_name).write_text(sentinel, encoding="utf-8")

    result = run_refresh(build_root, trust_root)
    assert result.returncode != 0
    assert f"{names[-1]}.exe" in result.stderr

    for _, file_name in WORKERS.values():
        assert (trust_root / file_name).read_text(encoding="utf-8") == sentinel
