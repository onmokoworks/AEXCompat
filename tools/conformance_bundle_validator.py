import json
from pathlib import Path

from jsonschema import Draft202012Validator
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[1]
SCHEMAS = ROOT / "schemas"


class BundleValidationError(ValueError):
    pass


def _load_schema(name: str) -> dict:
    return json.loads((SCHEMAS / name).read_text(encoding="utf-8"))


def _validators():
    manifest_schema = _load_schema("conformance-manifest.schema.json")
    report_schema = _load_schema("conformance-report.schema.json")
    registry = Registry().with_resource(
        manifest_schema["$id"], Resource.from_contents(manifest_schema)
    )
    return (
        Draft202012Validator(manifest_schema, registry=registry),
        Draft202012Validator(report_schema, registry=registry),
    )


def _schema_errors(validator, value: dict, label: str) -> list[str]:
    return [
        f"{label}{'.' + '.'.join(map(str, error.absolute_path)) if error.absolute_path else ''}: {error.message}"
        for error in sorted(validator.iter_errors(value), key=lambda item: list(item.absolute_path))
    ]


def validate_bundle(manifest: dict, report: dict) -> None:
    """Validate both documents and the invariants JSON Schema cannot express."""
    manifest_validator, report_validator = _validators()
    errors = _schema_errors(manifest_validator, manifest, "manifest")
    errors.extend(_schema_errors(report_validator, report, "report"))
    if errors:
        raise BundleValidationError("; ".join(errors))

    expected_identities = {
        "aex": manifest["plugin"]["aex"],
        "dependencies": manifest["plugin"]["dependencies"],
        "input": manifest["input"],
        "runner": manifest["runner"],
    }
    if report["fixture_id"] != manifest["fixture_id"]:
        errors.append("report fixture_id does not match manifest")
    if report["identities"] != expected_identities:
        errors.append("report identities do not match manifest")

    requested = manifest["requested_depths"]
    reported = [result["depth"] for result in report["results"]]
    if len(reported) != len(set(reported)):
        errors.append("report contains duplicate depth results")
    if set(reported) != set(requested) or len(reported) != len(requested):
        errors.append("report depths do not exactly match requested_depths")

    for result in report["results"]:
        oracle = result["oracle"]
        if oracle["exact"] and oracle["expected_sha256"] != oracle["actual_sha256"]:
            errors.append(f"{result['depth']} exact oracle hashes differ")

    if errors:
        raise BundleValidationError("; ".join(errors))
