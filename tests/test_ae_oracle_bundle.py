from pathlib import Path
import hashlib
import json


ROOT = Path(__file__).resolve().parents[1]
PREPARE = (ROOT / "tools" / "prepare-ae-oracle-bundle.ps1").read_text(encoding="utf-8")
MANAGE = (ROOT / "tools" / "manage-ae-oracle-bundle.ps1").read_text(encoding="utf-8")
EVIDENCE = ROOT / "analysis" / "AE_ORACLE_BUNDLE_PREPARATION_2026-07-16.json"


def test_bundle_contains_all_three_reversible_oracles() -> None:
    for fixture in (
        "pf_transform_multimatrix_oracle.aex",
        "pf_path_curve_probe.aex",
        "pf_color_oracle.aex",
    ):
        assert fixture in PREPARE
    assert "aexcompat-ae-oracle-bundle-v1" in PREPARE
    assert "manifest_sha256" in PREPARE
    assert "AEXCompat PF Path Curve'" in PREPARE


def test_all_bundle_runners_use_the_capture_environment_contract() -> None:
    for name in (
        "ae-transform-multimatrix-oracle-run.jsx",
        "ae-path-curve-oracle-run.jsx",
        "ae-color-oracle-run.jsx",
    ):
        source = (ROOT / "tools" / name).read_text(encoding="utf-8")
        for variable in ("AEXCOMPAT_AE_RESULT", "AEXCOMPAT_AE_OUTPUT", "AEXCOMPAT_AE_EFFECT", "AEXCOMPAT_AE_BPC"):
            assert variable in source
        assert "app.quit()" in source
        assert "DO_NOT_SAVE_CHANGES" in source


def test_manager_never_self_elevates_and_validates_before_mutation() -> None:
    lowered = MANAGE.lower()
    assert "runas" not in lowered
    assert "start-process" not in lowered
    assert "get-process afterfx,aerender,aerendercore" in lowered
    assert "assert-manifestpayload -root $bundle" in lowered
    assert "assert-manifestpayload -root $installroot -installed" in lowered
    assert "refusing removal because the installed manifest differs" in lowered


def test_manager_uses_one_fixed_child_directory() -> None:
    assert "AEXCompatOracleBundle" in MANAGE
    assert "[System.IO.Path]::GetFileName($installRoot)" in MANAGE
    assert "Remove-Item -LiteralPath $installRoot -Recurse -Force" in MANAGE


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
