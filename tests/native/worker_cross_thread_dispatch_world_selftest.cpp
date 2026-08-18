// A plug-in may run its own render body on a thread it owns and ask the host
// about the world the host handed it from there. PSL_Adjustments does exactly
// that: its SMART_RENDER hands the work to `U_SuspendContext::CallOnThreadedExecutor`
// and calls `PF_GetPixelFormat` from a "COR PSL Thread" whose stack carries no
// host frame at all, about the host's own 256x144 output world - and got
// PF_Err_OUT_OF_MEMORY back, because the dispatch-world registry the host
// resolves through was `thread_local` and empty on that thread (issue #1299).
//
// What the registry may not do in answering is relax: a foreign match is held
// to the same identity and geometry equality, and the same refusal of an
// ambiguous one, as a same-thread match. This self-test drives the registry
// directly with fake worlds and real threads, so it needs no plug-in, no AEX
// and no worker process.

#include "worker_pf_world_facade.hpp"
#include "worker_world_safety.hpp"

#include <array>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <iostream>
#include <mutex>
#include <thread>

using aexcompat::world_safety::DispatchWorldFormat;
using aexcompat::world_safety::DispatchWorldFormatScope;
using aexcompat::world_safety::OwnedWorldResolution;
using aexcompat::world_safety::kEffectWorldSize;

namespace {

int failures = 0;

void check(bool condition, const char* what) {
  if (condition) return;
  std::fprintf(stderr, "FAIL: %s\n", what);
  ++failures;
}

// worker_world_registry.hpp's pixel-format tags. Repeated rather than included
// so this target links against world_safety and the facade alone; any two
// distinct tags would do, and the registry only ever compares them.
constexpr int32_t kArgb32 = 1650946657;
constexpr int32_t kArgb64 = 909206881;

// A PF_EffectWorld as far as the registry is concerned: it reads the pixel
// pointer, the stride and the extent out of the struct and never dereferences
// the pixels.
struct FakeWorld {
  alignas(16) std::array<std::byte, kEffectWorldSize> bytes{};

  void describe(void* data, int32_t rowbytes, int32_t width, int32_t height) {
    std::memcpy(bytes.data() + 24, &data, sizeof(data));
    std::memcpy(bytes.data() + 32, &rowbytes, sizeof(rowbytes));
    std::memcpy(bytes.data() + 36, &width, sizeof(width));
    std::memcpy(bytes.data() + 40, &height, sizeof(height));
  }
  void* address() { return bytes.data(); }
};

// This harness has no PF_NewWorld registry, so every world reaches the
// dispatch-scope passes - which is what is under test.
OwnedWorldResolution no_owned_world(const void*, void*, int32_t, int32_t, int32_t,
                                    DispatchWorldFormat&) {
  return OwnedWorldResolution::not_owned;
}

bool resolve(const void* world, DispatchWorldFormat& result) {
  return aexcompat::world_safety::resolve_dispatch_world_format(world, &no_owned_world,
                                                                result);
}

unsigned char g_pixels_a[64]{};
unsigned char g_pixels_b[64]{};

// Holds a dispatch scope open on another thread until `release` is set, so the
// registrations under test are provably live while the reader runs.
struct HeldScope {
  std::mutex mutex;
  std::condition_variable changed;
  int ready{};
  bool release{};

  void wait_for(int registrars) {
    std::unique_lock<std::mutex> lock(mutex);
    changed.wait(lock, [&] { return ready >= registrars; });
  }
  void announce() {
    std::unique_lock<std::mutex> lock(mutex);
    ++ready;
    changed.notify_all();
    changed.wait(lock, [&] { return release; });
  }
  void let_go() {
    {
      std::lock_guard<std::mutex> lock(mutex);
      release = true;
    }
    changed.notify_all();
  }
};

}  // namespace

