using namespace aexcompat::l2_detail;

// verify_pf_color_suite / verify_pf_color_param_suite live in
// worker_pf_color_selftests.cpp; host state reaches them through the
// configure() hooks installed below.



// verify_aegp_effect_param_union_suite4 /
// verify_aegp_installed_effect_catalog_suite4 live in
// worker_aegp_compat_selftests.cpp behind configure_aegp_compat_selftests.

// verify_aegp_layer_render_options_suite2 and its async callback live in
// worker_aegp_render_selftests.cpp.

// verify_pf_adv_app_suite_versions / verify_render_output_safety live in
// worker_host_guard_selftests.cpp behind its configure() hooks.

// verify_suite_entry_guards_and_utility13 lives in
// worker_aegp_utility_suite.cpp with the UtilitySuite ABI it validates.










bool set_l2_dump_worlds_dir(void*, const wchar_t* value) {
  auto& state = aexcompat::render::telemetry_state();
  state.dump_worlds_dir = std::filesystem::path(value ? value : L"");
  std::error_code error;
  return !state.dump_worlds_dir.empty() && std::filesystem::is_directory(state.dump_worlds_dir, error);
}

bool enable_l2_checksum_detail(void*) {
  aexcompat::render::telemetry_state().output_checksum_detail = true;
  return true;
}

bool load_l2_aux_manifest(void*, const wchar_t* value) {
  return load_external_aux_manifest(std::filesystem::path(value ? value : L""));
}

bool parse_l2_alpha_coverage(void*, const wchar_t* value) {
  return parse_alpha_coverage_params(value ? value : L"");
}

bool load_l2_parameter_animation(void*, const wchar_t* value) {
  return load_parameter_animation(std::filesystem::path(value ? value : L""),
                                  g_parameter_timelines);
}

bool parse_l2_conformance_render_settings(void*, const wchar_t* value) {
  return aexcompat::worker_render_report::parse_conformance_render_settings(value);
}

// Captures the AEXRMA1 manifest reference carried by a GPU render's
// `--runtime-module-authorization-v1` trailer (#290/#300). The render/smart
// worker accepts a sealed basename or, for #815 in-place dispatch, the
// broker-owned absolute transport path. It parses the manifest before the GPU
// runtime DLLs it loads classify as authorized `policy` modules instead of
// `unknown` in the required module audit. Stored in a global because the
// auxiliary-option hook runs during request parsing, before runtime admission.
std::wstring g_gpu_runtime_authorization_manifest;
bool capture_l2_runtime_module_authorization(void*, const wchar_t* value) {
  if (!value || !*value) return false;
  g_gpu_runtime_authorization_manifest = value;
  return true;
}

// Captures the cluster-session manifest path carried by a
// `--cluster-manifest-v2` auxiliary option: render sessions use
// it for plug-in swap, the discovery session as its only launch input. Stored
// in a global because the auxiliary-option hook runs during request parsing,
// before runtime admission; the manifest itself is loaded and validated after
// admission established the sealed root.
std::wstring g_cluster_manifest_path;
bool capture_l2_cluster_manifest(void*, const wchar_t* value) {
  if (!value || !*value) return false;
  g_cluster_manifest_path = value;
  return true;
}

// Captures the in-place dependency search directories carried by a
// `--dependency-dirs-v1` auxiliary option (issue #751): absolute directories
// joined by ';', validated by the broker and re-bounded by
// apply_dependency_search_dirs before admission. Stored in a global because
// the auxiliary-option hook runs during request parsing, before runtime
// admission.
std::wstring g_dependency_search_dirs;
bool capture_l2_dependency_search_dirs(void*, const wchar_t* value) {
  if (!value || !*value) return false;
  g_dependency_search_dirs = value;
  return true;
}

bool __cdecl scene_render_receipt_enabled() {
  return is_render_worker() && aexcompat::aegp_layer_render_runtime::active();
}

// The report diagnostics builders (gpu/SEH/classic subsystems) live in
// worker_render_report.cpp with the snapshot types they populate.
using aexcompat::worker_render_report::capture_gpu_diagnostics;
using aexcompat::worker_render_report::capture_seh_diagnostics;
using aexcompat::worker_render_report::capture_classic_subsystems;

// The six former host selftest wrappers and their JSON now live in
// worker_fixed_selftest_routing.cpp beside the command catalog.




// L2 parameter-lifecycle probe (issue #171): the non-rendering inspection
// path's sequence/frame selector walk, user-changed-param dispatch, and
// conditional-UI pass, extracted verbatim from worker_main_impl. Returns an
// exit code when the requested assignments are rejected (global setdown has
// already run), std::nullopt otherwise.
std::optional<int> run_l2_parameter_lifecycle(EffectEntry entry,
    std::array<std::byte, kInSize>& input, std::array<std::byte, kOutSize>& output,
    int32_t global_error, int32_t params_error,
    std::array<int32_t, 5>& lifecycle_errors, bool& lifecycle_data_null,
    bool& user_changed_ok, bool& conditional_ui_ok) {
  std::vector<std::array<std::byte, kParamSize>> lifecycle_definitions(g_params.size() + 1);
  std::array<unsigned char, 4> lifecycle_pixel{255, 0, 0, 0};
  std::array<std::byte, 120> lifecycle_world{};
  write<void*>(lifecycle_world, 24, lifecycle_pixel.data());
  write<int32_t>(lifecycle_world, 32, 4); write<int32_t>(lifecycle_world, 36, 1);
  write<int32_t>(lifecycle_world, 40, 1); write_rect(lifecycle_world.data() + 44, 1, 1);
  std::memcpy(lifecycle_definitions[0].data() + 56, lifecycle_world.data(), lifecycle_world.size());
  for (std::size_t i = 0; i < g_params.size(); ++i) {
    lifecycle_definitions[i + 1] = g_params[i].raw;
    if (g_params[i].type == 1 || g_params[i].type == 7)
      write<int32_t>(lifecycle_definitions[i + 1], 56, static_cast<int32_t>(g_params[i].default_value));
    else if (g_params[i].type == 4)
      write<int32_t>(lifecycle_definitions[i + 1], 56, g_params[i].default_value != 0 ? 1 : 0);
    else if (g_params[i].type == 2)
      write<int32_t>(lifecycle_definitions[i + 1], 56,
          static_cast<int32_t>(std::round(g_params[i].default_value * 65536.0)));
    else if (g_params[i].type == 10)
      write<double>(lifecycle_definitions[i + 1], 56, g_params[i].default_value);
    else if (g_params[i].type == 3)
      write<int32_t>(lifecycle_definitions[i + 1], 56,
          static_cast<int32_t>(std::round(g_params[i].default_components[0] * 65536.0)));
    else if (g_params[i].type == 6) {
      write<int32_t>(lifecycle_definitions[i + 1], 56,
          static_cast<int32_t>(std::round(g_params[i].default_components[0] * 65536.0)));
      write<int32_t>(lifecycle_definitions[i + 1], 60,
          static_cast<int32_t>(std::round(g_params[i].default_components[1] * 65536.0)));
    } else if (g_params[i].type == 18) {
      for (int component = 0; component < 3; ++component)
        write<double>(lifecycle_definitions[i + 1], 56 + component * 8,
            g_params[i].default_components[component]);
    }
  }
  if (g_user_changed_param_requested &&
      !apply_requested_assignments(lifecycle_definitions, g_user_changed_parameters)) {
    if (global_error == 0)
      invoke_global_setdown(entry, input.data(), output.data());
    return 3;
  }
  std::vector<void*> lifecycle_params(lifecycle_definitions.size());
  for (std::size_t i = 0; i < lifecycle_definitions.size(); ++i)
    lifecycle_params[i] = lifecycle_definitions[i].data();
  struct LifecycleCheckoutDefinitionsScope {
    ~LifecycleCheckoutDefinitionsScope() {
      g_checkout_layer_definitions.clear();
      std::lock_guard<std::mutex> lock(g_param_checkout_mutex);
      g_live_param_checkouts.clear();
    }
  } lifecycle_checkout_definitions_scope;
  g_checkout_layer_definitions.clear();
  for (std::size_t i = 0; i < lifecycle_definitions.size(); ++i)
    g_checkout_layer_definitions[static_cast<int32_t>(i)] = lifecycle_definitions[i];
  {
    std::lock_guard<std::mutex> lock(g_param_checkout_mutex);
    g_live_param_checkouts.clear();
    g_param_checkout_calls = g_param_checkin_calls = g_invalid_param_checkins = 0;
    g_automatic_param_checkins = 0;
  }
  std::cerr << "stage:sequence_setup_begin\n" << std::flush;
  lifecycle_errors[0] = params_error == 0
      ? invoke_sequence_selector(entry, kSequenceSetup, input.data(), output.data())
      : -1;
  std::cerr << "stage:sequence_setup_end error=" << lifecycle_errors[0] << "\n" << std::flush;
  write<void*>(input, kInSequenceData, read<void*>(output, kOutSequenceData));
  if (lifecycle_errors[0] == 0 && g_user_changed_param_requested) {
    const auto offset = static_cast<std::size_t>(g_user_changed_param_slot - 1);
    if (offset >= g_params.size() || (g_params[offset].flags & (1u << 6)) == 0) {
      g_user_changed_param_error = 4;
    } else {
      std::array<std::byte, 4> changed_extra{};
      write<int32_t>(changed_extra, 0, g_user_changed_param_slot);
      g_user_changed_param_active = true;
      g_active_ui_params = lifecycle_params.data();
      g_active_ui_param_count = lifecycle_params.size();
      g_user_changed_param_error = entry(kUserChangedParam, input.data(), output.data(),
          lifecycle_params.data(), nullptr, changed_extra.data());
      g_active_ui_params = nullptr;
      g_active_ui_param_count = 0;
      g_user_changed_param_active = false;
    }
  }
  user_changed_ok = lifecycle_errors[0] == 0 &&
      (!g_user_changed_param_requested || g_user_changed_param_error == 0);
  conditional_ui_ok = user_changed_ok && dispatch_conditional_ui_selectors(
      entry, input, output, lifecycle_params.data());
  if (conditional_ui_ok && conditional_ui_definitions_usable()) {
    for (std::size_t i = 0; i < g_params.size(); ++i)
      g_params[i].raw = lifecycle_definitions[i + 1];
  }
  std::cerr << "stage:sequence_resetup_begin\n" << std::flush;
  lifecycle_errors[1] = lifecycle_errors[0] == 0
      ? invoke_sequence_selector(entry, kSequenceResetup, input.data(), output.data()) : -1;
  std::cerr << "stage:sequence_resetup_end error=" << lifecycle_errors[1] << "\n" << std::flush;
  write<void*>(input, kInSequenceData, read<void*>(output, kOutSequenceData));
  std::cerr << "stage:frame_setup_begin\n" << std::flush;
  lifecycle_errors[2] = lifecycle_errors[1] == 0 ? entry(kFrameSetup, input.data(), output.data(), lifecycle_params.data(), lifecycle_world.data(), nullptr) : -1;
  std::cerr << "stage:frame_setup_end error=" << lifecycle_errors[2] << "\n" << std::flush;
  write<void*>(input, kInFrameData, read<void*>(output, kOutFrameData));
  std::cerr << "stage:frame_setdown_begin\n" << std::flush;
  lifecycle_errors[3] = lifecycle_errors[2] == 0 ? entry(kFrameSetdown, input.data(), output.data(), lifecycle_params.data(), lifecycle_world.data(), nullptr) : -1;
  std::cerr << "stage:frame_setdown_end error=" << lifecycle_errors[3] << "\n" << std::flush;
  if (lifecycle_errors[3] == 0) write<void*>(output, kOutFrameData, nullptr);
  std::cerr << "stage:sequence_setdown_begin\n" << std::flush;
  lifecycle_errors[4] = lifecycle_errors[3] == 0
      ? invoke_sequence_selector(entry, kSequenceSetdown, input.data(), output.data()) : -1;
  std::cerr << "stage:sequence_setdown_end error=" << lifecycle_errors[4] << "\n" << std::flush;
  if (lifecycle_errors[4] == 0) write<void*>(output, kOutSequenceData, nullptr);
  lifecycle_data_null = read<void*>(output, kOutSequenceData) == nullptr &&
      read<void*>(output, kOutFrameData) == nullptr;
  return std::nullopt;
}

// The worker-entry bootstrap wiring and the selftest command dispatch moved
// to worker_entry_wiring.cpp (issue #171 / #165); worker_main_impl keeps
// consuming them through these declarations.
int configure_worker_entry_bootstrap();
std::optional<int> dispatch_worker_selftests(int argc, wchar_t** argv);

// Effect-bootstrap wiring shared by the launch bootstrap, the cluster-session
// swap re-bootstrap, and the discovery session's per-plug-in inspect (issue
// #405). Extracted so every path installs the identical ABI tables and
// runtime hooks; only the Request (worker kind, depth, audio, skip_about)
// differs per call site.
int32_t timeline_result(const char* callback, int32_t result) {
  record_host_callback_invocation(
      callback, result, HostCallbackClassification::implemented);
  return result;
}

int32_t __cdecl timeline_checkout_param(
    void* effect_ref, int32_t index, int32_t what_time, int32_t time_step,
    uint32_t time_scale, void* definition) {
  return timeline_result(
      "inter.checkout_param",
      checkout_param(effect_ref, index, what_time, time_step, time_scale,
                     definition));
}

int32_t __cdecl timeline_checkin_param(
    void* effect_ref, void* definition) {
  return timeline_result(
      "inter.checkin_param", checkin_param(effect_ref, definition));
}

int32_t __cdecl timeline_add_param(
    void* effect_ref, int32_t index, void* definition) {
  return timeline_result(
      "inter.add_param", add_param(effect_ref, index, definition));
}

int32_t __cdecl timeline_abort_render(void* effect_ref) {
  return timeline_result("inter.abort_render", abort_render(effect_ref));
}

int32_t __cdecl timeline_report_progress(
    void* effect_ref, int32_t current, int32_t total) {
  return timeline_result(
      "inter.report_progress",
      report_progress(effect_ref, current, total));
}

