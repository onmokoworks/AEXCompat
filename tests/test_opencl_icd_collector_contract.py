import json
from pathlib import Path

import pytest
from jsonschema import Draft202012Validator


ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "schemas" / "opencl-icd-collector.schema.json"
SOURCE_PATH = (
    ROOT
    / "broker"
    / "crates"
    / "broker"
    / "src"
    / "opencl_icd_collector.rs"
)


def _sample_report():
    identity = {
        "size": 4096,
        "sha256": "11" * 32,
        "pe_machine": "amd64",
        "volume_serial_number": "12345678",
        "file_index": "1234567890abcdef",
        "authenticode": "embedded",
    }
    candidates = []
    for classification, enabled, candidate_identity in (
        ("identity_verified", True, identity),
        ("disabled", False, None),
        ("malformed_registry_value", None, None),
        ("missing_dll", True, None),
        ("inaccessible_dll", True, None),
    ):
        candidates.append(
            {
                "registry_view": "registry64",
                "enabled": enabled,
                "classification": classification,
                "path_basename": "vendor-opencl.dll",
                "path_fingerprint_sha256": "22" * 32,
                "identity": candidate_identity,
                "adapter_association": "unverified",
                "backend_ready": False,
            }
        )
    return {
        "schema_version": 1,
        "registry_path": r"SOFTWARE\Khronos\OpenCL\Vendors",
        "candidates": candidates,
        "diagnostics": [
            {
                "registry_view": "registry32",
                "classification": "missing_registry_key",
            }
        ],
    }


def test_opencl_icd_report_schema_is_closed_and_accepts_the_failure_matrix():
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    Draft202012Validator(schema).validate(_sample_report())

    unexpected = _sample_report()
    unexpected["candidates"][0]["vendor_inferred"] = "nvidia"
    errors = list(Draft202012Validator(schema).iter_errors(unexpected))
    assert errors

    rounded_failure = _sample_report()
    rounded_failure["candidates"][3]["identity"] = rounded_failure["candidates"][0][
        "identity"
    ]
    errors = list(Draft202012Validator(schema).iter_errors(rounded_failure))
    assert errors


@pytest.mark.parametrize(
    "path",
    (
        r"C:\Windows\System32\vendor-opencl.dll",
        "/usr/lib/vendor-opencl.dll",
        "vendor/subdir/opencl.dll",
        r"vendor\subdir\opencl.dll",
    ),
)
def test_opencl_icd_schema_rejects_paths_where_a_basename_is_required(path):
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    report = _sample_report()
    report["candidates"][0]["path_basename"] = path
    assert list(Draft202012Validator(schema).iter_errors(report))


@pytest.mark.parametrize(
    "field",
    ("path_basename", "path_fingerprint_sha256", "identity"),
)
def test_identity_verified_rejects_null_required_evidence(field):
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    report = _sample_report()
    report["candidates"][0][field] = None
    assert list(Draft202012Validator(schema).iter_errors(report))


@pytest.mark.parametrize(
    "field",
    ("path_basename", "path_fingerprint_sha256", "identity"),
)
def test_identity_verified_rejects_missing_required_evidence(field):
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    report = _sample_report()
    del report["candidates"][0][field]
    assert list(Draft202012Validator(schema).iter_errors(report))


@pytest.mark.parametrize("fingerprint", ("A" * 64, "a" * 63, "a" * 65))
def test_identity_verified_requires_exact_lowercase_sha256_fingerprint(fingerprint):
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    report = _sample_report()
    report["candidates"][0]["path_fingerprint_sha256"] = fingerprint
    assert list(Draft202012Validator(schema).iter_errors(report))


