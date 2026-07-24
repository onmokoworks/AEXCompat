#pragma once

#include "worker_host_suite_router.hpp"

#include <cstddef>
#include <array>

namespace aexcompat::worker_runtime::host_suites {

struct CatalogConfiguration {
  const StaticSuite* suites{};
  std::size_t suite_count{};
  Provider scene_provider{};
};

struct AssemblyHooks {
  std::array<void*, 4> path_query{};
  std::array<void*, 11> path_data{};
  void* duck{};
  void* effect_ui{};
  void* adv_info{};
  void* adv_info3{};
  std::array<void*, 2> drawbot_draw{};
  void* drawbot_new_pen{};
  void* drawbot_new_brush{};
  void* drawbot_new_path{};
  void* drawbot_release{};
  void* drawbot_paint_rect{};
  void* drawbot_fill_path{};
  void* drawbot_stroke_path{};
  void* drawbot_path_point{};
  void* drawbot_add_rect{};
  void* drawing_reference{};
  void* context_async_manager{};
  void* overlay_foreground{};
  void* overlay_stroke_path{};
  std::array<void*, 15> app{};
  std::array<void*, 19> ansi{};
  void* dynamic_stream_set_flag{};
  std::array<void*, 13> aegp_world{};
  std::array<void*, 14> layer_render_options1{};
  std::array<void*, 15> layer_render_options2{};
  std::array<void*, 17> render_options1{};
  std::array<void*, 23> render_options4{};
  std::array<void*, 10> render2{};
  std::array<void*, 12> render5{};
  std::array<void*, 14> render8{};
  std::array<void*, 2> render_async_manager{};
};

bool configure_suite_assembly(const AssemblyHooks& hooks);
const void* provide_path_query1(void*);
const void* provide_path_data1(void*);
const void* provide_duck1(void*);
const void* provide_effect_ui1(void*);
const void* provide_adv_app1(void*);
const void* provide_adv_app2(void*);
const void* provide_drawbot_draw1(void*);
const void* provide_drawbot_supplier1(void*);
const void* provide_drawbot_surface2(void*);
const void* provide_drawbot_path1(void*);
const void* provide_custom_ui1(void*);
const void* provide_custom_ui2(void*);
const void* provide_overlay_theme1(void*);
const void* provide_app_suite4(void*);
const void* provide_app_suite5(void*);
const void* provide_app_suite6(void*);
const void* provide_ansi1(void*);
const void* provide_ansi2(void*);
const void* provide_dynamic_stream2(void*);
const void* provide_aegp_world_suite3(void*);
const void* provide_layer_render_options1(void*);
const void* provide_layer_render_options2(void*);
const void* provide_render_options1(void*);
const void* provide_render_options4(void*);
const void* provide_render_suite2(void*);
const void* provide_render_suite5(void*);
const void* provide_render_suite8(void*);
const void* provide_render_async_manager1(void*);
const void* layer_render_options_suite(int version) noexcept;
const void* adv_app_suite(int version) noexcept;

// Copies the immutable registration metadata into catalog-owned storage.
// Callback tables and availability contexts remain host-owned opaque pointers.
bool configure_host_suite_catalog(const CatalogConfiguration& configuration);
bool host_suite_catalog_configured() noexcept;

int32_t acquire_catalog_suite(const char* name, int32_t version,
                              const void** suite, TraceWriter* trace_writer);
int32_t release_catalog_suite(const char* name, int32_t version,
                              TraceWriter* trace_writer);

}  // namespace aexcompat::worker_runtime::host_suites
