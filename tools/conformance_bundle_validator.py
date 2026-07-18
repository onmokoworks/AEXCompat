import hashlib
import json
import os
import stat
import sys
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


def _open_posix_beneath(bundle_root: Path, relative: Path) -> int:
    nofollow = getattr(os, "O_NOFOLLOW", None)
    directory = getattr(os, "O_DIRECTORY", None)
    if nofollow is None or directory is None or os.open not in os.supports_dir_fd:
        raise OSError("secure dirfd traversal is unavailable")

    root_fd = os.open(bundle_root, os.O_RDONLY | directory | nofollow)
    current_fd = root_fd
    try:
        root_path_metadata = bundle_root.lstat()
        root_handle_metadata = os.fstat(root_fd)
        if (root_path_metadata.st_dev, root_path_metadata.st_ino) != (
            root_handle_metadata.st_dev, root_handle_metadata.st_ino
        ):
            raise OSError("bundle root changed while it was opened")
        for index, part in enumerate(relative.parts):
            final = index == len(relative.parts) - 1
            flags = os.O_RDONLY | nofollow | (0 if final else directory)
            next_fd = os.open(part, flags, dir_fd=current_fd)
            if current_fd != root_fd:
                os.close(current_fd)
            current_fd = next_fd
        descriptor = current_fd
        current_fd = -1
        return descriptor
    finally:
        if current_fd >= 0 and current_fd != root_fd:
            os.close(current_fd)
        os.close(root_fd)


def _windows_final_path(handle) -> str:
    import ctypes

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    get_final_path = kernel32.GetFinalPathNameByHandleW
    get_final_path.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32]
    get_final_path.restype = ctypes.c_uint32
    length = get_final_path(handle, None, 0, 0)
    if not length:
        raise ctypes.WinError(ctypes.get_last_error())
    buffer = ctypes.create_unicode_buffer(length + 1)
    if not get_final_path(handle, buffer, len(buffer), 0):
        raise ctypes.WinError(ctypes.get_last_error())
    value = buffer.value
    if value.startswith("\\\\?\\UNC\\"):
        return "\\\\" + value[8:]
    if value.startswith("\\\\?\\"):
        return value[4:]
    return value


def _open_windows_beneath(bundle_root: Path, relative: Path) -> tuple[int, tuple[int, int, int]]:
    import ctypes
    import msvcrt
    from ctypes import wintypes

    class ByHandleFileInformation(ctypes.Structure):
        _fields_ = [
            ("attributes", wintypes.DWORD), ("creation_low", wintypes.DWORD),
            ("creation_high", wintypes.DWORD), ("access_low", wintypes.DWORD),
            ("access_high", wintypes.DWORD), ("write_low", wintypes.DWORD),
            ("write_high", wintypes.DWORD), ("volume_serial", wintypes.DWORD),
            ("size_high", wintypes.DWORD), ("size_low", wintypes.DWORD),
            ("links", wintypes.DWORD), ("file_index_high", wintypes.DWORD),
            ("file_index_low", wintypes.DWORD),
        ]

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    create_file = kernel32.CreateFileW
    create_file.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p,
                            wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    create_file.restype = wintypes.HANDLE
    get_info = kernel32.GetFileInformationByHandle
    get_info.argtypes = [wintypes.HANDLE, ctypes.POINTER(ByHandleFileInformation)]
    get_info.restype = wintypes.BOOL
    close_handle = kernel32.CloseHandle

    share_all = 0x1 | 0x2 | 0x4
    open_existing = 3
    backup_semantics = 0x02000000
    open_reparse_point = 0x00200000
    invalid = wintypes.HANDLE(-1).value

    def open_handle(path: Path, directory: bool):
        flags = open_reparse_point | (backup_semantics if directory else 0)
        handle = create_file(str(path), 0x80000000, share_all, None, open_existing, flags, None)
        if handle == invalid:
            raise ctypes.WinError(ctypes.get_last_error())
        return handle

    def handle_info(handle):
        info = ByHandleFileInformation()
        if not get_info(handle, ctypes.byref(info)):
            raise ctypes.WinError(ctypes.get_last_error())
        if info.attributes & _REPARSE_POINT:
            raise OSError("artifact path contains a reparse point")
        identity = (info.volume_serial, info.file_index_high, info.file_index_low)
        return info, identity

    def normalized(path: Path) -> str:
        return os.path.normcase(os.path.normpath(str(path)))

    handles = []
    file_handle = None
    try:
        root_handle = open_handle(bundle_root, True)
        handles.append(root_handle)
        _, root_identity = handle_info(root_handle)
        root_path = Path(_windows_final_path(root_handle))
        expected = root_path
        identities = [root_identity]

        for index, part in enumerate(relative.parts):
            final = index == len(relative.parts) - 1
            expected = expected / part
            handle = open_handle(bundle_root.joinpath(*relative.parts[: index + 1]), not final)
            handles.append(handle)
            _, identity = handle_info(handle)
            identities.append(identity)
            if normalized(Path(_windows_final_path(handle))) != normalized(expected):
                raise OSError("artifact component handle does not match its bundle path")

        # Keep every component handle alive and verify the complete chain again so
        # rename/replacement races cannot splice handles from different trees.
        expected = root_path
        for index, handle in enumerate(handles):
            if index:
                expected = expected / relative.parts[index - 1]
            _, identity = handle_info(handle)
            if identity != identities[index] or normalized(Path(_windows_final_path(handle))) != normalized(expected):
                raise OSError("artifact path changed while component handles were opened")

        file_handle = handles.pop()
        identity = identities[-1]
        descriptor = msvcrt.open_osfhandle(file_handle, os.O_RDONLY | getattr(os, "O_BINARY", 0))
        file_handle = None  # descriptor now owns the HANDLE
        return descriptor, identity
    finally:
        if file_handle is not None:
            close_handle(file_handle)
        for handle in reversed(handles):
            close_handle(handle)


