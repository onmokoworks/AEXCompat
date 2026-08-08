#include "worker_aegp_compat_selftests.hpp"
#include "worker_parameter_runtime.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_mask_runtime.hpp"
#include "worker_aegp_scene.hpp"
#include "worker_aegp_scene_model.hpp"
#include "worker_aegp_scene_transaction.hpp"
#include "worker_aegp_scene_runtime.hpp"
#include "worker_aegp_external_render_runtime.hpp"
#include "worker_aegp_staged_item_runtime.hpp"
#include "worker_render_receipts.hpp"
#include "worker_suite_registry.hpp"
#include "worker_world_registry.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_pf_helper_runtime.hpp"
#include "worker_pf_state_runtime.hpp"
#include "render_subsystem.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <cstring>
#include <limits>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

namespace aexcompat::l2_detail {
bool configure_mask_scene(const std::string&);
bool prepare_scene_staged_item(void*);
namespace { AegpCompatSelftestHooks g_hooks; }

namespace {
int32_t compat_acquire_suite(const char* name, int32_t version, const void** suite) {
  return g_hooks.acquire_suite(name, version, suite);
}
int32_t compat_release_suite(const char* name, int32_t version) {
  return g_hooks.release_suite(name, version);
}
bool compat_suite_leases_balanced() { return g_hooks.suite_leases_balanced(); }

uint32_t unsupported_effect_slot_seven_count(const std::string& report) {
  constexpr char marker[] =
      "{\"name\":\"AEGP Effect Suite\",\"version\":4,"
      "\"slot\":7,\"call_count\":";
  const auto marker_offset = report.find(marker);
  if (marker_offset == std::string::npos) return 0;
  auto cursor = marker_offset + sizeof(marker) - 1;
  uint32_t count = 0;
  bool found_digit = false;
  while (cursor < report.size() &&
         report[cursor] >= '0' && report[cursor] <= '9') {
    found_digit = true;
    const uint32_t digit = static_cast<uint32_t>(report[cursor] - '0');
    if (count > (std::numeric_limits<uint32_t>::max() - digit) / 10u)
      return std::numeric_limits<uint32_t>::max();
    count = count * 10u + digit;
    ++cursor;
  }
  return found_digit ? count : 0;
}
}

#define g_aegp_layers scene_runtime_state().layers
#define g_aegp_comp_idle_roundtrip_mode (*g_hooks.comp_idle_roundtrip_mode)
#define g_active_ui_param_count (*g_hooks.active_ui_param_count)
#define g_aegp_stream_acquires scene_runtime_state().stream_acquires
#define g_aegp_stream_disposes scene_runtime_state().stream_disposes
#define g_aegp_stream_value_acquires scene_runtime_state().stream_value_acquires
#define g_aegp_stream_value_disposes scene_runtime_state().stream_value_disposes
#define g_aegp_active_camera_layer_index scene_runtime_state().active_camera_layer_index
#define aegp_get_new_effect_stream_by_index_v2 g_hooks.get_new_effect_stream_v2
#define aegp_get_stream_name_v2 g_hooks.get_stream_name_v2
#define aegp_get_stream_type_v2 g_hooks.get_stream_type_v2
#define aegp_get_new_stream_value_v2 g_hooks.get_new_stream_value_v2
#define aegp_set_stream_value_v2 g_hooks.set_stream_value_v2
#define aegp_dispose_stream_value_v2 g_hooks.dispose_stream_value_v2
#define aegp_dispose_stream_v2 g_hooks.dispose_stream_v2

void configure_aegp_compat_selftests(AegpCompatSelftestHooks hooks) { g_hooks = hooks; }

bool verify_legacy_effect_compat_suites() {
  if (!g_hooks.acquire_suite || !g_hooks.release_suite || !g_hooks.get_comp_bg_color ||
      !g_hooks.convert_effect_time || !g_hooks.comp || !g_hooks.effect) return false;
  const void* comp_suite = nullptr;
  const void* interface_suite = nullptr;
  const void* helper_suite = nullptr;
  bool ok = aexcompat::pf_helper::selftest() &&
      g_hooks.acquire_suite("AEGP Comp Suite", 21, &comp_suite) == 0 &&
      comp_suite == g_hooks.comp_suite &&
      g_hooks.acquire_suite("AEGP PF Interface Suite", 1, &interface_suite) == 0 &&
      interface_suite == g_hooks.interface_suite &&
      g_hooks.acquire_suite("AE Plugin Helper Suite", 1, &helper_suite) == 0 &&
      helper_suite == g_hooks.helper_suite;
  AegpCompatColor color{-1.0, -2.0, -3.0, -4.0};
  const AegpCompatColor color_sentinel = color;
  ok = ok && g_hooks.get_comp_bg_color(g_hooks.comp, &color) == 0 &&
      color.alpha == 1.0 && color.red == 0.0 && color.green == 0.0 && color.blue == 0.0;
  color = color_sentinel;
  ok = ok && g_hooks.get_comp_bg_color(nullptr, &color) != 0 &&
      std::memcmp(&color, &color_sentinel, sizeof(color)) == 0 &&
      g_hooks.get_comp_bg_color(g_hooks.comp_item, &color) != 0 &&
      std::memcmp(&color, &color_sentinel, sizeof(color)) == 0 &&
      g_hooks.get_comp_bg_color(g_hooks.comp, nullptr) != 0;
  suite_abi::AegpTime time{77, 99};
  const auto sentinel = time;
  ok = ok && g_hooks.convert_effect_time(g_hooks.effect, -17, 24000, &time) == 0 &&
      time.value == -17 && time.scale == 24000;
  time = sentinel;
  ok = ok && g_hooks.convert_effect_time(nullptr, 1, 30, &time) != 0 &&
      time.value == sentinel.value && time.scale == sentinel.scale &&
      g_hooks.convert_effect_time(g_hooks.effect, 1, 0, &time) != 0 &&
      time.value == sentinel.value && time.scale == sentinel.scale &&
      g_hooks.convert_effect_time(g_hooks.effect,
          (std::numeric_limits<int32_t>::min)(),
          (std::numeric_limits<uint32_t>::max)(), &time) == 0 &&
      time.value == (std::numeric_limits<int32_t>::min)() &&
      time.scale == (std::numeric_limits<uint32_t>::max)() &&
      g_hooks.convert_effect_time(g_hooks.effect, 0, 1, nullptr) != 0;
  aexcompat::pf_helper::set_effect_tool_for_test(14);
  int32_t tool = -1;
  ok = ok && aexcompat::pf_helper::get_current_tool(&tool) == 0 && tool == 0 &&
      aexcompat::pf_helper::get_current_tool(nullptr) == 516 &&
      aexcompat::pf_helper::effect_tool_for_test() == 14;
  aexcompat::pf_helper::reset();
  ok = g_hooks.release_suite("AE Plugin Helper Suite", 1) == 0 && ok;
  ok = g_hooks.release_suite("AEGP PF Interface Suite", 1) == 0 && ok;
  ok = g_hooks.release_suite("AEGP Comp Suite", 21) == 0 && ok;
  return ok;
}

bool verify_aegp_layer_source_item() {
  if (!g_hooks.get_layer_source_item || !g_hooks.get_item_type ||
      !g_hooks.comp_item || !g_hooks.item_suite ||
      !g_hooks.layer_source_item_calls || !g_hooks.item_type_calls) return false;
  const uint32_t calls_before = *g_hooks.layer_source_item_calls;
  std::array<void*, 3> source_items{};
  void* item = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  const bool pf_ok =
      g_hooks.get_layer_source_item(g_hooks.pf_layer, &item) == 0 &&
      item != nullptr;
  bool ok = pf_ok;
  const void* pf_source_item = item;
  std::array<bool, 3> layer_ok{};
  for (std::size_t index = 0; index < g_aegp_layers.size(); ++index) {
    layer_ok[index] = g_hooks.get_layer_source_item(
        &g_aegp_layers[index], &source_items[index]) == 0 &&
        source_items[index] != nullptr;
    ok = layer_ok[index] && ok;
  }
  const bool same_source = pf_source_item == source_items[0];
  ok = same_source && ok;
  scene_runtime::AegpSceneObject foreign{0x464f5245};
  item = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = g_hooks.get_layer_source_item(&foreign, &item) == 4 && ok &&
       item == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234)) &&
       g_hooks.get_layer_source_item(nullptr, &item) == 4 &&
       item == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234)) &&
       g_hooks.get_layer_source_item(g_hooks.pf_layer, nullptr) == 4 &&
       *g_hooks.layer_source_item_calls == calls_before + 4;
  const uint32_t item_type_calls_before = *g_hooks.item_type_calls;
  const bool saved_comp_idle_mode = g_aegp_comp_idle_roundtrip_mode;
  g_aegp_comp_idle_roundtrip_mode = true;
  const void* acquired{};
  ok = ok && compat_acquire_suite("AEGP Item Suite", 14, &acquired) == 0 &&
       acquired == g_hooks.item_suite;
  int16_t item_type = -1;
  ok = ok && g_hooks.get_item_type(g_hooks.comp_item, &item_type) == 0 &&
       item_type == 2;
  scene_runtime::AegpSceneObject foreign_item{0x464f5249};
  item_type = 0x1234;
  ok = ok && g_hooks.get_item_type(&foreign_item, &item_type) == 4 &&
       item_type == 0x1234 && g_hooks.get_item_type(nullptr, &item_type) == 4 &&
       item_type == 0x1234 && g_hooks.get_item_type(g_hooks.comp_item, nullptr) == 4 &&
       *g_hooks.item_type_calls == item_type_calls_before + 1;
  g_aegp_comp_idle_roundtrip_mode = saved_comp_idle_mode;
  return ok;
}

// `AEGP Layer Suite` version 5 is the SDK's `AEGP_LayerSuite1`, frozen in AE
// 5.0. Its slots are not the later table shifted by a constant: version 11
// (`AEGP_LayerSuite5`) inserted `AEGP_GetLayerSourceItemID` at 5 and
// `AEGP_ConvertLayerToCompTime` at 35, so a version 1 slot moves by +1 from 5
// through 33 and by +2 from 34 on. Wiring the later indices here would hand the
// plug-in a different function at every one of them (issue #712).
//
// The offset is pinned against version 14 rather than only against a literal
// list, and on both sides of the second insertion, so a wrong constant fails
// whichever direction it is wrong in. `AEGP_GetLayerName` is pinned as
// *unwired*: version 1 hands back two `A_char` buffers while
// `aegp_get_layer_name` implements the version 8 shape (a plug-in id and two
// `AEGP_MemHandle` outputs).
bool verify_aegp_layer_suite1_slots() {
  if (!g_hooks.acquire_suite || !g_hooks.release_suite ||
      !g_hooks.suite_leases_balanced)
    return false;
  const void* raw = nullptr;
  if (compat_acquire_suite("AEGP Layer Suite", 5, &raw) != 0) return false;
  // Everything past the acquire runs through this, so no exit path leaves the
  // lease held - a leaked lease would poison the balance check that other
  // self-tests in this process depend on.
  const auto finish = [](bool result) {
    const bool released = compat_release_suite("AEGP Layer Suite", 5) == 0;
    return result && released && g_hooks.suite_leases_balanced();
  };
  const auto* slots = static_cast<void* const*>(raw);
  if (!slots || raw != g_aegp_layer_suite1.data()) return finish(false);

  const std::array<std::pair<std::size_t, void*>, 17> wired{{
      {0, reinterpret_cast<void*>(&aegp_get_comp_num_layers)},
      {1, reinterpret_cast<void*>(&aegp_get_comp_layer_by_index)},
      {2, reinterpret_cast<void*>(&aegp_get_active_layer)},
      {3, reinterpret_cast<void*>(&aegp_get_layer_index)},
      {4, reinterpret_cast<void*>(&aegp_get_layer_source_item)},
      {5, reinterpret_cast<void*>(&aegp_get_layer_parent_comp)},
      {9, reinterpret_cast<void*>(&aegp_get_layer_flags)},
      {10, reinterpret_cast<void*>(&aegp_set_layer_flag)},
      {14, reinterpret_cast<void*>(&aegp_get_layer_in_point)},
      {15, reinterpret_cast<void*>(&aegp_get_layer_duration)},
      {16, reinterpret_cast<void*>(&aegp_set_layer_in_point_and_duration)},
      {21, reinterpret_cast<void*>(&aegp_get_layer_transfer_mode)},
      {26, reinterpret_cast<void*>(&aegp_get_layer_masked_bounds)},
      {27, reinterpret_cast<void*>(&aegp_get_layer_object_type)},
      {33, reinterpret_cast<void*>(&aegp_convert_comp_to_layer_time)},
      {35, reinterpret_cast<void*>(&aegp_get_layer_id)},
      {36, reinterpret_cast<void*>(&aegp_get_layer_to_world_xform)},
  }};
  bool ok = true;
  for (const auto& [slot, implementation] : wired)
    ok = ok && slots[slot] == implementation;

  // Every slot this table does not claim is exactly the stub, not merely
  // non-null: wiring a real function into the wrong slot has to fail.
  const auto& stubs = aexcompat::worker_runtime::unsupported_suite_slots<
      aexcompat::worker_runtime::UnsupportedSuiteId::aegp_layer_5, 39>();
  const auto claimed = [&wired](std::size_t slot) {
    for (const auto& entry : wired)
      if (entry.first == slot) return true;
    return false;
  };
  for (std::size_t slot = 0; slot < stubs.size(); ++slot)
    if (!claimed(slot)) ok = ok && slots[slot] == stubs[slot];
  // `AEGP_GetLayerName` is one of those, and is the one that must stay a stub
  // for a reason other than "not implemented yet".
  ok = ok && slots[6] != reinterpret_cast<void*>(&aegp_get_layer_name);

  // Both tables are filled at acquire time, so version 14 has to be acquired
  // for the comparison to mean anything. Four pairs are compared, straddling
  // the second insertion, so the +1 and the +2 are both pinned.
  const void* later_raw = nullptr;
  if (compat_acquire_suite("AEGP Layer Suite", 14, &later_raw) != 0)
    return finish(false);
  const auto* later = static_cast<void* const*>(later_raw);
  ok = ok && later && later_raw == g_aegp_layer_suite8.data();
  if (later) {
    // Only functions wired in both tables can be compared this way, which is
    // why `AEGP_GetLayerToWorldXform` carries the +2 rather than
    // `AEGP_GetLayerID`: version 14 leaves slot 37 on its stub.
    const std::array<std::pair<std::size_t, std::size_t>, 4> offsets{{
        {5, 6},    // +1, first slot after AEGP_GetLayerSourceItemID
        {10, 11},  // +1
        {21, 22},  // +1, last slot before AEGP_ConvertLayerToCompTime
        {36, 38},  // +2, after it
    }};
    for (const auto& [here, there] : offsets)
      ok = ok && slots[here] == later[there] && slots[here] != later[here];
  }
  ok = compat_release_suite("AEGP Layer Suite", 14) == 0 && ok;

  // Version 11 is `AEGP_LayerSuite5`. Its slot 7 is the legacy three-argument
  // fixed-buffer `AEGP_GetLayerName`, not the four-argument MemHandle form
  // implemented by `aegp_get_layer_name`. Pin it to the exact unsupported stub
  // so a future neighbouring-table copy cannot silently restore the ABI
  // mismatch (issue #718).
  const bool saved_mode = g_aegp_comp_idle_roundtrip_mode;
  g_aegp_comp_idle_roundtrip_mode = true;
  const void* version11_raw = nullptr;
  const bool version11_acquired =
      compat_acquire_suite("AEGP Layer Suite", 11, &version11_raw) == 0;
  if (version11_acquired) {
    const auto* version11 = static_cast<void* const*>(version11_raw);
    const auto& version11_stubs =
        aexcompat::worker_runtime::unsupported_suite_slots<
            aexcompat::worker_runtime::UnsupportedSuiteId::aegp_layer_11, 46>();
    ok = ok && version11 && version11_raw == g_aegp_layer_suite5.data() &&
        version11[7] == version11_stubs[7] &&
        version11[7] != reinterpret_cast<void*>(&aegp_get_layer_name);
    ok = compat_release_suite("AEGP Layer Suite", 11) == 0 && ok;
  } else {
    ok = false;
  }
  g_aegp_comp_idle_roundtrip_mode = saved_mode;

  const void* version8_raw = nullptr;
  const bool version8_acquired =
      compat_acquire_suite("AEGP Layer Suite", 8, &version8_raw) == 0;
  if (version8_acquired) {
    const auto* version8 = static_cast<void* const*>(version8_raw);
    const std::array<std::pair<std::size_t, void*>, 21> version8_wired{{
        {0, reinterpret_cast<void*>(&aegp_get_comp_num_layers)},
        {1, reinterpret_cast<void*>(&aegp_get_comp_layer_by_index)},
        {2, reinterpret_cast<void*>(&aegp_get_active_layer)},
        {3, reinterpret_cast<void*>(&aegp_get_layer_index)},
        {4, reinterpret_cast<void*>(&aegp_get_layer_source_item)},
        {5, reinterpret_cast<void*>(&aegp_get_layer_parent_comp)},
        {9, reinterpret_cast<void*>(&aegp_get_layer_flags)},
        {10, reinterpret_cast<void*>(&aegp_set_layer_flag)},
        {14, reinterpret_cast<void*>(&aegp_get_layer_in_point)},
        {15, reinterpret_cast<void*>(&aegp_get_layer_duration)},
        {16, reinterpret_cast<void*>(&aegp_set_layer_in_point_and_duration)},
        {21, reinterpret_cast<void*>(&aegp_get_layer_transfer_mode)},
        {26, reinterpret_cast<void*>(&aegp_get_layer_masked_bounds)},
        {27, reinterpret_cast<void*>(&aegp_get_layer_object_type)},
        {33, reinterpret_cast<void*>(&aegp_convert_comp_to_layer_time)},
        {34, reinterpret_cast<void*>(&aegp_convert_layer_to_comp_time)},
        {36, reinterpret_cast<void*>(&aegp_get_layer_id)},
        {37, reinterpret_cast<void*>(&aegp_get_layer_to_world_xform)},
        {40, reinterpret_cast<void*>(&aegp_get_layer_parent)},
        {41, reinterpret_cast<void*>(&aegp_set_layer_parent)},
        {42, reinterpret_cast<void*>(&aegp_delete_layer)},
    }};
    ok = ok && version8 && version8_raw == g_aegp_layer_suite3.data();
    if (version8) {
      for (const auto& [slot, implementation] : version8_wired)
        ok = ok && version8[slot] == implementation;
      const auto& version8_stubs =
          aexcompat::worker_runtime::unsupported_suite_slots<
              aexcompat::worker_runtime::UnsupportedSuiteId::aegp_layer_8, 43>();
      const auto version8_claimed = [&version8_wired](std::size_t slot) {
        for (const auto& entry : version8_wired)
          if (entry.first == slot) return true;
        return false;
      };
      for (std::size_t slot = 0; slot < version8_stubs.size(); ++slot)
        if (!version8_claimed(slot))
          ok = ok && version8[slot] == version8_stubs[slot];
      ok = ok && version8[6] != reinterpret_cast<void*>(&aegp_get_layer_name);
    }
    ok = compat_release_suite("AEGP Layer Suite", 8) == 0 && ok;
  } else {
    ok = false;
  }
  return finish(ok);
}

