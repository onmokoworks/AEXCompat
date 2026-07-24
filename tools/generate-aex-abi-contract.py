#!/usr/bin/env python3
"""Generate the C++ and Rust x64 AE ABI contract from a probe observation."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_INPUT = ROOT / "analysis" / "AE_ABI_LAYOUT_OBSERVATION_2026-07-13.json"
DEFAULT_CPP = ROOT / "minihost" / "src" / "generated" / "aex_abi_contract.hpp"
DEFAULT_RUST = ROOT / "guest" / "crates" / "aex-abi" / "src" / "generated.rs"

FIELD_CONTAINER_SIZES = {
    "in": "pf_in_data_size",
    "out": "pf_out_data_size",
    "inter": "pf_interact_callbacks_size",
    "utils": "pf_util_callbacks_size",
    "param": "pf_param_def_size",
    "layer": "pf_layer_def_size",
    "pixel": "pf_pixel_size",
    "pixel16": "pf_pixel16_size",
    "pixel_float": "pf_pixel_float_size",
    "arbitrary": "pf_arbitrary_def_size",
    "event": "pf_event_extra_size",
    "context": "pf_context_size",
    "pre_callbacks": "pf_pre_render_callbacks_size",
    "smart_callbacks": "pf_smart_render_callbacks_size",
}

REQUIRED_FIELDS = {
    "in.inter",
    "in.utils",
    "in.effect_ref",
    "in.quality",
    "in.version",
    "in.appl_id",
    "in.num_params",
    "in.pica_basicP",
    "inter.checkout_param",
    "inter.checkin_param",
    "inter.add_param",
    "param.param_type",
    "param.u",
    "layer.data",
    "layer.rowbytes",
    "layer.width",
    "layer.height",
}

CALLBACK_TABLES = {
    "INPUT_CALLBACK_OFFSETS": (
        "inter.checkout_param",
        "inter.checkin_param",
        "inter.add_param",
        "inter.abort",
        "inter.progress",
        "inter.register_ui",
        "inter.checkout_layer_audio",
        "inter.checkin_layer_audio",
        "inter.get_audio_data",
        "inter.reserved_0",
        "inter.reserved_1",
        "inter.reserved_2",
    ),
    "UTILITY_CALLBACK_OFFSETS": (
        "utils.begin_sampling",
        "utils.subpixel_sample",
        "utils.area_sample",
        "utils.end_sampling",
        "utils.blend",
        "utils.convolve",
        "utils.copy",
        "utils.fill",
        "utils.premultiply",
        "utils.premultiply_color",
        "utils.fill16",
        "utils.premultiply_color16",
        "utils.iterate",
        "utils.new_world",
        "utils.dispose_world",
        "utils.transform_world",
        "utils.ansi_ceil",
        "utils.ansi_fabs",
        "utils.ansi_pow",
        "utils.ansi_sin",
        "utils.ansi_sprintf",
        "utils.ansi_strcpy",
        "utils.get_platform_data",
        "utils.get_pixel_data8",
        "utils.get_pixel_data16",
        "utils.host_new_handle",
        "utils.host_lock_handle",
        "utils.host_unlock_handle",
        "utils.host_dispose_handle",
        "utils.host_get_handle_size",
        "utils.host_resize_handle",
    ),
}
REQUIRED_FIELDS.update(
    field for table in CALLBACK_TABLES.values() for field in table
)
REQUIRED_FIELDS.add("utils.color_callbacks")


class ContractError(ValueError):
    pass


def _reject_duplicate_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ContractError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_contract(path: Path) -> dict[str, Any]:
    try:
        data = json.loads(
            path.read_text(encoding="utf-8"),
            object_pairs_hook=_reject_duplicate_pairs,
            parse_constant=lambda value: (_ for _ in ()).throw(
                ContractError(f"non-finite JSON number: {value}")
            ),
        )
    except (OSError, json.JSONDecodeError) as exc:
        raise ContractError(str(exc)) from exc
    if not isinstance(data, dict):
        raise ContractError("contract root must be an object")
    if data.get("schema_version") != 1:
        raise ContractError("schema_version must be 1")
    if data.get("source_kind") != "compiled_instrument_observation":
        raise ContractError("source_kind must be compiled_instrument_observation")
    if data.get("architecture") != "x86_64-windows":
        raise ContractError("architecture must be x86_64-windows")
    if data.get("pointer_size") != 8:
        raise ContractError("pointer_size must be 8")

    for name, value in data.items():
        if name.endswith("_size") or name == "pointer_size":
            if type(value) is not int or value <= 0 or value > 1 << 20:
                raise ContractError(f"{name} must be an integer in 1..1048576")

    fields = data.get("fields")
    if not isinstance(fields, dict) or not fields:
        raise ContractError("fields must be a non-empty object")
    missing = sorted(REQUIRED_FIELDS - fields.keys())
    if missing:
        raise ContractError(f"missing required fields: {', '.join(missing)}")
    for name, field in fields.items():
        if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_.]+", name):
            raise ContractError(f"invalid field name: {name!r}")
        if not isinstance(field, dict) or set(field) != {"offset", "size"}:
            raise ContractError(f"{name} must contain exactly offset and size")
        offset, size = field["offset"], field["size"]
        if type(offset) is not int or offset < 0:
            raise ContractError(f"{name}.offset must be a non-negative integer")
        if type(size) is not int or size <= 0:
            raise ContractError(f"{name}.size must be a positive integer")
        container_key = FIELD_CONTAINER_SIZES.get(name.split(".", 1)[0])
        if container_key and container_key in data and offset + size > data[container_key]:
            raise ContractError(
                f"{name} exceeds {container_key}: {offset} + {size} > {data[container_key]}"
            )
    for table_name, field_names in CALLBACK_TABLES.items():
        offsets = []
        for name in field_names:
            field = fields[name]
            if field["size"] != data["pointer_size"]:
                raise ContractError(
                    f"{table_name} entry {name} has size {field['size']}, "
                    f"expected pointer_size {data['pointer_size']}"
                )
            offsets.append(field["offset"])
        if len(offsets) != len(set(offsets)):
            raise ContractError(f"{table_name} contains duplicate offsets")
    return data


def ident(name: str) -> str:
    return re.sub(r"[^A-Za-z0-9]+", "_", name).strip("_").upper()


def constants(data: dict[str, Any]) -> list[tuple[str, int]]:
    values = [("SCHEMA_VERSION", data["schema_version"])]
    values += [
        (ident(name), value)
        for name, value in data.items()
        if (name.endswith("_size") or name == "pointer_size") and type(value) is int
    ]
    for name, field in data["fields"].items():
        values.append((f"{ident(name)}_OFFSET", field["offset"]))
        values.append((f"{ident(name)}_SIZE", field["size"]))
    values.sort()
    names = [name for name, _ in values]
    if len(names) != len(set(names)):
        raise ContractError("generated constant names collide")
    return values


def render_cpp(data: dict[str, Any], source: Path) -> str:
    rows = "\n".join(
        f"inline constexpr std::size_t {name} = {value};"
        for name, value in constants(data)
    )
    tables = "\n".join(
        "inline constexpr std::array<std::size_t, "
        f"{len(field_names)}> {table_name}{{"
        + ", ".join(str(data["fields"][name]["offset"]) for name in field_names)
        + "};"
        for table_name, field_names in CALLBACK_TABLES.items()
    )
    return (
        "// Generated by tools/generate-aex-abi-contract.py; do not edit.\n"
        f"// Source: {source.name}\n"
        "#pragma once\n\n"
        "#include <array>\n"
        "#include <cstddef>\n\n"
        "namespace aexcompat::abi::x86_64_windows {\n"
        f"{rows}\n{tables}\n"
        "}  // namespace aexcompat::abi::x86_64_windows\n"
    )


def render_rust(data: dict[str, Any], source: Path) -> str:
    rows = "\n".join(
        f"pub const {name}: usize = {value};" for name, value in constants(data)
    )
    tables = "\n".join(
        f"pub const {table_name}: [usize; {len(field_names)}] = ["
        + ", ".join(str(data["fields"][name]["offset"]) for name in field_names)
        + "];"
        for table_name, field_names in CALLBACK_TABLES.items()
    )
    return (
        "// Generated by tools/generate-aex-abi-contract.py; do not edit.\n"
        f"// Source: {source.name}\n\n"
        f"{rows}\n{tables}\n"
    )


def write_or_check(path: Path, content: str, check: bool) -> None:
    if check:
        try:
            current = path.read_text(encoding="utf-8")
        except OSError as exc:
            raise ContractError(f"generated file missing: {path}") from exc
        if current != content:
            raise ContractError(f"generated file is stale: {path}")
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8", newline="\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, default=DEFAULT_INPUT)
    parser.add_argument("--cpp-output", type=Path, default=DEFAULT_CPP)
    parser.add_argument("--rust-output", type=Path, default=DEFAULT_RUST)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    try:
        data = load_contract(args.input)
        write_or_check(args.cpp_output, render_cpp(data, args.input), args.check)
        write_or_check(args.rust_output, render_rust(data, args.input), args.check)
    except ContractError as exc:
        parser.exit(1, f"error: {exc}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
