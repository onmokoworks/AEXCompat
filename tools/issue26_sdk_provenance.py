#!/usr/bin/env python3
"""Build and independently validate Issue #26 SDK sample provenance."""

from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import os
import re
from pathlib import Path
from typing import Any


EXCLUDED_SOURCE_PARTS = {"debug", "release", "x64", ".vs"}
SOURCE_ROLES = {
    "sample_source",
    "shared_util",
    "sdk_header",
    "injected_props",
}
GENERATED_ROLES = {
    "pipl_preprocessed",
    "pipl_compiled",
    "pipl_resource",
}
TOOLCHAIN_ROLES = {
    "vcvars",
    "msbuild",
    "compiler",
    "linker",
    "resource_compiler",
    "pipl_tool",
}
VERSIONED_TOOLCHAIN_ROLES = {
    "msbuild",
    "compiler",
    "linker",
    "resource_compiler",
}


def reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(
        path.read_text(encoding="utf-8-sig"),
        object_pairs_hook=reject_duplicate_keys,
    )
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain one object")
    return value


def digest_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def artifact(path: Path) -> dict[str, Any]:
    resolved = path.resolve(strict=True)
    if not resolved.is_file():
        raise ValueError(f"not a file: {resolved}")
    return {
        "path": str(resolved),
        "sha256": digest_bytes(resolved.read_bytes()),
        "size_bytes": resolved.stat().st_size,
    }


def role_artifact(role: str, path: Path) -> dict[str, Any]:
    return {"role": role, **artifact(path)}


def normalized(path: Path | str) -> str:
    return str(Path(path).resolve()).replace("\\", "/").casefold()


def files_under(root: Path) -> list[Path]:
    resolved = root.resolve(strict=True)
    if not resolved.is_dir():
        raise ValueError(f"not a directory: {resolved}")
    result = []
    for path in resolved.rglob("*"):
        if not path.is_file():
            continue
        relative_parts = {
            part.casefold() for part in path.relative_to(resolved).parts
        }
        if relative_parts & EXCLUDED_SOURCE_PARTS:
            continue
        result.append(path)
    return sorted(result, key=normalized)


def discover_source_inputs(
    sdk_root: Path, source_root: Path, props: Path
) -> list[dict[str, Any]]:
    sdk = sdk_root.resolve(strict=True)
    source = source_root.resolve(strict=True)
    util = (sdk / "Examples" / "Util").resolve(strict=True)
    headers = (sdk / "Examples" / "Headers").resolve(strict=True)
    entries = [
        *(role_artifact("sample_source", path)
          for path in files_under(source)),
        *(role_artifact("shared_util", path)
          for path in files_under(util)),
        *(role_artifact("sdk_header", path)
          for path in files_under(headers)),
        role_artifact("injected_props", props),
    ]
    paths = [normalized(value["path"]) for value in entries]
    if len(paths) != len(set(paths)):
        raise ValueError("transitive source inputs overlap")
    return sorted(entries, key=lambda value: (value["role"], normalized(value["path"])))


def entries_sha256(entries: list[dict[str, Any]]) -> str:
    lines = [
        "\0".join(
            (
                value["role"],
                normalized(value["path"]),
                value["sha256"],
                str(value["size_bytes"]),
            )
        )
        for value in sorted(
            entries,
            key=lambda item: (item["role"], normalized(item["path"])),
        )
    ]
    return digest_bytes("\n".join(lines).encode("utf-8"))


class VSFixedFileInfo(ctypes.Structure):
    _fields_ = [
        ("signature", ctypes.c_uint32),
        ("struct_version", ctypes.c_uint32),
        ("file_version_ms", ctypes.c_uint32),
        ("file_version_ls", ctypes.c_uint32),
        ("product_version_ms", ctypes.c_uint32),
        ("product_version_ls", ctypes.c_uint32),
        ("file_flags_mask", ctypes.c_uint32),
        ("file_flags", ctypes.c_uint32),
        ("file_os", ctypes.c_uint32),
        ("file_type", ctypes.c_uint32),
        ("file_subtype", ctypes.c_uint32),
        ("file_date_ms", ctypes.c_uint32),
        ("file_date_ls", ctypes.c_uint32),
    ]