bool verify_aegp_scene_registry_suites() {
  if (!g_hooks.acquire_suite || !g_hooks.release_suite ||
      !g_hooks.comp_idle_roundtrip_mode ||
      !g_hooks.suite_leases_balanced)
    return false;
  using GetCompFromItem = int32_t (__cdecl*)(void*, void**);
  using GetLayerCount = int32_t (__cdecl*)(void*, int32_t*);
  using GetLayerByIndex = int32_t (__cdecl*)(void*, int32_t, void**);
  using GetLayerIndex = int32_t (__cdecl*)(void*, int32_t*);
  using GetLayerParentComp = int32_t (__cdecl*)(void*, void**);
  using GetLayerFromId = int32_t (__cdecl*)(void*, int32_t, void**);
  struct alignas(std::max_align_t) ForgedBorrowedToken {
    uint64_t lease_identity{};
  };

  const bool saved_mode = g_aegp_comp_idle_roundtrip_mode;
  g_aegp_comp_idle_roundtrip_mode = true;
  const void* item_suite_raw = nullptr;
  const void* comp_suite_raw = nullptr;
  const void* layer_suite_raw = nullptr;
  const bool item_acquired =
      compat_acquire_suite("AEGP Item Suite", 14, &item_suite_raw) == 0;
  const bool comp_acquired =
      compat_acquire_suite("AEGP Comp Suite", 25, &comp_suite_raw) == 0;
  const bool layer_acquired =
      compat_acquire_suite("AEGP Layer Suite", 14, &layer_suite_raw) == 0;
  bool ok = item_acquired && comp_acquired && layer_acquired &&
      item_suite_raw == &g_aegp_item_suite &&
      comp_suite_raw == g_aegp_comp_suite11.data() &&
      layer_suite_raw == g_aegp_layer_suite8.data();

  const auto* item_suite =
      static_cast<const AegpItemSuite*>(item_suite_raw);
  const auto* comp_slots = static_cast<void* const*>(comp_suite_raw);
  const auto* layer_slots = static_cast<void* const*>(layer_suite_raw);
  const auto get_comp_from_item = comp_slots
      ? reinterpret_cast<GetCompFromItem>(comp_slots[0]) : nullptr;
  const auto get_layer_count = layer_slots
      ? reinterpret_cast<GetLayerCount>(layer_slots[0]) : nullptr;
  const auto get_layer_by_index = layer_slots
      ? reinterpret_cast<GetLayerByIndex>(layer_slots[1]) : nullptr;
  const auto get_layer_index = layer_slots
      ? reinterpret_cast<GetLayerIndex>(layer_slots[3]) : nullptr;
  const auto get_layer_parent_comp = layer_slots
      ? reinterpret_cast<GetLayerParentComp>(layer_slots[6]) : nullptr;
  const auto get_layer_from_id = layer_slots
      ? reinterpret_cast<GetLayerFromId>(layer_slots[45]) : nullptr;
  ok = ok && item_suite && item_suite->get_active_item &&
      item_suite->get_item_type && get_comp_from_item &&
      get_layer_count && get_layer_by_index && get_layer_index &&
      get_layer_parent_comp && get_layer_from_id;

  void* item = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1110));
  void* comp = reinterpret_cast<void*>(static_cast<uintptr_t>(0x2220));
  int16_t item_type = -1;
  int32_t layer_count = -1;
  if (ok) {
    ok = item_suite->get_active_item(&item) == 0 && item &&
        reinterpret_cast<uintptr_t>(item) % alignof(std::max_align_t) == 0 &&
        item_suite->get_item_type(item, &item_type) == 0 &&
        item_type == 2 &&
        get_comp_from_item(item, &comp) == 0 && comp &&
        reinterpret_cast<uintptr_t>(comp) % alignof(std::max_align_t) == 0 &&
        get_layer_count(comp, &layer_count) == 0 && layer_count == 3;
  }
  std::array<void*, 3> traversed_layers{};
  for (int32_t index = 0; ok && index < layer_count; ++index) {
    void* layer = reinterpret_cast<void*>(static_cast<uintptr_t>(0x3330));
    int32_t observed_index = -1;
    void* parent_comp =
        reinterpret_cast<void*>(static_cast<uintptr_t>(0x4440));
    ok = get_layer_by_index(comp, index, &layer) == 0 && layer &&
        reinterpret_cast<uintptr_t>(layer) % alignof(std::max_align_t) == 0 &&
        get_layer_index(layer, &observed_index) == 0 &&
        observed_index == index &&
        get_layer_parent_comp(layer, &parent_comp) == 0 &&
        parent_comp == comp;
    traversed_layers[static_cast<std::size_t>(index)] = layer;
  }
  ok = ok && traversed_layers[0] && traversed_layers[1] &&
      traversed_layers[2] && traversed_layers[0] != traversed_layers[1] &&
      traversed_layers[1] != traversed_layers[2];

  void* unchanged_handle =
      reinterpret_cast<void*>(static_cast<uintptr_t>(0x5550));
  int32_t unchanged_i32 = 0x12345678;
  const void* handle_sentinel = unchanged_handle;
  const int32_t i32_sentinel = unchanged_i32;
  ok = ok && get_comp_from_item(comp, &unchanged_handle) != 0 &&
      unchanged_handle == handle_sentinel &&
      get_layer_count(item, &unchanged_i32) != 0 &&
      unchanged_i32 == i32_sentinel &&
      get_layer_index(comp, &unchanged_i32) != 0 &&
      unchanged_i32 == i32_sentinel;

  auto& registry = scene_model::registry();
  const auto observed_lease_identity =
      *static_cast<const uint64_t*>(item);
  ForgedBorrowedToken forged{observed_lease_identity};
  unchanged_handle =
      reinterpret_cast<void*>(static_cast<uintptr_t>(0x5550));
  ok = ok && observed_lease_identity != 0 &&
      get_comp_from_item(&forged, &unchanged_handle) != 0 &&
      unchanged_handle == handle_sentinel;

  alignas(std::max_align_t) std::array<std::byte, 64> foreign{};
  unchanged_handle =
      reinterpret_cast<void*>(static_cast<uintptr_t>(0x5550));
  ok = ok && get_comp_from_item(foreign.data(), &unchanged_handle) != 0 &&
      unchanged_handle == handle_sentinel;

  int local_item{};
  int local_comp{};
  std::array<int, 3> local_layers{};
  std::array<void*, 3> local_layer_handles{{
      &local_layers[0], &local_layers[1], &local_layers[2]}};
  scene_model::Registry other_registry;
  ok = ok && other_registry.initialize_fixture(
      &local_item, &local_comp, local_layer_handles.data(),
      local_layer_handles.size());
  void* cross_registry_item =
      other_registry.borrow(other_registry.active_item());
  unchanged_handle =
      reinterpret_cast<void*>(static_cast<uintptr_t>(0x5550));
  ok = ok && cross_registry_item &&
      get_comp_from_item(cross_registry_item, &unchanged_handle) != 0 &&
      unchanged_handle == handle_sentinel;

  const scene_model::Identity project_b{
      2, 2, 1, scene_model::ObjectKind::project, {}};
  scene_model::ObjectSnapshot root_b{};
  scene_model::ObjectSnapshot item_b{};
  scene_model::ObjectSnapshot comp_b{};
  scene_model::ObjectSnapshot layer_b{};
  ok = ok && registry.first_child(project_b, root_b) &&
      registry.first_child(root_b.identity, item_b) &&
      registry.comp_from_item(item_b.identity, comp_b) &&
      registry.layer_by_index(comp_b.identity, 0, layer_b) &&
      layer_b.identity.object_id <= INT32_MAX;
  unchanged_handle =
      reinterpret_cast<void*>(static_cast<uintptr_t>(0x5550));
  ok = ok && get_layer_from_id(
      comp, static_cast<int32_t>(layer_b.identity.object_id),
      &unchanged_handle) != 0 &&
      unchanged_handle == handle_sentinel;

  bool released = true;
  if (layer_acquired)
    released = compat_release_suite("AEGP Layer Suite", 14) == 0 &&
        released;
  if (comp_acquired)
    released = compat_release_suite("AEGP Comp Suite", 25) == 0 &&
        released;
  if (item_acquired)
    released = compat_release_suite("AEGP Item Suite", 14) == 0 &&
        released;
  g_aegp_comp_idle_roundtrip_mode = saved_mode;
  return ok && released && g_hooks.suite_leases_balanced();
}

bool verify_camera_case(bool smart_case) {
  if (!g_hooks.get_camera || !g_hooks.set_camera_index || !g_hooks.camera_index ||
      !g_hooks.layer_at || !g_hooks.layer_index) return false;
  const int32_t saved_index = g_hooks.camera_index();
  const bool saved_live = pf_state_runtime::effect_is_live();
  const auto saved_in = g_aegp_layer_in_points;
  const auto saved_duration = g_aegp_layer_durations;
  pf_state_runtime::reset_effect_lifetime(true);
  g_hooks.set_camera_index(-1);
  const suite_abi::AegpTime active{smart_case ? 45 : 15, 30};
  void* camera = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  bool ok = g_hooks.get_camera(g_hooks.effect, &active, &camera) == 0 && !camera;
  g_hooks.set_camera_index(2);
  g_aegp_layer_in_points[2] = {smart_case ? 30 : 10, 30};
  g_aegp_layer_durations[2] = {60, 30};
  camera = nullptr;
  ok = ok && g_hooks.get_camera(g_hooks.effect, &active, &camera) == 0 &&
      camera == g_hooks.layer_at(2) && g_hooks.layer_index(camera) == 2;
  void* unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678));
  camera = unchanged;
  suite_abi::AegpTime invalid{active.value, 0};
  suite_abi::AegpTime before{smart_case ? 29 : 9, 30};
  suite_abi::AegpTime after{smart_case ? 90 : 70, 30};
  uint32_t foreign = 0x464f5247;
  ok = ok && g_hooks.get_camera(nullptr, &active, &camera) != 0 && camera == unchanged &&
      g_hooks.get_camera(&foreign, &active, &camera) != 0 && camera == unchanged &&
      g_hooks.get_camera(g_hooks.effect, nullptr, &camera) != 0 && camera == unchanged &&
      g_hooks.get_camera(g_hooks.effect, &invalid, &camera) != 0 && camera == unchanged;
  camera = unchanged;
  ok = ok && g_hooks.get_camera(g_hooks.effect, &before, &camera) == 0 && !camera;
  camera = unchanged;
  ok = ok && g_hooks.get_camera(g_hooks.effect, &after, &camera) == 0 && !camera &&
      g_hooks.get_camera(g_hooks.effect, &active, nullptr) != 0;
  camera = unchanged;
  pf_state_runtime::reset_effect_lifetime(false);
  ok = ok && g_hooks.get_camera(g_hooks.effect, &active, &camera) != 0 && camera == unchanged;
  g_hooks.set_camera_index(saved_index);
  g_aegp_layer_in_points = saved_in;
  g_aegp_layer_durations = saved_duration;
  pf_state_runtime::reset_effect_lifetime(saved_live);
  return ok;
}

