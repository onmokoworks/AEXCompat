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

std::wstring g_companion_manifest_path;
bool capture_l2_companion_manifest(void*, const wchar_t* value) {
  if (!value || !*value) return false;
  g_companion_manifest_path = value;
  return true;
}

struct SessionPreUnloadContext {
  aexcompat::worker_runtime::companions::Runtime* companions{};
};

bool __cdecl teardown_session_providers(void* opaque) noexcept {
  auto* context = static_cast<SessionPreUnloadContext*>(opaque);
  const bool companions_ok =
      !context || !context->companions || context->companions->shutdown();
  const bool bib_ok = teardown_bib_suite(nullptr);
  return companions_ok && bib_ok;
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
    bool& user_changed_ok, bool& conditional_ui_ok,
    int32_t layer_width, int32_t layer_height) {
  std::vector<std::array<std::byte, kParamSize>> lifecycle_definitions(g_params.size() + 1);
  std::array<unsigned char, 4> lifecycle_pixel{255, 0, 0, 0};
  aexcompat::world_safety::EffectWorldStorage lifecycle_world{};
  write<void*>(lifecycle_world, 24, lifecycle_pixel.data());
  write<int32_t>(lifecycle_world, 32, 4); write<int32_t>(lifecycle_world, 36, 1);
  write<int32_t>(lifecycle_world, 40, 1); write_rect(lifecycle_world.data() + 44, 1, 1);
  // The one world the host builds without render::prepare_world_layout, so it
  // needs the PF_World shape put on it here: the discovery path hands it to
  // FRAME_SETUP / FRAME_SETDOWN, and an effect that reads `world + 0x50` and
  // calls slot 1 without a null check (the Glow shape, issue #1276) would
  // otherwise dereference null right here. This covers the world handed over
  // directly; the copy inside `lifecycle_definitions[0]` carries the same
  // `reserved_long4`, but `world - 8` on *that* copy is ParamDef bytes rather
  // than a prefix, so a Spill2/Curl_Noise-shape effect computing it during
  // discovery still finds no vtable there (as it does for every ParamDef
  // world; see copy_world_into_param_def in the render runtimes).
  (void)aexcompat::worker_runtime::pf_world_facade::embed(lifecycle_world.data(), 4);
  std::memcpy(lifecycle_definitions[0].data() + 56, lifecycle_world.data(), lifecycle_world.size());
  initialize_parameter_definitions(
      lifecycle_definitions, layer_width, layer_height);
  if (g_user_changed_param_requested &&
      !apply_requested_assignments(
          lifecycle_definitions, g_user_changed_parameters,
          layer_width, layer_height)) {
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

struct L2PointDefaultProbe {
  bool frame_setup_seen{};
  std::array<double, 2> point{};
  std::array<double, 3> point3d{};
};
L2PointDefaultProbe g_l2_point_default_probe;

int32_t __cdecl l2_point_default_probe_entry(
    int32_t selector, void*, void*, void** params, void*, void*) {
  if (selector != kFrameSetup) return 0;
  if (!params || !params[1] || !params[2]) return 4;
  int32_t point_x = 0;
  int32_t point_y = 0;
  std::memcpy(&point_x, static_cast<const std::byte*>(params[1]) + 56,
              sizeof(point_x));
  std::memcpy(&point_y, static_cast<const std::byte*>(params[1]) + 60,
              sizeof(point_y));
  g_l2_point_default_probe.point = {
      point_x / 65536.0, point_y / 65536.0};
  for (int component = 0; component < 3; ++component)
    std::memcpy(&g_l2_point_default_probe.point3d[component],
                static_cast<const std::byte*>(params[2]) + 56 + component * 8,
                sizeof(double));
  g_l2_point_default_probe.frame_setup_seen = true;
  return 0;
}

bool verify_l2_point_default_units() {
  auto& parameter_state =
      aexcompat::worker_runtime::parameters::state();
  const auto saved_records = parameter_state.records;
  const auto saved_timelines = parameter_state.timelines;
  const bool saved_user_changed_requested = g_user_changed_param_requested;
  const auto saved_user_changed_parameters = g_user_changed_parameters;
  parameter_state.records.clear();
  parameter_state.timelines.clear();
  g_user_changed_param_requested = false;
  g_user_changed_parameters.clear();

  aexcompat::worker_runtime::parameters::ParamRecord point{};
  point.index = 1;
  point.type = 6;
  point.component_count = 2;
  point.default_components = {25.0, 50.0, 0.0};
  parameter_state.records.push_back(point);
  aexcompat::worker_runtime::parameters::ParamRecord point3d{};
  point3d.index = 2;
  point3d.type = 18;
  point3d.component_count = 3;
  point3d.default_components = {25.0, 50.0, 75.0};
  parameter_state.records.push_back(point3d);

  std::array<std::byte, kInSize> input{};
  std::array<std::byte, kOutSize> output{};
  std::array<int32_t, 5> lifecycle_errors{-1, -1, -1, -1, -1};
  bool lifecycle_data_null = false;
  bool user_changed_ok = false;
  bool conditional_ui_ok = false;
  g_l2_point_default_probe = {};
  const auto lifecycle_exit = run_l2_parameter_lifecycle(
      &l2_point_default_probe_entry, input, output, 0, 0,
      lifecycle_errors, lifecycle_data_null, user_changed_ok,
      conditional_ui_ok, 256, 144);
  const bool passed = !lifecycle_exit &&
      g_l2_point_default_probe.frame_setup_seen &&
      g_l2_point_default_probe.point == std::array<double, 2>{64.0, 72.0} &&
      g_l2_point_default_probe.point3d ==
          std::array<double, 3>{64.0, 72.0, 108.0} &&
      std::all_of(lifecycle_errors.begin(), lifecycle_errors.end(),
                  [](int32_t error) { return error == 0; }) &&
      lifecycle_data_null && user_changed_ok && conditional_ui_ok;

  parameter_state.records = saved_records;
  parameter_state.timelines = saved_timelines;
  g_user_changed_param_requested = saved_user_changed_requested;
  g_user_changed_parameters = saved_user_changed_parameters;
  return passed;
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
  // Trace the request as the plug-in made it (issue #1253: AudWave's
  // SMART_RENDER answered PF_Err_OUT_OF_MEMORY with the audio counters at
  // zero because the checkout was refused before any counter it increments,
  // and nothing showed the arguments it was refused on).
  const void* audio_in = audio ? *audio : nullptr;
  const int32_t result = timeline_result(
      "inter.checkout_layer_audio",
      checkout_layer_audio(
          effect_ref, index, start_time, duration, time_scale, rate,
          bytes_per_sample, channels, format, audio));
  if (extended_diag_enabled())
    std::cerr << "extended_diag:checkout_layer_audio index=" << index
              << " start=" << start_time << " duration=" << duration
              << " scale=" << time_scale << " rate=" << rate
              << " bytes=" << bytes_per_sample << " channels=" << channels
              << " format=" << format << " audio_out=" << static_cast<const void*>(audio)
              << " audio_in=" << audio_in << " -> " << result << "\n" << std::flush;
  return result;
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
                                    aexcompat::callback_diagnostics::Reason::CallbackError,
                                const std::string& detail = {}) {
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
    if (!detail.empty()) std::cerr << ' ' << detail;
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
// PF_UtilCallbacks.composite_rect (0x28). Same implementation the World
// Transform Suite hands out as its slot 0, so the two entry points agree on
// arguments, bounds and transfer modes. Write_on's RENDER calls it with the
// input layer as source and the output as destination (issue #1252).
int32_t __cdecl diag_composite_rect(void* effect_ref, ::LegacyRect* source_rect,
                                   int32_t source_opacity, void* source_world,
                                   int32_t destination_x, int32_t destination_y,
                                   int32_t field, int32_t transfer_mode,
                                   void* destination_world) {
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::CompositeRect,
      composite_rect8(effect_ref, source_rect, source_opacity, source_world, destination_x,
                      destination_y, field, transfer_mode, destination_world),
      !effect_ref || !source_rect || !source_world || !destination_world
          ? aexcompat::callback_diagnostics::Reason::InvalidArguments
          : aexcompat::callback_diagnostics::Reason::CallbackError);
}
// PF_UtilCallbacks.gaussian_kernel (0x50). AE's PF_GaussianKernel values
// (worker_pf_gaussian_kernel.cpp); Inner/Outer Key's RENDER builds its 1D
// NORMALIZED edge-blur kernel through it (issue #1253).
int32_t __cdecl diag_gaussian_kernel(void* effect_ref, double radius, uint32_t flags,
                                     double multiplier, int32_t* diameter, void* kernel) {
  // Every non-zero answer of this callback is an argument refusal (null
  // effect_ref / diameter / kernel, negative or unbounded radius) made before
  // anything is written; it has no other failure mode.
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::GaussianKernel,
      aexcompat::pf_gaussian_kernel::gaussian_kernel(effect_ref, radius, flags, multiplier,
                                                    diameter, kernel),
      aexcompat::callback_diagnostics::Reason::InvalidArguments);
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
  std::string request_text;
  if (result != 0 || extended_diag_enabled()) {
    std::ostringstream request;
    request << "id=" << id << " quality=" << quality << " mode=" << mode_flags;
    request_text = request.str();
  }
  if (result != 0 && callback) {
    // Which callback id was refused, always on like
    // `stage:suite_acquire_failed`: on a frame that fails with 516 this is the
    // whole diagnosis, and without it the requested id is recoverable only by
    // disassembling the plug-in (issue #985, Bulge asking for id -5). A null
    // out pointer is excluded: that refusal is the plug-in's own argument bug,
    // and recording its id would file a capability gap against a callback the
    // host may well implement.
    std::cerr << "stage:callback_addr_denied " << request_text << "\n" << std::flush;
  }
  return utility_callback_result(aexcompat::callback_diagnostics::Callback::GetCallbackAddr,
      result,
      !callback ? aexcompat::callback_diagnostics::Reason::InvalidArguments
                : aexcompat::callback_diagnostics::Reason::Unsupported,
      request_text);
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
  // composite_rect (0x28) had no contract offset until issue #1252, so the
  // table handed plug-ins a null pointer there: Write_on's RENDER (paint style
  // "On Original Image") composites the input behind its strokes through this
  // slot and jumped to address 0.
  sources.composite_rect = reinterpret_cast<void*>(&diag_composite_rect);
  // gaussian_kernel (0x50) had no contract offset until issue #1253, so the
  // table handed plug-ins a null pointer there: Inner/Outer Key's RENDER
  // builds its edge-blur kernel through this slot and jumped to address 0.
  sources.gaussian_kernel = reinterpret_cast<void*>(&diag_gaussian_kernel);
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
  // The rest of the ANSI block (issue #981). These eight had implementations in
  // worker_pf_ansi_runtime.cpp and no contract offset to hang them on, so the
  // table handed plug-ins a null pointer: Basic_3D's FRAME_SETUP calls fmod and
  // Bulge's and Spherize's call floor, and each jumped to address 0.
  sources.ansi_atan = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_atan);
  sources.ansi_atan2 = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_atan2);
  sources.ansi_exp = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_exp);
  sources.ansi_floor = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_floor);
  sources.ansi_fmod = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_fmod);
  sources.ansi_log = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_log);
  sources.ansi_log10 = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_log10);
  sources.ansi_tan = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_tan);
  sources.get_platform_data = reinterpret_cast<void*>(&diag_get_platform_data);
  sources.get_pixel_data8 = reinterpret_cast<void*>(&diag_get_pixel_data8);
  sources.get_pixel_data16 = reinterpret_cast<void*>(&diag_get_pixel_data16);
  sources.host_new_handle = reinterpret_cast<void*>(&diag_new_handle);
  sources.host_lock_handle = reinterpret_cast<void*>(&diag_lock_handle);
  sources.host_unlock_handle = reinterpret_cast<void*>(&diag_unlock_handle);
  sources.host_dispose_handle = reinterpret_cast<void*>(&diag_dispose_handle);
  sources.host_get_handle_size = reinterpret_cast<void*>(&diag_handle_size);
  sources.iterate_origin_non_clip_src =
      reinterpret_cast<void*>(&iterate_origin_non_clip8);
  sources.iterate_generic = reinterpret_cast<void*>(&iterate_generic);
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