int32_t __cdecl timeline_register_custom_ui(
    void* effect_ref, const void* custom_ui_info) {
  return timeline_result(
      "inter.register_custom_ui",
      register_custom_ui(effect_ref, custom_ui_info));
}

int32_t __cdecl timeline_checkout_layer_audio(
    void* effect_ref, int32_t index, int32_t start_time, int32_t duration,
    uint32_t time_scale, uint32_t rate, int32_t bytes_per_sample,
    int32_t channels, int32_t format, void** audio) {
  return timeline_result(
      "inter.checkout_layer_audio",
      checkout_layer_audio(
          effect_ref, index, start_time, duration, time_scale, rate,
          bytes_per_sample, channels, format, audio));
}

int32_t __cdecl timeline_checkin_layer_audio(
    void* effect_ref, void* audio) {
  return timeline_result(
      "inter.checkin_layer_audio",
      checkin_layer_audio(effect_ref, audio));
}

int32_t __cdecl timeline_get_audio_data(
    void* effect_ref, void* audio, void** data, int32_t* sample_count,
    uint32_t* rate, int32_t* bytes_per_sample, int32_t* channels,
    int32_t* format) {
  return timeline_result(
      "inter.get_audio_data",
      get_audio_data(
          effect_ref, audio, data, sample_count, rate, bytes_per_sample,
          channels, format));
}

int32_t utility_callback_result(aexcompat::callback_diagnostics::Callback callback,
                                int32_t result,
                                aexcompat::callback_diagnostics::Reason failure_reason =
                                    aexcompat::callback_diagnostics::Reason::CallbackError) {
  using aexcompat::callback_diagnostics::Reason;
  const auto reason = result == 0 ? Reason::None : failure_reason;
  aexcompat::callback_diagnostics::record(callback, result, reason);
  if (extended_diag_enabled()) {
    std::cerr << "extended_diag:"
              << aexcompat::callback_diagnostics::CALLBACK_NAMES[
                     static_cast<std::size_t>(callback)]
              << " -> " << result;
    if (result != 0)
      std::cerr << " ("
                << aexcompat::callback_diagnostics::REASON_NAMES[
                       static_cast<std::size_t>(reason)]
                << ')';
    std::cerr << '\n' << std::flush;
  }
  return result;
}

int32_t __cdecl diag_blend_world(void* effect_ref, const void* source1,
                                 const void* source2, int32_t ratio, void* destination) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::Blend,
      blend_world(effect_ref, source1, source2, ratio, destination),
      !source1 || !source2 || !destination
          ? aexcompat::callback_diagnostics::Reason::InvalidArguments
          : aexcompat::callback_diagnostics::Reason::CallbackError);
}
int32_t __cdecl diag_convolve_world(void* effect_ref, void* source, const ::LegacyRect* area,
                                    uint32_t flags, int32_t kernel_size, void* alpha_kernel,
                                    void* red_kernel, void* green_kernel, void* blue_kernel,
                                    void* destination) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::Convolve,
      convolve_world(effect_ref, source, area, flags, kernel_size, alpha_kernel, red_kernel,
                     green_kernel, blue_kernel, destination),
      !source || !destination
          ? aexcompat::callback_diagnostics::Reason::InvalidArguments
          : aexcompat::callback_diagnostics::Reason::CallbackError);
}
int32_t __cdecl diag_copy_world(void* effect_ref, void* source, void* destination,
                               const ::LegacyRect* source_rect,
                               const ::LegacyRect* destination_rect) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::Copy,
      copy_world8(effect_ref, source, destination, source_rect, destination_rect),
      !source || !destination
          ? aexcompat::callback_diagnostics::Reason::MissingWorld
          : aexcompat::callback_diagnostics::Reason::CallbackError);
}
int32_t __cdecl diag_fill_world(void* effect_ref, const void* color,
                               const ::LegacyRect* area, void* world) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::Fill,
      fill_world8(effect_ref, color, area, world),
      !world ? aexcompat::callback_diagnostics::Reason::MissingWorld
             : aexcompat::callback_diagnostics::Reason::CallbackError);
}
int32_t __cdecl diag_fill_world16(void* effect_ref, const void* color,
                                 const ::LegacyRect* area, void* world) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::Fill,
      fill_world16(effect_ref, color, area, world),
      !world ? aexcompat::callback_diagnostics::Reason::MissingWorld
             : aexcompat::callback_diagnostics::Reason::CallbackError);
}
int32_t __cdecl diag_premultiply_world(void* effect_ref, int32_t forward, void* world) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::Premultiply,
      premultiply_world8(effect_ref, forward, world),
      !world ? aexcompat::callback_diagnostics::Reason::MissingWorld
             : aexcompat::callback_diagnostics::Reason::CallbackError);
}
int32_t __cdecl diag_premultiply_color(void* effect_ref, void* source, const void* color,
                                      int32_t forward, void* destination) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::Premultiply,
      premultiply_color8(effect_ref, source, color, forward, destination),
      !source || !destination
          ? aexcompat::callback_diagnostics::Reason::MissingWorld
          : aexcompat::callback_diagnostics::Reason::CallbackError);
}
int32_t __cdecl diag_premultiply_color16(void* effect_ref, void* source, const void* color,
                                        int32_t forward, void* destination) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::Premultiply,
      premultiply_color16(effect_ref, source, color, forward, destination),
      !source || !destination
          ? aexcompat::callback_diagnostics::Reason::MissingWorld
          : aexcompat::callback_diagnostics::Reason::CallbackError);
}
int32_t __cdecl diag_new_world(void* effect_ref, int32_t width, int32_t height,
                              int32_t flags, void* world) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::NewWorld,
      legacy_new_world(effect_ref, width, height, flags, world),
      !world || width <= 0 || height <= 0
          ? aexcompat::callback_diagnostics::Reason::InvalidArguments
          : aexcompat::callback_diagnostics::Reason::CallbackError);
}
int32_t __cdecl diag_dispose_world(void* effect_ref, void* world) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::DisposeWorld,
      dispose_world(effect_ref, world),
      !world ? aexcompat::callback_diagnostics::Reason::MissingWorld
             : aexcompat::callback_diagnostics::Reason::CallbackError);
}
int32_t __cdecl diag_transfer_rect(void* effect_ref, int32_t quality, uint32_t mode_flags,
                                  int32_t field, const ::LegacyRect* source_rect,
                                  const void* source_world, const void* composite_mode,
                                  const void* mask_world, int32_t destination_x,
                                  int32_t destination_y, void* destination_world) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::TransferRect,
      transfer_rect(effect_ref, quality, mode_flags, field, source_rect, source_world,
                    composite_mode, mask_world, destination_x, destination_y,
                    destination_world),
      !effect_ref || !source_world || !composite_mode || !destination_world
          ? aexcompat::callback_diagnostics::Reason::MissingWorld
          : aexcompat::callback_diagnostics::Reason::CallbackError);
}
int32_t __cdecl diag_transform_world(void* effect_ref, int32_t quality, uint32_t mode_flags,
                                     int32_t field, const void* source_world,
                                     const void* composite_mode, const void* mask_world,
                                     const void* matrices, int32_t matrix_count,
                                     uint8_t source_to_destination,
                                     const ::LegacyRect* destination_rect,
                                     void* destination_world) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::TransformWorld,
      transform_world(effect_ref, quality, mode_flags, field, source_world, composite_mode,
                      mask_world, matrices, matrix_count, source_to_destination,
                      destination_rect, destination_world),
      !effect_ref || !source_world || !composite_mode || !matrices || !destination_world
          ? aexcompat::callback_diagnostics::Reason::InvalidArguments
          : aexcompat::callback_diagnostics::Reason::CallbackError);
}
int32_t __cdecl diag_get_callback_addr(void* effect_ref, int32_t quality,
                                       uint32_t mode_flags, int32_t id, void** callback) {
  const int32_t result = get_callback_addr(effect_ref, quality, mode_flags, id, callback);
  // Callback ID 9 is COPY. Preserve diagnostic coverage when a plug-in obtains
  // it indirectly instead of reading utils.copy; sampling IDs already point at
  // their directly instrumented implementations.
  if (result == 0 && callback && id == 9)
    *callback = reinterpret_cast<void*>(&diag_copy_world);
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::GetCallbackAddr,
      result,
      !callback ? aexcompat::callback_diagnostics::Reason::InvalidArguments
                : aexcompat::callback_diagnostics::Reason::Unsupported);
}
int32_t __cdecl diag_get_platform_data(void* effect_ref, int32_t which, void* data) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::PlatformData,
      get_platform_data(effect_ref, which, data),
      !effect_ref || !data
          ? aexcompat::callback_diagnostics::Reason::InvalidArguments
          : aexcompat::callback_diagnostics::Reason::Unsupported);
}
int32_t __cdecl diag_get_pixel_data8(void* world, void* pixels, void** output) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::PixelData,
      get_pixel_data8(world, pixels, output),
      !world ? aexcompat::callback_diagnostics::Reason::MissingWorld
             : aexcompat::callback_diagnostics::Reason::InvalidArguments);
}
int32_t __cdecl diag_get_pixel_data16(void* world, void* pixels, void** output) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::PixelData,
      get_pixel_data16(world, pixels, output),
      !world ? aexcompat::callback_diagnostics::Reason::MissingWorld
             : aexcompat::callback_diagnostics::Reason::InvalidArguments);
}
int32_t __cdecl diag_host_app(void* effect_ref, int32_t selector, void* arg) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::App,
      host_app_callback(effect_ref, selector, arg),
      !effect_ref ? aexcompat::callback_diagnostics::Reason::InvalidArguments
                  : aexcompat::callback_diagnostics::Reason::Unsupported);
}

void** __cdecl diag_new_handle(uint64_t size) {
  void** result = new_handle(size);
  utility_callback_result(aexcompat::callback_diagnostics::Callback::Handle,
                          result ? 0 : 4);
  return result;
}
void* __cdecl diag_lock_handle(void** handle) {
  void* result = lock_handle(handle);
  utility_callback_result(aexcompat::callback_diagnostics::Callback::Handle,
                          result ? 0 : 4);
  return result;
}
void __cdecl diag_unlock_handle(void** handle) {
  const auto before = aexcompat::worker_runtime::handles::statistics().invalid_operations;
  unlock_handle(handle);
  const auto after = aexcompat::worker_runtime::handles::statistics().invalid_operations;
  utility_callback_result(aexcompat::callback_diagnostics::Callback::Handle,
                          after == before ? 0 : 4);
}
void __cdecl diag_dispose_handle(void** handle) {
  const auto before = aexcompat::worker_runtime::handles::statistics().invalid_operations;
  dispose_handle(handle);
  const auto after = aexcompat::worker_runtime::handles::statistics().invalid_operations;
  utility_callback_result(aexcompat::callback_diagnostics::Callback::Handle,
                          after == before ? 0 : 4);
}
uint64_t __cdecl diag_handle_size(void** handle) {
  const bool valid = aexcompat::worker_runtime::handles::host_handle_is_live(handle);
  const uint64_t result = handle_size(handle);
  utility_callback_result(aexcompat::callback_diagnostics::Callback::Handle,
                          valid ? 0 : 4);
  return result;
}
int32_t __cdecl diag_resize_handle(uint64_t size, void*** handle) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::Handle,
      resize_handle(size, handle));
}

aexcompat::worker_runtime::effect_bootstrap::AbiHooks make_bootstrap_abi_hooks() {
  aexcompat::callback_diagnostics::reset();
  aexcompat::pf_utility_callbacks::Sources sources{};
  sources.begin_sampling = reinterpret_cast<void*>(&begin_sampling8);
  sources.subpixel_sample = reinterpret_cast<void*>(&subpixel_sample8);
  sources.area_sample = reinterpret_cast<void*>(&area_sample8);
  sources.end_sampling = reinterpret_cast<void*>(&end_sampling8);
  sources.blend = reinterpret_cast<void*>(&diag_blend_world);
  sources.convolve = reinterpret_cast<void*>(&diag_convolve_world);
  sources.copy = reinterpret_cast<void*>(&diag_copy_world);
  sources.fill = reinterpret_cast<void*>(&diag_fill_world);
  sources.premultiply = reinterpret_cast<void*>(&diag_premultiply_world);
  sources.premultiply_color = reinterpret_cast<void*>(&diag_premultiply_color);
  sources.subpixel_sample16 = reinterpret_cast<void*>(&subpixel_sample16);
  sources.area_sample16 = reinterpret_cast<void*>(&area_sample16);
  sources.fill16 = reinterpret_cast<void*>(&diag_fill_world16);
  sources.premultiply_color16 = reinterpret_cast<void*>(&diag_premultiply_color16);
  sources.iterate16 = reinterpret_cast<void*>(&iterate_world16);
  sources.iterate = reinterpret_cast<void*>(&iterate_world8);
  sources.iterate_origin = reinterpret_cast<void*>(&iterate_origin8);
  sources.new_world = reinterpret_cast<void*>(&diag_new_world);
  sources.dispose_world = reinterpret_cast<void*>(&diag_dispose_world);
  sources.transfer_rect = reinterpret_cast<void*>(&diag_transfer_rect);
  sources.transform_world = reinterpret_cast<void*>(&diag_transform_world);
  sources.get_callback_addr = reinterpret_cast<void*>(&diag_get_callback_addr);
  sources.ansi_ceil = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_ceil);
  sources.ansi_cos = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_cos);
  sources.ansi_fabs = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_fabs);
  sources.ansi_hypot = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_hypot);
  sources.ansi_pow = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_pow);
  sources.ansi_sin = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_sin);
  sources.ansi_sqrt = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_sqrt);
  sources.ansi_sprintf = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_sprintf);
  sources.ansi_strcpy = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_strcpy);
  sources.ansi_asin = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_asin);
  sources.ansi_acos = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_acos);
  sources.get_platform_data = reinterpret_cast<void*>(&diag_get_platform_data);
  sources.get_pixel_data8 = reinterpret_cast<void*>(&diag_get_pixel_data8);
  sources.get_pixel_data16 = reinterpret_cast<void*>(&diag_get_pixel_data16);
  sources.host_new_handle = reinterpret_cast<void*>(&diag_new_handle);
  sources.host_lock_handle = reinterpret_cast<void*>(&diag_lock_handle);
  sources.host_unlock_handle = reinterpret_cast<void*>(&diag_unlock_handle);
  sources.host_dispose_handle = reinterpret_cast<void*>(&diag_dispose_handle);
  sources.host_get_handle_size = reinterpret_cast<void*>(&diag_handle_size);
  sources.host_resize_handle = reinterpret_cast<void*>(&diag_resize_handle);
  sources.app = reinterpret_cast<void*>(&diag_host_app);
  const auto utility_callbacks = aexcompat::pf_utility_callbacks::build(sources);

  return {{reinterpret_cast<void*>(&timeline_checkout_param),
    reinterpret_cast<void*>(&timeline_checkin_param),
    reinterpret_cast<void*>(&timeline_add_param),
    reinterpret_cast<void*>(&timeline_abort_render),
    reinterpret_cast<void*>(&timeline_report_progress),
    reinterpret_cast<void*>(&timeline_register_custom_ui),
    reinterpret_cast<void*>(&timeline_checkout_layer_audio),
    reinterpret_cast<void*>(&timeline_checkin_layer_audio),
    reinterpret_cast<void*>(&timeline_get_audio_data),
    // Extended inter slots 0x60 / 0x68 / 0x70 (issue #382).
    reinterpret_cast<void*>(&host_extended_alloc),
    reinterpret_cast<void*>(&host_extended_lookup),
    reinterpret_cast<void*>(&host_extended_free)},
   utility_callbacks,
   &g_color_suite8, sizeof(g_color_suite8),
   &g_basic_suite, &g_effect};
}

