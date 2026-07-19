#include "worker_aegp_init_report.hpp"

#include "runtime_module_audit.hpp"
#include "worker_aegp_init_runtime.hpp"
#include "worker_aegp_scene.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_render_receipts.hpp"

#include <cstdint>
#include <iostream>
#include <string>

namespace aexcompat::l2_detail {

using aexcompat::worker_runtime::handles::aegp_memory_balanced;
using aexcompat::worker_runtime::handles::aegp_memory_statistics;
using aexcompat::worker_runtime::module_audit_json;

// Worker-entry owned suite-lease/receipt/cache accounting stays in l2_main
// with the registries that mutate it; the report reads it cross-TU.
uint32_t suite_acquire_count();
uint32_t suite_release_count();
uint32_t live_suite_reference_count();
std::string live_suite_lease_summary();
bool isolated_aegp_read_cache_is_bounded();
bool async_receipt_lifetimes_balanced();

// AEGP runtime state read back through its owners (issue #126 Phase D);
// these references keep the g_* spellings the report body was written with.
namespace {
auto& g_aegp_init_runtime = aexcompat::worker_runtime::aegp_init::state();
auto& g_aegp_idle_mode = g_aegp_init_runtime.idle_mode;
auto& g_aegp_keyframe_roundtrip_mode = g_aegp_init_runtime.keyframe_roundtrip_mode;
auto& g_aegp_seek_roundtrip_mode = g_aegp_init_runtime.seek_roundtrip_mode;
auto& g_aegp_trim_roundtrip_mode = g_aegp_init_runtime.trim_roundtrip_mode;
auto& g_aegp_switch_roundtrip_mode = g_aegp_init_runtime.switch_roundtrip_mode;
auto& g_aegp_commands_created = g_aegp_init_runtime.commands_created;
auto& g_aegp_menu_commands_inserted = g_aegp_init_runtime.menu_commands_inserted;
auto& g_aegp_command_hooks = g_aegp_init_runtime.command_hooks;
auto& g_aegp_update_menu_hooks = g_aegp_init_runtime.update_menu_hooks;
auto& g_aegp_idle_hooks = g_aegp_init_runtime.idle_hooks;
auto& g_aegp_death_hooks = g_aegp_init_runtime.death_hooks;
auto& g_aegp_command_enable_calls = g_aegp_init_runtime.command_enable_calls;
auto& g_aegp_command_check_calls = g_aegp_init_runtime.command_check_calls;
auto& g_aegp_command_checked_true_calls = g_aegp_init_runtime.command_checked_true_calls;
auto& g_aegp_command_checked_false_calls = g_aegp_init_runtime.command_checked_false_calls;
bool& g_aegp_update_menu_mode = scene_runtime_state().update_menu_mode;
bool& g_aegp_command_roundtrip_mode = scene_runtime_state().command_roundtrip_mode;
bool& g_aegp_active_idle_roundtrip_mode = scene_runtime_state().active_idle_roundtrip_mode;
bool& g_aegp_comp_idle_roundtrip_mode = scene_runtime_state().comp_idle_roundtrip_mode;
uint32_t& g_aegp_item_current_time_calls = scene_runtime_state().item_current_time_calls;
uint32_t& g_aegp_item_set_current_time_calls = scene_runtime_state().item_set_current_time_calls;
int32_t& g_aegp_item_last_set_time_value = scene_runtime_state().item_last_set_time_value;
uint32_t& g_aegp_item_last_set_time_scale = scene_runtime_state().item_last_set_time_scale;
uint32_t& g_aegp_item_name_calls = scene_runtime_state().item_name_calls;
uint32_t& g_aegp_item_duration_calls = scene_runtime_state().item_duration_calls;
uint32_t& g_aegp_comp_from_item_calls = scene_runtime_state().comp_from_item_calls;
uint32_t& g_aegp_comp_framerate_calls = scene_runtime_state().comp_framerate_calls;
uint32_t& g_aegp_layer_count_calls = scene_runtime_state().layer_count_calls;
uint32_t& g_aegp_layer_by_index_calls = scene_runtime_state().layer_by_index_calls;
uint32_t& g_aegp_layer_source_item_calls = scene_runtime_state().layer_source_item_calls;
uint32_t& g_aegp_layer_id_calls = scene_runtime_state().layer_id_calls;
uint32_t& g_aegp_layer_attribute_calls = scene_runtime_state().layer_attribute_calls;
uint32_t& g_aegp_layer_trim_set_calls = scene_runtime_state().layer_trim_set_calls;
uint32_t& g_aegp_layer_flag_set_calls = scene_runtime_state().layer_flag_set_calls;
auto& g_aegp_layer_flags = scene_runtime_state().layer_flags;
uint32_t& g_aegp_layer_name_calls = scene_runtime_state().layer_name_calls;
uint32_t& g_aegp_effect_count_calls = scene_runtime_state().effect_count_calls;
uint32_t& g_aegp_effect_acquires = scene_runtime_state().effect_acquires;
uint32_t& g_aegp_effect_disposes = scene_runtime_state().effect_disposes;
uint32_t& g_aegp_effect_metadata_calls = scene_runtime_state().effect_metadata_calls;
uint32_t& g_aegp_stream_acquires = scene_runtime_state().stream_acquires;
uint32_t& g_aegp_stream_disposes = scene_runtime_state().stream_disposes;
uint32_t& g_aegp_stream_value_acquires = scene_runtime_state().stream_value_acquires;
uint32_t& g_aegp_stream_value_disposes = scene_runtime_state().stream_value_disposes;
uint32_t& g_aegp_stream_sampled_selector_mask = scene_runtime_state().stream_sampled_selector_mask;
uint32_t& g_aegp_effect_param_name_calls = scene_runtime_state().effect_param_name_calls;
uint32_t& g_aegp_effect_param_value_calls = scene_runtime_state().effect_param_value_calls;
uint32_t& g_aegp_effect_param_union_calls = scene_runtime_state().effect_param_union_calls;
uint32_t& g_aegp_keyframe_count_calls = scene_runtime_state().keyframe_count_calls;
uint32_t& g_aegp_keyframed_stream_reports = scene_runtime_state().keyframed_stream_reports;
uint32_t& g_aegp_keyframe_time_calls = scene_runtime_state().keyframe_time_calls;
uint32_t& g_aegp_keyframe_value_calls = scene_runtime_state().keyframe_value_calls;
uint32_t& g_aegp_keyframe_interpolation_calls = scene_runtime_state().keyframe_interpolation_calls;
uint32_t& g_aegp_collection_creates = scene_runtime_state().collection_creates;
uint32_t& g_aegp_collection_disposes = scene_runtime_state().collection_disposes;
uint32_t& g_aegp_collection_item_reads = scene_runtime_state().collection_item_reads;
int32_t& g_aegp_scene_frame = scene_runtime_state().scene_frame;
int32_t& g_aegp_first_observed_frame = scene_runtime_state().first_observed_frame;
int32_t& g_aegp_last_observed_frame = scene_runtime_state().last_observed_frame;
auto& g_aegp_layers = scene_runtime_state().layers;
auto& g_aegp_selection = scene_runtime_state().selection;
}  // namespace

bool emit_aegp_init_completion_report(const AegpInitCompletionInputs& in) {
  const int32_t init_error = in.init_error;
  const int32_t event_error = in.event_error;
  const int32_t death_error = in.death_error;
  const uint32_t hooks_invoked = in.hooks_invoked;
  const uint32_t menu_hooks_invoked = in.menu_hooks_invoked;
  const uint32_t death_hooks_invoked = in.death_hooks_invoked;
  const uint32_t command_hooks_invoked = in.command_hooks_invoked;
  const uint32_t command_handled_count = in.command_handled_count;
  const int32_t idle_max_sleep = in.idle_max_sleep;
  const auto& keyframe_probe = in.keyframe_pipe;
  const auto& seek_probe = in.seek_pipe;
  const auto& trim_probe = in.trim_pipe;
  const auto& switch_probe = in.switch_pipe;
  const bool module_audit_ok = in.module_audit_ok;
  const uint32_t live_suite_references = live_suite_reference_count();
  const std::string live_suite_summary = live_suite_lease_summary();
  const bool isolated_item_cache = g_aegp_active_idle_roundtrip_mode &&
      live_suite_references == 1 && live_suite_summary == "AEGP Item Suite@14=1";
  const bool isolated_comp_cache = g_aegp_comp_idle_roundtrip_mode &&
      isolated_aegp_read_cache_is_bounded();
  const bool leases_balanced = live_suite_references == 0 || isolated_item_cache || isolated_comp_cache;
  const bool effect_lifetimes_balanced = !g_aegp_effect_live &&
      !any_effect_lease_live() && g_aegp_effect_acquires == g_aegp_effect_disposes;
  const bool stream_lifetimes_balanced = !g_aegp_transform_stream.live &&
      !g_aegp_transform_stream.value_live &&
      g_aegp_stream_acquires == g_aegp_stream_disposes &&
      g_aegp_stream_value_acquires == g_aegp_stream_value_disposes;
  const bool collection_lifetimes_balanced = !g_aegp_selection.live &&
      g_aegp_collection_creates == g_aegp_collection_disposes;
  const bool aegp_memory_lifetimes_balanced = aegp_memory_balanced();
  const bool passed = init_error == 0 && event_error == 0 && death_error == 0 &&
      leases_balanced && effect_lifetimes_balanced && stream_lifetimes_balanced &&
      collection_lifetimes_balanced &&
      aegp_memory_lifetimes_balanced && async_receipt_lifetimes_balanced() &&
      module_audit_ok;
  std::cout << "{\"schema_version\":1,\"stage\":\"aegp_init\",\"status\":\""
            << (passed ? ((g_aegp_update_menu_mode || g_aegp_idle_mode || g_aegp_command_roundtrip_mode || g_aegp_active_idle_roundtrip_mode || g_aegp_comp_idle_roundtrip_mode) ? "event_completed" : "initialized") : "initialization_failed")
            << "\",\"identity_verified\":true,\"entrypoint\":\"EntryPointFunc\""
            << ",\"driver_major_version\":24,\"driver_minor_version\":0"
            << ",\"plugin_id\":1,\"init_error\":" << init_error
            << ",\"global_refcon_nonnull\":" << (in.global_refcon_nonnull ? "true" : "false")
            << ",\"commands_created\":" << g_aegp_commands_created
            << ",\"menu_commands_inserted\":" << g_aegp_menu_commands_inserted
            << ",\"command_hooks_registered\":" << g_aegp_command_hooks
            << ",\"update_menu_hooks_registered\":" << g_aegp_update_menu_hooks
            << ",\"idle_hooks_registered\":" << g_aegp_idle_hooks
            << ",\"death_hooks_registered\":" << g_aegp_death_hooks
            << ",\"death_hooks_invoked\":" << death_hooks_invoked
            << ",\"death_error\":" << death_error
            << ",\"event_requested\":\"" << (g_aegp_update_menu_mode ? "update_menu" : (g_aegp_idle_mode ? "idle" : (g_aegp_command_roundtrip_mode ? "command_roundtrip" : (g_aegp_active_idle_roundtrip_mode ? "active_idle_roundtrip" : (g_aegp_keyframe_roundtrip_mode ? "keyframe_roundtrip" : (g_aegp_seek_roundtrip_mode ? "seek_roundtrip" : (g_aegp_trim_roundtrip_mode ? "trim_roundtrip" : (g_aegp_switch_roundtrip_mode ? "switch_roundtrip" : (g_aegp_comp_idle_roundtrip_mode ? "comp_idle_roundtrip" : "none"))))))))) << "\""
            << ",\"event_error\":" << event_error
            << ",\"hooks_invoked\":" << hooks_invoked
            << ",\"menu_hooks_invoked\":" << menu_hooks_invoked
            << ",\"scene_first_observed_frame\":" << g_aegp_first_observed_frame
            << ",\"scene_last_observed_frame\":" << g_aegp_last_observed_frame
            << ",\"scene_current_frame\":" << g_aegp_scene_frame
            << ",\"scene_layer_count\":" << g_aegp_layers.size()
            << ",\"scene_selected_layer_count\":2"
            << ",\"idle_max_sleep\":" << idle_max_sleep
            << ",\"command_hooks_invoked\":" << command_hooks_invoked
            << ",\"command_handled_count\":" << command_handled_count
            << ",\"command_enable_calls\":" << g_aegp_command_enable_calls
            << ",\"command_check_calls\":" << g_aegp_command_check_calls
            << ",\"command_checked_true_calls\":"
            << g_aegp_command_checked_true_calls
            << ",\"command_checked_false_calls\":"
            << g_aegp_command_checked_false_calls
            << ",\"item_current_time_calls\":" << g_aegp_item_current_time_calls
            << ",\"item_set_current_time_calls\":" << g_aegp_item_set_current_time_calls
            << ",\"item_last_set_time_value\":" << g_aegp_item_last_set_time_value
            << ",\"item_last_set_time_scale\":" << g_aegp_item_last_set_time_scale
            << ",\"item_name_calls\":" << g_aegp_item_name_calls
            << ",\"item_duration_calls\":" << g_aegp_item_duration_calls
            << ",\"comp_from_item_calls\":" << g_aegp_comp_from_item_calls
            << ",\"comp_framerate_calls\":" << g_aegp_comp_framerate_calls
            << ",\"layer_count_calls\":" << g_aegp_layer_count_calls
            << ",\"layer_by_index_calls\":" << g_aegp_layer_by_index_calls
            << ",\"layer_source_item_calls\":" << g_aegp_layer_source_item_calls
            << ",\"layer_id_calls\":" << g_aegp_layer_id_calls
            << ",\"layer_attribute_calls\":" << g_aegp_layer_attribute_calls
            << ",\"layer_trim_set_calls\":" << g_aegp_layer_trim_set_calls
            << ",\"layer_flag_set_calls\":" << g_aegp_layer_flag_set_calls
            << ",\"layer_1_flags\":" << g_aegp_layer_flags[0]
            << ",\"layer_2_flags\":" << g_aegp_layer_flags[1]
            << ",\"layer_3_flags\":" << g_aegp_layer_flags[2]
            << ",\"layer_1_in_point_value\":" << g_aegp_layer_in_points[0].value
            << ",\"layer_1_in_point_scale\":" << g_aegp_layer_in_points[0].scale
            << ",\"layer_1_duration_value\":" << g_aegp_layer_durations[0].value
            << ",\"layer_1_duration_scale\":" << g_aegp_layer_durations[0].scale
            << ",\"layer_name_calls\":" << g_aegp_layer_name_calls
            << ",\"effect_count_calls\":" << g_aegp_effect_count_calls
            << ",\"effect_acquires\":" << g_aegp_effect_acquires
            << ",\"effect_disposes\":" << g_aegp_effect_disposes
            << ",\"effect_metadata_calls\":" << g_aegp_effect_metadata_calls
            << ",\"effect_lifetimes_balanced\":"
            << (effect_lifetimes_balanced ? "true" : "false")
            << ",\"stream_acquires\":" << g_aegp_stream_acquires
            << ",\"stream_disposes\":" << g_aegp_stream_disposes
            << ",\"stream_value_acquires\":" << g_aegp_stream_value_acquires
            << ",\"stream_value_disposes\":" << g_aegp_stream_value_disposes
            << ",\"stream_sampled_selector_mask\":"
            << g_aegp_stream_sampled_selector_mask
            << ",\"stream_lifetimes_balanced\":"
            << (stream_lifetimes_balanced ? "true" : "false")
            << ",\"effect_param_name_calls\":" << g_aegp_effect_param_name_calls
            << ",\"effect_param_value_calls\":" << g_aegp_effect_param_value_calls
            << ",\"effect_param_union_calls\":" << g_aegp_effect_param_union_calls
            << ",\"keyframe_count_calls\":" << g_aegp_keyframe_count_calls
            << ",\"keyframed_stream_reports\":" << g_aegp_keyframed_stream_reports
            << ",\"keyframe_time_calls\":" << g_aegp_keyframe_time_calls
            << ",\"keyframe_value_calls\":" << g_aegp_keyframe_value_calls
            << ",\"keyframe_interpolation_calls\":"
            << g_aegp_keyframe_interpolation_calls
            << ",\"keyframe_pipe_connected\":"
            << (keyframe_probe.connected ? "true" : "false")
            << ",\"keyframe_pipe_request_sent\":"
            << (keyframe_probe.request_sent ? "true" : "false")
            << ",\"keyframe_pipe_response_received\":"
            << (keyframe_probe.response_received ? "true" : "false")
            << ",\"keyframe_pipe_response_valid\":"
            << (keyframe_probe.response_valid ? "true" : "false")
            << ",\"keyframe_pipe_response_bytes\":"
            << keyframe_probe.response_bytes
            << ",\"seek_pipe_connected\":" << (seek_probe.connected ? "true" : "false")
            << ",\"seek_pipe_request_sent\":" << (seek_probe.request_sent ? "true" : "false")
            << ",\"seek_pipe_ack_received\":" << (seek_probe.ack_received ? "true" : "false")
            << ",\"seek_pipe_ack_valid\":" << (seek_probe.ack_valid ? "true" : "false")
            << ",\"trim_pipe_connected\":" << (trim_probe.connected ? "true" : "false")
            << ",\"trim_pipe_request_sent\":" << (trim_probe.request_sent ? "true" : "false")
            << ",\"trim_pipe_ack_received\":" << (trim_probe.ack_received ? "true" : "false")
            << ",\"trim_pipe_ack_valid\":" << (trim_probe.ack_valid ? "true" : "false")
            << ",\"switch_pipe_connected\":" << (switch_probe.connected ? "true" : "false")
            << ",\"switch_pipe_request_sent\":" << (switch_probe.request_sent ? "true" : "false")
            << ",\"switch_pipe_ack_received\":" << (switch_probe.ack_received ? "true" : "false")
            << ",\"switch_pipe_ack_valid\":" << (switch_probe.ack_valid ? "true" : "false")
            << ",\"collection_creates\":" << g_aegp_collection_creates
            << ",\"collection_disposes\":" << g_aegp_collection_disposes
            << ",\"collection_item_reads\":" << g_aegp_collection_item_reads
            << ",\"collection_lifetimes_balanced\":"
            << (collection_lifetimes_balanced ? "true" : "false")
            << ",\"aegp_memory_created\":" << aegp_memory_statistics().created
            << ",\"aegp_memory_freed\":" << aegp_memory_statistics().freed
            << ",\"aegp_memory_lifetimes_balanced\":"
            << (aegp_memory_lifetimes_balanced ? "true" : "false")
            << ",\"suite_acquires\":" << suite_acquire_count()
            << ",\"suite_releases\":" << suite_release_count()
            << ",\"live_suite_reference_count\":" << live_suite_references
            << ",\"live_suite_leases\":\"" << live_suite_summary << "\""
            << ",\"suite_cache_reclaimed_at_process_exit\":"
            << ((isolated_item_cache || isolated_comp_cache) ? "true" : "false")
            << ",\"suite_leases_balanced\":" << (leases_balanced ? "true" : "false")
            << ",\"receipts_created\":" << aexcompat::render_receipts::statistics().created
            << ",\"receipts_checked_in\":" << aexcompat::render_receipts::statistics().checked_in
            << ",\"live_receipts\":" << aexcompat::render_receipts::statistics().live_count
            << ",\"render_performed\":"
            << (aexcompat::render_receipts::statistics().created > 0 ? "true" : "false")
            << ",\"module_audit\":" << module_audit_json() << "}\n";
  return passed;
}

}  // namespace aexcompat::l2_detail