// Asks `unwired_installed_offsets` (see its declaration for what the answer
// covers) of the production wiring, rather than of an AbiHooks a test filled in
// itself. This route is the one caller that can: the assignment list lives in
// the worker's own translation unit, so nothing outside it can reach the
// pointers that ship.
//
// `make_bootstrap_abi_hooks` resets the callback diagnostics on entry, which is
// why this stays a self-test route - it runs before any plug-in is loaded and
// nothing else has recorded a call yet.
namespace aexcompat::l2_detail {
bool verify_production_utility_callback_table() {
  namespace boot = aexcompat::worker_runtime::effect_bootstrap;
  namespace contract = aexcompat::abi::x86_64_windows;
  boot::State state{};
  boot::install_callback_tables(state, ::make_bootstrap_abi_hooks());
  void* utils{};
  std::memcpy(&utils, state.input.data() + contract::IN_UTILS_OFFSET, sizeof(utils));
  if (utils != state.utils.data()) {
    std::cerr << "utility_callback_table: in_data->utils does not point at the "
                 "installed block\n" << std::flush;
    return false;
  }
  const auto unwired = boot::unwired_installed_offsets(state);
  // Name each hole, block and all. The verdict alone would leave an operator
  // bisecting the assignment list by hand, which is the work this route exists
  // to skip, and a bare offset would send them to the wrong list.
  for (const auto& slot : unwired)
    std::cerr << "utility_callback_table: unwired block=" << slot.block
              << " offset=" << slot.offset << "\n" << std::flush;
  return unwired.empty();
}

// PF_UtilCallbacks.composite_rect reached the way a plug-in reaches it: read
// the pointer back out of the installed `in_data->utils` block at the generated
// offset and call through it (issue #1252, Write_on's RENDER). The table check
// above proves the slot is non-null; this proves the pointer behind it is the
// composite that PF_Xfer_BEHIND / COPY / IN_FRONT describe and that malformed
// arguments come back as PF_Err_BAD_CALLBACK_PARAM with the destination
// untouched, rather than as a write through an unchecked world.
bool verify_production_composite_rect_callback() {
  namespace boot = aexcompat::worker_runtime::effect_bootstrap;
  namespace contract = aexcompat::abi::x86_64_windows;
  using CompositeRect = int32_t(__cdecl*)(void*, ::LegacyRect*, int32_t, void*, int32_t,
                                          int32_t, int32_t, int32_t, void*);
  boot::State state{};
  boot::install_callback_tables(state, ::make_bootstrap_abi_hooks());
  void* utils{};
  std::memcpy(&utils, state.input.data() + contract::IN_UTILS_OFFSET, sizeof(utils));
  CompositeRect composite{};
  std::memcpy(&composite, static_cast<const std::byte*>(utils) + contract::UTILS_COMPOSITE_RECT_OFFSET,
              sizeof(composite));
  if (!composite) {
    std::cerr << "utils_composite_rect: slot at offset " << contract::UTILS_COMPOSITE_RECT_OFFSET
              << " is null\n" << std::flush;
    return false;
  }
  void* effect_ref{};
  std::memcpy(&effect_ref, state.input.data() + contract::IN_EFFECT_REF_OFFSET, sizeof(effect_ref));

  // Two 2x1 ARGB8 worlds registered the way the classic render path registers
  // the input and output worlds. Layout offsets are the LAYER_* contract ones.
  DispatchWorldFormatScope worlds;
  auto make_world = [&](std::array<std::byte, aexcompat::world_safety::kEffectWorldSize>& world, unsigned char* pixels) {
    world.fill(std::byte{});
    const int32_t rowbytes = 8, width = 2, height = 1;
    std::memcpy(world.data() + contract::LAYER_DATA_OFFSET, &pixels, sizeof(pixels));
    std::memcpy(world.data() + contract::LAYER_ROWBYTES_OFFSET, &rowbytes, sizeof(rowbytes));
    std::memcpy(world.data() + contract::LAYER_WIDTH_OFFSET, &width, sizeof(width));
    std::memcpy(world.data() + contract::LAYER_HEIGHT_OFFSET, &height, sizeof(height));
    return worlds.register_world(world.data(), kPixelFormatArgb32);
  };
  // Source: two opaque pixels. Destination: pixel 0 transparent, pixel 1 opaque.
  std::array<unsigned char, 8> source{255, 200, 100, 50, 255, 10, 20, 30};
  std::array<unsigned char, 8> destination{0, 0, 0, 0, 255, 90, 80, 70};
  std::array<std::byte, aexcompat::world_safety::kEffectWorldSize> source_world{}, destination_world{};
  if (!make_world(source_world, source.data()) ||
      !make_world(destination_world, destination.data())) {
    std::cerr << "utils_composite_rect: world registration failed\n" << std::flush;
    return false;
  }
  ::LegacyRect whole{0, 0, 2, 1};
  bool passed = true;
  auto check = [&](bool condition, const char* what) {
    if (condition) return;
    passed = false;
    std::cerr << "utils_composite_rect: " << what << "\n" << std::flush;
  };
  // BEHIND (Write_on's mode): a transparent destination pixel takes the source,
  // an opaque one keeps its own value.
  check(composite(effect_ref, &whole, 255, &source_world, 0, 0, 0, 1, &destination_world) == 0,
        "PF_Xfer_BEHIND call failed");
  check((destination == std::array<unsigned char, 8>{255, 200, 100, 50, 255, 90, 80, 70}),
        "PF_Xfer_BEHIND result");
  // COPY at full opacity replaces the destination outright.
  destination = {0, 0, 0, 0, 255, 90, 80, 70};
  check(composite(effect_ref, &whole, 255, &source_world, 0, 0, 0, 0, &destination_world) == 0,
        "PF_Xfer_COPY call failed");
  check(destination == source, "PF_Xfer_COPY result");
  // IN_FRONT with an opaque source covers the destination.
  destination = {0, 0, 0, 0, 255, 90, 80, 70};
  check(composite(effect_ref, &whole, 255, &source_world, 0, 0, 0, 2, &destination_world) == 0,
        "PF_Xfer_IN_FRONT call failed");
  check(destination == source, "PF_Xfer_IN_FRONT result");
  // Fail-closed: each malformed call returns PF_Err_BAD_CALLBACK_PARAM (516) and
  // leaves the destination as it was.
  const std::array<unsigned char, 8> before = destination;
  check(composite(effect_ref, &whole, 255, &source_world, 0, 0, 0, 3, &destination_world) == kPfBadCallbackParam,
        "transfer mode 3 accepted");
  check(composite(effect_ref, &whole, 256, &source_world, 0, 0, 0, 1, &destination_world) == kPfBadCallbackParam,
        "opacity 256 accepted");
  check(composite(effect_ref, nullptr, 255, &source_world, 0, 0, 0, 1, &destination_world) == kPfBadCallbackParam,
        "null source rect accepted");
  check(composite(nullptr, &whole, 255, &source_world, 0, 0, 0, 1, &destination_world) == kPfBadCallbackParam,
        "null effect_ref accepted");
  check(composite(effect_ref, &whole, 255, nullptr, 0, 0, 0, 1, &destination_world) == kPfBadCallbackParam,
        "null source world accepted");
  check(composite(effect_ref, &whole, 255, &source_world, 0, 0, 3, 1, &destination_world) == kPfBadCallbackParam,
        "field 3 accepted");
  std::array<std::byte, aexcompat::world_safety::kEffectWorldSize> foreign_world{};
  unsigned char foreign_pixels[8]{};
  unsigned char* foreign_data = foreign_pixels;
  const int32_t foreign_rowbytes = 8, foreign_width = 2, foreign_height = 1;
  std::memcpy(foreign_world.data() + contract::LAYER_DATA_OFFSET, &foreign_data, sizeof(foreign_data));
  std::memcpy(foreign_world.data() + contract::LAYER_ROWBYTES_OFFSET, &foreign_rowbytes, sizeof(foreign_rowbytes));
  std::memcpy(foreign_world.data() + contract::LAYER_WIDTH_OFFSET, &foreign_width, sizeof(foreign_width));
  std::memcpy(foreign_world.data() + contract::LAYER_HEIGHT_OFFSET, &foreign_height, sizeof(foreign_height));
  check(composite(effect_ref, &whole, 255, &foreign_world, 0, 0, 0, 1, &destination_world) == kPfBadCallbackParam,
        "unregistered source world accepted");
  check(destination == before, "a rejected call wrote the destination");
  return passed;
}

// PF_UtilCallbacks.gaussian_kernel reached the way a plug-in reaches it: the
// pointer read back out of the installed `in_data->utils` block at the
// generated offset and called through it (issue #1253, Inner/Outer Key's
// RENDER). The table check proves the slot non-null; this proves the pointer
// behind it produces AE's PF_GaussianKernel values (radius 1, 1D, NORMALIZED
// -> 192, 380, 192, diameter 3; radius 1, 2D, unnormalized -> the ceil-radius
// bell rounded to 0..255 with the centre at 255) and that malformed calls come
// back as errors with the buffer untouched.
bool verify_production_gaussian_kernel_callback() {
  namespace boot = aexcompat::worker_runtime::effect_bootstrap;
  namespace contract = aexcompat::abi::x86_64_windows;
  using GaussianKernel = int32_t(__cdecl*)(void*, double, uint32_t, double, int32_t*, void*);
  boot::State state{};
  boot::install_callback_tables(state, ::make_bootstrap_abi_hooks());
  void* utils{};
  std::memcpy(&utils, state.input.data() + contract::IN_UTILS_OFFSET, sizeof(utils));
  GaussianKernel gaussian{};
  std::memcpy(&gaussian, static_cast<const std::byte*>(utils) + contract::UTILS_GAUSSIAN_KERNEL_OFFSET,
              sizeof(gaussian));
  if (!gaussian) {
    std::cerr << "utils_gaussian_kernel: slot at offset " << contract::UTILS_GAUSSIAN_KERNEL_OFFSET
              << " is null\n" << std::flush;
    return false;
  }
  void* effect_ref{};
  std::memcpy(&effect_ref, state.input.data() + contract::IN_EFFECT_REF_OFFSET, sizeof(effect_ref));
  bool passed = true;
  auto check = [&](bool condition, const char* what) {
    if (condition) return;
    passed = false;
    std::cerr << "utils_gaussian_kernel: " << what << "\n" << std::flush;
  };
  // Inner/Outer Key's call shape: 1D, NORMALIZED, longs; radius 1 -> 3 entries.
  std::array<int32_t, 9> kernel{};
  int32_t diameter = -1;
  check(gaussian(effect_ref, 1.0, 0x3, 1.0, &diameter, kernel.data()) == 0,
        "1D normalized call failed");
  check(diameter == 3, "1D normalized diameter");
  check((kernel[0] == 192 && kernel[1] == 380 && kernel[2] == 192),
        "1D normalized values (AE PF_GaussianKernel: 192, 380, 192)");
  // 2D unnormalized radius 1: the bell over hypot(x, y) / 2, times 255,
  // rounded: corners d = 0.7071 -> exp(-1.189) = 0.3045 -> 1 - 0.6955 * 1.102
  // = 0.2336 -> 60; edges d = 0.5 -> 129; centre 255.
  kernel.fill(-1);
  diameter = -1;
  check(gaussian(effect_ref, 1.0, 0x0, 1.0, &diameter, kernel.data()) == 0,
        "2D unnormalized call failed");
  check(diameter == 3, "2D diameter");
  check((kernel == std::array<int32_t, 9>{60, 129, 60, 129, 255, 129, 60, 129, 60}),
        "2D unnormalized values");
  // The multiplier scales before rounding and clamping (2.0 clamps the centre
  // at 255 and doubles the rest); a fractional radius ceils.
  kernel.fill(-1);
  check(gaussian(effect_ref, 0.5, 0x1, 2.0, &diameter, kernel.data()) == 0 && diameter == 3 &&
            kernel[1] == 255 && kernel[0] == kernel[2] && kernel[0] > 0 && kernel[0] < 255,
        "multiplier / fractional radius");
  // Fail-closed: AE's own refusals (negative radius, null outputs) and the
  // host's (null effect_ref, radius past the bound) leave the buffer untouched.
  kernel.fill(-1);
  diameter = -1;
  check(gaussian(effect_ref, -1.0, 0x1, 1.0, &diameter, kernel.data()) != 0, "negative radius accepted");
  check(gaussian(effect_ref, 1.0, 0x1, 1.0, nullptr, kernel.data()) != 0, "null diameter accepted");
  check(gaussian(effect_ref, 1.0, 0x1, 1.0, &diameter, nullptr) != 0, "null kernel accepted");
  check(gaussian(nullptr, 1.0, 0x1, 1.0, &diameter, kernel.data()) == kPfBadCallbackParam,
        "null effect_ref accepted");
  check(gaussian(effect_ref, 1e9, 0x1, 1.0, &diameter, kernel.data()) == kPfBadCallbackParam,
        "unbounded radius accepted");
  check(diameter == -1 && kernel == std::array<int32_t, 9>{-1, -1, -1, -1, -1, -1, -1, -1, -1},
        "a rejected call wrote its outputs");
  return passed;
}

// PF_InteractCallbacks.checkout_param / checkin_param reached through the
// installed in_data block, asked for slots past the published table (issue
// #1251, Pixel Motion Blur's checkout of Timewarp's slots 29/31 against a
// 5-slot table). AE 2026 answers those with PF_Err_NONE and an empty layer
// definition and takes the checkin (instruments/pf-checkout-index-probe,
// docs/CHECKOUT_PARAM_INDEX_OBSERVATION_2026-08-17.md); this proves the host
// does the same on both the classic-context and the hosted-table path, that
// the checkout is recorded (the checkin balances instead of counting as
// invalid), that an in-table slot still comes back with its definition, and
// that a negative slot and an unpublished table stay refused.
bool verify_checkout_param_beyond_table() {
  namespace boot = aexcompat::worker_runtime::effect_bootstrap;
  namespace contract = aexcompat::abi::x86_64_windows;
  namespace classic = aexcompat::worker_runtime::classic;
  namespace parameters = aexcompat::worker_runtime::parameters;
  using Checkout = int32_t(__cdecl*)(void*, int32_t, int32_t, int32_t, uint32_t, void*);
  using Checkin = int32_t(__cdecl*)(void*, void*);
  boot::State state{};
  boot::install_callback_tables(state, ::make_bootstrap_abi_hooks());
  Checkout checkout{};
  Checkin checkin{};
  std::memcpy(&checkout, state.input.data() + contract::IN_INTER_OFFSET +
                             contract::INTER_CHECKOUT_PARAM_OFFSET, sizeof(checkout));
  std::memcpy(&checkin, state.input.data() + contract::IN_INTER_OFFSET +
                            contract::INTER_CHECKIN_PARAM_OFFSET, sizeof(checkin));
  if (!checkout || !checkin) {
    std::cerr << "checkout_param_beyond_table: inter slot is null\n" << std::flush;
    return false;
  }
  void* effect_ref{};
  std::memcpy(&effect_ref, state.input.data() + contract::IN_EFFECT_REF_OFFSET, sizeof(effect_ref));
  bool passed = true;
  auto check = [&](bool condition, const char* what) {
    if (condition) return;
    passed = false;
    std::cerr << "checkout_param_beyond_table: " << what << "\n" << std::flush;
  };
  const classic::ParameterDefinition zero{};
  auto marked = [](std::byte marker) {
    classic::ParameterDefinition definition{};
    definition.fill(marker);
    return definition;
  };
  constexpr int32_t kTime = 0, kStep = 1;
  constexpr uint32_t kScale = 30;
  // Classic-context path: the table the classic render publishes (slots 0..4,
  // Pixel Motion Blur's shape) with each slot carrying its own marker.
  {
    classic::Context context;
    for (int32_t slot = 0; slot <= 4; ++slot)
      context.set_definition(slot, marked(std::byte{static_cast<unsigned char>(0x10 + slot)}));
    for (const int32_t slot : {29, 31, 5}) {
      classic::ParameterDefinition definition = marked(std::byte{0xAB});
      check(checkout(effect_ref, slot, kTime, kStep, kScale, definition.data()) == 0,
            "classic: slot past the table refused");
      check(definition == zero, "classic: slot past the table did not come back empty");
      check(checkin(effect_ref, definition.data()) == 0,
            "classic: checkin of a past-the-table checkout refused");
    }
    classic::ParameterDefinition in_table{};
    check(checkout(effect_ref, 2, kTime, kStep, kScale, in_table.data()) == 0 &&
              in_table == marked(std::byte{0x12}),
          "classic: in-table slot lost its definition");
    check(checkin(effect_ref, in_table.data()) == 0, "classic: in-table checkin refused");
    classic::ParameterDefinition negative = marked(std::byte{0xAB});
    check(checkout(effect_ref, -1, kTime, kStep, kScale, negative.data()) != 0,
          "classic: negative slot accepted");
    check(negative == marked(std::byte{0xAB}), "classic: refused checkout wrote the def");
    check(context.checkouts_balanced(), "classic: checkouts did not balance");
  }
  {
    // No table published yet: nothing is "beyond" it, so the slot is refused.
    classic::Context unpublished;
    classic::ParameterDefinition definition = marked(std::byte{0xAB});
    check(checkout(effect_ref, 29, kTime, kStep, kScale, definition.data()) != 0,
          "classic: slot accepted before the table was published");
    check(definition == marked(std::byte{0xAB}), "classic: refused checkout wrote the def");
  }
  // Hosted-table path (no classic context; the lifecycle / smart routes).
  {
    auto& hosted = parameters::state().checkout;
    struct HostedScope {
      parameters::CheckoutState& hosted;
      ~HostedScope() {
        hosted.definitions.clear();
        std::lock_guard<std::mutex> lock(hosted.mutex);
        hosted.live.clear();
      }
    } hosted_scope{hosted};
    hosted.definitions.clear();
    {
      std::lock_guard<std::mutex> lock(hosted.mutex);
      hosted.live.clear();
    }
    classic::ParameterDefinition definition = marked(std::byte{0xAB});
    check(checkout(effect_ref, 29, kTime, kStep, kScale, definition.data()) != 0,
          "hosted: slot accepted with no table published");
    for (int32_t slot = 0; slot <= 4; ++slot)
      hosted.definitions[slot] = marked(std::byte{static_cast<unsigned char>(0x20 + slot)});
    for (const int32_t slot : {29, 31, 5}) {
      definition = marked(std::byte{0xAB});
      check(checkout(effect_ref, slot, kTime, kStep, kScale, definition.data()) == 0,
            "hosted: slot past the table refused");
      check(definition == zero, "hosted: slot past the table did not come back empty");
      check(checkin(effect_ref, definition.data()) == 0,
            "hosted: checkin of a past-the-table checkout refused");
    }
    definition = marked(std::byte{0xAB});
    check(checkout(effect_ref, -1, kTime, kStep, kScale, definition.data()) != 0,
          "hosted: negative slot accepted");
    std::lock_guard<std::mutex> lock(hosted.mutex);
    check(hosted.live.empty(), "hosted: checkouts did not balance");
  }
  return passed;
}

// AE's private get_callback_addr ids -5 and -2, obtained the way a plug-in
// obtains them: through the get_callback_addr slot read back out of the
// installed in_data->utils block (issue #985; Bulge asks for -5, Compound
// Blur / CC Cross Blur / Matte Choker for -2). Expected values are AE 2026's
// own answers, captured by hooking the live dispatcher with Frida
// (docs/PRIVATE_CALLBACK_IDS_OBSERVATION_2026-08-17.md): PFp_GaussianValue's
// curve for -5, and for -2 the 8-bit impulse responses and straight/premul
// alpha handling of the FLT.dll blur on synthetic worlds handed to AE's own
// function. Malformed calls fail closed with the world untouched.
bool verify_pf_private_callbacks() {
  namespace boot = aexcompat::worker_runtime::effect_bootstrap;
  namespace contract = aexcompat::abi::x86_64_windows;
  using GetCallbackAddr = int32_t(__cdecl*)(void*, int32_t, uint32_t, int32_t, void**);
  using Easing = double(__cdecl*)(double);
  using PrivateBlur = int32_t(__cdecl*)(void*, void*, double, void*, int32_t, void*);
  boot::State state{};
  boot::install_callback_tables(state, ::make_bootstrap_abi_hooks());
  void* utils{};
  std::memcpy(&utils, state.input.data() + contract::IN_UTILS_OFFSET, sizeof(utils));
  GetCallbackAddr get_callback_addr{};
  std::memcpy(&get_callback_addr,
              static_cast<const std::byte*>(utils) + contract::UTILS_GET_CALLBACK_ADDR_OFFSET,
              sizeof(get_callback_addr));
  if (!get_callback_addr) {
    std::cerr << "pf_private_callbacks: get_callback_addr slot is null\n" << std::flush;
    return false;
  }
  void* effect_ref{};
  std::memcpy(&effect_ref, state.input.data() + contract::IN_EFFECT_REF_OFFSET, sizeof(effect_ref));
  bool passed = true;
  auto check = [&](bool condition, const char* what) {
    if (condition) return;
    passed = false;
    std::cerr << "pf_private_callbacks: " << what << "\n" << std::flush;
  };
  auto set_quality = [&](int32_t quality) {
    std::memcpy(state.input.data() + contract::IN_QUALITY_OFFSET, &quality, sizeof(quality));
  };

  // id -5: the same function for every (quality, mode); PFp_GaussianValue's
  // values as AE returned them (double, tolerance for the CRT's exp).
  void* raw_easing{};
  check(get_callback_addr(effect_ref, 1, 0, -5, &raw_easing) == 0 && raw_easing,
        "id -5 refused");
  for (int32_t quality = 0; quality <= 1; ++quality)
    for (uint32_t mode = 0; mode <= 2; ++mode) {
      void* other{};
      check(get_callback_addr(effect_ref, quality, mode, -5, &other) == 0 && other == raw_easing,
            "id -5 depends on quality/mode");
    }
  if (raw_easing) {
    const auto easing = reinterpret_cast<Easing>(raw_easing);
    auto close_to = [](double a, double b) { return std::fabs(a - b) <= 1e-12; };
    check(easing(0.0) == 1.0, "easing(0) != 1");
    check(close_to(easing(0.5), 0.5061259345505494), "easing(0.5)");
    check(close_to(easing(0.25), 0.847805274821874), "easing(0.25)");
    check(close_to(easing(0.9), 0.05856550493005852), "easing(0.9)");
    check(close_to(easing(1.0), 0.00019492200675952365), "easing(1.0)");
    check(easing(1.1) == 0.0 && easing(2.0) == 0.0, "easing(>1) != 0");
    check(close_to(easing(-0.5), 0.5061259345505494) && close_to(easing(-1.0), 0.00019492200675952365),
          "easing(negative)");
  }

  // id -2: mode 1 selects the straight-alpha entry, any other mode the
  // premultiplied one; quality does not matter at request time.
  void* raw_straight{};
  void* raw_premul{};
  void* raw_mode2{};
  void* raw_q0{};
  check(get_callback_addr(effect_ref, 1, 1, -2, &raw_straight) == 0 && raw_straight, "id -2 mode 1 refused");
  check(get_callback_addr(effect_ref, 1, 0, -2, &raw_premul) == 0 && raw_premul, "id -2 mode 0 refused");
  check(get_callback_addr(effect_ref, 1, 2, -2, &raw_mode2) == 0 && raw_mode2 == raw_premul, "id -2 mode 2");
  check(get_callback_addr(effect_ref, 0, 1, -2, &raw_q0) == 0 && raw_q0 == raw_straight, "id -2 quality 0");
  check(raw_straight != raw_premul, "id -2 mode 1 and mode 0 answer the same function");
  void* raw_unknown = reinterpret_cast<void*>(1);
  check(get_callback_addr(effect_ref, 1, 0, -3, &raw_unknown) == kPfBadCallbackParam && raw_unknown == nullptr,
        "id -3 answered");
  if (!raw_straight || !raw_premul) return false;
  const auto blur_straight = reinterpret_cast<PrivateBlur>(raw_straight);
  const auto blur_premul = reinterpret_cast<PrivateBlur>(raw_premul);

  // A 33x1 ARGB8 world registered like the classic render path's worlds.
  constexpr int32_t width = 33, height = 1, rowbytes = width * 4;
  DispatchWorldFormatScope worlds;
  std::array<unsigned char, static_cast<std::size_t>(rowbytes) * height> pixels{};
  std::array<std::byte, aexcompat::world_safety::kEffectWorldSize> world{};
  {
    unsigned char* data = pixels.data();
    std::memcpy(world.data() + contract::LAYER_DATA_OFFSET, &data, sizeof(data));
    std::memcpy(world.data() + contract::LAYER_ROWBYTES_OFFSET, &rowbytes, sizeof(rowbytes));
    std::memcpy(world.data() + contract::LAYER_WIDTH_OFFSET, &width, sizeof(width));
    std::memcpy(world.data() + contract::LAYER_HEIGHT_OFFSET, &height, sizeof(height));
    if (!worlds.register_world(world.data(), kPixelFormatArgb32)) {
      std::cerr << "pf_private_callbacks: world registration failed\n" << std::flush;
      return false;
    }
  }
  auto impulse = [&](unsigned char a, unsigned char r, unsigned char g, unsigned char b) {
    pixels.fill(0);
    pixels[16 * 4 + 0] = a; pixels[16 * 4 + 1] = r; pixels[16 * 4 + 2] = g; pixels[16 * 4 + 3] = b;
  };
  auto channel_row = [&](int channel, int first, int count) {
    std::vector<int> row;
    for (int i = 0; i < count; ++i) row.push_back(pixels[static_cast<std::size_t>(first + i) * 4 + channel]);
    return row;
  };
  const int32_t kHorizontalAll = 0x4f;  // horizontal, all channels, no repeat

  // Gaussian path (rho <= 1): AE's integer kernel, r=1 -> [64,127,64], r=2 -> [23,62,84,62,23].
  set_quality(1);
  impulse(255, 255, 255, 255);
  check(blur_premul(state.input.data(), nullptr, 1.0, nullptr, kHorizontalAll, world.data()) == 0, "premul r=1 refused");
  check(channel_row(0, 14, 5) == std::vector<int>{0, 64, 127, 64, 0}, "premul r=1 alpha kernel");
  check(channel_row(1, 14, 5) == std::vector<int>{0, 64, 127, 64, 0}, "premul r=1 red kernel");
  impulse(255, 255, 255, 255);
  check(blur_premul(state.input.data(), nullptr, 2.0, nullptr, kHorizontalAll, world.data()) == 0, "premul r=2 refused");
  check(channel_row(0, 13, 7) == std::vector<int>{0, 23, 62, 84, 62, 23, 0}, "premul r=2 kernel");
  // Box path (rho > 1): quality 1 = three passes, r=6.3 -> AE's 15-tap response;
  // quality 0 = one pass at 1.4x, r=6.3 -> [9,34x7,9].
  impulse(255, 255, 255, 255);
  check(blur_premul(state.input.data(), nullptr, 6.3, nullptr, kHorizontalAll, world.data()) == 0, "premul r=6.3 refused");
  check(channel_row(0, 9, 15) == std::vector<int>{2, 5, 9, 14, 21, 28, 32, 33, 32, 28, 21, 14, 9, 5, 2},
        "premul q1 r=6.3 kernel");
  set_quality(0);
  impulse(255, 255, 255, 255);
  check(blur_premul(state.input.data(), nullptr, 6.3, nullptr, kHorizontalAll, world.data()) == 0, "premul q0 r=6.3 refused");
  check(channel_row(0, 12, 9) == std::vector<int>{9, 34, 34, 34, 34, 34, 34, 34, 9}, "premul q0 r=6.3 kernel");
  // Straight vs premultiplied on a half-alpha impulse (a=127, r=204, g=102):
  // the straight entry keeps the color where alpha spreads, the premultiplied
  // one spreads the color values themselves.
  set_quality(1);
  impulse(127, 204, 102, 0);
  check(blur_straight(state.input.data(), nullptr, 1.0, nullptr, kHorizontalAll, world.data()) == 0, "straight r=1 refused");
  check(channel_row(0, 15, 3) == std::vector<int>{32, 63, 32} &&
        channel_row(1, 15, 3) == std::vector<int>{205, 205, 205} &&
        channel_row(2, 15, 3) == std::vector<int>{102, 102, 102},
        "straight half-alpha result");
  impulse(127, 204, 102, 0);
  check(blur_premul(state.input.data(), nullptr, 1.0, nullptr, kHorizontalAll, world.data()) == 0, "premul half-alpha refused");
  check(channel_row(0, 15, 3) == std::vector<int>{32, 63, 32} &&
        channel_row(1, 15, 3) == std::vector<int>{51, 101, 51} &&
        channel_row(2, 15, 3) == std::vector<int>{26, 51, 26},
        "premul half-alpha result");
  // Alpha-only channel flag leaves the colors alone (Matte Choker's 0x71).
  impulse(255, 200, 100, 50);
  check(blur_premul(state.input.data(), nullptr, 1.0, nullptr, 0x41, world.data()) == 0, "alpha-only refused");
  check(channel_row(0, 15, 3) == std::vector<int>{64, 127, 64} &&
        channel_row(1, 15, 3) == std::vector<int>{0, 200, 0}, "alpha-only result");
  // Radius 0 is a no-op that succeeds (AE builds no blur node), the pixels
  // untouched.
  impulse(255, 200, 100, 50);
  check(blur_premul(state.input.data(), nullptr, 0.0, nullptr, kHorizontalAll, world.data()) == 0 &&
        channel_row(1, 15, 3) == std::vector<int>{0, 200, 0}, "radius 0");
  // Both axes, straight, no repeat, on AE's 65x65 step world (opaque
  // (255,200,100,50) for x<32 && y<32, transparent elsewhere; r=1, flags
  // 0x6f): X pass premultiplied intermediate, Y pass unpremultiplies at the
  // end, zero beyond the world. Values are the ones AE's function wrote
  // (probe case "step m=1 q=1 r=1 flags=0x6f", 8-bit).
  {
    constexpr int32_t side = 65;
    std::vector<unsigned char> step(static_cast<std::size_t>(side) * side * 4);
    for (int32_t y = 0; y < side; ++y)
      for (int32_t x = 0; x < side; ++x) {
        auto* px = &step[(static_cast<std::size_t>(y) * side + x) * 4];
        const bool on = x < 32 && y < 32;
        px[0] = on ? 255 : 0; px[1] = on ? 200 : 0; px[2] = on ? 100 : 0; px[3] = on ? 50 : 0;
      }
    std::array<std::byte, aexcompat::world_safety::kEffectWorldSize> step_world{};
    unsigned char* data = step.data();
    const int32_t step_rowbytes = side * 4;
    std::memcpy(step_world.data() + contract::LAYER_DATA_OFFSET, &data, sizeof(data));
    std::memcpy(step_world.data() + contract::LAYER_ROWBYTES_OFFSET, &step_rowbytes, sizeof(step_rowbytes));
    std::memcpy(step_world.data() + contract::LAYER_WIDTH_OFFSET, &side, sizeof(side));
    std::memcpy(step_world.data() + contract::LAYER_HEIGHT_OFFSET, &side, sizeof(side));
    check(worlds.register_world(step_world.data(), kPixelFormatArgb32), "step world registration");
    check(blur_straight(state.input.data(), nullptr, 1.0, nullptr, 0x6f, step_world.data()) == 0,
          "straight two-axis step refused");
    auto at = [&](int32_t x, int32_t y, int32_t c) {
      return static_cast<int>(step[(static_cast<std::size_t>(y) * side + x) * 4 + c]);
    };
    // Corner (30..34, 30..34): alpha falls off in both axes, colors keep their
    // straight values, blue shows AE's premul/unpremul rounding (50/49/52).
    const std::vector<int> alpha_rows{255, 191, 64, 0, 0, 191, 143, 48, 0, 0, 64, 48, 16, 0, 0};
    std::vector<int> got_alpha, got_red, got_blue;
    for (int32_t y = 30; y < 33; ++y)
      for (int32_t x = 30; x < 35; ++x) {
        got_alpha.push_back(at(x, y, 0));
        got_red.push_back(at(x, y, 1));
        got_blue.push_back(at(x, y, 3));
      }
    check(got_alpha == alpha_rows, "two-axis straight alpha corner");
    check(got_red == std::vector<int>{200, 200, 199, 0, 0, 200, 200, 199, 0, 0, 200, 200, 199, 0, 0},
          "two-axis straight red corner");
    check(got_blue == std::vector<int>{50, 49, 52, 0, 0, 50, 49, 52, 0, 0, 50, 49, 52, 0, 0},
          "two-axis straight blue corner");
    check(at(33, 33, 0) == 0 && at(33, 33, 1) == 0, "two-axis straight outside the step");
    // World border: zero beyond the world in both axes (row 0 / column 0 of the
    // opaque region reads 143 at the corner, 191 along the edges).
    check(at(0, 0, 0) == 143 && at(1, 0, 0) == 191 && at(4, 0, 0) == 191 && at(0, 1, 0) == 191 &&
              at(0, 4, 0) == 191 && at(0, 0, 1) == 200 && at(4, 0, 1) == 200,
          "two-axis straight world border");
  }
  // 16-bit (ARGB64, 0..32768): the same integer gaussian weights and the same
  // three-pass box, values as AE wrote them into a 16-bit world (probe cases
  // "impulse deep m=0 q=1 r=1 / r=6.3", "halfalpha deep m=1 q=1 r=1").
  {
    constexpr int32_t deep_width = 33, deep_rowbytes = deep_width * 8;
    std::vector<uint16_t> deep(static_cast<std::size_t>(deep_width) * 4);
    std::array<std::byte, aexcompat::world_safety::kEffectWorldSize> deep_world{};
    uint16_t* data = deep.data();
    const int32_t one = 1;
    std::memcpy(deep_world.data() + contract::LAYER_DATA_OFFSET, &data, sizeof(data));
    std::memcpy(deep_world.data() + contract::LAYER_ROWBYTES_OFFSET, &deep_rowbytes, sizeof(deep_rowbytes));
    std::memcpy(deep_world.data() + contract::LAYER_WIDTH_OFFSET, &deep_width, sizeof(deep_width));
    std::memcpy(deep_world.data() + contract::LAYER_HEIGHT_OFFSET, &one, sizeof(one));
    check(worlds.register_world(deep_world.data(), kPixelFormatArgb64), "16-bit world registration");
    auto deep_impulse = [&](uint16_t a, uint16_t r, uint16_t g) {
      std::fill(deep.begin(), deep.end(), uint16_t{0});
      deep[16 * 4 + 0] = a; deep[16 * 4 + 1] = r; deep[16 * 4 + 2] = g;
    };
    auto deep_row = [&](int channel, int first, int count) {
      std::vector<int> row;
      for (int i = 0; i < count; ++i) row.push_back(deep[static_cast<std::size_t>(first + i) * 4 + channel]);
      return row;
    };
    set_quality(1);
    deep_impulse(32768, 32768, 32768);
    check(blur_premul(state.input.data(), nullptr, 1.0, nullptr, kHorizontalAll, deep_world.data()) == 0,
          "16-bit r=1 refused");
    check(deep_row(0, 15, 3) == std::vector<int>{8240, 16288, 8240} &&
              deep_row(1, 15, 3) == std::vector<int>{8240, 16288, 8240},
          "16-bit r=1 kernel");
    deep_impulse(32768, 32768, 32768);
    check(blur_premul(state.input.data(), nullptr, 6.3, nullptr, kHorizontalAll, deep_world.data()) == 0,
          "16-bit r=6.3 refused");
    // Exact: the 16-bit path rounds every pass to a whole step the way AE's
    // integer path does (kept in double, the 3rd pass lands one step off at
    // two taps).
    check(deep_row(0, 7, 19) == std::vector<int>{6, 57, 234, 593, 1134, 1856, 2721, 3549, 4094, 4276,
                                                 4094, 3549, 2721, 1856, 1134, 593, 234, 57, 6},
          "16-bit q1 r=6.3 kernel");
    deep_impulse(16384, 26214, 13107);
    check(blur_straight(state.input.data(), nullptr, 1.0, nullptr, kHorizontalAll, deep_world.data()) == 0,
          "16-bit straight refused");
    // Exact: green is 13106, not 13107, only when the 16-bit premultiply
    // truncates (13107 * 16384 / 32768 = 6553.5 -> 6553), which is what the
    // host does at that depth.
    check(deep_row(0, 15, 3) == std::vector<int>{4120, 8144, 4120} &&
              deep_row(1, 15, 3) == std::vector<int>{26214, 26214, 26214} &&
              deep_row(2, 15, 3) == std::vector<int>{13106, 13106, 13106},
          "16-bit straight half-alpha result");
  }
  // Fail-closed: each malformed call answers PF_Err_BAD_CALLBACK_PARAM (516)
  // and leaves the world untouched.
  impulse(255, 200, 100, 50);
  const auto before = pixels;
  check(blur_premul(nullptr, nullptr, 1.0, nullptr, kHorizontalAll, world.data()) == kPfBadCallbackParam,
        "null in_data accepted");
  check(blur_premul(state.input.data(), nullptr, 1.0, nullptr, kHorizontalAll, nullptr) == kPfBadCallbackParam,
        "null world accepted");
  check(blur_premul(state.input.data(), nullptr, -1.0, nullptr, kHorizontalAll, world.data()) == kPfBadCallbackParam,
        "negative radius accepted");
  check(blur_premul(state.input.data(), nullptr, 1.0, nullptr, kHorizontalAll | 0x200, world.data()) == kPfBadCallbackParam,
        "unknown flag accepted");
  check(blur_premul(state.input.data(), nullptr, 1.0, nullptr, 0x40, world.data()) == kPfBadCallbackParam,
        "no channel accepted");
  set_quality(2);
  check(blur_premul(state.input.data(), nullptr, 1.0, nullptr, kHorizontalAll, world.data()) == kPfBadCallbackParam,
        "quality 2 accepted");
  set_quality(1);
  std::array<std::byte, aexcompat::world_safety::kEffectWorldSize> foreign_world{};
  check(blur_premul(state.input.data(), nullptr, 1.0, nullptr, kHorizontalAll, foreign_world.data()) == kPfBadCallbackParam,
        "empty unregistered world accepted");
  auto other_in_data = state.input;
  void* wrong_ref = reinterpret_cast<void*>(0x1234);
  std::memcpy(other_in_data.data() + contract::IN_EFFECT_REF_OFFSET, &wrong_ref, sizeof(wrong_ref));
  check(blur_premul(other_in_data.data(), nullptr, 1.0, nullptr, kHorizontalAll, world.data()) == kPfBadCallbackParam,
        "foreign effect_ref accepted");
  check(pixels == before, "a rejected call wrote the world");
  return passed;
}

// The BEE.dll-compatible scene object behind the effect layer handle (issue
// #1210). Timecode.aex acquires "AE Timecode Helper Suite" v1 as a
// host-presence gate (never calls a slot), then reads the AEGP_LayerH from
// AEGP PF Interface Suite::AEGP_GetEffectLayer as a `BEE_AVLayer*`: parent
// comp item at +0x260, its project at item+0x38, the item tag/type/flags, and
// the vtable slots BEE.dll's exports call (docs/BEE_SCENE_OBJECT_ABI_2026-08-17.md).
// This proves the production hand-out path publishes that object: the helper
// suite acquires with 32 distinct slots (three of them called and answering
// the diagnosed refusal), the layer the production suite returns carries the
// facade vtable and object graph with the host's comp dimensions, and the
// facade's own contract (observed slots implemented, sampled unobserved slots
// on each object trapping with their code) holds.
bool verify_bee_scene_facade() {
  namespace bee = aexcompat::worker_runtime::bee_facade;
  bool passed = true;
  auto check = [&](bool condition, const char* what) {
    if (condition) return;
    passed = false;
    std::cerr << "bee_scene_facade: " << what << "\n" << std::flush;
  };
  const auto recorded = [](const char* name, int32_t version, uint32_t slot) {
    std::ostringstream needle;
    needle << "{\"name\":\"" << name << "\",\"version\":" << version
           << ",\"slot\":" << slot << ",";
    return aexcompat::worker_runtime::suite_registry()
               .unsupported_suite_calls_report_json()
               .find(needle.str()) != std::string::npos;
  };
  // The facade's own contract first (observed slots answer as recorded, every
  // unobserved slot traps with its code on each object). It also proves the
  // record half of a trap: the slots it takes must show up in the report list
  // the workers emit, under the object's pseudo-suite name. The list is
  // bounded (32 distinct entries per worker life), so this runs before the
  // helper-suite probes below and those sample three slots instead of 32.
  check(bee::selftest(), "facade contract self-test failed");
  check(recorded("BEE_AVLayer vtable", 1, 0) &&
            recorded("BEE_AVLayer vtable", 1,
                     static_cast<uint32_t>(bee::kLayerVtableSlots - 1)) &&
            recorded("BEE_CompItem vtable", 1, 3) &&
            recorded("BEE_FootageItem vtable", 1, 3) &&
            recorded("BEE_Project vtable", 1,
                     static_cast<uint32_t>(bee::kProjectVtableSlots - 1)),
        "trapped vtable slots are not in the unsupported_suite_calls report");
  const void* helper = nullptr;
  check(acquire_suite("AE Timecode Helper Suite", 1, &helper) == 0 && helper,
        "AE Timecode Helper Suite v1 did not acquire");
  if (helper) {
    using Slot = int32_t(__cdecl*)();
    const auto* slots = static_cast<void* const*>(helper);
    for (std::size_t slot = 0; slot < 32; ++slot) {
      check(slots[slot] != nullptr, "helper suite slot is null");
      for (std::size_t earlier = 0; earlier < slot; ++earlier)
        check(slots[earlier] != slots[slot], "helper suite slots are not distinct");
    }
    for (const std::size_t slot : {std::size_t{0}, std::size_t{15}, std::size_t{31}}) {
      if (!slots[slot]) continue;
      check(reinterpret_cast<Slot>(slots[slot])() == 4,
            "helper suite slot did not answer the diagnosed refusal");
      check(recorded("AE Timecode Helper Suite", 1, static_cast<uint32_t>(slot)),
            "helper suite slot call is not in the unsupported_suite_calls report");
    }
    check(release_suite("AE Timecode Helper Suite", 1) == 0,
          "AE Timecode Helper Suite v1 did not release");
  }
  // A render context for the dimension checks below (this route has none of
  // its own); the same pattern the scene self-tests use, restored on exit.
  constexpr int32_t kFacadeContextWidth = 640, kFacadeContextHeight = 360;
  const int32_t saved_width = g_full_resolution_width;
  const int32_t saved_height = g_full_resolution_height;
  g_full_resolution_width = kFacadeContextWidth;
  g_full_resolution_height = kFacadeContextHeight;
  struct RestoreContext {
    int32_t width, height;
    ~RestoreContext() {
      g_full_resolution_width = width;
      g_full_resolution_height = height;
    }
  } restore_context{saved_width, saved_height};
  const void* interface_suite = nullptr;
  check(acquire_suite("AEGP PF Interface Suite", 1, &interface_suite) == 0 &&
            interface_suite,
        "AEGP PF Interface Suite v1 did not acquire");
  if (interface_suite) {
    using GetEffectLayer = int32_t(__cdecl*)(void*, void**);
    const auto get_layer =
        reinterpret_cast<GetEffectLayer>(static_cast<void* const*>(interface_suite)[0]);
    void* layer = nullptr;
    check(get_layer(&g_effect, &layer) == 0 && layer == &g_layer,
          "AEGP_GetEffectLayer did not hand out the effect layer");
    void* foreign = nullptr;
    check(get_layer(&g_layer, &foreign) != 0 && foreign == nullptr,
          "AEGP_GetEffectLayer accepted a non-effect handle");
    if (layer == &g_layer) {
      const auto* object = static_cast<const bee::LayerObject*>(layer);
      check(object->vtable == bee::layer_vtable(), "layer handle lacks the facade vtable");
      check(object->parent_comp_item == &bee::comp_item(), "layer+0x260 is not the comp item");
      check(object->source_item == &bee::footage_item(), "layer+0x2720 is not the source item");
      const bee::ItemObject& comp = bee::comp_item();
      check(comp.tag == bee::kItemTag && comp.type == bee::kItemTypeComposition &&
                comp.parent_project == &bee::project(),
            "comp item header is not the observed BEE_Item shape");
      // The comp dimensions are the ones the AEGP item suite reports for the
      // configured render context (the item suite refuses without one, so
      // one is configured for this check and restored afterwards).
      int32_t width = 0, height = 0;
      check(aegp_get_item_dimensions(aegp_comp_item_handle(), &width, &height) == 0 &&
                width == kFacadeContextWidth && height == kFacadeContextHeight,
            "AEGP item dimensions do not report the configured render context");
      check(comp.width == kFacadeContextWidth && comp.height == kFacadeContextHeight &&
                comp.width == width && comp.height == height,
            "comp item dimensions differ from the AEGP item suite");
      double fps = 0.0;
      check(aegp_get_comp_framerate(aexcompat::scene_runtime::composition_handle(), &fps) == 0 &&
                comp.frame_rate_fixed == static_cast<int32_t>(fps) << 16,
            "comp item frame rate differs from the AEGP comp suite");
      AegpTime duration{};
      check(aegp_get_item_duration(aegp_comp_item_handle(), &duration) == 0 &&
                comp.duration.value == duration.value &&
                comp.duration.scale == duration.scale,
            "comp item duration differs from the AEGP item suite");
      // The effect layer's parent comp handle (AEGP Layer Suite
      // AEGP_GetLayerParentComp) is the same comp item object the layer's
      // +0x260 names, so a caller that reads its AEGP_CompH as a BEE_CompItem*
      // (ShapeBlur, issue #1264) lands on the facade; the comp accessors
      // resolve it as this worker's composition, and the project it names
      // carries an empty shared_ptr<PF_ColorSettings> at +0xe8/+0xf0 (what
      // BEE_CompItem::GetColorSettings copies out).
      void* parent_comp = nullptr;
      check(aegp_get_layer_parent_comp(layer, &parent_comp) == 0 &&
                parent_comp == bee::comp_item_handle() &&
                parent_comp == object->parent_comp_item,
            "AEGP_GetLayerParentComp did not hand out the facade comp item");
      check(scene_handle_is_composition(parent_comp),
            "the facade comp item does not resolve as the composition");
      void* item_from_comp = nullptr;
      int32_t item_width = 0, item_height = 0;
      check(aegp_get_item_from_comp(parent_comp, &item_from_comp) == 0 &&
                item_from_comp != nullptr &&
                aegp_get_item_dimensions(item_from_comp, &item_width, &item_height) == 0 &&
                item_width == kFacadeContextWidth && item_height == kFacadeContextHeight,
            "AEGP_GetItemFromComp did not resolve the facade comp handle to the comp item");
      double parent_fps = 0.0;
      check(aegp_get_comp_framerate(parent_comp, &parent_fps) == 0 && parent_fps == fps,
            "AEGP_GetCompFramerate did not resolve the facade comp handle");
      const auto* project_bytes = reinterpret_cast<const std::byte*>(&bee::project());
      void* color_settings = nullptr;
      void* color_settings_control = nullptr;
      std::memcpy(&color_settings, project_bytes + 0xe8, sizeof(color_settings));
      std::memcpy(&color_settings_control, project_bytes + 0xf0, sizeof(color_settings_control));
      check(comp.parent_project == &bee::project() && color_settings == nullptr &&
                color_settings_control == nullptr,
            "project colour settings shared_ptr is not the empty one");
    }
    check(release_suite("AEGP PF Interface Suite", 1) == 0,
          "AEGP PF Interface Suite v1 did not release");
  }
  check(suite_leases_balanced(), "suite leases did not balance");
  return passed;
}

// The PF_ProgressInfo-shaped effect ref (issue #1275): the object the
// production bootstrap installs at in_data->effect_ref (`g_effect`) carries
// the published layout, and both function slots, read at their raw offsets
// the way PF.dll's PF_TransferRect body and CannedWarp's RENDER read them,
// forward to the host's abort / progress callbacks (the ones the inter table
// carries). Then the module's own contract (republish after a plug-in edit).
bool verify_pf_progress_info() {
  namespace info = aexcompat::worker_runtime::pf_progress_info;
  namespace contract = aexcompat::abi::x86_64_windows;
  bool passed = true;
  auto check = [&](bool condition, const char* what) {
    if (condition) return;
    passed = false;
    std::cerr << "pf_progress_info: " << what << "\n" << std::flush;
  };
  publish_effect_ref_object();
  check(info::published(g_effect), "the production effect ref is not published");
  // Through the installed in_data: the bootstrap writes &g_effect at
  // IN_EFFECT_REF_OFFSET, and the inter table's abort / progress are the host
  // callbacks the slots must forward to.
  aexcompat::worker_runtime::effect_bootstrap::State state{};
  aexcompat::worker_runtime::effect_bootstrap::install_callback_tables(
      state, ::make_bootstrap_abi_hooks());
  void* effect_ref = nullptr;
  std::memcpy(&effect_ref, state.input.data() + contract::IN_EFFECT_REF_OFFSET,
              sizeof(effect_ref));
  check(effect_ref == &g_effect, "in_data->effect_ref is not the host effect ref");
  if (!effect_ref) return false;
  const auto* bytes = static_cast<const std::byte*>(effect_ref);
  void* refcon = nullptr;
  info::AbortFn abort_fn = nullptr;
  info::ProgressFn progress_fn = nullptr;
  std::memcpy(&refcon, bytes + 0x00, sizeof(refcon));
  std::memcpy(&abort_fn, bytes + 0x08, sizeof(abort_fn));
  std::memcpy(&progress_fn, bytes + 0x10, sizeof(progress_fn));
  check(refcon == effect_ref, "effect_ref[0] (refcon) is not the effect ref itself");
  check(abort_fn != nullptr && progress_fn != nullptr, "a function slot is null");
  if (!abort_fn || !progress_fn) return false;
  const auto& telemetry = aexcompat::worker_runtime::classic::host_callback_telemetry();
  const uint32_t aborts = telemetry.abort_calls;
  const uint32_t progresses = telemetry.progress_calls;
  const uint32_t abort_slot = info::abort_slot_calls();
  const uint32_t progress_slot = info::progress_slot_calls();
  check(progress_fn(refcon, 7, 20) == 0, "progress slot did not answer 0");
  check(abort_fn(refcon) == 0, "abort slot did not answer 0");
  check(telemetry.abort_calls == aborts + 1 && telemetry.progress_calls == progresses + 1 &&
            telemetry.last_progress_current == 7 && telemetry.last_progress_total == 20,
        "slots did not forward to the host abort / progress callbacks");
  check(info::abort_slot_calls() == abort_slot + 1 &&
            info::progress_slot_calls() == progress_slot + 1,
        "slot call counters did not advance");
  // Echo's substitution: it writes effect_ref[2] = effect_ref[1] for its
  // PF_TransferRect loop; PF.dll then calls that slot with the progress
  // arguments. A republish (the next plug-in bootstrap) restores the layout.
  std::memcpy(static_cast<std::byte*>(effect_ref) + 0x10, &abort_fn, sizeof(abort_fn));
  info::ProgressFn substituted = nullptr;
  std::memcpy(&substituted, bytes + 0x10, sizeof(substituted));
  check(substituted(refcon, 3, 9) == 0 && telemetry.abort_calls == aborts + 2,
        "the substituted slot did not act as the abort poll");
  check(!info::published(g_effect), "an edited effect ref still reads as published");
  publish_effect_ref_object();
  check(info::published(g_effect), "republish did not restore the layout");
  check(info::selftest(), "module contract self-test failed");
  return passed;
}

// The PF_World facade behind reserved_long4 (issue #1276): a world registered
// for dispatch the way the render paths register theirs carries the object,
// PF_NewWorld's world carries one too and loses it on dispose, slot 1 answers
// the registered depth, and a sampled unobserved slot traps into the report.
bool verify_pf_world_facade() {
  namespace facade = aexcompat::worker_runtime::pf_world_facade;
  bool passed = true;
  auto check = [&](bool condition, const char* what) {
    if (condition) return;
    passed = false;
    std::cerr << "pf_world_facade: " << what << "\n" << std::flush;
  };
  const auto recorded = [](uint32_t slot) {
    std::ostringstream needle;
    needle << "{\"name\":\"PF_World vtable\",\"version\":1,\"slot\":" << slot << ",";
    return aexcompat::worker_runtime::suite_registry()
               .unsupported_suite_calls_report_json()
               .find(needle.str()) != std::string::npos;
  };
  using Depth = int16_t (__cdecl*)(const void*);
  check(facade::selftest(), "facade contract self-test failed");
  // That self-test ends by filling the quarantine to its bound, so from here
  // on every detach / dispose that actually retires an object evicts - freeing
  // whichever object has been quarantined longest, not the one it just
  // retired (a detach of something the pool never held retires nothing). Nothing
  // below reads an object it has taken out of service, and nothing added below
  // should; the property that a retired object stays readable is asserted
  // inside `facade::selftest()`, before its churn section.
  // The module self-test trapped slots 0, 10 and the last one; the worker's
  // recorder must have put them in the report under the pseudo-suite name.
  check(recorded(0) && recorded(10) &&
            recorded(static_cast<uint32_t>(facade::kVtableSlots - 1)),
        "trapped slots are not in the unsupported_suite_calls report");
  // A world prepared and registered the way the classic / smart paths do it
  // (EffectWorldStorage through prepare_world_layout, then register_world), at
  // each depth: AE's embedded shape, vtable at world - 8, no pool object.
  const struct { int32_t pixel_format; int16_t depth; } cases[] = {
      {kPixelFormatArgb32, 8}, {kPixelFormatArgb64, 16}, {kPixelFormatArgb128, 32}};
  for (const auto& entry : cases) {
    world_safety::EffectWorldStorage world{};
    std::array<std::byte, 16 * 4> pixels{};
    std::array<std::byte, world_safety::kEffectWorldSize> bare{};
    // Declared after every buffer it can register: the scope's destructor
    // detaches the worlds it gave a pool object to (an embedded world never
    // enters that list), and that must not run on storage whose lifetime has
    // already ended.
    aexcompat::world_safety::DispatchWorldFormatScope worlds;
    const int32_t pixel_bytes = entry.depth / 8 * 4;
    if (!aexcompat::render::prepare_world_layout(
            world, {pixel_bytes == 4 ? 0 : 1, pixel_bytes, 4, 1, 4 * pixel_bytes},
            pixels.data())) {
      check(false, "world layout preparation failed");
      continue;
    }
    check(worlds.register_world(world.data(), entry.pixel_format), "world registration failed");
    void* reserved_long4 = nullptr;
    std::memcpy(&reserved_long4, world.data() + 0x50, sizeof(reserved_long4));
    check(reserved_long4 == world.data() - 8 && facade::embedded(world.data()) &&
              facade::attached(world.data()) == nullptr,
          "prepared world is not the embedded PF_World shape");
    if (!reserved_long4) continue;
    const void* const* vtable = *static_cast<const void* const* const*>(reserved_long4);
    check(vtable == world.pf_world_vtable && vtable == facade::vtable_for(pixel_bytes) &&
              reinterpret_cast<Depth>(vtable[facade::kSlotDepth])(reserved_long4) == entry.depth,
          "slot 1 did not answer the registered depth");
    // The LayerDef the plug-in sees is the object's own +8 (no mirror): its
    // data pointer, and a write through the object reaches the LayerDef.
    const auto* object = static_cast<const std::byte*>(reserved_long4);
    void* object_data = nullptr;
    std::memcpy(&object_data, object + 0x20, sizeof(object_data));
    check(object_data == pixels.data(), "object +0x20 is not the LayerDef's data pointer");
    // A bare 120-byte struct registered the same way gets the mirror object.
    bare = world.layer_def;
    void* null = nullptr;
    std::memcpy(bare.data() + 0x50, &null, sizeof(null));
    check(worlds.register_world(bare.data(), entry.pixel_format), "bare world registration failed");
    std::memcpy(&reserved_long4, bare.data() + 0x50, sizeof(reserved_long4));
    check(reserved_long4 != nullptr && reserved_long4 == facade::attached(bare.data()) &&
              !facade::embedded(bare.data()),
          "bare registered world did not get a mirror object");
    if (reserved_long4) {
      const void* const* bare_vtable = *static_cast<const void* const* const*>(reserved_long4);
      check(reinterpret_cast<Depth>(bare_vtable[facade::kSlotDepth])(reserved_long4) == entry.depth,
            "mirror object slot 1 did not answer the registered depth");
      facade::detach(bare.data());
    }
  }
  // PF_NewWorld: the world the World Suite fills carries the facade for the
  // allocation's lifetime; dispose clears the struct with it.
  {
    std::array<std::byte, world_safety::kEffectWorldSize> world{};
    check(aexcompat::world_registry::new_world(&g_effect, 3, 2, 1, kPixelFormatArgb64,
                                               world.data()) == 0,
          "PF_NewWorld failed");
    void* reserved_long4 = nullptr;
    std::memcpy(&reserved_long4, world.data() + 0x50, sizeof(reserved_long4));
    check(reserved_long4 != nullptr, "PF_NewWorld left reserved_long4 null");
    if (reserved_long4) {
      const void* const* vtable = *static_cast<const void* const* const*>(reserved_long4);
      check(vtable && reinterpret_cast<Depth>(vtable[facade::kSlotDepth])(reserved_long4) == 16,
            "PF_NewWorld facade slot 1 did not answer 16");
      // Channel Blur's write lands in the mirror's origin, not in host memory
      // (issue #1090): the LayerDef itself keeps its origin.
      auto* mirror = static_cast<std::byte*>(reserved_long4);
      const int32_t origin_x = 5, origin_y = -6;
      std::memcpy(mirror + 0x70, &origin_x, sizeof(origin_x));
      std::memcpy(mirror + 0x74, &origin_y, sizeof(origin_y));
      int32_t world_origin_x = 1;
      std::memcpy(&world_origin_x, world.data() + 0x68, sizeof(world_origin_x));
      check(world_origin_x == 0, "a mirror write reached the LayerDef");
    }
    check(aexcompat::world_registry::dispose_world(&g_effect, world.data()) == 0,
          "PF_DisposeWorld failed");
    std::memcpy(&reserved_long4, world.data() + 0x50, sizeof(reserved_long4));
    check(reserved_long4 == nullptr, "dispose left reserved_long4 set");
  }
  return passed;
}
}  // namespace aexcompat::l2_detail

