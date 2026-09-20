#pragma once
#include "worker_parameter_runtime.hpp"
#include "worker_render_session.hpp"
#include "worker_request_parser.hpp"
#include "worker_smart_execution.hpp"
#include <array>
#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

namespace aexcompat::l2_detail {
// Outcome of worker_main's resident render-session frame loop
// (docs/RENDER_SESSION_PROTOCOL_2026-07-19.md). Defined here so the final
// dispatch owner can call l2_main's run_render_session across TUs; the
// protocol implementation stays in l2_main.
struct RenderSessionOutcome {
  int32_t setup_error{-1};
  int32_t setdown_error{-1};
  int32_t frames_attempted{0};
  bool protocol_violation{false};
  bool invariant_failure{false};
  // Cluster-session swap failure (quiescence/setdown/unload/audit, closure
  // design §4.1/§7): the worker exits with the dedicated swap exit code.
  bool swap_failure{false};
  int32_t width{0};
  int32_t height{0};
  int32_t rowbytes{0};
  std::string input_hash;
  std::string output_hash;
  bool guards_intact{true};
  int32_t render_error{-1};
};

// SmartFX resident session (protocol v1.1): the shared session mechanics plus
// the last rendered frame's smart result, which feeds the smart completion
// report the same way the one-shot path does. `last` stays default-initialized
// when no frame reached rendering.
struct SmartRenderSessionOutcome {
  RenderSessionOutcome session;
  worker_runtime::smart_execution::Result last;
};
}  // namespace aexcompat::l2_detail
namespace aexcompat::worker_runtime::invocation {
using parameters::RequestedAssignments;
struct InvocationState {
    bool request_mode{};
    bool render_session_mode{};
    // Discovery session (`--discovery-session-v1`, closure-session design
    // §4.2): cluster-manifest-driven parameter inspection over the control
    // pipes; no launch-time plug-in load.
    bool discovery_session_mode{};
    std::wstring cluster_manifest_path;
    bool audio_session_mode{};
    int32_t audio_session_max_samples{};
    int32_t audio_session_channels{1};
    bool smart_force_cpu{};
    bool smart_opencl{};
    bool smart_directx{};
    int32_t external_pixel_bytes{4};
    int image_environment_argc{};
    int image_trailer_argc{};
    int image_argc{};
    int smart_image_environment_argc{};
    int smart_image_trailer_argc{};
    int smart_image_argc{};
    bool image_render_environment{};
    bool image_spatial_context{};
    bool image_mask_context{};
    bool smart_image_render_environment{};
    bool smart_image_spatial_context{};
    bool smart_image_mask_context{};
    bool mask_request_mode{};
    bool mask_scene_request_mode{};
    bool mask_context_request_mode{};
    bool mask_count_error_mode{};
    bool mask_count_crash_mode{};
    bool mask_double_dispose_mode{};
    bool stream_live_value_dispose_mode{};
    bool stream_metadata_ownership_mode{};
    bool keyframe_ownership_mode{};
    bool dynamic_stream_tree_mode{};
    bool aegp_memory_strings_mode{};
    bool suite_release_without_acquire_mode{};
    bool handle_resize_while_locked_mode{};
    bool world_double_dispose_mode{};
    bool world_allocation_limit_mode{};
    bool pixel_format_registry_mode{};
    bool outline_mutation_mode{};
    bool mask_attribute_mode{};
    bool user_changed_mode{};
    bool force_user_changed_diagnostic{};
    // Explicit Effect Controls lifecycle probe. Unlike the headless
    // render/discovery walk, this mode owns a real UI-context dispatch of
    // PF_Cmd_UPDATE_PARAMS_UI for effects that advertise it.
    bool update_params_ui_mode{};
    bool params_only_mode{};
    // Broker-authorized, one-shot recovery for an inspection whose ordinary
    // lifecycle was already proven to crash only in GLOBAL_SETDOWN. It emits
    // the inspected parameter schema and terminates the process without
    // invoking plug-in cleanup; it is never a resident/session mode.
    bool cleanup_contained_params_only_mode{};
    bool runtime_module_authorization_mode{};
    bool external_dependencies_mode{};
    bool do_dialog_mode{};
    bool auto_dialog_mode{};
    bool adjust_cursor_mode{};
    bool draw_event_mode{};
    bool click_event_mode{};
    bool drag_event_mode{};
    bool ui_lifecycle_mode{};
    bool ui_idle_mode{};
    bool ui_keydown_mode{};
    bool ui_mouse_exited_mode{};
    bool ui_event_assignment_mode{};
    bool aegp_update_menu_mode{};
    bool aegp_idle_mode{};
    bool aegp_command_roundtrip_mode{};
    bool aegp_active_idle_roundtrip_mode{};
    bool aegp_keyframe_roundtrip_mode{};
    bool aegp_seek_roundtrip_mode{};
    bool aegp_trim_roundtrip_mode{};
    bool aegp_switch_roundtrip_mode{};
    bool aegp_comp_idle_roundtrip_mode{};
    bool aegp_init_mode{};
    bool aegp_boundary_regression_mode{};
    bool skip_about_mode{};
    bool user_changed_param_requested{};
    int32_t user_changed_param_slot{-1};
    std::array<float, 4> picker_color{1.0f, 0.25f, 0.75f, 0.5f};
    RequestedAssignments user_changed_parameters;
    RequestedAssignments update_params_ui_parameters;
    RequestedAssignments requested_parameters;
    RequestedAssignments ui_event_assignments;