bool verify_matrix_case(bool smart_case) {
  if (!g_hooks.get_camera_matrix || !g_hooks.get_dimensions ||
      !g_hooks.set_dimensions || !g_hooks.set_camera_index ||
      !g_hooks.camera_index) return false;
  const bool saved_live = pf_state_runtime::effect_is_live();
  const int32_t saved_camera_index = g_hooks.camera_index();
  int32_t saved_width = 0, saved_height = 0;
  g_hooks.get_dimensions(&saved_width, &saved_height);
  const auto saved_transforms = scene_runtime_state().layer_transforms;
  const auto saved_parent_indices = scene_runtime_state().layer_parent_indices;
  const auto saved_camera_zoom = scene_runtime_state().layer_camera_zoom;
  const auto saved_camera_zoom_keyframes = scene_runtime_state().layer_camera_zoom_keyframes;
  const auto saved_spatial = aexcompat::render::render_context_state();
  pf_state_runtime::reset_effect_lifetime(true);
  g_hooks.set_dimensions(smart_case ? 1920 : 640, smart_case ? 1080 : 480);
  auto& spatial = aexcompat::render::render_context_state();
  spatial.downsample_x = {1, 1};
  spatial.downsample_y = {1, 1};
  spatial.pixel_aspect_ratio = {1, 1};
  g_hooks.set_camera_index(-1);
  const suite_abi::AegpTime time{smart_case ? 45 : 15, 30};
  AegpMatrix4 matrix{};
  double distance = -1.0;
  int16_t width = -1, height = -1;
  const int16_t expected_width = smart_case ? 1920 : 640;
  const int16_t expected_height = smart_case ? 1080 : 480;
  const auto near = [](double actual, double expected) {
    return std::abs(actual - expected) < 1.0e-9;
  };
  bool ok = g_hooks.get_camera_matrix(g_hooks.effect, &time, &matrix, &distance,
      &width, &height) == 0 && distance == expected_width &&
      width == expected_width && height == expected_height;
  for (std::size_t row = 0; row < 4; ++row)
    for (std::size_t column = 0; column < 4; ++column)
      ok = ok && matrix.mat[row][column] == (row == column ? 1.0 : 0.0);

  spatial.downsample_x = {1, 2};
  spatial.downsample_y = {1, 2};
  spatial.pixel_aspect_ratio = {10, 11};
  matrix = {};
  distance = -1.0;
  width = -1;
  height = -1;
  ok = ok && g_hooks.get_camera_matrix(g_hooks.effect, &time, &matrix, &distance,
      &width, &height) == 0 && distance == expected_width &&
      width == expected_width && height == expected_height;
  for (std::size_t row = 0; row < 4; ++row)
    for (std::size_t column = 0; column < 4; ++column)
      ok = ok && matrix.mat[row][column] == (row == column ? 1.0 : 0.0);

  spatial.pixel_aspect_ratio.denominator = 0;
  AegpMatrix4 sentinel{};
  std::memset(&sentinel, 0x5a, sizeof(sentinel));
  matrix = sentinel;
  distance = -2.0;
  width = -2;
  height = -2;
  ok = ok && g_hooks.get_camera_matrix(g_hooks.effect, &time, &matrix, &distance,
      &width, &height) != 0 && std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0 &&
      distance == -2.0 && width == -2 && height == -2;
  spatial.downsample_x = {1, 1};
  spatial.downsample_y = {1, 1};
  spatial.pixel_aspect_ratio = {1, 1};

  scene_runtime_state().layer_transforms[2] = {};
  scene_runtime_state().layer_transforms[2].position = {{100.0, 200.0, 0.0}};
  scene_runtime_state().layer_transforms[2].scale = {{100.0, 100.0, 100.0}};
  scene_runtime_state().layer_transforms[2].is_3d = true;
  scene_runtime_state().layer_parent_indices[2] = -1;
  g_hooks.set_camera_index(2);
  matrix = {};
  distance = -1.0;
  width = -1;
  height = -1;
  ok = ok && g_hooks.get_camera_matrix(g_hooks.effect, &time, &matrix, &distance,
      &width, &height) == 0 && near(matrix.mat[0][3], -100.0) &&
      near(matrix.mat[1][3], -200.0) && near(matrix.mat[2][3], 0.0) &&
      distance == expected_width && width == expected_width && height == expected_height;

  scene_runtime_state().layer_camera_zoom[2] = 1400.0;
  matrix = {};
  distance = -1.0;
  width = -1;
  height = -1;
  ok = ok && g_hooks.get_camera_matrix(g_hooks.effect, &time, &matrix, &distance,
      &width, &height) == 0 && near(matrix.mat[0][3], -100.0) &&
      near(matrix.mat[1][3], -200.0) && distance == 1400.0 &&
      width == expected_width && height == expected_height;

  auto& zoom_keyframes = scene_runtime_state().layer_camera_zoom_keyframes[2];
  zoom_keyframes = {};
  zoom_keyframes[0].valid = true;
  zoom_keyframes[0].time = {0, 30};
  zoom_keyframes[0].zoom = 1000.0;
  zoom_keyframes[1].valid = true;
  zoom_keyframes[1].time = {60, 30};
  zoom_keyframes[1].zoom = 2000.0;
  const suite_abi::AegpTime zoom_midpoint{30, 30};
  matrix = {};
  distance = -1.0;
  width = -1;
  height = -1;
  ok = ok && g_hooks.get_camera_matrix(g_hooks.effect, &zoom_midpoint, &matrix,
      &distance, &width, &height) == 0 && near(matrix.mat[0][3], -100.0) &&
      near(matrix.mat[1][3], -200.0) && distance == 1500.0 &&
      width == expected_width && height == expected_height;
  const suite_abi::AegpTime zoom_after{90, 30};
  matrix = {};
  distance = -1.0;
  width = -1;
  height = -1;
  ok = ok && g_hooks.get_camera_matrix(g_hooks.effect, &zoom_after, &matrix,
      &distance, &width, &height) == 0 && distance == 2000.0 &&
      width == expected_width && height == expected_height;
  zoom_keyframes[1].zoom = 0.0;
  matrix = sentinel;
  distance = -2.0;
  width = -2;
  height = -2;
  ok = ok && g_hooks.get_camera_matrix(g_hooks.effect, &zoom_after, &matrix,
      &distance, &width, &height) != 0 && std::memcmp(&matrix, &sentinel,
      sizeof(matrix)) == 0 && distance == -2.0 && width == -2 && height == -2;

  scene_runtime_state().layer_transforms[2].scale[0] = 0.0;
  matrix = sentinel;
  distance = -2.0;
  width = -2;
  height = -2;
  ok = ok && g_hooks.get_camera_matrix(g_hooks.effect, &time, &matrix, &distance,
      &width, &height) != 0 && std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0 &&
      distance == -2.0 && width == -2 && height == -2;
  g_hooks.set_camera_index(-1);
  scene_runtime_state().layer_transforms = saved_transforms;
  scene_runtime_state().layer_parent_indices = saved_parent_indices;
  scene_runtime_state().layer_camera_zoom = saved_camera_zoom;
  scene_runtime_state().layer_camera_zoom_keyframes = saved_camera_zoom_keyframes;
  matrix = sentinel; distance = -2.0; width = -2; height = -2;
  suite_abi::AegpTime invalid{time.value, 0};
  ok = ok && g_hooks.get_camera_matrix(nullptr, &time, &matrix, &distance,
      &width, &height) != 0 && std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0 &&
      g_hooks.get_camera_matrix(g_hooks.effect, &invalid, &matrix, &distance,
      &width, &height) != 0 && std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0;
  pf_state_runtime::reset_effect_lifetime(false);
  ok = ok && g_hooks.get_camera_matrix(g_hooks.effect, &time, &matrix, &distance,
      &width, &height) != 0 && std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0;
  spatial = saved_spatial;
  g_hooks.set_dimensions(saved_width, saved_height);
  g_hooks.set_camera_index(saved_camera_index);
  pf_state_runtime::reset_effect_lifetime(saved_live);
  return ok;
}

bool verify_aegp_get_effect_camera() {
  if (!g_hooks.acquire_suite || !g_hooks.release_suite) return false;
  const void* suite = nullptr;
  bool ok = g_hooks.acquire_suite("AEGP PF Interface Suite", 1, &suite) == 0 &&
      suite == g_hooks.interface_suite && verify_camera_case(false) &&
      verify_camera_case(true) && verify_matrix_case(false) && verify_matrix_case(true);
  ok = g_hooks.release_suite("AEGP PF Interface Suite", 1) == 0 && ok;
  return ok && (!g_hooks.suite_leases_balanced || g_hooks.suite_leases_balanced());
}

#define acquire_suite compat_acquire_suite
#define release_suite compat_release_suite
#define suite_leases_balanced compat_suite_leases_balanced
bool verify_aegp_apply_effect() {
  const auto saved_instances = g_aegp_effect_instances;
  const auto saved_leases = g_aegp_effect_leases;
  const bool saved_live = g_aegp_effect_live;
  const bool saved_mode = g_aegp_comp_idle_roundtrip_mode;
  const uint32_t timestamp_before =
      aexcompat::aegp_external_render_runtime::project_generation();
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
  const uint32_t timestamp_after_apply =
      aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && aegp_get_new_effect_stream_by_index(8, applied, 1, &unchanged) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678)) &&
      aegp_apply_effect(7, nullptr, kAegpInstalledEffects[0].key, &unchanged) == 4 &&
      aegp_apply_effect(7, &g_aegp_layers[1], 9999, &unchanged) == 4 &&
      aegp_apply_effect(7, &g_aegp_layers[1], kAegpInstalledEffects[0].key, nullptr) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678)) &&
      std::memcmp(g_aegp_effect_instances.data(), scene_before_failures.data(),
                  sizeof(g_aegp_effect_instances)) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() == timestamp_after_apply;

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
      aegp_dispose_effect(g_hooks.effect) == 4;
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
      aegp_dispose_stream_v2(stream) == 4 &&
      aegp_get_effect_flags(duplicate, &flags) == 4 &&
      aegp_dispose_effect(duplicate) == 4;

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
       type == 0x12345678 && aegp_dispose_stream_v2(stale_stream) == 4 &&
       aegp_dispose_effect(effect) == 4;

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
bool verify_aegp_resizer_3d_chain() {
  const void* layer_suite = nullptr;
  const void* stream_suite = nullptr;
  const void* comp_suite = nullptr;
  const void* item_suite = nullptr;
  const int32_t saved_camera_index = g_aegp_active_camera_layer_index;
  int32_t saved_width = 0, saved_height = 0;
  g_hooks.get_dimensions(&saved_width, &saved_height);
  const auto saved_in_points = g_aegp_layer_in_points;
  const auto saved_durations = g_aegp_layer_durations;
  const auto saved_transforms = g_aegp_layer_transforms;
  const auto saved_parent_indices = g_aegp_layer_parent_indices;
  const auto saved_keyframes = scene_runtime_state().layer_transform_keyframes;
  const auto saved_camera_zoom = scene_runtime_state().layer_camera_zoom;
  const auto saved_camera_zoom_keyframes = scene_runtime_state().layer_camera_zoom_keyframes;
  g_aegp_active_camera_layer_index = 2;
  g_aegp_layer_in_points[2] = {0, 30};
  g_aegp_layer_durations[2] = {300, 30};
  g_hooks.set_dimensions(1920, 1080);
  const AegpTime time{45, 30};

  bool ok = acquire_suite("AEGP Layer Suite", 14, &layer_suite) == 0 &&
      layer_suite == g_aegp_layer_suite8.data() &&
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
  AegpMatrix4 matrix{};
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &time, &matrix) == 0;
  for (std::size_t row = 0; row < 4; ++row) {
    for (std::size_t column = 0; column < 4; ++column)
      ok = ok && matrix.mat[row][column] == (row == column ? 1.0 : 0.0);
  }
  g_aegp_layer_transforms[2] = {};
  g_aegp_layer_transforms[2].anchor = {{1.0, 2.0, 3.0}};
  g_aegp_layer_transforms[2].position = {{10.0, 20.0, 30.0}};
  g_aegp_layer_transforms[2].scale = {{200.0, 50.0, 100.0}};
  g_aegp_layer_transforms[2].rotation_degrees = {{0.0, 0.0, 90.0}};
  g_aegp_layer_transforms[2].is_3d = true;
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &time, &matrix) == 0;
  const auto near = [](double actual, double expected) {
    return std::abs(actual - expected) < 1.0e-9;
  };
  const double expected[4][4] = {
      {0.0, -0.5, 0.0, 11.0},
      {2.0, 0.0, 0.0, 18.0},
      {0.0, 0.0, 1.0, 27.0},
      {0.0, 0.0, 0.0, 1.0}};
  for (std::size_t row = 0; row < 4; ++row)
    for (std::size_t column = 0; column < 4; ++column)
      ok = ok && near(matrix.mat[row][column], expected[row][column]);
  AegpMatrix4 matrix_sentinel{};
  std::memset(&matrix_sentinel, 0x5a, sizeof(matrix_sentinel));
  g_aegp_layer_transforms[2].scale[0] = 0.0;
  matrix = matrix_sentinel;
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &time, &matrix) != 0 &&
      std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0;
  g_aegp_layer_transforms[2].scale[0] = 200.0;
  g_aegp_layer_transforms[2].rotation_degrees[1] =
      std::numeric_limits<double>::quiet_NaN();
  matrix = matrix_sentinel;
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &time, &matrix) != 0 &&
      std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0;
  int foreign_layer = 0;
  matrix = matrix_sentinel;
  ok = ok && aegp_get_layer_to_world_xform(&foreign_layer, &time, &matrix) != 0 &&
      std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0;
  g_aegp_layer_transforms[2] = saved_transforms[2];
  g_aegp_layer_transforms[2].position = {{10.0, 20.0, 30.0}};
  g_aegp_layer_transforms[2].scale = {{100.0, 100.0, 100.0}};
  g_aegp_layer_transforms[2].is_3d = true;
  g_aegp_layer_transforms[1] = {};
  g_aegp_layer_transforms[1].position = {{100.0, 200.0, 0.0}};
  g_aegp_layer_transforms[1].scale = {{100.0, 100.0, 100.0}};
  g_aegp_layer_transforms[1].is_3d = true;
  g_aegp_layer_parent_indices[2] = 1;
  void* parent = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && aegp_get_layer_parent(&g_aegp_layers[2], &parent) == 0 &&
      parent != nullptr &&
      parent != reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  const void* expected_parent = parent;
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &time, &matrix) == 0 &&
      near(matrix.mat[0][3], 110.0) && near(matrix.mat[1][3], 220.0) &&
      near(matrix.mat[2][3], 30.0);
  g_aegp_layer_parent_indices[1] = 2;
  matrix = matrix_sentinel;
  parent = reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678));
  ok = ok && aegp_get_layer_parent(&g_aegp_layers[2], &parent) == 0 &&
      parent == expected_parent &&
      aegp_get_layer_to_world_xform(&g_aegp_layers[2], &time, &matrix) != 0 &&
      std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0;
  g_aegp_layer_parent_indices[1] = -1;
  g_aegp_layer_parent_indices[2] = 7;
  matrix = matrix_sentinel;
  parent = reinterpret_cast<void*>(static_cast<uintptr_t>(0x9abc));
  ok = ok && aegp_get_layer_parent(&g_aegp_layers[2], &parent) != 0 &&
      parent == reinterpret_cast<void*>(static_cast<uintptr_t>(0x9abc)) &&
      aegp_get_layer_to_world_xform(&g_aegp_layers[2], &time, &matrix) != 0 &&
      std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0;
  g_aegp_layer_parent_indices = saved_parent_indices;
  g_aegp_layer_transforms[1] = saved_transforms[1];

  auto& keyframes = scene_runtime_state().layer_transform_keyframes[2];
  keyframes = {};
  keyframes[0].valid = true;
  keyframes[0].time = {0, 30};
  keyframes[0].transform.position = {{10.0, 20.0, 0.0}};
  keyframes[0].transform.scale = {{100.0, 100.0, 100.0}};
  keyframes[0].transform.is_3d = true;
  keyframes[1].valid = true;
  keyframes[1].time = {60, 30};
  keyframes[1].transform.position = {{70.0, 80.0, 30.0}};
  keyframes[1].transform.scale = {{100.0, 100.0, 100.0}};
  keyframes[1].transform.is_3d = true;
  g_aegp_layer_parent_indices[2] = -1;
  const AegpTime midpoint{30, 30};
  matrix = {};
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &midpoint, &matrix) == 0 &&
      near(matrix.mat[0][3], 40.0) && near(matrix.mat[1][3], 50.0) &&
      near(matrix.mat[2][3], 15.0);
  keyframes[1].time = {0, 30};
  matrix = matrix_sentinel;
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &midpoint, &matrix) != 0 &&
      std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0;
  scene_runtime_state().layer_transform_keyframes = saved_keyframes;
  AegpLegacyStreamVal zoom{-1.0};
  int32_t type = -1;
  scene_runtime_state().layer_camera_zoom[2] = 0.0;
  ok = ok && aegp_get_layer_stream_value_v2(&g_aegp_layers[2], 11, 1,
      &time, 0, &zoom, &type) == 0 && zoom.one_d == 1920.0 && type == 5;
  scene_runtime_state().layer_camera_zoom[2] = 1400.0;
  zoom = {-1.0};
  type = -1;
  ok = ok && aegp_get_layer_stream_value_v2(&g_aegp_layers[2], 11, 1,
      &time, 0, &zoom, &type) == 0 && near(zoom.one_d, 1400.0) && type == 5;
  auto& zoom_keyframes = scene_runtime_state().layer_camera_zoom_keyframes[2];
  zoom_keyframes = {};
  zoom_keyframes[0].valid = true;
  zoom_keyframes[0].time = {0, 30};
  zoom_keyframes[0].zoom = 1000.0;
  zoom_keyframes[1].valid = true;
  zoom_keyframes[1].time = {60, 30};
  zoom_keyframes[1].zoom = 2000.0;
  const AegpTime zoom_midpoint{30, 30};
  zoom = {-1.0};
  type = -1;
  ok = ok && aegp_get_layer_stream_value_v2(&g_aegp_layers[2], 11, 1,
      &zoom_midpoint, 0, &zoom, &type) == 0 && near(zoom.one_d, 1500.0) && type == 5;
  zoom_keyframes[1].time = {0, 30};
  zoom = {123.0};
  type = 77;
  ok = ok && aegp_get_layer_stream_value_v2(&g_aegp_layers[2], 11, 1,
      &zoom_midpoint, 0, &zoom, &type) != 0 && near(zoom.one_d, 123.0) && type == 77;
  scene_runtime_state().layer_camera_zoom = saved_camera_zoom;
  scene_runtime_state().layer_camera_zoom_keyframes = saved_camera_zoom_keyframes;
  void* item = nullptr;
  int32_t width = -1;
  int32_t height = -1;
  ok = ok && aegp_get_item_from_comp(g_hooks.comp, &item) == 0 &&
      item != nullptr &&
      aegp_get_item_dimensions(item, &width, &height) == 0 &&
      width == 1920 && height == 1080;

  AegpLegacyStreamVal sentinel{-2.0};
  type = -2;
  AegpTime invalid{45, 0};
  ok = ok && aegp_get_layer_stream_value_v2(&g_aegp_layers[1], 11, 1,
      &time, 0, &sentinel, &type) != 0 && sentinel.one_d == -2.0 && type == -2 &&
      aegp_get_layer_stream_value_v2(&g_aegp_layers[2], 10, 1,
      &time, 0, &sentinel, &type) != 0 && sentinel.one_d == -2.0 && type == -2;
  matrix = matrix_sentinel;
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &invalid, &matrix) != 0 &&
      std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0;
  void* item_sentinel = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  width = -2;
  height = -3;
  ok = ok && aegp_get_item_from_comp(g_hooks.pf_layer, &item_sentinel) != 0 &&
      item_sentinel == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234)) &&
      aegp_get_item_dimensions(g_hooks.pf_layer, &width, &height) != 0 &&
      width == -2 && height == -3;

  ok = release_suite("AEGP Item Suite", 10) == 0 && ok;
  ok = release_suite("AEGP Comp Suite", 9) == 0 && ok;
  ok = release_suite("AEGP Stream Suite", 7) == 0 && ok;
  ok = release_suite("AEGP Layer Suite", 14) == 0 && ok;
  g_aegp_active_camera_layer_index = saved_camera_index;
  g_hooks.set_dimensions(saved_width, saved_height);
  g_aegp_layer_in_points = saved_in_points;
  g_aegp_layer_durations = saved_durations;
  g_aegp_layer_transforms = saved_transforms;
  g_aegp_layer_parent_indices = saved_parent_indices;
  scene_runtime_state().layer_transform_keyframes = saved_keyframes;
  scene_runtime_state().layer_camera_zoom = saved_camera_zoom;
  scene_runtime_state().layer_camera_zoom_keyframes = saved_camera_zoom_keyframes;
  return ok && suite_leases_balanced();
}

