import hashlib
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "target" / "minihost-build" / "aex_l2_worker.exe"
TRUST = ROOT / "broker" / "crates" / "broker" / "src" / "generated_l2_worker_trust.rs"
SCRIPT = ROOT / "tools" / "refresh-sdk-grabba-evidence.ps1"


def test_generated_trust_tuple_matches_release_worker():
    source = TRUST.read_text(encoding="utf-8")
    encoded = bytes(int(value, 16) for value in re.findall(r"0x([0-9a-f]{2})", source))
    size = int(re.search(r"expected_size: ([0-9_]+)", source).group(1).replace("_", ""))

    assert len(encoded) == 32
    assert encoded.hex() == hashlib.sha256(WORKER.read_bytes()).hexdigest()
    assert size == WORKER.stat().st_size


def test_refresh_keeps_release_build_and_hash_bound_dispatch():
    source = SCRIPT.read_text(encoding="utf-8")
    assert "-DCMAKE_BUILD_TYPE=Release" in source
    assert "--target', 'aex_l2_worker" in source
    assert "generated_l2_worker_trust.rs" in source
    assert "Get-FileHash" in source
    assert "--dispatch-experimental-aegp-update-menu" in source
    assert "--dispatch-experimental-aegp-idle" in source
    assert "--dispatch-experimental-aegp-command-roundtrip" in source
