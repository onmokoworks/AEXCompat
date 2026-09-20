#include <windows.h>

#include <cassert>
#include <cstdint>
#include <filesystem>
#include <iostream>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "runtime_module_path_cache.hpp"

namespace cache = aexcompat::worker_runtime::module_path_cache;

namespace {

struct FakeMetadata {
  std::wstring canonical_path;
  std::wstring canonical_parent;
  std::string basename;
};

using FakeCache = cache::GenerationMetadataCache<std::uintptr_t, FakeMetadata>;

FakeMetadata metadata(std::wstring path, std::wstring parent,
                      std::string basename) {
  return {std::move(path), std::move(parent), std::move(basename)};
}

void verify_stable_reuse_and_generation_invalidation() {
  FakeCache values(8);
  constexpr std::uintptr_t key = 0x1000;
  int resolutions = 0;
  const auto first = values.resolve(7, key, [&] {
    ++resolutions;
    return std::optional{
        metadata(L"C:\\plugins\\one.dll", L"C:\\plugins", "one.dll")};
  });
  const auto second = values.resolve(7, key, [&] {
    ++resolutions;
    return std::optional{metadata(L"wrong", L"wrong", "wrong.dll")};
  });
  assert(first && second);
  assert(first == second);
  assert(resolutions == 1);
  assert(values.size() == 1);
  assert(values.generation() == 7);

  // The same address in a new loader generation must not reuse the old DLL's
  // metadata: Windows may assign a recently freed image base to another DLL.
  const auto reused_address = values.resolve(8, key, [&] {
    ++resolutions;
    return std::optional{
        metadata(L"D:\\other\\two.dll", L"D:\\other", "two.dll")};
  });
  assert(reused_address);
  assert(reused_address != first);
  assert(reused_address->basename == "two.dll");
  assert(resolutions == 2);
  assert(values.size() == 1);
  assert(values.generation() == 8);
}

void verify_capacity_falls_back_without_growing() {
  FakeCache values(1);
  int uncached_resolutions = 0;
  assert(values.resolve(9, 0x4000, [] {
    return std::optional{metadata(L"C:\\first.dll", L"C:\\", "first.dll")};
  }));
  assert(values.size() == 1);

  const auto resolve_uncached = [&] {
    return values.resolve(9, 0x5000, [&] {
      ++uncached_resolutions;
      return std::optional{metadata(L"C:\\second.dll", L"C:\\", "second.dll")};
    });
  };
  assert(resolve_uncached());
  assert(resolve_uncached());
  assert(uncached_resolutions == 2);
  assert(values.size() == 1);
}

void verify_resolution_failure_is_not_cached() {
  FakeCache values(8);
  int resolutions = 0;
  const auto failed =
      values.resolve(4, 0x2000, [&]() -> std::optional<FakeMetadata> {
        ++resolutions;
        return std::nullopt;
      });
  assert(!failed);
  assert(values.size() == 0);

  const auto recovered = values.resolve(4, 0x2000, [&] {
    ++resolutions;
    return std::optional{
        metadata(L"C:\\recovered.dll", L"C:\\", "recovered.dll")};
  });
  assert(recovered);
  assert(recovered->basename == "recovered.dll");
  assert(resolutions == 2);
  assert(values.size() == 1);
}

class SequenceGenerationReader {
 public:
  explicit SequenceGenerationReader(std::vector<std::optional<uint64_t>> values)
      : values_(std::move(values)) {}

  std::optional<uint64_t> operator()() {
    assert(index_ < values_.size());
    return values_[index_++];
  }