#define g_aegp_effect scene_runtime_state().effect
#define aegp_get_effect_param_union_by_index_v3 g_hooks.get_effect_param_union_v3

bool verify_aegp_effect_param_union_suite4() {
  constexpr std::size_t kParamSize = worker_runtime::parameters::kDefinitionSize;
  const bool saved_live = g_aegp_effect_live;
  g_aegp_effect_live = true;
  std::array<std::byte, kParamSize - 56> value{};
  std::array<std::byte, kParamSize - 56> sentinel{};
  sentinel.fill(std::byte{0x5a});
  int32_t type = -1;
  bool ok = aegp_get_effect_param_union_by_index_v3(
                1, &g_aegp_effect, 0, &type, value.data()) == 0 &&
            type == 0 &&
            std::all_of(value.begin(), value.end(),
                        [](std::byte byte) { return byte == std::byte{0}; });
  value = sentinel;
  type = -1;
  ok = ok && aegp_get_effect_param_union_by_index_v3(
                 1, &g_aegp_effect, 1, &type, value.data()) == 0 &&
       type == 1;
  for (const int32_t index : {-1, 5}) {
    value = sentinel;
    type = 0x12345678;
    ok = ok && aegp_get_effect_param_union_by_index_v3(
                   1, &g_aegp_effect, index, &type, value.data()) == 4 &&
         type == 0x12345678 && value == sentinel;
  }
  value = sentinel;
  type = 0x12345678;
  ok = ok && aegp_get_effect_param_union_by_index_v3(
                 1, &g_aegp_effect, 0, nullptr, value.data()) == 4 &&
       value == sentinel &&
       aegp_get_effect_param_union_by_index_v3(
           1, &g_aegp_effect, 0, &type, nullptr) == 4 &&
       type == 0x12345678;
  g_aegp_effect_live = false;
  ok = ok && aegp_get_effect_param_union_by_index_v3(
                 1, &g_aegp_effect, 0, &type, value.data()) == 4;
  g_aegp_effect_live = saved_live;
  return ok;
}

