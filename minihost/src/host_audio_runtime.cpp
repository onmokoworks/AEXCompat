#include "host_audio_runtime.hpp"

#include <algorithm>
#include <cmath>
#include <cstring>

namespace aexcompat::host_audio {
namespace { constexpr int kCallbackFailure = 4; constexpr std::int64_t kMaxCheckoutSamples = 10'000'000; }
void Runtime::set_source(const std::vector<float>* source, std::int32_t sample_count) {
  source_ = source; source_sample_count_ = source && sample_count >= 0 ? sample_count : 0;
  telemetry_.source_available = source_ != nullptr;
}
void Runtime::configure_admission(bool /*audio_only_mode*/, bool usage_advertised,
                                  const std::vector<std::int32_t>& layer_indices) {
  telemetry_.usage_advertised = usage_advertised;
  // AE records the I_USE_AUDIO contract but still services the callback.  The
  // broker remains responsible for requiring a real sidecar when one was
  // requested; a video-only render has a deterministic silent audio graph.
  telemetry_.checkout_allowed = true;
  telemetry_.rejected_unadvertised_checkouts = 0;
  layer_indices_ = layer_indices;
  if (std::find(layer_indices_.begin(), layer_indices_.end(), 0) ==
      layer_indices_.end())
    layer_indices_.push_back(0);
}
std::uint32_t Runtime::live_handle_count() const {
  return static_cast<std::uint32_t>(std::count_if(handles_.begin(), handles_.end(), [](const Handle& value) { return value.checked_out; }));
}
bool Runtime::lifetimes_balanced() const { return live_handle_count() == 0 && telemetry_.checkout_calls == telemetry_.checkin_calls + telemetry_.automatic_checkins; }
int Runtime::checkout(void* effect_ref, std::int32_t index, std::int32_t start_time, std::int32_t duration,
                      std::uint32_t time_scale, std::uint32_t rate, std::int32_t bytes_per_sample,
                      std::int32_t channels, std::int32_t format, void** audio) {
  telemetry_.last_checkout_index = index;
  telemetry_.last_checkout_start_time = start_time;
  telemetry_.last_checkout_duration = duration;
  telemetry_.last_checkout_time_scale = time_scale;
  telemetry_.last_output_rate = rate;
  telemetry_.last_output_bytes_per_sample = bytes_per_sample;
  telemetry_.last_output_channels = channels;
  telemetry_.last_output_format = format;
  // `*audio` is an out parameter: what it holds on entry is never read by the
  // host, so its value cannot name host state and is not checked. AudWave
  // passes an uninitialised stack slot (0x00003fe0.... on the trace, issue
  // #1253) and AE services the call; refusing it here made SMART_RENDER answer
  // PF_Err_OUT_OF_MEMORY with every audio counter at zero.
  if (!effect_ref || !audio ||
      std::find(layer_indices_.begin(), layer_indices_.end(), index) ==
          layer_indices_.end() ||
      duration < 0 || time_scale == 0) { ++telemetry_.invalid_operations; return kCallbackFailure; }
  if (!telemetry_.usage_advertised)
    ++telemetry_.unadvertised_checkout_calls;
  if (rate < (1000u << 16) || rate > (65535u << 16) || (channels != 1 && channels != 2) ||
      (bytes_per_sample != 1 && bytes_per_sample != 2 && bytes_per_sample != 4) ||
      (format != 0 && format != 1 && format != 2) || (format == 2 && bytes_per_sample != 4)) { ++telemetry_.rejected_format_requests; return kCallbackFailure; }
  const auto handle_it = std::find_if(handles_.begin(), handles_.end(), [](const Handle& value) { return !value.checked_out; });
  if (handle_it == handles_.end()) { ++telemetry_.handle_exhaustions; return kCallbackFailure; }
  const double requested_rate = static_cast<double>(rate) / 65536.0;
  const auto floor_samples = [=](std::int64_t time) { return static_cast<std::int64_t>(std::floor(static_cast<double>(time) * requested_rate / time_scale)); };
  const auto ceil_samples = [=](std::int64_t time) { return static_cast<std::int64_t>(std::ceil(static_cast<double>(time) * requested_rate / time_scale)); };
  const auto window_start = floor_samples(start_time);
  const auto window_count = ceil_samples(static_cast<std::int64_t>(start_time) + duration) - window_start;
  if (window_count < 0 || window_count > kMaxCheckoutSamples) { ++telemetry_.invalid_operations; return kCallbackFailure; }
  const auto returned_frames = window_count + 1; // SDK requires the trailing silent sentinel frame.
  const auto byte_count = static_cast<std::uint64_t>(returned_frames) * channels * bytes_per_sample;
  if (byte_count > static_cast<std::uint64_t>(kMaxCheckoutSamples + 1) * 2 * 4) { ++telemetry_.invalid_operations; return kCallbackFailure; }
  Handle& handle = *handle_it; handle.samples.assign(static_cast<std::size_t>(byte_count), 0);
  const auto encode = [&](std::size_t offset, float value) {
    value = std::clamp(value, -1.0f, 1.0f);
    if (format == 2) std::memcpy(handle.samples.data() + offset, &value, 4);
    else if (format == 1) {
      if (bytes_per_sample == 1) { const auto v = static_cast<std::int8_t>(std::lround(value * 127.0)); std::memcpy(handle.samples.data() + offset, &v, 1); }
      else if (bytes_per_sample == 2) { const auto v = static_cast<std::int16_t>(std::lround(value * 32767.0)); std::memcpy(handle.samples.data() + offset, &v, 2); }
      else { const auto v = static_cast<std::int32_t>(std::llround(value * 2147483647.0)); std::memcpy(handle.samples.data() + offset, &v, 4); }
    } else {
      if (bytes_per_sample == 1) { const auto v = static_cast<std::uint8_t>(std::lround((value + 1.0) * 127.5)); std::memcpy(handle.samples.data() + offset, &v, 1); }
      else if (bytes_per_sample == 2) { const auto v = static_cast<std::uint16_t>(std::lround((value + 1.0) * 32767.5)); std::memcpy(handle.samples.data() + offset, &v, 2); }
      else { const auto v = static_cast<std::uint32_t>(std::llround((value + 1.0) * 2147483647.5)); std::memcpy(handle.samples.data() + offset, &v, 4); }
    }
  };
  std::int64_t silence_frames = 0;
  for (std::int64_t frame = 0; frame < returned_frames; ++frame) {
    const bool sentinel_frame = frame == window_count;
    const double source_position = static_cast<double>(window_start + frame) * 44100.0 / requested_rate;
    float value = 0.0f;
    if (!sentinel_frame && source_position >= 0.0 && source_position < source_sample_count_) {
      const auto left = static_cast<std::int64_t>(std::floor(source_position)); const auto right = std::min<std::int64_t>(left + 1, source_sample_count_ - 1); const double fraction = source_position - left;
      value = static_cast<float>((*source_)[left] * (1.0 - fraction) + (*source_)[right] * fraction);
    } else if (!sentinel_frame) ++silence_frames;
    for (std::int32_t channel = 0; channel < channels; ++channel) encode(static_cast<std::size_t>((frame * channels + channel) * bytes_per_sample), value);
  }
  handle.checked_out = true; handle.rate = rate; handle.sample_frames = static_cast<std::int32_t>(returned_frames); handle.bytes_per_sample = bytes_per_sample; handle.channels = channels; handle.format = format;
  telemetry_.last_window_start_sample = window_start; telemetry_.last_window_sample_count = static_cast<std::int32_t>(window_count); telemetry_.last_window_silence_samples = static_cast<std::int32_t>(silence_frames); telemetry_.last_returned_sample_frames = static_cast<std::int32_t>(returned_frames);
  *audio = &handle; ++telemetry_.checkout_calls; telemetry_.peak_live_handles = std::max(telemetry_.peak_live_handles, live_handle_count()); return 0;
}
int Runtime::checkin(void* effect_ref, void* audio) { const auto it = std::find_if(handles_.begin(), handles_.end(), [=](const Handle& value) { return audio == &value; }); if (!effect_ref || it == handles_.end() || !it->checked_out) { ++telemetry_.invalid_operations; return kCallbackFailure; } it->checked_out = false; it->samples.clear(); ++telemetry_.checkin_calls; return 0; }
void Runtime::automatic_checkin() {
  for (auto& handle : handles_) {
    if (!handle.checked_out) continue;
    handle.checked_out = false;
    handle.samples.clear();
    ++telemetry_.automatic_checkins;
  }
}
int Runtime::get_data(void* effect_ref, void* audio, void** data, std::int32_t* num_samples, std::uint32_t* rate, std::int32_t* bytes_per_sample, std::int32_t* channels, std::int32_t* format) { const auto it = std::find_if(handles_.begin(), handles_.end(), [=](const Handle& value) { return audio == &value; }); if (!effect_ref || it == handles_.end() || !it->checked_out || it->samples.size() > 80'000'008) { ++telemetry_.invalid_operations; return kCallbackFailure; } if (data) *data = it->samples.empty() ? nullptr : it->samples.data(); if (num_samples) *num_samples = it->sample_frames; if (rate) *rate = it->rate; if (bytes_per_sample) *bytes_per_sample = it->bytes_per_sample; if (channels) *channels = it->channels; if (format) *format = it->format; ++telemetry_.get_data_calls; return 0; }
Runtime& runtime() { static Runtime value; return value; }
int __cdecl checkout_layer_audio(void* e, std::int32_t i, std::int32_t s, std::int32_t d, std::uint32_t t, std::uint32_t r, std::int32_t b, std::int32_t c, std::int32_t f, void** a) { return runtime().checkout(e, i, s, d, t, r, b, c, f, a); }
int __cdecl checkin_layer_audio(void* e, void* a) { return runtime().checkin(e, a); }
int __cdecl get_audio_data(void* e, void* a, void** d, std::int32_t* n, std::uint32_t* r, std::int32_t* b, std::int32_t* c, std::int32_t* f) { return runtime().get_data(e, a, d, n, r, b, c, f); }
int cleanup_after_render(void*) { runtime().automatic_checkin(); return 0; }
} // namespace aexcompat::host_audio