int main() {
  FakeWorld registered;
  registered.describe(g_pixels_a, 1024, 256, 144);
  // What PSL_Adjustments' worker thread actually holds: a struct of its own
  // carrying the same pixels, stride and extent, at a different address.
  FakeWorld plugin_copy;
  plugin_copy.describe(g_pixels_a, 1024, 256, 144);

  {
    DispatchWorldFormatScope dispatch;
    check(dispatch.register_world(registered.address(), kArgb32),
          "the dispatch thread registers the world it hands out");

    DispatchWorldFormat own{};
    check(resolve(registered.address(), own) && own.pixel_format == kArgb32,
          "the registering thread still resolves its own world by reference");
    DispatchWorldFormat own_copy{};
    check(resolve(plugin_copy.address(), own_copy) &&
              own_copy.world == registered.address(),
          "the registering thread still resolves a copy of that struct by layout");

    bool by_reference = false;
    bool by_copy = false;
    bool mismatch_refused = false;
    bool base_reference_known = false;
    bool reference_known = false;
    bool registered_by_reference = false;
    bool registered_by_copy = true;
    std::thread plugin_owned_thread([&] {
      DispatchWorldFormat resolved{};
      by_reference = resolve(registered.address(), resolved) &&
                     resolved.pixel_format == kArgb32 &&
                     resolved.data == g_pixels_a && resolved.rowbytes == 1024 &&
                     resolved.width == 256 && resolved.height == 144;
      DispatchWorldFormat copied{};
      by_copy = resolve(plugin_copy.address(), copied) &&
                copied.pixel_format == kArgb32 &&
                copied.world == registered.address();
      // A struct nobody registered, whose pixels are a registered world's but
      // whose extent is not: neither pass may answer it. (The *refusal* path -
      // the registered struct itself no longer describing its registration -
      // is the retargeting case at the end of this block.)
      FakeWorld tampered;
      tampered.describe(g_pixels_a, 1024, 255, 144);
      DispatchWorldFormat refused{};
      mismatch_refused = !resolve(tampered.address(), refused);
      reference_known =
          aexcompat::world_safety::dispatch_world_reference_known(registered.address());
      // The other half of that gate: a struct nobody registered whose pixel
      // base is a registered world's. The copy callbacks' foreign-operand
      // fallback consults this so the registry's mismatch refusal stays a
      // refusal instead of degrading into foreign admission, and it has to
      // answer that from a plug-in-owned thread too.
      FakeWorld same_base;
      same_base.describe(g_pixels_a, 512, 128, 72);
      base_reference_known =
          aexcompat::world_safety::dispatch_world_reference_known(same_base.address());
      DispatchWorldFormat exact{};
      registered_by_reference =
          aexcompat::world_safety::resolve_registered_dispatch_world(
              registered.address(), exact);
      DispatchWorldFormat exact_copy{};
      registered_by_copy = aexcompat::world_safety::resolve_registered_dispatch_world(
          plugin_copy.address(), exact_copy);
    });
    plugin_owned_thread.join();

    check(by_reference, "a plug-in-owned thread resolves the host's world by reference");
    check(by_copy, "a plug-in-owned thread resolves a copy of the struct by layout");
    check(mismatch_refused,
          "a foreign thread gets no answer for an extent nothing registered");
    check(reference_known,
          "the registry tells a plug-in-owned thread it knows the reference");
    check(base_reference_known,
          "a plug-in-owned thread is told a registered pixel base is known");
    check(registered_by_reference,
          "resolve_registered_dispatch_world answers a plug-in-owned thread");
    check(!registered_by_copy,
          "resolve_registered_dispatch_world still takes only the exact reference");

    // The registered struct itself, retargeted after registration at a second
    // registered world's pixels and geometry. This is the exact-reference
    // *refusal*, and it has to stay one: the layout pass would happily answer
    // with `sibling`, so a resolver that treated the mismatch as a miss would
    // hand a plug-in-owned thread the wrong world's pixel format. Done last,
    // and undone, so the earlier assertions saw an intact registration.
    FakeWorld sibling;
    sibling.describe(g_pixels_b, 1024, 64, 64);
    check(dispatch.register_world(sibling.address(), kArgb64),
          "the dispatch thread registers a second, differently shaped world");
    const std::array<std::byte, kEffectWorldSize> intact = registered.bytes;
    registered.describe(g_pixels_b, 1024, 64, 64);
    bool mutation_refused = false;
    bool mutation_refused_registered = false;
    std::thread mutation_reader([&] {
      DispatchWorldFormat resolved{};
      mutation_refused = !resolve(registered.address(), resolved);
      mutation_refused_registered =
          !aexcompat::world_safety::resolve_registered_dispatch_world(
              registered.address(), resolved);
    });
    mutation_reader.join();
    registered.bytes = intact;
    check(mutation_refused,
          "a retargeted registration is refused across threads, not answered "
          "from another registration's layout");
    check(mutation_refused_registered,
          "resolve_registered_dispatch_world refuses the retargeted reference too");
  }

  // The publication lasts exactly as long as the dispatch does. Past it the
  // world is unreachable from every thread, not merely from the one that left.
  bool reachable_after_the_dispatch = true;
  std::thread late_caller([&] {
    DispatchWorldFormat resolved{};
    reachable_after_the_dispatch = resolve(registered.address(), resolved) ||
                                   resolve(plugin_copy.address(), resolved);
  });
  late_caller.join();
  check(!reachable_after_the_dispatch,
        "a dispatch that ended is unreachable from any thread");

  {
    // Two dispatch threads, each holding a live scope, describing the same
    // pixels through different structs with different formats. A layout lookup
    // that two registrations answer differently is ambiguous, and ambiguity is
    // refused rather than resolved by whichever thread happened to publish
    // first - the same rule the single-thread passes have always applied.
    HeldScope held;
    FakeWorld first;
    FakeWorld second;
    first.describe(g_pixels_b, 1024, 64, 64);
    second.describe(g_pixels_b, 1024, 64, 64);
    // Both registrations have to have succeeded, or the refusal below would
    // pass because there was nothing to be ambiguous about.
    bool both_registered[2] = {false, false};
    const auto registrar = [&held](FakeWorld& world, int32_t pixel_format,
                                   bool& registered) {
      DispatchWorldFormatScope dispatch;
      registered = dispatch.register_world(world.address(), pixel_format);
      held.announce();
    };
    std::thread one(registrar, std::ref(first), kArgb32,
                    std::ref(both_registered[0]));
    std::thread two(registrar, std::ref(second), kArgb64,
                    std::ref(both_registered[1]));
    held.wait_for(2);

    check(both_registered[0] && both_registered[1],
          "both dispatch threads registered their world");
    FakeWorld copy;
    copy.describe(g_pixels_b, 1024, 64, 64);
    DispatchWorldFormat resolved{};
    check(!resolve(copy.address(), resolved),
          "an ambiguous cross-thread layout match is refused");
    // The same ambiguity by exact reference: one struct, two threads, two
    // formats. Innermost-wins is a rule inside a thread and no rule at all
    // across threads, so this has to be refused rather than raced for.
    FakeWorld shared;
    shared.describe(g_pixels_b, 1024, 48, 48);
    bool shared_registered[2] = {false, false};
    HeldScope shared_held;
    const auto shared_registrar = [&shared_held, &shared](int32_t pixel_format,
                                                          bool& registered) {
      DispatchWorldFormatScope dispatch;
      registered = dispatch.register_world(shared.address(), pixel_format);
      shared_held.announce();
    };
    std::thread three(shared_registrar, kArgb32, std::ref(shared_registered[0]));
    std::thread four(shared_registrar, kArgb64, std::ref(shared_registered[1]));
    shared_held.wait_for(2);
    check(shared_registered[0] && shared_registered[1],
          "both dispatch threads registered the shared reference");
    DispatchWorldFormat contested{};
    check(!resolve(shared.address(), contested),
          "an ambiguous cross-thread reference match is refused");
    check(!aexcompat::world_safety::resolve_registered_dispatch_world(
              shared.address(), contested),
          "resolve_registered_dispatch_world refuses the same ambiguity");
    shared_held.let_go();
    three.join();
    four.join();

    held.let_go();
    one.join();
    two.join();
  }

  {
    // Crossing threads is a fallback, not a merge: a caller with a registration
    // of its own is answered by that one, and never has to compete with another
    // thread's for the same layout.
    HeldScope held;
    FakeWorld theirs;
    theirs.describe(g_pixels_b, 1024, 32, 32);
    bool theirs_registered = false;
    std::thread other([&] {
      DispatchWorldFormatScope dispatch;
      theirs_registered = dispatch.register_world(theirs.address(), kArgb64);
      held.announce();
    });
    held.wait_for(1);
    check(theirs_registered, "the other dispatch thread registered its world");

    FakeWorld mine;
    mine.describe(g_pixels_b, 1024, 32, 32);
    DispatchWorldFormatScope dispatch;
    check(dispatch.register_world(mine.address(), kArgb32),
          "this thread registers a world with the same layout");
    FakeWorld copy;
    copy.describe(g_pixels_b, 1024, 32, 32);
    DispatchWorldFormat resolved{};
    check(resolve(copy.address(), resolved) && resolved.world == mine.address() &&
              resolved.pixel_format == kArgb32,
          "the calling thread's own registration answers before any foreign one");

    held.let_go();
    other.join();
  }

  {
    // The facade a scope attached belongs to the world for as long as *any*
    // live registration still hands that world out - which, since a plug-in-
    // owned thread resolves the same registry, includes a registration on
    // another thread. A scope that took the facade back while another thread's
    // registration was still live would leave that thread's caller holding a
    // world whose reserved_long4 the host had just cleared, which is a null
    // vtable where the design promises a refusal.
    namespace facade = aexcompat::worker_runtime::pf_world_facade;
    HeldScope held;
    FakeWorld shared;
    shared.describe(g_pixels_a, 64, 16, 16);
    bool other_registered = false;
    std::thread other([&] {
      DispatchWorldFormatScope dispatch;
      other_registered = dispatch.register_world(shared.address(), kArgb32);
      held.announce();
    });
    held.wait_for(1);
    check(other_registered, "the other dispatch thread registered the shared world");
    check(facade::attached(shared.address()) != nullptr,
          "registering the world attached a facade to it");
    {
      DispatchWorldFormatScope dispatch;
      check(dispatch.register_world(shared.address(), kArgb32),
            "this thread registers the same world");
    }
    check(facade::attached(shared.address()) != nullptr,
          "the facade survives a scope ending while another thread's "
          "registration still names the world");

    held.let_go();
    other.join();
    check(facade::attached(shared.address()) == nullptr,
          "the facade goes when the last live registration does");
  }

  if (failures != 0) return 1;
  std::cout << "{\"cross_thread_dispatch_world\":\"passed\"}\n";
  return 0;
}