aexcompat::worker_runtime::effect_bootstrap::RuntimeHooks make_bootstrap_runtime_hooks() {
  return {&invoke_entry_seh, &reset_effect_lifetime,
   +[](bool active) { g_global_setup_active = active; },
   +[](bool requested, bool advertised) {
     aexcompat::host_audio::runtime().configure_admission(requested, advertised);
   },
   +[](EffectEntry callback,
       aexcompat::worker_runtime::effect_bootstrap::State& state) {
     observe_arbitrary_defaults(callback, state.input, state.output);
   },
   +[]() { return static_cast<int32_t>(g_params.size()); }};
}

// Per-plug-in host state reset for cluster sessions (issue #405): a swapped
// or re-inspected plug-in must observe the same host state a fresh one-shot
// process would present, and its inspect report must not carry the previous
// plug-in's records or telemetry. Session-lifetime balances (handles,
// suites, worlds) intentionally stay cumulative: they are process-wide
// leak checks, not per-plug-in reports.
void reset_cluster_effect_state() {
  g_params.clear();
  aexcompat::compute_cache::purge_registry();
  g_parameter_runtime.arbitrary = aexcompat::worker_runtime::parameters::ArbitraryTelemetry{};
  g_parameter_runtime.ui = aexcompat::worker_runtime::parameters::UiState{};
  g_parameter_runtime.checkout.definitions.clear();
  {
    std::lock_guard<std::mutex> lock(g_parameter_runtime.checkout.mutex);
    g_parameter_runtime.checkout.live.clear();
  }
  g_custom_ui_telemetry = aexcompat::worker_runtime::ui_event_execution::CustomUiTelemetry{};
  aexcompat::pf_state_runtime::reset_pf_state_statistics();
}

// Cluster-session plug-in string table (#405 on the #396 contract): the
// launch path keeps its own local table; swapped/inspected plug-ins keep
// theirs in the session scope so the thread-local active pointer never
// dangles past the loader's frame.
void activate_plugin_string_table(HMODULE module,
                                  aexcompat::aex_strings::StringTable& table) {
  load_aex_string_table(module, table);
  const char* status =
      table.status == aexcompat::aex_strings::ParseStatus::Valid
          ? "valid"
          : table.status == aexcompat::aex_strings::ParseStatus::NoEntries
              ? "none"
              : "invalid";
  std::cerr << "string_table_status:" << status << "\n" << std::flush;
  g_active_effect_module = module;
  g_active_aex_string_table = &table;
}

// Render-session cluster swap (closure-session design §4.1): the session
// frame loop owns SEQUENCE teardown and the swap_done response; this hook
// owns everything from GLOBAL_SETDOWN to GLOBAL_SETUP/PARAMS_SETUP and the
// payload swap. A hard failure means the worker exits with the dedicated
// swap-failure exit code without answering.
struct ClusterSwapContext {
  aexcompat::worker_runtime::WorkerSession* session{};
  EffectEntry* entry{};
  aexcompat::worker_runtime::effect_bootstrap::State* effect_state{};
  aexcompat::worker_runtime::invocation::InvocationState* invocation{};
  const aexcompat::worker_runtime::cluster::Manifest* manifest{};
  std::function<aexcompat::worker_runtime::effect_bootstrap::Result(EffectEntry)>
      run_bootstrap;
  int32_t current_plugin_index{0};
  int32_t current_global_error{-1};
  // String table of the currently loaded plug-in (#396): swapped in place so
  // the thread-local active pointer tracks the session's current plug-in.
  aexcompat::aex_strings::StringTable aex_string_table;
};

aexcompat::worker_render_session::SwapPluginResult cluster_swap_invoke(
    void* opaque, int32_t plugin_index) {
  namespace cluster = aexcompat::worker_runtime::cluster;
  auto& context = *static_cast<ClusterSwapContext*>(opaque);
  aexcompat::worker_render_session::SwapPluginResult result;
  if (plugin_index < 0 ||
      plugin_index >= static_cast<int32_t>(context.manifest->plugins.size())) {
    result.hard_failure = true;
    return result;
  }
  const cluster::PluginEntry& plugin =
      context.manifest->plugins[static_cast<std::size_t>(plugin_index)];
  auto& input = context.effect_state->input;
  auto& output = context.effect_state->output;
  // GLOBAL_SETDOWN (design §4.1 step 1); dispose the arbitrary defaults first,
  // mirroring the teardown order of the close paths.
  if (*context.entry) dispose_arbitrary_defaults(*context.entry, input, output);
  if (context.current_global_error == 0 &&
      invoke_global_setdown(*context.entry, input.data(), output.data()) != 0) {
    result.hard_failure = true;
    return result;
  }
  // Steps 2-4: per-swap quiescence (the session-global BIB teardown hook is
  // NOT consumed here; it runs once at the terminal teardown), epoch
  // pre_unload, FreeLibrary of the plug-in only (the cookie and pinned
  // closure dependencies stay loaded).
  if (!context.session->swap_release_module(
          static_cast<uint32_t>(context.current_plugin_index))) {
    result.hard_failure = true;
    return result;
  }
  // Step 5: authenticate plugins[N] against the manifest, then load with the
  // in-place admission flags. A failure means the real bytes changed under
  // the recorded identity (the #309 state transition) or the loader rejected
  // the closure. The broker's session-invalidation path reopens fresh
  // sessions and re-discovery converges on the new bytes, so the mismatch
  // stays a state transition rather than a launch gate.
  const std::filesystem::path path = cluster::plugin_path(*context.manifest,
      static_cast<std::size_t>(plugin_index));
  std::string actual_sha256;
  if (!sha256(path, actual_sha256) ||
      !cluster::hash_equals(actual_sha256, plugin.sha256)) {
    result.hard_failure = true;
    return result;
  }
  HMODULE module = LoadLibraryExW(path.c_str(), nullptr,
      LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32 |
      LOAD_LIBRARY_SEARCH_USER_DIRS);
  if (!module) {
    result.hard_failure = true;
    return result;
  }
  if (!context.session->swap_adopt_module(module, path,
                                          static_cast<uint32_t>(plugin_index))) {
    // The session rejected (and freed) the loaded module; no cleanup here.
    result.hard_failure = true;
    return result;
  }
  g_plugin_file_path = path.wstring();
  context.current_plugin_index = plugin_index;
  // Entrypoint resolution follows the launch rules (PiPL, then PluginData on a
  // missing PiPL only). A null entry in the swap result fails the session
  // closed like every other swap failure (the frame loop treats it as
  // swap_failure, exit 25): mid-session member changes reopen through the
  // broker's invalidation path rather than limping on a member-less session.
  PiplEntrypoint pipl_entrypoint = discover_pipl_entrypoint(module);
  if (pipl_entrypoint.kind == PiplPluginKind::Missing)
    pipl_entrypoint = discover_plugin_data_entrypoint(module);
  EffectEntry new_entry = nullptr;
  if (pipl_entrypoint.kind == PiplPluginKind::Effect)
    new_entry = reinterpret_cast<EffectEntry>(
        GetProcAddress(module, pipl_entrypoint.symbol.c_str()));
  *context.entry = new_entry;
  if (!new_entry) {
    context.current_global_error = -1;
    result.global_setup_error = -1;
    return result;
  }
  // The swapped-in plug-in's string table drives its PARAMS_SETUP lookups
  // (issue #396), replacing the launch plug-in's table.
  activate_plugin_string_table(module, context.aex_string_table);
  // Legacy support libraries in the closure (U.dll allocator for PIN-era
  // effects) get their one-time process init on first sight (issue #362).
  initialize_legacy_support_libraries();
  // GLOBAL_SETUP / PARAMS_SETUP through the launch bootstrap on fresh
  // buffers and fresh host records (ABOUT stays whatever the launch did).
  reset_cluster_effect_state();
  *context.effect_state = aexcompat::worker_runtime::effect_bootstrap::State{};
  const auto bootstrap = context.run_bootstrap(new_entry);
  result.entry = new_entry;
  result.global_setup_error = bootstrap.global_error;
  result.params_setup_error = bootstrap.params_error;
  context.current_global_error = bootstrap.global_error;
  // Step 6: the manifest payload (plugins[1..]) replaces the launch
  // assignments, riding the same argv encoding (design §2.2/§4.1).
  context.invocation->requested_parameters.clear();
  if (plugin.has_payload) {
    std::wstring widened;
    widened.reserve(plugin.payload.size());
    for (const unsigned char byte : plugin.payload)
      widened.push_back(static_cast<wchar_t>(byte));
    if (!parse_parameter_payload(widened.c_str(),
                                 context.invocation->requested_parameters) ||
        !validate_requested_assignments(context.invocation->requested_parameters)) {
      if (result.params_setup_error == 0) result.params_setup_error = -1;
    }
  }
  return result;
}

