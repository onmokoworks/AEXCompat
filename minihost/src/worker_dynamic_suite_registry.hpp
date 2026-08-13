#pragma once

#include "worker_suite_registry.hpp"

#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

namespace aexcompat::worker_runtime::dynamic_suites {

inline constexpr char kSPSuitesSuiteName[] = "SP Suites Suite";
inline constexpr int32_t kSPSuitesSuiteVersion = 2;
inline constexpr std::size_t kMaximumSuites = 32;
inline constexpr std::size_t kMaximumSuiteNameBytes = 255;

// PICA errors from SPErrorCodes.h. Keep the values explicit: multi-character
// literals are implementation-defined and these reports cross compiler builds.
inline constexpr int32_t kSuiteNotFound = 0x53214664;       // 'S!Fd'
inline constexpr int32_t kSuiteAlreadyExists = 0x53457869; // 'SExi'
inline constexpr int32_t kSuiteAlreadyReleased = 0x5352656c; // 'SRel'
inline constexpr int32_t kBadSuiteInternalVersion = 0x53495673; // 'SIVs'
inline constexpr int32_t kBadParameter = 4;

struct RegistryStatistics {
  std::size_t registered{};
  std::size_t live_references{};
};

struct RegisteredSuiteIdentity {
  std::string name;
  int32_t api_version{};
  int32_t internal_version{};
};

const void* sp_suites_suite2() noexcept;
SuiteResolveResult resolve(void*, const char* name, int32_t version,
                           const void** suite) noexcept;
int32_t retain(const char* name, int32_t version, const void* suite) noexcept;
int32_t release(const char* name, int32_t version) noexcept;
RegistryStatistics statistics() noexcept;
std::vector<RegisteredSuiteIdentity> registered_suites();
// Bounded identities successfully registered during this process, retained
// after provider death so discovery can report what the AEGP supplied.
std::vector<RegisteredSuiteIdentity> observed_suites();
bool references_drained() noexcept;

// Refuses to forget tables while a PF consumer still owns a reference. The
// companion module that owns the function pointers must remain loaded until
// this succeeds.
bool drain() noexcept;
void reset_for_selftest() noexcept;

}  // namespace aexcompat::worker_runtime::dynamic_suites