bool verify_aegp_scene_mutation_transactions() {
  using ApplyEffect = int32_t (__cdecl*)(int32_t, void*, int32_t, void**);
  using DuplicateEffect = int32_t (__cdecl*)(void*, void**);
  using DeleteEffect = int32_t (__cdecl*)(void*);
  using ReorderEffect = int32_t (__cdecl*)(void*, int32_t);
  using SetEffectFlags = int32_t (__cdecl*)(void*, uint32_t, uint32_t);
  using GetEffectStream = int32_t (__cdecl*)(int32_t, void*, int32_t, void**);
  using GetStreamType = int32_t (__cdecl*)(void*, int32_t*);
  using GetStreamValue = int32_t (__cdecl*)(
      int32_t, void*, int32_t, const AegpTime*, uint8_t, AegpStreamValue*);
  using SetStreamValue = int32_t (__cdecl*)(
      int32_t, void*, AegpStreamValue*);
  using DisposeStreamValue = int32_t (__cdecl*)(AegpStreamValue*);
  using DisposeStream = int32_t (__cdecl*)(void*);

  const auto saved_instances = g_aegp_effect_instances;
  const auto saved_leases = g_aegp_effect_leases;
  const auto saved_streams = g_aegp_legacy_effect_streams;
  const auto saved_transform = g_aegp_transform_stream;
  const bool saved_mode = g_aegp_comp_idle_roundtrip_mode;
  g_aegp_effect_instances = {};
  g_aegp_effect_instances[0] = {
      &g_aegp_layers[0], kAegpInstalledEffects[0].key, 0, 1, 1, true};
  g_aegp_effect_leases = {};
  g_aegp_legacy_effect_streams = {};
  g_aegp_transform_stream = {};
  g_aegp_comp_idle_roundtrip_mode = true;

  const void* effect_suite_raw = nullptr;
  const void* stream7_raw = nullptr;
  bool ok = acquire_suite("AEGP Effect Suite", 4, &effect_suite_raw) == 0 &&
      acquire_suite("AEGP Stream Suite", 7, &stream7_raw) == 0 &&
      effect_suite_raw == g_aegp_effect_suite4.data() &&
      stream7_raw == g_aegp_stream_suite2.data();
  const auto* effect_slots =
      static_cast<void* const*>(const_cast<void*>(effect_suite_raw));
  const auto* stream_slots =
      static_cast<void* const*>(const_cast<void*>(stream7_raw));
  const auto apply = effect_slots
      ? reinterpret_cast<ApplyEffect>(effect_slots[9]) : nullptr;
  const auto duplicate = effect_slots
      ? reinterpret_cast<DuplicateEffect>(effect_slots[16]) : nullptr;
  const auto erase = effect_slots
      ? reinterpret_cast<DeleteEffect>(effect_slots[10]) : nullptr;
  const auto reorder = effect_slots
      ? reinterpret_cast<ReorderEffect>(effect_slots[6]) : nullptr;
  const auto set_flags = effect_slots
      ? reinterpret_cast<SetEffectFlags>(effect_slots[5]) : nullptr;
  const auto get_effect_stream = stream_slots
      ? reinterpret_cast<GetEffectStream>(stream_slots[5]) : nullptr;
  const auto get_stream_type = stream_slots
      ? reinterpret_cast<GetStreamType>(stream_slots[12]) : nullptr;
  const auto get_stream_value = stream_slots
      ? reinterpret_cast<GetStreamValue>(stream_slots[13]) : nullptr;
  const auto set_stream_value = stream_slots
      ? reinterpret_cast<SetStreamValue>(stream_slots[15]) : nullptr;
  const auto dispose_stream_value = stream_slots
      ? reinterpret_cast<DisposeStreamValue>(stream_slots[14]) : nullptr;
  const auto dispose_stream = stream_slots
      ? reinterpret_cast<DisposeStream>(stream_slots[7]) : nullptr;
  ok = ok && apply && duplicate && erase && reorder && set_flags &&
      get_effect_stream && get_stream_type && get_stream_value &&
      set_stream_value && dispose_stream_value && dispose_stream;

  uint32_t generation =
      aexcompat::aegp_external_render_runtime::project_generation();
  void* first = nullptr;
  ok = ok && apply(7, &g_aegp_layers[1],
                   kAegpInstalledEffects[0].key, &first) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  const auto scene_before_failure = g_aegp_effect_instances;
  void* unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && apply(7, &g_aegp_layers[1], 9999, &unchanged) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234)) &&
      std::memcmp(scene_before_failure.data(), g_aegp_effect_instances.data(),
                  sizeof(scene_before_failure)) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation;

  aexcompat::scene_model::ObjectSnapshot project_b_root{};
  aexcompat::scene_model::ObjectSnapshot project_b_item{};
  aexcompat::scene_model::ObjectSnapshot project_b_comp{};
  aexcompat::scene_model::ObjectSnapshot project_b_layer{};
  const aexcompat::scene_model::Identity project_b{
      2, 2, 1, aexcompat::scene_model::ObjectKind::project, {}};
  auto& registry = aexcompat::scene_model::registry();
  ok = ok && registry.first_child(project_b, project_b_root) &&
      registry.first_child(project_b_root.identity, project_b_item) &&
      registry.comp_from_item(project_b_item.identity, project_b_comp) &&
      registry.layer_by_index(project_b_comp.identity, 0, project_b_layer);
  void* cross_project_layer = registry.borrow(project_b_layer.identity);
  unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678));
  ok = ok && apply(7, cross_project_layer,
                   kAegpInstalledEffects[0].key, &unchanged) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678)) &&
      apply(7, first, kAegpInstalledEffects[0].key, &unchanged) == 4 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation;

  void* second = nullptr;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && duplicate(first, &second) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && set_flags(second, 3, 2) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && reorder(second, 0) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;

  void* effect_stream = nullptr;
  unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x9abc));
  ok = ok && get_effect_stream(8, second, 1, &unchanged) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x9abc)) &&
      get_effect_stream(7, second, 1, &effect_stream) == 0;
  AegpTime time{0, 30};
  AegpStreamValue effect_value{};
  ok = ok && get_stream_value(
                   7, effect_stream, 1, &time, 0, &effect_value) == 0;
  const auto stream_scene_before_failure = g_aegp_effect_instances;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  const double nan = std::numeric_limits<double>::quiet_NaN();
  std::memcpy(effect_value.value.data(), &nan, sizeof(nan));
  ok = ok && set_stream_value(7, effect_stream, &effect_value) == 4 &&
      std::memcmp(stream_scene_before_failure.data(),
                  g_aegp_effect_instances.data(),
                  sizeof(stream_scene_before_failure)) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation;
  const double amount = 72.5;
  std::memcpy(effect_value.value.data(), &amount, sizeof(amount));
  ok = ok && set_stream_value(7, effect_stream, &effect_value) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1 &&
      dispose_stream_value(&effect_value) == 0;
  aexcompat::scene_model::ObjectSnapshot effect_stream_identity{};
  ok = ok && registry.resolve(
      effect_stream, aexcompat::scene_model::ObjectKind::stream,
      effect_stream_identity);
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && erase(second) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;
  int32_t type_sentinel = 0x12345678;
  ok = ok && get_stream_type(effect_stream, &type_sentinel) == 4 &&
      type_sentinel == 0x12345678 &&
      dispose_stream(effect_stream) == 4 &&
      !registry.snapshot(effect_stream_identity.identity,
                         effect_stream_identity) &&
      aegp_dispose_effect(second) == 4 &&
      aegp_dispose_effect(first) == 0;

  ok = release_suite("AEGP Stream Suite", 7) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 4) == 0 && ok;

  using GetMask = int32_t (__cdecl*)(void*, int32_t, void**);
  using DisposeMask = int32_t (__cdecl*)(void*);
  using GetMaskStream = int32_t (__cdecl*)(
      int32_t, void*, int32_t, void**);
  using InsertKeyframe = int32_t (__cdecl*)(
      void*, int16_t, const HostTime*, int32_t*);
  using DeleteKeyframe = int32_t (__cdecl*)(void*, int32_t);
  using KeyframeCount = int32_t (__cdecl*)(void*, int32_t*);
  using StartAdd = int32_t (__cdecl*)(void*, void**);
  using AddKey = int32_t (__cdecl*)(
      void*, int16_t, const HostTime*, int32_t*);
  using EndAdd = int32_t (__cdecl*)(uint8_t, void*);
  using GetKeyValue = int32_t (__cdecl*)(
      int32_t, void*, int32_t, StreamValue*);
  using SetKeyValue = int32_t (__cdecl*)(
      void*, int32_t, const StreamValue*);
  using GetTangents = int32_t (__cdecl*)(
      int32_t, void*, int32_t, StreamValue*, StreamValue*);
  using SetTangents = int32_t (__cdecl*)(
      void*, int32_t, const StreamValue*, const StreamValue*);
  using SetEase = int32_t (__cdecl*)(
      void*, int32_t, int32_t, const KeyframeEase*, const KeyframeEase*);
  using SetKeyFlag = int32_t (__cdecl*)(
      void*, int32_t, int32_t, uint8_t);
  using SetInterpolation = int32_t (__cdecl*)(
      void*, int32_t, int32_t, int32_t);
  using SetLabel = int32_t (__cdecl*)(void*, int32_t, int32_t);
  using GetDynamicForLayer = int32_t (__cdecl*)(int32_t, void*, void**);
  using GetDynamicByIndex = int32_t (__cdecl*)(
      int32_t, void*, int32_t, void**);
  using GetDynamicFlags = int32_t (__cdecl*)(void*, uint32_t*);
  using SetDynamicFlag = int32_t (__cdecl*)(
      void*, uint32_t, uint8_t, uint8_t);
  using DeleteDynamic = int32_t (__cdecl*)(void*);
  using ReorderDynamic = int32_t (__cdecl*)(void*, int32_t);
  using DuplicateDynamic = int32_t (__cdecl*)(int32_t, void*, int32_t*);
  using SetDynamicName = int32_t (__cdecl*)(void*, const uint16_t*);
  using AddDynamic = int32_t (__cdecl*)(
      int32_t, void*, const char*, void**);

  ok = configure_mask_scene("rectangle") && ok;
  const bool saved_mask_model_enabled =
      aexcompat::mask_runtime::model_enabled();
  aexcompat::mask_runtime::set_model_enabled(true);
  g_aegp_comp_idle_roundtrip_mode = false;
  const void* mask_suite_raw = nullptr;
  const void* mask_stream_suite_raw = nullptr;
  const void* keyframe_suite_raw = nullptr;
  const void* dynamic_suite_raw = nullptr;
  ok = acquire_suite("AEGP Layer Mask Suite", 7, &mask_suite_raw) == 0 &&
      acquire_suite("AEGP Stream Suite", 11, &mask_stream_suite_raw) == 0 &&
      acquire_suite("AEGP Keyframe Suite", 5, &keyframe_suite_raw) == 0 &&
      acquire_suite("AEGP Dynamic Stream Suite", 5, &dynamic_suite_raw) == 0 &&
      ok;
  const auto* mask_slots =
      static_cast<void* const*>(const_cast<void*>(mask_suite_raw));
  const auto* mask_stream_slots =
      static_cast<void* const*>(const_cast<void*>(mask_stream_suite_raw));
  const auto* key_slots =
      static_cast<void* const*>(const_cast<void*>(keyframe_suite_raw));
  const auto* dynamic_slots =
      static_cast<void* const*>(const_cast<void*>(dynamic_suite_raw));
  const auto get_mask = mask_slots
      ? reinterpret_cast<GetMask>(mask_slots[1]) : nullptr;
  const auto dispose_mask_fn = mask_slots
      ? reinterpret_cast<DisposeMask>(mask_slots[2]) : nullptr;
  const auto get_mask_stream = mask_stream_slots
      ? reinterpret_cast<GetMaskStream>(mask_stream_slots[6]) : nullptr;
  const auto dispose_mask_stream = mask_stream_slots
      ? reinterpret_cast<DisposeStream>(mask_stream_slots[7]) : nullptr;
  const auto dispose_mask_value = mask_stream_slots
      ? reinterpret_cast<int32_t (__cdecl*)(StreamValue*)>(
            mask_stream_slots[14]) : nullptr;
  const auto key_count = key_slots
      ? reinterpret_cast<KeyframeCount>(key_slots[0]) : nullptr;
  const auto insert_key = key_slots
      ? reinterpret_cast<InsertKeyframe>(key_slots[2]) : nullptr;
  const auto delete_key = key_slots
      ? reinterpret_cast<DeleteKeyframe>(key_slots[3]) : nullptr;
  const auto start_add = key_slots
      ? reinterpret_cast<StartAdd>(key_slots[16]) : nullptr;
  const auto add_key = key_slots
      ? reinterpret_cast<AddKey>(key_slots[17]) : nullptr;
  const auto end_add = key_slots
      ? reinterpret_cast<EndAdd>(key_slots[19]) : nullptr;
  const auto get_key_value = key_slots
      ? reinterpret_cast<GetKeyValue>(key_slots[4]) : nullptr;
  const auto set_key_value = key_slots
      ? reinterpret_cast<SetKeyValue>(key_slots[5]) : nullptr;
  const auto get_tangents = key_slots
      ? reinterpret_cast<GetTangents>(key_slots[8]) : nullptr;
  const auto set_tangents = key_slots
      ? reinterpret_cast<SetTangents>(key_slots[9]) : nullptr;
  const auto set_ease = key_slots
      ? reinterpret_cast<SetEase>(key_slots[11]) : nullptr;
  const auto set_key_flag = key_slots
      ? reinterpret_cast<SetKeyFlag>(key_slots[13]) : nullptr;
  const auto set_interpolation = key_slots
      ? reinterpret_cast<SetInterpolation>(key_slots[15]) : nullptr;
  const auto set_label = key_slots
      ? reinterpret_cast<SetLabel>(key_slots[21]) : nullptr;
  const auto get_dynamic_for_layer = dynamic_slots
      ? reinterpret_cast<GetDynamicForLayer>(dynamic_slots[0]) : nullptr;
  const auto get_dynamic_by_index = dynamic_slots
      ? reinterpret_cast<GetDynamicByIndex>(dynamic_slots[7]) : nullptr;
  const auto get_dynamic_flags = dynamic_slots
      ? reinterpret_cast<GetDynamicFlags>(dynamic_slots[5]) : nullptr;
  const auto set_dynamic_flag = dynamic_slots
      ? reinterpret_cast<SetDynamicFlag>(dynamic_slots[6]) : nullptr;
  const auto delete_dynamic = dynamic_slots
      ? reinterpret_cast<DeleteDynamic>(dynamic_slots[9]) : nullptr;
  const auto reorder_dynamic = dynamic_slots
      ? reinterpret_cast<ReorderDynamic>(dynamic_slots[10]) : nullptr;
  const auto duplicate_dynamic = dynamic_slots
      ? reinterpret_cast<DuplicateDynamic>(dynamic_slots[11]) : nullptr;
  const auto set_dynamic_name = dynamic_slots
      ? reinterpret_cast<SetDynamicName>(dynamic_slots[12]) : nullptr;
  const auto add_dynamic = dynamic_slots
      ? reinterpret_cast<AddDynamic>(dynamic_slots[14]) : nullptr;
  ok = ok && get_mask && dispose_mask_fn && get_mask_stream &&
      dispose_mask_stream && dispose_mask_value && key_count && insert_key &&
      delete_key && start_add && add_key && end_add && get_key_value &&
      set_key_value && get_tangents && set_tangents && set_ease &&
      set_key_flag && set_interpolation && set_label &&
      get_dynamic_for_layer && get_dynamic_by_index && get_dynamic_flags &&
      set_dynamic_flag && delete_dynamic && reorder_dynamic &&
      duplicate_dynamic && set_dynamic_name && add_dynamic;

  void* mask = nullptr;
  void* mask_stream = nullptr;
  ok = ok && get_mask(&g_layer, 0, &mask) == 0 &&
      get_mask_stream(1, mask, 400, &mask_stream) == 0;
  int32_t keyframes_before = -1;
  ok = ok && key_count(mask_stream, &keyframes_before) == 0;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  const uint64_t registry_before_cancel = registry.fingerprint();
  void* add_transaction = nullptr;
  int32_t staged_index = -1;
  const HostTime staged_time{15, 30};
  const auto transaction_before =
      aexcompat::scene_transaction::diagnostics();
  ok = ok && start_add(mask_stream, &add_transaction) == 0 &&
      add_key(add_transaction, 1, &staged_time, &staged_index) == 0 &&
      end_add(0, add_transaction) == 0;
  int32_t keyframes_after_cancel = -1;
  const auto transaction_after =
      aexcompat::scene_transaction::diagnostics();
  ok = ok && key_count(mask_stream, &keyframes_after_cancel) == 0 &&
      keyframes_after_cancel == keyframes_before &&
      registry.fingerprint() == registry_before_cancel &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation &&
      transaction_after.cancelled == transaction_before.cancelled + 1;

  int32_t unchanged_index = 0x12345678;
  const HostTime invalid_time{1, 0};
  ok = ok && insert_key(
                   mask_stream, 1, &invalid_time, &unchanged_index) == 4 &&
      unchanged_index == 0x12345678 &&
      registry.fingerprint() == registry_before_cancel &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation;
  int32_t inserted_index = -1;
  const HostTime key_time{30, 30};
  ok = ok && insert_key(
                   mask_stream, 1, &key_time, &inserted_index) == 0 &&
      inserted_index == keyframes_before &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;
  HostStreamRef* mask_stream_record = find_stream(mask_stream);
  HostKeyframe* key = keyframe_at(mask_stream_record, inserted_index);
  ok = ok && mask_stream_record && key &&
      key->identity.kind == aexcompat::scene_model::ObjectKind::keyframe;
  const auto key_identity = key ? key->identity :
      aexcompat::scene_model::Identity{};

  StreamValue wrong_owner_value{};
  wrong_owner_value.stream =
      reinterpret_cast<void*>(static_cast<uintptr_t>(0x4444));
  const StreamValue wrong_owner_sentinel = wrong_owner_value;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && get_key_value(
                   2, mask_stream, inserted_index, &wrong_owner_value) == 4 &&
      std::memcmp(&wrong_owner_value, &wrong_owner_sentinel,
                  sizeof(wrong_owner_value)) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation;

  StreamValue key_value{};
  StreamValue tangent_in{};
  StreamValue tangent_out{};
  ok = ok && get_key_value(
                   1, mask_stream, inserted_index, &key_value) == 0 &&
      get_tangents(1, mask_stream, inserted_index,
                   &tangent_in, &tangent_out) == 0;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && set_key_value(
                   mask_stream, inserted_index, &key_value) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && set_tangents(mask_stream, inserted_index,
                          &tangent_in, &tangent_out) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;
  KeyframeEase ease_in{12.0, 25.0};
  KeyframeEase ease_out{18.0, 75.0};
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && set_ease(mask_stream, inserted_index, 0,
                      &ease_in, &ease_out) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;
  const uint64_t before_invalid_ease = registry.fingerprint();
  KeyframeEase invalid_ease{1.0, 101.0};
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && set_ease(mask_stream, inserted_index, 0,
                      &invalid_ease, nullptr) == 4 &&
      registry.fingerprint() == before_invalid_ease &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && set_key_flag(
                   mask_stream, inserted_index, 1, 1) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && set_interpolation(
                   mask_stream, inserted_index, 2, 3) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && set_label(mask_stream, inserted_index, 9) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;

  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && delete_key(mask_stream, inserted_index) == 4 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation;
  ok = dispose_mask_value(&tangent_out) == 0 && ok;
  ok = dispose_mask_value(&tangent_in) == 0 && ok;
  ok = dispose_mask_value(&key_value) == 0 && ok;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && delete_key(mask_stream, inserted_index) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1 &&
      !registry.snapshot(key_identity, project_b_layer);

  const HostTime final_time{45, 30};
  int32_t final_index = -1;
  ok = ok && insert_key(mask_stream, 1, &final_time, &final_index) == 0;
  key = keyframe_at(find_stream(mask_stream), final_index);
  const auto child_before_dispose = key ? key->identity :
      aexcompat::scene_model::Identity{};
  ok = ok && dispose_mask_stream(mask_stream) == 0 &&
      !registry.snapshot(child_before_dispose, project_b_layer) &&
      dispose_mask_stream(mask_stream) == 4;

  void* dynamic_root = nullptr;
  void* dynamic_parade = nullptr;
  void* dynamic_atom = nullptr;
  void* dynamic_outline = nullptr;
  ok = ok && get_dynamic_for_layer(1, &g_layer, &dynamic_root) == 0 &&
      get_dynamic_by_index(1, dynamic_root, 0, &dynamic_parade) == 0 &&
      get_dynamic_by_index(1, dynamic_parade, 0, &dynamic_atom) == 0 &&
      get_dynamic_by_index(1, dynamic_atom, 0, &dynamic_outline) == 0;

  const std::size_t transactions_before_wrong_kind =
      g_add_keyframe_transactions.size();
  void* unchanged_transaction =
      reinterpret_cast<void*>(static_cast<uintptr_t>(0x1357));
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  const uint64_t registry_before_wrong_kind = registry.fingerprint();
  ok = ok && start_add(dynamic_root, &unchanged_transaction) == 4 &&
      unchanged_transaction ==
          reinterpret_cast<void*>(static_cast<uintptr_t>(0x1357)) &&
      start_add(dynamic_parade, &unchanged_transaction) == 4 &&
      unchanged_transaction ==
          reinterpret_cast<void*>(static_cast<uintptr_t>(0x1357)) &&
      g_add_keyframe_transactions.size() == transactions_before_wrong_kind &&
      registry.fingerprint() == registry_before_wrong_kind &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation;

  void* wrong_kind_transaction = nullptr;
  ok = ok && start_add(dynamic_outline, &wrong_kind_transaction) == 0;
  AddKeyframesTransaction* wrong_kind_record =
      find_add_transaction(wrong_kind_transaction);
  HostStreamRef* parade_record = find_stream(dynamic_parade);
  if (wrong_kind_record) wrong_kind_record->stream = parade_record;
  const HostTime wrong_kind_time{60, 30};
  int32_t wrong_kind_index = 0x12345678;
  ok = ok && wrong_kind_record && parade_record &&
      add_key(wrong_kind_transaction, 1, &wrong_kind_time,
              &wrong_kind_index) == 4 &&
      wrong_kind_index == 0x12345678 &&
      end_add(1, wrong_kind_transaction) == 4 &&
      g_add_keyframe_transactions.size() == transactions_before_wrong_kind &&
      add_key(wrong_kind_transaction, 1, &wrong_kind_time,
              &wrong_kind_index) == 4 &&
      end_add(1, wrong_kind_transaction) == 4 &&
      registry.fingerprint() == registry_before_wrong_kind &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation;

  void* invalid_terminal_transaction = nullptr;
  ok = ok && start_add(
                 dynamic_outline, &invalid_terminal_transaction) == 0 &&
      end_add(2, invalid_terminal_transaction) == 4 &&
      g_add_keyframe_transactions.size() == transactions_before_wrong_kind &&
      end_add(1, invalid_terminal_transaction) == 4 &&
      registry.fingerprint() == registry_before_wrong_kind &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation;

  void* rollback_transaction = nullptr;
  int32_t rollback_first_index = -1;
  int32_t rollback_second_index = -1;
  const HostTime rollback_first_time{61, 30};
  const HostTime rollback_second_time{62, 30};
  ok = ok && start_add(dynamic_outline, &rollback_transaction) == 0 &&
      add_key(rollback_transaction, 1, &rollback_first_time,
              &rollback_first_index) == 0 &&
      add_key(rollback_transaction, 1, &rollback_second_time,
              &rollback_second_index) == 0;
  const uint64_t scene_before_rollback = mask_scene_fingerprint();
  const uint64_t registry_before_rollback = registry.fingerprint();
  const uint64_t handles_before_rollback =
      registry.handle_table_fingerprint();
  const auto diagnostics_before_rollback =
      aexcompat::scene_transaction::diagnostics();
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  inject_keyframe_apply_failure_after(1);
  ok = ok && end_add(1, rollback_transaction) == 4 &&
      mask_scene_fingerprint() == scene_before_rollback &&
      registry.fingerprint() == registry_before_rollback &&
      registry.handle_table_fingerprint() == handles_before_rollback &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation &&
      g_add_keyframe_transactions.size() == transactions_before_wrong_kind &&
      aexcompat::scene_transaction::diagnostics().rolled_back ==
          diagnostics_before_rollback.rolled_back + 1 &&
      aexcompat::scene_transaction::diagnostics().rollback_failures ==
          diagnostics_before_rollback.rollback_failures &&
      add_key(rollback_transaction, 1, &rollback_first_time,
              &rollback_first_index) == 4 &&
      end_add(1, rollback_transaction) == 4;

  uint32_t dynamic_flags = 0;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  const uint64_t dynamic_before_failure = registry.fingerprint();
  ok = ok && set_dynamic_flag(dynamic_outline, 2, 2, 1) == 4 &&
      get_dynamic_flags(dynamic_outline, &dynamic_flags) == 0 &&
      dynamic_flags == 0 &&
      registry.fingerprint() == dynamic_before_failure &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && set_dynamic_flag(dynamic_outline, 2, 0, 1) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;
  const uint16_t dynamic_name[]{'I','s','s','u','e','2','6',0};
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && set_dynamic_name(dynamic_atom, dynamic_name) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;

  int32_t duplicate_index = 0x12345678;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && duplicate_dynamic(2, dynamic_atom, &duplicate_index) == 4 &&
      duplicate_index == 0x12345678 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && duplicate_dynamic(1, dynamic_atom, &duplicate_index) == 0 &&
      duplicate_index == 1 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && reorder_dynamic(dynamic_atom, 1) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;

  void* duplicated_atom = nullptr;
  ok = ok && get_dynamic_by_index(
                 1, dynamic_parade, 0, &duplicated_atom) == 0;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && delete_dynamic(duplicated_atom) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1 &&
      dispose_mask_stream(duplicated_atom) == 0;

  void* added_atom =
      reinterpret_cast<void*>(static_cast<uintptr_t>(0x2468));
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && add_dynamic(
                 1, dynamic_parade, "ADBE Not A Mask", &added_atom) == 4 &&
      added_atom == reinterpret_cast<void*>(static_cast<uintptr_t>(0x2468)) &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && add_dynamic(
                 1, dynamic_parade, "ADBE Mask Atom", &added_atom) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1;
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && delete_dynamic(added_atom) == 0 &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation + 1 &&
      dispose_mask_stream(added_atom) == 0;

  HostStreamRef* capacity_stream = find_stream(dynamic_outline);
  HostKeyframe* capacity_key = keyframe_at(capacity_stream, final_index);
  ok = ok && capacity_stream && capacity_key &&
      ensure_keyframe_identity(capacity_stream, capacity_key, final_index);

  std::array<StreamValue, kMaxCheckedStreamValues> reserved_map_values{};
  std::size_t reserved_map_count = 0;
  while (g_stream_values.size() < kMaxCheckedStreamValues - 1 &&
         reserved_map_count < reserved_map_values.size()) {
    const auto inserted = g_stream_values.emplace(
        &reserved_map_values[reserved_map_count], CheckedStreamValue{});
    if (!inserted.second) break;
    ++reserved_map_count;
  }
  StreamValue map_in_failure{};
  StreamValue map_out_failure{};
  std::memset(&map_in_failure, 0xa5, sizeof(map_in_failure));
  std::memset(&map_out_failure, 0x5a, sizeof(map_out_failure));
  const StreamValue map_in_sentinel = map_in_failure;
  const StreamValue map_out_sentinel = map_out_failure;
  const uint64_t registry_before_map_failure = registry.fingerprint();
  const uint32_t live_values_before_map_failure =
      capacity_stream ? capacity_stream->live_values : 0;
  ok = ok &&
      g_stream_values.size() == kMaxCheckedStreamValues - 1 &&
      get_tangents(1, dynamic_outline, final_index,
                   &map_in_failure, &map_out_failure) == 4 &&
      std::memcmp(
          &map_in_failure, &map_in_sentinel,
          sizeof(map_in_failure)) == 0 &&
      std::memcmp(
          &map_out_failure, &map_out_sentinel,
          sizeof(map_out_failure)) == 0 &&
      registry.fingerprint() == registry_before_map_failure &&
      capacity_stream->live_values == live_values_before_map_failure;
  for (std::size_t index = 0; index < reserved_map_count; ++index)
    g_stream_values.erase(&reserved_map_values[index]);

  std::vector<aexcompat::scene_model::Identity> capacity_fillers;
  while (capacity_key &&
         registry.can_create_children(capacity_key->identity, 2)) {
    aexcompat::scene_model::Identity filler{};
    if (!registry.create_child(
            aexcompat::scene_model::ObjectKind::value,
            capacity_key->identity,
            static_cast<int32_t>(capacity_fillers.size()), nullptr,
            u"Capacity Filler", filler)) {
      ok = false;
      break;
    }
    capacity_fillers.push_back(filler);
  }
  ok = ok && capacity_key &&
      registry.can_create_child(capacity_key->identity) &&
      !registry.can_create_children(capacity_key->identity, 2);

  StreamValue registry_in_failure{};
  StreamValue registry_out_failure{};
  std::memset(&registry_in_failure, 0xc3, sizeof(registry_in_failure));
  std::memset(&registry_out_failure, 0x3c, sizeof(registry_out_failure));
  const StreamValue registry_in_sentinel = registry_in_failure;
  const StreamValue registry_out_sentinel = registry_out_failure;
  const uint64_t registry_before_pair_failure = registry.fingerprint();
  const std::size_t values_before_pair_failure = g_stream_values.size();
  const uint32_t live_values_before_pair_failure =
      capacity_stream ? capacity_stream->live_values : 0;
  ok = ok && get_tangents(
                 1, dynamic_outline, final_index,
                 &registry_in_failure, &registry_out_failure) == 4 &&
      std::memcmp(
          &registry_in_failure, &registry_in_sentinel,
          sizeof(registry_in_failure)) == 0 &&
      std::memcmp(
          &registry_out_failure, &registry_out_sentinel,
          sizeof(registry_out_failure)) == 0 &&
      registry.fingerprint() == registry_before_pair_failure &&
      g_stream_values.size() == values_before_pair_failure &&
      capacity_stream->live_values == live_values_before_pair_failure;

  aexcompat::scene_model::Identity final_capacity_filler{};
  ok = ok && registry.create_child(
      aexcompat::scene_model::ObjectKind::value,
      capacity_key->identity,
      static_cast<int32_t>(capacity_fillers.size()), nullptr,
      u"Final Capacity Filler", final_capacity_filler);
  void* failed_commit_transaction = nullptr;
  int32_t failed_commit_index = 0x12345678;
  const HostTime failed_commit_time{90, 30};
  ok = ok && start_add(
                 dynamic_outline, &failed_commit_transaction) == 0 &&
      add_key(failed_commit_transaction, 1, &failed_commit_time,
              &failed_commit_index) == 0;
  const std::size_t keys_before_failed_commit =
      capacity_stream->mask->keyframes.size();
  const uint64_t registry_before_failed_commit = registry.fingerprint();
  generation = aexcompat::aegp_external_render_runtime::project_generation();
  ok = ok && end_add(1, failed_commit_transaction) == 4 &&
      g_add_keyframe_transactions.size() == transactions_before_wrong_kind &&
      capacity_stream->mask->keyframes.size() == keys_before_failed_commit &&
      registry.fingerprint() == registry_before_failed_commit &&
      aexcompat::aegp_external_render_runtime::project_generation() ==
          generation &&
      add_key(failed_commit_transaction, 1, &failed_commit_time,
              &failed_commit_index) == 4 &&
      end_add(1, failed_commit_transaction) == 4;

  ok = dispose_mask_stream(dynamic_outline) == 0 && ok;
  ok = dispose_mask_stream(dynamic_atom) == 0 && ok;
  ok = dispose_mask_stream(dynamic_parade) == 0 && ok;
  ok = dispose_mask_stream(dynamic_root) == 0 && ok;
  ok = dispose_mask_fn(mask) == 0 && ok;

  ok = release_suite("AEGP Dynamic Stream Suite", 5) == 0 && ok;
  ok = release_suite("AEGP Keyframe Suite", 5) == 0 && ok;
  ok = release_suite("AEGP Stream Suite", 11) == 0 && ok;
  ok = release_suite("AEGP Layer Mask Suite", 7) == 0 && ok;
  g_mask_scene.clear();
  aexcompat::mask_runtime::set_model_enabled(saved_mask_model_enabled);
  g_aegp_effect_instances = saved_instances;
  g_aegp_effect_leases = saved_leases;
  g_aegp_legacy_effect_streams = saved_streams;
  g_aegp_transform_stream = saved_transform;
  g_aegp_comp_idle_roundtrip_mode = saved_mode;
  return ok && suite_leases_balanced();
}

