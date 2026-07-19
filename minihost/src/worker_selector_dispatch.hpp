#pragma once

#include <cstdint>
#include <string>

namespace aexcompat::worker_runtime {

using EffectEntry = int32_t(__cdecl*)(int32_t, void*, void*, void**, void*, void*);
using AuditCapture = void(*)();
using AuditPassed = bool(*)();
using SelectorDispatchTrace = void(*)(const char* selector);

struct SelectorDispatchTelemetry {
  uint32_t seh_code{};
  uint64_t seh_address{};
  std::string seh_module;
  std::string selector;
  int32_t error{};
};

void configure_selector_dispatch_audit(AuditCapture capture,
                                       AuditPassed passed) noexcept;
void configure_selector_dispatch_trace(SelectorDispatchTrace trace) noexcept;
SelectorDispatchTelemetry& selector_dispatch_telemetry() noexcept;
const char* effect_selector_name(int32_t command) noexcept;
int32_t invoke_entry_seh(EffectEntry entry, int32_t command, void* input,
                         void* output, void** params, void* world, void* extra,
                         uint32_t* out_exception_code);
int32_t guarded_effect_call(EffectEntry entry, int32_t command, void* input,
                            void* output, void** params, void* world, void* extra);
int32_t invoke_smart_pre_render_cleanup_seh(void(__cdecl* cleanup)(void*),
                                            void* data);

}  // namespace aexcompat::worker_runtime
