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
    "adjust_cursor": "pf_adjust_cursor_info_size",
    "adv_app": "pf_adv_app_suite2_size",
    "aegp_collection": "aegp_collection_suite2_size",
    "aegp_collection_item": "aegp_collection_item_v2_size",
    "aegp_command": "aegp_command_suite1_size",
    "aegp_comp": "aegp_comp_suite11_size",
    "aegp_effect": "aegp_effect_suite4_size",
    "aegp_item": "aegp_item_suite9_size",
    "aegp_keyframe": "aegp_keyframe_suite5_size",
    "aegp_layer": "aegp_layer_suite9_size",
    "aegp_layer5": "aegp_layer_suite5_size",
    "aegp_layer8": "aegp_layer_suite8_size",
    "aegp_layer9": "aegp_layer_suite9_size",
    "aegp_register": "aegp_register_suite5_size",
    "aegp_stream": "aegp_stream_suite6_size",
    "aegp_stream_value": "aegp_stream_value2_size",
    "app4": "pf_app_suite4_size",
    "arb_compare": "pf_arb_params_extra_size",
    "arb_copy": "pf_arb_params_extra_size",
    "arb_dispose": "pf_arb_params_extra_size",
    "arb_extra": "pf_arb_params_extra_size",
    "arb_flat_size": "pf_arb_params_extra_size",
    "arb_flatten": "pf_arb_params_extra_size",
    "arb_interp": "pf_arb_params_extra_size",
    "arb_new": "pf_arb_params_extra_size",
    "arb_print": "pf_arb_params_extra_size",
    "arb_print_size": "pf_arb_params_extra_size",
    "arb_unflatten": "pf_arb_params_extra_size",
    "arbitrary": "pf_arbitrary_def_size",
    "batch_sampling": "pf_batch_sampling_suite1_size",
    "checkbox": "pf_checkbox_def_size",
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
    "context": "pf_context_size",
    "custom_ui": "pf_custom_ui_info_size",
    "do_click": "pf_do_click_event_info_size",
    "drawbot_draw": "drawbot_draw_suite_size",
    "drawbot_path": "drawbot_path_suite_size",
    "drawbot_supplier": "drawbot_supplier_suite_size",
    "drawbot_surface": "drawbot_surface_suite_size",
    "effect_custom_ui": "pf_effect_custom_ui_suite1_size",
    "effect_window": "pf_effect_window_info_size",
    "event": "pf_event_extra_size",
    "external_dependencies": "pf_ext_dependencies_extra_size",
    "float_slider": "pf_float_slider_def_size",
    "gpu_setdown_extra": "pf_gpu_device_setdown_extra_size",
    "gpu_setdown_input": "pf_gpu_device_setdown_input_size",
    "gpu_setup_extra": "pf_gpu_device_setup_extra_size",
    "gpu_setup_input": "pf_gpu_device_setup_input_size",
    "gpu_setup_output": "pf_gpu_device_setup_output_size",
    "key_down": "pf_key_down_event_size",
    "overlay_theme": "pf_effect_overlay_theme_suite1_size",
    "popup": "pf_popup_def_size",
    "pre_extra": "pf_pre_render_extra_size",
    "pre_input": "pf_pre_render_input_size",
    "pre_output": "pf_pre_render_output_size",
    "slider": "pf_slider_def_size",
    "smart_extra": "pf_smart_render_extra_size",
    "smart_input": "pf_smart_render_input_size",
    "user_changed": "pf_user_changed_param_extra_size",
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
    "layer.pix_aspect_ratio",
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
        "utils.subpixel_sample16",
        "utils.area_sample16",
        "utils.fill16",
        "utils.premultiply_color16",
        "utils.iterate16",
        "utils.iterate",
        "utils.iterate_origin",
        "utils.new_world",
        "utils.dispose_world",
        "utils.transfer_rect",
        "utils.transform_world",
        "utils.get_callback_addr",
        "utils.ansi_ceil",
        "utils.ansi_cos",
        "utils.ansi_fabs",
        "utils.ansi_hypot",
        "utils.ansi_pow",
        "utils.ansi_sin",
        "utils.ansi_sqrt",
        "utils.ansi_sprintf",
        "utils.ansi_strcpy",
        "utils.ansi_asin",
        "utils.ansi_acos",
        "utils.get_platform_data",
        "utils.get_pixel_data8",
        "utils.get_pixel_data16",
        "utils.host_new_handle",
        "utils.host_lock_handle",
        "utils.host_unlock_handle",
        "utils.host_dispose_handle",
        "utils.host_get_handle_size",
        "utils.iterate_origin_non_clip_src",
        "utils.iterate_generic",
        "utils.host_resize_handle",
        # Legacy application-specific callback `app` at PF_UtilCallbacks+0xC8
        # (issue #362 selector families: PIN-era effects such as Drop_Shadow
        # call it from GLOBAL_SETUP). Kept last so the hook order in
        # l2_main's make_bootstrap_abi_hooks matches one-to-one.
        "utils.app",
    ),
}
REQUIRED_FIELDS.update(
    field for table in CALLBACK_TABLES.values() for field in table
)
REQUIRED_FIELDS.add("utils.color_callbacks")
REQUIRED_FIELDS.add("utils.iterate_origin")
REQUIRED_FIELDS.add("utils.get_callback_addr")
REQUIRED_FIELDS.add("utils.ansi_cos")
REQUIRED_FIELDS.add("utils.ansi_sqrt")
REQUIRED_FIELDS.add("utils.ansi_asin")
REQUIRED_FIELDS.add("utils.ansi_acos")
REQUIRED_FIELDS.update(
    {
        "pre_input.gpu_data",
        "pre_input.what_gpu",
        "pre_input.device_index",
        "pre_input.bitdepth",
        "pre_output.flags",
        "pre_output.pre_render_data",
        "smart_input.gpu_data",
        "smart_input.what_gpu",
        "smart_input.device_index",
        "smart_input.pre_render_data",
        "gpu_setup_extra.input",
        "gpu_setup_extra.output",
        "gpu_setup_input.what_gpu",
        "gpu_setup_input.device_index",
        "gpu_setup_output.gpu_data",
        "gpu_setdown_extra.input",
        "gpu_setdown_input.gpu_data",
        "gpu_setdown_input.what_gpu",
        "gpu_setdown_input.device_index",
    }
)

