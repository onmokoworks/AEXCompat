import copy
import hashlib
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
REAL_RUNNER = ROOT / "tools" / "run-issue26-scene-probe-real-ae.ps1"
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


def write_json(path: Path, value) -> None:
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


def artifact_identity(path: Path):
    payload = path.read_bytes()
    return {
        "path": str(path.resolve()),
        "sha256": hashlib.sha256(payload).hexdigest(),
        "size_bytes": len(payload),
    }


def provenance_sha256(value) -> str:
    payload = {
        key: item
        for key, item in value.items()
        if key != "provenance_sha256"
    }
    return hashlib.sha256(
        json.dumps(
            payload,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
    ).hexdigest()


def bind_probe_report(tmp_path: Path, name: str, record) -> None:
    raw = tmp_path / f"{name}-raw-probe.json"
    write_json(raw, record["probe_report"])
    record["artifacts"]["raw_probe_report"] = artifact_identity(raw)
    record["cleanup"] = record["probe_report"]["cleanup"]
    record["unsupported_slots"] = record["probe_report"][
        "unsupported_slots"
    ]


def bind_stdout(record, host) -> None:
    stdout = json.dumps(host, separators=(",", ":")) + "\n"
    record["execution"]["stdout"] = stdout
    record["execution"]["stdout_sha256"] = hashlib.sha256(
        stdout.encode("utf-8")
    ).hexdigest()
    record["host_report"] = {
        key: host[key] for key in record["host_report"]
    }


def test_issue26_evidence_schema_is_strict_and_valid():
    schema = strict_load(SCHEMA_PATH)
    Draft202012Validator.check_schema(schema)
    assert schema["additionalProperties"] is False
    assert schema["$defs"]["probe_report"]["additionalProperties"] is False
    assert schema["$defs"]["coverage"]["additionalProperties"] is False
    assert schema["$defs"]["cleanup"]["additionalProperties"] is False
    assert schema["$defs"]["fixture_report"]["additionalProperties"] is False


def test_evidence_validator_rejects_duplicate_keys(tmp_path: Path):
    source = (CORPUS / "aexcompat.json").read_text(encoding="utf-8")
    duplicate = source.replace(
        '"status": "passed",',
        '"status": "passed",\n  "status": "passed",',
        1,
    )
    path = tmp_path / "duplicate-key.json"
    path.write_text(duplicate, encoding="utf-8")
    completed = subprocess.run(
        [sys.executable, str(TOOL), "validate", str(path)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert completed.returncode != 0
    assert "duplicate JSON key: status" in completed.stderr


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
        "AEGP_DeleteKeyframe",
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
        '"Issue26 Child Comp"',
        '"Issue26 Child Footage"',
        "layers.addSolid",
        "layers.addNull",
        "layers.addCamera",
        "solid.parent = parent",
        "zoom.setValueAtTime(0.0, 700)",
        "zoom.setValueAtTime(1.0, 900)",
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
    non_real_records = [
        path
        for path in records
        if path.name != "after-effects-26.3-blocked.json"
    ]
    completed = subprocess.run(
        [sys.executable, str(TOOL), "validate", *map(str, non_real_records)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    blocked = subprocess.run(
        [sys.executable, str(TOOL), "validate", *map(str, records)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert blocked.returncode != 0
    assert "status=blocked" in blocked.stderr
    by_target = {strict_load(path)["target"]: strict_load(path) for path in records}
    aexcompat = by_target["aexcompat"]
    assert aexcompat["status"] == "passed"
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
    assert (
        aexcompat["probe_report"]["coverage"]["keyframes"][
            "spatial_tangents"
        ]
        is True
    )
    assert aexcompat["unsupported_slots"] == []
    assert aexcompat["host_report"]["effect_lifetimes_balanced"] is True
    assert aexcompat["host_report"]["stream_lifetimes_balanced"] is True
    assert aexcompat["host_report"]["suite_leases_balanced"] is True
    real = by_target["after_effects"]
    assert real["status"] == "blocked"
    assert real["execution"]["blocker"] is not None
    assert real["probe_report"] is None
    projector = by_target["sdk_projector_aexcompat"]
    assert projector["sample_report"]["classification"] == (
        "guarded_initialization_failure"
    )
    assert projector["host_report"]["status"] == "initialization_failed"
    assert projector["host_report"]["boundary_regression_passed"] is True
    assert projector["host_report"]["entry_fault"] == "cpp_exception"
    assert projector["host_report"]["entry_exception_code"] == 0
    assert projector["host_report"]["forced_suite_releases"] == 0
    assert projector["host_report"]["suite_acquires"] == 1
    assert projector["host_report"]["suite_releases"] == 1
    assert projector["host_report"]["live_suite_reference_count"] == 0
    assert projector["execution"]["exit_code"] == 0
    assert "0xC0000409" not in (
        projector["execution"]["stdout"] + projector["execution"]["stderr"]
    )
    assert "suite_acquire_failed" in projector["execution"]["stderr"]
    for target in ("sdk_projector_aexcompat", "sdk_resizer_aexcompat"):
        record = by_target[target]
        assert record["sample_report"]["sdk_source_unchanged"] is True
        assert record["sample_report"]["build_receipt_sha256"] == (
            record["artifacts"]["build_receipt"]["sha256"]
        )
        assert record["execution"]["stdout"]
        assert record["execution"]["stdout_sha256"]
        assert record["sample_report"]["source_inputs_sha256"]
        receipt = strict_load(
            Path(record["artifacts"]["build_receipt"]["path"])
        )
        sample = next(
            value
            for value in receipt["samples"]
            if value["sample"] == record["sample_report"]["sample"]
        )
        assert {value["role"] for value in sample["source_inputs"]} == {
            "sample_source",
            "shared_util",
            "sdk_header",
            "injected_props",
        }
        assert {value["role"] for value in sample["generated_inputs"]} == {
            "pipl_preprocessed",
            "pipl_compiled",
            "pipl_resource",
        }
        assert {value["role"] for value in sample["toolchain"]} == {
            "vcvars",
            "msbuild",
            "compiler",
            "linker",
            "resource_compiler",
            "pipl_tool",
        }
        for role in (
            "msbuild",
            "compiler",
            "linker",
            "resource_compiler",
        ):
            identity = next(
                value
                for value in sample["toolchain"]
                if value["role"] == role
            )
            assert identity["file_version"]
        assert sample["build_log"]["size_bytes"] > 0
        assert sample["build_binlog"]["size_bytes"] > 0


def test_readiness_validator_rejects_semantically_incomplete_records(
    tmp_path: Path,
):
    source = strict_load(CORPUS / "aexcompat.json")

    cases = []
    partial = copy.deepcopy(source)
    partial["status"] = "partial"
    cases.append(("partial", partial))

    nonzero = copy.deepcopy(source)
    nonzero["execution"]["exit_code"] = 1
    cases.append(("nonzero", nonzero))

    blocked = copy.deepcopy(source)
    blocked["execution"]["blocker"] = {
        "code": "test_blocker",
        "message": "synthetic blocker",
        "details": [],
    }
    cases.append(("blocker", blocked))

    unbalanced = copy.deepcopy(source)
    unbalanced["cleanup"]["balanced"] = False
    unbalanced["probe_report"]["cleanup"]["balanced"] = False
    cases.append(("cleanup", unbalanced))

    no_tangents = copy.deepcopy(source)
    no_tangents["probe_report"]["coverage"]["keyframes"][
        "spatial_tangents"
    ] = False
    cases.append(("spatial", no_tangents))

    unsupported = copy.deepcopy(source)
    required_slot = {
        "operation": "AEGP_GetNewKeyframeSpatialTangents",
        "suite": "AEGP Keyframe Suite",
        "version": 5,
        "slot": 17,
        "error": 4,
    }
    unsupported["unsupported_slots"] = [required_slot]
    unsupported["probe_report"]["unsupported_slots"] = [required_slot]
    cases.append(("unsupported", unsupported))

    for name, record in cases:
        if record["probe_report"] is not None:
            bind_probe_report(tmp_path, name, record)
        path = tmp_path / f"{name}.json"
        write_json(path, record)
        completed = subprocess.run(
            [sys.executable, str(TOOL), "validate", str(path)],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=False,
        )
        assert completed.returncode != 0, name


def test_readiness_validator_rejects_adversarial_internal_contradictions(
    tmp_path: Path,
):
    source = strict_load(CORPUS / "aexcompat.json")
    cases = []

    duplicate_identity = copy.deepcopy(source)
    duplicate_identity["probe_report"]["identities"][1]["token"] = (
        duplicate_identity["probe_report"]["identities"][0]["token"]
    )
    cases.append(("duplicate-identity", duplicate_identity))

    foreign_owner = copy.deepcopy(source)
    effect = next(
        value
        for value in foreign_owner["probe_report"]["identities"]
        if value["kind"] == "effect"
    )
    effect["owner_id"] = 999999
    cases.append(("foreign-owner", foreign_owner))

    impossible_streams = copy.deepcopy(source)
    impossible_streams["probe_report"]["streams"][1]["ordinal"] = 0
    cases.append(("stream-order", impossible_streams))

    transaction = copy.deepcopy(source)
    transaction["probe_report"]["coverage"]["transaction"][
        "after_cleanup"
    ] += 1
    cases.append(("transaction", transaction))

    cleanup = copy.deepcopy(source)
    cleanup["probe_report"]["cleanup"]["residual_scene_mutations"] = 1
    cases.append(("residual-mutation", cleanup))

    empty_output = copy.deepcopy(source)
    empty_output["execution"]["stdout"] = ""
    empty_output["execution"]["stdout_sha256"] = hashlib.sha256(
        b""
    ).hexdigest()
    cases.append(("empty-stdout", empty_output))

    bad_host = copy.deepcopy(source)
    raw_host = json.loads(source["execution"]["stdout"])
    raw_host["suite_releases"] = raw_host["suite_acquires"] - 1
    raw_host["suite_leases_balanced"] = True
    bind_stdout(bad_host, raw_host)
    cases.append(("host-lifetime", bad_host))

    for name, record in cases:
        bind_probe_report(tmp_path, name, record)
        path = tmp_path / f"{name}.json"
        write_json(path, record)
        completed = subprocess.run(
            [sys.executable, str(TOOL), "validate", str(path)],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=False,
        )
        assert completed.returncode != 0, name


def test_readiness_validator_rejects_missing_or_mismatched_provenance(
    tmp_path: Path,
):
    source = strict_load(CORPUS / "aexcompat.json")
    missing = copy.deepcopy(source)
    missing["artifacts"]["worker"]["path"] = str(
        tmp_path / "missing-worker.exe"
    )

    mismatched = copy.deepcopy(source)
    mismatched["artifacts"]["worker"]["sha256"] = "0" * 64

    projector = strict_load(CORPUS / "sdk-projector-aexcompat.json")
    receipt_mismatch = copy.deepcopy(projector)
    receipt_mismatch["sample_report"]["build_receipt_sha256"] = "0" * 64

    receipt_tamper = copy.deepcopy(projector)
    original_receipt = strict_load(
        Path(receipt_tamper["artifacts"]["build_receipt"]["path"])
    )
    tampered_receipt = copy.deepcopy(original_receipt)
    projector_entry = next(
        value
        for value in tampered_receipt["samples"]
        if value["sample"] == "Projector"
    )
    projector_entry["source_inputs"] = projector_entry[
        "source_inputs"
    ][1:]
    projector_entry["provenance_sha256"] = provenance_sha256(
        projector_entry
    )
    tampered_path = tmp_path / "tampered-build-receipt.json"
    write_json(tampered_path, tampered_receipt)
    tampered_identity = artifact_identity(tampered_path)
    receipt_tamper["artifacts"]["build_receipt"] = tampered_identity
    receipt_tamper["sample_report"]["build_receipt_sha256"] = (
        tampered_identity["sha256"]
    )

    for name, record in (
        ("missing", missing),
        ("mismatched", mismatched),
        ("receipt", receipt_mismatch),
        ("transitive-receipt", receipt_tamper),
    ):
        path = tmp_path / f"{name}.json"
        write_json(path, record)
        completed = subprocess.run(
            [sys.executable, str(TOOL), "validate", str(path)],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=False,
        )
        assert completed.returncode != 0, name


def test_blocked_and_crashed_evidence_generators_return_nonzero(
    tmp_path: Path,
):
    source = strict_load(CORPUS / "aexcompat.json")
    environment = source["environment"]
    artifacts = source["artifacts"]
    common = [
        "--probe",
        artifacts["probe"]["path"],
        "--fixture",
        artifacts["fixture"]["path"],
        "--worker",
        artifacts["worker"]["path"],
        "--after-effects",
        environment["after_effects"]["executable"]["path"],
        "--ae-version",
        environment["after_effects"]["version"],
        "--sdk-root",
        environment["sdk"]["root"],
        "--sdk-api-version",
        str(environment["sdk"]["aefx_api_version"]),
        "--sdk-guide",
        environment["sdk"]["guide"]["path"],
    ]

    blocked = subprocess.run(
        [
            sys.executable,
            str(TOOL),
            "record-real-blocker",
            *common,
            "--output",
            str(tmp_path / "blocked.json"),
            "--command-part",
            "not-run",
            "--blocker-code",
            "test_blocker",
            "--blocker-message",
            "synthetic blocker",
        ],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert blocked.returncode != 0

    raw_report = tmp_path / "raw-report.json"
    raw_report.write_text(
        json.dumps(source["probe_report"], ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    fixture_metadata = tmp_path / "fixture-metadata.json"
    fixture_metadata.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "fixture": "issue26-scene-probe",
                "project_count": 1,
                "folder_count": 1,
                "footage_count": 2,
                "comp_count": 2,
                "layer_count": 4,
                "effect_count": 2,
                "mask_count": 1,
                "position_keyframes": 2,
                "mask_keyframes": 2,
                "camera_zoom_keyframes": 2,
            }
        )
        + "\n",
        encoding="utf-8",
    )
    stdout_file = tmp_path / "real-ae.stdout.txt"
    stderr_file = tmp_path / "real-ae.stderr.txt"
    stdout_file.write_text("synthetic stdout\n", encoding="utf-8")
    stderr_file.write_text("synthetic crash\n", encoding="utf-8")
    crashed = subprocess.run(
        [
            sys.executable,
            str(TOOL),
            "wrap-real",
            *common,
            "--output",
            str(tmp_path / "crashed.json"),
            "--raw-report",
            str(raw_report),
            "--fixture-metadata",
            str(fixture_metadata),
            "--stdout-file",
            str(stdout_file),
            "--stderr-file",
            str(stderr_file),
            "--command-part",
            "synthetic-real-host",
            "--exit-code",
            "3221226505",
        ],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert crashed.returncode != 0


def test_public_abi_source_contracts_are_exact():
    scene = (
        ROOT / "minihost" / "src" / "worker_aegp_scene.cpp"
    ).read_text(encoding="utf-8")
    registry = (
        ROOT / "minihost" / "src" / "worker_suite_registry.cpp"
    ).read_text(encoding="utf-8")
    probe = PROBE.read_text(encoding="utf-8")
    assert '{"AEGP Proj Suite", 9}' in registry
    assert "case ItemKind::footage: result = 4;" in scene
    assert 'case 0: name = u"Input"; break;' in scene
    assert "type == AEGP_StreamType_LAYER_ID" in probe
    assert "input_value.val.layer_id == layer_id" in probe


def test_real_runner_retains_outputs_and_fixture_report_inputs():
    runner = REAL_RUNNER.read_text(encoding="utf-8")
    tool = TOOL.read_text(encoding="utf-8")
    assert "-RedirectStandardOutput $standardOutput" in runner
    assert "-RedirectStandardError $standardError" in runner
    assert (
        'record["artifacts"]["fixture_metadata"] = artifact(fixture_path)'
        in tool
    )
    assert 'record["fixture_report"] = fixture_report' in tool
