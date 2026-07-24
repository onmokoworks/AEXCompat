#include "worker_pf_state_runtime.hpp"

#include <windows.h>
#include <bcrypt.h>

#include <algorithm>
#include <array>
#include <cstring>
#include <mutex>
#include <new>
#include <unordered_map>
#include <utility>

namespace aexcompat::pf_state_runtime {
namespace {

constexpr int32_t kPfBadCallbackParam = 516;
constexpr std::size_t kMaxPfStateRegistryEntries = 4096;
constexpr std::size_t kMaxLiveEffectSequences = 64;

using PfStateToken = std::array<unsigned char, sizeof(PfState)>;
struct PfStateTokenHash {
  std::size_t operator()(const PfStateToken& token) const noexcept {
    std::size_t value = 0;
    std::memcpy(&value, token.data(), (std::min)(sizeof(value), token.size()));
    return value;
  }
};
struct PfStateRegistryEntry {
  void* owner{};
  uint64_t generation{};
  int32_t param_index{};
  bool has_range{};
  PfTime start{};
  PfTime duration{};
  std::vector<unsigned char> canonical_snapshot;
};

HostHooks g_host_hooks{};
std::mutex g_pf_state_registry_mutex;
std::unordered_map<PfStateToken, PfStateRegistryEntry, PfStateTokenHash>
    g_pf_state_registry;
uint64_t g_pf_state_effect_generation = 1;
bool g_pf_state_effect_live = true;
uint32_t g_pf_get_current_state_calls{};
uint32_t g_pf_are_states_identical_calls{};

template <typename T>
void append_bytes(std::vector<unsigned char>& snapshot, const T& value) {
  const auto* bytes = reinterpret_cast<const unsigned char*>(&value);
  snapshot.insert(snapshot.end(), bytes, bytes + sizeof(value));
}

void* effect_ref() {
  return g_host_hooks.effect_ref ? g_host_hooks.effect_ref() : nullptr;
}

bool valid_param_index(int32_t index, bool allow_groups) {
  return g_host_hooks.valid_param_index &&
      g_host_hooks.valid_param_index(index, allow_groups);
}

bool canonical_param_state_snapshot(int32_t index, const PfTime* start,
                                    const PfTime* duration,
                                    std::vector<unsigned char>& snapshot) {
  try {
    snapshot.clear();
    append_bytes(snapshot, index);
    const uint8_t has_range = start ? 1 : 0;
    append_bytes(snapshot, has_range);
    if (start) {
      append_bytes(snapshot, *start);
      append_bytes(snapshot, *duration);
    }
    return g_host_hooks.capture_parameter_state &&
        g_host_hooks.capture_parameter_state(index, snapshot);
  } catch (const std::bad_alloc&) {
    snapshot.clear();
    return false;
  }
}

void purge_pf_state_registry_locked(void* owner) {
  for (auto it = g_pf_state_registry.begin(); it != g_pf_state_registry.end();) {
    if (!owner || it->second.owner == owner) it = g_pf_state_registry.erase(it);
    else ++it;
  }
}

bool valid_obsolete_param_state(void* owner, const PfState* state) {
  if (owner != effect_ref() || !state) return false;
  PfStateToken token{};
  std::memcpy(token.data(), state, token.size());
  std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
  const auto found = g_pf_state_registry.find(token);
  return g_pf_state_effect_live && found != g_pf_state_registry.end() &&
      found->second.owner == owner &&
      found->second.generation == g_pf_state_effect_generation;
}

struct LiveEffectSequence {
  void* effect_ref{};
  PfConstHandle sequence_handle{};
  uint64_t generation{};
};
std::mutex g_effect_sequence_mutex;
std::vector<LiveEffectSequence> g_live_effect_sequences;
uint64_t g_effect_sequence_generation{};
uint64_t g_effect_sequence_publication_count{};
uint64_t g_effect_sequence_invalidation_count{};

int32_t __cdecl get_effect_sequence_data(
    void* owner, PfConstHandle* sequence_handle) {
  if (sequence_handle) *sequence_handle = nullptr;
  if (!owner || !sequence_handle) return kPfBadCallbackParam;
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  const auto found = std::find_if(
      g_live_effect_sequences.begin(), g_live_effect_sequences.end(),
      [owner](const auto& live) { return live.effect_ref == owner; });
  if (found == g_live_effect_sequences.end() || !found->sequence_handle)
    return kPfBadCallbackParam;
  *sequence_handle = found->sequence_handle;
  return 0;
}

}  // namespace

PfEffectSequenceDataSuite1 g_effect_sequence_data_suite1{
    &get_effect_sequence_data};

void configure_host_hooks(HostHooks hooks) { g_host_hooks = hooks; }

void reset_effect_lifetime(bool live) {
  std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
  purge_pf_state_registry_locked(nullptr);
  if (++g_pf_state_effect_generation == 0) ++g_pf_state_effect_generation;
  g_pf_state_effect_live = live;
}
bool effect_is_live() {
  std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
  return g_pf_state_effect_live;
}
std::size_t live_state_count() {
  std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
  return g_pf_state_registry.size();
}
Statistics pf_state_statistics() {
  std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
  return {g_pf_get_current_state_calls, g_pf_are_states_identical_calls};
}

void reset_pf_state_statistics() {
  std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
  g_pf_get_current_state_calls = 0;
  g_pf_are_states_identical_calls = 0;
}

void on_global_setdown() { reset_effect_lifetime(false); }

int32_t __cdecl get_current_param_state(void* owner, int32_t index,
                                        const PfTime* start,
                                        const PfTime* duration,
                                        PfState* state) {
  if (owner != effect_ref() || !state || !valid_param_index(index, true) ||
      ((!start) != (!duration)) ||
      (start && (start->scale == 0 || duration->scale == 0)))
    return kPfBadCallbackParam;
  std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
  if (!g_pf_state_effect_live ||
      g_pf_state_registry.size() >= kMaxPfStateRegistryEntries)
    return kPfBadCallbackParam;
  PfStateRegistryEntry entry{owner, g_pf_state_effect_generation, index,
                             start != nullptr, start ? *start : PfTime{},
                             duration ? *duration : PfTime{}, {}};
  if (!canonical_param_state_snapshot(index, start, duration,
                                      entry.canonical_snapshot))
    return kPfBadCallbackParam;
  PfStateToken token{};
  do {
    if (BCryptGenRandom(nullptr, token.data(), static_cast<ULONG>(token.size()),
                        BCRYPT_USE_SYSTEM_PREFERRED_RNG) < 0)
      return kPfBadCallbackParam;
  } while (std::all_of(token.begin(), token.end(),
                       [](unsigned char byte) { return byte == 0; }) ||
           g_pf_state_registry.find(token) != g_pf_state_registry.end());
  try {
    g_pf_state_registry.emplace(token, std::move(entry));
  } catch (const std::bad_alloc&) {
    return kPfBadCallbackParam;
  }
  std::memcpy(state, token.data(), token.size());
  ++g_pf_get_current_state_calls;
  return 0;
}

int32_t __cdecl are_param_states_identical(void* owner,
                                           const PfState* first,
                                           const PfState* second,
                                           uint8_t* same) {
  if (owner != effect_ref() || !first || !second || !same)
    return kPfBadCallbackParam;
  PfStateToken first_token{}, second_token{};
  std::memcpy(first_token.data(), first, first_token.size());
  std::memcpy(second_token.data(), second, second_token.size());
  std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
  const auto left = g_pf_state_registry.find(first_token);
  const auto right = g_pf_state_registry.find(second_token);
  if (!g_pf_state_effect_live || left == g_pf_state_registry.end() ||
      right == g_pf_state_registry.end() || left->second.owner != owner ||
      right->second.owner != owner ||
      left->second.generation != g_pf_state_effect_generation ||
      right->second.generation != g_pf_state_effect_generation)
    return kPfBadCallbackParam;
  *same = left->second.param_index == right->second.param_index &&
          left->second.has_range == right->second.has_range &&
          (!left->second.has_range ||
           (left->second.start.value == right->second.start.value &&
            left->second.start.scale == right->second.start.scale &&
            left->second.duration.value == right->second.duration.value &&
            left->second.duration.scale == right->second.duration.scale)) &&
          left->second.canonical_snapshot == right->second.canonical_snapshot
      ? 1 : 0;
  ++g_pf_are_states_identical_calls;
  return 0;
}

int32_t __cdecl get_current_param_state_obsolete(void* owner, PfState* state) {
  return get_current_param_state(owner, -1, nullptr, nullptr, state);
}
int32_t __cdecl has_param_changed_obsolete(void* owner, const PfState* state,
                                           int32_t, uint8_t* changed) {
  if (!changed || !valid_obsolete_param_state(owner, state))
    return kPfBadCallbackParam;
  *changed = 1;
  return 0;
}
int32_t __cdecl have_inputs_changed_over_time_span_obsolete(
    void* owner, const PfState* state, const PfTime* start,
    const PfTime* duration, uint8_t* changed) {
  if (!changed || ((!start) != (!duration)) ||
      (start && (start->scale == 0 || duration->scale == 0)) ||
      !valid_obsolete_param_state(owner, state))
    return kPfBadCallbackParam;
  *changed = 1;
  return 0;
}

bool corrupt_state_owner_for_test(const PfState& state, void* replacement_owner) {
  PfStateToken token{};
  std::memcpy(token.data(), &state, token.size());
  std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
  const auto found = g_pf_state_registry.find(token);
  if (found == g_pf_state_registry.end()) return false;
  found->second.owner = replacement_owner;
  return true;
}

void fill_registry_to_capacity_for_test(void* owner, int32_t param_index) {
  std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
  PfStateRegistryEntry filler{owner, g_pf_state_effect_generation, param_index,
                              false, {}, {}, {}};
  for (std::size_t i = 0; i < kMaxPfStateRegistryEntries; ++i) {
    PfStateToken token{};
    std::memcpy(token.data(), &i, sizeof(i));
    token.back() = 0xa5;
    g_pf_state_registry.emplace(token, filler);
  }
}

void invalidate_effect_sequence(void* owner) {
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  const auto old_size = g_live_effect_sequences.size();
  g_live_effect_sequences.erase(
      std::remove_if(g_live_effect_sequences.begin(), g_live_effect_sequences.end(),
                     [owner](const auto& live) { return live.effect_ref == owner; }),
      g_live_effect_sequences.end());
  if (g_live_effect_sequences.size() != old_size)
    ++g_effect_sequence_invalidation_count;
}

bool publish_effect_sequence(void* owner, void* sequence_handle) {
  if (!owner || !sequence_handle) return false;
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  auto found = std::find_if(
      g_live_effect_sequences.begin(), g_live_effect_sequences.end(),
      [owner](const auto& live) { return live.effect_ref == owner; });
  if (found == g_live_effect_sequences.end()) {
    if (g_live_effect_sequences.size() >= kMaxLiveEffectSequences) return false;
    g_live_effect_sequences.push_back(
        {owner, reinterpret_cast<PfConstHandle>(sequence_handle),
         ++g_effect_sequence_generation});
  } else {
    found->sequence_handle = reinterpret_cast<PfConstHandle>(sequence_handle);
    found->generation = ++g_effect_sequence_generation;
  }
  ++g_effect_sequence_publication_count;
  return true;
}

uint64_t effect_sequence_publications() {
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  return g_effect_sequence_publication_count;
}
uint64_t effect_sequence_invalidations() {
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  return g_effect_sequence_invalidation_count;
}
std::size_t live_effect_sequence_count() {
  std::lock_guard<std::mutex> lock(g_effect_sequence_mutex);
  return g_live_effect_sequences.size();
}

}  // namespace aexcompat::pf_state_runtime