    std::vector<request_parser::LayerInput> external_layers;
    std::vector<float> external_audio;
    int32_t external_width{};
    int32_t external_height{};
    int32_t external_current_time{};
    int32_t external_time_step{1};
    int32_t external_total_time{1};
    uint32_t external_time_scale{1};
    int32_t external_audio_samples{};
    int32_t external_audio_rate{};
    int32_t click_x{101};
    int32_t click_y{101};
    int32_t drag_end_x{101};
    int32_t drag_end_y{101};
    int32_t drag_steps{};
    uint32_t keydown_code{};
    uint32_t keydown_modifiers{};
  };

struct ApplyHooks {
  void (*set_mask_mode)(bool){};
  void (*set_mask_fault)(bool, bool){};
  void (*set_audio_source)(std::vector<float>*, int32_t){};
};
struct L2ModeHooks {
  bool (*parse_parameter_payload)(const wchar_t*, RequestedAssignments&){};
  std::size_t max_params{};
};
int parse_l2_modes(int argc, wchar_t** argv, InvocationState&, const L2ModeHooks&);
void apply_render(const request_parser::WorkerInvocation&, InvocationState&, const ApplyHooks&);
void apply_smart(const request_parser::WorkerInvocation&, InvocationState&, const ApplyHooks&);

// Final render/smart dispatch seam (issue #125 C6). worker_main_impl hands
// case selection and the render invocation chain to this owner; the
// l2_main-private renderers it drives (render_once, run_render_session,
// smart_render_once, invoke_sequence_selector) stay defined in l2_main and
// are reached through cross-TU declarations, so selector order, error
// priority, and stderr stage traces are unchanged.
struct FinalDispatchRequest {
  parameter_execution::EffectEntry entry{};
  parameter_execution::BufferIn* input{};
  parameter_execution::BufferOut* output{};
  const InvocationState* invocation{};
  wchar_t** argv{};
  int32_t params_error{};
  bool image_render_supported{};
  bool depth_supported{};
  bool smart_render_supported{};
  // Cluster-session swap hook (classic and SmartFX resident sessions, issue #405);
  // null on every non-cluster path, where a swap_plugin message stays a
  // protocol violation.
  const worker_render_session::SwapPluginHook* cluster_swap{};
  // PF_OutFlag_AUDIO_EFFECT_ONLY: the classic render session serves such an
  // effect's frames as input passthrough instead of failing the session for
  // having no video selector to dispatch (issue #1048). Appended after the
  // pointer member on purpose - the call sites initialize this aggregate
  // positionally, and a bool inserted before `cluster_swap` would silently
  // swallow the pointer through pointer-to-bool conversion.
  bool audio_effect_only{};
  // Native behavioral seam for the spawned-thread activation boundary. The
  // production caller leaves this null; self-tests substitute a bounded probe
  // for render_once so the thread/context contract can be tested without a
  // loaded AEX or the process-global selector diagnostics it would require.
  void (*concurrent_thread_context_probe)(){};
};

struct ClassicFinalDispatchResult {
  // Non-ASCII case argument; worker_main_impl exits with code 2 unchanged.
  bool case_id_rejected{};
  std::string case_id;
  std::string input_hash;
  std::string output_hash;
  bool guards_intact{};
  int32_t render_width{};
  int32_t render_height{};
  int32_t render_rowbytes{};
  std::array<int32_t, 2> thread_errors{-1, -1};
  std::array<std::string, 2> thread_hashes{};
  std::array<bool, 2> thread_guards{false, false};
  bool concurrent_render{};
  bool persistent_sequence{};
  bool session_protocol_violation{};
  bool session_invariant_failure{};
  // Cluster-session swap failure: dedicated non-zero exit (closure design §7)
  // so the broker can tell a contamination-suspect abort from a crash.
  bool session_swap_failure{};
  bool flattened_sequence{};
  bool copied_flattened_sequence{};
  int32_t persistent_sequence_setup_error{-1};
  int32_t persistent_sequence_setdown_error{-1};
  std::array<int32_t, 2> persistent_frame_errors{-1, -1};
  std::array<std::string, 2> persistent_frame_hashes{};
  int32_t sequence_flatten_error{-1};
  int32_t sequence_resetup_error{-1};
  bool flattened_handle_replaced{};
  bool resetup_handle_replaced{};
  bool flattened_handle_host_disposed{};
  int32_t get_flattened_sequence_data_error{-1};
  bool original_sequence_preserved{};
  int32_t render_error{-1};
};

struct SmartFinalDispatchResult {
  bool case_id_rejected{};
  std::string case_id;
  smart_execution::Result smart;
  bool lifetime_fault_observed{};
  // Resident smart session (protocol v1.1) summary; meaningful only when the
  // invocation ran in session mode. Mirrors ClassicFinalDispatchResult's
  // session fields so worker_main keeps the same 23/24 exit contract.
  bool session_protocol_violation{};
  bool session_invariant_failure{};
  bool session_swap_failure{};
  int32_t session_frames_attempted{};
  int32_t session_sequence_setup_error{-1};
  int32_t session_sequence_setdown_error{-1};
  int32_t session_render_error{-1};
};

ClassicFinalDispatchResult run_classic_final_dispatch(const FinalDispatchRequest&);
SmartFinalDispatchResult run_smart_final_dispatch(const FinalDispatchRequest&);
}  // namespace aexcompat::worker_runtime::invocation

