#pragma once

#include <cstddef>
#include <cstdint>

namespace aexcompat::render_lifecycle {

#if defined(AEXCOMPAT_RENDER_WORKER) || defined(AEXCOMPAT_SMART_WORKER)
struct RenderLifecycle {
  bool sequence_started{};
  bool frame_started{};
  int32_t setup_error{};
};

struct Layout {
  std::size_t in_sequence_data{};
  std::size_t out_sequence_data{};
  std::size_t in_frame_data{};
  std::size_t out_frame_data{};
  int32_t sequence_setup{};
  int32_t sequence_setdown{};
  int32_t frame_setup{};
  int32_t frame_setdown{};
};

struct Hooks {
  void* context{};
  int32_t (*invoke_frame)(void*, int32_t, void*, void*, void**, void*){};
  int32_t (*invoke_sequence)(void*, int32_t, void*, void*){};
  void (*activate_aux)(void*){};
  void (*cleanup_aux)(void*){};
};

RenderLifecycle begin_frame(const Hooks& hooks, const Layout& layout,
                            void* input, void* output, void** params, void* world);
int32_t end_frame(const Hooks& hooks, const Layout& layout, void* input,
                  void* output, void** params, void* world,
                  const RenderLifecycle& lifecycle, int32_t primary_error);
RenderLifecycle begin_render(const Hooks& hooks, const Layout& layout,
                             void* input, void* output, void** params, void* world);
int32_t end_render(const Hooks& hooks, const Layout& layout, void* input,
                   void* output, void** params, void* world,
                   const RenderLifecycle& lifecycle, int32_t primary_error);
#endif

}  // namespace aexcompat::render_lifecycle
