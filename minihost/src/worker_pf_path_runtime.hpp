#pragma once

#include "worker_mask_runtime.hpp"

#include <array>
#include <cstdint>
#include <vector>

namespace aexcompat::pf_path_runtime {

struct PathInfo {
  void* handle{};
  int32_t id{};
  int32_t dynamic_order{};
  bool open{};
  bool inverted{};
  int32_t mode{};
};

struct WorldView {
  unsigned char* pixels{};
  int32_t rowbytes{};
  int32_t width{};
  int32_t height{};
  int32_t pixel_bytes{};
};

struct HostHooks {
  void* effect_ref{};
  std::vector<PathInfo> (*enumerate)(){};
  bool (*snapshot)(void*, mask_runtime::CurveSnapshot&){};
  bool (*bounded_world)(void*, WorldView&){};
};

struct Snapshot {
  uint32_t checkout_calls{};
  uint32_t checkin_calls{};
  uint32_t mask_calls{};
  uint32_t invalid_operations{};
  uint32_t preps_created{};
  uint32_t preps_disposed{};
  uint32_t live_preps{};
  int32_t reject_reason{};
  double last_feather_x{};
  double last_feather_y{};
  double last_opacity{};
  int32_t last_quality{};
  std::array<int32_t, 4> last_bounds{};
};

struct LegacyRect { int32_t left, top, right, bottom; };
struct PathVertex { double x, y, tan_in_x, tan_in_y, tan_out_x, tan_out_y; };
static_assert(sizeof(LegacyRect) == 16);
static_assert(sizeof(PathVertex) == 48);

void configure(HostHooks hooks);
HostHooks host_hooks();
void reset();
Snapshot snapshot();
bool lifetimes_balanced();

int32_t __cdecl num_paths(void*, int32_t*);
int32_t __cdecl path_info(void*, int32_t, int32_t*);
int32_t __cdecl checkout_path(void*, int32_t, int32_t, int32_t, uint32_t, void**);
int32_t __cdecl checkin_path(void*, int32_t, int32_t, void*);
int32_t __cdecl path_is_open(void*, void*, int8_t*);
int32_t __cdecl path_num_segments(void*, void*, int32_t*);
int32_t __cdecl path_vertex_info(void*, void*, int32_t, PathVertex*);
int32_t __cdecl path_prepare_seg_length(void*, void*, int32_t, int32_t, void**);
int32_t __cdecl path_get_seg_length(void*, void*, int32_t, void**, double*);
int32_t __cdecl path_eval_seg_length(void*, void*, void**, int32_t, double, double*, double*);
int32_t __cdecl path_eval_seg_length_deriv1(void*, void*, void**, int32_t, double,
                                             double*, double*, double*, double*);
int32_t __cdecl path_cleanup_seg_length(void*, void*, int32_t, void**);
int32_t __cdecl path_is_inverted(void*, int32_t, int8_t*);
int32_t __cdecl path_get_mask_mode(void*, int32_t, int32_t*);
int32_t __cdecl path_get_name(void*, int32_t, char*);
int32_t __cdecl mask_world_with_path(void*, void**, double, double, int32_t, double,
                                      int32_t, void*, LegacyRect*);

}  // namespace aexcompat::pf_path_runtime