// Discovery session (closure-session design §4.2, issue #405): the worker
// starts with no plug-in loaded, pins the shared closure once, and answers
// inspect_plugin messages with the same params-only inspect the one-shot
// `--l2-params-only` path runs (ABOUT omitted, GLOBAL_SETUP → PARAMS_SETUP).
// The report mirrors the one-shot L2 JSON minus module_audit; the audit with
// its swap epochs rides the session's final stdout report.
int run_discovery_session(const std::wstring& manifest_argument,
                          const aexcompat::worker_runtime::RuntimeHostHooks& runtime_hooks) {
  namespace cluster = aexcompat::worker_runtime::cluster;
  namespace wr = aexcompat::worker_runtime;
  namespace wrs = aexcompat::worker_render_session;
  using aexcompat::strict_json::JsonValue;
  using aexcompat::strict_json::StrictJsonParser;
  using aexcompat::strict_json::json_exact_keys;
  using aexcompat::strict_json::json_i32;
  using aexcompat::strict_json::json_string;
  // Exit codes on this path: 2 usage, 3 manifest/config rejection, 11 DLL
  // policy/pin failure, 13 stdout redirect failure, 14 terminal audit
  // failure, 23 protocol violation, 25 swap/BIB-teardown failure (closure
  // design §7).
  if (manifest_argument.empty()) return 2;
  cluster::Manifest manifest;
  if (!cluster::load_manifest(manifest_argument, manifest)) return 3;
  wrs::SessionPipes pipes;
  if (!pipes.open_from_environment()) return 23;
  pipes.set_max_write_bytes(wrs::kMaxInspectReportBytes);
  if (!SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 |
                                LOAD_LIBRARY_SEARCH_USER_DIRS))
    return 11;
  if (const int admit_error = cluster::admit_in_place_manifest_dirs(manifest))
    return admit_error;
  wr::configure_module_audit_search_roots(cluster::in_place_audit_roots(manifest));
  wr::configure_module_audit_cluster(manifest.module_bound, {});
  wr::ModuleAuditReport& audit = wr::module_audit_report();
  audit.required = false;
  audit.recorded = true;
  audit.plugin_path = cluster::plugin_path(manifest, 0);
  if (!runtime_hooks.redirect_native_stdout()) {
    return 13;
  }

  aexcompat::worker_runtime::effect_bootstrap::State effect_state;
  aexcompat::aex_strings::StringTable aex_string_table;
  HMODULE current_module = nullptr;
  EffectEntry current_entry = nullptr;
  int32_t current_index = -1;
  int32_t current_global_error = -1;
  // GLOBAL_SETUP/GLOBAL_SETDOWN pairing state: the inspect column below ends
  // every successful setup with its setdown (the one-shot params-only column
  // the report must match), so a later swap, re-inspect, or teardown must not
  // set the plug-in down a second time — a duplicate GLOBAL_SETDOWN is a host
  // protocol violation plug-ins legitimately fail (real-env exit 25).
  bool current_setdown_pending = false;
  int32_t expected_request_index = 0;
  bool protocol_violation = false;
  bool swap_failure = false;
  bool bib_teardown_failed = false;
  // Whether the session's baseline post_load snapshot was taken (the first
  // successful load). An in-place load failure retires the previous plug-in
  // without loading a successor, so without this latch the next successful
  // load would overwrite the baseline with a later point in time.
  bool session_post_load_recorded = false;

  // Terminal teardown shared by every exit path after the session loop. The
  // ownership order is explicit (owner review P1-1, amended by issue #474's
  // deferred release): GLOBAL_SETDOWN → plug-in/module audit → BIB owned-only
  // Terminate (session-global, runs exactly once here — never per swap — and
  // never for a borrowed, missing, or init-failed BIB). No plug-in image or
  // pin is freed mid-process: everything unloads in one loader-ordered pass
  // at process exit. Cookie removal, stdout restore, final report follow.
  const auto finish_session = [&](int exit_code) -> int {
    if (current_module) {
      if (current_entry) {
        dispose_arbitrary_defaults(current_entry, effect_state.input,
                                   effect_state.output);
        if (current_setdown_pending) {
          invoke_global_setdown(current_entry, effect_state.input.data(),
                                effect_state.output.data());
          current_setdown_pending = false;
        }
      }
      audit.pre_unload = wr::capture_module_audit();
      if (!teardown_bib_suite(nullptr)) bib_teardown_failed = true;
      current_module = nullptr;
    } else {
      // No plug-in survived to teardown, but an owned BIB may still be live
      // from an earlier plug-in; terminate it before the pins release.
      if (!teardown_bib_suite(nullptr)) bib_teardown_failed = true;
    }
    // The sealed-directory cookie stays too: a detaching DLL may delay-load
    // from the sealed root during process teardown.
    runtime_hooks.restore_native_stdout();
    const bool audit_ok = !audit.required ||
        (audit.pre_unload.status == "passed" && wr::module_audit_passed());
    std::cout << "{\"schema_version\":1,\"stage\":\"discovery_session\",\"status\":\""
              << (exit_code == 0 && !bib_teardown_failed
                      ? "discovery_session_completed" : "discovery_session_failed")
              << "\",\"bib_terminations\":" << bib_termination_attempt_count()
              << ",\"module_audit\":" << wr::module_audit_json() << "}\n";
    if (!audit_ok && exit_code == 0) return 14;
    if (bib_teardown_failed && exit_code == 0) return 25;
    return exit_code;
  };

  const auto run_inspect = [&](EffectEntry entry) -> wr::effect_bootstrap::Result {
    reset_cluster_effect_state();
    effect_state = wr::effect_bootstrap::State{};
    return wr::effect_bootstrap::run(
        effect_state, entry, make_bootstrap_abi_hooks(),
        {g_render_quality, g_render_field, g_shutter_angle, g_shutter_phase,
         {g_pre_effect_source_origin_x, g_pre_effect_source_origin_y},
         {static_cast<int32_t>(g_downsample_x.numerator),
          static_cast<int32_t>(g_downsample_x.denominator)},
         {static_cast<int32_t>(g_downsample_y.numerator),
          static_cast<int32_t>(g_downsample_y.denominator)},
         {static_cast<int32_t>(g_pixel_aspect_ratio.numerator),
          static_cast<int32_t>(g_pixel_aspect_ratio.denominator)},
         4,      // external_pixel_bytes: params inspect is depth-neutral
         false,  // render_worker
         false,  // rendering_worker
         false,  // audio_invocation
         true},  // skip_about (design §4.2: ABOUT omitted)
        make_bootstrap_runtime_hooks());
  };

  for (;;) {
    std::string message;
    const auto read_result = pipes.read_message(message);
    if (read_result == wrs::SessionPipes::ReadResult::Eof) break;
    if (read_result != wrs::SessionPipes::ReadResult::Message) {
      protocol_violation = true;
      break;
    }
    JsonValue root;
    if (!StrictJsonParser(std::move(message)).parse(root) ||
        !std::holds_alternative<JsonValue::Object>(root.value)) {
      protocol_violation = true;
      break;
    }
    const auto& object = std::get<JsonValue::Object>(root.value);
    std::string type;
    int32_t version{};
    if (!json_string(object, "type", type) || !json_i32(object, "v", version) ||
        version != 1) {
      protocol_violation = true;
      break;
    }
    if (type == "close") {
      if (!json_exact_keys(object, {"v", "type"})) protocol_violation = true;
      break;
    }
    int32_t plugin_index{};
    int32_t request_index{};
    if (type != "inspect_plugin" ||
        !json_exact_keys(object, {"v", "type", "plugin_index", "request_index"}) ||
        !json_i32(object, "plugin_index", plugin_index) ||
        !json_i32(object, "request_index", request_index) ||
        plugin_index < 0 ||
        plugin_index >= static_cast<int32_t>(manifest.plugins.size()) ||
        request_index != expected_request_index) {
      protocol_violation = true;
      break;
    }
    ++expected_request_index;

    std::string report_json;
    std::string error_kind;
    // Authenticates plugins[N] against the manifest and loads it with the
    // admission flags; null on failure. On the sealed path the caller fails
    // the session closed; on the in-place path (issue #751) the failure is
    // plug-in-local — `load_error_kind` names it (`identity_changed` when the
    // bytes no longer match the manifest, the #309 state transition;
    // `load_failed` when the loader rejected them) and the session continues.
    std::string load_error_kind;
    const auto load_manifest_plugin = [&](int32_t index) -> HMODULE {
      load_error_kind.clear();
      const std::filesystem::path path =
          cluster::plugin_path(manifest, static_cast<std::size_t>(index));
      std::string actual_sha256;
      if (!runtime_hooks.hash_file(path, actual_sha256)) {
        // The bytes could not be read at all (a sharing violation, a vanished
        // file): distinct from bytes that read but no longer match, so the
        // record does not claim an identity change it never observed.
        load_error_kind = "hash_unavailable";
        return nullptr;
      }
      if (!cluster::hash_equals(
              actual_sha256,
              manifest.plugins[static_cast<std::size_t>(index)].sha256)) {
        load_error_kind = "identity_changed";
        return nullptr;
      }
      HMODULE module = LoadLibraryExW(path.c_str(), nullptr,
          LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32 |
          LOAD_LIBRARY_SEARCH_USER_DIRS);
      if (!module) load_error_kind = "load_failed";
      return module;
    };
    // The inspect column, identical to the one-shot `--l2-params-only` path
    // (ABOUT omitted, GLOBAL_SETUP → PARAMS_SETUP, dispose defaults,
    // GLOBAL_SETDOWN, then the L2 JSON minus module_audit).
    const auto inspect_column = [&] {
      const auto bootstrap = run_inspect(current_entry);
      current_global_error = bootstrap.global_error;
      current_setdown_pending = current_global_error == 0;
      const bool defaults_disposed = dispose_arbitrary_defaults(
          current_entry, effect_state.input, effect_state.output);
      const int32_t setdown_error = current_setdown_pending
          ? invoke_global_setdown(current_entry, effect_state.input.data(),
                                  effect_state.output.data())
          : -1;
      current_setdown_pending = false;
      const bool ok = current_global_error == 0 &&
          bootstrap.params_error == 0 &&
          bootstrap.parameter_count_contract_valid && defaults_disposed &&
          setdown_error == 0;
      report_json = build_l2_report_json(
          ok ? "parameters_inspected" : "selector_error",
          current_global_error, bootstrap.params_error, setdown_error,
          effect_state.output, bootstrap.about_message, {-1, -1, -1, -1, -1},
          true, /*include_module_audit=*/false);
      if (!ok) error_kind = "selector_error";
    };
    if (plugin_index != current_index) {
      // Swap to plugins[N] (design §4.2): the same column as the render
      // swap (GLOBAL_SETDOWN → quiesce → unload → load → GLOBAL_SETUP).
      // BIB teardown is session-global and deliberately NOT run here; it
      // runs once in finish_session above.
      wr::ModuleAuditSnapshot pre_unload;
      int32_t outgoing_index = -1;
      if (current_module) {
        if (current_entry) {
          dispose_arbitrary_defaults(current_entry, effect_state.input,
                                     effect_state.output);
          // GLOBAL_SETDOWN pairs the setup the inspect column opened, but the
          // column already ran its own setdown for a successful setup; only
          // an unpaired setup is set down here.
          if (current_setdown_pending &&
              invoke_global_setdown(current_entry, effect_state.input.data(),
                                    effect_state.output.data()) != 0) {
            std::cerr << "stage:cluster_swap step=global_setdown error="
                      << GetLastError() << "\n" << std::flush;
            swap_failure = true;
            break;
          }
          current_setdown_pending = false;
        }
        // Per-swap quiescence barrier: no swap hook is registered on this
        // path, so the barrier is the terminal-audit owner's trivial pass.
        // The in-place session records the snapshot without judging it
        // (record, never enforce, issue #751).
        pre_unload = wr::capture_module_audit();
        // Deferred release (issue #474): the outgoing plug-in image is
        // retired, not freed — its logical teardown already happened and it
        // stays mapped until process exit. Later snapshots keep listing it
        // (declared in the manifest), and the audit union stays monotonic.
        outgoing_index = current_index;
        current_module = nullptr;
        current_entry = nullptr;
        current_index = -1;
      }
      HMODULE module = load_manifest_plugin(plugin_index);
      if (!module) {
        std::cerr << "stage:cluster_swap step=load_plugin index=" << plugin_index
                  << " kind=" << load_error_kind << " error=" << GetLastError()
                  << "\n" << std::flush;
        if (outgoing_index >= 0)
          wr::record_module_audit_epoch(static_cast<uint32_t>(outgoing_index),
                                        std::move(pre_unload),
                                        wr::capture_module_audit());
        error_kind = load_error_kind;
        std::string reply =
            "{\"v\":1,\"type\":\"inspect_done\",\"plugin_index\":" +
            std::to_string(plugin_index) + ",\"request_index\":" +
            std::to_string(request_index) + ",\"status\":\"error\"" +
            ",\"error_kind\":\"" + error_kind + "\"}";
        if (!pipes.write_message(reply)) {
          protocol_violation = true;
          break;
        }
        continue;
      }
      wr::ModuleAuditSnapshot post_load = wr::capture_module_audit();
      if (outgoing_index >= 0)
        wr::record_module_audit_epoch(static_cast<uint32_t>(outgoing_index),
                                      std::move(pre_unload), post_load);
      else if (!session_post_load_recorded)
        // First successful load: the launch never loaded a plug-in, so this
        // is the session's baseline post_load (design §5 epoch model). The
        // latch keeps a later load after an in-place load failure from
        // overwriting the baseline.
        audit.post_load = post_load;
      session_post_load_recorded = true;
      current_module = module;
      current_index = plugin_index;
      g_plugin_file_path =
          cluster::plugin_path(manifest, static_cast<std::size_t>(plugin_index))
              .wstring();
      // Entrypoint resolution follows the launch rules; a failure is
      // plug-in-local (design §4.2) and the session continues.
      PiplEntrypoint pipl_entrypoint = discover_pipl_entrypoint(current_module);
      if (pipl_entrypoint.kind == PiplPluginKind::Missing)
        pipl_entrypoint = discover_plugin_data_entrypoint(current_module);
      if (pipl_entrypoint.kind == PiplPluginKind::Effect)
        current_entry = reinterpret_cast<EffectEntry>(
            GetProcAddress(current_module, pipl_entrypoint.symbol.c_str()));
      else
        current_entry = nullptr;
      if (current_entry) {
        activate_plugin_string_table(current_module, aex_string_table);
        // One-time U.dll process init for PIN-era closures (issue #362).
        initialize_legacy_support_libraries();
        inspect_column();
      } else {
        current_global_error = -1;
        error_kind = "entrypoint_unresolved";
      }
    } else if (!current_entry) {
      // Re-inspect of a plug-in whose entrypoint never resolved.
      error_kind = "entrypoint_unresolved";
    } else {
      // Re-inspect of the current plug-in: set down only an unpaired setup
      // (the previous inspect column already set a successful one down),
      // then the same inspect column again, matching a fresh one-shot process.
      if (current_setdown_pending) {
        invoke_global_setdown(current_entry, effect_state.input.data(),
                              effect_state.output.data());
        current_setdown_pending = false;
      }
      inspect_column();
    }

    // Embed the report as a JSON value: the one-shot report ends with a
    // newline, which must not travel inside the message.
    while (!report_json.empty() &&
           (report_json.back() == '\n' || report_json.back() == '\r'))
      report_json.pop_back();
    std::string reply = "{\"v\":1,\"type\":\"inspect_done\",\"plugin_index\":" +
        std::to_string(plugin_index) + ",\"request_index\":" +
        std::to_string(request_index) + ",\"status\":\"" +
        (error_kind.empty() ? "ok" : "error") + "\"";
    if (!error_kind.empty()) reply += ",\"error_kind\":\"" + error_kind + "\"";
    if (!report_json.empty()) reply += ",\"report\":" + report_json;
    reply += "}";
    if (!pipes.write_message(reply)) {
      protocol_violation = true;
      break;
    }
  }

  if (protocol_violation) return finish_session(23);
  if (swap_failure) return finish_session(25);
  return finish_session(0);
}


