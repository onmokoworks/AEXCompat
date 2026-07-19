#include "worker_mask_runtime.hpp"

#include <atomic>
#include <utility>

namespace aexcompat::mask_runtime {
namespace {

std::atomic<Fault> g_fault{Fault::None};
std::atomic<void*> g_layer{nullptr};
std::atomic<void (*)()> g_raise_access_violation{nullptr};
std::atomic<Snapshot (*)()> g_snapshot{nullptr};
std::atomic<bool (*)(void*, CurveSnapshot&)> g_snapshot_curve{nullptr};
std::atomic<bool (*)()> g_lifetimes_balanced{nullptr};

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

void configure_host_context(HostContext context) {
  g_layer.store(context.layer, std::memory_order_release);
  g_raise_access_violation.store(context.raise_access_violation, std::memory_order_release);
  g_snapshot.store(context.snapshot, std::memory_order_release);
  g_snapshot_curve.store(context.snapshot_curve, std::memory_order_release);
  g_lifetimes_balanced.store(context.lifetimes_balanced, std::memory_order_release);
}

HostContext host_context() {
  return {g_layer.load(std::memory_order_acquire),
          g_raise_access_violation.load(std::memory_order_acquire),
          g_snapshot.load(std::memory_order_acquire),
          g_snapshot_curve.load(std::memory_order_acquire),
          g_lifetimes_balanced.load(std::memory_order_acquire)};
}

bool snapshot_curve(void* handle, CurveSnapshot& curve) {
  const auto provider = g_snapshot_curve.load(std::memory_order_acquire);
  if (!provider) return false;
  CurveSnapshot candidate;
  if (!provider(handle, candidate)) return false;
  curve = std::move(candidate);
  return true;
}

Snapshot snapshot() {
  const auto provider = g_snapshot.load(std::memory_order_acquire);
  Snapshot value = provider ? provider() : Snapshot{};
  value.fault = fault();
  return value;
}

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
