#include "render_lifecycle.hpp"

#include <cstring>
#include <iostream>

namespace aexcompat::render_lifecycle {

#if defined(AEXCOMPAT_RENDER_WORKER) || defined(AEXCOMPAT_SMART_WORKER)
namespace {

void transfer_pointer(void* destination, std::size_t destination_offset,
                      const void* source, std::size_t source_offset) {
  void* value{};
  std::memcpy(&value, static_cast<const unsigned char*>(source) + source_offset,
              sizeof(value));
  std::memcpy(static_cast<unsigned char*>(destination) + destination_offset,
              &value, sizeof(value));
}

void clear_pointer(void* destination, std::size_t offset) {
  void* value{};
  std::memcpy(static_cast<unsigned char*>(destination) + offset, &value,
              sizeof(value));
}

}  // namespace

RenderLifecycle begin_frame(const Hooks& hooks, const Layout& layout,
                            void* input, void* output, void** params, void* world) {
  RenderLifecycle lifecycle;
  if (!hooks.invoke_frame) {
    lifecycle.setup_error = 512;
    return lifecycle;
  }
  std::cerr << "stage:frame_setup_begin\n" << std::flush;
  const int32_t frame_error = hooks.invoke_frame(
      hooks.context, layout.frame_setup, input, output, params, world);
  std::cerr << "stage:frame_setup_end error=" << frame_error << "\n" << std::flush;
  if (frame_error != 0) {
    lifecycle.setup_error = frame_error;
    return lifecycle;
  }
  lifecycle.frame_started = true;
  transfer_pointer(input, layout.in_frame_data, output, layout.out_frame_data);
  return lifecycle;
}

int32_t end_frame(const Hooks& hooks, const Layout& layout, void* input,
                  void* output, void** params, void* world,
                  const RenderLifecycle& lifecycle, int32_t primary_error) {
  int32_t result = primary_error;
  if (lifecycle.frame_started) {
    std::cerr << "stage:frame_setdown_begin\n" << std::flush;
    const int32_t error = hooks.invoke_frame
        ? hooks.invoke_frame(hooks.context, layout.frame_setdown, input, output,
                             params, world)
        : 512;
    std::cerr << "stage:frame_setdown_end error=" << error << "\n" << std::flush;
    if (result == 0 && error != 0) result = error;
    clear_pointer(input, layout.in_frame_data);
  }
  return result;
}

RenderLifecycle begin_render(const Hooks& hooks, const Layout& layout,
                             void* input, void* output, void** params, void* world) {
  RenderLifecycle lifecycle;
  if (!hooks.invoke_sequence) {
    lifecycle.setup_error = 512;
    return lifecycle;
  }
  std::cerr << "stage:sequence_setup_begin\n" << std::flush;
  const int32_t sequence_error = hooks.invoke_sequence(
      hooks.context, layout.sequence_setup, input, output);
  std::cerr << "stage:sequence_setup_end error=" << sequence_error << "\n" << std::flush;
  if (sequence_error != 0) {
    lifecycle.setup_error = sequence_error;
    return lifecycle;
  }
  lifecycle.sequence_started = true;
  if (hooks.activate_aux) hooks.activate_aux(hooks.context);
  transfer_pointer(input, layout.in_sequence_data, output,
                   layout.out_sequence_data);

  const RenderLifecycle frame =
      begin_frame(hooks, layout, input, output, params, world);
  lifecycle.frame_started = frame.frame_started;
  lifecycle.setup_error = frame.setup_error;
  return lifecycle;
}

int32_t end_render(const Hooks& hooks, const Layout& layout, void* input,
                   void* output, void** params, void* world,
                   const RenderLifecycle& lifecycle, int32_t primary_error) {
  int32_t result = end_frame(hooks, layout, input, output, params, world,
                             lifecycle, primary_error);
  if (lifecycle.sequence_started) {
    std::cerr << "stage:sequence_setdown_begin\n" << std::flush;
    const int32_t error = hooks.invoke_sequence
        ? hooks.invoke_sequence(hooks.context, layout.sequence_setdown, input,
                                output)
        : 512;
    std::cerr << "stage:sequence_setdown_end error=" << error << "\n" << std::flush;
    if (result == 0 && error != 0) result = error;
    clear_pointer(input, layout.in_sequence_data);
  }
  if (hooks.cleanup_aux) hooks.cleanup_aux(hooks.context);
  return result;
}
#endif

}  // namespace aexcompat::render_lifecycle
