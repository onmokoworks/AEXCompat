#pragma once

#include <cstddef>
#include <cstdint>

namespace aexcompat::render_lifecycle {

struct FrameSetupOutput {
  bool available{};
  int32_t width{};
  int32_t height{};
  int32_t origin_x{};
  int32_t origin_y{};
};

struct RenderLifecycle {
  bool sequence_started{};
  bool frame_started{};
  int32_t setup_error{};
  // PF_OutData is reused by selectors that follow FRAME_SETUP. Geometry is
  // FRAME_SETUP's answer, so the host owns this copy before dispatching any of
  // those selectors (#999).
  FrameSetupOutput frame_setup_output{};
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
  // Where the output extent an expanding effect may revise lives in out_data,
  // and where the extent the host is offering lives in the output world it
  // hands to the same selector (issue #984).
  std::size_t out_width{};
  std::size_t out_height{};
  // Origin is frame-local independently of extent negotiation. SmartFX keeps
  // these offsets so a resident session cannot carry an earlier PRE_RENDER
  // answer into the next frame (issue #996).
  std::size_t out_origin{};
  // Where in_data carries the same origin back to the plug-in. Cleared beside
  // out_data's so an effect that reads it during FRAME_SETUP sees this frame's
  // state - undecided - rather than an earlier frame's answer.
  std::size_t in_origin{};
  std::size_t world_width{};
  std::size_t world_height{};
};

// The same layout with the extent negotiation dropped, for a route that runs
// the lifecycle but never reads an extent back (SmartFX states its geometry
// through PRE_RENDER instead).
//
// Subtractive on purpose. Listing the members to keep would make this an
// allowlist, so a member added to Layout later would arrive as zero on the
// SmartFX route - dispatching selector 0, or transferring to offset 0 of
// in_data - while the comment claimed derivation prevented exactly that. Naming
// only what the extent negotiation owns means a new member is carried by
// default and a member removed from Layout is a build error here.
constexpr Layout without_extent_negotiation(const Layout& layout) {
  Layout reduced = layout;
  reduced.out_width = 0;
  reduced.out_height = 0;
  reduced.world_width = 0;
  reduced.world_height = 0;
  return reduced;
}

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

}  // namespace aexcompat::render_lifecycle
