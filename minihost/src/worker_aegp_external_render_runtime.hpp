#pragma once

#include "worker_aegp_render_options.hpp"

#include <cstdint>

namespace aexcompat::aegp_external_render_runtime {

struct Hooks {
  void (*invalidate_staged_items)();
  bool (*valid_item)(void* item);
};

struct Diagnostics {
  uint32_t frames_checked_in{};
  std::size_t cached_frames{};
  bool timestamp_exhausted{};
};

void configure(Hooks hooks) noexcept;
uint32_t project_generation() noexcept;
void bump_project_generation() noexcept;
bool cache_empty() noexcept;
Diagnostics diagnostics() noexcept;

int32_t publish_cached_receipt(const render_options::ItemValue& options,
                               void** output, bool* cache_hit);
int32_t __cdecl timestamp(void* output);
int32_t __cdecl changed(void* item, const void* start, const void* duration,
                        const void* timestamp, uint8_t* changed);
int32_t __cdecl worthwhile(void* options, const void* timestamp, uint8_t* worthwhile);
int32_t __cdecl checkin_rendered(void* options, const void* timestamp,
                                 uint32_t ticks_to_render, void* image);

}  // namespace aexcompat::aegp_external_render_runtime
