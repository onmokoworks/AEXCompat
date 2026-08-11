#include "render_lifecycle.hpp"

#include <cstring>
#include <iostream>

namespace aexcompat::render_lifecycle {

namespace {

template <typename T>
void transfer(void* destination, std::size_t destination_offset,
              const void* source, std::size_t source_offset) {
  T value{};
  std::memcpy(&value, static_cast<const unsigned char*>(source) + source_offset,
              sizeof(value));
  std::memcpy(static_cast<unsigned char*>(destination) + destination_offset,
              &value, sizeof(value));
}

void transfer_pointer(void* destination, std::size_t destination_offset,
                      const void* source, std::size_t source_offset) {
  transfer<void*>(destination, destination_offset, source, source_offset);
}

void clear_pointer(void* destination, std::size_t offset) {
  void* value{};
  std::memcpy(static_cast<unsigned char*>(destination) + offset, &value,
              sizeof(value));
}

// FRAME_SETUP receives `out_data->width/height` already holding the extent the
// host is offering, which is the output world's own extent. An effect that
// expands revises them; one that does not leaves them, and the host reads back
// what it wrote - so "unchanged" arrives as the offered extent rather than as a
// zero the caller has to interpret.
//
// Leaving them zero is not the same thing. AE's own Basic_3D derives its answer
// from what it finds there: from 0x0 it answered 1x1, which is a shrink it never
// declared `PF_OutFlag_I_SHRINK_BUFFER` for, so the host refused the resize and
// the frame died as an output-validation failure without RENDER ever running
// (issue #984). Given the extent, it answers the extent and renders.
//
// Origin is frame-local even on routes that do not negotiate an extent.
// SmartFX states geometry through PRE_RENDER, but its resident session reuses
// these input/output buffers. Clear each named slot before FRAME_SETUP so an
// empty or invalid PRE_RENDER cannot inherit the previous frame's answer
// (issue #996).
void clear_output_origin(const Layout& layout, void* input, void* output) {
  // The two structs describe one answer. A partial layout opts out rather than
  // clearing only half and leaving the other half stale.
  if (!layout.out_origin || !layout.in_origin) return;
  const int32_t undecided_origin[2]{};
  std::memcpy(static_cast<unsigned char*>(output) + layout.out_origin,
              undecided_origin, sizeof(undecided_origin));
  std::memcpy(static_cast<unsigned char*>(input) + layout.in_origin,
              undecided_origin, sizeof(undecided_origin));
}

// The extent is overwritten on every classic frame, not filled only when
// unset. A value found there is an earlier frame's answer, so carrying it
// forward would hand FRAME_SETUP the extent the host used two frames ago. A
// layout missing any extent offset opts out; offset zero is a real field, not
// an absent destination.
void offer_output_extent(const Layout& layout, void* output, const void* world) {
  if (!world || !layout.out_width || !layout.out_height || !layout.world_width ||
      !layout.world_height)
    return;
  transfer<int32_t>(output, layout.out_width, world, layout.world_width);
  transfer<int32_t>(output, layout.out_height, world, layout.world_height);
}

}  // namespace

RenderLifecycle begin_frame(const Hooks& hooks, const Layout& layout,
                            void* input, void* output, void** params, void* world) {
  RenderLifecycle lifecycle;
  if (!hooks.invoke_frame) {
    lifecycle.setup_error = 512;
    return lifecycle;
  }
  clear_output_origin(layout, input, output);
  offer_output_extent(layout, output, world);
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

}  // namespace aexcompat::render_lifecycle