def fixed_file_version(path: Path) -> str:
    if os.name != "nt":
        raise ValueError("Windows file-version validation is required")
    version = ctypes.WinDLL("version", use_last_error=True)
    size = version.GetFileVersionInfoSizeW(str(path), None)
    if not size:
        raise ValueError(f"file version is absent: {path}")
    buffer = ctypes.create_string_buffer(size)
    if not version.GetFileVersionInfoW(str(path), 0, size, buffer):
        raise ctypes.WinError(ctypes.get_last_error())
    pointer = ctypes.c_void_p()
    length = ctypes.c_uint()
    if not version.VerQueryValueW(
        buffer, "\\", ctypes.byref(pointer), ctypes.byref(length)
    ):
        raise ctypes.WinError(ctypes.get_last_error())
    info = ctypes.cast(pointer, ctypes.POINTER(VSFixedFileInfo)).contents
    if info.signature != 0xFEEF04BD:
        raise ValueError(f"invalid file-version signature: {path}")
    return ".".join(
        str(value)
        for value in (
            info.file_version_ms >> 16,
            info.file_version_ms & 0xFFFF,
            info.file_version_ls >> 16,
            info.file_version_ls & 0xFFFF,
        )
    )


def optional_fixed_file_version(path: Path) -> str | None:
    try:
        return fixed_file_version(path)
    except ValueError as error:
        if not str(error).startswith("file version is absent:"):
            raise
        return None


def parse_assignments(values: list[str]) -> list[tuple[str, Path]]:
    result = []
    for value in values:
        role, separator, path = value.partition("=")
        if not separator or not role or not path:
            raise ValueError(f"expected role=path: {value}")
        result.append((role, Path(path)))
    return result


def verify_artifact(identity: dict[str, Any]) -> None:
    if artifact(Path(identity["path"])) != {
        "path": identity["path"],
        "sha256": identity["sha256"],
        "size_bytes": identity["size_bytes"],
    }:
        raise ValueError(f"artifact mismatch: {identity['path']}")


def provenance_payload(value: dict[str, Any]) -> dict[str, Any]:
    return {
        key: item
        for key, item in value.items()
        if key != "provenance_sha256"
    }