 private:
  std::vector<std::optional<uint64_t>> values_;
  std::size_t index_{};
};

void verify_consistent_generation_capture() {
  SequenceGenerationReader one_change({10, 11, 11, 11});
  int captures = 0;
  const auto retried = cache::capture_consistent_generation(
      [&] { return one_change(); },
      [&](uint64_t generation) {
        ++captures;
        return static_cast<int>(generation * 10);
      });
  assert(retried.captured());
  assert(retried.attempts == 2);
  assert(retried.generation == 11);
  assert(*retried.value == 110);
  assert(captures == 2);

  SequenceGenerationReader repeated_change({20, 21, 22, 23});
  captures = 0;
  const auto rejected =
      cache::capture_consistent_generation([&] { return repeated_change(); },
                                           [&](uint64_t generation) {
                                             ++captures;
                                             return generation;
                                           });
  assert(!rejected.captured());
  assert(rejected.status ==
         cache::ConsistentCaptureStatus::generation_changed_twice);
  assert(rejected.attempts == 2);
  assert(!rejected.value);
  assert(captures == 2);

  SequenceGenerationReader unavailable({std::nullopt});
  const auto no_tracker = cache::capture_consistent_generation(
      [&] { return unavailable(); },
      [](uint64_t generation) { return generation; });
  assert(!no_tracker.captured());
  assert(no_tracker.status ==
         cache::ConsistentCaptureStatus::generation_unavailable);
  assert(no_tracker.attempts == 1);
}

void verify_mutable_checks_stay_outside_metadata_cache() {
  FakeCache values(8);
  int metadata_resolutions = 0;
  int classifications = 0;
  int reparse_checks = 0;
  int hash_checks = 0;
  std::wstring admitted_parent = L"C:\\plugins";
  bool reparse_safe = true;
  bool hash_matches = true;

  const auto audit = [&] {
    const auto path = values.resolve(30, 0x3000, [&] {
      ++metadata_resolutions;
      return std::optional{
          metadata(L"C:\\plugins\\effect.dll", L"C:\\plugins", "effect.dll")};
    });
    if (!path) return false;
    ++classifications;
    const bool classified = path->canonical_parent == admitted_parent;
    ++reparse_checks;
    const bool current_reparse_safe = reparse_safe;
    ++hash_checks;
    const bool current_hash_matches = hash_matches;
    return classified && current_reparse_safe && current_hash_matches;
  };

  assert(audit());
  admitted_parent = L"D:\\new-root";
  assert(!audit());
  admitted_parent = L"C:\\plugins";
  reparse_safe = false;
  assert(!audit());
  reparse_safe = true;
  hash_matches = false;
  assert(!audit());
  assert(metadata_resolutions == 1);
  assert(classifications == 4);
  assert(reparse_checks == 4);
  assert(hash_checks == 4);
}

void verify_real_loader_notifications(const std::filesystem::path& fixture) {
  const auto before = cache::current_loader_generation();
  assert(before);

  HMODULE module = LoadLibraryW(fixture.c_str());
  assert(module);
  const auto loaded = cache::current_loader_generation();
  assert(loaded && *loaded > *before);

  using FixtureValue = int (*)();
  const auto fixture_value = reinterpret_cast<FixtureValue>(
      GetProcAddress(module, "runtime_module_path_cache_fixture_value"));
  assert(fixture_value && fixture_value() == 37);

  assert(FreeLibrary(module));
  const auto unloaded = cache::current_loader_generation();
  assert(unloaded && *unloaded > *loaded);
}

}  // namespace

int wmain(int argc, wchar_t** argv) {
  assert(argc == 1 || argc == 2);
  verify_stable_reuse_and_generation_invalidation();
  verify_capacity_falls_back_without_growing();
  verify_resolution_failure_is_not_cached();
  verify_consistent_generation_capture();
  verify_mutable_checks_stay_outside_metadata_cache();

  const std::filesystem::path fixture =
      argc == 2 ? std::filesystem::path(argv[1])
                : std::filesystem::path(argv[0]).parent_path() /
                      L"runtime_module_path_cache_fixture.dll";
  assert(std::filesystem::is_regular_file(fixture));
  verify_real_loader_notifications(fixture);
  std::cout << "runtime_module_path_cache_selftest: passed\n";
  return 0;
}
