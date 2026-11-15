#include "worker_world_registry.hpp"
#include "trace_writer.hpp"
#include "gpu_memory_world_transport.hpp"
#include <array>
#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace aexcompat::l2_detail { aexcompat::TraceWriter* g_trace_writer{}; }
namespace aexcompat::gpu_runtime::memory_world_transport {
HostNewWorld create{};
HostDisposeWorld dispose{};
void configure_host_world_fallback(HostNewWorld n, HostDisposeWorld d, HostOwnsWorld,
                                   HostRecognizesSmartWorld) { create = n; dispose = d; }
}
namespace wr = aexcompat::world_registry;
using World = std::array<std::byte, aexcompat::world_safety::kEffectWorldSize>;
int failures{};
int checks{};
void check(bool value, const char* label) {
  ++checks;
  if (!value) { ++failures; std::fprintf(stderr, "FAIL: %s\n", label); }
}
int main() {
  World first{}, retained{}, leaked{}, gpu{}, host{};
  check(wr::new_world(nullptr, 2, 3, 1, wr::kPixelFormatArgb32, first.data()) == 0, "allocate first");
  check(wr::legacy_new_world(nullptr, 2, 3, 1, retained.data()) == 0, "allocate retained legacy");
  check(wr::new_world(nullptr, 2, 3, 1, wr::kPixelFormatArgb64, leaked.data()) == 0, "allocate leaked");
  check(wr::dispose_world(nullptr, first.data()) == 0, "explicit disposal");
  // Simulate successive frames with the same global/sequence world. Observation
  // must not free it; a plug-in may write it until its final setdown callback.
  for (int frame = 0; frame < 3; ++frame) {
    wr::OwnedWorldSnapshot snapshot{};
    check(wr::snapshot_owned_world(retained.data(), snapshot), "retained across frames");
    if (snapshot.world.data) std::memset(snapshot.world.data, frame, 24);
    check(wr::statistics().live_count == 2 && !wr::lifetimes_balanced(), "live before teardown");
  }
  wr::reclaim_plugin_worlds_after_setdown(true);
  check(wr::statistics().live_count == 2, "no reclamation before GLOBAL_SETDOWN");
  check(wr::dispose_world(nullptr, retained.data()) == 0, "sequence/global setdown can dispose");
  wr::note_global_setdown_result(true);
  wr::reclaim_plugin_worlds_after_setdown(false);
  wr::OwnedWorldSnapshot provider_world{};
  check(wr::snapshot_owned_world(leaked.data(), provider_world), "provider callback after GLOBAL_SETDOWN still resolves world");
  if (provider_world.world.data) std::memset(provider_world.world.data, 42, 48);
  wr::reclaim_plugin_worlds_after_setdown(true);
  auto s = wr::statistics();
  check(s.created == 3 && s.disposed == 2 && s.reclaimed == 1 && s.reclaimed_bytes == 48,
        "separate plugin disposals and host reclamation");
  check(s.live_count == 0 && s.live_bytes == 0 && wr::lifetimes_balanced(), "balanced after reclaim");
  wr::reclaim_plugin_worlds_after_setdown(true);
  check(wr::statistics().reclaimed == 1, "idempotent teardown");
  const auto faults = s.invalid_operations;
  check(wr::dispose_world(nullptr, first.data()) != 0, "double dispose rejected");
  check(wr::dispose_world(nullptr, leaked.data()) != 0, "reclaimed stale dispose rejected");
  unsigned char host_pixels[16] = {42};
  void* host_data = host_pixels;
  std::memcpy(host.data() + 24, &host_data, sizeof(host_data));
  check(wr::dispose_world(nullptr, host.data()) != 0, "host/foreign world rejected");
  check(wr::statistics().invalid_operations == faults + 3, "fault evidence retained");
  wr::configure_gpu_fallback_bridge(nullptr);
  namespace transport = aexcompat::gpu_runtime::memory_world_transport;
  check(transport::create(nullptr, 1, 1, 1, wr::kPixelFormatGpuBgra128, gpu.data()) == 0, "gpu fallback allocation");
  wr::reclaim_plugin_worlds_after_setdown(true);
  check(wr::owns_world(gpu.data()) && wr::statistics().reclaimed == 1 && host_pixels[0] == 42,
        "host and GPU transport excluded");
  check(!wr::lifetimes_balanced(), "GPU lifetime still enforced");
  check(wr::dispose_world(nullptr, gpu.data()) != 0, "PF dispose cannot free GPU transport world");
  check(transport::dispose(nullptr, gpu.data()) == 0, "GPU explicit disposal");
  std::array<World, 65> worlds{};
  for (int i = 0; i < 64; ++i)
    check(wr::new_world(nullptr, 1, 1, 1, wr::kPixelFormatArgb32, worlds[i].data()) == 0, "allocate to count ceiling");
  check(wr::new_world(nullptr, 1, 1, 1, wr::kPixelFormatArgb32, worlds[64].data()) == 4, "live leaks enforce count ceiling");
  check(wr::statistics().live_count == 64, "ceiling refusal does not allocate");
  wr::reclaim_plugin_worlds_after_setdown(true);
  check(wr::statistics().reclaimed == 65 && wr::lifetimes_balanced(), "all count-ceiling leaks reclaimed");
  check(wr::new_world(nullptr, 4096, 8192, 1, wr::kPixelFormatArgb32, first.data()) == 0, "allocate 128 MiB");
  check(wr::new_world(nullptr, 4096, 8193, 1, wr::kPixelFormatArgb32, leaked.data()) == 4, "live leaks enforce byte ceiling");
  check(wr::statistics().live_bytes == 128ULL * 1024 * 1024, "byte refusal preserves live accounting");
  wr::reclaim_plugin_worlds_after_setdown(true);
  s = wr::statistics();
  check(s.reclaimed == 66 && s.reclaimed_bytes == 128ULL * 1024 * 1024 + 304 && wr::lifetimes_balanced(), "byte-ceiling reclamation accounting");
  check(s.invalid_operations == faults + 6, "reclamation never clears faults");
  check(wr::new_world(nullptr, 1, 1, 1, wr::kPixelFormatArgb32, first.data()) == 0, "failed teardown setup");
  wr::note_global_setdown_result(false);
  wr::note_global_setdown_result(true);
  wr::reclaim_plugin_worlds_after_setdown(true);
  check(wr::owns_world(first.data()), "failed setdown stays sticky across later successful setdowns");
  check(wr::dispose_world(nullptr, first.data()) == 0, "failed teardown test cleanup");
  std::printf("{\"world_reclamation\":\"%s\",\"checks\":%d,\"reclaimed\":%llu,\"reclaimed_bytes\":%llu}\n",
      failures ? "failed" : "passed", checks, static_cast<unsigned long long>(s.reclaimed),
      static_cast<unsigned long long>(s.reclaimed_bytes));
  return failures ? 1 : 0;
}
