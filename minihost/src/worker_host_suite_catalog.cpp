#include "worker_host_suite_catalog.hpp"
#include "worker_pf_ansi_runtime.hpp"
#include "worker_suite_registry.hpp"

#include <algorithm>
#include <mutex>
#include <vector>

namespace aexcompat::worker_runtime::host_suites {
namespace {

struct OwnedCatalog {
  std::mutex mutex;
  std::vector<StaticSuite> suites;
  StaticProviderCatalog static_catalog{};
  Provider providers[2]{};
  ProviderCatalog provider_catalog{};
  bool configured{};
  AssemblyHooks assembly{};
  bool assembly_configured{};
  std::array<void*, 4> path_query{};
  std::array<void*, 11> path_data{};
  std::array<void*, 1> duck{};
  std::array<void*, 1> effect_ui{};
  std::array<void*, 10> adv_app1{};
  std::array<void*, 11> adv_app2{};
  std::array<void*, 2> drawbot_draw{};
  std::array<void*, 13> drawbot_supplier{};
  std::array<void*, 17> drawbot_surface{};
  std::array<void*, 6> drawbot_path{};
  std::array<void*, 1> custom_ui1{};
  std::array<void*, 2> custom_ui2{};
  std::array<void*, 8> overlay{};
  std::array<void*, 11> app4{};
  std::array<void*, 12> app5{};
  std::array<void*, 15> app6{};
  std::array<void*, 19> ansi{};
  std::array<void*, 21> ansi2{};
  std::array<void*, 14> dynamic_stream2{};
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

OwnedCatalog& state() {
  static OwnedCatalog catalog;
  return catalog;
}

}  // namespace

bool configure_suite_assembly(const AssemblyHooks& hooks) {
  if (!hooks.duck || !hooks.effect_ui ||
      !hooks.adv_info || !hooks.adv_info3 || !hooks.dynamic_stream_set_flag)
    return false;
  auto& catalog = state();
  std::lock_guard<std::mutex> lock(catalog.mutex);
  catalog.assembly = hooks;
  catalog.assembly_configured = true;
  return true;
}

namespace {
template <UnsupportedSuiteId Suite, std::size_t N>
void fill_unsupported(std::array<void*, N>& suite) {
  suite = unsupported_suite_slots<Suite, N>();
}

template <std::size_t N>
const void* populate_app(std::array<void*, N>& suite, bool language, bool progress) {
  auto& c = state();
  std::size_t out = 0, in = 0;
  suite[out++] = c.assembly.app[in++]; // background
  suite[out++] = c.assembly.app[in++]; // color
  if (language) suite[out++] = c.assembly.app[in];
  ++in;
  while (in < 12) suite[out++] = c.assembly.app[in++];
  if (progress) while (in < c.assembly.app.size()) suite[out++] = c.assembly.app[in++];
  return suite.data();
}
}  // namespace

const void* provide_path_query1(void*) { auto& c=state(); c.path_query=c.assembly.path_query; return c.path_query.data(); }
const void* provide_path_data1(void*) { auto& c=state(); c.path_data=c.assembly.path_data; return c.path_data.data(); }
const void* provide_duck1(void*) { auto& c=state(); c.duck[0]=c.assembly.duck; return c.duck.data(); }
const void* provide_effect_ui1(void*) { auto& c=state(); c.effect_ui[0]=c.assembly.effect_ui; return c.effect_ui.data(); }
const void* provide_adv_app1(void*) { auto& c=state(); fill_unsupported<UnsupportedSuiteId::pf_ae_adv_app_1>(c.adv_app1); c.adv_app1[6]=c.assembly.adv_info; c.adv_app1[8]=c.assembly.adv_info3; return c.adv_app1.data(); }
const void* provide_adv_app2(void*) { auto& c=state(); fill_unsupported<UnsupportedSuiteId::pf_ae_adv_app_2>(c.adv_app2); c.adv_app2[6]=c.assembly.adv_info; c.adv_app2[8]=c.assembly.adv_info3; return c.adv_app2.data(); }
const void* provide_drawbot_draw1(void*) { auto& c=state(); c.drawbot_draw=c.assembly.drawbot_draw; return c.drawbot_draw.data(); }
const void* provide_drawbot_supplier1(void*) { auto& c=state(); fill_unsupported<UnsupportedSuiteId::drawbot_supplier_1>(c.drawbot_supplier); c.drawbot_supplier[0]=c.assembly.drawbot_new_pen; c.drawbot_supplier[1]=c.assembly.drawbot_new_brush; c.drawbot_supplier[6]=c.assembly.drawbot_new_path; c.drawbot_supplier[12]=c.assembly.drawbot_release; return c.drawbot_supplier.data(); }
const void* provide_drawbot_surface2(void*) { auto& c=state(); fill_unsupported<UnsupportedSuiteId::drawbot_surface_2>(c.drawbot_surface); c.drawbot_surface[2]=c.assembly.drawbot_paint_rect; c.drawbot_surface[3]=c.assembly.drawbot_fill_path; c.drawbot_surface[4]=c.assembly.drawbot_stroke_path; return c.drawbot_surface.data(); }
const void* provide_drawbot_path1(void*) { auto& c=state(); fill_unsupported<UnsupportedSuiteId::drawbot_path_1>(c.drawbot_path); c.drawbot_path[0]=c.assembly.drawbot_path_point; c.drawbot_path[1]=c.assembly.drawbot_path_point; c.drawbot_path[3]=c.assembly.drawbot_add_rect; return c.drawbot_path.data(); }
const void* provide_custom_ui1(void*) { auto& c=state(); c.custom_ui1[0]=c.assembly.drawing_reference; return c.custom_ui1.data(); }
const void* provide_custom_ui2(void*) { auto& c=state(); c.custom_ui2={c.assembly.drawing_reference,c.assembly.context_async_manager}; return c.custom_ui2.data(); }
const void* provide_overlay_theme1(void*) { auto& c=state(); fill_unsupported<UnsupportedSuiteId::pf_effect_custom_ui_overlay_theme_1>(c.overlay); c.overlay[0]=c.assembly.overlay_foreground; c.overlay[5]=c.assembly.overlay_stroke_path; return c.overlay.data(); }
const void* provide_app_suite4(void*) { auto& c=state(); return populate_app(c.app4,false,false); }
const void* provide_app_suite5(void*) { auto& c=state(); return populate_app(c.app5,true,false); }
const void* provide_app_suite6(void*) { auto& c=state(); return populate_app(c.app6,true,true); }
const void* provide_ansi1(void*) { auto& c=state(); c.ansi=c.assembly.ansi; return c.ansi.data(); }
const void* provide_ansi2(void*) {
  auto& c=state();
  c.ansi2 = {};
  std::copy(c.assembly.ansi.begin(), c.assembly.ansi.end(), c.ansi2.begin());
  // Index 20 (0xa0, issue #362): bounded string copy used by the VR family to
  // fill fixed-size parameter-name fields. Index 19 stays null (unused so
  // far; a null there fails loudly instead of guessing an ABI).
  c.ansi2[20] = reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_strcpy_bounded);
  return c.ansi2.data();
}
const void* provide_dynamic_stream2(void*) { auto& c=state(); fill_unsupported<UnsupportedSuiteId::aegp_dynamic_stream_2>(c.dynamic_stream2); c.dynamic_stream2[5]=c.assembly.dynamic_stream_set_flag; return c.dynamic_stream2.data(); }
const void* provide_aegp_world_suite3(void*) { auto& c=state(); c.aegp_world=c.assembly.aegp_world; return c.aegp_world.data(); }
const void* provide_layer_render_options1(void*) { auto& c=state(); c.layer_render_options1=c.assembly.layer_render_options1; return c.layer_render_options1.data(); }
const void* provide_layer_render_options2(void*) { auto& c=state(); c.layer_render_options2=c.assembly.layer_render_options2; return c.layer_render_options2.data(); }
const void* provide_render_options1(void*) { auto& c=state(); c.render_options1=c.assembly.render_options1; return c.render_options1.data(); }
const void* provide_render_options4(void*) { auto& c=state(); c.render_options4=c.assembly.render_options4; return c.render_options4.data(); }
const void* provide_render_suite2(void*) { auto& c=state(); c.render2=c.assembly.render2; return c.render2.data(); }
const void* provide_render_suite5(void*) { auto& c=state(); c.render5=c.assembly.render5; return c.render5.data(); }
const void* provide_render_suite8(void*) { auto& c=state(); c.render8=c.assembly.render8; return c.render8.data(); }
const void* provide_render_async_manager1(void*) { auto& c=state(); c.render_async_manager=c.assembly.render_async_manager; return c.render_async_manager.data(); }
const void* layer_render_options_suite(int version) noexcept { auto& c=state(); return version==1 ? static_cast<const void*>(c.layer_render_options1.data()) : static_cast<const void*>(c.layer_render_options2.data()); }
const void* adv_app_suite(int version) noexcept { auto& c=state(); return version==1 ? static_cast<const void*>(c.adv_app1.data()) : static_cast<const void*>(c.adv_app2.data()); }

bool configure_host_suite_catalog(const CatalogConfiguration& configuration) {
  if (!configuration.suites || configuration.suite_count == 0 ||
      !configuration.scene_provider.resolve) return false;
  auto& catalog = state();
  std::lock_guard<std::mutex> lock(catalog.mutex);
  if (catalog.configured) return true;
  try {
    catalog.suites.assign(configuration.suites,
                          configuration.suites + configuration.suite_count);
  } catch (...) {
    return false;
  }
  catalog.static_catalog = {catalog.suites.data(), catalog.suites.size()};
  catalog.providers[0] = configuration.scene_provider;
  catalog.providers[1] = {&resolve_static_provider, &catalog.static_catalog};
  catalog.provider_catalog = {catalog.providers, 2, nullptr, nullptr};
  catalog.configured = true;
  return true;
}

bool host_suite_catalog_configured() noexcept {
  auto& catalog = state();
  std::lock_guard<std::mutex> lock(catalog.mutex);
  return catalog.configured;
}

int32_t acquire_catalog_suite(const char* name, int32_t version,
                              const void** suite, TraceWriter* trace_writer) {
  auto& catalog = state();
  {
    std::lock_guard<std::mutex> lock(catalog.mutex);
    if (!catalog.configured) {
      if (suite) *suite = nullptr;
      return 4;
    }
  }
  return acquire_host_suite(catalog.provider_catalog, name, version, suite,
                            trace_writer);
}

int32_t release_catalog_suite(const char* name, int32_t version,
                              TraceWriter* trace_writer) {
  return release_host_suite(name, version, trace_writer);
}

}  // namespace aexcompat::worker_runtime::host_suites
