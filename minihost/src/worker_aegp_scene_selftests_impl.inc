// Private AEGP scene-family native selftests. Included after all worker suite adapters.

bool verify_legacy_effect_compat_suites() {
  const void* comp_suite = nullptr;
  const void* interface_suite = nullptr;
  const void* helper_suite = nullptr;
  bool ok = acquire_suite("AEGP Comp Suite", 21, &comp_suite) == 0 &&
      comp_suite == g_aegp_comp_suite10.data() &&
      g_aegp_comp_suite10[1] ==
          reinterpret_cast<void*>(&aegp_get_item_from_comp) &&
      acquire_suite("AEGP PF Interface Suite", 1, &interface_suite) == 0 &&
      interface_suite == &g_pf_interface_suite &&
      acquire_suite("AE Plugin Helper Suite", 1, &helper_suite) == 0 &&
      helper_suite == g_pf_helper_suite1.data();

  AegpColorVal color{-1.0, -2.0, -3.0, -4.0};
  const AegpColorVal color_sentinel = color;
  ok = ok && aegp_get_comp_bg_color(&g_aegp_comp, &color) == 0 &&
      color.alpha == 1.0 && color.red == 0.0 && color.green == 0.0 && color.blue == 0.0;
  color = color_sentinel;
  ok = ok && aegp_get_comp_bg_color(nullptr, &color) != 0 &&
      std::memcmp(&color, &color_sentinel, sizeof(color)) == 0 &&
      aegp_get_comp_bg_color(&g_aegp_comp_item, &color) != 0 &&
      std::memcmp(&color, &color_sentinel, sizeof(color)) == 0 &&
      aegp_get_comp_bg_color(&g_aegp_comp, nullptr) != 0;

  using GetItemFromComp = int32_t(__cdecl*)(void*, void**);
  auto get_item_from_comp =
      reinterpret_cast<GetItemFromComp>(g_aegp_comp_suite10[1]);
  void* item = nullptr;
  ok = ok && get_item_from_comp(&g_aegp_comp, &item) == 0 &&
      item == &g_aegp_comp_item;
  void* item_sentinel = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && get_item_from_comp(&g_layer, &item_sentinel) != 0 &&
      item_sentinel == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));

  AegpTime time{77, 99};
  const AegpTime time_sentinel = time;
  ok = ok && convert_effect_to_comp_time(&g_effect, -17, 24000, &time) == 0 &&
      time.value == -17 && time.scale == 24000;
  time = time_sentinel;
  ok = ok && convert_effect_to_comp_time(nullptr, 1, 30, &time) != 0 &&
      time.value == time_sentinel.value && time.scale == time_sentinel.scale &&
      convert_effect_to_comp_time(&g_effect, 1, 0, &time) != 0 &&
      time.value == time_sentinel.value && time.scale == time_sentinel.scale &&
      convert_effect_to_comp_time(&g_effect, (std::numeric_limits<int32_t>::min)(),
                                  (std::numeric_limits<uint32_t>::max)(), &time) == 0 &&
      time.value == (std::numeric_limits<int32_t>::min)() &&
      time.scale == (std::numeric_limits<uint32_t>::max)() &&
      convert_effect_to_comp_time(&g_effect, 0, 1, nullptr) != 0;

  g_pf_helper_effect_tool.store(14, std::memory_order_release);
  int32_t tool = -1;
  ok = ok && pf_get_current_tool(&tool) == 0 && tool == kPfSuiteToolNone &&
      pf_get_current_tool(nullptr) == kPfBadCallbackParam &&
      g_pf_helper_effect_tool.load(std::memory_order_acquire) == 14;
  reset_pf_helper_tools();

  ok = release_suite("AE Plugin Helper Suite", 1) == 0 && ok;
  ok = release_suite("AEGP PF Interface Suite", 1) == 0 && ok;
  ok = release_suite("AEGP Comp Suite", 21) == 0 && ok;
  return ok;
}

