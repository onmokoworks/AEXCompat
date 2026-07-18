import hashlib
import json
import os
import stat
from pathlib import Path

from jsonschema import Draft202012Validator
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[1]
SCHEMAS = ROOT / "schemas"
_REPARSE_POINT = 0x400
_PIXEL_BYTES = {"argb8": 4, "argb16": 8, "argb32f": 16}


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


def _is_reparse(metadata: os.stat_result) -> bool:
    return bool(getattr(metadata, "st_file_attributes", 0) & _REPARSE_POINT)


def _verify_artifact(bundle_root: Path, artifact: dict, label: str) -> str | None:
    relative = Path(*artifact["path"].split("/"))
    candidate = bundle_root / relative
    try:
        if os.path.commonpath((str(bundle_root), str(candidate.resolve(strict=False)))) != str(bundle_root):
            return f"{label} resolves outside bundle root"

        current = bundle_root
        for part in relative.parts:
            current = current / part
            metadata = current.lstat()
            if stat.S_ISLNK(metadata.st_mode) or _is_reparse(metadata):
                return f"{label} contains a symlink or reparse point"

        flags = os.O_RDONLY | getattr(os, "O_BINARY", 0)
        descriptor = os.open(candidate, flags)
        try:
            before = os.fstat(descriptor)
            path_metadata = candidate.stat()
            if not stat.S_ISREG(before.st_mode) or _is_reparse(before):
                return f"{label} is not a regular file"
            if (before.st_dev, before.st_ino) != (path_metadata.st_dev, path_metadata.st_ino):
                return f"{label} changed while it was opened"
            if os.path.commonpath((str(bundle_root), str(candidate.resolve(strict=True)))) != str(bundle_root):
                return f"{label} changed to resolve outside bundle root"
            current = bundle_root
            for part in relative.parts:
                current = current / part
                metadata = current.lstat()
                if stat.S_ISLNK(metadata.st_mode) or _is_reparse(metadata):
                    return f"{label} changed to contain a symlink or reparse point"
            if before.st_size != artifact["size_bytes"]:
                return f"{label} size does not match manifest"

            digest = hashlib.sha256()
            with os.fdopen(descriptor, "rb", closefd=False) as stream:
                for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(chunk)

            after = os.fstat(descriptor)
            stable_fields = ("st_dev", "st_ino", "st_size", "st_mtime_ns", "st_ctime_ns")
            if any(getattr(before, field) != getattr(after, field) for field in stable_fields):
                return f"{label} changed while it was hashed"
            if digest.hexdigest() != artifact["sha256"]:
                return f"{label} SHA-256 does not match manifest"
        finally:
            os.close(descriptor)
    except (FileNotFoundError, NotADirectoryError):
        return f"{label} does not exist"
    except OSError as error:
        return f"{label} cannot be verified: {error}"
    return None


def _artifacts(manifest: dict, report: dict):
    yield "manifest plugin", manifest["plugin"]["aex"]
    for index, artifact in enumerate(manifest["plugin"]["dependencies"]):
        yield f"manifest dependency {index}", artifact
    yield "manifest input", manifest["input"]
    yield "manifest runner", manifest["runner"]
    if "artifact" in manifest["oracle"]:
        yield "manifest oracle", manifest["oracle"]["artifact"]
    for index, result in enumerate(report["results"]):
        yield f"result {index} raw_input", result["raw_input"]
        if result["raw_output"] is not None:
            yield f"result {index} raw_output", result["raw_output"]


def _validate_world(result: dict, errors: list[str]) -> None:
    depth = result["depth"]
    for key in ("input_world", "world"):
        world = result[key]
        if world is None:
            continue
        if world["pixel_format"] != depth:
            errors.append(f"{depth} {key} pixel_format does not match depth")
        minimum_row_bytes = world["width"] * _PIXEL_BYTES[world["pixel_format"]]
        if world["row_bytes"] < minimum_row_bytes:
            errors.append(f"{depth} {key} row_bytes is smaller than one pixel row")
        extent = world["extent_hint"]
        if not (
            0 <= extent["left"] <= extent["right"] <= world["width"]
            and 0 <= extent["top"] <= extent["bottom"] <= world["height"]
        ):
            errors.append(f"{depth} {key} extent_hint is outside world dimensions")

    expected_input_size = result["input_world"]["row_bytes"] * result["input_world"]["height"]
    if result["raw_input"]["size_bytes"] != expected_input_size:
        errors.append(f"{depth} raw_input size does not match input_world layout")
    if result["world"] is not None and result["raw_output"] is not None:
        expected_output_size = result["world"]["row_bytes"] * result["world"]["height"]
        if result["raw_output"]["size_bytes"] != expected_output_size:
            errors.append(f"{depth} raw_output size does not match output world layout")


def validate_bundle(manifest: dict, report: dict, bundle_root: Path) -> None:
    """Validate documents, cross-document invariants, and artifact identities."""
    manifest_validator, report_validator = _validators()
    errors = _schema_errors(manifest_validator, manifest, "manifest")
    errors.extend(_schema_errors(report_validator, report, "report"))
    if errors:
        raise BundleValidationError("; ".join(errors))

    original_root = Path(bundle_root)
    try:
        original_metadata = original_root.lstat()
        if stat.S_ISLNK(original_metadata.st_mode) or _is_reparse(original_metadata):
            raise BundleValidationError("bundle root must not be a symlink or reparse point")
        root = original_root.resolve(strict=True)
    except (FileNotFoundError, OSError) as error:
        raise BundleValidationError(f"bundle root is unavailable: {error}") from error
    if not root.is_dir() or root.is_symlink() or _is_reparse(root.lstat()):
        raise BundleValidationError("bundle root must be a real directory")

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

    oracle_artifact = manifest["oracle"].get("artifact")
    for result in report["results"]:
        _validate_world(result, errors)
        oracle = result["oracle"]
        if oracle["state"] == "captured":
            if oracle_artifact is None or oracle["expected_sha256"] != oracle_artifact["sha256"]:
                errors.append(f"{result['depth']} expected oracle hash does not match manifest")
            output_hash = result["output_sha256"]
            raw_output = result["raw_output"]
            if output_hash is None or raw_output is None:
                errors.append(f"{result['depth']} captured oracle has no real output")
            elif oracle["actual_sha256"] != output_hash or output_hash != raw_output["sha256"]:
                errors.append(f"{result['depth']} actual/output/raw output hashes differ")
        if oracle["exact"]:
            if result["classification"] != "ok" or result["world"] is None or result["raw_output"] is None:
                errors.append(f"{result['depth']} exact oracle requires a successful real output")
            if oracle["expected_sha256"] != oracle["actual_sha256"]:
                errors.append(f"{result['depth']} exact oracle hashes differ")

    verified = set()
    for label, artifact in _artifacts(manifest, report):
        identity = (artifact["path"], artifact["sha256"], artifact["size_bytes"])
        if identity in verified:
            continue
        error = _verify_artifact(root, artifact, label)
        if error:
            errors.append(error)
        verified.add(identity)

    if errors:
        raise BundleValidationError("; ".join(errors))
