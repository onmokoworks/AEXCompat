#include "worker_dynamic_suite_registry.hpp"
#include "worker_companion_runtime.hpp"
#include "worker_dynamic_suite_fixture_abi.hpp"
#include "worker_host_suite_catalog.hpp"

#include <Windows.h>
#include <cassert>
#include <cstdint>
#include <filesystem>
#include <string>

namespace {
using namespace aexcompat::worker_runtime::dynamic_suites;

struct SPSuitesSuite2 {
  void* allocate_list;
  void* free_list;
  int32_t(__cdecl* add)(void*, void*, const char*, int32_t, int32_t,
                        const void*, void**);
  int32_t(__cdecl* acquire)(void*, const char*, int32_t, int32_t,
                            const void**);
  int32_t(__cdecl* release)(void*, const char*, int32_t, int32_t);
};

FixtureBasicSuite g_basic{};

aexcompat::worker_runtime::SuiteResolveResult scene_resolve(
    void*, const char*, int32_t, const void**) {
  return aexcompat::worker_runtime::SuiteResolveResult::not_found;
}

int32_t __cdecl basic_acquire(const char* name, int32_t version,
                              const void** suite) {
  return aexcompat::worker_runtime::host_suites::acquire_catalog_suite(
      name, version, suite, nullptr);
}

int32_t __cdecl basic_release(const char* name, int32_t version) {
  return aexcompat::worker_runtime::host_suites::release_catalog_suite(
      name, version, nullptr);
}

bool fixture_sha256(const std::filesystem::path&, std::string& result) {
  result.assign(64, 'a');
  return true;
}

int32_t __cdecl fixture_callback(int32_t* value) {
  const void* nested{};
  assert(resolve(nullptr, "Fixture Companion Suite", 1, &nested) ==
         aexcompat::worker_runtime::SuiteResolveResult::acquired);
  assert(nested != nullptr);
  assert(retain("Fixture Companion Suite", 1, nested) == 0);
  assert(release("Fixture Companion Suite", 1) == 0);
  if (value) *value += 7;
  return value ? 0 : 4;
}
}  // namespace