bool verify_aegp_get_effect_camera_case(bool smart_case) {
  const int32_t saved_camera_index = g_aegp_active_camera_layer_index;
  const bool saved_effect_live = g_pf_state_effect_live;
  const auto saved_in_points = g_aegp_layer_in_points;
  const auto saved_durations = g_aegp_layer_durations;
  reset_pf_state_effect_lifetime(&g_effect, true);
  g_aegp_active_camera_layer_index = -1;

  const AegpTime active_time{smart_case ? 45 : 15, 30};
  void* camera = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  bool ok = get_effect_camera(&g_effect, &active_time, &camera) == 0 && camera == nullptr;

  g_aegp_active_camera_layer_index = 2;
  g_aegp_layer_in_points[2] = {smart_case ? 30 : 10, 30};
  g_aegp_layer_durations[2] = {60, 30};
  camera = nullptr;
  ok = ok && get_effect_camera(&g_effect, &active_time, &camera) == 0 &&
      camera == &g_aegp_layers[2] && aegp_layer_index(camera) == 2;

  const auto unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678));
  camera = unchanged;
  AegpTime invalid_scale{active_time.value, 0};
  AegpTime before_in{smart_case ? 29 : 9, 30};
  AegpTime after_out{smart_case ? 90 : 70, 30};
  OpaqueHostObject foreign_effect{0x464f5247};
  ok = ok && get_effect_camera(nullptr, &active_time, &camera) != 0 && camera == unchanged &&
      get_effect_camera(&foreign_effect, &active_time, &camera) != 0 && camera == unchanged &&
      get_effect_camera(&g_effect, nullptr, &camera) != 0 && camera == unchanged &&
      get_effect_camera(&g_effect, &invalid_scale, &camera) != 0 && camera == unchanged;
  camera = unchanged;
  ok = ok && get_effect_camera(&g_effect, &before_in, &camera) == 0 && camera == nullptr;
  camera = unchanged;
  ok = ok && get_effect_camera(&g_effect, &after_out, &camera) == 0 && camera == nullptr &&
      get_effect_camera(&g_effect, &active_time, nullptr) != 0;
  camera = unchanged;
  reset_pf_state_effect_lifetime(&g_effect, false);
  ok = ok && get_effect_camera(&g_effect, &active_time, &camera) != 0 && camera == unchanged;

  g_aegp_active_camera_layer_index = saved_camera_index;
  g_aegp_layer_in_points = saved_in_points;
  g_aegp_layer_durations = saved_durations;
  reset_pf_state_effect_lifetime(&g_effect, saved_effect_live);
  return ok;
}

bool verify_aegp_get_effect_camera_matrix_case(bool smart_case) {
  const bool saved_effect_live = g_pf_state_effect_live;
  const int32_t saved_width = g_full_resolution_width;
  const int32_t saved_height = g_full_resolution_height;
  reset_pf_state_effect_lifetime(&g_effect, true);
  g_full_resolution_width = smart_case ? 1920 : 640;
  g_full_resolution_height = smart_case ? 1080 : 480;
  const AegpTime time{smart_case ? 45 : 15, 30};
  AegpMatrix4 matrix{};
  double distance = -1.0;
  int16_t width = -1, height = -1;
  bool ok = get_effect_camera_matrix(&g_effect, &time, &matrix, &distance,
      &width, &height) == 0 && distance == g_full_resolution_width &&
      width == g_full_resolution_width && height == g_full_resolution_height;
  for (std::size_t row = 0; row < 4; ++row) {
    for (std::size_t column = 0; column < 4; ++column) {
      ok = ok && matrix.mat[row][column] == (row == column ? 1.0 : 0.0);
    }
  }

  AegpMatrix4 sentinel{};
  std::memset(&sentinel, 0x5a, sizeof(sentinel));
  matrix = sentinel;
  distance = -2.0; width = -2; height = -2;
  AegpTime invalid_time{time.value, 0};
  ok = ok && get_effect_camera_matrix(nullptr, &time, &matrix, &distance,
      &width, &height) != 0 && std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0 &&
      distance == -2.0 && width == -2 && height == -2 &&
      get_effect_camera_matrix(&g_effect, &invalid_time, &matrix, &distance,
      &width, &height) != 0 && std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0;
  reset_pf_state_effect_lifetime(&g_effect, false);
  ok = ok && get_effect_camera_matrix(&g_effect, &time, &matrix, &distance,
      &width, &height) != 0 && std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0;

  g_full_resolution_width = saved_width;
  g_full_resolution_height = saved_height;
  reset_pf_state_effect_lifetime(&g_effect, saved_effect_live);
  return ok;
}

