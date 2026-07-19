#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace aexcompat::mask_runtime {

enum class Fault { None, CountError, CountCrash };

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

}  // namespace aexcompat::mask_runtime
