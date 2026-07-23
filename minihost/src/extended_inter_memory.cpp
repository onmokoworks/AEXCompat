≠rá^—f•ñÿ¶{MÏy 'v√Æ∂õ≠#include "extended_inter_memory.hpp"

#include <algorithm>
#include <cstdlib>
#include <mutex>
#include <unordered_set>

namespace aexcompat::extended_inter {
namespace {

constexpr std::size_t kMaxAllocation = std::size_t{1} << 24;
std::mutex g_allocation_mutex;
std::unordered_set<void*> g_owned_allocations;

bool remember(void* allocation) {
  try {
    std::lock_guard lock(g_allocation_mutex);
    return g_owned_allocations.insert(allocation).second;
  } catch (...) {
    return false;
  }
}

bool forget(void* allocation) {
  try {
    std::lock_guard lock(g_allocation_mutex);
    const auto it = g_owned_allocations.find(allocation);
    if (it == g_owned_allocations.end()) return false;
    g_owned_allocations.erase(it);
    return true;
  } catch (...) {
    return false;
  }
}

}  // namespace

int32_t __cdecl allocate(void** out, std::size_t size) {
  if (!out) return 4;
  *out = nullptr;
  if (size > kMaxAllocation) return 4;

  // C permits calloc(1, 0) to return nullptr.  The observed AE contract
  // expects a successful zero-size slot to produce a releasable token, so use
  // one zeroed byte while preserving the requested logical size.
  void* allocation = std::calloc(1, std::max<std::size_t>(size, 1));
  if (!allocation || !remember(allocation)) {
    std::free(allocation);
    return 4;
  }
  *out = allocation;
  return 0;
}

int32_t __cdecl release(void** ptr) {
  if (!ptr) return 0;
  void* allocation = *ptr;
  *ptr = nullptr;
  if (!allocation) return 0;

  // Never pass a plug-in-owned/static/foreign pointer to free().
  if (!forget(allocation)) return 4;
  std::free(allocation);
  return 0;
}

}  // namespace aexcompat::extended_inter
