#include "host_audio_runtime.hpp"
#include "render_subsystem.h"
#include "worker_effect_bootstrap.hpp"

#include <cstdint>
#include <cstring>
#include <iostream>

namespace {
namespace bootstrap = aexcompat::worker_runtime::effect_bootstrap;
namespace contract = aexcompat::abi::x86_64_windows;

int configure_calls{};

int32_t __cdecl effect_entry(int32_t, void*, void*, void**, void*, void*) {
  return 0;
}

int32_t invoke(bootstrap::EffectEntry, int32_t selector, void*, void* output,
               void**, void*, void*, uint32_t*) {
  if (selector == 1) {
    std::memset(output, 0, contract::PF_OUT_DATA_SIZE);
    return 0;
  }
  if (selector == 4) {
    auto* raw = static_cast<std::byte*>(output);
    const int32_t count = 3;
    std::memcpy(raw + contract::OUT_NUM_PARAMS_OFFSET, &count, sizeof(count));
    return 0;
  }
  return 0;
}

void configure(bool, bool) {
  ++configure_calls;
}

int32_t discovered_parameter_count() { return 2; }

int render_with_unchecked_audio(void*) {
  void* audio{};
  const int checkout = aexcompat::host_audio::runtime().checkout(
      reinterpret_cast<void*>(1), 0, 0, 2, 30000, 44100u << 16, 4, 1, 2,
      &audio);
  return checkout == 0 ? 4 : checkout;
}

bool dependencies_ready(void*) { return true; }
}  // namespace

int main() {
  aexcompat::host_audio::Runtime runtime;
  runtime.configure_admission(false, false, {0, 2});
  runtime.set_source(nullptr, 0);

  void* audio{};
  const int checkout = runtime.checkout(
      reinterpret_cast<void*>(1), 2, 0, 2, 30000, 44100u << 16, 4, 1, 2,
      &audio);
  void* samples{reinterpret_cast<void*>(1)};
  std::int32_t sample_count{-1};
  const int data = checkout == 0
      ? runtime.get_data(reinterpret_cast<void*>(1), audio, &samples,
                         &sample_count, nullptr, nullptr, nullptr, nullptr)
      : -1;
  const auto* values = static_cast<const float*>(samples);
  const bool silent = data == 0 && sample_count == 4 && values &&
      values[0] == 0.0f && values[1] == 0.0f && values[2] == 0.0f &&
      values[3] == 0.0f;

  runtime.automatic_checkin();
  // The out pointer's incoming value is not a precondition (issue #1253:
  // AudWave hands over an uninitialised slot and AE services the call); a
  // garbage value is overwritten, not refused. Negative start times are the
  // plug-in's window (AudWave asks from -offset), answered as silence.
  void* garbage_audio{reinterpret_cast<void*>(0x00003fe020bbaec3)};
  const int garbage_checkout = runtime.checkout(
      reinterpret_cast<void*>(1), 2, -2, 4, 30, 44100u << 16, 2, 2, 1,
      &garbage_audio);
  const bool garbage_overwritten = garbage_checkout == 0 &&
      garbage_audio != reinterpret_cast<void*>(0x00003fe020bbaec3) &&
      garbage_audio != nullptr;
  runtime.automatic_checkin();
  void* invalid_audio{};
  const int invalid_index = runtime.checkout(
      reinterpret_cast<void*>(1), 1, 0, 2, 30000, 44100u << 16, 4, 1, 2,
      &invalid_audio);
  const auto& telemetry = runtime.telemetry();
  const bool passed = checkout == 0 && silent && garbage_overwritten &&
      invalid_index != 0 &&
      telemetry.usage_advertised == false &&
      telemetry.unadvertised_checkout_calls == 2 &&
      telemetry.rejected_unadvertised_checkouts == 0 &&
      telemetry.checkout_calls == 2 && telemetry.checkin_calls == 0 &&
      telemetry.automatic_checkins == 2 && telemetry.invalid_operations == 1 &&
      runtime.lifetimes_balanced();

  bootstrap::State state{};
  bootstrap::Request smart_request{};
  smart_request.rendering_worker = true;
  smart_request.skip_about = true;
  const bootstrap::RuntimeHooks hooks{
      &invoke, +[](bool) {}, +[](bool) {}, &configure,
      +[](bootstrap::EffectEntry, bootstrap::State&) {},
      &discovered_parameter_count};
  const auto smart = bootstrap::run(
      state, &effect_entry, {}, smart_request,
      hooks);
  const bool smart_configured = smart.global_error == 0 &&
      smart.params_error == 0 && smart.parameter_count_contract_valid &&
      configure_calls == 1;

  auto& shared = aexcompat::host_audio::runtime();
  shared.configure_admission(false, false, {0});
  shared.set_source(nullptr, 0);
  int render_request{};
  aexcompat::render::RenderContext render_context{
      aexcompat::render::RenderKind::Classic, &render_request,
      {&render_with_unchecked_audio, &aexcompat::host_audio::cleanup_after_render,
       &dependencies_ready},
      false};
  const int dispatch_error = aexcompat::render::dispatch(render_context);
  const bool error_cleanup = dispatch_error == 4 &&
      shared.telemetry().checkout_calls == 1 &&
      shared.telemetry().automatic_checkins == 1 && shared.lifetimes_balanced();

  std::cout << "{\"host_audio_runtime_selftest\":\""
            << (passed && smart_configured && error_cleanup ? "passed" : "failed")
            << "\",\"checkout\":" << checkout
            << ",\"invalid_index\":" << invalid_index
            << ",\"automatic_checkins\":" << telemetry.automatic_checkins
            << "}\n";
  return passed && smart_configured && error_cleanup ? 0 : 1;
}
