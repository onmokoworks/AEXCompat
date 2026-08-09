// Behavioral self-test for what `begin_frame` hands FRAME_SETUP (issue #984).
//
// FRAME_SETUP is where an effect that declared PF_OutFlag_I_EXPAND_BUFFER
// revises the output extent. It is not only a place to write: AE's own Basic_3D
// derives its answer from what it finds in `out_data->width/height` on entry.
// The host used to leave those zero, so Basic_3D answered 1x1 - a shrink it had
// never declared PF_OutFlag_I_SHRINK_BUFFER for - the host refused the resize,
// and the frame died as an output-validation failure with RENDER never running.
//
// `begin_frame` takes its hooks as a table, so the contract is exercised with a
// recording fake: no plug-in, no worker process, no AEX.

#include "generated/aex_abi_contract.hpp"
#include "render_lifecycle.hpp"
#include "render_subsystem.h"

#include <array>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <initializer_list>

using namespace aexcompat::render_lifecycle;

namespace {

int failures = 0;

void check(bool condition, const char* what) {
  if (condition) return;
  std::fprintf(stderr, "FAIL: %s\n", what);
  ++failures;
}

// The offsets the classic runtime passes in its Layout. Written as literals so
// a contract regeneration that moved one has to be noticed here rather than
// absorbed, and then pinned against the generated contract so "noticed" is a
// build failure rather than a hope. The out_data ones matter most: the offer
// memcpys into them before FRAME_SETUP, so an offset that silently moved would
// have the host writing an extent over a neighbouring PF_OutData field.
constexpr std::size_t kOutWidth = 80;
constexpr std::size_t kOutHeight = 84;
constexpr std::size_t kWorldWidth = 36;
constexpr std::size_t kWorldHeight = 40;

constexpr std::size_t kOutOrigin = 88;
constexpr std::size_t kInOrigin = 276;

namespace contract = aexcompat::abi::x86_64_windows;
static_assert(kOutWidth == contract::OUT_WIDTH_OFFSET,
              "the extent this test offers is not where the host writes it");
static_assert(kOutHeight == contract::OUT_HEIGHT_OFFSET,
              "the extent this test offers is not where the host writes it");
static_assert(kOutOrigin == contract::OUT_ORIGIN_OFFSET,
              "the out_data origin this test states is not the one the host clears");
static_assert(kWorldWidth == contract::LAYER_WIDTH_OFFSET,
              "the world extent this test offers is not where the host reads it");
static_assert(kWorldHeight == contract::LAYER_HEIGHT_OFFSET,
              "the world extent this test offers is not where the host reads it");
static_assert(kInOrigin == contract::IN_OUTPUT_ORIGIN_X_OFFSET,
              "the in_data origin this test clears is not the one the host clears");
static_assert(kInOrigin + 4 == contract::IN_OUTPUT_ORIGIN_Y_OFFSET,
              "PF_InData::output_origin_y is no longer adjacent to output_origin_x");

// Filled by name for the same reason the runtime's own layout is: Layout is a
// long run of same-typed members, so positional init turns a field inserted
// into it into a silent one-slot shift rather than a build error.
constexpr Layout make_layout() {
  Layout layout{};
  layout.in_sequence_data = 320;
  layout.out_sequence_data = 56;
  layout.in_frame_data = 328;
  layout.out_frame_data = 72;
  layout.sequence_setup = 5;
  layout.sequence_setdown = 8;
  layout.frame_setup = 10;
  layout.frame_setdown = 12;
  layout.out_width = kOutWidth;
  layout.out_height = kOutHeight;
  layout.out_origin = kOutOrigin;
  layout.in_origin = kInOrigin;
  layout.world_width = kWorldWidth;
  layout.world_height = kWorldHeight;
  return layout;
}
constexpr Layout kLayout = make_layout();

// The full layout with one offset knocked out, for the cases that check an
// incomplete Layout offers nothing.
constexpr Layout layout_without(std::size_t Layout::*offset) {
  Layout layout = make_layout();
  layout.*offset = 0;
  return layout;
}

struct Buffers {
  unsigned char input[408]{};
  unsigned char output[408]{};
  unsigned char world[120]{};
};

struct Observation {
  int32_t width_on_entry{};
  int32_t height_on_entry{};
  int32_t selector{};
  int calls{};
  // What FRAME_SETUP writes back, so a fake can stand in for an expanding
  // effect as well as a passive one.
  bool revise{};
  int32_t revised_width{};
  int32_t revised_height{};
  bool state_origin{};
  int32_t origin_x{};
  int32_t origin_y{};
  int32_t answer{};
};

Observation g_observed;

int32_t recording_frame(void*, int32_t selector, void*, void* output, void**, void*) {
  if (selector != kLayout.frame_setup) return 0;
  ++g_observed.calls;
  g_observed.selector = selector;
  std::memcpy(&g_observed.width_on_entry,
              static_cast<unsigned char*>(output) + kOutWidth, sizeof(int32_t));
  std::memcpy(&g_observed.height_on_entry,
              static_cast<unsigned char*>(output) + kOutHeight, sizeof(int32_t));
  if (g_observed.revise) {
    std::memcpy(static_cast<unsigned char*>(output) + kOutWidth,
                &g_observed.revised_width, sizeof(int32_t));
    std::memcpy(static_cast<unsigned char*>(output) + kOutHeight,
                &g_observed.revised_height, sizeof(int32_t));
  }
  if (g_observed.state_origin) {
    std::memcpy(static_cast<unsigned char*>(output) + kOutOrigin,
                &g_observed.origin_x, sizeof(int32_t));
    std::memcpy(static_cast<unsigned char*>(output) + kOutOrigin + 4,
                &g_observed.origin_y, sizeof(int32_t));
  }
  return g_observed.answer;
}

void write_world_extent(Buffers& buffers, int32_t width, int32_t height) {
  std::memcpy(buffers.world + kWorldWidth, &width, sizeof(width));
  std::memcpy(buffers.world + kWorldHeight, &height, sizeof(height));
}

int32_t out_extent(const Buffers& buffers, std::size_t offset) {
  int32_t value{};
  std::memcpy(&value, buffers.output + offset, sizeof(value));
  return value;
}

Hooks recording_hooks() {
  Hooks hooks{};
  hooks.invoke_frame = &recording_frame;
  return hooks;
}

void frame_setup_receives_the_offered_output_extent() {
  Buffers buffers;
  write_world_extent(buffers, 256, 144);
  g_observed = {};

  const RenderLifecycle lifecycle = begin_frame(
      recording_hooks(), kLayout, buffers.input, buffers.output, nullptr,
      buffers.world);

  check(g_observed.calls == 1, "FRAME_SETUP is dispatched once");
  check(g_observed.width_on_entry == 256 && g_observed.height_on_entry == 144,
        "FRAME_SETUP sees the output world's extent in out_data, not zero");
  check(lifecycle.frame_started && lifecycle.setup_error == 0,
        "a successful FRAME_SETUP starts the frame");
  // The host reads back what the effect left, so a passive effect must leave
  // the offered extent rather than a zero the caller would have to interpret.
  check(out_extent(buffers, kOutWidth) == 256 && out_extent(buffers, kOutHeight) == 144,
        "a FRAME_SETUP that writes nothing leaves the offered extent standing");
}

void an_expanding_effect_still_overrides_what_it_was_offered() {
  Buffers buffers;
  write_world_extent(buffers, 256, 144);
  g_observed = {};
  g_observed.revise = true;
  g_observed.revised_width = 300;
  g_observed.revised_height = 200;

  begin_frame(recording_hooks(), kLayout, buffers.input, buffers.output, nullptr,
              buffers.world);

  check(g_observed.width_on_entry == 256 && g_observed.height_on_entry == 144,
        "an expanding effect is offered the extent before it revises it");
  check(out_extent(buffers, kOutWidth) == 300 && out_extent(buffers, kOutHeight) == 200,
        "the effect's revision survives the offer");
}

void a_layout_without_extent_offsets_leaves_out_data_untouched() {
  // The classic runtime is the only caller today, but the offsets are Layout
  // fields rather than constants precisely so a second caller can decline. One
  // that names none of them must not have the offer made on its behalf: offset
  // zero is a field in both structs, not "absent".
  Buffers buffers;
  write_world_extent(buffers, 256, 144);
  constexpr Layout bare =
      aexcompat::render_lifecycle::without_extent_negotiation(kLayout);
  g_observed = {};

  begin_frame(recording_hooks(), bare, buffers.input, buffers.output, nullptr,
              buffers.world);

  check(g_observed.width_on_entry == 0 && g_observed.height_on_entry == 0,
        "a Layout with no extent offsets offers nothing");

  // Every single offset the offer depends on, knocked out one at a time: each
  // one missing is the same refusal, from either side of the transfer.
  for (const Layout partial :
       {layout_without(&Layout::out_width), layout_without(&Layout::out_height),
        layout_without(&Layout::out_origin), layout_without(&Layout::in_origin),
        layout_without(&Layout::world_width),
        layout_without(&Layout::world_height)}) {
    Buffers half;
    write_world_extent(half, 256, 144);
    g_observed = {};
    begin_frame(recording_hooks(), partial, half.input, half.output, nullptr,
                half.world);
    check(g_observed.width_on_entry == 0 && g_observed.height_on_entry == 0,
          "a Layout naming only one side of the offer offers nothing");
    check(out_extent(half, 0) == 0,
          "an incomplete Layout does not write an extent to offset zero");
  }
}

void an_earlier_frames_answer_does_not_carry_into_the_next() {
  // `out_data` is one buffer for the whole session and nothing else clears it,
  // so a value found in these fields is an earlier frame's answer. Carrying it
  // forward would hand FRAME_SETUP an extent the host is no longer offering and
  // leave RENDER reading an origin nobody asked for this frame (#843's reuse,
  // seen from the output side).
  Buffers buffers;
  write_world_extent(buffers, 256, 144);
  const int32_t stale_extent[2]{300, 200};
  const int32_t stale_origin[2]{-44, -44};
  std::memcpy(buffers.output + kOutWidth, &stale_extent[0], sizeof(int32_t));
  std::memcpy(buffers.output + kOutHeight, &stale_extent[1], sizeof(int32_t));
  std::memcpy(buffers.output + kOutOrigin, stale_origin, sizeof(stale_origin));
  std::memcpy(buffers.input + kInOrigin, stale_origin, sizeof(stale_origin));
  g_observed = {};

  begin_frame(recording_hooks(), kLayout, buffers.input, buffers.output, nullptr,
              buffers.world);

  check(g_observed.width_on_entry == 256 && g_observed.height_on_entry == 144,
        "a previous frame's extent is replaced by the one being offered");
  check(out_extent(buffers, kOutOrigin) == 0 && out_extent(buffers, kOutOrigin + 4) == 0,
        "a previous frame's origin is cleared before FRAME_SETUP");
  int32_t in_x{}, in_y{};
  std::memcpy(&in_x, buffers.input + kInOrigin, sizeof(in_x));
  std::memcpy(&in_y, buffers.input + kInOrigin + 4, sizeof(in_y));
  check(in_x == 0 && in_y == 0,
        "in_data's copy of the origin is cleared too, so an effect that reads "
        "it during FRAME_SETUP sees this frame's state");
}

void an_origin_stated_this_frame_survives() {
  // Clearing the origin must not stop the effect from stating one: the clear
  // happens before FRAME_SETUP, and what the effect writes is what the caller
  // transfers into in_data.
  Buffers buffers;
  write_world_extent(buffers, 256, 144);
  g_observed = {};
  g_observed.revise = true;
  g_observed.revised_width = 300;
  g_observed.revised_height = 200;
  g_observed.state_origin = true;
  // PF_OutData::origin's own convention: where the input's (0,0) lands inside
  // the grown output, so non-negative. A negative pair would be a shape the
  // host refuses (see the origin bounds case below), which would make this a
  // fixture describing an answer that never survives prepare_output.
  g_observed.origin_x = 22;
  g_observed.origin_y = 28;

  begin_frame(recording_hooks(), kLayout, buffers.input, buffers.output, nullptr,
              buffers.world);

  check(out_extent(buffers, kOutOrigin) == 22 &&
            out_extent(buffers, kOutOrigin + 4) == 28,
        "an origin stated during FRAME_SETUP survives the clear");
}

void a_null_world_offers_nothing() {
  Buffers buffers;
  g_observed = {};

  begin_frame(recording_hooks(), kLayout, buffers.input, buffers.output, nullptr,
              nullptr);

  check(g_observed.calls == 1, "FRAME_SETUP still runs without an output world");
  check(g_observed.width_on_entry == 0 && g_observed.height_on_entry == 0,
        "a null world is not dereferenced for an extent");
}

void a_refused_frame_setup_does_not_start_the_frame() {
  Buffers buffers;
  write_world_extent(buffers, 256, 144);
  g_observed = {};
  g_observed.answer = 512;

  const RenderLifecycle lifecycle = begin_frame(
      recording_hooks(), kLayout, buffers.input, buffers.output, nullptr,
      buffers.world);

  check(!lifecycle.frame_started, "a refused FRAME_SETUP does not start the frame");
  check(lifecycle.setup_error == 512, "the refusal reaches the caller unchanged");
}

// `prepare_output`'s two ways of declining, which live in render_subsystem so
// they can be tested away from the dispatch owner they are called from. The
// zero pair is how an effect declined before the offer existed; the equal pair
// is how it declines now that it is handed the extent it already has.
void a_declined_resize_is_recognised_from_either_shape() {
  using aexcompat::render::output_extent_unchanged;
  check(output_extent_unchanged(256, 144, 0, 0), "a zero extent is no resize");
  check(output_extent_unchanged(256, 144, 256, 144),
        "restating the offered extent is no resize");
  check(!output_extent_unchanged(256, 144, 300, 200), "an expand is a resize");
  check(!output_extent_unchanged(256, 144, 128, 72), "a shrink is a resize");
  // One axis moving is still a resize; the host has to re-lay the world for it.
  check(!output_extent_unchanged(256, 144, 256, 200), "one axis moving is a resize");
  check(!output_extent_unchanged(256, 144, 300, 144), "the other axis too");
  // Deliberately exact. A half-zero or negative pair is a malformed answer, not
  // a decline: absorbing it here would swallow the output-validation diagnostic
  // `validate_output_extent` owes for it.
  check(!output_extent_unchanged(256, 144, 256, 0), "a half-zero extent is not a decline");
  check(!output_extent_unchanged(256, 144, 0, 144), "either half");
  check(!output_extent_unchanged(256, 144, -256, -144), "a negative extent is not a decline");
  check(!aexcompat::render::validate_output_extent(256, 144, 256, 0, 0xFFFFFFFFu),
        "and validation refuses the half-zero pair whatever the flags say");
  // `validate_output_extent` decides declining by calling the predicate above,
  // so a decline passes validation without any flag being advertised. That is
  // what lets `prepare_output` check one and then the other without the two
  // disagreeing about what a 0x0 answer means.
  check(aexcompat::render::validate_output_extent(256, 144, 0, 0, 0u),
        "a zero pair passes validation with no resize flag advertised");
  check(aexcompat::render::validate_output_extent(256, 144, 256, 144, 0u),
        "and so does restating the offered extent");
}

// PF_OutData::origin is where the input buffer's top-left sits in the output
// buffer (AE_Effect.h, on the in_data field it is copied into). Positive for an
// expand, negative for a crop. The host does not index with it, so the check is
// only that it is not absurd and that the input it places still reaches the
// output at all.
void a_stated_origin_has_to_reach_the_output() {
  using aexcompat::render::validate_output_origin;
  check(validate_output_origin(0, 0, 256, 144, 256, 144),
        "no offset at the same extent reaches the output");
  check(validate_output_origin(22, 28, 256, 144, 300, 200),
        "an expand's inset reaches the output");
  check(validate_output_origin(44, 56, 256, 144, 300, 200),
        "an inset that exactly fills the output reaches it");
  // A crop states where the input's corner is relative to the window it kept,
  // which is above and left of it, so the origin is negative. Refusing that was
  // wrong twice over: it is the canonical PF_OutFlag_I_SHRINK_BUFFER answer, and
  // an extent `validate_output_extent` accepts must not then be rejected here.
  check(validate_output_origin(-3, -2, 16, 12, 8, 6),
        "a crop to an inner rect states a negative origin and is accepted");
  check(validate_output_origin(0, 0, 16, 12, 12, 8),
        "a shrink at the output's own origin reaches it");
  check(validate_output_origin(0, 0, 256, 144, 1, 1),
        "and the smallest shrink of all still reaches it");
  // Placing the input entirely off the output describes nothing in the buffer.
  check(!validate_output_origin(300, 0, 256, 144, 300, 200),
        "an origin at the far edge puts the input past the output");
  check(!validate_output_origin(0, 200, 256, 144, 300, 200),
        "the other axis too");
  check(!validate_output_origin(-256, 0, 256, 144, 300, 200),
        "an origin a whole source-width above the output puts it past the other side");
  check(!validate_output_origin(0, -144, 256, 144, 300, 200), "and that axis too");
  check(!validate_output_origin(1 << 25, 0, 256, 144, 300, 200),
        "an absurd magnitude is refused before anything derives arithmetic from it");
  check(!validate_output_origin(0, -(1 << 25), 256, 144, 300, 200),
        "in either direction");
  check(!validate_output_origin(0, 0, 16, 12, -1, 8),
        "a negative output extent is refused rather than read as unbounded");
  check(!validate_output_origin(0, 0, -1, 12, 12, 8), "as is a negative source extent");
}

// The hint an accepted shrink hands back has to be inside the buffer that now
// backs the output, without being grown, inverted, or moved into a coordinate
// frame the effect was not handed.
void the_extent_hint_comes_back_inside_the_new_buffer() {
  using aexcompat::render::extent_hint_within;
  const auto same = [](const std::array<int32_t, 4>& left,
                       const std::array<int32_t, 4>& right) { return left == right; };
  check(same(extent_hint_within({0, 0, 16, 12}, 8, 6), {0, 0, 8, 6}),
        "a shrink brings the hint back to the buffer's edge");
  check(same(extent_hint_within({0, 0, 16, 12}, 64, 48), {0, 0, 16, 12}),
        "an expand leaves it alone: growing it would name rows the input never had");
  check(same(extent_hint_within({3, 2, 11, 8}, 8, 6), {3, 2, 8, 6}),
        "a partial hint keeps its top-left and loses only the overhang");
  check(same(extent_hint_within({3, 2, 11, 8}, 64, 48), {3, 2, 11, 8}),
        "and keeps all of it when it already fits");
  // Never inverted: a top-left past the new edge comes back to the edge and the
  // bottom-right follows it, rather than crossing over into a negative extent.
  const std::array<int32_t, 4> collapsed = extent_hint_within({30, 20, 40, 30}, 8, 6);
  check(collapsed[2] >= collapsed[0] && collapsed[3] >= collapsed[1],
        "a hint entirely outside the new buffer stays non-inverted");
  check(collapsed[0] <= 8 && collapsed[1] <= 6 && collapsed[2] <= 8 && collapsed[3] <= 6,
        "and lands inside it");
  // The rect keeps the frame it was written in. Translating it by the accepted
  // origin was tried and withdrawn: this host's own PF_Iterate refuses an area
  // wider than the input world, so an output-frame hint breaks the canonical
  // iterate call for every expanding effect (issue #997 owns which frame AE
  // uses).
  check(same(extent_hint_within({0, 0, 37, 23}, 137, 123), {0, 0, 37, 23}),
        "an expand's hint stays in the input's frame, where iterate accepts it");
}

// The two origin conventions meet in exactly one place, and the direction is
// not guessable from either field's name. The worked example is the origin
// probe's: it grows by 4 on each axis and places the input 3px inside the grown
// buffer, so the buffer's own top-left is 3px outside the layer.
void the_frame_report_origin_is_the_negation_of_the_stated_one() {
  using aexcompat::render::layer_origin_from_input_origin;
  check(layer_origin_from_input_origin(3) == -3,
        "an expand that insets the input by 3 starts its buffer 3 above the layer");
  check(layer_origin_from_input_origin(0) == 0, "no inset is no displacement");
  // A crop states a negative origin (the input's corner is outside the window
  // it kept), so its buffer starts inside the layer and the report is positive.
  check(layer_origin_from_input_origin(-3) == 3,
        "a crop that kept an inner rect starts its buffer 3 inside the layer");
  // Round-tripping is what makes the direction checkable without asserting the
  // implementation against itself: whichever way the conversion runs, applying
  // it twice has to land back where it started, and applying it once must not.
  for (const int32_t stated : {3, -3, 22, -128}) {
    check(layer_origin_from_input_origin(layer_origin_from_input_origin(stated)) == stated,
          "the conversion is its own inverse");
    check(layer_origin_from_input_origin(stated) != stated,
          "and a non-zero origin does not survive it unchanged");
  }
}

// SmartFX runs the same lifecycle without the extent negotiation, and its
// layout is derived from the classic one rather than copied. Derivation has to
// keep every selector offset and drop every extent offset; keeping one extent
// offset would invite FRAME_SETUP to revise an extent nothing reads back.
void the_smart_layout_keeps_the_selectors_and_drops_the_extents() {
  constexpr Layout smart =
      aexcompat::render_lifecycle::without_extent_negotiation(kLayout);
  check(smart.in_sequence_data == kLayout.in_sequence_data &&
            smart.out_sequence_data == kLayout.out_sequence_data &&
            smart.in_frame_data == kLayout.in_frame_data &&
            smart.out_frame_data == kLayout.out_frame_data &&
            smart.sequence_setup == kLayout.sequence_setup &&
            smart.sequence_setdown == kLayout.sequence_setdown &&
            smart.frame_setup == kLayout.frame_setup &&
            smart.frame_setdown == kLayout.frame_setdown,
        "the derived layout drives the same selectors and data slots");
  check(smart.out_width == 0 && smart.out_height == 0 && smart.out_origin == 0 &&
            smart.in_origin == 0 && smart.world_width == 0 &&
            smart.world_height == 0,
        "and names no extent offsets at all");

  Buffers buffers;
  write_world_extent(buffers, 256, 144);
  g_observed = {};
  begin_frame(recording_hooks(), smart, buffers.input, buffers.output, nullptr,
              buffers.world);
  check(g_observed.width_on_entry == 0 && g_observed.height_on_entry == 0,
        "so FRAME_SETUP on the derived layout is offered nothing");
}

}  // namespace

int main() {
  frame_setup_receives_the_offered_output_extent();
  an_expanding_effect_still_overrides_what_it_was_offered();
  a_layout_without_extent_offsets_leaves_out_data_untouched();
  an_earlier_frames_answer_does_not_carry_into_the_next();
  an_origin_stated_this_frame_survives();
  a_declined_resize_is_recognised_from_either_shape();
  a_stated_origin_has_to_reach_the_output();
  the_extent_hint_comes_back_inside_the_new_buffer();
  the_frame_report_origin_is_the_negation_of_the_stated_one();
  the_smart_layout_keeps_the_selectors_and_drops_the_extents();
  a_null_world_offers_nothing();
  a_refused_frame_setup_does_not_start_the_frame();
  if (failures == 0)
    std::printf("{\"render_lifecycle_selftest\":\"passed\"}\n");
  return failures == 0 ? 0 : 1;
}
