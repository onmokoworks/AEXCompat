#pragma once

#include "worker_suite_registry.hpp"

#include <cstddef>
#include <cstdint>

namespace aexcompat {
class TraceWriter;
}

namespace aexcompat::worker_runtime::host_suites {

struct Provider {
  SuiteResolver resolve{};
  void* context{};
};

struct ProviderCatalog {
  const Provider* providers{};
  std::size_t provider_count{};
  SuiteResolver fallback{};
  void* fallback_context{};
};

struct StaticSuite {
  const char* name{};
  int32_t version{};
  const void* suite{};
  const void* (*factory)(void* context){};
  void* factory_context{};
  bool (*available)(void* context){};
  void* availability_context{};
};

struct StaticProviderCatalog {
  const StaticSuite* suites{};
  std::size_t suite_count{};
};

SuiteResolveResult resolve_static_provider(void* context, const char* name,
                                           int32_t version,
                                           const void** suite);

int32_t acquire_host_suite(const ProviderCatalog& catalog, const char* name,
                           int32_t version, const void** suite,
                           TraceWriter* trace_writer);
int32_t release_host_suite(const char* name, int32_t version,
                           TraceWriter* trace_writer);

}  // namespace aexcompat::worker_runtime::host_suites
