"""契約名 → owner ファイル群の一元マニフェスト (issue #127)。

source-text test が読むソースの解決をここに集約する。TU 抽出で実装の
owner が移動したときは、このファイルの該当契約 (WORKER_RUNTIME_OWNERS
または CONTRACTS の 1 エントリ) に新しい owner を 1 行追加するだけで
テスト側の追従が完了する。テスト本体の assert 内容 (マーカー文字列や
ABI static_assert の確認) はこのファイルの変更では変わらない。

使い分け:

- ``worker_text()``: l2_main.cpp とその抽出先 (WORKER_RUNTIME_OWNERS) の
  連結テキスト。「worker 実装のどこかが契約を満たす」ことを検証する
  肯定 assert 用。抽出で実装が移動しても owner を追記すれば追従できる。
- ``L2_MAIN``: l2_main.cpp そのもののパス。「l2_main にはもう無い」ことを
  検証する否定 assert や、entry/admission/dispatch 固有の契約用。
  否定 assert を持つテストを worker_text() に切り替えてはならない
  (owner 追記で否定側が偽陽性の fail になるため)。
- ``contract_text(name)`` / ``contract_files(name)``: 特定領域の
  owner ファイル集合を名前で解決する。集約読みしていたテストはこちら。
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "minihost" / "src"

L2_MAIN = SRC / "l2_main.cpp"
L2_TRANSLATION_UNIT_FILES = (
    L2_MAIN,
    SRC / "l2_main_support.inc",
    SRC / "l2_main_entry.inc",
)


def l2_translation_unit_text() -> str:
    """Return l2_main's textual translation unit in include order."""
    return "\n".join(path.read_text(encoding="utf-8") for path in L2_TRANSLATION_UNIT_FILES)

# l2_main.cpp から抽出された実装の owner 群。TU 抽出のたびにここへ追記する。
# 宣言→定義の出現順を前提にする slice (index の 2 回目参照など) が
# あるため、宣言だけを持つ ABI header は l2_main より前に置く。
WORKER_RUNTIME_OWNERS = (
    "minihost/src/worker_l2_render_abi.hpp",
    "minihost/src/l2_main.cpp",
    "minihost/src/l2_main_support.inc",
    "minihost/src/l2_main_entry.inc",
    "minihost/src/worker_aegp_utility_suite.hpp",
    "minihost/src/worker_aegp_utility_suite.cpp",
    "minihost/src/worker_pf_pixel_data_suite.hpp",
    "minihost/src/worker_pf_pixel_data_suite.cpp",
    "minihost/src/worker_pf_world_suite.hpp",
    "minihost/src/worker_pf_world_suite.cpp",
    "minihost/src/worker_pf_pixel_format_registry.hpp",
    "minihost/src/worker_pf_pixel_format_registry.cpp",
    "minihost/src/worker_pf_param_suites.hpp",
    "minihost/src/worker_pf_param_suites.cpp",
    "minihost/src/worker_aegp_pf_interface_suite.hpp",
    "minihost/src/worker_aegp_pf_interface_suite.cpp",
    "minihost/src/worker_aegp_command_suites.hpp",
    "minihost/src/worker_aegp_command_suites.cpp",
    "minihost/src/worker_mask_suite_tables.hpp",
    "minihost/src/worker_mask_suite_tables.cpp",
    "minihost/src/worker_l2_render_abi.cpp",
    "minihost/src/worker_classic_report.hpp",
    "minihost/src/worker_classic_report.cpp",
    "minihost/src/worker_smart_report.hpp",
    "minihost/src/worker_smart_report.cpp",
    "minihost/src/worker_invocation_orchestration.hpp",
    "minihost/src/worker_invocation_orchestration.cpp",
    "minihost/src/worker_smart_runtime.hpp",
    "minihost/src/worker_smart_runtime.cpp",
    "minihost/src/worker_aegp_init_report.hpp",
    "minihost/src/worker_aegp_init_report.cpp",
    "minihost/src/worker_ui_event_report.hpp",
    "minihost/src/worker_ui_event_report.cpp",
    "minihost/src/worker_audio_execution.hpp",
    "minihost/src/worker_audio_execution.cpp",
    "minihost/src/worker_render_session.hpp",
    "minihost/src/worker_render_session.cpp",
    "minihost/src/worker_drawbot_runtime.hpp",
    "minihost/src/worker_drawbot_runtime.cpp",
    "minihost/src/worker_param_checkout_runtime.hpp",
    "minihost/src/worker_param_checkout_runtime.cpp",
    "minihost/src/worker_classic_render_runtime.cpp",
    "minihost/src/worker_host_suite_wiring.cpp",
    "minihost/src/worker_l2_shared_helpers.cpp",
    "minihost/src/worker_l2_payload_parsers.cpp",
    "minihost/src/worker_early_mode_bridge.hpp",
    "minihost/src/worker_early_mode_bridge.cpp",
    "minihost/src/worker_entry_wiring.cpp",
)

