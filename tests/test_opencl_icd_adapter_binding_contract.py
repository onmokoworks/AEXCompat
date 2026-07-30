import json
from pathlib import Path

import pytest
from jsonschema import Draft202012Validator
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "schemas" / "opencl-icd-adapter-binding.schema.json"


def _validator():
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    collector = json.loads(
        (ROOT / "schemas" / "opencl-icd-collector.schema.json").read_text(
            encoding="utf-8"
        )
    )
    registry = Registry().with_resource(
        collector["$id"],
        Resource.from_contents(collector),
    )
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema, registry=registry)


def _adapter():
    return {
        "schema_version": 1,
        "backend": "opencl",
        "adapter_luid": "0123456789abcdef",
        "pci_vendor_id": "10de",
        "pci_device_id": "2684",
        "pci_subsystem_id": "14584100",
        "pci_revision_id": "a1",
        "driver_inf": "oem42.inf",
        "driver_catalog_sha256": "aa" * 32,
        "driver_version": "32.0.15.9999",
        "os_build": 26100,
    }


def _binding(status="verified"):
    verified = status == "verified"
    return {
        "registry_view": "registry64",
        "candidate_classification": "identity_verified",
        "candidate_path_basename": "vendor-opencl.dll",
        "candidate_path_fingerprint_sha256": "11" * 32,
        "candidate_identity_sha256": "22" * 32,
        "status": status,
        "authoritative_evidence": (
            "catalog_sha256_exact_match" if verified else None
        ),
        "adapter": _adapter() if verified else None,
        "backend_ready": False,
    }


def _report(binding=None):
    return {
        "schema_version": 1,
        "binding_kind": "opencl_icd_adapter_driver",
        "bindings": [binding or _binding()],
        "registry_diagnostics": [],
        "platform_diagnostics": [],
    }


def test_schema_accepts_verified_and_all_fail_closed_classifications():
    validator = _validator()
    validator.validate(_report())
    for status in (
        "unverified_missing_candidate_identity",
        "unverified_no_catalog_evidence",
        "unverified_no_matching_driver_package",
        "unverified_incomplete_platform_evidence",
        "ambiguous_multiple_driver_packages",
        "conflict_missing_catalog_digest",
        "conflict_driver_identity",
    ):
        validator.validate(_report(_binding(status)))


@pytest.mark.parametrize(
    "field",
    (
        "candidate_path_basename",
        "candidate_path_fingerprint_sha256",
        "candidate_identity_sha256",
        "authoritative_evidence",
        "adapter",
    ),
)
def test_verified_binding_rejects_missing_or_null_authoritative_evidence(field):
    validator = _validator()
    missing = _report()
    del missing["bindings"][0][field]
    assert list(validator.iter_errors(missing))

    null = _report()
    null["bindings"][0][field] = None
    assert list(validator.iter_errors(null))


@pytest.mark.parametrize(
    "classification",
    (
        "missing_dll",
        "disabled",
        "untrusted_dll",
        "registry_read_failure",
    ),
)
def test_verified_binding_requires_identity_verified_candidate(classification):
    validator = _validator()
    report = _report()
    report["bindings"][0]["candidate_classification"] = classification
    assert list(validator.iter_errors(report))


@pytest.mark.parametrize(
    "field,value",
    (
        ("candidate_path_basename", r"C:\Private\vendor-opencl.dll"),
        ("candidate_path_basename", "/usr/lib/vendor-opencl.dll"),
        ("candidate_path_basename", r"subdir\vendor-opencl.dll"),
        ("candidate_path_fingerprint_sha256", "AA" * 32),
        ("candidate_identity_sha256", "a" * 63),
        ("adapter.driver_inf", r"C:\Windows\INF\oem42.inf"),
        ("adapter.driver_inf", "../oem42.inf"),
    ),
)
def test_schema_rejects_private_paths_and_malformed_identity(field, value):
    validator = _validator()
    report = _report()
    if field.startswith("adapter."):
        report["bindings"][0]["adapter"][field.split(".", 1)[1]] = value
    else:
        report["bindings"][0][field] = value
    assert list(validator.iter_errors(report))


def test_nonverified_binding_cannot_claim_adapter_or_backend_readiness():
    validator = _validator()
    report = _report(_binding("ambiguous_multiple_driver_packages"))
    report["bindings"][0]["adapter"] = _adapter()
    assert list(validator.iter_errors(report))

    report = _report(_binding("unverified_no_matching_driver_package"))
    report["bindings"][0]["backend_ready"] = True
    assert list(validator.iter_errors(report))
