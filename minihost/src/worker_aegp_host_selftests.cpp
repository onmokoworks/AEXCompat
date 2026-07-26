#include "worker_aegp_host_selftests.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_effect_bootstrap.hpp"
#include "worker_smart_runtime.hpp"
#include "worker_mask_selftests.hpp"

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>

namespace aexcompat::l2_detail {
using namespace aexcompat::worker_runtime::handles;
using SmartRuntimeSession = aexcompat::worker_runtime::smart::Session;
auto& host_smart_state() { return aexcompat::worker_runtime::smart::state(); }
bool configure_mask_scene(const std::string&);
bool mask_lifetimes_balanced();
constexpr std::size_t kCheckoutResultBytes = 76;
bool verify_pre_checkout_result_case(int32_t expected_par_numerator,
                                     int32_t expected_par_denominator,
                                     int32_t expected_reference_width,
                                     int32_t expected_reference_height) {
  std::array<std::byte, kCheckoutResultBytes> result{};
  result.fill(std::byte{0xCD});
  const int32_t status = aexcompat::worker_runtime::smart::pre_checkout_layer(
      nullptr, 0, 0, nullptr, host_smart_state().current_time, 1,
      host_smart_state().current_time_scale, result.data());
  if (status != 0) return false;
  int32_t rect[4];
  std::memcpy(rect, result.data(), sizeof(rect));
  if (rect[0] != 0 || rect[1] != 0 || rect[2] != 640 || rect[3] != 360) return false;
  std::memcpy(rect, result.data() + 16, sizeof(rect));
  if (rect[0] != 0 || rect[1] != 0 || rect[2] != 640 || rect[3] != 360) return false;
  int32_t par[2];
  std::memcpy(par, result.data() + 32, sizeof(par));
  if (par[0] != expected_par_numerator || par[1] != expected_par_denominator) return false;
  int32_t reference_size[2];
  std::memcpy(reference_size, result.data() + 44, sizeof(reference_size));
  if (reference_size[0] != expected_reference_width ||
      reference_size[1] != expected_reference_height) return false;
  for (std::size_t offset = 40; offset < 44; ++offset) {
    if (result[offset] != std::byte{0}) return false;
  }
  for (std::size_t offset = 52; offset < kCheckoutResultBytes; ++offset) {
    if (result[offset] != std::byte{0}) return false;
  }
  return true;
}

bool verify_pre_checkout_result_contract() {
  SmartRuntimeSession smart_session;
  const int32_t saved_width = host_smart_state().width;
  const int32_t saved_height = host_smart_state().height;
  host_smart_state().width = 640;
  host_smart_state().height = 360;
  host_smart_state().current_time = 0;
  host_smart_state().current_time_scale = 1;
  host_smart_state().pixel_aspect_numerator = 1;
  host_smart_state().pixel_aspect_denominator = 1;
  host_smart_state().full_resolution_width = 0;
  host_smart_state().full_resolution_height = 0;
  bool passed = verify_pre_checkout_result_case(1, 1, 640, 360);
  host_smart_state().pixel_aspect_numerator = 10;
  host_smart_state().pixel_aspect_denominator = 11;
  host_smart_state().full_resolution_width = 1280;
  host_smart_state().full_resolution_height = 720;
  passed = verify_pre_checkout_result_case(10, 11, 1280, 720) && passed;
  host_smart_state().width = saved_width;
  host_smart_state().height = saved_height;
  return passed;
}

bool verify_handle_resize_while_locked_rejected() {
  const uint32_t invalid_before = statistics().invalid_operations;
  void** handle = new_handle(16);
  if (!handle || !lock_handle(handle)) return false;
  const int32_t resize_error = resize_handle(32, &handle);
  unlock_handle(handle);
  dispose_handle(handle);
  return resize_error != 0 && statistics().invalid_operations == invalid_before + 1 &&
      handle_lifetimes_balanced();
}

bool verify_utils_handle_callbacks_wired() {
  namespace boot = aexcompat::worker_runtime::effect_bootstrap;
  boot::State state{};
  boot::AbiHooks abi{};
  // The handle callbacks occupy the tail of the utility table, index-aligned
  // with kUtilityCallbackOffsets (160/168/176/184/440/464). transfer_rect at
  // index 15 shifts these host callbacks to indices 26..31. Mirror the exact
  // functions l2_main installs so this drives the production write path.
  abi.utility_callbacks[26] = reinterpret_cast<void*>(&new_handle);
  abi.utility_callbacks[27] = reinterpret_cast<void*>(&lock_handle);
  abi.utility_callbacks[28] = reinterpret_cast<void*>(&unlock_handle);
  abi.utility_callbacks[29] = reinterpret_cast<void*>(&dispose_handle);
  abi.utility_callbacks[30] = reinterpret_cast<void*>(&handle_size);
  abi.utility_callbacks[31] = reinterpret_cast<void*>(&resize_handle);
  boot::install_callback_tables(state, abi);

  // A plug-in reaches these as in_data->utils->host_*; recover the utility
  // block through the in_data+176 link install writes, then read each callback
  // at its SDK offset. Reading via the offsets (not the AbiHooks array) is what
  // catches an offset/index misalignment in the production wiring.
  std::byte* utils = *reinterpret_cast<std::byte**>(state.input.data() + 176);
  if (!utils) return false;
  auto slot = [&](std::size_t offset) {
    void* value{};
    std::memcpy(&value, utils + offset, sizeof(value));
    return value;
  };
  auto new_fn = reinterpret_cast<void** (__cdecl*)(std::uint64_t)>(slot(160));
  auto lock_fn = reinterpret_cast<void* (__cdecl*)(void**)>(slot(168));
  auto unlock_fn = reinterpret_cast<void (__cdecl*)(void**)>(slot(176));
  auto dispose_fn = reinterpret_cast<void (__cdecl*)(void**)>(slot(184));
  auto size_fn = reinterpret_cast<std::uint64_t (__cdecl*)(void**)>(slot(440));
  auto resize_fn =
      reinterpret_cast<std::int32_t (__cdecl*)(std::uint64_t, void***)>(slot(464));
  if (reinterpret_cast<void*>(new_fn) != reinterpret_cast<void*>(&new_handle) ||
      reinterpret_cast<void*>(lock_fn) != reinterpret_cast<void*>(&lock_handle) ||
      reinterpret_cast<void*>(unlock_fn) != reinterpret_cast<void*>(&unlock_handle) ||
      reinterpret_cast<void*>(dispose_fn) != reinterpret_cast<void*>(&dispose_handle) ||
      reinterpret_cast<void*>(size_fn) != reinterpret_cast<void*>(&handle_size) ||
      reinterpret_cast<void*>(resize_fn) != reinterpret_cast<void*>(&resize_handle))
    return false;

  const uint32_t invalid_before = statistics().invalid_operations;
  void** handle = new_fn(16);
  if (!handle) return false;
  auto* data = static_cast<std::uint8_t*>(lock_fn(handle));
  if (!data) {
    dispose_fn(handle);
    return false;
  }
  data[0] = 0xAB;
  data[15] = 0xCD;
  unlock_fn(handle);
  bool passed = size_fn(handle) == 16 && resize_fn(32, &handle) == 0 &&
      size_fn(handle) == 32;
  auto* resized = static_cast<std::uint8_t*>(lock_fn(handle));
  passed = passed && resized && resized[0] == 0xAB && resized[15] == 0xCD;
  if (resized) unlock_fn(handle);
  dispose_fn(handle);
  return passed && statistics().invalid_operations == invalid_before &&
      handle_lifetimes_balanced();
}

bool verify_aegp_memory_and_strings_rejection() {
  const auto original_scene = g_mask_scene;
  const auto memory_before = aegp_memory_statistics();
  void* memory = nullptr; void* data = nullptr; uint32_t size{}; int32_t count{}, total{};
  bool passed = new_aegp_mem_handle(1, "fault probe", 32, 1, &memory) == 0 &&
      lock_aegp_mem_handle(memory, &data) == 0 && data &&
      std::all_of(static_cast<std::byte*>(data), static_cast<std::byte*>(data) + 32,
          [](std::byte value) { return value == std::byte{}; }) &&
      lock_aegp_mem_handle(memory, &data) == 0 &&
      resize_aegp_mem_handle("locked", 64, memory) != 0 &&
      unlock_aegp_mem_handle(memory) == 0 && unlock_aegp_mem_handle(memory) == 0 &&
      resize_aegp_mem_handle("resized", 64, memory) == 0 &&
      get_aegp_mem_handle_size(memory, &size) == 0 && size == 64 &&
      get_aegp_mem_stats(1, &count, &total) == 0 && count == 1 && total == 64 &&
      free_aegp_mem_handle(memory) == 0;
  void* mask = nullptr; void* stream = nullptr; void* name = nullptr; void* expression = nullptr;
  passed = passed && get_layer_mask_by_index(&g_layer, 0, &mask) == 0 &&
      get_new_mask_stream(1, mask, 400, &stream) == 0 &&
      unsupported_stream_name(1, stream, 1, &name) == 0 &&
      lock_aegp_mem_handle(name, &data) == 0 && data &&
      std::u16string(static_cast<const char16_t*>(data)) == u"Mask Path" &&
      unlock_aegp_mem_handle(name) == 0 && free_aegp_mem_handle(name) == 0;
  const uint16_t source[]{'t','i','m','e','*','2',0}; uint8_t enabled{};
  passed = passed && unsupported_set_expression(1, stream, source) == 0 &&
      get_expression_state(1, stream, &enabled) == 0 && enabled == 1 &&
      unsupported_get_expression(1, stream, &expression) == 0 &&
      lock_aegp_mem_handle(expression, &data) == 0 && data &&
      std::u16string(static_cast<const char16_t*>(data)) == u"time*2" &&
      unlock_aegp_mem_handle(expression) == 0 && free_aegp_mem_handle(expression) == 0 &&
      reject_expression_state(1, stream, 0) == 0 &&
      get_expression_state(1, stream, &enabled) == 0 && enabled == 0 &&
      dispose_stream(stream) == 0 && dispose_mask(mask) == 0;
  const bool balanced = mask_lifetimes_balanced() && aegp_memory_balanced();
  g_mask_scene = original_scene; g_mask_scene.reserve(kMaxHostMasks);
  const auto memory_after = aegp_memory_statistics();
  return passed && balanced &&
      memory_after.invalid_operations == memory_before.invalid_operations + 1 &&
      memory_after.created == memory_before.created + 3 &&
      memory_after.freed == memory_before.freed + 3;
}

bool verify_aegp_keyframe_suite5_mutations(bool abi_wiring) {
  if (!abi_wiring || !configure_mask_scene("rectangle")) return false;
  const bool passed = verify_keyframe_ownership_rejection() &&
      g_stream_refs.empty() && g_stream_values.empty() &&
      g_add_keyframe_transactions.empty() && mask_lifetimes_balanced();
  g_mask_scene.clear();
  return passed;
}
}  // namespace aexcompat::l2_detail
