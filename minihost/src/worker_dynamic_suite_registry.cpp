#include "worker_dynamic_suite_registry.hpp"

#include <array>
#include <cstring>
#include <mutex>

namespace aexcompat::worker_runtime::dynamic_suites {
namespace {

struct Entry {
  bool occupied{};
  std::array<char, kMaximumSuiteNameBytes + 1> name{};
  int32_t api_version{};
  int32_t internal_version{};
  const void* procedures{};
  void* host{};
  uint32_t references{};
};

std::mutex g_mutex;
std::array<Entry, kMaximumSuites> g_entries{};
std::array<RegisteredSuiteIdentity, kMaximumSuites> g_observed{};
std::size_t g_observed_count{};

bool valid_name(const char* name, std::size_t& length) noexcept {
  if (!name) return false;
  length = 0;
  while (length <= kMaximumSuiteNameBytes && name[length] != '\0') ++length;
  return length != 0 && length <= kMaximumSuiteNameBytes;
}

Entry* find_locked(const char* name, int32_t api_version,
                   int32_t internal_version) noexcept {
  for (auto& entry : g_entries) {
    if (entry.occupied && entry.api_version == api_version &&
        entry.internal_version == internal_version &&
        std::strcmp(entry.name.data(), name) == 0)
      return &entry;
  }
  return nullptr;
}

int32_t __cdecl unsupported_list_operation(...) { return kBadParameter; }

int32_t __cdecl add_suite(void* list, void* host, const char* name,
                          int32_t api_version, int32_t internal_version,
                          const void* procedures, void** suite) {
  if (suite) *suite = nullptr;
  std::size_t length{};
  if (list || !valid_name(name, length) || api_version <= 0 ||
      internal_version < 0 || !procedures)
    return kBadParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  if (find_locked(name, api_version, internal_version))
    return kSuiteAlreadyExists;
  for (auto& entry : g_entries) {
    if (entry.occupied) continue;
    entry = {};
    entry.occupied = true;
    std::memcpy(entry.name.data(), name, length);
    entry.name[length] = '\0';
    entry.api_version = api_version;
    entry.internal_version = internal_version;
    entry.procedures = procedures;
    entry.host = host;
    if (g_observed_count < g_observed.size())
      g_observed[g_observed_count++] = {name, api_version, internal_version};
    if (suite) *suite = &entry;
    return 0;
  }
  return kBadParameter;
}

int32_t __cdecl acquire_suite(void* list, const char* name,
                              int32_t api_version, int32_t internal_version,
                              const void** procedures) {
  if (procedures) *procedures = nullptr;
  std::size_t ignored{};
  if (list || !procedures || !valid_name(name, ignored) || api_version <= 0 ||
      internal_version < 0)
    return kBadParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  Entry* entry = find_locked(name, api_version, internal_version);
  if (!entry) return kSuiteNotFound;
  if (entry->references == UINT32_MAX) return kBadParameter;
  ++entry->references;
  *procedures = entry->procedures;
  return 0;
}

int32_t __cdecl release_suite(void* list, const char* name,
                              int32_t api_version, int32_t internal_version) {
  std::size_t ignored{};
  if (list || !valid_name(name, ignored) || api_version <= 0 ||
      internal_version < 0)
    return kBadParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  Entry* entry = find_locked(name, api_version, internal_version);
  if (!entry) return kSuiteNotFound;
  if (entry->references == 0) return kSuiteAlreadyReleased;
  --entry->references;
  return 0;
}

int32_t __cdecl find_suite(void* list, const char* name, int32_t api_version,
                           int32_t internal_version, void** suite) {
  if (suite) *suite = nullptr;
  std::size_t ignored{};
  if (list || !suite || !valid_name(name, ignored) || api_version <= 0 ||
      internal_version < 0)
    return kBadParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  Entry* entry = find_locked(name, api_version, internal_version);
  if (!entry) return kSuiteNotFound;
  *suite = entry;
  return 0;
}

Entry* checked_entry(void* suite) noexcept {
  if (!suite) return nullptr;
  for (auto& entry : g_entries) {
    if (&entry == suite && entry.occupied) return &entry;
  }
  return nullptr;
}

int32_t __cdecl get_host(void* suite, void** host) {
  if (host) *host = nullptr;
  if (!host) return kBadParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  Entry* entry = checked_entry(suite);
  if (!entry) return kBadParameter;
  *host = entry->host;
  return 0;
}

int32_t __cdecl get_name(void* suite, const char** name) {
  if (name) *name = nullptr;
  if (!name) return kBadParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  Entry* entry = checked_entry(suite);
  if (!entry) return kBadParameter;
  *name = entry->name.data();
  return 0;
}

int32_t __cdecl get_api_version(void* suite, int32_t* version) {
  if (!version) return kBadParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  Entry* entry = checked_entry(suite);
  if (!entry) return kBadParameter;
  *version = entry->api_version;
  return 0;
}

int32_t __cdecl get_internal_version(void* suite, int32_t* version) {
  if (!version) return kBadParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  Entry* entry = checked_entry(suite);
  if (!entry) return kBadParameter;
  *version = entry->internal_version;
  return 0;
}

int32_t __cdecl get_procedures(void* suite, const void** procedures) {
  if (procedures) *procedures = nullptr;
  if (!procedures) return kBadParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  Entry* entry = checked_entry(suite);
  if (!entry) return kBadParameter;
  *procedures = entry->procedures;
  return 0;
}

int32_t __cdecl get_acquire_count(void* suite, int32_t* count) {
  if (!count) return kBadParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  Entry* entry = checked_entry(suite);
  if (!entry || entry->references > INT32_MAX) return kBadParameter;
  *count = static_cast<int32_t>(entry->references);
  return 0;
}

struct SPSuitesSuite2 {
  void* allocate_list;
  void* free_list;
  decltype(&add_suite) add;
  decltype(&acquire_suite) acquire;
  decltype(&release_suite) release;
  decltype(&find_suite) find;
  void* new_iterator;
  void* next;
  void* delete_iterator;
  decltype(&get_host) host;
  decltype(&get_name) name;
  decltype(&get_api_version) api_version;
  decltype(&get_internal_version) internal_version;
  decltype(&get_procedures) procedures;
  decltype(&get_acquire_count) acquire_count;
};

SPSuitesSuite2 g_suite{
    reinterpret_cast<void*>(&unsupported_list_operation),
    reinterpret_cast<void*>(&unsupported_list_operation),
    &add_suite, &acquire_suite, &release_suite, &find_suite,
    reinterpret_cast<void*>(&unsupported_list_operation),
    reinterpret_cast<void*>(&unsupported_list_operation),
    reinterpret_cast<void*>(&unsupported_list_operation),
    &get_host, &get_name, &get_api_version, &get_internal_version,
    &get_procedures, &get_acquire_count};

}  // namespace

const void* sp_suites_suite2() noexcept { return &g_suite; }

SuiteResolveResult resolve(void*, const char* name, int32_t version,
                           const void** suite) noexcept {
  if (suite) *suite = nullptr;
  std::size_t ignored{};
  if (!suite || !valid_name(name, ignored) || version <= 0)
    return SuiteResolveResult::rejected_bad_param;
  std::lock_guard<std::mutex> lock(g_mutex);
  // SPBasicSuite has no internal-version argument. PICA resolves the highest
  // registered internal revision for the exact public name/version.
  Entry* selected = nullptr;
  for (auto& entry : g_entries) {
    if (!entry.occupied || entry.api_version != version ||
        std::strcmp(entry.name.data(), name) != 0)
      continue;
    if (!selected || entry.internal_version > selected->internal_version)
      selected = &entry;
  }
  if (!selected) return SuiteResolveResult::not_found;
  *suite = selected->procedures;
  return SuiteResolveResult::acquired;
}

int32_t retain(const char* name, int32_t version, const void* suite) noexcept {
  std::size_t ignored{};
  if (!valid_name(name, ignored) || version <= 0 || !suite)
    return kBadParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  Entry* selected = nullptr;
  for (auto& entry : g_entries) {
    if (!entry.occupied || entry.api_version != version ||
        entry.procedures != suite || std::strcmp(entry.name.data(), name) != 0)
      continue;
    if (!selected || entry.internal_version > selected->internal_version)
      selected = &entry;
  }
  if (!selected) return kSuiteNotFound;
  if (selected->references == UINT32_MAX) return kBadParameter;
  ++selected->references;
  return 0;
}

int32_t release(const char* name, int32_t version) noexcept {
  std::size_t ignored{};
  if (!valid_name(name, ignored) || version <= 0) return kBadParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  Entry* selected = nullptr;
  for (auto& entry : g_entries) {
    if (!entry.occupied || entry.api_version != version ||
        std::strcmp(entry.name.data(), name) != 0)
      continue;
    if (!selected || entry.internal_version > selected->internal_version)
      selected = &entry;
  }
  if (!selected) return kSuiteNotFound;
  if (selected->references == 0) return kSuiteAlreadyReleased;
  --selected->references;
  return 0;
}

RegistryStatistics statistics() noexcept {
  std::lock_guard<std::mutex> lock(g_mutex);
  RegistryStatistics result;
  for (const auto& entry : g_entries) {
    if (!entry.occupied) continue;
    ++result.registered;
    result.live_references += entry.references;
  }
  return result;
}

std::vector<RegisteredSuiteIdentity> registered_suites() {
  std::lock_guard<std::mutex> lock(g_mutex);
  std::vector<RegisteredSuiteIdentity> result;
  result.reserve(kMaximumSuites);
  for (const auto& entry : g_entries) {
    if (!entry.occupied) continue;
    result.push_back(
        {entry.name.data(), entry.api_version, entry.internal_version});
  }
  return result;
}

std::vector<RegisteredSuiteIdentity> observed_suites() {
  std::lock_guard<std::mutex> lock(g_mutex);
  return {g_observed.begin(), g_observed.begin() + g_observed_count};
}

bool references_drained() noexcept {
  std::lock_guard<std::mutex> lock(g_mutex);
  for (const auto& entry : g_entries)
    if (entry.occupied && entry.references != 0) return false;
  return true;
}

bool drain() noexcept {
  std::lock_guard<std::mutex> lock(g_mutex);
  for (const auto& entry : g_entries)
    if (entry.occupied && entry.references != 0) return false;
  g_entries = {};
  return true;
}

void reset_for_selftest() noexcept {
  std::lock_guard<std::mutex> lock(g_mutex);
  g_entries = {};
  g_observed = {};
  g_observed_count = 0;
}

}  // namespace aexcompat::worker_runtime::dynamic_suites