bool verify_aegp_resizer_3d_chain() {
  const void* layer_suite = nullptr;
  const void* stream_suite = nullptr;
  const void* comp_suite = nullptr;
  const void* item_suite = nullptr;
  const int32_t saved_camera_index = g_aegp_active_camera_layer_index;
  const int32_t saved_width = g_full_resolution_width;
  const int32_t saved_height = g_full_resolution_height;
  const auto saved_in_points = g_aegp_layer_in_points;
  const auto saved_durations = g_aegp_layer_durations;
  g_aegp_active_camera_layer_index = 2;
  g_aegp_layer_in_points[2] = {30, 30};
  g_aegp_layer_durations[2] = {300, 30};
  g_full_resolution_width = 1920;
  g_full_resolution_height = 1080;
  const AegpTime time{45, 30};

  bool ok = acquire_suite("AEGP Layer Suite", 14, &layer_suite) == 0 &&
      layer_suite == g_aegp_layer_suite8.data() &&
      g_aegp_layer_suite8[14] ==
          reinterpret_cast<void*>(&aegp_get_layer_current_time) &&
      g_aegp_layer_suite8[38] == reinterpret_cast<void*>(&aegp_get_layer_to_world_xform) &&
      acquire_suite("AEGP Stream Suite", 7, &stream_suite) == 0 &&
      stream_suite == g_aegp_stream_suite2.data() &&
      g_aegp_stream_suite2[16] == reinterpret_cast<void*>(&aegp_get_layer_stream_value_v2) &&
      acquire_suite("AEGP Comp Suite", 9, &comp_suite) == 0 &&
      comp_suite == g_aegp_comp_suite4.data() &&
      g_aegp_comp_suite4[1] == reinterpret_cast<void*>(&aegp_get_item_from_comp) &&
      acquire_suite("AEGP Item Suite", 10, &item_suite) == 0 &&
      item_suite == &g_aegp_legacy_item_suite6 &&
      reinterpret_cast<void**>(&g_aegp_legacy_item_suite6)[16] ==
          reinterpret_cast<void*>(&aegp_get_item_dimensions);
  using GetLayerCurrentTime =
      int32_t(__cdecl*)(void*, int16_t, AegpTime*);
  auto get_layer_current_time =
      reinterpret_cast<GetLayerCurrentTime>(g_aegp_layer_suite8[14]);
  const int32_t saved_scene_frame = g_aegp_scene_frame;
  g_aegp_scene_frame = 45;
  AegpTime current_time{-1, 1};
  ok = ok && get_layer_current_time(&g_aegp_layers[2], 1, &current_time) == 0 &&
      current_time.value == 45 && current_time.scale == 30;
  current_time = {-1, 1};
  ok = ok && get_layer_current_time(&g_aegp_layers[2], 0, &current_time) == 0 &&
      current_time.value == 15 && current_time.scale == 30;
  const AegpTime current_time_sentinel{77, 99};
  current_time = current_time_sentinel;
  ok = ok && get_layer_current_time(&g_aegp_comp, 1, &current_time) != 0 &&
      current_time.value == current_time_sentinel.value &&
      current_time.scale == current_time_sentinel.scale &&
      get_layer_current_time(&g_aegp_layers[2], 2, &current_time) != 0 &&
      current_time.value == current_time_sentinel.value &&
      current_time.scale == current_time_sentinel.scale;
  g_aegp_scene_frame = saved_scene_frame;
  AegpMatrix4 matrix{};
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &time, &matrix) == 0;
  for (std::size_t row = 0; row < 4; ++row) {
    for (std::size_t column = 0; column < 4; ++column)
      ok = ok && matrix.mat[row][column] == (row == column ? 1.0 : 0.0);
  }
  AegpLegacyStreamVal zoom{-1.0};
  int32_t type = -1;
  ok = ok && aegp_get_layer_stream_value_v2(&g_aegp_layers[2], 11, 1,
      &time, 0, &zoom, &type) == 0 && zoom.one_d == 1920.0 && type == 5;
  void* item = nullptr;
  int32_t width = -1;
  int32_t height = -1;
  ok = ok && aegp_get_item_from_comp(&g_aegp_comp, &item) == 0 &&
      item == &g_aegp_comp_item &&
      aegp_get_item_dimensions(item, &width, &height) == 0 &&
      width == 1920 && height == 1080;

  AegpLegacyStreamVal sentinel{-2.0};
  type = -2;
  AegpTime invalid{45, 0};
  ok = ok && aegp_get_layer_stream_value_v2(&g_aegp_layers[1], 11, 1,
      &time, 0, &sentinel, &type) != 0 && sentinel.one_d == -2.0 && type == -2 &&
      aegp_get_layer_stream_value_v2(&g_aegp_layers[2], 10, 1,
      &time, 0, &sentinel, &type) != 0 && sentinel.one_d == -2.0 && type == -2;
  AegpMatrix4 matrix_sentinel{};
  std::memset(&matrix_sentinel, 0x5a, sizeof(matrix_sentinel));
  matrix = matrix_sentinel;
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &invalid, &matrix) != 0 &&
      std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0;
  void* item_sentinel = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  width = -2;
  height = -3;
  ok = ok && aegp_get_item_from_comp(&g_layer, &item_sentinel) != 0 &&
      item_sentinel == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234)) &&
      aegp_get_item_dimensions(&g_layer, &width, &height) != 0 &&
      width == -2 && height == -3;

  ok = release_suite("AEGP Item Suite", 10) == 0 && ok;
  ok = release_suite("AEGP Comp Suite", 9) == 0 && ok;
  ok = release_suite("AEGP Stream Suite", 7) == 0 && ok;
  ok = release_suite("AEGP Layer Suite", 14) == 0 && ok;
  g_aegp_active_camera_layer_index = saved_camera_index;
  g_full_resolution_width = saved_width;
  g_full_resolution_height = saved_height;
  g_aegp_layer_in_points = saved_in_points;
  g_aegp_layer_durations = saved_durations;
  return ok && suite_leases_balanced();
}

