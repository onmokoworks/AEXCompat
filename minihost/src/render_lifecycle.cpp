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
// A `Layout` that names none of these offsets gets nothing written, which is how
// a caller with no output world of its own, or one that reads the answer back
// nowhere, opts out. They are required together: writing an extent to offset 0
// of out_data, or reading one from offset 0 of the world, would corrupt a field
// rather than skip the offer.
//
// The origin is cleared and the extent overwritten on every frame, not filled
// only when unset. `out_data` is one buffer for the whole session here, and
// nothing else ever clears these fields, so a value found in them is an earlier
// frame's answer - carrying it forward would hand FRAME_SETUP the extent the
// host used two frames ago and leave RENDER reading an origin no one asked for
// this frame (#843's reuse, seen from the output side).
void offer_output_extent(const Layout& layout, void* input, void* output,
                         const void* world) {
  if (!world || !layout.out_width || !layout.out_height || !layout.out_origin ||
      !layout.in_origin || !layout.world_width || !layout.world_height)
    return;
  const int32_t undecided_origin[2]{};
  std::memcpy(static_cast<unsigned char*>(output) + layout.out_origin,
              undecided_origin, sizeof(undecided_origin));
  std::memcpy(static_cast<unsigned char*>(input) + layout.in_origin,
              undecided_origin, sizeof(undecided_origin));
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
  offer_output_extent(layout, input, output, world);
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
