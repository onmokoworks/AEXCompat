#pragma once

#include <array>
#include <cstdint>
#include <vector>

namespace aexcompat::host_audio {

// Sole owner of callback-issued audio buffers. The source is borrowed only
// while a worker invocation is active; returned audio memory remains owned by
// this runtime until its matching checkin callback.
struct Telemetry {
  std::uint32_t checkout_calls{};
  std::uint32_t checkin_calls{};
  std::uint32_t get_data_calls{};
  std::uint32_t invalid_operations{};
  bool checkout_allowed{};
  bool usage_advertised{};
  bool source_available{};
  std::uint32_t rejected_unadvertised_checkouts{};
  std::uint32_t rejected_format_requests{};
  std::uint32_t handle_exhaustions{};
  std::int32_t last_checkout_start_time{};
  std::int32_t last_checkout_duration{};
  std::uint32_t last_checkout_time_scale{};
  std::int64_t last_window_start_sample{};
  std::int32_t last_window_sample_count{};
  std::int32_t last_window_silence_samples{};
  std::uint32_t last_output_rate{};
  std::int32_t last_output_bytes_per_sample{};
  std::int32_t last_output_channels{};
  std::int32_t last_output_format{};
  std::int32_t last_returned_sample_frames{};
  std::uint32_t peak_live_handles{};
};

class Runtime {
 public:
  void set_source(const std::vector<float>* source, std::int32_t sample_count);
  void configure_admission(bool audio_only_mode, bool usage_advertised);
  int checkout(void* effect_ref, std::int32_t index, std::int32_t start_time,
               std::int32_t duration, std::uint32_t time_scale, std::uint32_t rate,
               std::int32_t bytes_per_sample, std::int32_t channels,
               std::int32_t format, void** audio);
  int checkin(void* effect_ref, void* audio);
  int get_data(void* effect_ref, void* audio, void** data, std::int32_t* num_samples,
               std::uint32_t* rate, std::int32_t* bytes_per_sample,
               std::int32_t* channels, std::int32_t* format);
  bool lifetimes_balanced() const;
  const Telemetry& telemetry() const noexcept { return telemetry_; }

 private:
  struct Handle {
    std::vector<unsigned char> samples;
    bool checked_out{};
    std::uint32_t rate{};
    std::int32_t sample_frames{};
    std::int32_t bytes_per_sample{};
    std::int32_t channels{};
    std::int32_t format{};
  };
  std::uint32_t live_handle_count() const;
  std::array<Handle, 16> handles_{};
  const std::vector<float>* source_{};
  std::int32_t source_sample_count_{};
  Telemetry telemetry_{};
};

Runtime& runtime();
int __cdecl checkout_layer_audio(void*, std::int32_t, std::int32_t, std::int32_t,
                                 std::uint32_t, std::uint32_t, std::int32_t,
                                 std::int32_t, std::int32_t, void**);
int __cdecl checkin_layer_audio(void*, void*);
int __cdecl get_audio_data(void*, void*, void**, std::int32_t*, std::uint32_t*,
                           std::int32_t*, std::int32_t*, std::int32_t*);
}  // namespace aexcompat::host_audio
