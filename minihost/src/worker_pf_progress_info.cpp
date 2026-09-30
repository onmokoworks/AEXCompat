#include "worker_pf_progress_info.hpp"

#include <atomic>
#include <cstring>

// The host's own abort / progress callbacks (worker_classic_runtime.cpp), the
// same functions `in_data->inter.abort` / `inter.progress` carry.
extern "C" {
int32_t __cdecl abort_render(void*);
int32_t __cdecl report_progress(void*, int32_t, int32_t);
}

namespace aexcompat::worker_runtime::pf_progress_info {
namespace {

std::atomic<uint32_t> g_abort_slot_calls{};
std::atomic<uint32_t> g_progress_slot_calls{};

int32_t __cdecl slot_abort(void* refcon) {
  ++g_abort_slot_calls;
  return abort_render(refcon);
}

int32_t __cdecl slot_progress(void* refcon, int32_t current, int32_t total) {
  ++g_progress_slot_calls;
  return report_progress(refcon, current, total);
}

}  // namespace

void publish(EffectRefObject& object) noexcept {
  if (published(object)) return;
  object.refcon = &object;
  object.abort_fn = &slot_abort;
  object.progress_fn = &slot_progress;
  std::memset(object.reserved_18, 0, sizeof(object.reserved_18));
}

bool published(const EffectRefObject& object) noexcept {
  if (object.refcon != &object || object.abort_fn != &slot_abort ||
      object.progress_fn != &slot_progress)
    return false;
  for (const std::byte b : object.reserved_18)
    if (b != std::byte{}) return false;
  return true;
}

uint32_t abort_slot_calls() noexcept { return g_abort_slot_calls.load(); }
uint32_t progress_slot_calls() noexcept { return g_progress_slot_calls.load(); }

bool selftest() {
  EffectRefObject object{};
  publish(object);
  if (!published(object)) return false;
  // Read the slots the way PF.dll / CannedWarp do: raw loads at +0 / +8 / +0x10
  // of the pointer a plug-in holds, no knowledge of the host's type.
  const auto* bytes = reinterpret_cast<const std::byte*>(&object);
  void* refcon{};
  AbortFn abort_fn{};
  ProgressFn progress_fn{};
  std::memcpy(&refcon, bytes + 0x00, sizeof(refcon));
  std::memcpy(&abort_fn, bytes + 0x08, sizeof(abort_fn));
  std::memcpy(&progress_fn, bytes + 0x10, sizeof(progress_fn));
  if (refcon != &object || !abort_fn || !progress_fn) return false;
  const uint32_t aborts = abort_slot_calls();
  const uint32_t progresses = progress_slot_calls();
  // The abort poll answers 0 (continue) for the host's effect; progress with
  // any ratio (the host clamps rather than refuses, issue #1037) answers 0.
  if (abort_fn(refcon) != 0 || progress_fn(refcon, 3, 10) != 0 ||
      progress_fn(refcon, -1, 0) != 0)
    return false;
  if (abort_slot_calls() != aborts + 1 || progress_slot_calls() != progresses + 2)
    return false;
  // Echo's substitution: the +8 slot called with the progress arguments is a
  // valid abort poll (extra arguments are ignored by the callee).
  const auto as_progress = reinterpret_cast<ProgressFn>(abort_fn);
  if (as_progress(refcon, 5, 10) != 0 || abort_slot_calls() != aborts + 2) return false;
  // A plug-in that left the object edited (Echo writes +0x10 <- +8 and restores
  // it; a crashed one might not) is republished at the next hand-out.
  object.progress_fn = reinterpret_cast<ProgressFn>(object.abort_fn);
  if (published(object)) return false;
  publish(object);
  if (!published(object)) return false;
  object.reserved_18[7] = std::byte{1};
  if (published(object)) return false;
  publish(object);
  return published(object);
}

}  // namespace aexcompat::worker_runtime::pf_progress_info