HARNESS_WINDOWS_OWNERS = (
    "broker/crates/harness/src/windows/preflight.rs",
    "broker/crates/harness/src/windows/render_contract.rs",
    "broker/crates/harness/src/windows/live_session.rs",
    "broker/crates/harness/src/windows/app.rs",
    "broker/crates/harness/src/windows/cli.rs",
    "broker/crates/harness/src/windows/tests.rs",
)

IMAGE_RENDER_OWNERS = (
    "broker/crates/broker/src/image_render.rs",
    "broker/crates/broker/src/image_render/diagnostics.rs",
    "broker/crates/broker/src/image_render/types_and_transport.rs",
    "broker/crates/broker/src/image_render/render_operations.rs",
    "broker/crates/broker/src/image_render/inspection_and_probes.rs",
    "broker/crates/broker/src/image_render/session.rs",
    "broker/crates/broker/src/image_render/tests.rs",
)

RENDER_SESSION_OWNERS = (
    "broker/crates/broker/src/render_session.rs",
    "broker/crates/broker/src/render_session/audio.rs",
    "broker/crates/broker/src/render_session/discovery.rs",
    "broker/crates/broker/src/render_session/tests.rs",
)


class CombinedSource:
    """Path-like source-contract reader spanning one split Rust module."""

    def __init__(self, owners):
        self._owners = owners

    def read_text(self, encoding="utf-8"):
        return "\n".join(
            (ROOT / relative).read_text(encoding=encoding)
            for relative in self._owners
        )


L2_SOURCE = CombinedSource(L2_TRANSLATION_UNIT_FILES)
IMAGE_RENDER_SOURCE = CombinedSource(IMAGE_RENDER_OWNERS)
RENDER_SESSION_SOURCE = CombinedSource(RENDER_SESSION_OWNERS)
HARNESS_WINDOWS_SOURCE = CombinedSource(HARNESS_WINDOWS_OWNERS)

