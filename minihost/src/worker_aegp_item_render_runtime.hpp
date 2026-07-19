#pragma once

#include "worker_aegp_render_options.hpp"
#include "worker_render_receipts.hpp"

#include <cstdint>

namespace aexcompat::aegp_item_render_runtime {

using Cancel = int32_t(__cdecl*)(void* refcon, uint8_t* canceled);
using SnapshotOptions = bool(*)(void* handle, render_options::ItemValue& output);
using PublishCached = int32_t(*)(const render_options::ItemValue& options,
                                 void** output, bool* cache_hit);
using PublishStaged = int32_t(*)(void* options, void** receipt);

struct Hooks {
  SnapshotOptions snapshot_options;
  PublishCached publish_cached;
  PublishStaged publish_staged;
};

void configure(Hooks hooks) noexcept;
void populate_synthetic_pixels(render_receipts::ReceiptDraft& receipt,
                               int32_t type, int32_t width, int32_t height);
int32_t publish_synthetic(int32_t pixel_format, void** output,
                          const render_options::ItemValue* options = nullptr);
int32_t publish_receipt(void* options, void** receipt);
int32_t checkout(void* options, Cancel check_cancel, void* cancel_refcon, void** receipt);

}  // namespace aexcompat::aegp_item_render_runtime
