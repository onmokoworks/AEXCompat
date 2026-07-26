import json
import subprocess
import sys
from pathlib import Path

from jsonschema import Draft202012Validator


ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "schemas" / "issue26-scene-probe-evidence.schema.json"
PROBE = (
    ROOT
    / "instruments"
    / "aex"
    / "issue26-scene-probe"
    / "issue26_scene_probe.cpp"
)
FIXTURE = PROBE.with_name("fixture.jsx")
TOOL = ROOT / "tools" / "issue26_scene_probe_evidence.py"
CORPUS = ROOT / "corpus" / "issue26-scene-probe"


def reject_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def strict_load(path: Path):
    return json.loads(
        path.read_text(encoding="utf-8-sig"),
        object_pairs_hook=reject_duplicate_keys,
    )


def test_issue26_evidence_schema_is_strict_and_valid():
    schema = strict_load(SCHEMA_PATH)
    Draft202012Validator.check_schema(schema)
    assert schema["additionalProperties"] is False
    assert schema["$defs"]["probe_report"]["additionalProperties"] is False
    assert schema["$defs"]["coverage"]["additionalProperties"] is False
    assert schema["$defs"]["cleanup"]["additionalProperties"] is False


def test_probe_is_host_neutral_and_uses_only_public_aegp_surfaces():
    source = PROBE.read_text(encoding="utf-8")
    assert "AEXCompat" not in source
    assert "GetModuleFileName" not in source
    assert "ISSUE26_SCENE_PROBE_EVIDENCE" in source
    for marker in (
        "AEGP_GetNumProjects",
        "AEGP_GetFirstProjItem",
        "AEGP_GetNextProjItem",
        "AEGP_GetCompLayerByIndex",
        "AEGP_GetLayerEffectByIndex",
        "AEGP_GetNewEffectStreamByIndex",
        "AEGP_GetLayerParent",
        "AEGP_LayerStream_ZOOM",
        "AEGP_GetKeyframeInterpolation",
        "AEGP_GetKeyframeTemporalEase",
        "AEGP_GetNewKeyframeSpatialTangents",
        "AEGP_StartAddKeyframes",
        "AEGP_EndAddKeyframes",
        "AEGP_DuplicateEffect",
        "AEGP_DeleteLayerEffect",
    ):
        assert marker in source
    assert "if (!report.active_comp) return A_Err_NONE;" in source


def test_fixture_authors_required_structural_scene():
    source = FIXTURE.read_text(encoding="utf-8")
    for marker in (
        "items.addFolder",
        "items.addComp",
        "layers.addSolid",
        "layers.addNull",
        "layers.addCamera",
        "solid.parent = parent",
        '"ADBE Slider Control"',
        '"ADBE Easy Levels"',
        '"ADBE Mask Atom"',
        "setInterpolationTypeAtKey",
        "setTemporalEaseAtKey",
        "ISSUE26_SCENE_FIXTURE_METADATA",
    ):
        assert marker in source


def test_aegp_admission_opt_in_keeps_pf_preflight_closed():
    header = (
        ROOT / "minihost" / "src" / "worker_runtime_admission.hpp"
    ).read_text(encoding="utf-8")
    admission = (
        ROOT / "minihost" / "src" / "worker_runtime_admission.cpp"
    ).read_text(encoding="utf-8")
    main = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text(
        encoding="utf-8"
    )
    assert "bool allow_aegp_plugin{}" in header
    assert (
        "!request.allow_aegp_plugin &&\n"
        "      is_aegp_candidate_without_execution(plugin_path)"
    ) in admission
    assert "runtime_request.allow_aegp_plugin = g_aegp_init_mode;" in main


def test_corpus_is_strict_schema_valid_and_derived():
    records = sorted(CORPUS.glob("*.json"))
    assert [path.name for path in records] == [
        "aexcompat.json",
        "after-effects-26.3-blocked.json",
        "sdk-projector-aexcompat.json",
        "sdk-resizer-aexcompat.json",
    ]
    completed = subprocess.run(
        [sys.executable, str(TOOL), "validate", *map(str, records)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    by_target = {strict_load(path)["target"]: strict_load(path) for path in records}
    aexcompat = by_target["aexcompat"]
    assert aexcompat["status"] in {"passed", "partial"}
    assert aexcompat["cleanup"]["balanced"] is True
    assert aexcompat["probe_report"]["coverage"]["effect_order"] == {
        "observed": True,
        "total": True,
    }
    assert aexcompat["probe_report"]["coverage"]["stream_metadata"] is True
    assert (
        aexcompat["probe_report"]["coverage"]["generation"][
            "stale_owner_rejected"
        ]
        is True
    )
    operations = {
        value["operation"]
        for value in aexcompat["probe_report"]["unsupported_slots"]
    }
    assert "AEGP_SetLayerParent(fixture)" in operations
    assert "AEGP_CreateCameraInComp(fixture)" in operations
    assert aexcompat["host_report"]["effect_lifetimes_balanced"] is True
    assert aexcompat["host_report"]["stream_lifetimes_balanced"] is True
    assert aexcompat["host_report"]["suite_leases_balanced"] is True
    real = by_target["after_effects"]
    assert real["status"] in {"passed", "partial", "blocked"}
    if real["status"] == "blocked":
        assert real["execution"]["blocker"] is not None
        assert real["probe_report"] is None
    for target in ("sdk_projector_aexcompat", "sdk_resizer_aexcompat"):
        assert by_target[target]["sample_report"]["sdk_source_unchanged"] is True
