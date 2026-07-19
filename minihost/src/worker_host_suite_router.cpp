#include "worker_host_suite_router.hpp"

#include <cstring>

namespace aexcompat::worker_runtime::host_suites {
namespace {

struct RoutingContext {
  const ProviderCatalog* catalog{};
};

SuiteResolveResult resolve_catalog(void* context, const char* name,
                                   int32_t version, const void** suite) {
  const auto* routing = static_cast<const RoutingContext*>(context);
  if (!routing || !routing->catalog) return SuiteResolveResult::not_found;
  const auto& catalog = *routing->catalog;
  for (std::size_t index = 0; index < catalog.provider_count; ++index) {
    const Provider& provider = catalog.providers[index];
    if (!provider.resolve) continue;
    const SuiteResolveResult result =
        provider.resolve(provider.context, name, version, suite);
    if (result != SuiteResolveResult::not_found) return result;
  }
  return catalog.fallback
      ? catalog.fallback(catalog.fallback_context, name, version, suite)
      : SuiteResolveResult::not_found;
}

}  // namespace

SuiteResolveResult resolve_static_provider(void* context, const char* name,
                                           int32_t version,
                                           const void** suite) {
  if (!context || !name || !suite) return SuiteResolveResult::not_found;
  const auto& catalog = *static_cast<const StaticProviderCatalog*>(context);
  for (std::size_t index = 0; index < catalog.suite_count; ++index) {
    const StaticSuite& candidate = catalog.suites[index];
    if (candidate.name && candidate.version == version &&
        std::strcmp(candidate.name, name) == 0) {
      if (candidate.available &&
          !candidate.available(candidate.availability_context))
        return SuiteResolveResult::not_found;
      *suite = candidate.factory
          ? candidate.factory(candidate.factory_context) : candidate.suite;
      return *suite ? SuiteResolveResult::acquired
                    : SuiteResolveResult::rejected_bad_param;
    }
  }
  return SuiteResolveResult::not_found;
}

int32_t acquire_host_suite(const ProviderCatalog& catalog, const char* name,
                           int32_t version, const void** suite,
                           TraceWriter* trace_writer) {
  RoutingContext context{&catalog};
  return suite_registry().acquire(name, version, suite, &resolve_catalog,
                                  &context, trace_writer);
}

int32_t release_host_suite(const char* name, int32_t version,
                           TraceWriter* trace_writer) {
  return suite_registry().release(name, version, trace_writer);
}

}  // namespace aexcompat::worker_runtime::host_suites
