#include "worker_suite_abi.hpp"

namespace aexcompat::suite_abi {
namespace {

AegpWorldSuite3 g_aegp_world_suite3{};
AegpLayerRenderOptionsSuite1 g_aegp_layer_render_options_suite1{};
AegpLayerRenderOptionsSuite2 g_aegp_layer_render_options_suite2{};
AegpRenderOptionsSuite1 g_aegp_render_options_suite1{};
AegpRenderOptionsSuite4 g_aegp_render_options_suite4{};

}  // namespace

AegpWorldSuite3& aegp_world_suite3_table() noexcept {
  return g_aegp_world_suite3;
}

AegpLayerRenderOptionsSuite1& aegp_layer_render_options_suite1_table() noexcept {
  return g_aegp_layer_render_options_suite1;
}
AegpLayerRenderOptionsSuite2& aegp_layer_render_options_suite2_table() noexcept {
  return g_aegp_layer_render_options_suite2;
}
AegpRenderOptionsSuite1& aegp_render_options_suite1_table() noexcept {
  return g_aegp_render_options_suite1;
}
AegpRenderOptionsSuite4& aegp_render_options_suite4_table() noexcept {
  return g_aegp_render_options_suite4;
}

}  // namespace aexcompat::suite_abi
