import json
from pathlib import Path

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


def test_opencl_icd_json_rejects_duplicate_keys_and_source_stays_fail_closed():
    duplicate = '{"schema_version":1,"schema_version":2}'

    def reject_duplicates(pairs):
        result = {}
        for key, value in pairs:
            assert key not in result, f"duplicate key: {key}"
            result[key] = value
        return result

    try:
        json.loads(duplicate, object_pairs_hook=reject_duplicates)
    except AssertionError as error:
        assert "duplicate key: schema_version" in str(error)
    else:
        raise AssertionError("duplicate JSON key was accepted")

    source = SOURCE_PATH.read_text(encoding="utf-8")
    for marker in (
        "OpenClIcdClassification::Disabled",
        "OpenClIcdClassification::MalformedRegistryValue",
        "OpenClIcdClassification::MissingDll",
        "OpenClIcdClassification::InaccessibleDll",
        "OpenClIcdClassification::RegistryReadFailure",
        '"adapter_association": "unverified"',
        '"backend_ready": false',
    ):
        assert marker in source
