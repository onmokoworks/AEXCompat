#include "worker_world_safety.hpp"

#include "worker_pf_world_facade.hpp"
#include "worker_world_registry.hpp"

#include <windows.h>

#include <algorithm>
#include <atomic>
#include <cstring>
#include <vector>

namespace aexcompat::world_safety {
namespace {

using ScopeStack = std::vector<std::vector<DispatchWorldFormat>>;

thread_local ScopeStack g_dispatch_world_formats;
// Worlds this scope gave a pool facade to, so the scope can take them back.
thread_local std::vector<std::vector<void*>> g_attached_facades;
std::atomic<uint64_t> g_dispatch_world_generation{};

// A plug-in may call a host callback from a thread it owns rather than from
// the one the host dispatched on: PSL_Adjustments hands its SmartFX body to
// `U_SuspendContext::CallOnThreadedExecutor` and asks `PF_GetPixelFormat` about
// the host's own output world from a "COR PSL Thread" whose stack carries no
// host frame at all (issue #1299). The scope stack stays per-thread - scopes
// nest per dispatch, and the facades a scope attached are that scope's to take
// back - so what crosses threads is *reading* it, never owning it.
//
// Every live stack publishes itself here for the lifetime of its outermost
// scope, and a foreign reader walks the published stacks under the lock below.
// Writers (scope push/pop, registration) take the same lock, so the only
// unsynchronized access left is a thread reading its own stack, which no other
// thread ever writes.
//
// This is a lookup reaching further, not a check relaxing: a foreign match is
// held to the same identity/geometry equality and the same ambiguity refusal as
// a same-thread one, and the same-thread passes still run first and unchanged.
//
// What the publication bounds is reachability, not the pixels' lifetime. A
// world stops being resolvable when the dispatch that registered it ends, but
// a foreign thread that resolved one a moment earlier holds a pointer whose
// validity only that dispatch guarantees. Nothing here can close that: the
// plug-in that started the thread is what decides whether it outlives the
// render body it was given, and AE is in the same position.
//
// SRW lock rather than `std::mutex`, for the reason `worker_pf_world_facade`
// gives for its own: `pf_world_facade::WorldResolver` is declared `noexcept`
// and the resolver installed for it (worker_entry_wiring) reaches this lock, so
// a lock that may throw would be a `terminate` path one call down. Acquiring an
// SRWLOCK cannot fail.
//
// It closes one of two, not both: the same `noexcept` resolver reaches
// `world_registry::resolve_owned_world` first, and that still opens with a
// `std::lock_guard<std::mutex>`. Both predate this change and MSVC's
// `std::mutex::lock()` throwing is close to unreachable, so this is a contract
// gap rather than an expected fault - recorded in #1304 rather than fixed by
// widening this change.
SRWLOCK g_registry_lock = SRWLOCK_INIT;
struct RegistryLock {
  RegistryLock() noexcept { AcquireSRWLockExclusive(&g_registry_lock); }
  ~RegistryLock() noexcept { ReleaseSRWLockExclusive(&g_registry_lock); }
  RegistryLock(const RegistryLock&) = delete;
  RegistryLock& operator=(const RegistryLock&) = delete;
};
std::vector<const ScopeStack*> g_live_stacks;

enum class ReferenceMatch { none, matched, refused };

// Exact-reference lookup: the plug-in handed back the very struct the host
// registered. A registration whose geometry no longer describes what the
// caller is holding is the registry's fail-closed refusal, not a reason to
// keep looking. At most one entry per world exists per scope (`register_world`
// erases the previous one), so scope order alone decides which registration
// answers.
ReferenceMatch match_registered_reference(const ScopeStack& stack, const void* world,
                                          void* data, int32_t rowbytes, int32_t width,
                                          int32_t height, DispatchWorldFormat& result) {
  for (auto scope = stack.rbegin(); scope != stack.rend(); ++scope) {
    for (auto entry = scope->rbegin(); entry != scope->rend(); ++entry) {
      if (entry->world != world) continue;
      if (entry->data != data || entry->rowbytes != rowbytes ||
          entry->width != width || entry->height != height)
        return ReferenceMatch::refused;
      result = *entry;
      return ReferenceMatch::matched;
    }
  }
  return ReferenceMatch::none;
}

// Layout lookup, for the plug-in that copied the PF_EffectWorld struct and
// hands back a different address for the same pixels (PSL_Adjustments' worker
// thread gets exactly such a copy on its own stack). The whole declared
// geometry has to equal a registration's, and two registrations answering to
// it disagreeing about the world or its format refuse the call.
struct LayoutMatch {
  const DispatchWorldFormat* entry{};
  bool ambiguous{};
};

void accumulate_layout_match(const ScopeStack& stack, void* data, int32_t rowbytes,
                             int32_t width, int32_t height, LayoutMatch& match) {
  for (auto scope = stack.rbegin(); scope != stack.rend(); ++scope) {
    for (const auto& entry : *scope) {
      if (entry.data != data || entry.rowbytes != rowbytes ||
          entry.width != width || entry.height != height)
        continue;
      if (match.entry && (match.entry->world != entry.world ||
                          match.entry->pixel_format != entry.pixel_format)) {
        match.ambiguous = true;
        return;
      }
      match.entry = &entry;
    }
  }
}

// The exact-reference pass over every *other* live thread's stack, accumulated
// rather than first-answer-wins.
//
// Within one thread, scopes nest and the innermost registration is the current
// one, so taking the first match walking outwards is a rule. Across threads
// there is no such rule - `g_live_stacks` is in publication order, which means
// nothing - so two stacks answering the same reference differently is
// ambiguity, and ambiguity is refused, exactly as in the layout pass. Any
// stack's geometry-mismatch refusal refuses the whole lookup, which also makes
// the answer independent of the order the stacks are walked in.
bool match_foreign_reference(const void* world, void* data, int32_t rowbytes,
                             int32_t width, int32_t height,
                             const ScopeStack* own, bool& refused,
                             DispatchWorldFormat& result) {
  bool found = false;
  DispatchWorldFormat candidate{};
  for (const ScopeStack* stack : g_live_stacks) {
    if (stack == own) continue;
    DispatchWorldFormat entry{};
    switch (match_registered_reference(*stack, world, data, rowbytes, width, height,
                                       entry)) {
      case ReferenceMatch::refused:
        refused = true;
        return false;
      case ReferenceMatch::matched:
        // `world` is the same by construction on this pass, so only the format
        // can actually differ; the reference is compared anyway so that this
        // reads as the same disagreement test `accumulate_layout_match` makes,
        // where both halves are live.
        if (found && (candidate.world != entry.world ||
                      candidate.pixel_format != entry.pixel_format)) {
          refused = true;
          return false;
        }
        candidate = entry;
        found = true;
        break;
      case ReferenceMatch::none:
        break;
    }
  }
  if (!found) return false;
  result = candidate;
  return true;
}

// Both passes again, over every other live thread's stack. Reached only when
// the calling thread's own stack answered neither, so a host thread that
// resolves through its own scope pays nothing for it; one that misses - the
// foreign-operand fallback in the copy and blur callbacks always does - pays a
// global lock and a walk of the live stacks.
bool resolve_in_foreign_scopes(const void* world, void* data, int32_t rowbytes,
                               int32_t width, int32_t height,
                               DispatchWorldFormat& result) {
  const ScopeStack* own = &g_dispatch_world_formats;
  const RegistryLock lock;
  bool refused = false;
  if (match_foreign_reference(world, data, rowbytes, width, height, own, refused,
                              result))
    return true;
  if (refused) return false;
  LayoutMatch match{};
  for (const ScopeStack* stack : g_live_stacks) {
    if (stack == own) continue;
    accumulate_layout_match(*stack, data, rowbytes, width, height, match);
    if (match.ambiguous) return false;
  }
  if (!match.entry) return false;
  result = *match.entry;
  return true;
}

bool read_world_layout(const void* world, void*& data, int32_t& rowbytes,
                       int32_t& width, int32_t& height) {
  if (!world) return false;
  const auto* bytes = static_cast<const std::byte*>(world);
  std::memcpy(&data, bytes + 24, sizeof(data));
  std::memcpy(&rowbytes, bytes + 32, sizeof(rowbytes));
  std::memcpy(&width, bytes + 36, sizeof(width));
  std::memcpy(&height, bytes + 40, sizeof(height));
  return true;
}

int32_t facade_pixel_bytes(int32_t pixel_format) {
  using namespace aexcompat::world_registry;
  if (pixel_format == kPixelFormatArgb32) return 4;
  if (pixel_format == kPixelFormatArgb64) return 8;
  if (pixel_format == kPixelFormatArgb128 || pixel_format == kPixelFormatGpuBgra128)
    return 16;
  return 0;
}

// Every world registered for dispatch is a world the host hands the plug-in,
// so this is where it gets AE's PF_World identity behind reserved_long4
// (worker_pf_world_facade, issue #1276). A registration that succeeds without
// a facade (pool exhausted) is still a registration: the world is usable, the
// plug-in just meets the pre-#1276 null there.
// A world that belongs to the GPU stack is not the CPU PF_World shape a
// plug-in dereferences behind reserved_long4, and its +0x50 and +0x40 are the
// VideoFrame / GPUFoundation stack's (registering the colour effects' GPU
// worlds with a facade crashed CreateGPUVideoFrame, issue #1276). Three
// separate signs, any of which disqualifies a world:
//   * the GPU pixel format (the caller says so outright);
//   * `platform_ref` (+0x40) set - the VideoFrame adapter stores its PPix
//     handle there, and PF.dll's own
//     `ae::pf::VideoFrameFactory::IsEffectWorldGPUBased` reads the same field;
//   * a null pixel pointer, the other half of PF.dll's test.
// The render transport also swaps a host world's +24 to a CUDA device pointer
// before re-registering it; that world keeps its `platform_ref`, so the second
// sign catches it and the facade is not refreshed onto device memory.
bool gpu_stack_world(const void* world, int32_t pixel_format) {
  if (pixel_format == aexcompat::world_registry::kPixelFormatGpuBgra128) return true;
  const auto* bytes = static_cast<const std::byte*>(world);
  void* data{};
  void* platform_ref{};
  std::memcpy(&data, bytes + 24, sizeof(data));
  std::memcpy(&platform_ref, bytes + 64, sizeof(platform_ref));
  return platform_ref != nullptr || data == nullptr;
}

void attach_facade(void* world, int32_t pixel_format) {
  if (gpu_stack_world(world, pixel_format)) return;
  namespace facade = aexcompat::worker_runtime::pf_world_facade;
  if (!facade::attach(world, facade_pixel_bytes(pixel_format))) return;
  // Only a pool object needs taking back; an embedded or registry-published
  // world was left untouched by `attach` and outlives the scope with its owner.
  //
  // A world registered twice in one scope lands here twice; `detach` is
  // idempotent, so the duplicate costs a repeated call and nothing else. The
  // one asymmetry worth naming is that `register_gpu_world` attaches nothing
  // and so puts nothing on this list: a struct registered as a CPU world and as
  // a GPU world would have the CPU scope decline to detach (the GPU
  // registration still names it) and the GPU scope never detach. That is not
  // new - nesting a CPU registration inside a GPU one on a single thread has
  // always had the same shape, because the retention check has always walked
  // the enclosing scopes; crossing threads only widens where the second
  // registration may live. (The other order does not: the inner GPU scope ends
  // with nothing attached, and the outer CPU scope then finds nothing naming
  // the world and detaches.) Registering one struct as both is contradictory
  // and nothing does it.
  if (facade::attached(world) && !g_attached_facades.empty())
    g_attached_facades.back().push_back(world);
}

}  // namespace

// A scope must not be live across a plug-in-facing SEH boundary. Under MSVC's
// `/EHsc` model a C++ destructor does not run when an SEH exception unwinds to
// an outer `__except`, so a skipped destructor would leave this thread's stack
// one entry deep for good - and, since #1299, published to every other thread
// for good, with the thread's exit turning that publication into a pointer to a
// destroyed `thread_local`. Nothing currently violates it: the two production
// scopes both live in `worker_classic_render_runtime` (the smart route borrows
// one of them by reference) and both enclose the selector `__try` in
// `worker_selector_dispatch` rather than sitting inside it, and every other
// construction site - `worker_flt_blur_suite`, `worker_pf_world_transform_runtime`,
// `worker_smart_setup`, `worker_pf_path_selftests`, `l2_main_entry.inc`, and
// `tests/native/worker_cross_thread_dispatch_world_selftest.cpp` - is in a
// self-test entry point (`verify_*`, `flt_blur::selftest`, or a test `main`).
// The invariant used to be tidiness; it is memory safety now, so it is written
// down.
DispatchWorldFormatScope::DispatchWorldFormatScope() {
  // Either both halves of the scope come into existence or neither does: the
  // destructor does not run when a constructor throws, so a half-built scope
  // would leave its thread's stack one entry deep forever - and, for the
  // outermost one, published to foreign readers after the thread stopped
  // dispatching, which outlives the thread itself.
  g_attached_facades.emplace_back();
  try {
    const RegistryLock lock;
    const bool publish = g_dispatch_world_formats.empty();
    g_dispatch_world_formats.emplace_back();
    if (publish) {
      try {
        g_live_stacks.push_back(&g_dispatch_world_formats);
      } catch (...) {
        g_dispatch_world_formats.pop_back();
        throw;
      }
    }
  } catch (...) {
    g_attached_facades.pop_back();
    throw;
  }
}

DispatchWorldFormatScope::~DispatchWorldFormatScope() {
  // The facade a registration attached lives exactly as long as the
  // registration: past the scope the world's pixels may be gone, and a mirror
  // that outlived them would still name them. Embedded worlds are not in this
  // list (nothing was attached for them).
  //
  // A world some *other live* registration also names keeps its facade -
  // whether that registration is an enclosing scope of this thread (scopes
  // nest: a callback's inside a dispatch's) or a scope on another thread.
  // Taking the facade away while any live registration still hands the plug-in
  // that world would leave it holding one whose reserved_long4 the host just
  // cleared, and since #1299 a registration on another thread is just as
  // reachable as one on this thread.
  //
  // Retire this scope's registrations first, then detach. Between a detach and
  // the pop, a plug-in-owned thread would still resolve a world whose facade
  // was already neutralized and get a null-vtable fault where the design
  // promises a refusal; popping under the lock closes that window. The decision
  // is made under the same lock, because it reads other threads' stacks, and
  // the detaching happens after it is released so that no call into the facade
  // pool ever nests inside this lock.
  //
  // What it closes is the resolve direction, not every direction. Because the
  // decision here and the detach below straddle the lock release (and
  // `register_world` likewise attaches after releasing it), two *host dispatch*
  // threads registering the same struct could interleave as: this scope decides
  // nothing else names the world, the other thread pushes its registration and
  // attaches, and this scope then detaches - leaving a live registration whose
  // reserved_long4 is cleared. The duplicate entries `attach_facade` can leave
  // on this list have the same precondition: the second `detach` is a no-op
  // only while nobody else has re-attached in between. Both need two concurrent
  // host dispatch threads on one struct, and the only production scopes are the
  // classic and smart render runtimes on the worker's dispatch thread, so
  // nothing reaches either; naming them is cheaper than implying the lock
  // covers more than it does.
  auto& attached = g_attached_facades.back();
  {
    const RegistryLock lock;
    g_dispatch_world_formats.pop_back();
    // The publication lasts exactly as long as the stack has scopes on it, so
    // a thread that has stopped dispatching leaves nothing stale behind.
    if (g_dispatch_world_formats.empty())
      g_live_stacks.erase(std::remove(g_live_stacks.begin(), g_live_stacks.end(),
                                      &g_dispatch_world_formats),
                          g_live_stacks.end());
    // Drops from the detach list every world some live registration still
    // names. `remove_if` shrinks the list in place - a destructor cannot
    // allocate a second one without risking `terminate`.
    const auto still_registered = [](const void* world) {
      const auto names = [&](const ScopeStack& stack) {
        for (const auto& entries : stack) {
          if (std::any_of(entries.begin(), entries.end(),
                          [&](const DispatchWorldFormat& entry) {
                            return entry.world == world;
                          }))
            return true;
        }
        return false;
      };
      // This scope's own entries are already popped, so what is left of this
      // thread's stack is exactly its enclosing scopes.
      if (names(g_dispatch_world_formats)) return true;
      for (const ScopeStack* stack : g_live_stacks)
        if (stack != &g_dispatch_world_formats && names(*stack)) return true;
      return false;
    };
    attached.erase(std::remove_if(attached.begin(), attached.end(), still_registered),
                   attached.end());
  }
  for (void* world : attached)
    aexcompat::worker_runtime::pf_world_facade::detach(world);
  g_attached_facades.pop_back();
}

bool DispatchWorldFormatScope::register_world(void* world, int32_t pixel_format) {
  if (!world || g_dispatch_world_formats.empty()) return false;
  DispatchWorldFormat entry{};
  entry.world = world;
  entry.pixel_format = pixel_format;
  entry.generation = g_dispatch_world_generation.fetch_add(1) + 1;
  if (!read_world_layout(world, entry.data, entry.rowbytes, entry.width, entry.height) ||
      !entry.data || entry.width <= 0 || entry.height <= 0 || entry.rowbytes <= 0)
    return false;
  {
    const RegistryLock lock;
    auto& entries = g_dispatch_world_formats.back();
    entries.erase(std::remove_if(entries.begin(), entries.end(),
                                 [&](const auto& old) { return old.world == world; }),
                  entries.end());
    entries.push_back(entry);
  }
  // Outside the lock: the facade pool is its own object with its own
  // synchronization, and this scope's facade list is thread-local.
  attach_facade(world, pixel_format);
  return true;
}

// No facade here by construction: this registers a world the GPU stack owns.
bool DispatchWorldFormatScope::register_gpu_world(void* world, int32_t pixel_format) {
  if (!world || g_dispatch_world_formats.empty()) return false;
  DispatchWorldFormat entry{};
  entry.world = world;
  entry.pixel_format = pixel_format;
  entry.generation = g_dispatch_world_generation.fetch_add(1) + 1;
  void* platform_ref{};
  std::memcpy(&platform_ref, static_cast<const std::byte*>(world) + 64,
              sizeof(platform_ref));
  if (!read_world_layout(world, entry.data, entry.rowbytes, entry.width,
                         entry.height) || entry.data || !platform_ref ||
      entry.width <= 0 || entry.height <= 0 || entry.rowbytes < 0)
    return false;
  const RegistryLock lock;
  auto& entries = g_dispatch_world_formats.back();
  entries.erase(std::remove_if(entries.begin(), entries.end(),
                               [&](const auto& old) { return old.world == world; }),
                entries.end());
  entries.push_back(entry);
  return true;
}

bool resolve_dispatch_world_format(const void* world,
                                   OwnedWorldResolver owned_resolver,
                                   DispatchWorldFormat& result) {
  void* data{};
  int32_t rowbytes{}, width{}, height{};
  if (!read_world_layout(world, data, rowbytes, width, height) || !owned_resolver)
    return false;

  const auto owned = owned_resolver(world, data, rowbytes, width, height, result);
  if (owned == OwnedWorldResolution::resolved) {
    result.generation = g_dispatch_world_generation.load();
    return true;
  }
  if (owned == OwnedWorldResolution::rejected) return false;

  switch (match_registered_reference(g_dispatch_world_formats, world, data, rowbytes,
                                     width, height, result)) {
    case ReferenceMatch::matched: return true;
    // The reference is one this thread registered and the caller's copy of it
    // no longer describes the registration: a mismatch refusal, not a miss, so
    // it does not go looking on another thread for a friendlier answer.
    case ReferenceMatch::refused: return false;
    case ReferenceMatch::none: break;
  }
  LayoutMatch own{};
  accumulate_layout_match(g_dispatch_world_formats, data, rowbytes, width, height, own);
  if (own.ambiguous) return false;
  if (own.entry) {
    result = *own.entry;
    return true;
  }
  // Nothing this thread registered. The plug-in may be calling from a thread it
  // owns, about a world the dispatch thread registered (issue #1299).
  return resolve_in_foreign_scopes(world, data, rowbytes, width, height, result);
}

bool dispatch_world_reference_known(const void* world) {
  void* data{};
  int32_t rowbytes{}, width{}, height{};
  if (!read_world_layout(world, data, rowbytes, width, height)) return false;
  const auto known_in = [&](const ScopeStack& stack) {
    for (auto scope = stack.rbegin(); scope != stack.rend(); ++scope) {
      for (const auto& entry : *scope) {
        if (entry.world == world || (data && entry.data == data)) return true;
      }
    }
    return false;
  };
  if (known_in(g_dispatch_world_formats)) return true;
  // Cross-thread for the same reason as the resolver, and in the same
  // direction: this answers "the registry knows this reference", so reaching
  // further can only turn a foreign-operand admission back into the refusal the
  // registry already decided on.
  const ScopeStack* own = &g_dispatch_world_formats;
  const RegistryLock lock;
  for (const ScopeStack* stack : g_live_stacks)
    if (stack != own && known_in(*stack)) return true;
  return false;
}

bool resolve_registered_dispatch_world(const void* world,
                                       DispatchWorldFormat& result) {
  void* data{};
  int32_t rowbytes{}, width{}, height{};
  if (!read_world_layout(world, data, rowbytes, width, height)) return false;
  switch (match_registered_reference(g_dispatch_world_formats, world, data, rowbytes,
                                     width, height, result)) {
    case ReferenceMatch::matched: return true;
    case ReferenceMatch::refused: return false;
    case ReferenceMatch::none: break;
  }
  // Exact reference only, as before - no layout fallback here. A plug-in-owned
  // thread still gets the same answer the dispatch thread would (issue #1299),
  // under the same across-threads ambiguity refusal as the resolver above.
  const ScopeStack* own = &g_dispatch_world_formats;
  const RegistryLock lock;
  // Nothing to do with `refused` here: this function has no second pass to
  // suppress, and `match_foreign_reference` already answers false whenever it
  // sets the flag.
  bool refused = false;
  return match_foreign_reference(world, data, rowbytes, width, height, own,
                                 refused, result);
}

bool bounded_typed_world(void* world, int32_t pixel_bytes,
                         unsigned char*& pixels, int32_t& rowbytes,
                         int32_t& width, int32_t& height) {
  if (!world) return false;
  auto* bytes = static_cast<std::byte*>(world);
  std::memcpy(&pixels, bytes + 24, sizeof(pixels));
  std::memcpy(&rowbytes, bytes + 32, sizeof(rowbytes));
  std::memcpy(&width, bytes + 36, sizeof(width));
  std::memcpy(&height, bytes + 40, sizeof(height));
  int32_t flags{};
  std::memcpy(&flags, bytes + 16, sizeof(flags));
  // The world_flags DEEP bit (bit 0) separates 8-bit from 16-bit; AE does not
  // set it on 32-bit float worlds. The video-frame float path builds its input
  // and output through AE's own PPix/pixel-format suites, and those worlds come
  // back with world_flags 0x02000000 (bit 0 clear) - so requiring the DEEP bit
  // for a float iterate rejected a valid AE float world and failed the callback
  // with missing_world (issue #1035, observed on Cartoon's iterateFloat). Keep
  // the 8-vs-16 distinction on the DEEP bit; accept float regardless of it.
  // Dropping the bit does not weaken buffer safety: that never came from the
  // DEEP bit (this function cannot see the real allocation) but from the
  // rowbytes >= width * pixel_bytes and rowbytes <= 4096 * 16 bounds below,
  // which cap every per-row walk at the declared stride for the whole declared
  // height. A caller that passes a shallower world to the float suite (its own
  // pixel_bytes choice) is read at its declared stride, in bounds, just with
  // the wrong depth semantics - the same latitude the pre-change code gave an
  // 8-bit world handed to the 8-bit suite.
  const bool depth_matches = pixel_bytes == 4 ? (flags & 1) == 0
      : pixel_bytes == 8 ? (flags & 1) != 0 : true;
  return pixels && (pixel_bytes == 4 || pixel_bytes == 8 || pixel_bytes == 16) &&
      width > 0 && height > 0 && width <= 4096 && height <= 4096 &&
      static_cast<int64_t>(width) * height <= 16'777'216 &&
      rowbytes >= width * pixel_bytes && rowbytes <= 4096 * 16 && depth_matches;
}

bool bounded_argb8_world(void* world, unsigned char*& pixels, int32_t& rowbytes,
                         int32_t& width, int32_t& height) {
  return bounded_typed_world(world, 4, pixels, rowbytes, width, height);
}

}  // namespace aexcompat::world_safety