// The parameters a loaded plug-in's effect handle answers with (issue #909).
//
// Slot 0 of the effect instance table is seeded at scene start with the probe
// fixture's key, and `AEGP_GetNewEffectForEffect` hands that same slot to the
// plug-in this worker loaded. Deciding whose parameters to answer from that
// key alone answered a real plug-in out of a five-entry fixture table; the
// instance carries a mark instead. Everything below is a property that broke
// at some point while getting there.
bool verify_aegp_loaded_plugin_effect_streams() {
  if (!g_hooks.get_new_effect_for_effect || !g_hooks.dispose_effect ||
      !g_hooks.get_effect_num_param_streams_v2 || !g_hooks.get_new_effect_stream_v2 ||
      !g_hooks.get_stream_name_v2 || !g_hooks.get_stream_type_v2 ||
      !g_hooks.get_new_stream_value_v2 || !g_hooks.dispose_stream_value_v2 ||
      !g_hooks.dispose_stream_v2 || !g_hooks.effect)
    return false;
  using aexcompat::worker_runtime::parameters::ParamRecord;
  auto& records = aexcompat::worker_runtime::parameters::state().records;
  const auto saved_records = records;
  const auto saved_instances = scene_runtime_state().effect_instances;
  const auto saved_streams = scene_runtime_state().legacy_effect_streams;
  const auto saved_leases = scene_runtime_state().effect_leases;
  const bool saved_effect_live = scene_runtime_state().effect_live;
  // Every stream and value this test opens has to be handed back. The suite
  // lease balance says nothing about that - this test acquires no suite - so
  // the counters the stream paths keep are what closes the loop, the way the
  // Levels projector test beside it does it.
  const uint32_t streams_before = g_aegp_stream_acquires;
  const uint32_t stream_disposes_before = g_aegp_stream_disposes;
  const uint32_t values_before = g_aegp_stream_value_acquires;
  const uint32_t value_disposes_before = g_aegp_stream_value_disposes;

  records.clear();
  ParamRecord slider{};
  slider.index = 1;
  slider.type = 10;  // PF_Param_FLOAT_SLIDER
  slider.name = "Amount";
  slider.default_value = 12.5;
  records.push_back(slider);
  ParamRecord matte{};
  matte.index = 2;
  matte.type = 0;  // PF_Param_LAYER
  matte.name = "Matte";
  records.push_back(matte);
  scene_runtime_state().effect_live = false;

  void* effect = nullptr;
  bool ok = g_hooks.get_new_effect_for_effect(0, g_hooks.effect, &effect) == 0 &&
      effect == &scene_runtime_state().effect;

  // Counted the way AE counts: the input layer plus one per declared
  // parameter. Keyed on the fixture instead, this answered 5.
  int32_t count = -1;
  ok = ok && g_hooks.get_effect_num_param_streams_v2(effect, &count) == 0 && count == 3;

  // Index 0 is the input layer, which has no record of its own and still
  // opens, as a layer stream.
  void* input = nullptr;
  char name[32]{};
  int32_t type = -1;
  ok = ok && g_hooks.get_new_effect_stream_v2(0, effect, 0, &input) == 0 && input &&
      g_hooks.get_stream_name_v2(input, 1, name) == 0 && std::strcmp(name, "Input") == 0 &&
      g_hooks.get_stream_type_v2(input, &type) == 0 && type == 9;

  // The declared parameters, by their own names and with their PF types
  // translated to AEGP stream types: FLOAT_SLIDER 10 -> OneD 5, LAYER 0 ->
  // LAYER_ID 9. Handing the PF value straight through left every stream
  // reading as no-data and refusing to open.
  void* amount = nullptr;
  ok = ok && g_hooks.get_new_effect_stream_v2(0, effect, 1, &amount) == 0 && amount &&
      g_hooks.get_stream_name_v2(amount, 1, name) == 0 && std::strcmp(name, "Amount") == 0 &&
      g_hooks.get_stream_type_v2(amount, &type) == 0 && type == 5;

  // Past the declared parameters there is nothing to open.
  void* past = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && g_hooks.get_new_effect_stream_v2(0, effect, 3, &past) != 0 &&
      past == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));

  // The value reads as zero: `parameter_values` holds the probe fixture's
  // defaults, which are not this plug-in's (issue #929).
  scene_runtime::AegpStreamValue value{};
  suite_abi::AegpTime time{0, 30};
  std::array<double, 4> scalars{-1.0, -1.0, -1.0, -1.0};
  ok = ok && g_hooks.get_new_stream_value_v2(0, amount, 1, &time, 1, &value) == 0;
  if (ok) std::memcpy(scalars.data(), value.value.data(), sizeof(scalars));
  ok = ok && scalars[0] == 0.0;
  // Non-short-circuiting, like the stream disposes below: the check that
  // can fail just above it is the one that leaves a value checked out,
  // and skipping the dispose there would leave the stream undisposable
  // and its borrow spent.
  ok = g_hooks.dispose_stream_value_v2(&value) == 0 && ok;

  // A null stream handle is refused, not dereferenced.
  scene_runtime::AegpStreamValue unused{};
  ok = ok && g_hooks.get_new_stream_value_v2(0, nullptr, 1, &time, 1, &unused) != 0;
  // So is a negative caller id.
  ok = ok && g_hooks.get_new_stream_value_v2(-1, amount, 1, &time, 1, &unused) != 0;

  // A stream opened under an id and read under 0. This is what an
  // unregistered plug-in does - it has no id of its own to give back, and 0
  // is not a claim to be anyone - and requiring the read to name the open's
  // id refused it, ending the frame DeepGlow2 had opened its matte stream to
  // answer (issue #958). A different id is still refused, which is the half
  // `verify_aegp_effect_stack` pins; without this half, restoring the strict
  // rule leaves every self-test green.
  void* under_seven = nullptr;
  scene_runtime::AegpStreamValue borrowed{};
  ok = ok && g_hooks.get_new_effect_stream_v2(7, effect, 1, &under_seven) == 0 &&
      under_seven &&
      g_hooks.get_new_stream_value_v2(8, under_seven, 1, &time, 1, &unused) != 0 &&
      g_hooks.get_new_stream_value_v2(0, under_seven, 1, &time, 1, &borrowed) == 0;
  ok = g_hooks.dispose_stream_value_v2(&borrowed) == 0 && ok;
  ok = g_hooks.dispose_stream_v2(under_seven) == 0 && ok;

  // Disposing the effect handle does not change what its streams answer:
  // they outlive it, and the mark belongs to the instance.
  ok = ok && g_hooks.dispose_effect(effect) == 0 &&
      g_hooks.get_stream_name_v2(amount, 1, name) == 0 &&
      std::strcmp(name, "Amount") == 0 &&
      g_hooks.get_stream_type_v2(amount, &type) == 0 && type == 5;

  ok = g_hooks.dispose_stream_v2(amount) == 0 && ok;
  ok = g_hooks.dispose_stream_v2(input) == 0 && ok;

  // Restored whether or not the checks passed: a failed run must not leave the
  // scene holding this test's streams and leases, the way the mutation
  // transaction test beside it restores the same three tables.
  records = saved_records;
  scene_runtime_state().effect_instances = saved_instances;
  scene_runtime_state().legacy_effect_streams = saved_streams;
  scene_runtime_state().effect_leases = saved_leases;
  scene_runtime_state().effect_live = saved_effect_live;
  return ok &&
      g_aegp_stream_acquires - streams_before ==
          g_aegp_stream_disposes - stream_disposes_before &&
      g_aegp_stream_value_acquires - values_before ==
          g_aegp_stream_value_disposes - value_disposes_before;
}

bool verify_aegp_installed_effect_catalog_suite4() {
  const bool saved_comp_idle_mode = g_aegp_comp_idle_roundtrip_mode;
  g_aegp_comp_idle_roundtrip_mode = true;
  const void* acquired = nullptr;
  bool ok = acquire_suite("AEGP Effect Suite", 4, &acquired) == 0 &&
            acquired == g_aegp_effect_suite4.data();
  int32_t count = -1;
  ok = ok && aegp_get_num_installed_effects(&count) == 0 &&
       count == static_cast<int32_t>(kAegpInstalledEffects.size());
  count = 0x12345678;
  ok = ok && aegp_get_num_installed_effects(nullptr) == 4 && count == 0x12345678;

  int32_t key = -1;
  ok = ok && aegp_get_next_installed_effect(kAegpInstalledEffectKeyNone, &key) == 0 &&
       key == kAegpInstalledEffects[0].key;
  const int32_t installed_key = key;
  for (std::size_t index = 1; index < kAegpInstalledEffects.size(); ++index)
    ok = ok && aegp_get_next_installed_effect(key, &key) == 0 &&
         key == kAegpInstalledEffects[index].key;
  ok = ok && aegp_get_next_installed_effect(key, &key) == 0 &&
       key == kAegpInstalledEffectKeyNone;
  key = 0x12345678;
  ok = ok && aegp_get_next_installed_effect(9999, &key) == 4 &&
       key == 0x12345678 &&
       aegp_get_next_installed_effect(kAegpInstalledEffectKeyNone, nullptr) == 4;

  std::array<char, kAegpMaxEffectCategoryNameSize> name{};
  std::array<char, kAegpMaxEffectCategoryNameSize> match_name{};
  std::array<char, kAegpMaxEffectCategoryNameSize> category{};
  ok = ok && aegp_get_effect_name(installed_key, name.data()) == 0 &&
       std::strcmp(name.data(), kAegpInstalledEffects[0].name) == 0 &&
       aegp_get_effect_match_name(installed_key, match_name.data()) == 0 &&
       std::strcmp(match_name.data(), kAegpInstalledEffects[0].match_name) == 0 &&
       aegp_get_effect_category(installed_key, category.data()) == 0 &&
       std::strcmp(category.data(), kAegpInstalledEffects[0].category) == 0 &&
       category[std::strlen(kAegpInstalledEffects[0].category)] == '\0';
  category.fill('Z');
  ok = ok && aegp_get_effect_category(9999, category.data()) == 4 &&
       std::all_of(category.begin(), category.end(), [](char value) { return value == 'Z'; }) &&
       aegp_get_effect_category(installed_key, nullptr) == 4;
  g_aegp_comp_idle_roundtrip_mode = saved_comp_idle_mode;
  return ok;
}

