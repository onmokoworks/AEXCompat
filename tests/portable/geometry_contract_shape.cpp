// Compile-time native shape check only. No Windows runtime or ABI claim.
#include "render_subsystem.h"
#include <type_traits>
using namespace aexcompat::render;
static_assert(std::is_same_v<decltype(SpatialRatio::numerator), int32_t>);
static_assert(std::is_same_v<decltype(SpatialRatio::denominator), uint32_t>);
static_assert(std::is_same_v<decltype(RenderContextState::downsample_x), SpatialRatio>);
static_assert(std::is_same_v<decltype(RenderContextState::pixel_aspect_ratio), SpatialRatio>);
static_assert(std::is_same_v<decltype(DiagnosticWorldLayout::request_rect), std::array<int32_t,4>>);
static_assert(std::is_same_v<decltype(DiagnosticWorldLayout::extent_hint), std::array<int32_t,4>>);
static_assert(std::is_same_v<decltype(WorldLayout::rowbytes), int32_t>);
static_assert(std::is_same_v<decltype(SmartOutputBounds::result_rect), std::array<int32_t,4>>);
static_assert(layer_origin_from_input_origin(3) == -3);
static_assert(layer_origin_from_input_origin(-2) == 2);
int main() { return 0; }
