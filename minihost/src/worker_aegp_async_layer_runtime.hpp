#pragma once

#include "worker_aegp_render_options.hpp"

#include <cstdint>
#include <vector>

namespace aexcompat::aegp_async_layer {

using Callback = int32_t(__cdecl*)(uint64_t, uint8_t, int32_t, void*, void*);

struct SourceSnapshot {
  void* entry{};
  int32_t pixel_bytes{};
  int32_t width{};
  int32_t height{};
  int32_t current_time{};
  int32_t time_scale{1};
  uint32_t project_generation{};
  std::vector<unsigned char> pixels;
};

struct Hooks {
  bool (*render_worker)(){};
  bool (*snapshot_options)(void*, render_options::LayerValue&){};
  bool (*capture_source)(const render_options::LayerValue&, SourceSnapshot&){};
  int32_t (*publish)(const SourceSnapshot&, const render_options::LayerValue&, void**){};
  bool (*checkin_if_live)(void*){};
  int32_t (*invoke_callback)(Callback, uint64_t, uint8_t, int32_t, void*, void*,
                             int32_t*, uint32_t*){};
};

struct Diagnostics {
  uint32_t created{}, completed{}, canceled{}, callback_failures{}, callback_exceptions{};
  std::size_t live{};
  uint64_t reserved_bytes{};
};

void configure(const Hooks&) noexcept;
void set_cancel_test_gate(bool enabled) noexcept;
int32_t checkout(void*, Callback, void*, uint64_t*);
int32_t cancel(uint64_t);
void drain();
bool balanced();
Diagnostics diagnostics();

}  // namespace aexcompat::aegp_async_layer