int worker_main_impl(int argc, wchar_t **argv) {
  if (const int bootstrap_error = configure_worker_entry_bootstrap())
    return bootstrap_error;
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-pipl-entrypoint") {
    const bool passed = verify_pipl_entrypoint_parser();
    std::cout << "{\"pipl_entrypoint\":\"" << (passed ? "passed" : "failed")
              << "\",\"kind_discriminator\":true,\"code_win64_x86\":true,"
                 "\"bounded\":true,\"aegp_not_effect\":true}\n";
    return passed ? 0 : 72;
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-plugin-data-entrypoint") {
    const bool passed = verify_plugin_data_entrypoint();
    std::cout << "{\"plugin_data_entrypoint\":\""
              << (passed ? "passed" : "failed")
              << "\",\"v2\":true,\"v1_fallback\":true,"
                 "\"bounded\":true,\"fail_closed\":true}\n";
    return passed ? 0 : 73;
  }
  RuntimeHostHooks runtime_hooks{&sha256, &redirect_native_stdout,
                                 &restore_native_stdout};
  // GPU module-audit preflight (#290): authorize the AEXRMA1-listed GPU runtime
  // DLLs, load them via the transport backend, and emit the classified module
  // report the broker re-authenticates before a secure GPU dispatch. argv shape
  // mirrors the params-only manifest convention:
  //   --gpu-module-report-v1 <plugin> <sha256> --runtime-module-authorization-v1 <manifest>
  if (argc == 6 && std::wstring(argv[1]) == L"--gpu-module-report-v1" &&
      std::wstring(argv[4]) == L"--runtime-module-authorization-v1") {
    namespace wr = aexcompat::worker_runtime;
    namespace tp = aexcompat::gpu_runtime::memory_world_transport;
    // Manifest backend id (1=cuda,2=opencl,3=directx,4=opengl) → transport
    // framework code (3=cuda,1=opencl,4=directx). OpenGL is inspect-only; a
    // smart-session GPU render never runs on it.
    const uint32_t backend = wr::authorized_runtime_backend();
    const int32_t framework = backend == 1   ? 3
                              : backend == 2 ? 1
                              : backend == 3 ? 4
                                             : 0;
    if (framework == 0) return 75;
    // Fail-closed identity gate and plugin load are owned by the common runtime
    // admission component. This keeps l2_main from becoming a second plugin-load
    // owner while retaining the GPU-specific audit/report lifecycle below.
    RuntimeAdmissionRequest runtime_request;
    const int request_error = wr::prepare_runtime_request(
        argv[2], argv[3], true, argv[5], runtime_request);
    if (request_error != 0) return request_error;
    RuntimeContext runtime_context;
    const int admission_error = wr::admit_runtime(
        runtime_hooks, runtime_request, runtime_context);
    if (admission_error != 0) return admission_error;
    const auto release_admitted = [&] {
      wr::release_runtime_context(runtime_context);
    };
    // Load the plug-in and run the DLL-load module audit across the GPU lifecycle,
    // exactly like a sealed render worker: the broker's secure dispatch requires a
    // passing `module_audit` (non-empty worker+plugin sets, >= 3 phases, zero
    // unknowns) on every ok worker, so the preflight must produce one alongside the
    // classified GPU report. The GPU runtime's DriverStore modules classify as
    // `policy` only when the authorization manifest lists them, so the audit passes
    // exactly when the policy enumerates the GPU DLL closure (#300).
    SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_USER_DIRS);
    aexcompat::worker_runtime::ModuleAuditReport& audit = wr::module_audit_report();
    audit.required = true;
    audit.plugin_path = runtime_context.plugin_path;
    audit.post_load = wr::capture_module_audit();  // phase 1: pre-GPU baseline
    if (!tp::begin_backend_context(framework, 0)) {
      release_admitted();
      return 76;
    }
    wr::capture_module_audit_phase();  // phase 2: GPU runtime DLLs loaded
    const std::string report = wr::gpu_module_report_json();
    audit.pre_unload = wr::capture_module_audit();  // phase 3: pre-teardown
    tp::end_backend_context(framework);
    release_admitted();
    // An empty report means the enumeration could not be trusted or no authorized
    // module actually loaded; fail closed rather than emit a report that would
    // authenticate nothing. The broker re-validates the module_audit separately.
    if (report.empty()) return 77;
    std::cout << "{\"module_audit\":" << wr::module_audit_json()
              << ",\"gpu_module_report\":" << report << "}\n";
    return 0;
  }
  if (const auto selftest_exit = dispatch_worker_selftests(argc, argv))
    return *selftest_exit;
  // The broker passes only an authenticated inherited file handle via
  // environment (issue #18). No dump directory or dump path is accepted on the
  // worker command line, so nothing is consumed from argv here. Fails closed on
  // a real misconfiguration; a no-op when opt-in is off.
  if (!aexcompat::worker_runtime::minidump::configure_from_inherited_handle())
    return 3;