bool verify_aegp_get_effect_camera() {
  const void* suite = nullptr;
  bool ok = acquire_suite("AEGP PF Interface Suite", 1, &suite) == 0 &&
      suite == &g_pf_interface_suite &&
      g_pf_interface_suite.get_effect_camera == &get_effect_camera &&
      g_pf_interface_suite.get_effect_camera_matrix == &get_effect_camera_matrix &&
      verify_aegp_get_effect_camera_case(false) &&
      verify_aegp_get_effect_camera_case(true) &&
      verify_aegp_get_effect_camera_matrix_case(false) &&
      verify_aegp_get_effect_camera_matrix_case(true);
  ok = release_suite("AEGP PF Interface Suite", 1) == 0 && ok;
  return ok && suite_leases_balanced();
}

bool verify_aegp_apply_effect() {
  const auto saved_instances = g_aegp_effect_instances;
  const auto saved_leases = g_aegp_effect_leases;
  const bool saved_live = g_aegp_effect_live;
  const bool saved_mode = g_aegp_comp_idle_roundtrip_mode;
  const uint32_t timestamp_before = g_render_project_timestamp.load();
  g_aegp_effect_instances = {};
  g_aegp_effect_instances[0] = {
      &g_aegp_layers[0], kAegpInstalledEffects[0].key, 0, 1, 1, true};
  g_aegp_effect_leases = {};
  g_aegp_effect_live = false;
  g_aegp_comp_idle_roundtrip_mode = true;

  const void* suite2 = nullptr;
  const void* suite3 = nullptr;
  const void* suite4 = nullptr;
  bool ok = acquire_suite("AEGP Effect Suite", 2, &suite2) == 0 &&
      acquire_suite("AEGP Effect Suite", 3, &suite3) == 0 &&
      acquire_suite("AEGP Effect Suite", 4, &suite4) == 0;
  if (suite2 && suite3 && suite4) {
    ok = ok && static_cast<void* const*>(const_cast<void*>(suite2))[9] ==
                   reinterpret_cast<void*>(&aegp_apply_effect) &&
        static_cast<void* const*>(const_cast<void*>(suite3))[9] ==
                   reinterpret_cast<void*>(&aegp_apply_effect) &&
        static_cast<void* const*>(const_cast<void*>(suite4))[9] ==
                   reinterpret_cast<void*>(&aegp_apply_effect);
  }

  int32_t layer0_count = -1;
  int32_t layer1_count = -1;
  void* applied = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && aegp_get_layer_num_effects(&g_aegp_layers[0], &layer0_count) == 0 &&
      layer0_count == 1 && aegp_get_layer_num_effects(&g_aegp_layers[1], &layer1_count) == 0 &&
      layer1_count == 0 && aegp_apply_effect(7, &g_aegp_layers[1],
          kAegpInstalledEffects[0].key, &applied) == 0 && applied &&
      applied != reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && aegp_get_layer_num_effects(&g_aegp_layers[1], &layer1_count) == 0 &&
      layer1_count == 1;

  void* stream = nullptr;
  ok = ok && aegp_get_new_effect_stream_by_index(7, applied, 1, &stream) == 0 && stream &&
      g_aegp_transform_stream.layer == &g_aegp_layers[1] &&
      g_aegp_transform_stream.effect_instance_index == 1 &&
      g_aegp_transform_stream.effect_instance_generation ==
          g_aegp_effect_instances[1].generation &&
      aegp_dispose_stream(stream) == 0;
  void* unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678));
  const auto scene_before_failures = g_aegp_effect_instances;
  const uint32_t timestamp_after_apply = g_render_project_timestamp.load();
  ok = ok && aegp_get_new_effect_stream_by_index(8, applied, 1, &unchanged) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678)) &&
      aegp_apply_effect(7, nullptr, kAegpInstalledEffects[0].key, &unchanged) == 4 &&
      aegp_apply_effect(7, &g_aegp_layers[1], 9999, &unchanged) == 4 &&
      aegp_apply_effect(7, &g_aegp_layers[1], kAegpInstalledEffects[0].key, nullptr) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678)) &&
      std::memcmp(g_aegp_effect_instances.data(), scene_before_failures.data(),
                  sizeof(g_aegp_effect_instances)) == 0 &&
      g_render_project_timestamp.load() == timestamp_after_apply;

  std::array<void*, kAegpEffectInstanceCapacity - 2> more{};
  for (std::size_t i = 0; ok && i < more.size(); ++i)
    ok = aegp_apply_effect(7, &g_aegp_layers[2], kAegpInstalledEffects[0].key,
                          &more[i]) == 0;
  unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x9abc));
  const auto full_scene = g_aegp_effect_instances;
  ok = ok && aegp_apply_effect(7, &g_aegp_layers[2], kAegpInstalledEffects[0].key,
                              &unchanged) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x9abc)) &&
      std::memcmp(g_aegp_effect_instances.data(), full_scene.data(),
                  sizeof(g_aegp_effect_instances)) == 0;

  const void* stale = applied;
  int32_t key = -1;
  ok = ok && aegp_dispose_effect(applied) == 0 &&
      aegp_get_installed_key_from_layer_effect(const_cast<void*>(stale), &key) == 4 &&
      aegp_dispose_effect(const_cast<void*>(stale)) == 4 &&
      aegp_dispose_effect(&g_aegp_effect) == 4;
  void* reacquired = nullptr;
  ok = ok && aegp_get_layer_effect_by_index(7, &g_aegp_layers[1], 0, &reacquired) == 0 &&
      reacquired != stale && aegp_get_installed_key_from_layer_effect(reacquired, &key) == 0 &&
      key == kAegpInstalledEffects[0].key && aegp_dispose_effect(reacquired) == 0;
  for (void* lease : more) if (lease) ok = aegp_dispose_effect(lease) == 0 && ok;

  ok = release_suite("AEGP Effect Suite", 4) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 3) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 2) == 0 && ok;
  g_aegp_effect_instances = saved_instances;
  g_aegp_effect_leases = saved_leases;
  g_aegp_effect_live = saved_live;
  g_aegp_comp_idle_roundtrip_mode = saved_mode;
  return ok && timestamp_before != timestamp_after_apply && suite_leases_balanced();
}

