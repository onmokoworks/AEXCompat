import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "build-pf-effect-sequence-data-abi-probe.ps1"
REPORT = ROOT / "target" / "pf-effect-sequence-data-abi-probe-build" / "pf-effect-sequence-data-abi.json"

def test_sdk_abi_probe_compiles_and_confirms_frozen_single_slot_suite():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
                   cwd=ROOT, check=True, timeout=180)
    report = json.loads(REPORT.read_text(encoding="utf-8"))
    assert report["acquisition"] == {"name": "PF Effect Sequence Data Suite", "version": 1}
    assert report["suite"]["size"] == 8
    assert report["suite"]["slot_count"] == 1
    member = report["suite"]["members"]["PF_GetConstSequenceData"]
    assert member == {"offset": 0, "size": 8, "type_matches": True}
    assert report["types"]["PF_ConstHandle"] == 8

def test_native_selftest_covers_pre_setup_foreign_null_and_stale_handles():
    worker = ROOT / "target" / "minihost-build" / "aex_worker.exe"
    if not worker.exists():
        worker = ROOT / "target" / "minihost-build" / "Release" / "aex_worker.exe"
    if not worker.exists():
        worker = ROOT / "target" / "minihost-timed-layers" / "Release" / "aex_worker.exe"
    result = subprocess.run([str(worker), "--kind", "discovery",
                             "--self-test-pf-effect-sequence-data-suite"],
                            cwd=ROOT, check=True, capture_output=True, text=True, timeout=30)
    payload = json.loads(result.stdout.strip())
    assert payload["pf_effect_sequence_data_suite1"] == "passed"
    assert payload["borrowed_handle"] is True
    assert payload["mfr_concurrent_reads"] == 2048
    assert payload["live_sequences"] == 0
    assert payload["publications"] >= 1
    assert payload["invalidations"] >= 1
