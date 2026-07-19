#include "worker_drawbot_runtime.hpp"

#include "worker_mask_runtime_internal.hpp"
#include "worker_pf_helper_runtime.hpp"
#include "worker_ui_event_execution.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <memory>
#include <new>
#include <unordered_map>

namespace aexcompat::l2_detail {

// Custom-UI/Drawbot/App telemetry read and written through its owner,
// aexcompat::worker_runtime::ui_event_execution::custom_ui_telemetry()
// (issue #126 Phase D); these references keep the g_* spellings.
namespace {
auto& g_custom_ui_telemetry =
    aexcompat::worker_runtime::ui_event_execution::custom_ui_telemetry();
auto& g_drawbot_objects_created = g_custom_ui_telemetry.drawbot_objects_created;
auto& g_drawbot_objects_released = g_custom_ui_telemetry.drawbot_objects_released;
auto& g_drawbot_paint_rect_calls = g_custom_ui_telemetry.drawbot_paint_rect_calls;
auto& g_drawbot_fill_path_calls = g_custom_ui_telemetry.drawbot_fill_path_calls;
auto& g_drawbot_stroke_path_calls = g_custom_ui_telemetry.drawbot_stroke_path_calls;
auto& g_drawbot_invalid_operations = g_custom_ui_telemetry.drawbot_invalid_operations;
auto& g_drawbot_get_supplier_calls = g_custom_ui_telemetry.drawbot_get_supplier_calls;
auto& g_drawbot_get_surface_calls = g_custom_ui_telemetry.drawbot_get_surface_calls;
auto& g_drawbot_get_drawing_ref_calls = g_custom_ui_telemetry.drawbot_get_drawing_ref_calls;
auto& g_overlay_stroke_path_calls = g_custom_ui_telemetry.overlay_stroke_path_calls;
auto& g_app_get_background_color_calls = g_custom_ui_telemetry.app_get_background_color_calls;
auto& g_app_color_picker_calls = g_custom_ui_telemetry.app_color_picker_calls;
auto& g_app_invalidate_rect_calls = g_custom_ui_telemetry.app_invalidate_rect_calls;
auto& g_app_progress_dialogs_created = g_custom_ui_telemetry.app_progress_dialogs_created;
auto& g_app_progress_dialogs_disposed = g_custom_ui_telemetry.app_progress_dialogs_disposed;
auto& g_app_picker_color = g_custom_ui_telemetry.app_picker_color;
auto& g_app_invalidated_rect = g_custom_ui_telemetry.app_invalidated_rect;
auto& g_ui_coordinate_transform_calls = g_custom_ui_telemetry.ui_coordinate_transform_calls;
auto& g_render_ui_context_active = g_custom_ui_telemetry.render_ui_context_active;
auto& g_drawbot_fill_colors = g_custom_ui_telemetry.drawbot_fill_colors;
}  // namespace

// Retained custom-UI callback-ABI state (issue #126 Phase D): the opaque
// Drawbot refs and the live object table are handed to the plug-in as raw
// pointers by the Drawbot suite callbacks below, so their storage stays with
// that ABI; the plain-data counters live in
// ui_event_execution::custom_ui_telemetry(). Lifetime: process-lifetime; the
// object table is balanced by the create/release callbacks.
struct DrawbotOpaque { uint32_t tag; };
DrawbotOpaque g_drawbot_draw{0x44524157};
DrawbotOpaque g_drawbot_supplier{0x53555050};
DrawbotOpaque g_drawbot_surface{0x53555246};
enum class DrawbotObjectKind { Pen, Brush, Path };
struct DrawbotObject {
  DrawbotObjectKind kind{};
  std::array<float, 4> color{};
  std::array<float, 4> rect{};
  float pen_size{};
  uint32_t path_points{};
};
std::unordered_map<void*, std::unique_ptr<DrawbotObject>> g_drawbot_objects;

// Retained custom-UI callback-ABI state: the event context block whose
// address is written into the PF event ABI, owned beside the callbacks that
// hand it out. Lifetime: process-lifetime, re-armed per UI dispatch.
HostUiContext g_ui_context;
HostUiContext* g_ui_context_pointer = &g_ui_context;
struct PfHelperUiContextScope {
  explicit PfHelperUiContextScope(int32_t context)
      : runtime_scope(context), previous_active(g_render_ui_context_active) {
    g_render_ui_context_active = context >= 0 && context < 3;
  }
  ~PfHelperUiContextScope() { g_render_ui_context_active = previous_active; }
  aexcompat::pf_helper::UiContextScope runtime_scope;
  bool previous_active;
};
void* enter_custom_ui_context(int32_t context) {
  return new (std::nothrow) PfHelperUiContextScope(context);
}
void leave_custom_ui_context(void* scope) {
  delete static_cast<PfHelperUiContextScope*>(scope);
}
bool custom_ui_context_stable() {
  return g_ui_context_pointer == &g_ui_context;
}
void set_custom_ui_context_tool(int32_t context) {
  aexcompat::pf_helper::set_context_tool(
      context, aexcompat::pf_helper::kExtendedToolMin);
}

int32_t __cdecl drawbot_get_supplier(void* draw, void** supplier) {
  if (draw != &g_drawbot_draw || !supplier) return 4;
  *supplier = &g_drawbot_supplier;
  ++g_drawbot_get_supplier_calls;
  return 0;
}
int32_t __cdecl drawbot_get_surface(void* draw, void** surface) {
  if (draw != &g_drawbot_draw || !surface) return 4;
  *surface = &g_drawbot_surface;
  ++g_drawbot_get_surface_calls;
  return 0;
}
int32_t new_drawbot_object(DrawbotObjectKind kind, void** output) {
  if (!output || g_drawbot_objects.size() >= 256) return 4;
  auto object = std::make_unique<DrawbotObject>();
  object->kind = kind;
  void* key = object.get();
  g_drawbot_objects.emplace(key, std::move(object));
  ++g_drawbot_objects_created;
  *output = key;
  return 0;
}
int32_t __cdecl drawbot_new_pen(void* supplier, const float* color, float size, void** pen) {
  if (supplier != &g_drawbot_supplier || !color || !std::isfinite(size) || size <= 0) return 4;
  const int32_t error = new_drawbot_object(DrawbotObjectKind::Pen, pen);
  if (!error) {
    std::copy_n(color, 4, g_drawbot_objects[*pen]->color.begin());
    g_drawbot_objects[*pen]->pen_size = size;
  }
  return error;
}
int32_t __cdecl drawbot_new_brush(void* supplier, const float* color, void** brush) {
  if (supplier != &g_drawbot_supplier || !color) return 4;
  const int32_t error = new_drawbot_object(DrawbotObjectKind::Brush, brush);
  if (!error) std::copy_n(color, 4, g_drawbot_objects[*brush]->color.begin());
  return error;
}
int32_t __cdecl drawbot_new_path(void* supplier, void** path) {
  return supplier == &g_drawbot_supplier ? new_drawbot_object(DrawbotObjectKind::Path, path) : 4;
}
int32_t __cdecl drawbot_release_object(void* object) {
  const auto found = g_drawbot_objects.find(object);
  if (found == g_drawbot_objects.end()) { ++g_drawbot_invalid_operations; return 4; }
  g_drawbot_objects.erase(found);
  ++g_drawbot_objects_released;
  return 0;
}
int32_t __cdecl drawbot_add_rect(void* path, const float* rect) {
  const auto found = g_drawbot_objects.find(path);
  if (found == g_drawbot_objects.end() || found->second->kind != DrawbotObjectKind::Path || !rect)
    return 4;
  if (!std::all_of(rect, rect + 4, [](float value) { return std::isfinite(value); }) ||
      rect[2] < 0 || rect[3] < 0) return 4;
  std::copy_n(rect, 4, found->second->rect.begin());
  return 0;
}
int32_t __cdecl drawbot_path_point(void* path, float x, float y) {
  const auto found = g_drawbot_objects.find(path);
  if (found == g_drawbot_objects.end() || found->second->kind != DrawbotObjectKind::Path ||
      !std::isfinite(x) || !std::isfinite(y) || found->second->path_points >= 4096) return 4;
  ++found->second->path_points;
  return 0;
}
int32_t __cdecl drawbot_paint_rect(void* surface, const float* color, const float* rect) {
  if (surface != &g_drawbot_surface || !color || !rect) return 4;
  ++g_drawbot_paint_rect_calls;
  return 0;
}
int32_t __cdecl drawbot_fill_path(void* surface, void* brush, void* path, int32_t fill_type) {
  const auto brush_it = g_drawbot_objects.find(brush), path_it = g_drawbot_objects.find(path);
  if (surface != &g_drawbot_surface || fill_type != 1 ||
      brush_it == g_drawbot_objects.end() || path_it == g_drawbot_objects.end() ||
      brush_it->second->kind != DrawbotObjectKind::Brush ||
      path_it->second->kind != DrawbotObjectKind::Path) return 4;
  g_drawbot_fill_colors.push_back(brush_it->second->color);
  ++g_drawbot_fill_path_calls;
  return 0;
}
int32_t __cdecl drawbot_stroke_path(void* surface, void* pen, void* path) {
  const auto pen_it = g_drawbot_objects.find(pen), path_it = g_drawbot_objects.find(path);
  if (surface != &g_drawbot_surface || pen_it == g_drawbot_objects.end() ||
      path_it == g_drawbot_objects.end() || pen_it->second->kind != DrawbotObjectKind::Pen ||
      path_it->second->kind != DrawbotObjectKind::Path) return 4;
  ++g_drawbot_stroke_path_calls;
  return 0;
}
int32_t __cdecl get_drawing_reference(void* context, void** drawing) {
  if (context != &g_ui_context_pointer || !drawing) return 4;
  *drawing = &g_drawbot_draw;
  ++g_drawbot_get_drawing_ref_calls;
  return 0;
}

int32_t __cdecl app_get_background_color(uint16_t* color) {
  if (!color) return 4;
  color[0] = color[1] = color[2] = 0x3030;
  ++g_app_get_background_color_calls;
  return 0;
}
int32_t __cdecl app_get_color(int16_t color_type, uint16_t* color) {
  if (!color || color_type < 0 || (color_type > 127 && (color_type < 1000 || color_type > 1004)))
    return 4;
  const uint16_t value = static_cast<uint16_t>(0x2020 + (color_type & 7) * 0x0808);
  color[0] = color[1] = color[2] = value;
  return 0;
}
int32_t __cdecl app_get_language(char* language) {
  if (!language) return 4;
  std::memcpy(language, "en_US", sizeof("en_US"));
  return 0;
}
int32_t __cdecl app_get_font_style(int16_t, char*, int16_t*, int16_t*, int16_t*) { return 4; }
int32_t __cdecl app_set_cursor(int16_t) { return 4; }
int32_t __cdecl app_is_render_engine(uint8_t* render_engine) {
  if (!render_engine) return 4;
  *render_engine = 1;  // The SDK includes no-UI hosts in render-engine semantics.
  return 0;
}
int32_t __cdecl app_color_picker(const char* title, const float* sample_color,
                                 int32_t, float* new_color) {
  if (!g_render_ui_context_active || !title || !sample_color || !new_color) return 4;
  // PF_PixelFloat is alpha, red, green, blue; the CLI color is RGBA.
  new_color[0] = g_app_picker_color[3];
  new_color[1] = g_app_picker_color[0];
  new_color[2] = g_app_picker_color[1];
  new_color[3] = g_app_picker_color[2];
  ++g_app_color_picker_calls;
  return 0;
}
int32_t __cdecl app_invalidate_rect(void* context, const int32_t* rect) {
  if (context != &g_ui_context_pointer || !g_render_ui_context_active) return 4;
  if (rect) std::copy_n(rect, 4, g_app_invalidated_rect.begin());
  else g_app_invalidated_rect.fill(0);
  ++g_app_invalidate_rect_calls;
  return 0;
}
int32_t __cdecl app_get_mouse(int32_t*) { return 4; }
int32_t __cdecl app_convert_local_to_global(const int32_t*, int32_t*) { return 4; }
int32_t __cdecl app_get_color_at_global_point(const int32_t*, int16_t, int16_t, float*) {
  return 4;
}
// Retained custom-UI callback-ABI state: live App progress-dialog handles
// returned to the plug-in; balanced by the create/dispose callbacks and
// counted in custom_ui_telemetry().
struct AppProgressDialog { uint32_t magic{0x50524744}; };
std::unordered_map<void*, std::unique_ptr<AppProgressDialog>> g_app_progress_dialogs;
int32_t __cdecl app_create_progress_dialog(const uint16_t* title, const uint16_t*, int32_t,
                                            void** dialog) {
  if (!title || !dialog || g_app_progress_dialogs.size() >= 32) return 4;
  auto progress = std::make_unique<AppProgressDialog>();
  void* key = progress.get();
  g_app_progress_dialogs.emplace(key, std::move(progress));
  *dialog = key;
  ++g_app_progress_dialogs_created;
  return 0;
}
int32_t __cdecl app_update_progress_dialog(void* dialog, int32_t count, int32_t total) {
  if (g_app_progress_dialogs.find(dialog) == g_app_progress_dialogs.end() || count < 0 ||
      total < 0 || (total != 0 && count > total)) return 4;
  return 0;
}
int32_t __cdecl app_dispose_progress_dialog(void* dialog) {
  if (g_app_progress_dialogs.erase(dialog) != 1) return 4;
  ++g_app_progress_dialogs_disposed;
  return 0;
}
int32_t __cdecl overlay_foreground(float* color) {
  if (!color) return 4;
  color[0] = color[1] = color[2] = 0.9f;
  color[3] = 1.0f;
  return 0;
}
int32_t __cdecl overlay_stroke_path(void* draw, void* path, int32_t) {
  const auto found = g_drawbot_objects.find(path);
  if (draw != &g_drawbot_draw || found == g_drawbot_objects.end() ||
      found->second->kind != DrawbotObjectKind::Path || found->second->path_points == 0) return 4;
  ++g_overlay_stroke_path_calls;
  return 0;
}
int32_t __cdecl ui_transform_point(void*, void* context, int32_t, uint32_t, int32_t* point) {
  if (context != &g_ui_context_pointer || !point) return 4;
  ++g_ui_coordinate_transform_calls;
  return 0;
}
int32_t __cdecl ui_transform_point_simple(void*, void* context, int32_t* point) {
  if (context != &g_ui_context_pointer || !point) return 4;
  ++g_ui_coordinate_transform_calls;
  return 0;
}

int32_t __cdecl app_get_personal_info(char* info) {
  if (!info) return 4;
  std::memset(info, 0, 64 * 3);
  std::memcpy(info, "AEXCompat", sizeof("AEXCompat"));
  std::memcpy(info + 64, "onmokoworks", sizeof("onmokoworks"));
  std::memcpy(info + 128, "SDK fixture", sizeof("SDK fixture"));
  return 0;
}

// Custom-UI registration and AdvApp info-text callbacks moved from
// worker_main (issue #170); the effect identity stays in l2_main and the
// callbacks keep the C linkage worker_l2_suite_abi.hpp froze.
extern "C" {
int32_t __cdecl register_custom_ui(void*, const void*);
int32_t __cdecl adv_app_info_text(const char*, const char*);
int32_t __cdecl adv_app_info_text3(const char*, const char*, const char*);
}
using CustomUiRegistration =
    aexcompat::worker_runtime::ui_event_execution::CustomUiRegistration;
extern OpaqueHostObject g_effect;
namespace {
auto& g_custom_ui_registration = g_custom_ui_telemetry.registration;
auto& g_invalid_custom_ui_registrations = g_custom_ui_telemetry.invalid_custom_ui_registrations;
auto& g_register_ui_calls = g_custom_ui_telemetry.register_ui_calls;
auto& g_adv_app_info_text_calls = g_custom_ui_telemetry.adv_app_info_text_calls;
auto& g_last_adv_app_info_text = g_custom_ui_telemetry.last_adv_app_info_text;
template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{};
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}
}  // namespace

