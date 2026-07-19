#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace aexcompat::mask_runtime {

enum class Fault { None, CountError, CountCrash };

struct Snapshot {
  Fault fault{Fault::None};
  uint32_t active_masks{};
  uint32_t masks_acquired{};
  uint32_t masks_disposed{};
  uint32_t streams_acquired{};
  uint32_t streams_disposed{};
  uint32_t values_acquired{};
  uint32_t values_disposed{};
  uint32_t mask_mutations{};
  uint32_t invalid_mask_operations{};
  uint32_t outline_mutations{};
  uint32_t invalid_outline_operations{};
  uint32_t keyframe_mutations{};
  uint32_t invalid_keyframe_operations{};
  uint32_t stream_metadata_queries{};
  uint32_t stream_duplicates{};
  uint32_t invalid_stream_operations{};
  uint32_t dynamic_stream_mutations{};
  uint32_t invalid_dynamic_stream_operations{};
};

struct CurveVertex {
  double x{}, y{};
  double tangent_in_x{}, tangent_in_y{};
  double tangent_out_x{}, tangent_out_y{};
};

struct CurveSnapshot {
  int32_t id{};
  bool open{};
  std::vector<CurveVertex> vertices;
};

// Worker-owned host identities used by the raw AEGP callback ABI.  Keeping
// these as opaque pointers prevents the mask runtime from depending on l2's
// concrete host-object layout.
struct HostContext {
  void* layer{};
  void (*raise_access_violation)(){};
  Snapshot (*snapshot)(){};
  bool (*snapshot_curve)(void* handle, CurveSnapshot& curve){};
  bool (*lifetimes_balanced)(){};
  bool (*install_synthetic_scene)(const std::vector<CurveSnapshot>& curves){};
};

struct Vertex {
  double x{};
  double y{};
  double tangent_in_x{};
  double tangent_in_y{};
  double tangent_out_x{};
  double tangent_out_y{};
};

struct MaskSeed {
  bool open{};
  std::vector<Vertex> vertices;
  int32_t dynamic_order{};
};

// Parser-facing DTO. It deliberately contains no worker handles: l2 creates
// those only after the complete request has been validated.
struct SceneSeed {
  std::string id;
  std::vector<MaskSeed> masks;
};

bool build_scene_seed(const std::string& scene_id, SceneSeed& seed);
void set_fault(Fault fault);
Fault fault();

// Mask scene identity, owned here (issue #126 Phase D): whether the host mask
// model is enabled for the current invocation and which scene seed is
// installed. Writers are worker_main (invocation apply hook, rendering-worker
// default, configure_mask_scene, the request_v4 mask-context path), the final
// dispatch owner, and the utility-suite selftest save/restore; readers are the
// mask suite provider gate and the smart completion report. Lifetime:
// process-lifetime, defaults enabled=false / id "none", never torn down.
bool model_enabled();
void set_model_enabled(bool enabled);
std::string mask_scene_id();
void set_mask_scene_id(const std::string& id);
void configure_host_context(HostContext context);
HostContext host_context();
Snapshot snapshot();
bool snapshot_curve(void* handle, CurveSnapshot& curve);

}  // namespace aexcompat::mask_runtime
