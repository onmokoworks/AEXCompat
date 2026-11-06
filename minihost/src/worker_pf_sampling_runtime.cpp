#include "worker_pf_sampling_runtime.hpp"
#include "worker_pf_sampling_math.hpp"
#include "worker_pf_suites_internal.hpp"
#include "worker_callback_diagnostics.hpp"
#include "worker_extended_diag.hpp"
#include "worker_suite_registry.hpp"
#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <iostream>
#include <mutex>
#include <thread>
#include <unordered_map>

namespace {
constexpr int32_t kPfBadCallbackParam = 516;
PfSamplingHostHooks g_hooks{};
int32_t finish_callback(aexcompat::callback_diagnostics::Callback callback, int32_t result,
                        aexcompat::callback_diagnostics::Reason reason =
                            aexcompat::callback_diagnostics::Reason::None) {
  aexcompat::callback_diagnostics::record(callback, result, reason);
  if (aexcompat::l2_detail::extended_diag_enabled()) {
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
bool resolve_world(void* world, int32_t pixel_bytes, unsigned char*& pixels,
                   int32_t& rowbytes, int32_t& width, int32_t& height) {
  return g_hooks.resolve_world &&
      g_hooks.resolve_world(world, pixel_bytes, pixels, rowbytes, width, height);
}
int32_t acquire_host_suite(const char* name, int32_t version, const void** suite) {
  if (!suite) return kPfBadCallbackParam;
  *suite = nullptr;
  return g_hooks.acquire_suite ? g_hooks.acquire_suite(name, version, suite)
                               : kPfBadCallbackParam;
}
int32_t release_host_suite(const char* name, int32_t version) {
  return g_hooks.release_suite ? g_hooks.release_suite(name, version)
                               : kPfBadCallbackParam;
}
}

std::array<void*, 3> g_sampling8_suite1{
    reinterpret_cast<void*>(&nearest_sample8),
    reinterpret_cast<void*>(&subpixel_sample8),
    reinterpret_cast<void*>(&area_sample8)};
std::array<void*, 3> g_sampling16_suite1{
    reinterpret_cast<void*>(&nearest_sample16),
    reinterpret_cast<void*>(&subpixel_sample16),
    reinterpret_cast<void*>(&area_sample16)};
std::array<void*, 3> g_sampling_float_suite1{
    reinterpret_cast<void*>(&nearest_sample_float),
    reinterpret_cast<void*>(&subpixel_sample_float),
    reinterpret_cast<void*>(&area_sample_float)};
PfBatchSamplingSuite1 g_batch_sampling_suite1{
    &begin_sampling8, &end_sampling8, &unsupported_batch_sample_func,
    &unsupported_batch_sample_func16};
static_assert(sizeof(PfBatchSamplingSuite1) == 4 * sizeof(void*));
static_assert(offsetof(PfBatchSamplingSuite1, get_batch_func) == 2 * sizeof(void*));

void configure_pf_sampling_runtime(const PfSamplingHostHooks& hooks) noexcept {
  g_hooks = hooks;
}

int32_t __cdecl get_callback_addr(void*, int32_t quality, uint32_t mode_flags, int32_t callback_id,
                                  void** callback) {
  if (!callback) return kPfBadCallbackParam;
  *callback = nullptr;
  // PF_CallbackID values from AE_EffectCB.h. Only return callbacks this host
  // actually implements; an unknown selector stays a structured PF error
  // rather than a non-null poison function (issue #793).
  //
  // The two negative ids are Adobe-private and were read off AE 2026's own
  // dispatcher (issue #985; worker_pf_private_callbacks.hpp): AE answers -5
  // with PFp_GaussianValue for every (quality, mode), and -2 with the
  // straight-alpha blur entry when mode == 1 and the premultiplied one
  // otherwise, quality ignored (the callee reads in_data->quality when
  // called). Any other private id stays refused - AE has functions behind
  // -1, -3, -4, -6, -8, -9, -11, -12 too, but nothing in the corpus asks for
  // them and none has been read.
  switch (callback_id) {
    case -5:
      if (!g_hooks.private_gaussian_value) return kPfBadCallbackParam;
      *callback = reinterpret_cast<void*>(g_hooks.private_gaussian_value);
      break;
    case -2: {
      const auto blur = mode_flags == 1 ? g_hooks.private_blur_straight
                                        : g_hooks.private_blur_premultiplied;
      if (!blur) return kPfBadCallbackParam;
      *callback = reinterpret_cast<void*>(blur);
      break;
    }
    case 2: *callback = reinterpret_cast<void*>(&subpixel_sample8); break;
    case 3: *callback = reinterpret_cast<void*>(&area_sample8); break;
    case 9:
      *callback = reinterpret_cast<void*>(
          (quality != 0 && quality != 1) || mode_flags > 2 ? &copy_world8 :
          quality == 0 ? &private_copy_world_nearest :
          mode_flags == 1 ? &private_copy_world_area8 : &private_copy_world_premultiplied);
      break;
    case 31: *callback = reinterpret_cast<void*>(&subpixel_sample16); break;
    case 32: *callback = reinterpret_cast<void*>(&area_sample16); break;
    default: return kPfBadCallbackParam;
  }
  return 0;
}

// `effect_ref` is accepted and ignored. It used to be rejected when null, which
// cost AE's own Displacement every frame: its SMART_RENDER pixel function calls
// the `utils->subpixel_sample` slot directly with a null ref - not through the
// PF_SUBPIXEL_SAMPLE macro, which would have passed `in_data->effect_ref` - and
// returned the 4 it got back as its own PF_Err_OUT_OF_MEMORY (issue #777). The
// host does populate `in_data->effect_ref`; the plug-in just does not use it
// here. That a first-party effect ships this way and renders in AE is the
// evidence that AE tolerates it (an inference from the plug-in's behaviour, not
// an observation of AE's own callback).
//
// Dropping the check costs no protection, and not because anything downstream
// re-establishes it: `resolve_world` bounds-checks the world the plug-in points
// at (worker_world_safety.cpp, `bounded_typed_world`) but consults no ownership
// registry, so it never refused a world *for being foreign* - only for failing
// its bounds or deep-flag checks. The reason is simpler: the plug-in runs in
// this process and is handed `effect_ref` in `in_data`, so it could always have
// passed whatever value the check demanded. The check caught accident, never
// abuse, and on a correct plug-in its only effect was turning a valid sample
// into a refusal reported as PF_Err_OUT_OF_MEMORY (which is itself the wrong
// code for an argument rejection - issue #813).
int32_t subpixel_sample_typed(int32_t pixel_bytes, void* effect_ref, int32_t fixed_x,
                              int32_t fixed_y, const void* sampling_params,
                              void* destination_pixel) {
  (void)effect_ref;
  using aexcompat::callback_diagnostics::Callback;
  using aexcompat::callback_diagnostics::Reason;
  if (!sampling_params || !destination_pixel)
    return finish_callback(Callback::Sampling, kPfBadCallbackParam, Reason::InvalidArguments);
  void* source_world{};
  std::memcpy(&source_world, static_cast<const std::byte*>(sampling_params) + 16,
              sizeof(source_world));
  unsigned char* source{};
  int32_t rowbytes{}, width{}, height{};
  if (!resolve_world(source_world, pixel_bytes, source, rowbytes, width, height))
    return finish_callback(Callback::Sampling, kPfBadCallbackParam, Reason::MissingWorld);
  const aexcompat::sampling::World world{source, rowbytes, width, height};
  if (pixel_bytes == 4) {
    const auto value = aexcompat::sampling::subpixel<uint8_t>(world, fixed_x, fixed_y);
    std::memcpy(destination_pixel, value.data(), sizeof(value));
  } else if (pixel_bytes == 8) {
    const auto value = aexcompat::sampling::subpixel<uint16_t>(world, fixed_x, fixed_y);
    std::memcpy(destination_pixel, value.data(), sizeof(value));
  } else {
    const auto value = aexcompat::sampling::subpixel<float>(world, fixed_x, fixed_y);
    std::memcpy(destination_pixel, value.data(), sizeof(value));
  }
  return finish_callback(Callback::Sampling, 0);
}

// `effect_ref` accepted and ignored, for the same reason as above (issue #777).
int32_t nearest_sample_typed(int32_t pixel_bytes, void* effect_ref, int32_t fixed_x,
                             int32_t fixed_y, const void* sampling_params,
                             void* destination_pixel) {
  (void)effect_ref;
  using aexcompat::callback_diagnostics::Callback;
  using aexcompat::callback_diagnostics::Reason;
  if (!sampling_params || !destination_pixel)
    return finish_callback(Callback::Sampling, kPfBadCallbackParam, Reason::InvalidArguments);
  void* source_world{};
  std::memcpy(&source_world, static_cast<const std::byte*>(sampling_params) + 16,
              sizeof(source_world));
  unsigned char* source{};
  int32_t rowbytes{}, width{}, height{};
  if (!resolve_world(source_world, pixel_bytes, source, rowbytes, width, height))
    return finish_callback(Callback::Sampling, kPfBadCallbackParam, Reason::MissingWorld);
  const int32_t x = static_cast<int32_t>(std::floor(fixed_x / 65536.0 + 0.5));
  const int32_t y = static_cast<int32_t>(std::floor(fixed_y / 65536.0 + 0.5));
  if (x < 0 || x >= width || y < 0 || y >= height) {
    std::memset(destination_pixel, 0, pixel_bytes);
    return finish_callback(Callback::Sampling, 0);
  }
  std::memcpy(destination_pixel,
              source + static_cast<std::size_t>(y) * rowbytes +
                  static_cast<std::size_t>(x) * pixel_bytes,
              pixel_bytes);
  return finish_callback(Callback::Sampling, 0);
}

int32_t __cdecl nearest_sample8(void* effect_ref, int32_t x, int32_t y,
                                const void* params, void* pixel) {
  return nearest_sample_typed(4, effect_ref, x, y, params, pixel);
}
int32_t __cdecl nearest_sample16(void* effect_ref, int32_t x, int32_t y,
                                 const void* params, void* pixel) {
  return nearest_sample_typed(8, effect_ref, x, y, params, pixel);
}
int32_t __cdecl nearest_sample_float(void* effect_ref, int32_t x, int32_t y,
                                     const void* params, void* pixel) {
  return nearest_sample_typed(16, effect_ref, x, y, params, pixel);
}

// `effect_ref` accepted and ignored, for the same reason as the other two
// samplers (issue #777). PF_SUBPIXEL_SAMPLE and PF_AREA_SAMPLE take the same
// argument on the same terms, so they answer a null ref the same way.
int32_t area_sample_typed(int32_t pixel_bytes, void* effect_ref, int32_t fixed_x,
                          int32_t fixed_y, const void* sampling_params,
                          void* destination_pixel) {
  (void)effect_ref;
  using aexcompat::callback_diagnostics::Callback;
  using aexcompat::callback_diagnostics::Reason;
  if (!sampling_params || !destination_pixel)
    return finish_callback(Callback::Sampling, kPfBadCallbackParam, Reason::InvalidArguments);
  int32_t fixed_radius_x{}, fixed_radius_y{}, fixed_area{};
  void* source_world{};
  const auto* params = static_cast<const std::byte*>(sampling_params);
  std::memcpy(&fixed_radius_x, params, sizeof(fixed_radius_x));
  std::memcpy(&fixed_radius_y, params + 4, sizeof(fixed_radius_y));
  std::memcpy(&fixed_area, params + 8, sizeof(fixed_area));
  std::memcpy(&source_world, params + 16, sizeof(source_world));
  const double radius_x = fixed_radius_x / 65536.0;
  const double radius_y = fixed_radius_y / 65536.0;
  // Which term refused, with the plug-in's value (the #1032
  // `stage:callback_denied` shape): eight of the 516 bucket's plug-ins died on
  // this predicate and the collapsed `invalid_area` could not say which term
  // or what they passed (issue #1033). The radii are raw 1/65536 fixed-point,
  // as received. One line per distinct reason per worker, not per call:
  // area_sample is a per-pixel callback, and a plug-in that ignores the
  // refusal would otherwise turn a frame into millions of flushed pipe
  // writes; the broker parser deduplicates anyway.
  static std::array<std::atomic<bool>, 4> reason_emitted{};
  const auto denied = [&](std::size_t index, const char* reason, int64_t value) {
    if (!reason_emitted[index].exchange(true))
      std::cerr << "stage:callback_denied callback=area_sample reason=" << reason
                << " value=" << value << "\n" << std::flush;
    return finish_callback(Callback::Sampling, kPfBadCallbackParam, Reason::InvalidArea);
  };
  // `area` is not validated, because nothing here uses it: the footprint is
  // computed from the radii below, and the measured corpus passes zero
  // (LightSweep), negative (Blobbylize, Glass) and garbage values in a field
  // AE demonstrably tolerates - refusing them was the whole frame error for
  // three plug-ins AE renders (issue #1033). `nearest`/`subpixel` above
  // already read only `src` out of the same struct on the same reasoning.
  (void)fixed_area;
  // AE CPU/Software ignores samp_behave, including the SDK's unsupported
  // REPEAT/WRAP values. Every sampler contributes zero outside the world.
  if (radius_x <= 0.0) return denied(0, "x_radius_nonpositive", fixed_radius_x);
  if (radius_y <= 0.0) return denied(1, "y_radius_nonpositive", fixed_radius_y);
  if (radius_x >= 128.0) return denied(2, "x_radius_over_128", fixed_radius_x);
  if (radius_y >= 128.0) return denied(3, "y_radius_over_128", fixed_radius_y);
  unsigned char* source{};
  int32_t rowbytes{}, width{}, height{};
  if (!resolve_world(source_world, pixel_bytes, source, rowbytes, width, height))
    return finish_callback(Callback::Sampling, kPfBadCallbackParam, Reason::MissingWorld);
  const aexcompat::sampling::World world{source, rowbytes, width, height};
  if (pixel_bytes == 4) {
    const auto value = aexcompat::sampling::area<uint8_t>(world, fixed_x, fixed_y, fixed_radius_x, fixed_radius_y);
    std::memcpy(destination_pixel, value.data(), sizeof(value));
  } else if (pixel_bytes == 8) {
    const auto value = aexcompat::sampling::area<uint16_t>(world, fixed_x, fixed_y, fixed_radius_x, fixed_radius_y);
    std::memcpy(destination_pixel, value.data(), sizeof(value));
  } else {
    const auto value = aexcompat::sampling::area<float>(world, fixed_x, fixed_y, fixed_radius_x, fixed_radius_y);
    std::memcpy(destination_pixel, value.data(), sizeof(value));
  }
  return finish_callback(Callback::Sampling, 0);
}

int32_t __cdecl area_sample8(void* effect_ref, int32_t x, int32_t y,
                             const void* params, void* pixel) {
  return area_sample_typed(4, effect_ref, x, y, params, pixel);
}
int32_t __cdecl area_sample16(void* effect_ref, int32_t x, int32_t y,
                              const void* params, void* pixel) {
  return area_sample_typed(8, effect_ref, x, y, params, pixel);
}
int32_t __cdecl area_sample_float(void* effect_ref, int32_t x, int32_t y,
                                   const void* params, void* pixel) {
  return area_sample_typed(16, effect_ref, x, y, params, pixel);
}

struct LegacySamplingSession {
  int32_t quality{};
  uint32_t mode_flags{};
  void* source_world{};
  std::thread::id thread_id{};
};
std::mutex g_legacy_sampling_mutex;
std::unordered_map<void*, LegacySamplingSession> g_legacy_sampling_sessions;

int32_t __cdecl begin_sampling8(void* effect_ref, int32_t quality, uint32_t mode_flags,
                                void* sampling_params) {
  using aexcompat::callback_diagnostics::Callback;
  using aexcompat::callback_diagnostics::Reason;
  if (effect_ref != g_hooks.effect_ref || !sampling_params || (quality != 0 && quality != 1))
    return finish_callback(Callback::BeginSampling, kPfBadCallbackParam,
                           Reason::InvalidArguments);
  void* source_world{};
  std::memcpy(&source_world, static_cast<std::byte*>(sampling_params) + 16,
              sizeof(source_world));
  unsigned char* source{};
  int32_t rowbytes{}, width{}, height{};
  if (!resolve_world(source_world, 4, source, rowbytes, width, height))
    return finish_callback(Callback::BeginSampling, kPfBadCallbackParam,
                           Reason::MissingWorld);
  std::lock_guard<std::mutex> lock(g_legacy_sampling_mutex);
  const auto [_, inserted] = g_legacy_sampling_sessions.emplace(
      sampling_params, LegacySamplingSession{quality, mode_flags, source_world,
                                             std::this_thread::get_id()});
  return finish_callback(Callback::BeginSampling,
      inserted ? 0 : kPfBadCallbackParam,
      inserted ? Reason::None : Reason::AlreadyCheckedOut);
}

int32_t __cdecl end_sampling8(void* effect_ref, int32_t quality, uint32_t mode_flags,
                              void* sampling_params) {
  using aexcompat::callback_diagnostics::Callback;
  using aexcompat::callback_diagnostics::Reason;
  if (effect_ref != g_hooks.effect_ref || !sampling_params)
    return finish_callback(Callback::EndSampling, kPfBadCallbackParam,
                           Reason::InvalidArguments);
  std::lock_guard<std::mutex> lock(g_legacy_sampling_mutex);
  const auto found = g_legacy_sampling_sessions.find(sampling_params);
  if (found == g_legacy_sampling_sessions.end() || found->second.quality != quality ||
      found->second.mode_flags != mode_flags ||
      found->second.thread_id != std::this_thread::get_id())
    return finish_callback(Callback::EndSampling, kPfBadCallbackParam,
                           Reason::NotCheckedOut);
  g_legacy_sampling_sessions.erase(found);
  return finish_callback(Callback::EndSampling, 0);
}

int32_t unsupported_batch_sample_func_for_slot(
    uint32_t slot, void* effect_ref, int32_t quality, uint32_t mode_flags,
    const void* sampling_params, void** batch) {
  if (!batch) return kPfBadCallbackParam;
  *batch = nullptr;
  if (effect_ref != g_hooks.effect_ref || !sampling_params || (quality != 0 && quality != 1))
    return kPfBadCallbackParam;
  (void)mode_flags;
  return aexcompat::worker_runtime::record_unsupported_suite_call(
      aexcompat::worker_runtime::UnsupportedSuiteId::pf_batch_sampling_1,
      slot);
}

int32_t __cdecl unsupported_batch_sample_func(void* effect_ref, int32_t quality,
                                               uint32_t mode_flags,
                                               const void* sampling_params,
                                               void** batch) {
  return unsupported_batch_sample_func_for_slot(
      2, effect_ref, quality, mode_flags, sampling_params, batch);
}

int32_t __cdecl unsupported_batch_sample_func16(void* effect_ref, int32_t quality,
                                                 uint32_t mode_flags,
                                                 const void* sampling_params,
                                                 void** batch) {
  return unsupported_batch_sample_func_for_slot(
      3, effect_ref, quality, mode_flags, sampling_params, batch);
}

bool verify_pf_batch_sampling_suite() {
  const void* acquired{};
  if (acquire_host_suite("PF Batch Sampling Suite", 1, &acquired) != 0 ||
      acquired != g_hooks.batch_sampling_suite)
    return false;

  alignas(void*) std::array<std::byte, 64> world{};
  alignas(void*) std::array<std::byte, 64> params{};
  std::array<std::byte, 16> pixels{};
  void* pixel_data = pixels.data();
  int32_t rowbytes = 8, width = 2, height = 2;
  std::memcpy(world.data() + 24, &pixel_data, sizeof(pixel_data));
  std::memcpy(world.data() + 32, &rowbytes, sizeof(rowbytes));
  std::memcpy(world.data() + 36, &width, sizeof(width));
  std::memcpy(world.data() + 40, &height, sizeof(height));
  void* source_world = world.data();
  std::memcpy(params.data() + 16, &source_world, sizeof(source_world));

  auto& suite = g_batch_sampling_suite1;
  bool passed = suite.begin_sampling(g_hooks.effect_ref, 1, 0x12, params.data()) == 0 &&
      suite.begin_sampling(g_hooks.effect_ref, 1, 0x12, params.data()) == kPfBadCallbackParam &&
      suite.end_sampling(g_hooks.effect_ref, 0, 0x12, params.data()) == kPfBadCallbackParam &&
      suite.end_sampling(g_hooks.effect_ref, 1, 0x13, params.data()) == kPfBadCallbackParam;

  int32_t cross_thread_result{};
  std::thread foreign_thread([&] {
    cross_thread_result = suite.end_sampling(g_hooks.effect_ref, 1, 0x12, params.data());
  });
  foreign_thread.join();
  passed = passed && cross_thread_result == kPfBadCallbackParam &&
      suite.end_sampling(g_hooks.effect_ref, 1, 0x12, params.data()) == 0 &&
      suite.end_sampling(g_hooks.effect_ref, 1, 0x12, params.data()) == kPfBadCallbackParam &&
      suite.begin_sampling(nullptr, 1, 0, params.data()) == kPfBadCallbackParam &&
      suite.begin_sampling(g_hooks.effect_ref, 2, 0, params.data()) == kPfBadCallbackParam &&
      suite.begin_sampling(g_hooks.effect_ref, 1, 0, nullptr) == kPfBadCallbackParam;

  void* batch = reinterpret_cast<void*>(0x1234);
  passed = passed && suite.get_batch_func(g_hooks.effect_ref, 1, 0, params.data(), &batch) == 4 &&
      batch == nullptr;
  batch = reinterpret_cast<void*>(0x5678);
  passed = passed && suite.get_batch_func16(g_hooks.effect_ref, 0, 0, params.data(), &batch) == 4 &&
      batch == nullptr;
  batch = reinterpret_cast<void*>(0x9abc);
  passed = passed &&
      suite.get_batch_func(nullptr, 1, 0, params.data(), &batch) == kPfBadCallbackParam &&
      batch == nullptr &&
      suite.get_batch_func(g_hooks.effect_ref, 1, 0, params.data(), nullptr) == kPfBadCallbackParam;

  passed = release_host_suite("PF Batch Sampling Suite", 1) == 0 && passed;
  std::lock_guard<std::mutex> lock(g_legacy_sampling_mutex);
  return passed && g_legacy_sampling_sessions.empty();
}

int32_t __cdecl subpixel_sample8(void* effect_ref, int32_t x, int32_t y,
                                 const void* params, void* pixel) {
  return subpixel_sample_typed(4, effect_ref, x, y, params, pixel);
}
int32_t __cdecl subpixel_sample16(void* effect_ref, int32_t x, int32_t y,
                                  const void* params, void* pixel) {
  return subpixel_sample_typed(8, effect_ref, x, y, params, pixel);
}
int32_t __cdecl subpixel_sample_float(void* effect_ref, int32_t x, int32_t y,
                                      const void* params, void* pixel) {
  return subpixel_sample_typed(16, effect_ref, x, y, params, pixel);
}