int32_t __cdecl register_custom_ui(void* effect_ref, const void* custom_ui_info) {
  if (effect_ref != &g_effect || !custom_ui_info) return 4;
  std::array<std::byte, 44> bytes{};
  std::memcpy(bytes.data(), custom_ui_info, bytes.size());
  CustomUiRegistration registration{
      read<uint32_t>(bytes, 4), read<int32_t>(bytes, 8), read<int32_t>(bytes, 12),
      read<int32_t>(bytes, 16), read<int32_t>(bytes, 20), read<int32_t>(bytes, 24),
      read<int32_t>(bytes, 28), read<int32_t>(bytes, 32), read<int32_t>(bytes, 36),
      read<int32_t>(bytes, 40)};
  const auto valid_dimension = [](int32_t value) { return value >= 0 && value <= 8192; };
  if ((registration.events & ~15u) != 0 ||
      !valid_dimension(registration.comp_width) ||
      !valid_dimension(registration.comp_height) ||
      !valid_dimension(registration.layer_width) ||
      !valid_dimension(registration.layer_height) ||
      !valid_dimension(registration.preview_width) ||
      !valid_dimension(registration.preview_height)) {
    ++g_invalid_custom_ui_registrations;
    return 4;
  }
  g_custom_ui_registration = registration;
  ++g_register_ui_calls;
  return 0;
}

int32_t __cdecl adv_app_info_text(const char* first, const char* second) {
  if (!first || !second || strnlen_s(first, 256) == 256 || strnlen_s(second, 256) == 256)
    return 4;
  g_last_adv_app_info_text = std::string(first) + " | " + second;
  ++g_adv_app_info_text_calls;
  return 0;
}

int32_t __cdecl adv_app_info_text3(const char* first, const char* second,
                                   const char* third) {
  if (!first || !second || (third && strnlen_s(third, 256) == 256) ||
      strnlen_s(first, 256) == 256 || strnlen_s(second, 256) == 256) return 4;
  g_last_adv_app_info_text = std::string(first) + " | " + second;
  if (third) g_last_adv_app_info_text += std::string(" | ") + third;
  ++g_adv_app_info_text_calls;
  return 0;
}

bool drawbot_objects_empty() { return g_drawbot_objects.empty(); }

}  // namespace aexcompat::l2_detail
