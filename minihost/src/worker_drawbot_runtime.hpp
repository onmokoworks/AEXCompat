#pragma once

#include <array>
#include <cstdint>

namespace aexcompat::l2_detail {

// Custom-UI event context block whose address is written into the PF event
// ABI; owned beside the Drawbot/App callbacks that hand it out (issue #170).
// Lifetime: process-lifetime, re-armed per UI dispatch.
struct HostUiContext {
  uint32_t magic{0x05ea771e};
  int32_t window_type{2};
  void* reserved_filter{};
  std::array<intptr_t, 4> plugin_state{};
  void* draw_ref{};
  void* pane{};
  void* job_manager{};
};

extern HostUiContext g_ui_context;
extern HostUiContext* g_ui_context_pointer;

void* enter_custom_ui_context(int32_t context);
void leave_custom_ui_context(void* scope);
bool custom_ui_context_stable();
void set_custom_ui_context_tool(int32_t context);
// True when the live Drawbot object table is empty; the object storage stays
// private to the owner TU.
bool drawbot_objects_empty();

int32_t __cdecl drawbot_get_supplier(void* draw, void** supplier);
int32_t __cdecl drawbot_get_surface(void* draw, void** surface);
int32_t __cdecl drawbot_new_pen(void* supplier, const float* color, float size, void** pen);
int32_t __cdecl drawbot_new_brush(void* supplier, const float* color, void** brush);
int32_t __cdecl drawbot_new_path(void* supplier, void** path);
int32_t __cdecl drawbot_release_object(void* object);
int32_t __cdecl drawbot_add_rect(void* path, const float* rect);
int32_t __cdecl drawbot_path_point(void* path, float x, float y);
int32_t __cdecl drawbot_paint_rect(void* surface, const float* color, const float* rect);
int32_t __cdecl drawbot_fill_path(void* surface, void* brush, void* path, int32_t fill_type);
int32_t __cdecl drawbot_stroke_path(void* surface, void* pen, void* path);
int32_t __cdecl get_drawing_reference(void* context, void** drawing);
int32_t __cdecl app_get_background_color(uint16_t* color);
int32_t __cdecl app_get_color(int16_t color_type, uint16_t* color);
int32_t __cdecl app_get_language(char* language);
int32_t __cdecl app_get_font_style(int16_t, char*, int16_t*, int16_t*, int16_t*);
int32_t __cdecl app_set_cursor(int16_t);
int32_t __cdecl app_is_render_engine(uint8_t* render_engine);
int32_t __cdecl app_color_picker(const char* title, const float* sample_color,
                                 int32_t, float* new_color);
int32_t __cdecl app_invalidate_rect(void* context, const int32_t* rect);
int32_t __cdecl app_get_mouse(int32_t*);
int32_t __cdecl app_convert_local_to_global(const int32_t*, int32_t*);
int32_t __cdecl app_get_color_at_global_point(const int32_t*, int16_t, int16_t, float*);
int32_t __cdecl app_create_progress_dialog(const uint16_t* title, const uint16_t*, int32_t,
                                           void** dialog);
int32_t __cdecl app_update_progress_dialog(void* dialog, int32_t count, int32_t total);
int32_t __cdecl app_dispose_progress_dialog(void* dialog);
int32_t __cdecl app_get_personal_info(char* info);
int32_t __cdecl overlay_foreground(float* color);
int32_t __cdecl overlay_stroke_path(void* draw, void* path, int32_t);
int32_t __cdecl ui_transform_point(void*, void* context, int32_t, uint32_t, int32_t* point);
int32_t __cdecl ui_transform_point_simple(void*, void* context, int32_t* point);

}  // namespace aexcompat::l2_detail
