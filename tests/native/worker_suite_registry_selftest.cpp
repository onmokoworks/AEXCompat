#include "worker_suite_registry.hpp"

#include <windows.h>

#include <array>
#include <cstring>
#include <iostream>

namespace {

using aexcompat::worker_runtime::SuiteResolveResult;
using aexcompat::worker_runtime::UnsupportedSuiteId;
using aexcompat::worker_runtime::unsupported_suite_slots;

int g_resolver_calls{};

SuiteResolveResult resolve_known(void*, const char* name, int32_t version,
                                 const void** suite) {
  ++g_resolver_calls;
  if (std::strcmp(name, "Known Suite") == 0 && version == 1) {
    *suite = reinterpret_cast<const void*>(0x1234);
    return SuiteResolveResult::acquired;
  }
  return SuiteResolveResult::not_found;
}

bool rejected_without_resolving(aexcompat::worker_runtime::SuiteRegistry& registry,
                                const char* name) {
  const int calls_before = g_resolver_calls;
  const void* suite = reinterpret_cast<const void*>(0x5678);
  return registry.acquire(name, 1, &suite, &resolve_known, nullptr, nullptr) == 4 &&
      suite == nullptr && g_resolver_calls == calls_before &&
      registry.release(name, 1, nullptr) == 1;
}

}  // namespace

int main() {
  aexcompat::worker_runtime::SuiteRegistry registry;
  const void* suite{};
  bool passed = registry.acquire("Known Suite", 1, &suite, &resolve_known,
                                 nullptr, nullptr) == 0 &&
      suite == reinterpret_cast<const void*>(0x1234) &&
      registry.release("Known Suite", 1, nullptr) == 0 && registry.balanced();

  std::array<char, 98> overlong{};
  overlong.fill('A');
  overlong.back() = '\0';
  passed = passed && rejected_without_resolving(registry, overlong.data());

  passed = passed && rejected_without_resolving(
      registry, reinterpret_cast<const char*>(static_cast<uintptr_t>(1)));

  const auto& unsupported =
      unsupported_suite_slots<UnsupportedSuiteId::aegp_comp_21, 41>();
  const auto unsupported_slot =
      reinterpret_cast<int32_t(__cdecl*)()>(unsupported[7]);
  passed = passed && unsupported_slot() == 4 && unsupported_slot() == 4;
  const std::string unsupported_report =
      aexcompat::worker_runtime::suite_registry()
          .unsupported_suite_calls_report_json();
  passed = passed && unsupported_report ==
      ",\"unsupported_suite_calls\":[{\"name\":\"AEGP Comp Suite\","
      "\"version\":21,\"slot\":7,\"call_count\":2}]";

  SYSTEM_INFO system_info{};
  GetSystemInfo(&system_info);
  const std::size_t page_size = system_info.dwPageSize;
  auto* pages = static_cast<unsigned char*>(VirtualAlloc(
      nullptr, page_size * 2, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE));
  if (!pages) return 2;
  DWORD old_protection{};
  if (!VirtualProtect(pages + page_size, page_size, PAGE_NOACCESS,
                      &old_protection)) {
    VirtualFree(pages, 0, MEM_RELEASE);
    return 2;
  }
  char* unterminated = reinterpret_cast<char*>(pages + page_size - 96);
  std::memset(unterminated, 'B', 96);
  passed = passed && rejected_without_resolving(registry, unterminated);
  VirtualFree(pages, 0, MEM_RELEASE);

  std::cout << "{\"suite_registry_bounds\":\""
            << (passed ? "passed" : "failed")
            << "\",\"maximum_name_bytes\":96,\"fail_closed\":true}\n";
  return passed ? 0 : 1;
}
