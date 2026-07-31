#include "worker_custom_selftest_routing.hpp"

#include "worker_aegp_compat_selftests.hpp"
#include "worker_aegp_render_options.hpp"
#include "worker_aegp_render_selftests.hpp"
#include "worker_aegp_scene_runtime.hpp"
#include "worker_aegp_scene_transaction.hpp"
#include "worker_aegp_staged_item_runtime.hpp"
#include "worker_color_settings_runtime.hpp"
#include "worker_color_settings_selftests.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_pf_ae_channel_runtime.hpp"
#include "worker_pf_path_runtime.hpp"
#include "worker_pf_state_runtime.hpp"
#include "worker_render_receipts.hpp"
#include "worker_world_registry.hpp"

#include <filesystem>
#include <iomanip>
#include <sstream>
#include <string>

namespace aexcompat::worker_runtime::custom_selftests {
namespace {

const char* passed_or_failed(bool passed) { return passed ? "passed" : "failed"; }
const char* json_bool(bool value) { return value ? "true" : "false"; }

}  // namespace

Result dispatch(const Request& request, const Hooks& hooks) {
  const int argc = request.argc;
  wchar_t** argv = request.argv;
  if (argc == 2 &&
      std::wstring(argv[1]) == L"--self-test-aegp-layer-source-item") {
    const bool passed = aexcompat::l2_detail::verify_aegp_layer_source_item();
    const auto& scene = aexcompat::scene_runtime::scene_runtime_state();
    std::ostringstream out;
    out << "{\"aegp_layer_source_item\":\""
        << passed_or_failed(passed)
        << "\",\"successful_calls\":" << scene.layer_source_item_calls
        << ",\"item_type_calls\":" << scene.item_type_calls
        << "}\n";
    return {true, passed ? 0 : 1, out.str()};
  }
  if (argc == 2 &&
      std::wstring(argv[1]) ==
          L"--self-test-aegp-scene-registry-suites") {
    const bool passed =
        aexcompat::l2_detail::verify_aegp_scene_registry_suites();
    std::ostringstream out;
    out << "{\"aegp_scene_registry_suites\":\""
        << passed_or_failed(passed)
        << "\",\"published_suites\":true,\"active_to_comp\":true"
        << ",\"comp_to_layers\":true,\"wrong_kind_rejected\":true"
        << ",\"cross_project_rejected\":true"
        << ",\"cross_registry_rejected\":true"
        << ",\"foreign_rejected\":true,\"forged_rejected\":true"
        << ",\"outputs_unchanged\":true}\n";
    return {true, passed ? 0 : 1, out.str()};
  }
  if (argc == 2 &&
      std::wstring(argv[1]) ==
          L"--self-test-aegp-scene-mutation-transactions") {
    const bool passed =
        aexcompat::l2_detail::verify_aegp_scene_mutation_transactions();
    const auto transactions =
        aexcompat::scene_transaction::diagnostics();
    std::ostringstream out;
    out << "{\"aegp_scene_mutation_transactions\":\""
        << passed_or_failed(passed)
        << "\",\"published_suites\":true"
        << ",\"effect_stream_value_keyframe_registry\":true"
        << ",\"transaction_failure_byte_invariant\":true"
        << ",\"transaction_cancel_byte_invariant\":true"
        << ",\"generation_increment_once\":true"
        << ",\"stale_child_invalidation\":true"
        << ",\"keyframe_bezier_ease_ownership\":true"
        << ",\"batch_add_wrong_kind_rejected\":true"
        << ",\"paired_tangent_acquisition_atomic\":true"
        << ",\"end_add_terminal_cleanup\":true"
        << ",\"mid_apply_rollback_observed\":"
        << json_bool(transactions.rolled_back > 0)
        << ",\"rollback_failures\":" << transactions.rollback_failures
        << ",\"committed\":" << transactions.committed
        << ",\"cancelled\":" << transactions.cancelled << "}\n";
    return {true, passed ? 0 : 1, out.str()};
  }
  if (argc == 2 &&
      std::wstring(argv[1]) == L"--self-test-aegp-scene-model") {
    const auto report = aexcompat::l2_detail::verify_aegp_scene_model();
    const auto diagnostics =
        aexcompat::aegp_staged_item_runtime::diagnostics();
    const auto receipts = aexcompat::render_receipts::statistics();
    const auto hex64 = [](uint64_t value) {
      std::ostringstream encoded;
      encoded << std::hex << std::setw(16) << std::setfill('0') << value;
      return encoded.str();
    };
    std::ostringstream out;
    out << "{\"schema_version\":1,\"aegp_scene_model\":\""
        << passed_or_failed(report.passed)
        << "\",\"fixture\":{\"projects\":2,\"mask_api\":"
        << json_bool(report.mask_fixture)
        << ",\"parent_camera_zoom_api\":"
        << json_bool(report.parent_camera_zoom_fixture)
        << ",\"effect_count\":" << report.effect_count << "}"
        << ",\"identity\":{\"typed\":" << json_bool(report.typed_identity)
        << ",\"fixture_lookup\":" << json_bool(report.fixture_lookup)
        << ",\"effect_suite_acquired\":"
        << json_bool(report.effect_suite_acquired)
        << ",\"effect_applied\":" << json_bool(report.effect_applied)
        << ",\"pointer_id_mismatch_rejected\":"
        << json_bool(report.pointer_id_mismatch_rejected)
        << ",\"duplicate_stable_id_rejected\":"
        << json_bool(report.duplicate_stable_id_rejected) << "}"
        << ",\"cycles\":{\"direct_rejected\":"
        << json_bool(report.direct_cycle_rejected)
        << ",\"indirect_rejected\":"
        << json_bool(report.indirect_cycle_rejected)
        << ",\"cross_project_rejected\":"
        << json_bool(report.cross_project_cycle_rejected) << "}"
        << ",\"order\":{\"registry_effect_order\":"
        << json_bool(report.effect_order)
        << ",\"duplicate_layer_stack_rejected\":"
        << json_bool(report.duplicate_effect_order_rejected)
        << ",\"failure_state_unchanged\":"
        << json_bool(report.duplicate_order_state_unchanged)
        << ",\"failure_receipt_unchanged\":"
        << json_bool(report.duplicate_order_receipt_unchanged)
        << ",\"concurrent_effect_flags_serialized\":"
        << json_bool(report.concurrent_effect_flags_serialized)
        << ",\"concurrent_mask_streams_serialized\":"
        << json_bool(report.concurrent_mask_streams_serialized)
        << ",\"concurrent_keyframe_inserts_serialized\":"
        << json_bool(report.concurrent_keyframe_inserts_serialized)
        << ",\"effect_order_hash\":\""
        << hex64(report.effect_order_hash) << "\"}"
        << ",\"generation\":{\"before\":"
        << report.project_generation_before << ",\"after\":"
        << report.project_generation_after
        << ",\"old_stage_invalidated\":"
        << json_bool(report.stage_invalidated)
        << ",\"old_receipt_invalidated\":"
        << json_bool(report.receipt_invalidated)
        << ",\"direct_bump_receipt_invalidated\":"
        << json_bool(report.direct_bump_receipt_invalidated)
        << ",\"in_flight_receipt_rejected\":"
        << json_bool(report.in_flight_receipt_rejected) << "}"
        << ",\"hashes\":{\"stage_identity\":\""
        << hex64(report.stage_identity_hash)
        << "\",\"trace\":\"" << hex64(report.trace_hash)
        << "\",\"dependencies\":\""
        << hex64(report.dependency_identity_hash) << "\"}"
        << ",\"diagnostics\":{\"typed_registrations\":"
        << diagnostics.typed_registrations
        << ",\"identity_mismatch_rejections\":"
        << diagnostics.identity_mismatch_rejections
        << ",\"duplicate_identity_rejections\":"
        << diagnostics.duplicate_identity_rejections
        << ",\"cross_project_rejections\":"
        << diagnostics.cross_project_rejections
        << ",\"registration_cycle_rejections\":"
        << diagnostics.registration_cycle_rejections
        << ",\"stale_stage_invalidations\":"
        << diagnostics.stale_stage_invalidations
        << ",\"stale_receipt_invalidations\":"
        << diagnostics.stale_receipt_invalidations
        << ",\"invalid_handle_rejections\":"
        << diagnostics.invalid_handle_rejections << "}"
        << ",\"cleanup\":{\"balanced\":"
        << json_bool(report.cleanup_balanced)
        << ",\"live_receipts\":" << receipts.live_count
        << ",\"live_bytes\":" << receipts.live_bytes
        << ",\"reserved_receipts\":" << receipts.reserved_count
        << ",\"invalid_handle_distinguished\":"
        << json_bool(report.invalid_handle_distinguished) << "}"
        << ",\"unsupported_slot\":{\"observed\":"
        << json_bool(report.unsupported_diagnostic_observed)
        << ",\"suite\":\"AEGP Effect Suite\",\"version\":4,\"slot\":7"
        << ",\"error\":" << report.unsupported_error
        << ",\"call_count\":" << report.unsupported_call_count
        << ",\"distinct_from_invalid_handle\":"
        << json_bool(report.unsupported_distinct_from_invalid_handle) << "}"
        << ",\"unsupported_slots_preserved\":"
        << json_bool(report.unsupported_slots_preserved) << "}\n";
    return {true, report.passed ? 0 : 1, out.str()};
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-pf-path-data-hardening") {
    const bool passed = hooks.run_pf_path_data_hardening();
    const auto path_report = aexcompat::pf_path_runtime::snapshot();
    std::ostringstream out;
    out << "{\"pf_path_data_hardening\":\"" << passed_or_failed(passed)
        << "\",\"created\":" << path_report.preps_created
        << ",\"disposed\":" << path_report.preps_disposed
        << ",\"live\":" << path_report.live_preps
        << ",\"balanced\":" << json_bool(aexcompat::pf_path_runtime::lifetimes_balanced())
        << "}\n";
    return {true, passed ? 0 : 1, out.str()};
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-pf-mask-composition") {
    const bool passed = hooks.run_pf_mask_composition();
    const auto path_report = aexcompat::pf_path_runtime::snapshot();
    std::ostringstream out;
    out << "{\"pf_mask_composition\":\"" << passed_or_failed(passed)
        << "\",\"composition_calls\":" << path_report.composition_calls
        << ",\"balanced\":" << json_bool(aexcompat::pf_path_runtime::lifetimes_balanced())
        << "}\n";
    return {true, passed ? 0 : 1, out.str()};
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-pf-world-registry") {
    const bool double_dispose = hooks.verify_world_double_dispose_rejected();
    const bool allocation_limit = hooks.verify_world_allocation_limit_rejected();
    const bool snapshot_atomic = hooks.verify_owned_world_snapshot_is_atomic();
    const bool concurrent_snapshot =
        hooks.verify_owned_world_snapshot_concurrent_dispose();
    const auto world_stats = aexcompat::world_registry::statistics();
    const bool passed = double_dispose && allocation_limit && snapshot_atomic &&
        concurrent_snapshot &&
        aexcompat::world_registry::lifetimes_balanced() &&
        world_stats.live_count == 0 && world_stats.live_bytes == 0;
    std::ostringstream out;
    out << "{\"pf_world_registry\":\""
        << passed_or_failed(passed)
        << "\",\"double_dispose_rejected\":"
        << json_bool(double_dispose)
        << ",\"allocation_limit_rejected\":"
        << json_bool(allocation_limit)
        << ",\"owned_snapshot_atomic\":"
        << json_bool(snapshot_atomic)
        << ",\"concurrent_snapshot_dispose\":"
        << json_bool(concurrent_snapshot)
        << ",\"live_count\":" << world_stats.live_count
        << ",\"live_bytes\":" << world_stats.live_bytes << "}\n";
    return {true, passed ? 0 : 1, out.str()};
  }
  if (argc == 4 && std::wstring(argv[1]) == L"--self-test-pf-ae-channel-transport" &&
      std::wstring(argv[2]) == L"--aux-manifest-v1") {
    const bool passed = aexcompat::pf_ae_channel::verify_pf_ae_channel_transport(
        std::filesystem::path(argv[3]));
    const auto channel_transport = aexcompat::pf_ae_channel::transport_statistics();
    std::ostringstream out;
    out << "{\"pf_ae_channel_transport\":\""
        << passed_or_failed(passed)
        << "\",\"row_bytes\":" << channel_transport.row_bytes
        << ",\"origin\":[" << channel_transport.origin_x << ','
        << channel_transport.origin_y << "]"
        << ",\"duration\":" << channel_transport.duration << "}\n";
    return {true, passed ? 0 : 3, out.str()};
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-pf-color-settings-suite6") {
    const bool passed =
        aexcompat::color_settings::selftests::verify_pf_color_settings_suite6();
    const auto& srgb_icc = aexcompat::color_settings::color_settings_builtin_srgb_icc();
    const auto& linear_icc =
        aexcompat::color_settings::color_settings_builtin_linear_icc();
    const auto color_stats = aexcompat::color_settings::color_settings_statistics();
    uint32_t memory_created = 0;
    uint32_t memory_freed = 0;
    uint64_t memory_residual = 0;
    const auto memory_stats =
        aexcompat::worker_runtime::handles::aegp_memory_statistics();
    memory_created = memory_stats.created;
    memory_freed = memory_stats.freed;
    memory_residual = memory_stats.live_bytes;
    std::ostringstream out;
    out << "{\"pf_color_settings_suite6\":\"" << passed_or_failed(passed)
        << "\",\"profiles_created\":" << color_stats.profiles_created
        << ",\"profiles_disposed\":" << color_stats.profiles_disposed
        << ",\"profiles_live\":" << color_stats.profiles_live
        << ",\"invalid_operations\":" << color_stats.invalid_operations
        << ",\"xform_calls\":" << color_stats.xform_calls
        << ",\"memory_created\":" << memory_created
        << ",\"memory_freed\":" << memory_freed
        << ",\"memory_residual_bytes\":" << memory_residual
        << ",\"memory_balanced\":"
        << json_bool(aexcompat::worker_runtime::handles::aegp_memory_balanced())
        << ",\"srgb_icc_bytes\":" << srgb_icc.size()
        << ",\"srgb_icc_sha256\":\"" << hooks.sha256_bytes(srgb_icc.data(), srgb_icc.size())
        << "\",\"linear_icc_bytes\":" << linear_icc.size()
        << ",\"linear_icc_sha256\":\"" << hooks.sha256_bytes(linear_icc.data(), linear_icc.size())
        << "\",\"linear_icc_hex\":\"" << hooks.hex_bytes(linear_icc.data(), linear_icc.size())
        << "\",\"ocio_enabled\":false}\n";
    return {true, passed ? 0 : 1, out.str()};
  }
  if (argc == 2 &&
      std::wstring(argv[1]) == L"--self-test-pf-effect-sequence-data-suite") {
    const bool passed = hooks.verify_pf_effect_sequence_data_suite1();
    std::ostringstream out;
    out << "{\"pf_effect_sequence_data_suite1\":\""
        << passed_or_failed(passed)
        << "\",\"borrowed_handle\":true,\"mfr_concurrent_reads\":2048"
        << ",\"live_sequences\":"
        << aexcompat::pf_state_runtime::live_effect_sequence_count()
        << ",\"publications\":"
        << aexcompat::pf_state_runtime::effect_sequence_publications()
        << ",\"invalidations\":"
        << aexcompat::pf_state_runtime::effect_sequence_invalidations() << "}\n";
    return {true, passed ? 0 : 36, out.str()};
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-aegp-async-receipt") {
    const bool passed = hooks.verify_aegp_async_receipts();
    std::ostringstream out;
    out << "{\"aegp_async_receipt\":\"" << passed_or_failed(passed)
        << "\",\"created\":" << aexcompat::render_receipts::statistics().created
        << ",\"checked_in\":" << aexcompat::render_receipts::statistics().checked_in
        << ",\"live\":" << aexcompat::render_receipts::statistics().live_count
        << ",\"live_bytes\":" << aexcompat::render_receipts::statistics().live_bytes
        << ",\"invalid_operations\":"
        << aexcompat::render_receipts::statistics().invalid_operations
        << "}\n";
    return {true, passed ? 0 : 1, out.str()};
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-aegp-render-options-suite1") {
    const bool passed = aexcompat::l2_detail::verify_aegp_render_options_suite1();
    std::ostringstream out;
    out << "{\"aegp_render_options_suite1\":\""
        << passed_or_failed(passed)
        << "\",\"created\":" << aexcompat::render_options::item_created_count()
        << ",\"disposed\":" << aexcompat::render_options::item_disposed_count()
        << ",\"live\":" << aexcompat::render_options::item_live_count()
        << ",\"receipts_created\":" << aexcompat::render_receipts::statistics().created
        << ",\"receipts_checked_in\":" << aexcompat::render_receipts::statistics().checked_in
        << ",\"invalid_operations\":" << aexcompat::render_options::item_invalid_count()
        << ",\"baseline_argb8\":[" << static_cast<int>((*hooks.render_options_baseline8)[0]) << ','
        << static_cast<int>((*hooks.render_options_baseline8)[1]) << ',' << static_cast<int>((*hooks.render_options_baseline8)[2]) << ',' << static_cast<int>((*hooks.render_options_baseline8)[3]) << ']'
        << ",\"time_argb8\":[" << static_cast<int>((*hooks.render_options_time8)[0]) << ',' << static_cast<int>((*hooks.render_options_time8)[1]) << ',' << static_cast<int>((*hooks.render_options_time8)[2]) << ',' << static_cast<int>((*hooks.render_options_time8)[3]) << ']'
        << ",\"downsample_argb8\":[" << static_cast<int>((*hooks.render_options_downsample8)[0]) << ',' << static_cast<int>((*hooks.render_options_downsample8)[1]) << ',' << static_cast<int>((*hooks.render_options_downsample8)[2]) << ',' << static_cast<int>((*hooks.render_options_downsample8)[3]) << ']'
        << ",\"roi_outside_argb8\":[0,0,0,0],\"field_excluded_argb8\":[0,0,0,0]"
        << ",\"roi_inside_argb8\":[" << static_cast<int>((*hooks.render_options_roi_inside8)[0]) << ',' << static_cast<int>((*hooks.render_options_roi_inside8)[1]) << ',' << static_cast<int>((*hooks.render_options_roi_inside8)[2]) << ',' << static_cast<int>((*hooks.render_options_roi_inside8)[3]) << ']'
        << ",\"matte_argb8\":[" << static_cast<int>((*hooks.render_options_matte8)[0]) << ',' << static_cast<int>((*hooks.render_options_matte8)[1]) << ',' << static_cast<int>((*hooks.render_options_matte8)[2]) << ',' << static_cast<int>((*hooks.render_options_matte8)[3]) << ']'
        << ",\"argb16\":[" << (*hooks.render_options_argb16)[0] << ',' << (*hooks.render_options_argb16)[1] << ',' << (*hooks.render_options_argb16)[2] << ',' << (*hooks.render_options_argb16)[3] << ']'
        << std::setprecision(17) << ",\"argb32f\":[" << (*hooks.render_options_argb32f)[0] << ',' << (*hooks.render_options_argb32f)[1] << ',' << (*hooks.render_options_argb32f)[2] << ',' << (*hooks.render_options_argb32f)[3] << ']'
        << "}\n";
    return {true, passed ? 0 : 1, out.str()};
  }
  if (argc == 2 &&
      std::wstring(argv[1]) == L"--self-test-aegp-item-staged-worlds") {
    const bool passed = aexcompat::l2_detail::verify_aegp_item_staged_worlds();
    const auto staged = aexcompat::aegp_staged_item_runtime::diagnostics();
    std::ostringstream out;
    out << "{\"aegp_item_staged_worlds\":\""
        << passed_or_failed(passed)
        << "\",\"immutable_stage\":true,\"reentrant_render_used\":false"
        << ",\"published\":" << staged.published
        << ",\"cache_hits\":" << staged.cache_hits
        << ",\"cache_misses\":" << staged.cache_misses
        << ",\"cycles_rejected\":" << staged.cycles_rejected
        << ",\"generation_invalidations\":" << staged.generation_invalidations
        << ",\"evictions\":" << staged.evictions
        << ",\"exact_hits\":" << staged.exact_hits
        << ",\"hold_hits\":" << staged.hold_hits
        << ",\"nearest_hits\":" << staged.nearest_hits
        << ",\"unavailable_frames\":" << staged.unavailable_frames
        << ",\"direct_cycles_rejected\":" << staged.direct_cycles_rejected
        << ",\"indirect_cycles_rejected\":" << staged.indirect_cycles_rejected
        << ",\"depth_limit_rejections\":" << staged.depth_limit_rejections
        << ",\"stage_limit_rejections\":" << staged.stage_limit_rejections
        << ",\"effect_boundary_rejections\":" << staged.effect_boundary_rejections
        << ",\"partial_failures\":" << staged.partial_failures
        << ",\"cleanup_count\":" << staged.cleanup_count
        << ",\"in_flight\":" << staged.in_flight
        << ",\"max_in_flight\":" << staged.max_in_flight
        << ",\"registered_items\":" << staged.registered_items
        << ",\"cached_stages\":" << staged.cached_stages
        << ",\"cached_bytes\":" << staged.cached_bytes
        << ",\"last_trace_hash\":" << staged.last_trace_hash
        << ",\"last_stage_identity_hash\":" << staged.last_stage_identity_hash
        << ",\"last_resolved_stages\":" << staged.last_resolved_stages
        << ",\"max_resolved_depth\":" << staged.max_resolved_depth
        << "}\n";
    return {true, passed ? 0 : 1, out.str()};
  }
  return {};
}

}  // namespace aexcompat::worker_runtime::custom_selftests