bool verify_aegp_effect_stack() {
  const auto saved_instances = g_aegp_effect_instances;
  const auto saved_leases = g_aegp_effect_leases;
  const auto saved_streams = g_aegp_legacy_effect_streams;
  const bool saved_mode = g_aegp_comp_idle_roundtrip_mode;
  const std::size_t saved_param_count = g_active_ui_param_count;
  g_aegp_effect_instances = {};
  g_aegp_effect_instances[0] = {
      &g_aegp_layers[0], kAegpInstalledEffects[0].key, 0, 1, 1, true};
  g_aegp_effect_leases = {};
  g_aegp_legacy_effect_streams = {};
  g_aegp_comp_idle_roundtrip_mode = true;
  g_active_ui_param_count = 5;

  const void* suite2 = nullptr;
  const void* suite3 = nullptr;
  const void* suite4 = nullptr;
  const void* stream_suite2 = nullptr;
  bool ok = acquire_suite("AEGP Effect Suite", 2, &suite2) == 0 &&
      acquire_suite("AEGP Effect Suite", 3, &suite3) == 0 &&
      acquire_suite("AEGP Effect Suite", 4, &suite4) == 0 &&
      acquire_suite("AEGP Stream Suite", 7, &stream_suite2) == 0;
  for (const void* suite : {suite2, suite3, suite4}) {
    if (!suite) { ok = false; continue; }
    const auto* slots = static_cast<void* const*>(const_cast<void*>(suite));
    ok = ok && slots[5] == reinterpret_cast<void*>(&aegp_set_effect_flags) &&
        slots[6] == reinterpret_cast<void*>(&aegp_reorder_effect) &&
        slots[10] == reinterpret_cast<void*>(&aegp_delete_layer_effect) &&
        slots[16] == reinterpret_cast<void*>(&aegp_duplicate_effect);
  }

  void* first = nullptr;
  void* second = nullptr;
  void* duplicate = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && aegp_apply_effect(7, &g_aegp_layers[1],
                 kAegpInstalledEffects[0].key, &first) == 0 &&
      aegp_apply_effect(7, &g_aegp_layers[1],
                 kAegpInstalledEffects[0].key, &second) == 0 &&
      aegp_duplicate_effect(first, &duplicate) == 0 && duplicate &&
      duplicate != reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  std::size_t first_index = 0;
  std::size_t second_index = 0;
  std::size_t duplicate_index = 0;
  ok = ok && resolve_effect_instance(first, 7, &first_index) &&
      resolve_effect_instance(second, 7, &second_index) &&
      resolve_effect_instance(duplicate, 7, &duplicate_index) &&
      g_aegp_effect_instances[first_index].stack_order == 0 &&
      g_aegp_effect_instances[duplicate_index].stack_order == 1 &&
      g_aegp_effect_instances[second_index].stack_order == 2;

  uint32_t flags = 0;
  ok = ok && aegp_set_effect_flags(duplicate, 3, 2) == 0 &&
      aegp_get_effect_flags(duplicate, &flags) == 0 && flags == 2 &&
      aegp_reorder_effect(duplicate, 2) == 0 &&
      g_aegp_effect_instances[second_index].stack_order == 1 &&
      g_aegp_effect_instances[duplicate_index].stack_order == 2;

  void* stream = nullptr;
  AegpTime time{0, 30};
  AegpStreamValue value{};
  char name[64]{};
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, duplicate, 1, &stream) == 0 &&
      aegp_get_stream_name_v2(stream, 1, name) == 0 &&
      std::strcmp(name, "Amount") == 0 &&
      aegp_get_new_stream_value_v2(8, stream, 1, &time, 0, &value) == 4 &&
      aegp_get_new_stream_value_v2(7, stream, 1, &time, 0, &value) == 0;
  double changed = 62.745098;
  std::memcpy(value.value.data(), &changed, sizeof(changed));
  ok = ok && aegp_set_stream_value_v2(7, stream, &value) == 0 &&
      aegp_dispose_stream_value_v2(&value) == 0 &&
      std::abs(g_aegp_effect_instances[duplicate_index].parameter_values[0][0] -
               changed) < 0.000001 &&
      aegp_delete_layer_effect(duplicate) == 0;
  int32_t type = -1;
  int32_t count = -1;
  ok = ok && aegp_get_stream_type_v2(stream, &type) == 4 &&
      aegp_get_layer_num_effects(&g_aegp_layers[1], &count) == 0 && count == 2 &&
      aegp_dispose_stream_v2(stream) == 0 &&
      aegp_get_effect_flags(duplicate, &flags) == 4 &&
      aegp_dispose_effect(duplicate) == 0;

  const auto snapshot = g_aegp_effect_instances;
  void* unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678));
  ok = ok && aegp_reorder_effect(first, 2) == 4 &&
      aegp_set_effect_flags(first, 1, 2) == 4 &&
      aegp_duplicate_effect(nullptr, &unchanged) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678)) &&
      std::memcmp(snapshot.data(), g_aegp_effect_instances.data(), sizeof(snapshot)) == 0;

  ok = aegp_dispose_effect(first) == 0 && ok;
  ok = aegp_dispose_effect(second) == 0 && ok;
  ok = release_suite("AEGP Stream Suite", 7) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 4) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 3) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 2) == 0 && ok;
  g_aegp_effect_instances = saved_instances;
  g_aegp_effect_leases = saved_leases;
  g_aegp_legacy_effect_streams = saved_streams;
  g_aegp_comp_idle_roundtrip_mode = saved_mode;
  g_active_ui_param_count = saved_param_count;
  return ok && suite_leases_balanced();
}