# 契約名 → owner ファイル群 (repo ルート相対)。
CONTRACTS = {
    "l2_family": (
        "minihost/src/l2_main.cpp",
        "minihost/src/l2_main_support.inc",
        "minihost/src/l2_main_entry.inc",
        "minihost/src/worker_aegp_utility_suite.hpp",
        "minihost/src/worker_aegp_utility_suite.cpp",
        "minihost/src/worker_pf_pixel_data_suite.hpp",
        "minihost/src/worker_pf_pixel_data_suite.cpp",
        "minihost/src/worker_pf_world_suite.hpp",
        "minihost/src/worker_pf_world_suite.cpp",
        "minihost/src/worker_pf_pixel_format_registry.hpp",
        "minihost/src/worker_pf_pixel_format_registry.cpp",
        "minihost/src/worker_pf_param_suites.hpp",
        "minihost/src/worker_pf_param_suites.cpp",
        "minihost/src/worker_aegp_pf_interface_suite.hpp",
        "minihost/src/worker_aegp_pf_interface_suite.cpp",
        "minihost/src/worker_aegp_command_suites.hpp",
        "minihost/src/worker_aegp_command_suites.cpp",
        "minihost/src/worker_mask_suite_tables.hpp",
        "minihost/src/worker_mask_suite_tables.cpp",
        "minihost/src/worker_l2_render_abi.hpp",
        "minihost/src/worker_l2_render_abi.cpp",
        "minihost/src/worker_classic_report.hpp",
        "minihost/src/worker_classic_report.cpp",
        "minihost/src/worker_smart_report.hpp",
        "minihost/src/worker_smart_report.cpp",
        "minihost/src/l2_mode_execution.hpp",
        "minihost/src/l2_mode_execution.cpp",
        "minihost/src/l2_cli_dispatch.cpp",
        "minihost/src/worker_l2_suite_abi.hpp",
        "minihost/src/worker_pf_suites_internal.hpp",
        "minihost/src/worker_pf_suites.cpp",
        "minihost/src/worker_pf_sampling_runtime.cpp",
        "minihost/src/worker_pf_ae_channel_runtime.cpp",
        "minihost/src/worker_pf_path_selftests.cpp",
        "minihost/src/worker_pf_world_transform_runtime.cpp",
        "minihost/src/worker_pf_ansi_runtime.cpp",
        "minihost/src/worker_host_suite_catalog.cpp",
        "minihost/src/worker_parameter_execution.cpp",
        "minihost/src/worker_ui_event_execution.hpp",
        "minihost/src/worker_ui_event_execution.cpp",
        "minihost/src/worker_entry_bootstrap.cpp",
        "minihost/src/worker_smart_runtime.cpp",
        "minihost/src/worker_smart_setup.cpp",
        "minihost/src/worker_smart_dispatch.cpp",
        "minihost/src/worker_smart_finalize.cpp",
        "minihost/src/worker_smart_render_runtime.cpp",
        "minihost/src/worker_classic_execution.cpp",
        "minihost/src/worker_aegp_scene.cpp",
        "minihost/src/worker_aegp_scene.hpp",
        "minihost/src/worker_aegp_scene_runtime.hpp",
        "minihost/src/worker_aegp_scene_runtime.cpp",
        "minihost/src/worker_aegp_init_runtime.hpp",
        "minihost/src/worker_aegp_init_runtime.cpp",
        "minihost/src/worker_aegp_init_execution.hpp",
        "minihost/src/worker_aegp_init_execution.cpp",
        "minihost/src/worker_aegp_init_report.hpp",
        "minihost/src/worker_aegp_init_report.cpp",
        "minihost/src/worker_ui_event_report.hpp",
        "minihost/src/worker_ui_event_report.cpp",
        "minihost/src/worker_audio_execution.hpp",
        "minihost/src/worker_audio_execution.cpp",
        "minihost/src/worker_render_session.hpp",
        "minihost/src/worker_render_session.cpp",
        "minihost/src/worker_drawbot_runtime.hpp",
        "minihost/src/worker_drawbot_runtime.cpp",
        "minihost/src/worker_param_checkout_runtime.hpp",
        "minihost/src/worker_param_checkout_runtime.cpp",
        "minihost/src/worker_classic_render_runtime.cpp",
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/worker_l2_shared_helpers.cpp",
        "minihost/src/worker_l2_payload_parsers.cpp",
        "minihost/src/worker_early_mode_bridge.hpp",
        "minihost/src/worker_early_mode_bridge.cpp",
        "minihost/src/worker_entry_wiring.cpp",
        "minihost/src/worker_aegp_timeline_probe.hpp",
        "minihost/src/worker_aegp_timeline_probe.cpp",
        "minihost/src/worker_aegp_host_selftests.cpp",
        "minihost/src/worker_aegp_compat_selftests.cpp",
        "minihost/src/worker_mask_runtime.hpp",
        "minihost/src/worker_mask_runtime.cpp",
        "minihost/src/worker_mask_runtime_callbacks.cpp",
        "minihost/src/worker_mask_selftests.cpp",
        "minihost/src/worker_handle_runtime.hpp",
        "minihost/src/worker_handle_runtime.cpp",
        "minihost/src/worker_report.hpp",
        "minihost/src/worker_report.cpp",
        "minihost/src/worker_runtime_admission.cpp",
        "minihost/src/worker_classic_runtime.hpp",
        "minihost/src/worker_classic_runtime.cpp",
        "minihost/src/worker_selftest_dispatch.cpp",
        "minihost/src/worker_fixed_selftest_routing.cpp",
        "minihost/src/worker_parameter_selftest_routing.cpp",
        "minihost/src/worker_request_parser.hpp",
        "minihost/src/worker_request_parser.cpp",
        "minihost/src/worker_invocation_orchestration.hpp",
        "minihost/src/worker_invocation_orchestration.cpp",
        "minihost/src/worker_render_report.hpp",
        "minihost/src/worker_render_report.cpp",
    ),
    "l2_render_abi": (
        "minihost/src/worker_l2_render_abi.hpp",
        "minihost/src/worker_l2_render_abi.cpp",
    ),
    "classic_report": (
        "minihost/src/worker_classic_report.hpp",
        "minihost/src/worker_classic_report.cpp",
    ),
    "aegp_resizer_3d_chain": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_aegp_scene.cpp",
        "minihost/src/worker_aegp_compat_selftests.cpp",
    ),
    "legacy_effect_compat": (
        # The PF Interface owner precedes l2_main so definition-anchored
        # rindex slices keep resolving into the production bodies.
        "minihost/src/worker_aegp_pf_interface_suite.hpp",
        "minihost/src/worker_aegp_pf_interface_suite.cpp",
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/worker_entry_wiring.cpp",
        "minihost/src/worker_aegp_scene.cpp",
        "minihost/src/worker_aegp_scene.hpp",
        "minihost/src/worker_pf_suites.cpp",
        "minihost/src/worker_pf_suites_internal.hpp",
        "minihost/src/worker_selftest_dispatch.cpp",
        "minihost/src/worker_pf_helper_runtime.cpp",
        "minihost/src/worker_pf_helper_runtime.hpp",
        "minihost/src/worker_host_suite_router.cpp",
    ),
    "pf_ae_adv_item_suite": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/worker_l2_render_abi.hpp",
        "minihost/src/worker_l2_render_abi.cpp",
        "minihost/src/worker_pf_suites.cpp",
        "minihost/src/worker_pf_suites_internal.hpp",
        "minihost/src/worker_world_safety.cpp",
    ),
    "pf_ae_channel_suite": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/worker_pf_suites.cpp",
        "minihost/src/worker_l2_suite_abi.hpp",
        "minihost/src/worker_pf_suites_internal.hpp",
        "minihost/src/worker_pf_ae_channel_runtime.hpp",
        "minihost/src/worker_pf_ae_channel_runtime.cpp",
    ),
    "pf_ansi_suite": (
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_l2_suite_abi.hpp",
        "minihost/src/worker_host_suite_catalog.hpp",
        "minihost/src/worker_pf_ansi_runtime.cpp",
    ),
    "pf_color_suite": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/worker_pf_suites.cpp",
        "minihost/src/worker_pf_suites_internal.hpp",
        "minihost/src/worker_pf_color_selftests.cpp",
    ),
    "pf_effect_sequence_data_suite": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_l2_shared_helpers.cpp",
        "minihost/src/worker_pf_suites_internal.hpp",
        "minihost/src/worker_pf_suites.cpp",
        "minihost/src/worker_pf_state_runtime.hpp",
        "minihost/src/worker_pf_state_runtime.cpp",
        "minihost/src/worker_pf_effect_sequence_selftests.hpp",
        "minihost/src/worker_pf_effect_sequence_selftests.cpp",
    ),
    "pf_fill_matte_legacy_callbacks": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/worker_pf_suites.cpp",
        "minihost/src/worker_l2_suite_abi.hpp",
        "minihost/src/worker_pf_suites_internal.hpp",
        "minihost/src/worker_pf_world_transform_runtime.cpp",
    ),
    "pf_helper_suite2": (
        # The helpers owner precedes l2_main so the definition-anchored
        # invoke_global_setdown slice resolves into the production body.
        "minihost/src/worker_l2_shared_helpers.cpp",
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/worker_l2_suite_abi.hpp",
        "minihost/src/worker_pf_helper_runtime.cpp",
        "minihost/src/worker_pf_helper_runtime.hpp",
        "minihost/src/worker_host_suite_router.cpp",
        "minihost/src/worker_ui_event_execution.cpp",
    ),
    "pf_world_transform_composite_rect": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/worker_l2_suite_abi.hpp",
        "minihost/src/worker_pf_suites.cpp",
        "minihost/src/worker_pf_world_transform_runtime.cpp",
    ),
    "sdk_backwards_audio_result": (
        "minihost/src/l2_main.cpp",
        "minihost/src/l2_cli_dispatch.cpp",
        "minihost/src/host_audio_runtime.hpp",
        "minihost/src/host_audio_runtime.cpp",
        "minihost/src/worker_audio_execution.cpp",
    ),
    "sdk_grabba_update_menu": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/worker_l2_render_abi.hpp",
        "minihost/src/worker_l2_render_abi.cpp",
        "minihost/src/worker_aegp_scene.cpp",
        "minihost/src/worker_aegp_scene.hpp",
        "minihost/src/worker_aegp_layer_render_runtime.cpp",
    ),
    "sdk_pathmaster_hard_edge_result": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/worker_pf_suites.cpp",
        "minihost/src/worker_pf_suites_internal.hpp",
        "minihost/src/worker_l2_suite_abi.hpp",
        "minihost/src/worker_pf_path_runtime.cpp",
        "minihost/src/worker_pf_path_selftests.cpp",
        "minihost/src/worker_pf_world_transform_runtime.cpp",
    ),
    "sdk_shifter_transform_sampling_result": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/worker_classic_render_runtime.cpp",
        "minihost/src/worker_pf_suites.cpp",
        "minihost/src/worker_render_report.cpp",
        "minihost/src/worker_pf_sampling_runtime.cpp",
    ),
    "sdk_transformer_multi_input_result": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/worker_classic_render_runtime.cpp",
        "minihost/src/worker_pf_suites.cpp",
        "minihost/src/worker_pf_world_transform_runtime.cpp",
    ),
    "smartfx_geometry_flags": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_entry_wiring.cpp",
        "minihost/src/render_subsystem.h",
        "minihost/src/render_subsystem.cpp",
        "minihost/src/worker_smart_dispatch.cpp",
        "minihost/src/worker_smart_finalize.cpp",
        "minihost/src/worker_render_report.cpp",
        "minihost/src/worker_smart_report.cpp",
    ),
    "native_depth_image_transport": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_classic_render_runtime.cpp",
        "minihost/src/render_subsystem.cpp",
        "minihost/src/worker_smart_finalize.cpp",
        "minihost/src/worker_render_session.cpp",
    ),
    "aegp_receipt_callbacks": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_l2_render_abi.hpp",
        "minihost/src/worker_l2_render_abi.cpp",
    ),
    "classic_param_checkout": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_classic_render_runtime.cpp",
        "minihost/src/worker_param_checkout_runtime.cpp",
    ),
    "pf_ae_app_suite_complete": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/worker_host_suite_catalog.cpp",
        "minihost/src/worker_drawbot_runtime.cpp",
    ),
    "pf_adv_app_suite": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_host_suite_wiring.cpp",
        "minihost/src/worker_l2_suite_abi.hpp",
        "minihost/src/worker_host_suite_catalog.cpp",
        "minihost/src/worker_host_guard_selftests.cpp",
    ),
    "pf_ae_channel_transport": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_pf_ae_channel_runtime.cpp",
    ),
    "smartfx_suite_fault": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_entry_wiring.cpp",
        "minihost/src/l2_cli_dispatch.cpp",
        "minihost/src/worker_mask_runtime.hpp",
        "minihost/src/worker_mask_runtime.cpp",
        "minihost/src/worker_mask_runtime_callbacks.cpp",
    ),
    "custom_ui_lifecycle": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_invocation_orchestration.cpp",
        "minihost/src/worker_ui_event_execution.cpp",
        "minihost/src/worker_ui_event_report.cpp",
    ),
    "path_parameter_assignment": (
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_parameter_execution.cpp",
    ),
    "runtime_module_authorization": (
        "minihost/src/worker_entry_wiring.cpp",
        "minihost/src/l2_main.cpp",
        "minihost/src/worker_invocation_orchestration.cpp",
        "minihost/src/worker_entry_admission.cpp",
        "minihost/src/worker_entry_bootstrap.cpp",
    ),
    "worker_runtime_admission": (
        "minihost/src/l2_main.cpp",
        "minihost/src/l2_main_support.inc",
        "minihost/src/l2_main_entry.inc",
        "minihost/src/worker_entry_admission.cpp",
    ),
}


def contract_files(name):
    relatives = []
    for relative in CONTRACTS[name]:
        if relative == "minihost/src/l2_main.cpp":
            relatives.extend(L2_TRANSLATION_UNIT_FILES)
        else:
            relatives.append(relative)
    return tuple(ROOT / relative for relative in dict.fromkeys(relatives))


def contract_text(name):
    return "\n".join(
        path.read_text(encoding="utf-8") for path in contract_files(name))


def worker_files():
    return tuple(ROOT / relative for relative in WORKER_RUNTIME_OWNERS)


def worker_text():
    return "\n".join(
        path.read_text(encoding="utf-8") for path in worker_files())


def harness_windows_text():
    return "".join(
        (ROOT / relative).read_text(encoding="utf-8")
        for relative in HARNESS_WINDOWS_OWNERS
    )
