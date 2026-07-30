import copy
import json
from pathlib import Path

import pytest
from jsonschema import Draft202012Validator


ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "schemas" / "pnp-opencl-runtime-collector.schema.json"


def _validator():
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema)


def _identity():
    return {
        "size": 4096,
        "sha256": "11" * 32,
        "pe_machine": "amd64",
        "volume_serial_number": "12345678",
        "file_index": "1234567890abcdef",
        "authenticode": "catalog",
        "catalog_sha256": "aa" * 32,
    }


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


def _candidate(status="verified", architecture="native"):
    verified = status == "verified"
    return {
        "source_class": "software_component",
        "architecture": architecture,
        "loader_selected": True,
        "value_name": (
            "OpenCLDriverName"
            if architecture == "native"
            else "OpenCLDriverNameWow"
        ),
        "classification": "identity_verified",
        "path_basename": "vendor-opencl.dll",
        "path_fingerprint_sha256": "22" * 32,
        "identity": _identity(),
        "status": status,
        "authoritative_evidence": (
            "pnp_software_key_catalog_adapter_exact_match" if verified else None
        ),
        "adapter": _adapter() if verified else None,
        "legacy_merge": "exact_match" if verified else "not_eligible",
        "backend_ready": False,
    }


def _report(candidate=None):
    return {
        "schema_version": 1,
        "contract": "windows_pnp_opencl_runtime",
        "host_architecture": "x86_64",
        "candidates": [candidate or _candidate()],
        "diagnostics": [],
    }


def test_schema_accepts_native_wow_and_fail_closed_statuses():
    validator = _validator()
    validator.validate(_report())
    validator.validate(_report(_candidate(architecture="wow32")))
    for status in (
        "unverified_candidate",
        "unverified_adapter",
        "unverified_no_catalog_evidence",
        "conflict_catalog_digest",
    ):
        validator.validate(_report(_candidate(status=status)))


@pytest.mark.parametrize(
    "field",
    (
        "loader_selected",
        "classification",
        "path_basename",
        "path_fingerprint_sha256",
        "identity",
        "authoritative_evidence",
        "adapter",
    ),
)
def test_verified_candidate_requires_exact_authoritative_evidence(field):
    validator = _validator()
    report = _report()
    if field == "loader_selected":
        report["candidates"][0][field] = False
    elif field == "classification":
        report["candidates"][0][field] = "missing_dll"
    else:
        report["candidates"][0][field] = None
    assert list(validator.iter_errors(report))


@pytest.mark.parametrize(
    "field,value",
    (
        ("path_basename", r"C:\Private\vendor-opencl.dll"),
        ("path_basename", "/usr/lib/vendor-opencl.dll"),
        ("path_basename", r"subdir\vendor-opencl.dll"),
        ("path_fingerprint_sha256", "AA" * 32),
        ("identity.sha256", "1" * 63),
        ("adapter.driver_inf", r"C:\Windows\INF\oem42.inf"),
        ("adapter.driver_inf", "../oem42.inf"),
    ),
)
def test_schema_rejects_private_paths_and_malformed_identity(field, value):
    validator = _validator()
    report = _report()
    target = report["candidates"][0]
    parts = field.split(".")
    for part in parts[:-1]:
        target = target[part]
    target[parts[-1]] = value
    assert list(validator.iter_errors(report))


def test_native_wow_value_names_are_not_interchangeable():
    validator = _validator()
    native = _report()
    native["candidates"][0]["value_name"] = "OpenCLDriverNameWow"
    assert list(validator.iter_errors(native))

    wow = _report(_candidate(architecture="wow32"))
    wow["candidates"][0]["value_name"] = "OpenCLDriverName"
    assert list(validator.iter_errors(wow))


def test_nonverified_candidate_cannot_claim_adapter_or_readiness():
    validator = _validator()
    report = _report(_candidate(status="unverified_candidate"))
    report["candidates"][0]["adapter"] = _adapter()
    assert list(validator.iter_errors(report))

    report = _report(_candidate(status="unverified_candidate"))
    report["candidates"][0]["backend_ready"] = True
    assert list(validator.iter_errors(report))


@pytest.mark.parametrize(
    "classification",
    (
        "pending_reboot",
        "device_status_failure",
        "child_traversal_failed",
        "child_class_read_failed",
        "no_software_component_child",
    ),
)
def test_configmgr_diagnostics_are_privacy_bounded(classification):
    validator = _validator()
    report = _report()
    report["diagnostics"] = [
        {
            "source_class": "software_component",
            "adapter_luid": "0123456789abcdef",
            "classification": classification,
        }
    ]
    validator.validate(report)
    leaked = copy.deepcopy(report)
    leaked["diagnostics"][0]["device_instance_id"] = r"PCI\VEN_10DE"
    assert list(validator.iter_errors(leaked))