def provenance_sha256(value: dict[str, Any]) -> str:
    payload = json.dumps(
        provenance_payload(value),
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return digest_bytes(payload)


def command_snapshot(args: argparse.Namespace) -> int:
    entries = discover_source_inputs(
        Path(args.sdk_root), Path(args.source_root), Path(args.props)
    )
    value = {
        "schema_version": 1,
        "sdk_root": str(Path(args.sdk_root).resolve(strict=True)),
        "source_root": str(Path(args.source_root).resolve(strict=True)),
        "props": str(Path(args.props).resolve(strict=True)),
        "source_inputs": entries,
        "source_inputs_sha256": entries_sha256(entries),
    }
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    return 0


def command_receipt(args: argparse.Namespace) -> int:
    before = load_json(Path(args.snapshot_before))
    current = discover_source_inputs(
        Path(args.sdk_root), Path(args.source_root), Path(args.props)
    )
    current_hash = entries_sha256(current)
    if (
        before["source_inputs"] != current
        or before["source_inputs_sha256"] != current_hash
    ):
        raise ValueError("transitive SDK source inputs changed during build")
    generated = [
        role_artifact(role, path)
        for role, path in parse_assignments(args.generated)
    ]
    tools = []
    for role, path in parse_assignments(args.tool):
        identity = role_artifact(role, path)
        identity["file_version"] = optional_fixed_file_version(
            path.resolve(strict=True)
        )
        tools.append(identity)
    sample_artifact = artifact(Path(args.artifact))
    result = {
        "sample": args.sample,
        "source_project": str(Path(args.source_project).resolve(strict=True)),
        "source_root": str(Path(args.source_root).resolve(strict=True)),
        "sdk_root": str(Path(args.sdk_root).resolve(strict=True)),
        "injected_props": str(Path(args.props).resolve(strict=True)),
        "platform_toolset": "v143",
        "configuration": "Release|x64",
        "artifact": sample_artifact["path"],
        "artifact_size": sample_artifact["size_bytes"],
        "artifact_sha256": sample_artifact["sha256"],
        "source_inputs": current,
        "source_inputs_sha256_before": before["source_inputs_sha256"],
        "source_inputs_sha256_after": current_hash,
        "sdk_source_unchanged": True,
        "generated_inputs": sorted(generated, key=lambda value: value["role"]),
        "toolchain": sorted(tools, key=lambda value: value["role"]),
        "build_command": artifact(Path(args.build_command)),
        "build_log": artifact(Path(args.build_log)),
        "build_binlog": artifact(Path(args.build_binlog)),
    }
    result["provenance_sha256"] = provenance_sha256(result)
    output = Path(args.output)
    output.write_text(
        json.dumps(result, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    return 0


def paths_for_role(
    entries: list[dict[str, Any]], role: str
) -> list[str]:
    return [
        normalized(value["path"])
        for value in entries
        if value["role"] == role
    ]


def command_path_mentions(command_text: str, path: str) -> bool:
    return normalized(path) in command_text.replace("\\", "/").casefold()


def read_build_log(path: Path) -> str:
    payload = path.read_bytes()
    if payload.startswith((b"\xff\xfe", b"\xfe\xff")):
        return payload.decode("utf-16")
    return payload.decode("utf-8", errors="replace")


def extract_tool_path(build_log: str, executable: str) -> str | None:
    match = re.search(
        rf"(?im)^\s*(?P<path>[a-z]:\\.+?\\{re.escape(executable)}\.exe)\s",
        build_log,
    )
    return match.group("path").strip() if match else None


def validate_sample_receipt(sample: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    try:
        current = discover_source_inputs(
            Path(sample["sdk_root"]),
            Path(sample["source_root"]),
            Path(sample["injected_props"]),
        )
        current_hash = entries_sha256(current)
        if sample["source_inputs"] != current:
            errors.append("resolved transitive source input set mismatch")
        if (
            sample["source_inputs_sha256_before"] != current_hash
            or sample["source_inputs_sha256_after"] != current_hash
            or not sample["sdk_source_unchanged"]
        ):
            errors.append("transitive source hash mismatch")
        if {
            value["role"] for value in current
        } != SOURCE_ROLES:
            errors.append("transitive source roles are incomplete")

        generated = sample["generated_inputs"]
        if (
            len(generated) != len(GENERATED_ROLES)
            or {value["role"] for value in generated} != GENERATED_ROLES
        ):
            errors.append("generated PiPL/resource inputs are incomplete")
        for value in generated:
            verify_artifact(value)

        tools = sample["toolchain"]
        if (
            len(tools) != len(TOOLCHAIN_ROLES)
            or {value["role"] for value in tools} != TOOLCHAIN_ROLES
        ):
            errors.append("toolchain roles are incomplete")
        for value in tools:
            verify_artifact(value)
            observed_version = optional_fixed_file_version(
                Path(value["path"])
            )
            if observed_version != value["file_version"]:
                errors.append(f"tool version mismatch: {value['role']}")
            if (
                value["role"] in VERSIONED_TOOLCHAIN_ROLES
                and not observed_version
            ):
                errors.append(
                    f"required tool version is absent: {value['role']}"
                )

        for key in ("build_command", "build_log", "build_binlog"):
            verify_artifact(sample[key])
        sample_identity = artifact(Path(sample["artifact"]))
        if (
            sample_identity["sha256"] != sample["artifact_sha256"]
            or sample_identity["size_bytes"] != sample["artifact_size"]
            or sample_identity["path"] != sample["artifact"]
        ):
            errors.append("sample artifact identity mismatch")

        command_text = Path(sample["build_command"]["path"]).read_text(
            encoding="ascii", errors="strict"
        )
        for value in [*generated, *tools]:
            if value["role"] in {"compiler", "linker", "resource_compiler"}:
                continue
            if not command_path_mentions(command_text, value["path"]):
                errors.append(
                    f"build command omits {value['role']} identity"
                )
        log_text = read_build_log(Path(sample["build_log"]["path"]))
        if not log_text.strip():
            errors.append("build log is empty")
        tool_by_role = {value["role"]: value for value in tools}
        for role, executable in (
            ("compiler", "cl"),
            ("linker", "link"),
            ("resource_compiler", "rc"),
        ):
            observed = extract_tool_path(log_text, executable)
            if (
                observed is None
                or normalized(observed)
                != normalized(tool_by_role[role]["path"])
            ):
                errors.append(f"build log {role} identity mismatch")
        if sample["build_binlog"]["size_bytes"] <= 0:
            errors.append("build binlog is empty")
        if provenance_sha256(sample) != sample["provenance_sha256"]:
            errors.append("sample provenance hash mismatch")
    except (KeyError, OSError, ValueError, TypeError) as error:
        errors.append(f"invalid SDK provenance: {error}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    snapshot = subparsers.add_parser("snapshot")
    snapshot.add_argument("--sdk-root", required=True)
    snapshot.add_argument("--source-root", required=True)
    snapshot.add_argument("--props", required=True)
    snapshot.add_argument("--output", required=True)
    snapshot.set_defaults(handler=command_snapshot)

    receipt = subparsers.add_parser("receipt")
    receipt.add_argument("--sample", required=True)
    receipt.add_argument("--sdk-root", required=True)
    receipt.add_argument("--source-root", required=True)
    receipt.add_argument("--source-project", required=True)
    receipt.add_argument("--props", required=True)
    receipt.add_argument("--snapshot-before", required=True)
    receipt.add_argument("--artifact", required=True)
    receipt.add_argument("--generated", action="append", required=True)
    receipt.add_argument("--tool", action="append", required=True)
    receipt.add_argument("--build-command", required=True)
    receipt.add_argument("--build-log", required=True)
    receipt.add_argument("--build-binlog", required=True)
    receipt.add_argument("--output", required=True)
    receipt.set_defaults(handler=command_receipt)
    args = parser.parse_args()
    return args.handler(args)


if __name__ == "__main__":
    raise SystemExit(main())
