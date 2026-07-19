#pragma once

#include <array>
#include <cstdint>
#include <string>
#include <vector>

namespace aexcompat::worker_runtime::smart {

struct HostedLayer {
  int32_t slot{};
  int32_t time{};
  uint32_t time_scale{};
  bool timed{};
  int32_t width{};
  int32_t height{};
  int32_t checkout_id{-1};
  void* world{};
};

struct State {
  void* input_world{};
  void* output_world{};
  void* map_world{};
  std::vector<HostedLayer> hosted_layers;
  int32_t width{16};
  int32_t height{12};
  int32_t map_width{};
  int32_t map_height{};
  int32_t rowbytes{64};
  std::string pixel_format{"argb8"};
  int32_t secondary_checkout_id{-1};
  int32_t checkout_time{};
  int32_t checkout_time_step{};
  uint32_t checkout_time_scale{};
  std::array<int32_t, 4> input_checkout_request{-1, -1, -1, -1};
  std::array<int32_t, 4> map_checkout_request{-1, -1, -1, -1};

  void clear_transient();
};

struct HostHooks {
  bool* wide_time_checkout_allowed{};
  int32_t* checkout_current_time{};
  uint32_t* checkout_current_time_scale{};
  uint32_t* rejected_temporal_checkouts{};
  int32_t* secondary_layer_slot{};
  int32_t* full_resolution_width{};
  int32_t* full_resolution_height{};
  int32_t* pixel_aspect_numerator{};
  uint32_t* pixel_aspect_denominator{};
};

bool configure_host_hooks(const HostHooks& hooks);
State& state();
int32_t __cdecl width();
int32_t __cdecl height();

class Session {
 public:
  Session();
  ~Session();
  Session(const Session&) = delete;
  Session& operator=(const Session&) = delete;

 private:
  State state_{};
  State* previous_{};
};

int32_t __cdecl pre_checkout_layer(void*, int32_t index, int32_t checkout_id,
                                   const void* request, int32_t what_time,
                                   int32_t time_step, uint32_t time_scale,
                                   void* result);
int32_t __cdecl checkout_pixels(void*, int32_t checkout_id, void** world);
int32_t __cdecl checkin_pixels(void*, int32_t checkout_id);
int32_t __cdecl checkout_output(void*, void** world);

}  // namespace aexcompat::worker_runtime::smart