OBSERVED_ENUM_GROUPS = {
    "selectors": "PF_CMD",
    "out_flags": "PF_OUT_FLAG",
    "out_flags2": "PF_OUT_FLAG2",
    "gpu_frameworks": "PF_GPU_FRAMEWORK",
    "render_output_flags": "PF_RENDER_OUTPUT_FLAG",
}

REQUIRED_ENUM_VALUES = {
    "selectors": {
        "smart_pre_render",
        "smart_render",
        "smart_render_gpu",
        "gpu_device_setup",
        "gpu_device_setdown",
    },
    "gpu_frameworks": {"opencl"},
    "render_output_flags": {"gpu_render_possible"},
}


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
    if type(data.get("schema_version")) is not int or data["schema_version"] != 1:
        raise ContractError("schema_version must be 1")
    if data.get("source_kind") != "compiled_instrument_observation":
        raise ContractError("source_kind must be compiled_instrument_observation")
    if data.get("sdk_boundary") != "instrument_observation":
        raise ContractError("sdk_boundary must be instrument_observation")
    if data.get("native_aex_loaded") is not False:
        raise ContractError("native_aex_loaded must be false")
    if data.get("selector_dispatched") is not False:
        raise ContractError("selector_dispatched must be false")
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
        family = name.split(".", 1)[0]
        container_key = FIELD_CONTAINER_SIZES.get(family)
        if container_key is None:
            raise ContractError(f"no container size mapping for field family: {family}")
        if container_key not in data:
            raise ContractError(f"missing container size: {container_key}")
        if offset + size > data[container_key]:
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
    for group_name, required_names in REQUIRED_ENUM_VALUES.items():
        values = data.get(group_name)
        if not isinstance(values, dict):
            raise ContractError(f"{group_name} must be an object")
        missing_values = sorted(required_names - values.keys())
        if missing_values:
            raise ContractError(
                f"{group_name} is missing required values: {', '.join(missing_values)}"
            )
    for group_name in OBSERVED_ENUM_GROUPS:
        values = data.get(group_name)
        if not isinstance(values, dict):
            raise ContractError(f"{group_name} must be an object")
        for name, value in values.items():
            if not isinstance(name, str) or not re.fullmatch(r"[a-z0-9_]+", name):
                raise ContractError(f"invalid {group_name} name: {name!r}")
            if type(value) is not int or value < 0 or value > 0xFFFF_FFFF:
                raise ContractError(
                    f"{group_name}.{name} must be an integer in 0..4294967295"
                )
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
    for group_name, prefix in OBSERVED_ENUM_GROUPS.items():
        values.extend(
            (f"{prefix}_{ident(name)}", value)
            for name, value in data[group_name].items()
        )
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
        + ", ".join(f"{ident(name)}_OFFSET" for name in field_names)
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
        f"pub const {table_name}: [usize; {len(field_names)}] = [\n"
        + "".join(f"    {ident(name)}_OFFSET,\n" for name in field_names)
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
