#pragma once
#include <array>
#include <cstdint>
namespace aexcompat::l2_detail {
bool verify_aegp_render_options_suite1();
bool verify_aegp_item_staged_worlds();
bool verify_aegp_layer_render_options_suite2();

// AEGP render-options probe fixtures, owned by worker_aegp_render_selftests.cpp
// (issue #126 Phase D). The render probes in verify_aegp_render_options_suite1
// write them and worker_main only wires their addresses into
// custom_selftests::dispatch so the selftest JSON reports can read them back.
// Lifetime: zero-initialized at process start, live for the whole worker
// process, never torn down.
extern std::array<uint8_t, 4> g_render_options_baseline8;
extern std::array<uint8_t, 4> g_render_options_time8;
extern std::array<uint8_t, 4> g_render_options_downsample8;
extern std::array<uint8_t, 4> g_render_options_roi_outside8;
extern std::array<uint8_t, 4> g_render_options_roi_inside8;
extern std::array<uint8_t, 4> g_render_options_field_excluded8;
extern std::array<uint8_t, 4> g_render_options_matte8;
extern std::array<uint16_t, 4> g_render_options_argb16;
extern std::array<float, 4> g_render_options_argb32f;
}
