// Keep behavioral checks active in Release without conflicting /D and /U flags.
#ifdef NDEBUG
#undef NDEBUG
#endif

#include "worker_cluster_effect_state.hpp"
#include "worker_compute_cache_suite.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_mask_runtime.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_parameter_runtime.hpp"
#include "worker_pf_state_runtime.hpp"
#include "worker_suite_registry.hpp"
#include "worker_ui_event_execution.hpp"
#include "worker_world_registry.hpp"

#include <cassert>
#include <iostream>
#include <tuple>
#include <type_traits>

namespace aexcompat::l2_detail {
bool configure_mask_scene(const std::string&);
}

namespace {
namespace wr = aexcompat::worker_runtime;
namespace reset = wr::cluster_effect_state;
namespace ui = wr::ui_event_execution;
namespace pf = aexcompat::pf_state_runtime;
namespace cache = aexcompat::compute_cache;
namespace worlds = aexcompat::world_registry;
namespace masks = aexcompat::l2_detail;

// Explicit field inventories compare values, not object representations (which
// contain padding). Every field in these tuples is dirtied before each swap,
// including nonzero defaults, pointers, arrays and variable-length telemetry.
template <class T> auto arbitrary_fields(T& s) {
  return std::tie(s.copy_calls, s.dispose_calls, s.invalid_operations,
      s.print_calls, s.print_failures, s.roundtrip_calls, s.roundtrip_failures,
      s.scan_calls, s.scan_failures, s.compare_disagreements, s.new_calls,
      s.interpolation_calls, s.interpolation_failures, s.last_interpolation_amount);
}
template <class T> auto ui_fields(T& s) {
  return std::tie(s.options_button_name, s.options_button_name_calls,
      s.update_advertised, s.dynamic_flags_advertised,
      s.conditional_selectors_dispatched, s.update_error, s.dynamic_flags_error,
      s.active_params, s.active_param_count, s.update_active, s.user_changed_active,
      s.update_calls, s.user_changed_requested, s.user_changed_slot,
      s.user_changed_error);
}
template <class T> auto checkout_fields(T& s) {
  return std::tie(s.checkout_calls, s.checkin_calls, s.automatic_checkins,
      s.invalid_checkins, s.redundant_checkins, s.rejected_temporal,
      s.wide_time_allowed, s.current_time, s.current_time_scale, s.last_index,
      s.last_time, s.last_time_step, s.last_time_scale);
}
template <class T> auto registration_fields(T& s) {
  return std::tie(s.events, s.comp_width, s.comp_height, s.comp_alignment,
      s.layer_width, s.layer_height, s.layer_alignment, s.preview_width,
      s.preview_height, s.preview_alignment);
}
template <class T> auto custom_ui_fields(T& s) {
  return std::tuple_cat(registration_fields(s.registration),
      std::tie(s.register_ui_calls, s.invalid_custom_ui_registrations,
      s.render_ui_context_active, s.drawbot_objects_created,
      s.drawbot_objects_released, s.drawbot_paint_rect_calls,
      s.drawbot_fill_path_calls, s.drawbot_stroke_path_calls,
      s.drawbot_invalid_operations, s.drawbot_get_supplier_calls,
      s.drawbot_get_surface_calls, s.drawbot_get_drawing_ref_calls,
      s.overlay_stroke_path_calls, s.app_get_background_color_calls,
      s.app_color_picker_calls, s.app_invalidate_rect_calls,
      s.app_progress_dialogs_created, s.app_progress_dialogs_disposed,
      s.app_picker_color, s.app_invalidated_rect, s.ui_drag_calls,
      s.ui_drag_requested, s.ui_drag_terminated, s.ui_coordinate_transform_calls,
      s.render_click_enabled, s.render_draw_enabled, s.render_click_x,
      s.render_click_y, s.render_click_error, s.render_click_out_flags,
      s.render_click_changed_value, s.render_draw_error, s.render_draw_out_flags,
      s.render_ui_lifecycle_errors, s.render_ui_context_closed,
      s.drawbot_fill_colors));
}

template <class T> void dirty(T& value) {
  if constexpr (std::is_same_v<T, bool>) value = !value;
  else if constexpr (std::is_arithmetic_v<T>) value += 7;
  else if constexpr (std::is_pointer_v<T>) {
    static void* identity{};
    value = reinterpret_cast<T>(&identity);  // Identity only, never dereferenced.
  } else value = "outgoing member";
}
template <class T, std::size_t N> void dirty(std::array<T, N>& values) {
  for (auto& value : values) dirty(value);
}
template <class T> void dirty(std::vector<T>& values) {
  values.emplace_back();
  dirty(values.back());
}
template <class... T> void dirty_fields(std::tuple<T&...> fields) {
  std::apply([](auto&... value) { (dirty(value), ...); }, fields);
}
template <class A, class B, std::size_t... I>
void assert_all_different(const A& fields, const B& fresh,
                          std::index_sequence<I...>) {
  assert(((std::get<I>(fields) != std::get<I>(fresh)) && ...));
}
template <class A, class B> void dirty_and_check(A fields, const B& fresh) {
  dirty_fields(fields);
  assert_all_different(fields, fresh,
      std::make_index_sequence<std::tuple_size_v<A>>{});
}

int owner;
int deleted_values{};
int32_t __cdecl generate_key(void* options, cache::AegpGuid* key) {
  *key = {{*static_cast<int*>(options), 0, 0, 0}};
  return 0;
}
int32_t __cdecl compute(void* options, void** value) {
  *value = options;
  return 0;
}
size_t __cdecl approx_size(void*) { return sizeof(int); }
void __cdecl delete_value(void*) { ++deleted_values; }
const cache::AegpComputeCacheCallbacks callbacks{
    generate_key, compute, approx_size, delete_value};

auto handle_counts() {
  const auto s = wr::handles::statistics();
  return std::make_tuple(s.created, s.disposed, s.locks, s.unlocks,
      s.locks_released_on_dispose, s.invalid_operations,
      s.automatic_pre_render_disposals, s.live_count, s.live_bytes,
      s.quarantined_count, s.quarantined_bytes);
}
auto world_counts() {
  const auto s = worlds::statistics();
  return std::make_tuple(s.created, s.disposed, s.invalid_operations,
      s.live_count, s.live_bytes, s.reclaimed, s.reclaimed_bytes);
}

void test_effect_reset() {
  auto& state = wr::parameters::state();
  auto& custom = ui::custom_ui_telemetry();
  const wr::parameters::State fresh;
  const ui::CustomUiTelemetry fresh_custom;

  // Live process resources deliberately span multiple simulated members. The
  // reset must neither dispose them nor conceal their outstanding balances.
  void** handle = wr::handles::new_handle(32);
  assert(handle && wr::handles::lock_handle(handle));
  aexcompat::world_safety::LocalEffectWorld world{};
  assert(worlds::new_world(&owner, 2, 2, 1, worlds::kPixelFormatArgb32, &world) == 0);
  auto& suites = wr::suite_registry();
  const void* table{};
  assert(suites.acquire("cluster-reset-selftest", 1, &table,
      +[](void*, const char*, int32_t, const void** out) {
        *out = &owner;
        return wr::SuiteResolveResult::acquired;
      }, nullptr, nullptr) == 0);
  const auto handles_before = handle_counts();
  const auto worlds_before = world_counts();
  const auto suites_before = suites.snapshot();

  pf::configure_host_hooks({+[]() -> void* { return &owner; },
      +[](int32_t index, bool) { return index == 1; },
      +[](int32_t, std::vector<unsigned char>& bytes) {
        bytes.push_back(42);
        return true;
      }});
  for (int member = 0; member < 3; ++member) {
    state.records.emplace_back();
    state.records.back().name = "old parameter";
    state.timelines.emplace_back();
    state.animation_layer_width = 1920;
    state.animation_layer_height = 1080;
    state.keyframe_checkout_ledger.emplace(&owner, wr::parameters::Definition{});
    state.user_changed_parameters.emplace_back();
    state.checkout.definitions.emplace(1, wr::parameters::Definition{});
    state.checkout.live.emplace(wr::ParamCheckoutKey{1, 2}, 3);
    state.checkout.completed.emplace(4, 5);
    dirty_and_check(arbitrary_fields(state.arbitrary), arbitrary_fields(fresh.arbitrary));
    dirty_and_check(ui_fields(state.ui), ui_fields(fresh.ui));
    dirty_and_check(checkout_fields(state.checkout), checkout_fields(fresh.checkout));
    dirty_and_check(custom_ui_fields(custom), custom_ui_fields(fresh_custom));
    ui::record_info_text("previous member info");
    ui::record_info_text("last info");
    assert(ui::snapshot_info_text_telemetry().calls == 2);
    assert(ui::snapshot_info_text_telemetry().last_text == "last info");

    pf::reset_effect_lifetime(true);
    pf::PfState token{};
    uint8_t same{};
    assert(pf::get_current_param_state(&owner, 1, nullptr, nullptr, &token) == 0);
    assert(pf::are_param_states_identical(&owner, &token, &token, &same) == 0 && same);
    assert(pf::pf_state_statistics().get_current_state_calls == 1);
    assert(pf::pf_state_statistics().are_states_identical_calls == 1);
    pf::on_global_setdown();  // Lifecycle invalidation precedes report reset.

    auto& compute_suite = cache::suite_table();
    void* receipt{};
    void* value{};
    assert(compute_suite.class_register("cluster-reset-cache", &callbacks) == 0);
    assert(compute_suite.compute_if_needed_and_checkout(
        "cluster-reset-cache", &member, 1, &receipt) == 0);
    assert(compute_suite.get_receipt_compute_value(receipt, &value) == 0 && value == &member);

    reset::reset();
    assert(state.records.empty() && state.timelines.empty());
    assert(state.animation_layer_width == fresh.animation_layer_width);
    assert(state.animation_layer_height == fresh.animation_layer_height);
    assert(state.keyframe_checkout_ledger.empty() && state.user_changed_parameters.empty());
    assert(state.checkout.definitions.empty() && state.checkout.live.empty());
    assert(state.checkout.completed.empty());
    assert(arbitrary_fields(state.arbitrary) == arbitrary_fields(fresh.arbitrary));
    assert(ui_fields(state.ui) == ui_fields(fresh.ui));
    assert(checkout_fields(state.checkout) == checkout_fields(fresh.checkout));
    assert(custom_ui_fields(custom) == custom_ui_fields(fresh_custom));
    const auto info = ui::snapshot_info_text_telemetry();
    assert(info.calls == 0 && info.last_text.empty());
    assert(pf::pf_state_statistics().get_current_state_calls == 0);
    assert(pf::pf_state_statistics().are_states_identical_calls == 0);
    assert(!pf::effect_is_live() && pf::live_state_count() == 0);
    pf::reset_effect_lifetime(true);
    assert(pf::are_param_states_identical(&owner, &token, &token, &same) != 0);
    pf::on_global_setdown();
    assert(deleted_values == member + 1);
    assert(compute_suite.get_receipt_compute_value(receipt, &value) != 0);
    assert(compute_suite.checkin_compute_receipt(receipt) != 0);
    assert(compute_suite.class_unregister("cluster-reset-cache") != 0);
    // Re-registering the same class must not recover its previous cached value.
    assert(compute_suite.class_register("cluster-reset-cache", &callbacks) == 0);
    assert(compute_suite.checkout_cached("cluster-reset-cache", &member, &receipt) != 0);
    reset::reset();  // Idempotent, with no cached value to delete twice.
    assert(deleted_values == member + 1);

    assert(handle_counts() == handles_before && world_counts() == worlds_before);
    const auto suites_after = suites.snapshot();
    assert(suites_after.acquires == suites_before.acquires);
    assert(suites_after.releases == suites_before.releases);
    assert(suites_after.live_leases == suites_before.live_leases);
    assert(!wr::handles::handle_lifetimes_balanced());
    assert(!worlds::lifetimes_balanced() && !suites.balanced());
    assert(wr::handles::host_handle_is_live(handle) && worlds::owns_world(&world));
  }
  wr::handles::unlock_handle(handle);
  wr::handles::dispose_handle(handle);
  assert(worlds::dispose_world(&owner, &world) == 0);
  assert(suites.release("cluster-reset-selftest", 1, nullptr) == 0);
  assert(wr::handles::handle_lifetimes_balanced());
  assert(worlds::lifetimes_balanced() && suites.balanced());
  pf::configure_host_hooks({});
}

void test_mask_reset() {
  assert(masks::configure_mask_scene("rectangle"));
  assert(masks::active_mask_count() > 0);
  const auto next_mask = masks::g_next_mask_id;
  const auto next_stream = masks::g_next_stream_id;
  // Ordinary effect-state reset must leave masks available to the separately
  // ordered discovery boundary; render sessions own their scene separately.
  reset::reset();
  assert(masks::active_mask_count() > 0);
  masks::g_stream_refs.emplace_back();
  assert(!reset::reset_member_mask_scene());
  assert(masks::active_mask_count() > 0);
  masks::g_stream_refs.clear();
  masks::g_stream_values.emplace(nullptr, masks::CheckedStreamValue{});
  assert(!reset::reset_member_mask_scene());
  assert(masks::active_mask_count() > 0);
  masks::g_stream_values.clear();
  masks::g_add_keyframe_transactions.emplace_back();
  assert(!reset::reset_member_mask_scene());
  assert(masks::active_mask_count() > 0);
  masks::g_add_keyframe_transactions.clear();
  assert(reset::reset_member_mask_scene());
  assert(masks::active_mask_count() == 0);
  assert(aexcompat::mask_runtime::mask_scene_id() == "empty");
  assert(masks::g_next_mask_id == next_mask && masks::g_next_stream_id == next_stream);
  assert(reset::reset_member_mask_scene());
  assert(masks::g_next_mask_id == next_mask && masks::g_next_stream_id == next_stream);
}
}  // namespace

int main() {
  test_effect_reset();
  test_mask_reset();
  std::cout << "worker_cluster_effect_state_selftest: PASS\n";
}
