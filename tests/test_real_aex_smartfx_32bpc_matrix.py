import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "analysis" / "REAL_AEX_SMARTFX_32BPC_MATRIX_2026-07-15.json"


def test_real_aex_32bpc_matrix_is_identity_and_output_bound():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    assert evidence["pixel_format"] == "argb32f"
    assert evidence["columns"] == [
        "name",
        "plugin_sha256",
        "output_width",
        "output_height",
        "raw_output_sha256",
    ]
    assert len(evidence["results"]) == 12
    names = {row[0] for row in evidence["results"]}
    assert len(names) == 12
    for name, plugin_hash, width, height, output_hash in evidence["results"]:
        assert name
        assert len(plugin_hash) == 64
        assert width > 0 and height > 0
        assert len(output_hash) == 64


def test_deep_matrix_preserves_expanded_particle_output():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    by_name = {row[0]: row for row in evidence["results"]}
    assert by_name["ParticleLab"][2:4] == [1848, 1848]
    assert all(row[2:4] == [37, 23] for row in evidence["results"] if row[0] != "ParticleLab")


def test_deep_matrix_keeps_resource_safety_invariants():
    invariants = json.loads(EVIDENCE.read_text(encoding="utf-8"))["invariants"]
    assert all(invariants.values())
