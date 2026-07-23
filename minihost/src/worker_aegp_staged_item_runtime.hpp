#pragma once

#include "worker_aegp_render_options.hpp"

#include <cstdint>

namespace aexcompat::aegp_staged_item_runtime {

using PublishSynthetic = int32_t(*)(int32_t pixel_format, void** output,
                                    const render_options::ItemValue* options);
using Cancel = int32_t(__cdecl*)(void* refcon, uint8_t* canceled);
using Checkout = int32_t(__cdecl*)(void* options, Cancel cancel, void* refcon, void** output);

struct Hooks {
  uint32_t (*project_generation)();
  bool (*synthetic_receipts_enabled)();
  PublishSynthetic publish_synthetic;
};

struct Diagnostics {
  uint32_t published{};
  uint32_t cache_hits{};
  uint32_t cache_misses{};
  uint32_t cycles_rejected{};
  uint32_t generation_invalidations{};
  uint32_t evictions{};
};

void configure(Hooks hooks) noexcept;
void clear() noexcept;
bool publish_world(void* item, suite_abi::AegpTime time, suite_abi::AegpTime time_step,
                   int8_t quality, uint8_t guide_layers, int32_t pixel_format,
                   int32_t width, int32_t height, int32_t rowbytes, const void* pixels);
int32_t publish_receipt(void* options, void** receipt);
bool verify_recursion_guard(void* item, suite_abi::AegpTime time,
                            void* options, Checkout checkout);
Diagnostics diagnostics() noexcept;

}  // namespace aexcompat::aegp_staged_item_runtime