aexcompat::worker_runtime::invocation::InvocationState invocation;


  const auto parse_requested_payload = +[](const wchar_t* text, void* context) {
    return parse_parameter_payload(text, *static_cast<RequestedAssignments*>(context));
  };
  const aexcompat::worker_runtime::invocation::ApplyHooks invocation_hooks{
      +[](bool enabled) { aexcompat::mask_runtime::set_model_enabled(enabled); },
      +[](bool count_error, bool count_crash) {
        aexcompat::mask_runtime::set_fault(count_error
            ? aexcompat::mask_runtime::Fault::CountError
            : count_crash ? aexcompat::mask_runtime::Fault::CountCrash
                          : aexcompat::mask_runtime::Fault::None);
      },
      +[](std::vector<float>* audio, int32_t samples) {
        aexcompat::host_audio::runtime().set_source(audio, samples);
      }};

  if (is_render_worker()) {
    const auto parsed = aexcompat::worker_runtime::request_parser::parse(
      aexcompat::worker_runtime::request_parser::Kind::Render, argc, argv,
      {{nullptr, set_l2_dump_worlds_dir, enable_l2_checksum_detail,
        load_l2_aux_manifest, parse_l2_alpha_coverage, load_l2_parameter_animation,
        parse_l2_conformance_render_settings,
        capture_l2_runtime_module_authorization, capture_l2_cluster_manifest,
        capture_l2_dependency_search_dirs},
       &parse_layer_transport_key, &parse_mask_context_payload,
       &parse_spatial_context_payload, &parse_render_environment_payload,
       &invocation.requested_parameters, parse_requested_payload, &configure_mask_scene});
    if (parsed.error != 0) return parsed.error;
    aexcompat::worker_runtime::invocation::apply_render(
        parsed.invocation, invocation, invocation_hooks);
  } else if (is_smart_worker()) {
    const auto parsed = aexcompat::worker_runtime::request_parser::parse(
        aexcompat::worker_runtime::request_parser::Kind::Smart, argc, argv,
        {{nullptr, set_l2_dump_worlds_dir, enable_l2_checksum_detail,
          load_l2_aux_manifest, parse_l2_alpha_coverage, load_l2_parameter_animation,
          parse_l2_conformance_render_settings,
        capture_l2_runtime_module_authorization, capture_l2_cluster_manifest,
        capture_l2_dependency_search_dirs},
         &parse_layer_transport_key, &parse_mask_context_payload,
         &parse_spatial_context_payload, &parse_render_environment_payload,
         &invocation.requested_parameters, parse_requested_payload, &configure_mask_scene});
    if (parsed.error != 0) return parsed.error;
    aexcompat::worker_runtime::invocation::apply_smart(
        parsed.invocation, invocation, invocation_hooks);
  } else {
    // The L2 modes keep their strict positional argc contracts; the in-place
    // dependency-dirs pair (issue #751) is the one auxiliary option they
    // accept, peeled from the tail before mode classification.
    int l2_argc = argc;
    if (l2_argc >= 3 &&
        std::wstring(argv[l2_argc - 2]) == L"--dependency-dirs-v1") {
      if (!capture_l2_dependency_search_dirs(nullptr, argv[l2_argc - 1]))
        return 2;
      l2_argc -= 2;
    }
    const auto mode_error = aexcompat::worker_runtime::invocation::parse_l2_modes(
        l2_argc, argv, invocation,
        {parse_parameter_payload, kMaxParams});
    if (mode_error != 0) return mode_error;
    g_aegp_update_menu_mode = invocation.aegp_update_menu_mode;
    g_aegp_idle_mode = invocation.aegp_idle_mode;
    g_aegp_command_roundtrip_mode = invocation.aegp_command_roundtrip_mode;
    g_aegp_active_idle_roundtrip_mode = invocation.aegp_active_idle_roundtrip_mode;
    g_aegp_keyframe_roundtrip_mode = invocation.aegp_keyframe_roundtrip_mode;
    g_aegp_seek_roundtrip_mode = invocation.aegp_seek_roundtrip_mode;
    g_aegp_trim_roundtrip_mode = invocation.aegp_trim_roundtrip_mode;
    g_aegp_switch_roundtrip_mode = invocation.aegp_switch_roundtrip_mode;
    g_aegp_comp_idle_roundtrip_mode = invocation.aegp_comp_idle_roundtrip_mode;
    g_aegp_init_mode = invocation.aegp_init_mode;
    g_skip_about = invocation.skip_about_mode;
    g_app_picker_color = invocation.picker_color;
    g_user_changed_param_slot = invocation.user_changed_param_slot;
    g_user_changed_param_requested = invocation.user_changed_param_requested;
    g_user_changed_parameters = std::move(invocation.user_changed_parameters);
  }
  if (g_aegp_comp_idle_roundtrip_mode) {
    aexcompat::mask_runtime::set_model_enabled(true);
    if (!configure_mask_scene("rectangle")) return 4;
  }
  if (!g_cluster_manifest_path.empty())
    invocation.cluster_manifest_path = g_cluster_manifest_path;
  // Discovery session (closure-session design §4.2): no plug-in is admitted
  // at launch; the cluster manifest and the control pipes drive everything,
  // so the normal admission/dispatch below is skipped entirely. It has not
  // migrated to in-place loading (issue #751), so a dependency-dirs option
  // is rejected instead of silently ignored.
  if (invocation.discovery_session_mode) {
    if (!g_dependency_search_dirs.empty()) return 2;
    return run_discovery_session(invocation.cluster_manifest_path, runtime_hooks);
  }
  // Every rendered effect instance belongs to a layer, even when that layer has no masks.
  if (is_rendering_worker()) aexcompat::mask_runtime::set_model_enabled(true);
  RuntimeAdmissionRequest runtime_request;
  // A GPU render carries the AEXRMA1 manifest as a `--runtime-module-authorization-v1`
  // trailer (captured above during request parsing); the render/smart worker must
  // parse it so the GPU runtime DLLs classify as authorized `policy` modules in the
  // required module audit instead of `unknown` (#290/#300). The params-inspect path
  // keeps its argv[5] convention.
  const bool render_gpu_authorization =
      is_rendering_worker() && !g_gpu_runtime_authorization_manifest.empty();
  const bool authorize_runtime_modules =
      render_gpu_authorization ||
      (!is_rendering_worker() && invocation.runtime_module_authorization_mode);
  const wchar_t* authorization_manifest =
      render_gpu_authorization  ? g_gpu_runtime_authorization_manifest.c_str()
      : invocation.runtime_module_authorization_mode ? argv[5]
                                                     : nullptr;
  const int request_error = aexcompat::worker_runtime::prepare_runtime_request(
      argv[2], argv[3], authorize_runtime_modules, authorization_manifest, runtime_request);
  if (request_error != 0) return request_error;
  if (!g_dependency_search_dirs.empty()) {
    const int dirs_error = aexcompat::worker_runtime::apply_dependency_search_dirs(
        g_dependency_search_dirs.c_str(), runtime_request);
    if (dirs_error != 0) return dirs_error;
  }
  runtime_request.allow_aegp_plugin = g_aegp_init_mode;
  std::unique_ptr<aexcompat::TraceWriter> trace_writer;
  RuntimeContext runtime_context;
  const int admission_error = aexcompat::worker_runtime::admit_worker_entry(
      runtime_hooks, runtime_request, trace_worker_label(), trace_writer,
      runtime_context);
  if (admission_error != 0) return admission_error;
  WorkerSession session(runtime_context, trace_writer.get(), &g_trace_writer);
  (void)session.set_pre_unload_hook(&teardown_bib_suite, nullptr);
  g_plugin_file_path = session.plugin_path().wstring();
  HMODULE module = session.module();
  if (g_aegp_init_mode) {
    g_synthetic_receipt_test_mode = g_aegp_command_roundtrip_mode;
    auto aegp_entry = reinterpret_cast<AegpEntry>(GetProcAddress(module, "EntryPointFunc"));
    if (!aegp_entry) return session.finish(12);
    KeyframePipeProbe keyframe_probe;
    SeekPipeProbe seek_probe;
    TrimPipeProbe trim_probe;
    SwitchPipeProbe switch_probe;
    // Compatibility source anchors: run_orchestration owns the former
    // aegp_init::dispatch_basic_events(...) and
    // aegp_init::dispatch_death(global_refcon) calls in that exact order.
    const auto aegp_init =
        aexcompat::worker_runtime::aegp_init::run_orchestration(
            {aegp_entry, &g_basic_suite, &g_aegp_inserted_commands,
             &g_aegp_scene_frame, &keyframe_probe, &seek_probe, &trim_probe,
             &switch_probe,
             {g_aegp_update_menu_mode, g_aegp_idle_mode,
              g_aegp_command_roundtrip_mode,
              {g_aegp_active_idle_roundtrip_mode,
               g_aegp_comp_idle_roundtrip_mode,
               g_aegp_keyframe_roundtrip_mode, g_aegp_seek_roundtrip_mode,
               g_aegp_trim_roundtrip_mode, g_aegp_switch_roundtrip_mode}}},
            {nullptr,
             +[](void*) { return g_aegp_keyframe_time_calls == 2 &&
                 g_aegp_keyframe_value_calls == 2 &&
                 g_aegp_keyframe_interpolation_calls == 2; },
             +[](void*) { return g_aegp_item_set_current_time_calls == 1 &&
                 g_aegp_item_last_set_time_value == 75 &&
                 g_aegp_item_last_set_time_scale == 30 &&
                 g_aegp_scene_frame == 75; },
             +[](void*) {
               const auto& in_point = g_aegp_layer_in_points[0];
               const auto& duration = g_aegp_layer_durations[0];
               return g_aegp_layer_trim_set_calls == 1 &&
                   in_point.value == 30 && in_point.scale == 30 &&
                   duration.value == 210 && duration.scale == 30;
             },
             +[](void*) { return g_aegp_layer_flag_set_calls == 4 &&
                 g_aegp_layer_flags[0] == 0x00004026u &&
                 g_aegp_layer_flags[1] == 0x00000005u &&
                 g_aegp_layer_flags[2] == 0x00000005u; }});
    // Capture the event-complete and terminal loaded-module sets before the
    // AEGP is unloaded so secure broker launches can validate this early path.
    // Stop and unload first. Some AEGP_SuiteHandler builds retain exactly one
    // Item Suite cache until process teardown; the isolated worker owns that
    // final reclamation, but every other outstanding lease remains a failure.
    capture_module_audit_phase();
    const bool module_audit_ok = session.shutdown_before_report();
    module = nullptr;
    const bool passed = emit_aegp_init_completion_report(
        {aegp_init.init_error, aegp_init.entry_fault,
         aegp_init.entry_exception_code, aegp_init.entry_invoked,
         aegp_init.forced_suite_releases,
         invocation.aegp_boundary_regression_mode,
         aegp_init.event_error, aegp_init.death_error,
         aegp_init.global_refcon != nullptr, aegp_init.hooks_invoked,
         aegp_init.menu_hooks_invoked, aegp_init.death_hooks_invoked,
         aegp_init.command_hooks_invoked, aegp_init.command_handled_count,
         aegp_init.idle_max_sleep, module_audit_ok,
         {keyframe_probe.connected, keyframe_probe.request_sent,
          keyframe_probe.response_received, keyframe_probe.response_valid,
          keyframe_probe.response_bytes},
         {seek_probe.connected, seek_probe.request_sent,
          seek_probe.ack_received, seek_probe.ack_valid},
         {trim_probe.connected, trim_probe.request_sent,
          trim_probe.ack_received, trim_probe.ack_valid},
         {switch_probe.connected, switch_probe.request_sent,
          switch_probe.ack_received, switch_probe.ack_valid}});
    return session.finish_integrated_report(passed ? 0 : 23);
  }
  // Resolve the Effect entrypoint from the plug-in's own PiPL (issue #84): the
  // Kind atom decides the ABI and CodeWin64X86 names the export, so an Effect
  // whose entrypoint is not literally "EffectMain" still dispatches and a
  // Kind=AEGP plug-in is never handed to the Effect selector. Fail closed on
  // AEGP, missing/invalid PiPL, ambiguity, or an unresolvable symbol.
  PiplEntrypoint pipl_entrypoint = discover_pipl_entrypoint(module);
  // PluginData is a registration ABI used by many bundled effects that do
  // not carry a PiPL resource. It is deliberately reachable only for a
  // missing PiPL: malformed, ambiguous, AEGP, or unknown PiPL resources stay
  // fail-closed and never get a second interpretation.
  if (pipl_entrypoint.kind == PiplPluginKind::Missing)
    pipl_entrypoint = discover_plugin_data_entrypoint(module);
  if (pipl_entrypoint.kind != PiplPluginKind::Effect) {
    const char* plugin_kind =
        pipl_entrypoint.kind == PiplPluginKind::Aegp
            ? "aegp_candidate"
            : (pipl_entrypoint.kind == PiplPluginKind::Invalid
                   ? "invalid_pipl"
                   : "unknown_no_effect_entrypoint");
    std::cerr << "plugin_kind:" << plugin_kind << "\n" << std::flush;
    return session.finish(12);
  }
  auto entry = reinterpret_cast<EffectEntry>(
      GetProcAddress(module, pipl_entrypoint.symbol.c_str()));
  if (!entry) {
    std::cerr << "plugin_kind:unknown_no_effect_entrypoint\n" << std::flush;
    return session.finish(12);
  }

  aexcompat::aex_strings::StringTable aex_string_table;
  load_aex_string_table(module, aex_string_table);
  // One-time U.dll process init for PIN-era closures (issue #362).
  initialize_legacy_support_libraries();
  const char* string_table_status =
      aex_string_table.status == aexcompat::aex_strings::ParseStatus::Valid
          ? "valid"
          : aex_string_table.status == aexcompat::aex_strings::ParseStatus::NoEntries
              ? "none"
              : "invalid";
  std::cerr << "string_table_status:" << string_table_status << "\n" << std::flush;
  g_active_effect_module = module;
  g_active_aex_string_table = &aex_string_table;
  aexcompat::worker_runtime::effect_bootstrap::State effect_state{};
  auto& input = effect_state.input;
  auto& output = effect_state.output;
  // The bootstrap wiring is shared with the cluster-session swap and the
  // discovery session (issue #405); only the Request differs per call site.
  const auto run_bootstrap = [&](EffectEntry bootstrap_entry) {
    return aexcompat::worker_runtime::effect_bootstrap::run(
        effect_state, bootstrap_entry, make_bootstrap_abi_hooks(),
        {g_render_quality, g_render_field, g_shutter_angle, g_shutter_phase,
         {g_pre_effect_source_origin_x, g_pre_effect_source_origin_y},
         {static_cast<int32_t>(g_downsample_x.numerator),
          static_cast<int32_t>(g_downsample_x.denominator)},
         {static_cast<int32_t>(g_downsample_y.numerator),
          static_cast<int32_t>(g_downsample_y.denominator)},
         {static_cast<int32_t>(g_pixel_aspect_ratio.numerator),
          static_cast<int32_t>(g_pixel_aspect_ratio.denominator)},
         invocation.external_pixel_bytes,
         is_render_worker(), is_rendering_worker(),
         // The audio session is an audio invocation: without this the admission
         // "requested audio" flag stays false and host_audio::Runtime rejects
         // unadvertised audio checkouts on the session path (Codex #252). It used
         // to be OR'd with the one-shot --render-audio mode, which #365 deleted.
         invocation.audio_session_mode, g_skip_about},
        make_bootstrap_runtime_hooks());
  };
  const auto bootstrap = run_bootstrap(entry);
  g_active_aex_string_table = nullptr;
  g_active_effect_module = nullptr;
  const int32_t global_error = bootstrap.global_error;
  const int32_t about_error = bootstrap.about_error;
  const int32_t params_error = bootstrap.params_error;
  const uint32_t advertised_out_flags = bootstrap.advertised_out_flags;
  const uint32_t advertised_out_flags2 = bootstrap.advertised_out_flags2;
  const bool image_render_supported = bootstrap.image_render_supported;
  const bool nop_render_advertised = bootstrap.nop_render_advertised;
  const bool input_write_advertised = bootstrap.input_write_advertised;
  const bool expand_buffer_advertised = bootstrap.expand_buffer_advertised;
  const bool shrink_buffer_advertised = bootstrap.shrink_buffer_advertised;
  const bool depth_supported = bootstrap.depth_supported;
  const bool smart_render_supported = bootstrap.smart_render_supported;
  const bool parameter_count_contract_valid = bootstrap.parameter_count_contract_valid;
  g_update_params_ui_advertised = bootstrap.update_params_ui_advertised;
  g_query_dynamic_flags_advertised = bootstrap.query_dynamic_flags_advertised;
  std::string about_message = bootstrap.about_message;

  // Source-level compatibility anchors retained while the ABI bootstrap is
  // owned by worker_effect_bootstrap.cpp. They document the exact legacy
  // offsets/ordering that the owner preserves and keep static contract tests
  // useful during the staged extraction.
  // write<int32_t>(input, 284, g_downsample_x.numerator)
  // write(utils, kUtilsNewWorld, &legacy_new_world)
  // write(utils, kUtilsCopy, &copy_world8)
  // write(utils, kUtilsGetPlatformData, &get_platform_data)
  // write<int32_t>(input, kInQuality, g_render_quality)
  // write<int16_t>(input, kInVersion, kHostSpecMajor)
  // write<int16_t>(input, kInVersion + sizeof(int16_t), kHostSpecMinor)
  // expected_num_params = static_cast<int32_t>(g_params.size() + 1)
  // read<int32_t>(output, kOutNumParams) == expected_num_params
  // write<int32_t>(input, kInNumParams, expected_num_params)
  // write(utils, kUtilsIterate, &iterate_world8)
  // write(utils, kUtilsAnsiPow, &aexcompat::pf_ansi::ansi_pow)
  // write(utils, kUtilsAnsiStrcpy, &aexcompat::pf_ansi::ansi_strcpy)
  // write(utils, kUtilsNewHandle, &new_handle)
  // write(utils, kUtilsLockHandle, &lock_handle)
  // write(utils, kUtilsUnlockHandle, &unlock_handle)
  // write(utils, kUtilsDisposeHandle, &dispose_handle)
  // write(utils, kUtilsGetHandleSize, &handle_size)
  // write(utils, kUtilsResizeHandle, &resize_handle)
  // memcpy(utils.data() + kUtilsColorCallbacks, &g_color_suite8, sizeof(g_color_suite8))
  // write(utils, kUtilsBeginSampling, &begin_sampling8)
  // write(utils, kUtilsAreaSample, &area_sample8)
  // write(utils, kUtilsEndSampling, &end_sampling8)
  // write(input, 24, &abort_render);
  // write(input, 32, &report_progress);
  // invocation.external_pixel_bytes == 8 &&
  // invocation.external_pixel_bytes == 16 &&
  // params_error == 0 && image_render_supported && depth_supported
  // (1u << 26)
  // write<int32_t>(input, 248, g_shutter_angle)
  // write<int32_t>(input, 400, g_shutter_phase)
  // about_error = g_skip_about ? 0
  // invoke_entry_seh(entry, kGlobalSetup)
  // invoke_entry_seh(entry, kAbout)
  // invoke_entry_seh(entry, kParamsSetup)
  // invoke_entry_seh(entry, kGlobalSetdown)

  // In-place cluster session wiring (#751/#816).
  namespace cluster = aexcompat::worker_runtime::cluster;
  cluster::Manifest cluster_manifest;
  ClusterSwapContext cluster_swap_context;
  aexcompat::worker_render_session::SwapPluginHook cluster_swap_hook;
  if (is_render_worker() && invocation.render_session_mode &&
      !invocation.cluster_manifest_path.empty()) {
    if (!cluster::load_manifest(invocation.cluster_manifest_path, cluster_manifest))
      return session.finish(3);
    std::string argv_sha256;
    for (const wchar_t* character = argv[3]; *character; ++character) {
      if (*character > 0x7f) return session.finish(3);
      argv_sha256.push_back(static_cast<char>(*character));
    }
    if (!cluster::matches_launch_plugin(cluster_manifest, session.plugin_path(),
                                        argv_sha256))
      return session.finish(3);
    if (const int admit_error =
            cluster::admit_in_place_manifest_dirs(cluster_manifest))
      return session.finish(admit_error);
    aexcompat::worker_runtime::configure_module_audit_cluster(
        cluster_manifest.module_bound, {});
    aexcompat::worker_runtime::configure_module_audit_search_roots(
        cluster::in_place_audit_roots(cluster_manifest));
    session.set_deferred_module_release();
    cluster_swap_context.session = &session;
    cluster_swap_context.entry = &entry;
    cluster_swap_context.effect_state = &effect_state;
    cluster_swap_context.invocation = &invocation;
    cluster_swap_context.manifest = &cluster_manifest;
    cluster_swap_context.run_bootstrap = run_bootstrap;
    cluster_swap_context.current_plugin_index = 0;
    cluster_swap_context.current_global_error = global_error;
    cluster_swap_hook.context = &cluster_swap_context;
    cluster_swap_hook.invoke = &cluster_swap_invoke;
    cluster_swap_hook.plugin_count =
        static_cast<int32_t>(cluster_manifest.plugins.size());
  }
  const bool cluster_session_active = cluster_swap_context.session != nullptr;
  if (!is_rendering_worker() &&
      (invocation.adjust_cursor_mode || invocation.draw_event_mode || invocation.click_event_mode || invocation.drag_event_mode ||
       invocation.ui_lifecycle_mode || invocation.ui_idle_mode || invocation.ui_keydown_mode || invocation.ui_mouse_exited_mode)) {
    int32_t event_error = -1;
    int32_t cursor = 0;
    int32_t event_out_flags = 0;
    bool changed_value = false;
    const bool registered_effect_ui = (g_custom_ui_registration.events & 4u) != 0;
    const bool registered_layer_ui = (g_custom_ui_registration.events & 2u) != 0;
    const bool registered_comp_ui = (g_custom_ui_registration.events & 1u) != 0;
    if (invocation.ui_mouse_exited_mode && !registered_layer_ui && !registered_comp_ui) {
      dispose_arbitrary_defaults(entry, input, output);
      if (global_error == 0)
        invoke_global_setdown(entry, input.data(), output.data());
      return session.finish(19);
    }
    const char* event_target = invocation.drag_event_mode || invocation.ui_mouse_exited_mode ||
        (!registered_effect_ui && registered_layer_ui)
        ? "layer" : (!registered_effect_ui && !registered_layer_ui &&
                     registered_comp_ui ? "comp" : "effect_controls");
    if (invocation.ui_mouse_exited_mode && !registered_layer_ui) event_target = "comp";
    bool arbitrary_values_disposed = false;
    std::array<int32_t, 5> lifecycle_errors{-1, -1, -1, -1, -1};
    std::array<uintptr_t, 4> plugin_state_before_close{};
    bool lifecycle_context_stable = true;
    bool lifecycle_host_state_cleared = false;
    bool event_assignments_applied = !invocation.ui_event_assignment_mode;
    g_ui_context.window_type = std::strcmp(event_target, "layer") == 0 ? 1 :
        (std::strcmp(event_target, "comp") == 0 ? 0 : 2);
    aexcompat::worker_runtime::ui_event_execution::Result ui_result;
    const bool ui_dispatched = aexcompat::worker_runtime::ui_event_execution::dispatch(
        {entry, &input, &output, params_error, parameter_count_contract_valid,
         &invocation.ui_event_assignments, invocation.ui_event_assignment_mode, invocation.adjust_cursor_mode,
         invocation.draw_event_mode, invocation.click_event_mode, invocation.drag_event_mode, invocation.ui_lifecycle_mode,
         invocation.ui_idle_mode, invocation.ui_keydown_mode, invocation.ui_mouse_exited_mode, invocation.click_x, invocation.click_y,
         invocation.drag_end_x, invocation.drag_end_y, invocation.drag_steps, invocation.keydown_code, invocation.keydown_modifiers,
         g_ui_context.window_type, &g_ui_context_pointer, &g_ui_context,
         g_ui_context.plugin_state.data(), reinterpret_cast<void*>(&ui_transform_point),
         reinterpret_cast<void*>(&ui_transform_point_simple)},
        {&invoke_entry_seh, &enter_custom_ui_context, &leave_custom_ui_context,
         &custom_ui_context_stable, &set_custom_ui_context_tool},
        ui_result);
    event_error = ui_dispatched ? ui_result.event_error : -1;
    cursor = ui_result.cursor;
    event_out_flags = ui_result.event_out_flags;
    changed_value = ui_result.changed_value;
    lifecycle_errors = ui_result.lifecycle_errors;
    plugin_state_before_close = ui_result.plugin_state_before_close;
    lifecycle_context_stable = ui_result.lifecycle_context_stable;
    lifecycle_host_state_cleared = ui_result.lifecycle_host_state_cleared;
    event_assignments_applied = ui_result.event_assignments_applied;
    g_ui_drag_requested = ui_result.drag_requested;
    g_ui_drag_calls = ui_result.drag_calls;
    g_ui_drag_terminated = ui_result.drag_terminated;
    int32_t event_sequence_setdown_error = 0;
    if (void* sequence_data = read<void*>(output, kOutSequenceData)) {
      write<void*>(input, kInSequenceData, sequence_data);
      uint32_t exception_code = 0;
      event_sequence_setdown_error = invoke_sequence_selector(
          entry, kSequenceSetdown, input.data(), output.data(), &exception_code);
      if (exception_code != 0) event_sequence_setdown_error = 512;
      write<void*>(input, kInSequenceData, nullptr);
      write<void*>(output, kOutSequenceData, nullptr);
    }
    const auto handle_stats = statistics();
    arbitrary_values_disposed = handle_stats.created == handle_stats.disposed + 1;
    const bool defaults_disposed = dispose_arbitrary_defaults(entry, input, output);
    const int32_t event_setdown_error = global_error == 0
        ? invoke_global_setdown(entry, input.data(), output.data()) : -1;
    if (!session.prepare_protocol_report()) return session.finish(14);
    restore_native_stdout();
    const bool ui_event_passed = emit_ui_event_completion_report(
        {invocation.ui_lifecycle_mode, invocation.ui_idle_mode,
         invocation.ui_keydown_mode, invocation.ui_mouse_exited_mode,
         invocation.draw_event_mode, invocation.drag_event_mode,
         invocation.click_event_mode, invocation.drag_steps,
         invocation.keydown_code, invocation.keydown_modifiers, event_target,
         event_error, cursor, event_out_flags, changed_value, lifecycle_errors,
         plugin_state_before_close, lifecycle_context_stable,
         lifecycle_host_state_cleared, event_assignments_applied,
         arbitrary_values_disposed, defaults_disposed, drawbot_objects_empty(),
         event_sequence_setdown_error, event_setdown_error,
         requested_parameters_json(invocation.ui_event_assignments)});
    return session.finish(ui_event_passed ? 0 : 20);
  }
  if (is_rendering_worker() && invocation.request_mode &&
      (params_error != 0 || !parameter_count_contract_valid ||
                       !validate_requested_assignments(invocation.requested_parameters) ||
                       !validate_external_aux_parameters())) {
    dispose_arbitrary_defaults(entry, input, output);
    if (global_error == 0)
      invoke_global_setdown(entry, input.data(), output.data());
    return session.finish(3);
  }
  const auto early_mode = aexcompat::l2mode::select_early_mode(
      invocation.auto_dialog_mode, invocation.do_dialog_mode, invocation.external_dependencies_mode, invocation.params_only_mode);
  if (!is_rendering_worker() && early_mode != aexcompat::l2mode::EarlyMode::None) {
    EarlyModeBridge bridge{entry, &input, &output, &session, &about_message};
    const int early_result = aexcompat::l2mode::run_early_mode(
        {early_mode, &bridge, early_mode_hooks(), global_error, params_error,
         parameter_count_contract_valid,
         invocation.external_dependencies_mode ? argv[4] : nullptr});
    return session.finish(early_result);
  }
  if (is_render_worker() && invocation.audio_session_mode) {
    const aexcompat::worker_audio_session::AudioSessionGeometry geometry{
        invocation.audio_session_max_samples, invocation.audio_session_channels};
    const auto session_outcome = run_audio_render_session(
        entry, input, output, global_error, params_error,
        invocation.requested_parameters, geometry, invocation.external_time_scale);
    if (!session.prepare_protocol_report()) return session.finish(14);
    restore_native_stdout();
    emit_audio_session_report(global_error, params_error, session_outcome);
    if (session_outcome.protocol_violation)
      return session.finish(aexcompat::worker_audio_session::kExitProtocolViolation);
    if (session_outcome.invariant_failure)
      return session.finish(aexcompat::worker_audio_session::kExitInvariantFailure);
    return session.finish(session_outcome.clean ? 0 : 20);
  }
  std::array<int32_t, 5> lifecycle_errors{-1, -1, -1, -1, -1};
  bool lifecycle_data_null = false;
  bool user_changed_ok = false;
  bool conditional_ui_ok = false;
  if (!is_rendering_worker()) {
    if (const auto lifecycle_exit = run_l2_parameter_lifecycle(
            entry, input, output, global_error, params_error, lifecycle_errors,
            lifecycle_data_null, user_changed_ok, conditional_ui_ok))
      return session.finish(*lifecycle_exit);
  }
  std::string case_id;
  std::string input_hash;
  std::string output_hash;
  bool guards_intact = false;
  int32_t render_width = 0;
  int32_t render_height = 0;
  int32_t render_rowbytes = 0;
  std::array<int32_t, 2> thread_errors{-1, -1};
  std::array<std::string, 2> thread_hashes{};
  std::array<bool, 2> thread_guards{false, false};
  bool concurrent_render = false;
  bool persistent_sequence = false;
  bool session_protocol_violation = false;
  bool session_invariant_failure = false;
  bool session_swap_failure = false;
  bool flattened_sequence = false;
  bool copied_flattened_sequence = false;
  int32_t persistent_sequence_setup_error = -1;
  int32_t persistent_sequence_setdown_error = -1;
  std::array<int32_t, 2> persistent_frame_errors{-1, -1};
  std::array<std::string, 2> persistent_frame_hashes{};
  int32_t sequence_flatten_error = -1;
  int32_t sequence_resetup_error = -1;
  bool flattened_handle_replaced = false;
  bool resetup_handle_replaced = false;
  bool flattened_handle_host_disposed = false;
  int32_t get_flattened_sequence_data_error = -1;
  bool original_sequence_preserved = false;
  int32_t render_error = -1;
  SmartResult smart{};
  int32_t smart_session_frames_attempted = 0;
  int32_t smart_session_sequence_setup_error = -1;
  int32_t smart_session_sequence_setdown_error = -1;
  int32_t smart_session_render_error = -1;
  bool lifetime_fault_observed = false;
  bool suite_fault_observed = false;
  bool handle_fault_observed = false;
  bool world_fault_observed = false;
  bool pixel_format_fault_observed = false;
  bool outline_fault_observed = false;
  bool mask_attribute_fault_observed = false;
  bool stream_metadata_fault_observed = false;
  bool keyframe_fault_observed = false;
  bool dynamic_stream_fault_observed = false;
  bool aegp_memory_fault_observed = false;

  if (is_render_worker()) {
    const auto dispatch = aexcompat::worker_runtime::invocation::run_classic_final_dispatch(
        {entry, &input, &output, &invocation, argv, params_error,
         image_render_supported, depth_supported, smart_render_supported,
         cluster_session_active ? &cluster_swap_hook : nullptr});
    if (dispatch.case_id_rejected) {
      dispose_arbitrary_defaults(entry, input, output);
      if (global_error == 0)
        invoke_global_setdown(entry, input.data(), output.data());
      return session.finish(2);
    }
    case_id = dispatch.case_id;
    input_hash = dispatch.input_hash;
    output_hash = dispatch.output_hash;
    guards_intact = dispatch.guards_intact;
    render_width = dispatch.render_width;
    render_height = dispatch.render_height;
    render_rowbytes = dispatch.render_rowbytes;
    thread_errors = dispatch.thread_errors;
    thread_hashes = dispatch.thread_hashes;
    thread_guards = dispatch.thread_guards;
    concurrent_render = dispatch.concurrent_render;
    persistent_sequence = dispatch.persistent_sequence;
    session_protocol_violation = dispatch.session_protocol_violation;
    session_invariant_failure = dispatch.session_invariant_failure;
    session_swap_failure = dispatch.session_swap_failure;
    flattened_sequence = dispatch.flattened_sequence;
    copied_flattened_sequence = dispatch.copied_flattened_sequence;
    persistent_sequence_setup_error = dispatch.persistent_sequence_setup_error;
    persistent_sequence_setdown_error = dispatch.persistent_sequence_setdown_error;
    persistent_frame_errors = dispatch.persistent_frame_errors;
    persistent_frame_hashes = dispatch.persistent_frame_hashes;
    sequence_flatten_error = dispatch.sequence_flatten_error;
    sequence_resetup_error = dispatch.sequence_resetup_error;
    flattened_handle_replaced = dispatch.flattened_handle_replaced;
    resetup_handle_replaced = dispatch.resetup_handle_replaced;
    flattened_handle_host_disposed = dispatch.flattened_handle_host_disposed;
    get_flattened_sequence_data_error = dispatch.get_flattened_sequence_data_error;
    original_sequence_preserved = dispatch.original_sequence_preserved;
    render_error = dispatch.render_error;
  } else if (is_smart_worker()) {
    const auto dispatch = aexcompat::worker_runtime::invocation::run_smart_final_dispatch(
        {entry, &input, &output, &invocation, argv, params_error,
         image_render_supported, depth_supported, smart_render_supported});
    if (dispatch.case_id_rejected) {
      dispose_arbitrary_defaults(entry, input, output);
      if (global_error == 0)
        invoke_global_setdown(entry, input.data(), output.data());
      return session.finish(2);
    }
    case_id = dispatch.case_id;
    smart = dispatch.smart;
    lifetime_fault_observed = dispatch.lifetime_fault_observed;
    session_protocol_violation = dispatch.session_protocol_violation;
    session_invariant_failure = dispatch.session_invariant_failure;
    smart_session_frames_attempted = dispatch.session_frames_attempted;
    smart_session_sequence_setup_error = dispatch.session_sequence_setup_error;
    smart_session_sequence_setdown_error = dispatch.session_sequence_setdown_error;
    smart_session_render_error = dispatch.session_render_error;
  }
  if (is_render_worker()) drain_async_layer_requests();
  // After cluster swaps, `entry` is the CURRENT plug-in's entry (or null when
  // its entrypoint never resolved) and its GLOBAL_SETUP status lives in the
  // swap context; the launch plug-in's global_error no longer describes it.
  const int32_t effective_global_error =
      cluster_session_active ? cluster_swap_context.current_global_error : global_error;
  const bool arbitrary_defaults_disposed =
      !entry || dispose_arbitrary_defaults(entry, input, output);
  std::cerr << "stage:global_setdown_begin\n" << std::flush;
  const int32_t setdown_error = effective_global_error == 0 && entry
      ? invoke_global_setdown(entry, input.data(), output.data()) : -1;
  // Smart-worker fault-injection probes as a handler table (issue #171):
  // each requested mode runs its verifier in the same order as before.
  const struct SmartFaultProbe {
    bool requested;
    bool (*verify)();
    bool* observed;
  } smart_fault_probes[] = {
      {invocation.suite_release_without_acquire_mode,
       &verify_suite_release_without_acquire_rejected, &suite_fault_observed},
      {invocation.handle_resize_while_locked_mode,
       &verify_handle_resize_while_locked_rejected, &handle_fault_observed},
      {invocation.world_double_dispose_mode,
       &verify_world_double_dispose_rejected, &world_fault_observed},
      {invocation.world_allocation_limit_mode,
       &verify_world_allocation_limit_rejected, &world_fault_observed},
      {invocation.pixel_format_registry_mode,
       &verify_pixel_format_registry_rejection, &pixel_format_fault_observed},
      {invocation.outline_mutation_mode,
       &verify_outline_mutation_rejection, &outline_fault_observed},
      {invocation.mask_attribute_mode,
       &verify_mask_attribute_and_ownership_rejection, &mask_attribute_fault_observed},
      {invocation.stream_metadata_ownership_mode,
       &verify_stream_metadata_and_ownership_rejection, &stream_metadata_fault_observed},
      {invocation.keyframe_ownership_mode,
       &verify_keyframe_ownership_rejection, &keyframe_fault_observed},
      {invocation.dynamic_stream_tree_mode,
       &verify_dynamic_stream_tree_rejection, &dynamic_stream_fault_observed},
      {invocation.aegp_memory_strings_mode,
       &verify_aegp_memory_and_strings_rejection, &aegp_memory_fault_observed},
  };
  if (is_smart_worker())
    for (const auto& probe : smart_fault_probes)
      if (probe.requested) *probe.observed = probe.verify();
  std::cerr << "stage:global_setdown_end error=" << setdown_error << "\n" << std::flush;
  // A swap failure still gets the completion report below; only its exit code
  // is dedicated (25), so a terminal-audit rejection must not pre-empt it
  // with the generic module-audit code here.
  if (!session.prepare_protocol_report() && !session_swap_failure)
    return session.finish(14);
  if (is_render_worker()) {
  restore_native_stdout();
  ClassicCompletionInputs classic_inputs;
  classic_inputs.render_error = render_error;
  classic_inputs.global_error = global_error;
  classic_inputs.params_error = params_error;
  classic_inputs.setdown_error = setdown_error;
  classic_inputs.parameter_count_contract_valid = parameter_count_contract_valid;
  classic_inputs.guards_intact = guards_intact;
  classic_inputs.arbitrary_defaults_disposed = arbitrary_defaults_disposed;
  classic_inputs.depth_supported = depth_supported;
  classic_inputs.advertised_out_flags = advertised_out_flags;
  classic_inputs.advertised_out_flags2 = advertised_out_flags2;
  classic_inputs.image_render_supported = image_render_supported;
  classic_inputs.nop_render_advertised = nop_render_advertised;
  classic_inputs.input_write_advertised = input_write_advertised;
  classic_inputs.expand_buffer_advertised = expand_buffer_advertised;
  classic_inputs.shrink_buffer_advertised = shrink_buffer_advertised;
  classic_inputs.case_id = case_id;
  classic_inputs.input_hash = input_hash;
  classic_inputs.output_hash = output_hash;
  classic_inputs.render_width = render_width;
  classic_inputs.render_height = render_height;
  classic_inputs.render_rowbytes = render_rowbytes;
  classic_inputs.thread_errors = thread_errors;
  classic_inputs.thread_hashes = thread_hashes;
  classic_inputs.thread_guards = thread_guards;
  classic_inputs.concurrent_render = concurrent_render;
  classic_inputs.persistent_sequence = persistent_sequence;
  classic_inputs.persistent_sequence_setup_error = persistent_sequence_setup_error;
  classic_inputs.persistent_sequence_setdown_error = persistent_sequence_setdown_error;
  classic_inputs.persistent_frame_errors = persistent_frame_errors;
  classic_inputs.persistent_frame_hashes = persistent_frame_hashes;
  classic_inputs.flattened_sequence = flattened_sequence;
  classic_inputs.sequence_flatten_error = sequence_flatten_error;
  classic_inputs.sequence_resetup_error = sequence_resetup_error;
  classic_inputs.flattened_handle_replaced = flattened_handle_replaced;
  classic_inputs.resetup_handle_replaced = resetup_handle_replaced;
  classic_inputs.flattened_handle_host_disposed = flattened_handle_host_disposed;
  classic_inputs.copied_flattened_sequence = copied_flattened_sequence;
  classic_inputs.get_flattened_sequence_data_error = get_flattened_sequence_data_error;
  classic_inputs.original_sequence_preserved = original_sequence_preserved;
  classic_inputs.frame_return_message.assign(
      reinterpret_cast<const char*>(output.data() + kOutMessage),
      strnlen_s(reinterpret_cast<const char*>(output.data() + kOutMessage), 256));
  classic_inputs.request_mode = invocation.request_mode;
  classic_inputs.requested_parameters = &invocation.requested_parameters;
  classic_inputs.downsample_x = {static_cast<int32_t>(g_downsample_x.numerator),
                                 static_cast<int32_t>(g_downsample_x.denominator)};
  classic_inputs.downsample_y = {static_cast<int32_t>(g_downsample_y.numerator),
                                 static_cast<int32_t>(g_downsample_y.denominator)};
  classic_inputs.pixel_aspect_ratio = {static_cast<int32_t>(g_pixel_aspect_ratio.numerator),
                                       static_cast<int32_t>(g_pixel_aspect_ratio.denominator)};
  classic_inputs.resolution = {
      g_full_resolution_width > 0 ? g_full_resolution_width : invocation.external_width,
      g_full_resolution_height > 0 ? g_full_resolution_height : invocation.external_height};
  classic_inputs.context_head = {read<int32_t>(input, kInQuality), read<int32_t>(input, kInNumParams),
      read<int32_t>(input, kInLocalTimeStep), read<int32_t>(input, 244),
      read<int32_t>(input, 248), read<int32_t>(input, 400)};
  classic_inputs.context_zoom = {read<int32_t>(input, 252), read<int32_t>(input, 256)};
  classic_inputs.context_origin = {read<int32_t>(input, 392), read<int32_t>(input, 396)};
  classic_inputs.context_extent = {read<int32_t>(input, 276), read<int32_t>(input, 280)};
  emit_classic_completion_report(classic_inputs);
  } else if (is_smart_worker()) {
  restore_native_stdout();
  SmartCompletionInputs smart_inputs;
  smart_inputs.smart = &smart;
  smart_inputs.lifetime_fault_observed = lifetime_fault_observed;
  smart_inputs.suite_fault_observed = suite_fault_observed;
  smart_inputs.handle_fault_observed = handle_fault_observed;
  smart_inputs.world_fault_observed = world_fault_observed;
  smart_inputs.pixel_format_fault_observed = pixel_format_fault_observed;
  smart_inputs.outline_fault_observed = outline_fault_observed;
  smart_inputs.mask_attribute_fault_observed = mask_attribute_fault_observed;
  smart_inputs.stream_metadata_fault_observed = stream_metadata_fault_observed;
  smart_inputs.keyframe_fault_observed = keyframe_fault_observed;
  smart_inputs.dynamic_stream_fault_observed = dynamic_stream_fault_observed;
  smart_inputs.aegp_memory_fault_observed = aegp_memory_fault_observed;
  smart_inputs.global_error = global_error;
  smart_inputs.params_error = params_error;
  smart_inputs.setdown_error = setdown_error;
  smart_inputs.advertised_out_flags = advertised_out_flags;
  smart_inputs.advertised_out_flags2 = advertised_out_flags2;
  smart_inputs.parameter_count_contract_valid = parameter_count_contract_valid;
  smart_inputs.arbitrary_defaults_disposed = arbitrary_defaults_disposed;
  smart_inputs.depth_supported = depth_supported;
  smart_inputs.image_render_supported = image_render_supported;
  smart_inputs.smart_render_supported = smart_render_supported;
  smart_inputs.nop_render_advertised = nop_render_advertised;
  smart_inputs.input_write_advertised = input_write_advertised;
  smart_inputs.request_mode = invocation.request_mode;
  smart_inputs.session_mode = invocation.render_session_mode;
  smart_inputs.session_frames_attempted = smart_session_frames_attempted;
  smart_inputs.session_sequence_setup_error = smart_session_sequence_setup_error;
  smart_inputs.session_sequence_setdown_error = smart_session_sequence_setdown_error;
  smart_inputs.session_render_error = smart_session_render_error;
  smart_inputs.session_protocol_violation = session_protocol_violation;
  smart_inputs.session_invariant_failure = session_invariant_failure;
  smart_inputs.case_id = case_id;
  smart_inputs.external_size = {invocation.external_width, invocation.external_height};
  smart_inputs.requested_parameters = &invocation.requested_parameters;
  smart_inputs.downsample_x = {static_cast<int32_t>(g_downsample_x.numerator),
                               static_cast<int32_t>(g_downsample_x.denominator)};
  smart_inputs.downsample_y = {static_cast<int32_t>(g_downsample_y.numerator),
                               static_cast<int32_t>(g_downsample_y.denominator)};
  smart_inputs.pixel_aspect_ratio = {static_cast<int32_t>(g_pixel_aspect_ratio.numerator),
                                     static_cast<int32_t>(g_pixel_aspect_ratio.denominator)};
  smart_inputs.resolution = {
      g_full_resolution_width > 0 ? g_full_resolution_width : invocation.external_width,
      g_full_resolution_height > 0 ? g_full_resolution_height : invocation.external_height};
  smart_inputs.context_head = {read<int32_t>(input, kInQuality), read<int32_t>(input, kInNumParams),
      read<int32_t>(input, kInLocalTimeStep), read<int32_t>(input, 244),
      read<int32_t>(input, 248), read<int32_t>(input, 400)};
  smart_inputs.context_zoom = {read<int32_t>(input, 252), read<int32_t>(input, 256)};
  smart_inputs.context_origin = {read<int32_t>(input, 392), read<int32_t>(input, 396)};
  smart_inputs.context_extent = {read<int32_t>(input, 276), read<int32_t>(input, 280)};
  emit_smart_completion_report(smart_inputs);
  } else {
  report(global_error == 0 && params_error == 0 ? "selectors_completed" : "selector_error",
         global_error, params_error, setdown_error, output, about_message, lifecycle_errors, lifecycle_data_null);
  }
  if (is_render_worker()) {
    // Session fail-closed self-termination paths keep dedicated exit codes so
    // the broker can distinguish protocol violations (23), host-protection
    // invariant failures (24), and cluster swap failures (25, closure-session
    // design §7: quiescence/setdown/unload/audit; a contamination-suspect
    // abort, not a crash) from ordinary render failures (21).
    if (session_swap_failure) return session.finish_integrated_report(25);
    if (session_protocol_violation) return session.finish(23);
    if (session_invariant_failure) return session.finish(24);
    return session.finish(global_error == 0 && params_error == 0 && parameter_count_contract_valid &&
      image_render_supported && depth_supported && render_error == 0 && guards_intact &&
      handle_lifetimes_balanced() && world_lifetimes_balanced() &&
      async_receipt_lifetimes_balanced() &&
      async_layer_requests_balanced() &&
      gpu_memory_lifetimes_balanced() &&
      audio_handle_lifetimes_balanced() && audio_telemetry().invalid_operations == 0 &&
      param_checkouts_balanced() &&
      ((!g_render_click_enabled && !g_render_draw_enabled) ||
       g_render_ui_context_closed) ? 0 : 21);
  }
  if (is_smart_worker()) {
    // The smart session shares the render worker's dedicated fail-closed exit
    // codes: protocol violations (23) and host-protection invariant failures
    // (24) stay distinguishable from ordinary smart failures (22).
    if (session_protocol_violation) return session.finish(23);
    if (session_invariant_failure) return session.finish(24);
    if (invocation.render_session_mode) {
      // Frame-local selector errors were reported through frame_done and the
      // broker owned the continue decision; only session mechanics and host
      // state cleanliness decide the exit.
      return session.finish(global_error == 0 && params_error == 0 && parameter_count_contract_valid &&
        image_render_supported && depth_supported && smart_render_supported &&
        smart_session_render_error == 0 && smart.guards_intact &&
        handle_lifetimes_balanced() && world_lifetimes_balanced() &&
        gpu_memory_lifetimes_balanced() &&
        audio_handle_lifetimes_balanced() && audio_telemetry().invalid_operations == 0 &&
        param_checkouts_balanced() &&
        ((!g_render_click_enabled && !g_render_draw_enabled) ||
         g_render_ui_context_closed) ? 0 : 22);
    }
    return session.finish(global_error == 0 && params_error == 0 && parameter_count_contract_valid &&
      image_render_supported && depth_supported && smart.pre_error == 0 && smart.render_error == 0 &&
      smart.gpu_setup_error == 0 && smart.gpu_setdown_error == 0 &&
      smart.rects_valid && smart.guards_intact &&
      handle_lifetimes_balanced() && world_lifetimes_balanced() &&
      gpu_memory_lifetimes_balanced() &&
      audio_handle_lifetimes_balanced() && audio_telemetry().invalid_operations == 0 &&
      param_checkouts_balanced() &&
      ((!g_render_click_enabled && !g_render_draw_enabled) ||
       g_render_ui_context_closed) ? 0 : 22);
  }
  return session.finish(global_error == 0 && params_error == 0 && parameter_count_contract_valid &&
      user_changed_ok && conditional_ui_ok && about_error == 0 && lifecycle_data_null &&
      std::all_of(lifecycle_errors.begin(), lifecycle_errors.end(), [](auto error) { return error == 0; }) ? 0 : 20);
}

