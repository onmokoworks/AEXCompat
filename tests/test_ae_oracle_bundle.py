from pathlib import Path
import hashlib
import json


ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "analysis" / "AE_ORACLE_BUNDLE_PREPARATION_2026-07-16.json"


def test_preparation_evidence_does_not_claim_an_ae_capture() -> None:
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    assert evidence["status"] == "prepared_not_captured"
    assert len(evidence["probes"]) == 3
    assert evidence["safety_contract"]["self_elevates"] is False
    assert any("No After Effects oracle values" in item for item in evidence["observations"])


def test_bundle_manifest_authenticates_source_artifacts_and_payloads() -> None:
    manifest_path = ROOT / "target/ae-oracle-bundle/manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8-sig"))
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    assert evidence["bundle_manifest"]["size_bytes"] == manifest_path.stat().st_size
    assert evidence["bundle_manifest"]["sha256"] == hashlib.sha256(manifest_path.read_bytes()).hexdigest()

    records = {record["id"]: record for record in evidence["probes"]}
    for entry in manifest["probes"]:
        source = ROOT / records[entry["id"]]["artifact"]
        payload = ROOT / "target/ae-oracle-bundle" / entry["file"]
        source_hash = hashlib.sha256(source.read_bytes()).hexdigest()
        assert payload.read_bytes() == source.read_bytes()
        assert entry["size"] == source.stat().st_size
        assert entry["sha256"] == source_hash
        assert records[entry["id"]]["size_bytes"] == source.stat().st_size
        assert records[entry["id"]]["sha256"] == source_hash