def _open_artifact_beneath(bundle_root: Path, relative: Path) -> tuple[int, object]:
    if os.name == "nt":
        return _open_windows_beneath(bundle_root, relative)
    if os.name == "posix":
        descriptor = _open_posix_beneath(bundle_root, relative)
        metadata = os.fstat(descriptor)
        return descriptor, (metadata.st_dev, metadata.st_ino)
    raise OSError(f"secure artifact opening is unsupported on {sys.platform}")


def _verify_artifact(bundle_root: Path, artifact: dict, label: str) -> str | None:
    relative = Path(*artifact["path"].split("/"))
    descriptor = None
    try:
        descriptor, identity = _open_artifact_beneath(bundle_root, relative)
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode) or _is_reparse(before):
            return f"{label} is not a regular file"
        if before.st_size != artifact["size_bytes"]:
            return f"{label} size does not match manifest"

        digest = hashlib.sha256()
        with os.fdopen(descriptor, "rb", closefd=False) as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)

        after = os.fstat(descriptor)
        after_identity = identity if os.name == "nt" else (after.st_dev, after.st_ino)
        stable_fields = ("st_dev", "st_ino", "st_size", "st_mtime_ns", "st_ctime_ns")
        if after_identity != identity or any(getattr(before, field) != getattr(after, field) for field in stable_fields):
            return f"{label} changed while it was hashed"
        if digest.hexdigest() != artifact["sha256"]:
            return f"{label} SHA-256 does not match manifest"
    except (FileNotFoundError, NotADirectoryError):
        return f"{label} does not exist"
    except OSError as error:
        return f"{label} cannot be verified: {error}"
    finally:
        if descriptor is not None:
            os.close(descriptor)
    return None


def _artifacts(manifest: dict, report: dict):
    yield "manifest plugin", manifest["plugin"]["aex"]
    for index, artifact in enumerate(manifest["plugin"]["dependencies"]):
        yield f"manifest dependency {index}", artifact
    yield "manifest input", manifest["input"]
    yield "manifest runner", manifest["runner"]
    for depth, artifact in manifest["oracle"].get("artifacts", {}).items():
        yield f"manifest oracle {depth}", artifact
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

    oracle_artifacts = manifest["oracle"].get("artifacts", {})
    manifest_oracle_identity = manifest["oracle"]["identity_match"]
    if manifest["oracle"]["state"] == "captured" and set(oracle_artifacts) != set(requested):
        errors.append("manifest oracle depths do not exactly match requested_depths")
    for result in report["results"]:
        _validate_world(result, errors)
        oracle = result["oracle"]
        if oracle["identity_match"] != manifest_oracle_identity:
            errors.append(f"{result['depth']} oracle identity_match does not match manifest")
        if not manifest_oracle_identity and oracle["exact"]:
            errors.append(f"{result['depth']} exact oracle requires manifest identity match")
        if oracle["state"] == "captured":
            oracle_artifact = oracle_artifacts.get(result["depth"])
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