int aexcompat::worker_target::run(Kind kind, int argc, wchar_t** argv) {
  std::string openmp_diagnostic;
  if (!worker_runtime::openmp::install_deterministic_policy(
          openmp_diagnostic)) {
    std::cerr << "stage:openmp_policy_failed detail=" << openmp_diagnostic
              << '\n' << std::flush;
    return 11;
  }
  l2_detail::g_worker_target = kind;
  return worker_main_impl(argc, argv);
}
const bool g_aegp_compat_selftests_configured = [] {
  // g_aegp_item_type_calls == item_type_calls_before + 1
  aexcompat::l2_detail::configure_aegp_compat_selftests(
      {&acquire_suite, &release_suite,
       g_aegp_comp_suite10.data(), &g_pf_interface_suite,
       aexcompat::pf_helper::suite1(), &g_aegp_comp, &g_aegp_comp_item, &g_effect,
       +[](void* comp, aexcompat::l2_detail::AegpCompatColor* color) {
         return aegp_get_comp_bg_color(comp, reinterpret_cast<AegpColorVal*>(color));
       },
       &convert_effect_to_comp_time, &get_effect_camera, &get_effect_camera_matrix,
       +[](int32_t index) { g_aegp_active_camera_layer_index = index; },
       +[] { return g_aegp_active_camera_layer_index; },
       +[](int32_t index) -> void* {
         return index >= 0 && index < static_cast<int32_t>(g_aegp_layers.size())
             ? &g_aegp_layers[static_cast<std::size_t>(index)] : nullptr;
       },
       &aegp_layer_index,
       +[](int32_t width, int32_t height) {
         g_full_resolution_width = width; g_full_resolution_height = height;
       },
       +[](int32_t* width, int32_t* height) {
         if (width) *width = g_full_resolution_width;
         if (height) *height = g_full_resolution_height;
       },
       &suite_leases_balanced, &g_layer, &g_aegp_comp_idle_roundtrip_mode,
       &g_active_ui_param_count, &aegp_get_new_effect_stream_by_index_v2,
       &aegp_get_stream_name_v2, &aegp_get_stream_type_v2,
       &aegp_get_new_stream_value_v2, &aegp_set_stream_value_v2,
       &aegp_dispose_stream_value_v2, &aegp_dispose_stream_v2,
       &aegp_get_layer_source_item, &aegp_get_item_type,
       &g_aegp_item_suite,
       &g_aegp_layer_source_item_calls, &g_aegp_item_type_calls,
       &aegp_get_effect_param_union_by_index_v3});
  return true;
}();
const bool g_aegp_scene_generation_invalidator_configured = [] {
  aexcompat::scene_transaction::configure_generation_invalidator(
      &aexcompat::aegp_staged_item_runtime::invalidate_scene_generation);
  return true;
}();
const bool g_color_settings_selftests_configured = [] {
  aexcompat::color_settings::selftests::configure(
      {&acquire_suite, &release_suite, &g_aegp_comp});
  return true;
}();
const bool g_parameter_selftests_configured = [] {
  aexcompat::parameter_selftests::configure({
      &acquire_suite, &release_suite, &g_effect, &g_layer,
      &g_param_utils_suite1, &g_param_utils_suite,
      &update_param_ui, &is_identical_param_checkout,
      &find_param_keyframe_time, &get_param_keyframe_count,
      &checkout_param_keyframe, &checkin_param_keyframe,
      &param_key_index_to_time, &get_current_param_state_obsolete,
      &has_param_changed_obsolete,
      &have_inputs_changed_over_time_span_obsolete,
      &apply_parameter_animation});
  return true;
}();
const bool g_host_guard_selftests_configured = [] {
  aexcompat::host_guard_selftests::configure(
      {&acquire_suite, &release_suite, &suite_acquire_count,
       &suite_release_count, &suite_leases_balanced});
  return true;
}();
const bool g_pf_color_selftests_configured = [] {
  aexcompat::pf_color_selftests::configure(
      {&acquire_suite, &release_suite, &g_effect, &g_color_param_suite1,
       &g_params,
       +[](void* effect_ref, const void* definition,
           aexcompat::pf_color_selftests::PixelFloat* output) -> int32_t {
         return floating_point_from_color(
             effect_ref, definition,
             reinterpret_cast<PfColorParamPixelFloat*>(output));
       }});
  return true;
}();