aexcompat::worker_runtime::effect_bootstrap::RuntimeHooks make_bootstrap_runtime_hooks() {
  return {&invoke_entry_seh, &reset_effect_lifetime,
   +[](bool active) { g_global_setup_active = active; },
   +[](bool requested, bool advertised) {
     std::vector<int32_t> layer_indices{0};
     for (const auto& parameter : g_params) {
       if (parameter.type == 0) layer_indices.push_back(parameter.index);
     }
     aexcompat::host_audio::runtime().configure_admission(
         requested, advertised, layer_indices);
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
  aexcompat::worker_runtime::ui_event_execution::reset_info_text_telemetry();
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
  aexcompat::worker_runtime::active_plugin::effect_module = module;
  aexcompat::worker_runtime::active_plugin::string_table = &table;
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
  PiplEntrypoint pipl_entrypoint = discover_effect_entrypoint(module);
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
  // effects) get their init once per U.dll mapping, asked again for every
  // member so a first member without U.dll decides nothing (#362, #1063).
  initialize_legacy_support_libraries();
  publish_effect_ref_object();
  // GLOBAL_SETUP / PARAMS_SETUP through the launch bootstrap on fresh
  // buffers and fresh host records (ABOUT stays whatever the launch did).
  reset_cluster_effect_state();
  *context.effect_state = aexcompat::worker_runtime::effect_bootstrap::State{};
  const auto bootstrap = context.run_bootstrap(new_entry);
  result.entry = new_entry;
  result.global_setup_error = bootstrap.global_error;
  result.params_setup_error = bootstrap.params_error;
  result.audio_effect_only = bootstrap.audio_effect_only;
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
  PiplPluginKind current_entrypoint_kind = PiplPluginKind::Unknown;
  int32_t current_index = -1;
  std::string current_plugin_data_selector;
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
    std::ostringstream report;
    report << "{\"schema_version\":1,\"stage\":\"discovery_session\",\"status\":\""
           << (exit_code == 0 && !bib_teardown_failed
                   ? "discovery_session_completed" : "discovery_session_failed")
           << "\",\"bib_terminations\":" << bib_termination_attempt_count()
           << ",\"module_audit\":" << wr::module_audit_json() << "}\n";
    if (!wr::emit_protocol_stdout(report.str())) return 23;
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
    int32_t effect_index{};
    std::string effect_match_name_hex;
    const bool has_effect_index = object.find("effect_index") != object.end();
    const bool has_effect_match = object.find("effect_match_name_hex") != object.end();
    if (type != "inspect_plugin" ||
        !(json_exact_keys(object,
                          {"v", "type", "plugin_index", "request_index"}) ||
          json_exact_keys(object,
                          {"v", "type", "plugin_index", "request_index",
                           "effect_index", "effect_match_name_hex"})) ||
        !json_i32(object, "plugin_index", plugin_index) ||
        !json_i32(object, "request_index", request_index) ||
        has_effect_index != has_effect_match ||
        (has_effect_index &&
         (!json_i32(object, "effect_index", effect_index) ||
          !json_string(object, "effect_match_name_hex", effect_match_name_hex))) ||
        plugin_index < 0 ||
        plugin_index >= static_cast<int32_t>(manifest.plugins.size()) ||
        request_index != expected_request_index) {
      protocol_violation = true;
      break;
    }
    ++expected_request_index;
    std::string requested_plugin_data_selector;
    g_plugin_data_selector_present = false;
    g_plugin_data_selector_index = 0;
    g_plugin_data_selector_match.clear();
    if (has_effect_index) {
      requested_plugin_data_selector = "v1|" + std::to_string(effect_index) +
          "|" + effect_match_name_hex;
      const std::wstring encoded(requested_plugin_data_selector.begin(),
                                 requested_plugin_data_selector.end());
      if (!capture_plugin_data_selector(nullptr, encoded.c_str())) {
        protocol_violation = true;
        break;
      }
    }

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
      const bool inspect_ready = current_global_error == 0 &&
          bootstrap.params_error == 0 &&
          bootstrap.parameter_count_contract_valid && defaults_disposed;
      bool checkpoint_written = true;
      // Keep the serialized checkpoint storage alive across GLOBAL_SETDOWN.
      // After a successful pipe write, the selector call below is the next
      // potentially faulting operation; string destruction cannot be mistaken
      // for a cleanup-selector crash.
      std::string checkpoint_report;
      std::string checkpoint;
      if (inspect_ready) {
        checkpoint_report = build_l2_report_json(
            "parameters_inspected_pre_setdown", current_global_error,
            bootstrap.params_error, -1, effect_state.output,
            bootstrap.about_message, {-1, -1, -1, -1, -1}, true,
            /*include_module_audit=*/false);
        while (!checkpoint_report.empty() &&
               (checkpoint_report.back() == '\n' ||
                checkpoint_report.back() == '\r'))
          checkpoint_report.pop_back();
        const std::string& plugin_sha256 =
            manifest.plugins[static_cast<std::size_t>(plugin_index)].sha256;
        checkpoint =
            "{\"v\":1,\"type\":\"inspect_checkpoint\",\"plugin_index\":" +
            std::to_string(plugin_index) + ",\"request_index\":" +
            std::to_string(request_index) + ",\"plugin_sha256\":\"" +
            plugin_sha256 + "\",\"report\":" + checkpoint_report + "}";
        checkpoint_written = !checkpoint_report.empty() &&
            pipes.write_message(checkpoint);
      }
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
      if (!checkpoint_written) protocol_violation = true;
    };
    if (plugin_index != current_index ||
        requested_plugin_data_selector != current_plugin_data_selector) {
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
      current_plugin_data_selector = requested_plugin_data_selector;
      g_plugin_file_path =
          cluster::plugin_path(manifest, static_cast<std::size_t>(plugin_index))
              .wstring();
      // Entrypoint resolution follows the launch rules; a failure is
      // plug-in-local (design §4.2) and the session continues.
      PiplEntrypoint pipl_entrypoint = discover_effect_entrypoint(current_module);
      current_entrypoint_kind = pipl_entrypoint.kind;
      if (pipl_entrypoint.kind == PiplPluginKind::Effect)
        current_entry = reinterpret_cast<EffectEntry>(
            GetProcAddress(current_module, pipl_entrypoint.symbol.c_str()));
      else
        current_entry = nullptr;
      if (current_entry) {
        activate_plugin_string_table(current_module, aex_string_table);
        // U.dll init for PIN-era closures, once per mapping (#362, #1063).
        initialize_legacy_support_libraries();
        publish_effect_ref_object();
        // Attribute the BEE facade counters to this column. Opened here and
        // not at the plug-in swap: the swap first runs the OUTGOING
        // plug-in's GLOBAL_SETDOWN and arbitrary-data disposal, whose facade
        // calls belong to that plug-in, and a re-inspect of the current
        // plug-in takes no swap at all (issue #1264).
        aexcompat::worker_runtime::bee_facade::begin_attribution_window();
        inspect_column();
      } else {
        current_global_error = -1;
        error_kind = "entrypoint_unresolved";
        // The one-shot path preserves this PiPL classification on stderr so
        // callers can route genuine AEGPs to the AEGP initializer.  A
        // DiscoverySession has no stderr-derived per-member diagnostics, so
        // carry the same bounded classification in its partial report.
        report_json = unresolved_entrypoint_report_json(pipl_entrypoint.kind);
      }
    } else if (!current_entry) {
      // Re-inspect of a plug-in whose entrypoint never resolved.
      error_kind = "entrypoint_unresolved";
      report_json = unresolved_entrypoint_report_json(current_entrypoint_kind);
    } else {
      // Re-inspect of the current plug-in: set down only an unpaired setup
      // (the previous inspect column already set a successful one down),
      // then the same inspect column again, matching a fresh one-shot process.
      if (current_setdown_pending) {
        invoke_global_setdown(current_entry, effect_state.input.data(),
                              effect_state.output.data());
        current_setdown_pending = false;
      }
      // Attribute the BEE facade counters to this column. Opened here and
      // not at the plug-in swap: the swap first runs the OUTGOING
      // plug-in's GLOBAL_SETDOWN and arbitrary-data disposal, whose facade
      // calls belong to that plug-in, and a re-inspect of the current
      // plug-in takes no swap at all (issue #1264).
      aexcompat::worker_runtime::bee_facade::begin_attribution_window();
      inspect_column();
    }

    if (protocol_violation) break;
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
              << "\",\"v2\":true,\"v1_fallback\":true,\"multi_effect\":true,"
                 "\"bounded\":true,\"fail_closed\":true}\n";
    return passed ? 0 : 73;
  }
  if (argc == 2 &&
      std::wstring(argv[1]) == L"--self-test-l2-point-default-units") {
    const bool passed = verify_l2_point_default_units();
    std::cout << "{\"l2_point_default_units\":\""
              << (passed ? "passed" : "failed")
              << "\",\"width\":256,\"height\":144,"
                 "\"point\":[64,72],\"point3d\":[64,72,108]}\n";
    return passed ? 0 : 74;
  }
  RuntimeHostHooks runtime_hooks{&sha256, &redirect_native_stdout,
                                  &restore_native_stdout};
  // GPU module-audit preflight (#290): authorize the AEXRMA1-listed GPU runtime
  // DLLs, load them via the transport backend, and emit the classified module
  // report the broker re-authenticates before a secure GPU dispatch. argv shape
  // mirrors the params-only manifest convention:
  //   --gpu-module-report-v1 <plugin> <sha256> --runtime-module-authorization-v1 <manifest>
  const bool gpu_module_report_has_dependency_dirs =
      argc == 8 && std::wstring(argv[6]) == L"--dependency-dirs-v1";
  if ((argc == 6 || gpu_module_report_has_dependency_dirs) &&
      std::wstring(argv[1]) == L"--gpu-module-report-v1" &&
      std::wstring(argv[4]) == L"--runtime-module-authorization-v1") {
    namespace wr = aexcompat::worker_runtime;
    namespace tp = aexcompat::gpu_runtime::memory_world_transport;
    // Manifest backend id (1=cuda,2=opencl,3=directx,4=opengl) → transport
    // framework code (3=cuda,1=opencl,4=directx). OpenGL is inspect-only; a
    // smart-session GPU render never runs on it.
    // Fail-closed identity gate and plugin load are owned by the common runtime
    // admission component. This keeps l2_main from becoming a second plugin-load
    // owner while retaining the GPU-specific audit/report lifecycle below.
    RuntimeAdmissionRequest runtime_request;
    const int request_error = wr::prepare_runtime_request(
        argv[2], argv[3], true, argv[5], runtime_request);
    if (request_error != 0) return request_error;
    if (gpu_module_report_has_dependency_dirs) {
      const int dirs_error = wr::apply_dependency_search_dirs(argv[7], runtime_request);
      if (dirs_error != 0) return dirs_error;
    }
    RuntimeContext runtime_context;
    const int admission_error = wr::admit_runtime(
        runtime_hooks, runtime_request, runtime_context);
    if (admission_error != 0) return admission_error;
    const uint32_t backend = wr::authorized_runtime_backend();
    const int32_t framework = backend == 1   ? 3
                              : backend == 2 ? 1
                              : backend == 3 ? 4
                                             : 0;
    if (framework == 0) {
      wr::release_runtime_context(runtime_context);
      return 75;
    }
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
      aexcompat::worker_runtime::request_parser::Kind::Classic, argc, argv,
      {{nullptr, set_l2_dump_worlds_dir, enable_l2_checksum_detail,
        load_l2_aux_manifest, parse_l2_alpha_coverage, load_l2_parameter_animation,
        parse_l2_conformance_render_settings,
        capture_l2_runtime_module_authorization, capture_l2_cluster_manifest,
        capture_l2_companion_manifest, capture_l2_dependency_search_dirs,
        capture_plugin_data_selector},
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
        capture_l2_companion_manifest, capture_l2_dependency_search_dirs,
        capture_plugin_data_selector},
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
    bool saw_dependency_dirs = false;
    bool saw_plugin_data_selector = false;
    while (l2_argc >= 3) {
      const std::wstring flag(argv[l2_argc - 2]);
      if (flag == L"--dependency-dirs-v1" && !saw_dependency_dirs) {
        if (!capture_l2_dependency_search_dirs(nullptr, argv[l2_argc - 1]))
          return 2;
        saw_dependency_dirs = true;
      } else if (flag == L"--plugin-data-selector-v1" &&
                 !saw_plugin_data_selector) {
        if (!capture_plugin_data_selector(nullptr, argv[l2_argc - 1])) return 2;
        saw_plugin_data_selector = true;
      } else {
        break;
      }
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
  const int prepare_error = aexcompat::worker_runtime::prepare_worker_entry(
      runtime_hooks, runtime_request, trace_worker_label(), trace_writer,
      runtime_context);
  if (prepare_error != 0) return prepare_error;
  // Companion AEGPs are host providers. Initialize them after the worker has
  // established its authenticated DLL-search/stdout context, but before the
  // consumer PF module executes.
  aexcompat::worker_runtime::companions::Runtime companion_runtime;
  if (!g_companion_manifest_path.empty()) {
    aexcompat::worker_runtime::companions::Manifest companion_manifest;
    const bool previous_aegp_init_mode = g_aegp_init_mode;
    g_aegp_init_mode = true;
    const bool manifest_loaded =
        aexcompat::worker_runtime::companions::load_manifest(
            g_companion_manifest_path, companion_manifest);
    const bool companion_initialized = manifest_loaded &&
        companion_runtime.initialize(companion_manifest, &g_basic_suite,
                                     &sha256);
    g_aegp_init_mode = previous_aegp_init_mode;
    if (!companion_initialized) {
      aexcompat::worker_runtime::release_runtime_context(runtime_context);
      return 26;
    }
  }
  const int admission_error =
      aexcompat::worker_runtime::load_runtime_plugin(runtime_request,
                                                     runtime_context);
  if (admission_error != 0) {
    (void)companion_runtime.shutdown();
    aexcompat::worker_runtime::release_runtime_context(runtime_context);
    return admission_error;
  }
  WorkerSession session(runtime_context, trace_writer.get(), &g_trace_writer);
  SessionPreUnloadContext pre_unload_context{&companion_runtime};
  (void)session.set_pre_unload_hook(
      &teardown_session_providers, &pre_unload_context);
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
  PiplEntrypoint pipl_entrypoint = discover_effect_entrypoint(module);
  // PluginData is a registration ABI used by many bundled effects that do
  // not carry a PiPL resource. It is deliberately reachable only for a
  // missing PiPL: malformed, ambiguous, AEGP, or unknown PiPL resources stay
  // fail-closed and never get a second interpretation.
  if (pipl_entrypoint.kind != PiplPluginKind::Effect) {
    const char* plugin_kind =
        unresolved_entrypoint_plugin_kind(pipl_entrypoint.kind);
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
  // U.dll init for PIN-era closures, once per mapping (#362, #1063).
  initialize_legacy_support_libraries();
  publish_effect_ref_object();
  const char* string_table_status =
      aex_string_table.status == aexcompat::aex_strings::ParseStatus::Valid
          ? "valid"
          : aex_string_table.status == aexcompat::aex_strings::ParseStatus::NoEntries
              ? "none"
              : "invalid";
  std::cerr << "string_table_status:" << string_table_status << "\n" << std::flush;
  aexcompat::worker_runtime::active_plugin::effect_module = module;
  aexcompat::worker_runtime::active_plugin::string_table = &aex_string_table;
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
         invocation.audio_session_mode, g_skip_about,
         invocation.external_current_time, invocation.external_time_step,
         invocation.external_total_time, invocation.external_time_scale},
        make_bootstrap_runtime_hooks());
  };
  aexcompat::worker_runtime::handles::HandleReclamationScope
      handle_reclamation_scope;
  const auto bootstrap = [&] {
    std::array<wchar_t, 32768> original_directory{};
    std::array<wchar_t, 32768> gpu_module_path{};
    const DWORD original_length = GetCurrentDirectoryW(
        static_cast<DWORD>(original_directory.size()), original_directory.data());
    const DWORD module_length = GetModuleFileNameW(
        GetModuleHandleW(L"GPUFoundation.dll"), gpu_module_path.data(),
        static_cast<DWORD>(gpu_module_path.size()));
    bool changed_directory = false;
    if (original_length > 0 && original_length < original_directory.size() &&
        module_length > 0 && module_length < gpu_module_path.size()) {
      for (DWORD cursor = module_length; cursor > 0; --cursor) {
        if (gpu_module_path[cursor - 1] == L'\\' ||
            gpu_module_path[cursor - 1] == L'/') {
          gpu_module_path[cursor - 1] = L'\0';
          changed_directory = SetCurrentDirectoryW(gpu_module_path.data()) != FALSE;
          break;
        }
      }
    }
    auto result = run_bootstrap(entry);
    if (changed_directory) SetCurrentDirectoryW(original_directory.data());
    return result;
  }();
  aexcompat::worker_runtime::active_plugin::string_table = nullptr;
  aexcompat::worker_runtime::active_plugin::effect_module = nullptr;
  const int32_t global_error = bootstrap.global_error;
  const int32_t about_error = bootstrap.about_error;
  const int32_t params_error = bootstrap.params_error;
  const uint32_t advertised_out_flags = bootstrap.advertised_out_flags;
  const uint32_t advertised_out_flags2 = bootstrap.advertised_out_flags2;
  const bool image_render_supported = bootstrap.image_render_supported;
  const bool audio_effect_only = bootstrap.audio_effect_only;
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
      invocation.auto_dialog_mode, invocation.do_dialog_mode,
      invocation.external_dependencies_mode, invocation.params_only_mode,
      invocation.cleanup_contained_params_only_mode);
  if (!is_rendering_worker() && early_mode != aexcompat::l2mode::EarlyMode::None) {
    EarlyModeBridge bridge{entry, &input, &output, &session, &about_message};
    const int early_result = aexcompat::l2mode::run_early_mode(
        {early_mode, &bridge, early_mode_hooks(), global_error, params_error,
         parameter_count_contract_valid,
         invocation.external_dependencies_mode ? argv[4] : nullptr});
    if (invocation.cleanup_contained_params_only_mode) {
      // Do not return through WorkerSession destruction: that would unload the
      // plug-in and re-enter the teardown race this mode exists to contain.
      std::cout.flush();
      std::cerr.flush();
      if (!TerminateProcess(GetCurrentProcess(), static_cast<UINT>(early_result)))
        ExitProcess(static_cast<UINT>(early_result));
      return early_result;  // Unreachable when TerminateProcess succeeds.
    }
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
            lifecycle_data_null, user_changed_ok, conditional_ui_ok,
            invocation.external_width, invocation.external_height))
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

  // The bootstrap cleared the active string table at its frame boundary above,
  // but the plug-in's own RENDER selector may still look up its embedded
  // strings (Basic_Text/Numbers/Path_Text read popup-menu ids during RENDER,
  // not only PARAMS_SETUP), and a null active table returns null instead of the
  // valid-table empty-string fallback, which those effects dereference and
  // crash on (issue #1073). Re-activate the launch plug-in's table (still alive
  // in this frame's scope) for the render dispatch and the setdown tail; a
  // cluster swap re-activates the swapped member's own table during the
  // session, so this only supplies the launch member's render, and GLOBAL_SETDOWN
  // of a swapped member runs under its own table (left active by the last swap).
  // The scope restores the thread-local to null on every exit of this function,
  // including the case_id_rejected early returns below, so the pointer never
  // dangles into the next invocation on this thread. The synchronous classic
  // and smart render paths and the render session run the RENDER selector on
  // this thread. The concurrent_render (threaded_default) path snapshots this
  // context and activates it independently in each spawned render thread.
  aexcompat::worker_runtime::active_plugin::Scope active_string_table_scope(
      &aex_string_table, module);
  if (is_render_worker()) {
    const auto dispatch = aexcompat::worker_runtime::invocation::run_classic_final_dispatch(
        {entry, &input, &output, &invocation, argv, params_error,
         image_render_supported, depth_supported, smart_render_supported,
         cluster_session_active ? &cluster_swap_hook : nullptr, audio_effect_only});
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
  dispose_all_live_handles();
  handle_reclamation_scope.reclaim();
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
  const bool protocol_report_ready = session.prepare_protocol_report();
  if (!protocol_report_ready && !session_swap_failure)
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
  // Through the generated contract, like the two sites that write the field
  // (worker_classic_render_runtime.cpp, worker_smart_dispatch.cpp): a
  // regeneration that moved it would otherwise leave the reader describing
  // whatever now lives at 276 while the writers followed the move.
  classic_inputs.context_extent = {
      read<int32_t>(input, aexcompat::abi::x86_64_windows::IN_OUTPUT_ORIGIN_X_OFFSET),
      read<int32_t>(input, aexcompat::abi::x86_64_windows::IN_OUTPUT_ORIGIN_Y_OFFSET)};
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
  smart_inputs.context_extent = {
      read<int32_t>(input, aexcompat::abi::x86_64_windows::IN_OUTPUT_ORIGIN_X_OFFSET),
      read<int32_t>(input, aexcompat::abi::x86_64_windows::IN_OUTPUT_ORIGIN_Y_OFFSET)};
  emit_smart_completion_report(smart_inputs);
  std::cout.flush();
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
    const int session_exit = session_swap_failure ? 25
        : session_protocol_violation ? 23
        : session_invariant_failure ? 24
        : global_error == 0 && params_error == 0 && parameter_count_contract_valid &&
              ((image_render_supported && depth_supported) || audio_effect_only) &&
              render_error == 0 && guards_intact &&
              handle_lifetimes_balanced() && world_lifetimes_balanced() &&
              async_receipt_lifetimes_balanced() &&
              async_layer_requests_balanced() &&
              gpu_memory_lifetimes_balanced() &&
              audio_handle_lifetimes_balanced() &&
              audio_telemetry().invalid_operations == 0 &&
              param_checkouts_balanced() &&
              ((!g_render_click_enabled && !g_render_draw_enabled) ||
               g_render_ui_context_closed)
          ? 0 : 21;
    if (invocation.render_session_mode && protocol_report_ready)
      session.terminate_after_protocol_report(session_exit);
    if (session_swap_failure) return session.finish_integrated_report(session_exit);
    if (session_protocol_violation || session_invariant_failure)
      return session.finish(session_exit);
    // An AUDIO_EFFECT_ONLY session succeeds by passthrough (issue #1048):
    // there is no video selector to support, so its exit is decided by the
    // session mechanics alone, like any other render.
    return session.finish(session_exit);
  }
  if (is_smart_worker()) {
    // The smart session shares the render worker's dedicated fail-closed exit
    // codes: protocol violations (23) and host-protection invariant failures
    // (24) stay distinguishable from ordinary smart failures (22).
    if (invocation.render_session_mode) {
      // Frame-local selector errors were reported through frame_done and the
      // broker owned the continue decision; only session mechanics and host
      // state cleanliness decide the exit.
      const int session_exit = session_protocol_violation ? 23
          : session_invariant_failure ? 24
          : global_error == 0 && params_error == 0 && parameter_count_contract_valid &&
        image_render_supported && depth_supported && smart_render_supported &&
        smart_session_render_error == 0 && smart.guards_intact &&
        handle_lifetimes_balanced() && world_lifetimes_balanced() &&
        gpu_memory_lifetimes_balanced() &&
        audio_handle_lifetimes_balanced() && audio_telemetry().invalid_operations == 0 &&
        param_checkouts_balanced() &&
        ((!g_render_click_enabled && !g_render_draw_enabled) ||
         g_render_ui_context_closed) ? 0 : 22;
      session.terminate_after_protocol_report(session_exit);
    }
    if (session_protocol_violation) return session.finish(23);
    if (session_invariant_failure) return session.finish(24);
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
  std::string sound_diagnostic;
  if (!worker_runtime::system_sound_suppression::install(sound_diagnostic)) {
    std::cerr << "stage:system_sound_suppression_failed detail="
              << sound_diagnostic << '\n' << std::flush;
    return 11;
  }
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
       &aegp_get_effect_param_union_by_index_v3,
       &get_new_effect_for_effect, &aegp_dispose_effect,
       &aegp_get_effect_num_param_streams_v2});
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
