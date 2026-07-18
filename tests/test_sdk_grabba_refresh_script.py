from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "refresh-sdk-grabba-evidence.ps1"


def test_refresh_keeps_release_build_and_dispatch_routes():
    source = SCRIPT.read_text(encoding="utf-8")
    assert "-DCMAKE_BUILD_TYPE=Release" in source
    assert "--target', 'aex_l2_worker" in source
    assert "--dispatch-experimental-aegp-update-menu" in source
    assert "--dispatch-experimental-aegp-idle" in source
    assert "--dispatch-experimental-aegp-command-roundtrip" in source


def test_refresh_no_longer_generates_frozen_worker_trust():
    source = SCRIPT.read_text(encoding="utf-8")
    assert "generated_l2_worker_trust" not in source
    assert "WorkerTrust" not in source
