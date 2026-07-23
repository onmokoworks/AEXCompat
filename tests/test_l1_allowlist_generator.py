import hashlib
import json
import subprocess
import sys
from pathlib import Path
from uuid import uuid4


ROOT = Path(__file__).resolve().parents[1]
TOOL = ROOT / "tools" / "generate_l1_allowlist.py"
OUTPUT_ROOT = ROOT / "target" / "l1-allowlist"


def _run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(TOOL), *args],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )


def test_generator_emits_authenticated_schema_v1_entry_and_rejects_outside_output(tmp_path):
    fixture = tmp_path / "fixture.aex"
    fixture.write_bytes(b"fixture-aex-bytes")
    output = OUTPUT_ROOT / f"codex-generator-{uuid4().hex}.json"
    outside = tmp_path / "outside.json"
    try:
        result = _run(
            "--plugin-id",
            "fixture",
            "--plugin-path",
            str(fixture),
            "--receipt-id",
            "fixture-l1-test-001",
            "--expires",
            "2026-08-12T23:59:59+09:00",
            "--timeout-ms",
            "5000",
            "--output",
            str(output),
        )
        assert result.returncode == 0, result.stderr
        document = json.loads(output.read_text(encoding="utf-8"))
        assert document["schema_version"] == 1
        assert len(document["entries"]) == 1
        entry = document["entries"][0]
        assert entry["id"] == "fixture"
        assert entry["approved_stage"] == "L1"
        assert entry["receipt_id"] == "fixture-l1-test-001"
        assert entry["byte_size"] == len(b"fixture-aex-bytes")
        assert entry["sha256"] == hashlib.sha256(b"fixture-aex-bytes").hexdigest().upper()

        rejected = _run(
            "--plugin-id",
            "fixture",
            "--plugin-path",
            str(fixture),
            "--receipt-id",
            "fixture-l1-test-002",
            "--expires",
            "2026-08-12T23:59:59+09:00",
            "--timeout-ms",
            "5000",
            "--output",
            str(outside),
        )
        assert rejected.returncode != 0
        assert "target/l1-allowlist" in rejected.stderr
        assert not outside.exists()
    finally:
        output.unlink(missing_ok=True)
