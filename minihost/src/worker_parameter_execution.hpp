#pragma once

#include "worker_parameter_runtime.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <vector>

namespace aexcompat::worker_runtime::parameter_execution {

inline constexpr std::size_t kInputSize = 408;
inline constexpr std::size_t kOutputSize = 408;
using BufferIn = std::array<std::byte, kInputSize>;
using BufferOut = std::array<std::byte, kOutputSize>;
using Definitions = std::vector<parameters::Definition>;
using EffectEntry = int32_t(__cdecl*)(int32_t, void*, void*, void**, void*, void*);

struct Hooks {
  int32_t (*invoke_entry)(EffectEntry, int32_t, void*, void*, void**, void*,
                          void*, uint32_t*){};
  bool (*handle_is_live)(const void*){};
};

bool configure_hooks(const Hooks& hooks) noexcept;
bool initialize_arbitrary_values(EffectEntry, BufferIn&, BufferOut&, Definitions&);
bool apply_arbitrary_text_assignments(EffectEntry, BufferIn&, BufferOut&,
                                      Definitions&,
                                      const parameters::RequestedAssignments&);
bool dispose_arbitrary_values(EffectEntry, BufferIn&, BufferOut&, Definitions&);
bool dispose_arbitrary_defaults(EffectEntry, BufferIn&, BufferOut&);
bool apply_arbitrary_parameter_animation(EffectEntry, BufferIn&, BufferOut&,
                                         Definitions&, int32_t, uint32_t);
void observe_arbitrary_defaults(EffectEntry, BufferIn&, BufferOut&);
void probe_arbitrary_scan(EffectEntry, BufferIn&, BufferOut&, Definitions&);
bool interpolate_arbitrary_values(EffectEntry, BufferIn&, BufferOut&, Definitions&);
bool roundtrip_arbitrary_values(EffectEntry, BufferIn&, BufferOut&, Definitions&);

struct ArbitraryValuesScope {
  EffectEntry entry{};
  BufferIn* input{};
  BufferOut* output{};
  Definitions* definitions{};
  ~ArbitraryValuesScope();
};

}  // namespace aexcompat::worker_runtime::parameter_execution
