#pragma once

#include <cstddef>
#include <cstdint>
#include <vector>

namespace aexcompat::pf_state_runtime {

struct PfState { int32_t reserved[4]; };
struct PfTime { int32_t value; uint32_t scale; };

struct HostHooks {
  void* (*effect_ref)(){};
  bool (*valid_param_index)(int32_t index, bool allow_groups){};
  bool (*capture_parameter_state)(int32_t index,
                                  std::vector<unsigned char>& snapshot){};
};

void configure_host_hooks(HostHooks hooks);
void reset_effect_lifetime(bool live);
bool effect_is_live();
std::size_t live_state_count();
struct Statistics {
  uint32_t get_current_state_calls{};
  uint32_t are_states_identical_calls{};
};
Statistics pf_state_statistics();
// Cluster sessions (issue #405) reset the per-effect counters between
// plug-ins so each swap/inspect report matches a fresh one-shot process.
void reset_pf_state_statistics();
void on_global_setdown();

int32_t __cdecl get_current_param_state(void*, int32_t, const PfTime*,
                                        const PfTime*, PfState*);
int32_t __cdecl are_param_states_identical(void*, const PfState*,
                                           const PfState*, uint8_t*);
int32_t __cdecl get_current_param_state_obsolete(void*, PfState*);
int32_t __cdecl has_param_changed_obsolete(void*, const PfState*, int32_t,
                                           uint8_t*);
int32_t __cdecl have_inputs_changed_over_time_span_obsolete(
    void*, const PfState*, const PfTime*, const PfTime*, uint8_t*);

// Bounded adversarial self-test support; never exposes the registry itself.
bool corrupt_state_owner_for_test(const PfState& state, void* replacement_owner);
void fill_registry_to_capacity_for_test(void* owner, int32_t param_index);

using PfConstHandle = const void* const*;
using GetEffectSequenceData = int32_t(__cdecl*)(void*, PfConstHandle*);
struct PfEffectSequenceDataSuite1 {
  GetEffectSequenceData get_effect_sequence_data;
};
static_assert(sizeof(PfEffectSequenceDataSuite1) == sizeof(void*));

extern PfEffectSequenceDataSuite1 g_effect_sequence_data_suite1;
void invalidate_effect_sequence(void* effect_ref);
bool publish_effect_sequence(void* effect_ref, void* sequence_handle);
uint64_t effect_sequence_publications();
uint64_t effect_sequence_invalidations();
std::size_t live_effect_sequence_count();

}  // namespace aexcompat::pf_state_runtime
