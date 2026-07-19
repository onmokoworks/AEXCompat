#include "worker_mask_runtime.hpp"

#include <atomic>
#include <utility>

namespace aexcompat::mask_runtime {
namespace {

std::atomic<Fault> g_fault{Fault::None};

MaskSeed rectangle(double left, double top, double right, double bottom) {
  MaskSeed mask;
  mask.vertices = {{left, top, 0, 0, 0, 0},
                   {right, top, 0, 0, 0, 0},
                   {right, bottom, 0, 0, 0, 0},
                   {left, bottom, 0, 0, 0, 0},
                   {left, top, 0, 0, 0, 0}};
  return mask;
}

}  // namespace

void set_fault(Fault fault) { g_fault.store(fault, std::memory_order_release); }

Fault fault() { return g_fault.load(std::memory_order_acquire); }

bool build_scene_seed(const std::string& scene_id, SceneSeed& seed) {
  SceneSeed candidate;
  candidate.id = scene_id;
  if (scene_id == "rectangle") {
    candidate.masks.push_back(rectangle(4, 3, 12, 9));
  } else if (scene_id == "translated_rectangle") {
    candidate.masks.push_back(rectangle(2, 2, 10, 8));
  } else if (scene_id == "two_rectangles") {
    candidate.masks.push_back(rectangle(1, 1, 7, 6));
    candidate.masks.push_back(rectangle(9, 5, 15, 11));
  } else if (scene_id != "empty") {
    return false;
  }
  for (std::size_t index = 0; index < candidate.masks.size(); ++index)
    candidate.masks[index].dynamic_order = static_cast<int32_t>(index);
  seed = std::move(candidate);
  return true;
}

}  // namespace aexcompat::mask_runtime