AegpSceneModelSelftestReport verify_aegp_scene_model() {
  using scene_model::Identity;
  using scene_model::ObjectKind;
  using aegp_staged_item_runtime::OrderedSceneEffect;
  using aegp_staged_item_runtime::SamplingPolicy;
  using aegp_staged_item_runtime::StageKind;

  AegpSceneModelSelftestReport report{};
  auto& registry = scene_model::registry();
  const bool saved_comp_idle_mode = g_aegp_comp_idle_roundtrip_mode;
  g_aegp_comp_idle_roundtrip_mode = true;
  report.two_projects = registry.project_count() == 2;
  if (!report.two_projects) return report;

  aegp_staged_item_runtime::clear();
  scene_model::ObjectSnapshot active_item{}, active_comp{}, active_layer{},
      root{}, child_item{}, project_b{}, root_b{}, item_b{};
  const Identity active = registry.active_item();
  bool ok = registry.snapshot(active, active_item) &&
      registry.comp_from_item(active, active_comp) &&
      registry.layer_by_index(active_comp.identity, 0, active_layer) &&
      registry.snapshot(active_item.owner, root) &&
      registry.child_by_index(root.identity, ObjectKind::item, 1, child_item) &&
      registry.project_identity(2, project_b.identity) &&
      registry.first_child(project_b.identity, root_b) &&
      registry.first_child(root_b.identity, item_b);
  report.fixture_lookup = ok;

  const void* effect_suite_raw = nullptr;
  using ApplyEffect = int32_t(__cdecl*)(int32_t, void*, int32_t, void**);
  using DeleteEffect = int32_t(__cdecl*)(void*);
  report.effect_suite_acquired =
      compat_acquire_suite("AEGP Effect Suite", 4,
                           &effect_suite_raw) == 0;
  ok = ok && report.effect_suite_acquired;
  const auto* effect_slots =
      static_cast<void* const*>(const_cast<void*>(effect_suite_raw));
  const auto apply_effect = effect_slots
      ? reinterpret_cast<ApplyEffect>(effect_slots[9]) : nullptr;
  const auto delete_effect = effect_slots
      ? reinterpret_cast<DeleteEffect>(effect_slots[10]) : nullptr;
  using UnsupportedSlot = int32_t(__cdecl*)();
  const auto unsupported_slot = effect_slots
      ? reinterpret_cast<UnsupportedSlot>(effect_slots[7]) : nullptr;
  const auto unsupported_before =
      worker_runtime::suite_registry()
          .unsupported_suite_calls_report_json();
  const auto invalid_before_unsupported =
      aegp_staged_item_runtime::diagnostics().invalid_handle_rejections;
  report.unsupported_error =
      unsupported_slot ? unsupported_slot() : 0;
  const auto unsupported_after =
      worker_runtime::suite_registry()
          .unsupported_suite_calls_report_json();
  const auto invalid_after_unsupported =
      aegp_staged_item_runtime::diagnostics().invalid_handle_rejections;
  const uint32_t unsupported_count_before =
      unsupported_effect_slot_seven_count(unsupported_before);
  const uint32_t unsupported_count_after =
      unsupported_effect_slot_seven_count(unsupported_after);
  report.unsupported_diagnostic_observed =
      unsupported_count_before != std::numeric_limits<uint32_t>::max() &&
      unsupported_count_after == unsupported_count_before + 1u;
  report.unsupported_call_count =
      report.unsupported_diagnostic_observed ? 1u : 0u;
  report.unsupported_distinct_from_invalid_handle =
      invalid_after_unsupported == invalid_before_unsupported;
  report.unsupported_slots_preserved =
      report.unsupported_error == 4 &&
      report.unsupported_diagnostic_observed &&
      report.unsupported_distinct_from_invalid_handle;
  void* applied_effect = nullptr;
  report.effect_applied = apply_effect && delete_effect &&
      apply_effect(7, active_layer.legacy_handle,
                   kAegpInstalledEffects[0].key, &applied_effect) == 0;
  ok = ok && report.effect_applied;

  if (report.effect_applied &&
      aegp_set_effect_flags(applied_effect, 3u, 0u) == 0) {
    std::atomic<int32_t> first_result{4};
    std::atomic<int32_t> second_result{4};
    bool both_waiting = false;
    std::unique_lock<std::mutex> held(
        aexcompat::scene_transaction::mutation_mutex());
    const uint64_t waiting_before =
        aexcompat::scene_transaction::waiting_mutations();
    std::thread first([&]() {
      first_result.store(
          aegp_set_effect_flags(applied_effect, 1u, 1u));
    });
    std::thread second([&]() {
      second_result.store(
          aegp_set_effect_flags(applied_effect, 2u, 2u));
    });
    const auto deadline =
        std::chrono::steady_clock::now() + std::chrono::seconds(2);
    while (aexcompat::scene_transaction::waiting_mutations() <
               waiting_before + 2 &&
           std::chrono::steady_clock::now() < deadline)
      std::this_thread::yield();
    both_waiting =
        aexcompat::scene_transaction::waiting_mutations() >=
        waiting_before + 2;
    held.unlock();
    first.join();
    second.join();
    uint32_t combined_flags = 0;
    report.concurrent_effect_flags_serialized =
        both_waiting && first_result.load() == 0 &&
        second_result.load() == 0 &&
        aegp_get_effect_flags(applied_effect, &combined_flags) == 0 &&
        combined_flags == 3u;
  }
  ok = ok && report.concurrent_effect_flags_serialized;

  std::vector<OrderedSceneEffect> effects;
  for (const auto& instance : g_aegp_effect_instances) {
    if (!instance.occupied || instance.layer != active_layer.legacy_handle)
      continue;
    scene_model::ObjectSnapshot effect_snapshot{};
    if (!registry.snapshot(instance.identity, effect_snapshot)) {
      ok = false;
      break;
    }
    effects.push_back(
        {instance.identity, static_cast<uint32_t>(instance.stack_order)});
  }
  std::sort(effects.begin(), effects.end(),
            [](const auto& left, const auto& right) {
              return left.order < right.order;
            });
  report.effect_count = static_cast<uint32_t>(effects.size());
  report.effect_order = !effects.empty();
  for (std::size_t index = 0; index < effects.size(); ++index)
    report.effect_order = report.effect_order &&
        effects[index].order == index;

  ok = ok && report.effect_order &&
      aegp_staged_item_runtime::register_scene_item(
          registry, child_item.identity, SamplingPolicy::exact,
          nullptr, 0, nullptr, 0) &&
      aegp_staged_item_runtime::register_scene_item(
          registry, active, SamplingPolicy::exact, &child_item.identity, 1,
          effects.data(), effects.size());
  report.typed_identity = ok;

  report.cross_project_cycle_rejected =
      !aegp_staged_item_runtime::register_scene_item(
          registry, active, SamplingPolicy::exact, &item_b.identity, 1,
          effects.data(), effects.size());
  report.direct_cycle_rejected =
      !aegp_staged_item_runtime::register_scene_item(
          registry, child_item.identity, SamplingPolicy::exact,
          &child_item.identity, 1, nullptr, 0);
  report.indirect_cycle_rejected =
      !aegp_staged_item_runtime::register_scene_item(
          registry, child_item.identity, SamplingPolicy::exact,
          &active, 1, nullptr, 0);

  constexpr int32_t width = 2;
  constexpr int32_t height = 1;
  std::array<std::byte, width * height * 4> pixels{{
      std::byte{255}, std::byte{17}, std::byte{29}, std::byte{41},
      std::byte{255}, std::byte{53}, std::byte{67}, std::byte{79}}};
  const suite_abi::AegpTime time{0, 30};
  const suite_abi::AegpTime step{1, 30};
  uint64_t stage_hash = 0;
  ok = ok && aegp_staged_item_runtime::publish_scene_stage_world(
      registry, child_item.identity, StageKind::final_item, {}, time, step,
      1, 0, world_registry::kPixelFormatArgb32, width, height, width * 4,
      pixels.data());
  for (const auto& effect : effects) {
    for (const auto kind : {StageKind::upstream, StageKind::all_effects,
                            StageKind::downstream}) {
      ok = ok && aegp_staged_item_runtime::publish_scene_stage_world(
          registry, active, kind, effect.identity, time, step, 1, 0,
          world_registry::kPixelFormatArgb32, width, height, width * 4,
          pixels.data(), &stage_hash);
    }
  }
  ok = ok && aegp_staged_item_runtime::publish_scene_stage_world(
      registry, active, StageKind::final_item, {}, time, step, 1, 0,
      world_registry::kPixelFormatArgb32, width, height, width * 4,
      pixels.data(), &stage_hash);
  report.stage_identity_hash = stage_hash;

  render_options::ItemValue request{};
  request.item = active_item.legacy_handle;
  request.time = time;
  request.time_step = step;
  request.world_type = 1;
  request.render_quality = 1;
  void* receipt = nullptr;
  render_receipts::ReceiptSnapshot receipt_snapshot{};
  ok = ok &&
      aegp_staged_item_runtime::publish_registered_receipt(
          request, &receipt) == 0 &&
      receipt && render_receipts::snapshot(receipt, receipt_snapshot) &&
      receipt_snapshot.scene_bound &&
      receipt_snapshot.scene_item == active &&
      receipt_snapshot.scene_project.project_id == active.project_id &&
      receipt_snapshot.dependency_identity_hash != 0 &&
      receipt_snapshot.effect_order_hash != 0;
  report.trace_hash = receipt_snapshot.trace_hash;
  report.dependency_identity_hash =
      receipt_snapshot.dependency_identity_hash;
  report.effect_order_hash = receipt_snapshot.effect_order_hash;

  const auto scene_before_duplicate_order = g_aegp_effect_instances;
  const uint64_t registry_before_duplicate_order = registry.fingerprint();
  const auto scheduler_before_duplicate_order =
      aegp_staged_item_runtime::diagnostics();
  const auto receipts_before_duplicate_order =
      render_receipts::statistics();
  const auto receipt_before_duplicate_order = receipt_snapshot;
  std::array<std::size_t, 2> same_layer_effects{};
  std::size_t same_layer_count = 0;
  for (std::size_t index = 0;
       index < g_aegp_effect_instances.size() && same_layer_count < 2;
       ++index) {
    const auto& instance = g_aegp_effect_instances[index];
    if (instance.occupied &&
        instance.layer == active_layer.legacy_handle)
      same_layer_effects[same_layer_count++] = index;
  }
  bool duplicate_order_scene_unchanged = false;
  if (same_layer_count == 2) {
    g_aegp_effect_instances[same_layer_effects[1]].stack_order =
        g_aegp_effect_instances[same_layer_effects[0]].stack_order;
    const auto malformed_scene = g_aegp_effect_instances;
    report.duplicate_effect_order_rejected =
        !prepare_scene_staged_item(active_item.legacy_handle);
    duplicate_order_scene_unchanged =
        std::memcmp(g_aegp_effect_instances.data(),
                    malformed_scene.data(),
                    sizeof(malformed_scene)) == 0;
  }
  g_aegp_effect_instances = scene_before_duplicate_order;
  const auto scheduler_after_duplicate_order =
      aegp_staged_item_runtime::diagnostics();
  const auto receipts_after_duplicate_order =
      render_receipts::statistics();
  render_receipts::ReceiptSnapshot receipt_after_duplicate_order{};
  report.duplicate_order_state_unchanged =
      same_layer_count == 2 &&
      duplicate_order_scene_unchanged &&
      std::memcmp(g_aegp_effect_instances.data(),
                  scene_before_duplicate_order.data(),
                  sizeof(scene_before_duplicate_order)) == 0 &&
      registry.fingerprint() == registry_before_duplicate_order &&
      scheduler_after_duplicate_order.published ==
          scheduler_before_duplicate_order.published &&
      scheduler_after_duplicate_order.registered_items ==
          scheduler_before_duplicate_order.registered_items &&
      scheduler_after_duplicate_order.cached_stages ==
          scheduler_before_duplicate_order.cached_stages &&
      scheduler_after_duplicate_order.cached_bytes ==
          scheduler_before_duplicate_order.cached_bytes &&
      receipts_after_duplicate_order.created ==
          receipts_before_duplicate_order.created &&
      receipts_after_duplicate_order.live_count ==
          receipts_before_duplicate_order.live_count &&
      receipts_after_duplicate_order.live_bytes ==
          receipts_before_duplicate_order.live_bytes;
  report.duplicate_order_receipt_unchanged =
      render_receipts::snapshot(receipt, receipt_after_duplicate_order) &&
      receipt_after_duplicate_order.stage_identity_hash ==
          receipt_before_duplicate_order.stage_identity_hash &&
      receipt_after_duplicate_order.trace_hash ==
          receipt_before_duplicate_order.trace_hash &&
      receipt_after_duplicate_order.project_generation ==
          receipt_before_duplicate_order.project_generation &&
      receipt_after_duplicate_order.dependency_identity_hash ==
          receipt_before_duplicate_order.dependency_identity_hash &&
      receipt_after_duplicate_order.effect_order_hash ==
          receipt_before_duplicate_order.effect_order_hash;
  ok = ok && report.duplicate_effect_order_rejected &&
      report.duplicate_order_state_unchanged &&
      report.duplicate_order_receipt_unchanged;

  report.duplicate_stable_id_rejected =
      !aegp_staged_item_runtime::register_item(
          reinterpret_cast<void*>(static_cast<uintptr_t>(0x26f0)),
          receipt_snapshot.item_identity, SamplingPolicy::exact,
          nullptr, 0, nullptr, 0);
  Identity stale_item = active;
  ++stale_item.generation;
  report.pointer_id_mismatch_rejected =
      !aegp_staged_item_runtime::register_scene_item(
          registry, stale_item, SamplingPolicy::exact,
          nullptr, 0, effects.data(), effects.size());

  report.project_generation_before =
      aegp_external_render_runtime::project_generation();
  const auto scheduler_before =
      aegp_staged_item_runtime::diagnostics();
  const auto receipt_stats_before = render_receipts::statistics();
  const bool saved_mask_model = mask_runtime::model_enabled();
  const bool configured_mask = configure_mask_scene("rectangle");
  mask_runtime::set_model_enabled(true);
  g_aegp_comp_idle_roundtrip_mode = false;
  const void* mask_suite_raw = nullptr;
  const void* stream_suite_raw = nullptr;
  const void* keyframe_suite_raw = nullptr;
  using GetMask = int32_t(__cdecl*)(void*, int32_t, void**);
  using DisposeMask = int32_t(__cdecl*)(void*);
  using GetMaskStream = int32_t(__cdecl*)(int32_t, void*, int32_t, void**);
  using DisposeStream = int32_t(__cdecl*)(void*);
  using GetStreamValue = int32_t(__cdecl*)(
      int32_t, void*, int32_t, const HostTime*, int32_t, StreamValue*);
  using SetStreamValue = int32_t(__cdecl*)(
      int32_t, void*, StreamValue*);
  using DisposeStreamValue = int32_t(__cdecl*)(StreamValue*);
  using InsertKeyframe = int32_t(__cdecl*)(
      void*, int16_t, const HostTime*, int32_t*);
  using DeleteKeyframe = int32_t(__cdecl*)(void*, int32_t);
  bool mask_commit = configured_mask &&
      compat_acquire_suite("AEGP Layer Mask Suite", 7,
                           &mask_suite_raw) == 0 &&
      compat_acquire_suite("AEGP Stream Suite", 11,
                           &stream_suite_raw) == 0 &&
      compat_acquire_suite("AEGP Keyframe Suite", 5,
                           &keyframe_suite_raw) == 0;
  const auto* mask_slots =
      static_cast<void* const*>(const_cast<void*>(mask_suite_raw));
  const auto* stream_slots =
      static_cast<void* const*>(const_cast<void*>(stream_suite_raw));
  const auto* key_slots =
      static_cast<void* const*>(const_cast<void*>(keyframe_suite_raw));
  const auto get_mask = mask_slots
      ? reinterpret_cast<GetMask>(mask_slots[1]) : nullptr;
  const auto dispose_mask = mask_slots
      ? reinterpret_cast<DisposeMask>(mask_slots[2]) : nullptr;
  const auto get_mask_stream = stream_slots
      ? reinterpret_cast<GetMaskStream>(stream_slots[6]) : nullptr;
  const auto dispose_stream = stream_slots
      ? reinterpret_cast<DisposeStream>(stream_slots[7]) : nullptr;
  const auto get_stream_value = stream_slots
      ? reinterpret_cast<GetStreamValue>(stream_slots[13]) : nullptr;
  const auto dispose_stream_value = stream_slots
      ? reinterpret_cast<DisposeStreamValue>(stream_slots[14]) : nullptr;
  const auto set_stream_value = stream_slots
      ? reinterpret_cast<SetStreamValue>(stream_slots[15]) : nullptr;
  const auto insert_key = key_slots
      ? reinterpret_cast<InsertKeyframe>(key_slots[2]) : nullptr;
  const auto delete_key = key_slots
      ? reinterpret_cast<DeleteKeyframe>(key_slots[3]) : nullptr;
  void* mask = nullptr;
  void* stream = nullptr;
  int32_t inserted_index = -1;
  const HostTime inserted_time{77, 30};
  mask_commit = mask_commit && get_mask && dispose_mask &&
      get_mask_stream && dispose_stream && get_stream_value &&
      dispose_stream_value && set_stream_value && insert_key && delete_key &&
      get_mask(g_hooks.pf_layer, 0, &mask) == 0 &&
      get_mask_stream(1, mask, 400, &stream) == 0 &&
      insert_key(stream, 1, &inserted_time, &inserted_index) == 0;
  report.project_generation_after =
      aegp_external_render_runtime::project_generation();
  report.mask_fixture = mask_commit;
  const auto scheduler_after =
      aegp_staged_item_runtime::diagnostics();
  const auto receipt_stats_after = render_receipts::statistics();
  report.stage_invalidated = mask_commit &&
      report.project_generation_after ==
          report.project_generation_before + 1 &&
      scheduler_after.stale_stage_invalidations >
          scheduler_before.stale_stage_invalidations &&
      scheduler_after.cached_stages == 0;
  report.receipt_invalidated =
      !render_receipts::snapshot(receipt, receipt_snapshot) &&
      receipt_stats_after.stale_invalidations >
          receipt_stats_before.stale_invalidations;
  void** unchanged_world =
      reinterpret_cast<void**>(static_cast<uintptr_t>(0x2610));
  report.invalid_handle_distinguished =
      render_receipts::get_world(receipt, &unchanged_world) != 0 &&
      unchanged_world == nullptr &&
      render_receipts::statistics().invalid_handle_operations >
          receipt_stats_after.invalid_handle_operations;

  const bool removed_fixture_key =
      mask_commit && delete_key(stream, inserted_index) == 0;
  void* opacity_stream = nullptr;
  void* expansion_stream = nullptr;
  StreamValue opacity_value{};
  StreamValue expansion_value{};
  const HostTime stream_value_time{0, 30};
  bool mask_stream_setup = removed_fixture_key &&
      get_mask_stream(1, mask, 401, &opacity_stream) == 0 &&
      get_mask_stream(1, mask, 403, &expansion_stream) == 0 &&
      get_stream_value(
          1, opacity_stream, 1, &stream_value_time, 0,
          &opacity_value) == 0 &&
      get_stream_value(
          1, expansion_stream, 1, &stream_value_time, 0,
          &expansion_value) == 0;
  if (mask_stream_setup) {
    opacity_value.one_d = 63.0;
    expansion_value.one_d = 12.5;
    std::atomic<int32_t> opacity_result{4};
    std::atomic<int32_t> expansion_result{4};
    bool both_waiting = false;
    std::unique_lock<std::mutex> held(
        aexcompat::scene_transaction::mutation_mutex());
    const uint64_t waiting_before =
        aexcompat::scene_transaction::waiting_mutations();
    std::thread opacity_setter([&]() {
      opacity_result.store(
          set_stream_value(1, opacity_stream, &opacity_value));
    });
    std::thread expansion_setter([&]() {
      expansion_result.store(
          set_stream_value(1, expansion_stream, &expansion_value));
    });
    const auto deadline =
        std::chrono::steady_clock::now() + std::chrono::seconds(2);
    while (aexcompat::scene_transaction::waiting_mutations() <
               waiting_before + 2 &&
           std::chrono::steady_clock::now() < deadline)
      std::this_thread::yield();
    both_waiting =
        aexcompat::scene_transaction::waiting_mutations() >=
        waiting_before + 2;
    held.unlock();
    opacity_setter.join();
    expansion_setter.join();
    const HostMask* mask_record = find_mask(mask);
    report.concurrent_mask_streams_serialized =
        both_waiting && opacity_result.load() == 0 &&
        expansion_result.load() == 0 && mask_record &&
        mask_record->opacity == 63.0 &&
        mask_record->expansion == 12.5;
  }
  if (opacity_value.stream)
    ok = dispose_stream_value(&opacity_value) == 0 && ok;
  if (expansion_value.stream)
    ok = dispose_stream_value(&expansion_value) == 0 && ok;
  if (opacity_stream)
    ok = dispose_stream(opacity_stream) == 0 && ok;
  if (expansion_stream)
    ok = dispose_stream(expansion_stream) == 0 && ok;
  ok = ok && report.concurrent_mask_streams_serialized;

  int32_t baseline_key_index = -1;
  const HostTime baseline_key_time{10, 30};
  const bool keyframe_setup = removed_fixture_key &&
      insert_key(
          stream, 1, &baseline_key_time, &baseline_key_index) == 0 &&
      baseline_key_index == 0;
  if (keyframe_setup) {
    const HostTime later_key_time{7, 30};
    const HostTime earlier_key_time{5, 30};
    int32_t later_key_index = -1;
    int32_t earlier_key_index = -1;
    std::atomic<int32_t> later_result{4};
    std::atomic<int32_t> earlier_result{4};
    bool both_waiting = false;
    std::unique_lock<std::mutex> held(
        aexcompat::scene_transaction::mutation_mutex());
    const uint64_t waiting_before =
        aexcompat::scene_transaction::waiting_mutations();
    std::thread later_inserter([&]() {
      later_result.store(
          insert_key(
              stream, 1, &later_key_time, &later_key_index));
    });
    std::thread earlier_inserter([&]() {
      earlier_result.store(
          insert_key(
              stream, 1, &earlier_key_time, &earlier_key_index));
    });
    const auto deadline =
        std::chrono::steady_clock::now() + std::chrono::seconds(2);
    while (aexcompat::scene_transaction::waiting_mutations() <
               waiting_before + 2 &&
           std::chrono::steady_clock::now() < deadline)
      std::this_thread::yield();
    both_waiting =
        aexcompat::scene_transaction::waiting_mutations() >=
        waiting_before + 2;
    held.unlock();
    later_inserter.join();
    earlier_inserter.join();
    HostStreamRef* stream_record = find_stream(stream);
    bool sorted = stream_record &&
        stream_record->mask->keyframes.size() == 3;
    if (sorted) {
      auto key = stream_record->mask->keyframes.begin();
      sorted = time_equal(key->time, earlier_key_time);
      ++key;
      sorted = sorted && time_equal(key->time, later_key_time);
      ++key;
      sorted = sorted && time_equal(key->time, baseline_key_time);
    }
    bool indexed = sorted;
    if (stream_record) {
      int32_t key_index = 0;
      for (const auto& keyframe : stream_record->mask->keyframes) {
        if (!indexed) break;
        scene_model::ObjectSnapshot key_snapshot{};
        indexed = registry.snapshot(
                      keyframe.identity, key_snapshot) &&
            key_snapshot.owner == stream_record->identity &&
            key_snapshot.local_index == key_index;
        ++key_index;
      }
    }
    report.concurrent_keyframe_inserts_serialized =
        both_waiting && later_result.load() == 0 &&
        earlier_result.load() == 0 && sorted && indexed;
  }
  ok = ok && report.concurrent_keyframe_inserts_serialized;

  Identity direct_project{};
  std::unique_ptr<render_receipts::ReceiptDraft> direct_draft;
  try {
    direct_draft = std::make_unique<render_receipts::ReceiptDraft>();
    direct_draft->pixels.resize(4);
  } catch (...) {
    direct_draft.reset();
  }
  void* direct_receipt = nullptr;
  const uint32_t direct_generation_before =
      aegp_external_render_runtime::project_generation();
  const auto direct_stats_before = render_receipts::statistics();
  bool direct_registered = direct_draft &&
      registry.project_identity(active.project_id, direct_project);
  if (direct_registered) {
    direct_draft->pixel_format = world_registry::kPixelFormatArgb32;
    direct_draft->world.data = direct_draft->pixels.data();
    direct_draft->world.rowbytes = 4;
    direct_draft->world.width = 1;
    direct_draft->world.height = 1;
    direct_draft->world.extent_hint = {0, 0, 1, 1};
    direct_draft->world.pix_aspect_ratio = {1, 1};
    direct_draft->scene_bound = true;
    direct_draft->scene_item = active;
    direct_draft->scene_project = direct_project;
    direct_draft->project_generation = direct_generation_before;
    direct_registered = render_receipts::register_scene_receipt(
        std::move(direct_draft), direct_generation_before,
        &direct_receipt) == 0 &&
        direct_receipt;
  }
  const AegpTime direct_time{1, 30};
  const bool direct_mutation = direct_registered &&
      aegp_set_item_current_time(
          active_item.legacy_handle, &direct_time) == 0;
  const auto direct_stats_after = render_receipts::statistics();
  render_receipts::ReceiptSnapshot direct_snapshot{};
  void** direct_world =
      reinterpret_cast<void**>(static_cast<uintptr_t>(0x2620));
  report.direct_bump_receipt_invalidated =
      direct_mutation &&
      aegp_external_render_runtime::project_generation() ==
          direct_generation_before + 1 &&
      !render_receipts::snapshot(direct_receipt, direct_snapshot) &&
      direct_stats_after.stale_invalidations >
          direct_stats_before.stale_invalidations &&
      render_receipts::get_world(direct_receipt, &direct_world) != 0 &&
      direct_world == nullptr;
  if (direct_receipt)
    render_receipts::checkin_if_live(direct_receipt);

  std::unique_ptr<render_receipts::ReceiptDraft> in_flight_draft;
  try {
    in_flight_draft = std::make_unique<render_receipts::ReceiptDraft>();
    in_flight_draft->pixels.resize(4);
  } catch (...) {
    in_flight_draft.reset();
  }
  void* in_flight_receipt =
      reinterpret_cast<void*>(static_cast<uintptr_t>(0x2630));
  const uint32_t in_flight_generation =
      aegp_external_render_runtime::project_generation();
  const auto in_flight_stats_before = render_receipts::statistics();
  bool in_flight_mutation = false;
  if (in_flight_draft &&
      direct_project.kind == scene_model::ObjectKind::project) {
    in_flight_draft->pixel_format =
        world_registry::kPixelFormatArgb32;
    in_flight_draft->world.data = in_flight_draft->pixels.data();
    in_flight_draft->world.rowbytes = 4;
    in_flight_draft->world.width = 1;
    in_flight_draft->world.height = 1;
    in_flight_draft->world.extent_hint = {0, 0, 1, 1};
    in_flight_draft->world.pix_aspect_ratio = {1, 1};
    in_flight_draft->scene_bound = true;
    in_flight_draft->scene_item = active;
    in_flight_draft->scene_project = direct_project;
    in_flight_draft->project_generation = in_flight_generation;
    const AegpTime next_direct_time{2, 30};
    in_flight_mutation =
        aegp_set_item_current_time(
            active_item.legacy_handle, &next_direct_time) == 0;
  }
  const int32_t stale_publication_result =
      in_flight_mutation
          ? render_receipts::register_scene_receipt(
                std::move(in_flight_draft), in_flight_generation,
                &in_flight_receipt)
          : 4;
  const auto in_flight_stats_after = render_receipts::statistics();
  report.in_flight_receipt_rejected =
      in_flight_mutation &&
      aegp_external_render_runtime::project_generation() ==
          in_flight_generation + 1 &&
      stale_publication_result != 0 && in_flight_receipt == nullptr &&
      in_flight_stats_after.created == in_flight_stats_before.created &&
      in_flight_stats_after.live_count ==
          in_flight_stats_before.live_count &&
      in_flight_stats_after.reserved_count ==
          in_flight_stats_before.reserved_count;
  if (in_flight_receipt)
    render_receipts::checkin_if_live(in_flight_receipt);

  if (stream && dispose_stream) ok = dispose_stream(stream) == 0 && ok;
  if (mask && dispose_mask) ok = dispose_mask(mask) == 0 && ok;
  if (keyframe_suite_raw)
    ok = compat_release_suite("AEGP Keyframe Suite", 5) == 0 && ok;
  if (stream_suite_raw)
    ok = compat_release_suite("AEGP Stream Suite", 11) == 0 && ok;
  if (mask_suite_raw)
    ok = compat_release_suite("AEGP Layer Mask Suite", 7) == 0 && ok;
  g_mask_scene.clear();
  mask_runtime::set_model_enabled(saved_mask_model);
  if (applied_effect && delete_effect)
    ok = delete_effect(applied_effect) == 0 && ok;
  if (effect_suite_raw)
    ok = compat_release_suite("AEGP Effect Suite", 4) == 0 && ok;
  g_aegp_comp_idle_roundtrip_mode = saved_comp_idle_mode;
  aegp_staged_item_runtime::clear();
  report.parent_camera_zoom_fixture =
      verify_aegp_get_effect_camera() && verify_aegp_resizer_3d_chain();
  report.cleanup_balanced =
      render_receipts::lifetimes_balanced() &&
      compat_suite_leases_balanced();
  report.passed = ok && report.mask_fixture &&
      report.parent_camera_zoom_fixture && report.typed_identity &&
      report.pointer_id_mismatch_rejected &&
      report.duplicate_stable_id_rejected &&
      report.direct_cycle_rejected && report.indirect_cycle_rejected &&
      report.cross_project_cycle_rejected && report.effect_order &&
      report.duplicate_effect_order_rejected &&
      report.duplicate_order_state_unchanged &&
      report.duplicate_order_receipt_unchanged &&
      report.concurrent_effect_flags_serialized &&
      report.concurrent_mask_streams_serialized &&
      report.concurrent_keyframe_inserts_serialized &&
      report.unsupported_slots_preserved &&
      report.stage_invalidated && report.receipt_invalidated &&
      report.direct_bump_receipt_invalidated &&
      report.in_flight_receipt_rejected &&
      report.invalid_handle_distinguished && report.cleanup_balanced &&
      report.stage_identity_hash != 0 && report.trace_hash != 0;
  return report;
}
}  // namespace aexcompat::l2_detail
