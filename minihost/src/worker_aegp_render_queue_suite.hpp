#pragma once

#include <cstddef>
#include <cstdint>

namespace aexcompat::l2_detail {

// AE_GeneralPlug.h: acquisition v1, three slots; A_u_long state is 32-bit.
struct RenderQueueSuite1 {
  int32_t (__cdecl* add_comp)(void* comp, const char* path);
  int32_t (__cdecl* set_state)(uint32_t state);
  int32_t (__cdecl* get_state)(uint32_t* state);
};
static_assert(sizeof(RenderQueueSuite1) == 3 * sizeof(void*));
static_assert(offsetof(RenderQueueSuite1, get_state) == 2 * sizeof(void*));
inline constexpr uint32_t render_queue_stopped = 0;

extern RenderQueueSuite1 g_render_queue_suite1;
int selftest_render_queue_suite();

}  // namespace aexcompat::l2_detail