int wmain(int argc, wchar_t** argv) {
  using namespace aexcompat::worker_runtime;
  using namespace aexcompat::worker_runtime::dynamic_suites;
  reset_for_selftest();
  const auto* suites = static_cast<const SPSuitesSuite2*>(sp_suites_suite2());
  assert(suites && suites->add && suites->acquire && suites->release);
  const int32_t gated_table = 1;
  const host_suites::StaticSuite static_suites[]{
      {"SP Suites Suite", 2, suites, nullptr, nullptr, nullptr, nullptr},
      {kFixtureGatedAegpSuiteName, 1, &gated_table, nullptr, nullptr,
       [](void*) { return companions::host_services_active(); }, nullptr}};
  assert(host_suites::configure_host_suite_catalog(
      {static_suites, 2, {&scene_resolve, nullptr}}));

  const std::filesystem::path directory =
      std::filesystem::path(argv[0]).parent_path();
  const auto consumer_path = directory / L"worker_dynamic_suite_consumer_fixture.dll";
  HMODULE consumer = LoadLibraryW(consumer_path.c_str());
  assert(consumer);
  using Consume = int32_t(__cdecl*)(const FixtureBasicSuite*, int32_t*);
  const auto consume = reinterpret_cast<Consume>(
      GetProcAddress(consumer, "FixturePfConsume"));
  assert(consume);
  g_basic = {&basic_acquire, &basic_release};
  int32_t boundary_value = 5;
  if (argc == 2 && std::wstring(argv[1]) == L"--without-companion") {
    assert(consume(&g_basic, &boundary_value) != 0);
    assert(boundary_value == 5);
    assert(FreeLibrary(consumer));
    return 0;
  }

  std::wstring child_command = L"\"" + std::filesystem::path(argv[0]).wstring() +
                               L"\" --without-companion";
  STARTUPINFOW startup{sizeof(startup)};
  PROCESS_INFORMATION process{};
  assert(CreateProcessW(nullptr, child_command.data(), nullptr, nullptr, FALSE,
                        0, nullptr, directory.c_str(), &startup, &process));
  WaitForSingleObject(process.hProcess, INFINITE);
  DWORD child_exit{};
  assert(GetExitCodeProcess(process.hProcess, &child_exit));
  CloseHandle(process.hThread);
  CloseHandle(process.hProcess);
  assert(child_exit == 0);

  const auto companion_path =
      directory / L"worker_dynamic_suite_companion_fixture.dll";
  HMODULE companion = LoadLibraryW(companion_path.c_str());
  assert(companion);
  using Initialize = int32_t(__cdecl*)(const FixtureSPSuitesSuite2*);
  using Death = int32_t(__cdecl*)();
  const auto initialize = reinterpret_cast<Initialize>(
      GetProcAddress(companion, "FixtureAegpInitialize"));
  const auto death = reinterpret_cast<Death>(
      GetProcAddress(companion, "FixtureAegpDeath"));
  assert(initialize && death);
  assert(initialize(static_cast<const FixtureSPSuitesSuite2*>(
             sp_suites_suite2())) == 0);
  assert(consume(&g_basic, &boundary_value) == 0);
  assert(boundary_value == 12);
  assert(statistics().live_references == 0);
  assert(death() == 0);
  assert(drain());
  assert(FreeLibrary(consumer));
  assert(FreeLibrary(companion));
  reset_for_selftest();

  consumer = LoadLibraryW(consumer_path.c_str());
  assert(consumer);
  const auto runtime_consume = reinterpret_cast<Consume>(
      GetProcAddress(consumer, "FixturePfConsume"));
  assert(runtime_consume);
  aexcompat::worker_runtime::companions::Manifest companion_manifest;
  aexcompat::worker_runtime::companions::Entry companion_entry;
  companion_entry.path = companion_path;
  companion_entry.sha256.assign(64, 'a');
  companion_entry.suites.push_back({"Fixture Companion Suite", 1, 0});
  companion_manifest.entries.push_back(std::move(companion_entry));
  aexcompat::worker_runtime::companions::Runtime companion_runtime;
  assert(companion_runtime.initialize(companion_manifest, &g_basic,
                                      &fixture_sha256));
  boundary_value = 9;
  assert(runtime_consume(&g_basic, &boundary_value) == 0);
  assert(boundary_value == 16);
  const void* runtime_lease{};
  assert(basic_acquire(kFixtureSuiteName, 1, &runtime_lease) == 0);
  assert(runtime_lease);
  assert(!companion_runtime.shutdown());
  assert(companion_runtime.active());
  assert(basic_release(kFixtureSuiteName, 1) == 0);
  assert(companion_runtime.shutdown());
  assert(statistics().registered == 0);
  const auto observed_after_death = observed_suites();
  assert(observed_after_death.size() == 1);
  assert(observed_after_death[0].name == kFixtureSuiteName);
  assert(FreeLibrary(consumer));

  void* registered{};
  void* table[]{reinterpret_cast<void*>(&fixture_callback)};
  assert(suites->add(nullptr, reinterpret_cast<void*>(1),
                     "Fixture Companion Suite", 1, 0, table,
                     &registered) == 0);
  assert(registered != nullptr);
  assert(suites->add(nullptr, nullptr, "Fixture Companion Suite", 1, 0,
                     table, nullptr) == kSuiteAlreadyExists);
  assert(suites->add(nullptr, nullptr, "", 1, 0, table, nullptr) ==
         kBadParameter);
  assert(suites->add(nullptr, nullptr, "Null", 1, 0, nullptr, nullptr) ==
         kBadParameter);
  const std::string overlong(kMaximumSuiteNameBytes + 1, 'x');
  assert(suites->add(nullptr, nullptr, overlong.c_str(), 1, 0, table,
                     nullptr) == kBadParameter);

  const void* acquired{};
  assert(resolve(nullptr, "Fixture Companion Suite", 1, &acquired) ==
         SuiteResolveResult::acquired);
  assert(acquired == table);
  assert(retain("Fixture Companion Suite", 1, acquired) == 0);
  int32_t value = 5;
  const auto callback = reinterpret_cast<int32_t(__cdecl*)(int32_t*)>(
      static_cast<void* const*>(const_cast<void*>(acquired))[0]);
  assert(callback(&value) == 0 && value == 12);
  assert(statistics().live_references == 1);
  assert(!drain());
  assert(release("Fixture Companion Suite", 1) == 0);
  assert(release("Fixture Companion Suite", 1) == kSuiteAlreadyReleased);
  assert(drain());
  assert(statistics().registered == 0);

  for (std::size_t index = 0; index < kMaximumSuites; ++index) {
    const std::string name = "Capacity Fixture Suite " +
                             std::to_string(index);
    assert(suites->add(nullptr, nullptr, name.c_str(), 1, 0, table,
                       nullptr) == 0);
  }
  assert(statistics().registered == kMaximumSuites);
  assert(suites->add(nullptr, nullptr, "Capacity Overflow Suite", 1, 0,
                     table, nullptr) == kBadParameter);
  assert(drain());

  acquired = reinterpret_cast<void*>(1);
  assert(resolve(nullptr, "Fixture Companion Suite", 1, &acquired) ==
         SuiteResolveResult::not_found);
  assert(acquired == nullptr);
  return 0;
}