bool verify_aegp_projector_levels() {
  const auto saved_instances = g_aegp_effect_instances;
  const auto saved_leases = g_aegp_effect_leases;
  const auto saved_streams = g_aegp_legacy_effect_streams;
  const bool saved_mode = g_aegp_comp_idle_roundtrip_mode;
  const uint32_t stream_acquires_before = g_aegp_stream_acquires;
  const uint32_t stream_disposes_before = g_aegp_stream_disposes;
  const uint32_t value_acquires_before = g_aegp_stream_value_acquires;
  const uint32_t value_disposes_before = g_aegp_stream_value_disposes;
  g_aegp_effect_instances = {};
  g_aegp_effect_leases = {};
  g_aegp_legacy_effect_streams = {};
  g_aegp_comp_idle_roundtrip_mode = true;

  bool ok = true;
  int32_t count = -1;
  int32_t key = kAegpInstalledEffectKeyNone;
  int32_t easy_levels_key = kAegpInstalledEffectKeyNone;
  std::array<char, kAegpMaxEffectCategoryNameSize> match_name{};
  ok = aegp_get_num_installed_effects(&count) == 0 && count == 3;
  for (int32_t index = 0; ok && index < count; ++index) {
    ok = aegp_get_next_installed_effect(key, &key) == 0 && key != 0 &&
         aegp_get_effect_match_name(key, match_name.data()) == 0;
    if (ok && std::strcmp(match_name.data(), "ADBE Easy Levels") == 0)
      easy_levels_key = key;
  }
  int32_t end_key = -1;
  ok = ok && easy_levels_key != 0 &&
       aegp_get_next_installed_effect(key, &end_key) == 0 && end_key == 0;

  void* effect = nullptr;
  ok = ok && aegp_apply_effect(7, &g_aegp_layers[1], easy_levels_key, &effect) == 0;
  ok = ok && aegp_get_effect_num_param_streams_v2(effect, &count) == 0 && count == 7;
  void* input = nullptr;
  void* black = nullptr;
  void* white = nullptr;
  void* black_duplicate = nullptr;
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, effect, 0, &input) == 0 &&
       aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &black) == 0 &&
       aegp_get_new_effect_stream_by_index_v2(7, effect, 5, &white) == 0 &&
       aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &black_duplicate) == 0 &&
       input != black && black != white && black != black_duplicate;

  char name[64]{};
  int32_t type = -1;
  AegpTime time{0, 30};
  AegpStreamValue input_value{}, black_value{}, white_value{}, duplicate_value{};
  ok = ok && aegp_get_stream_name_v2(input, 1, name) == 0 &&
       std::strcmp(name, "Input") == 0 && aegp_get_stream_type_v2(input, &type) == 0 &&
       type == 9 && aegp_get_new_stream_value_v2(7, input, 1, &time, 1, &input_value) == 0;
  void* input_layer = nullptr;
  std::memcpy(&input_layer, input_value.value.data(), sizeof(input_layer));
  ok = ok && input_layer == &g_aegp_layers[1] &&
       aegp_set_stream_value_v2(7, input, &input_value) == 4;

  ok = ok && aegp_get_stream_name_v2(black, 1, name) == 0 &&
       std::strcmp(name, "Input Black") == 0 && aegp_get_stream_type_v2(black, &type) == 0 &&
       type == 5 && aegp_get_new_stream_value_v2(7, black, 1, &time, 1, &black_value) == 0 &&
       aegp_get_stream_name_v2(white, 1, name) == 0 &&
       std::strcmp(name, "Input White") == 0 && aegp_get_stream_type_v2(white, &type) == 0 &&
       type == 5 && aegp_get_new_stream_value_v2(7, white, 1, &time, 1, &white_value) == 0 &&
       aegp_get_new_stream_value_v2(7, black_duplicate, 1, &time, 1, &duplicate_value) == 0;
  double black_default = -1.0, white_default = -1.0, duplicate_default = -1.0;
  std::memcpy(&black_default, black_value.value.data(), sizeof(double));
  std::memcpy(&white_default, white_value.value.data(), sizeof(double));
  std::memcpy(&duplicate_default, duplicate_value.value.data(), sizeof(double));
  const double black_changed = .62745098;
  const double white_changed = .92156862745;
  std::memcpy(black_value.value.data(), &black_changed, sizeof(double));
  std::memcpy(white_value.value.data(), &white_changed, sizeof(double));
  ok = ok && black_default == 0.0 && white_default == 1.0 && duplicate_default == 0.0 &&
       aegp_set_stream_value_v2(8, black, &black_value) == 4 &&
       aegp_set_stream_value_v2(7, black, &black_value) == 0 &&
       aegp_set_stream_value_v2(7, white, &white_value) == 0;

  const AegpStreamValue stale_black_value = black_value;
  ok = ok && aegp_dispose_stream_value_v2(&duplicate_value) == 0 &&
       aegp_dispose_stream_value_v2(&white_value) == 0 &&
       aegp_dispose_stream_value_v2(&black_value) == 0;
  AegpStreamValue black_readback{}, white_readback{};
  ok = ok && aegp_get_new_stream_value_v2(7, black_duplicate, 1, &time, 1,
                                           &black_readback) == 0 &&
       aegp_get_new_stream_value_v2(7, white, 1, &time, 1, &white_readback) == 0;
  double black_actual = 0.0, white_actual = 0.0;
  std::memcpy(&black_actual, black_readback.value.data(), sizeof(double));
  std::memcpy(&white_actual, white_readback.value.data(), sizeof(double));
  ok = ok && std::abs(black_actual - black_changed) < 1e-12 &&
       std::abs(white_actual - white_changed) < 1e-12 &&
       aegp_dispose_stream_value_v2(&white_readback) == 0 &&
       aegp_dispose_stream_value_v2(&black_readback) == 0 &&
       aegp_dispose_stream_value_v2(&input_value) == 0;
  AegpStreamValue current_black_value{};
  AegpStreamValue stale_copy = stale_black_value;
  ok = ok && aegp_get_new_stream_value_v2(7, black, 1, &time, 1,
                                           &current_black_value) == 0 &&
       aegp_dispose_stream_value_v2(&stale_copy) == 4 &&
       stale_copy.stream == black &&
       aegp_dispose_stream_value_v2(&current_black_value) == 0;

  std::array<void*, kAegpLegacyEffectStreamCapacity - 4> capacity_streams{};
  for (std::size_t index = 0; ok && index < capacity_streams.size(); ++index)
    ok = aegp_get_new_effect_stream_by_index_v2(7, effect, 4,
                                                &capacity_streams[index]) == 0;
  void* unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &unchanged) == 4 &&
       unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  for (auto iterator = capacity_streams.rbegin(); iterator != capacity_streams.rend(); ++iterator)
    if (*iterator) ok = aegp_dispose_stream_v2(*iterator) == 0 && ok;
  ok = aegp_dispose_stream_v2(black_duplicate) == 0 && ok;
  ok = aegp_dispose_stream_v2(white) == 0 && ok;
  ok = aegp_dispose_stream_v2(black) == 0 && ok;
  ok = aegp_dispose_stream_v2(input) == 0 && ok;

  void* reused_stream = nullptr;
  type = 0x12345678;
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &reused_stream) == 0 &&
       reused_stream != black && aegp_get_stream_type_v2(black, &type) == 4 &&
       type == 0x12345678 && aegp_get_stream_type_v2(reused_stream, &type) == 0 &&
       type == 5 && aegp_dispose_stream_v2(reused_stream) == 0;

  void* stale_stream = nullptr;
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &stale_stream) == 0 &&
       aegp_delete_layer_effect(effect) == 0;
  type = 0x12345678;
  ok = ok && aegp_get_stream_type_v2(stale_stream, &type) == 4 &&
       type == 0x12345678 && aegp_dispose_stream_v2(stale_stream) == 0 &&
       aegp_dispose_effect(effect) == 0;

  ok = ok && g_aegp_stream_acquires - stream_acquires_before ==
                 g_aegp_stream_disposes - stream_disposes_before &&
       g_aegp_stream_value_acquires - value_acquires_before ==
                 g_aegp_stream_value_disposes - value_disposes_before;
  g_aegp_effect_instances = saved_instances;
  g_aegp_effect_leases = saved_leases;
  g_aegp_legacy_effect_streams = saved_streams;
  g_aegp_comp_idle_roundtrip_mode = saved_mode;
  return ok;
}

