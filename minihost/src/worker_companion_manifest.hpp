#pragma once

#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

namespace aexcompat::worker_runtime::companions {

inline constexpr std::size_t kMaximumCompanions = 8;
inline constexpr std::size_t kMaximumDeclaredSuites = 32;
inline constexpr uint64_t kMaximumManifestBytes = 1024 * 1024;

struct SuiteIdentity {
  std::string name;
  int32_t api_version{};
  int32_t internal_version{};
};

struct Entry {
  std::filesystem::path path;
  std::string sha256;
  std::vector<SuiteIdentity> suites;
};

struct Manifest {
  std::filesystem::path manifest_path;
  std::vector<Entry> entries;
};

bool load_manifest(const std::filesystem::path& path, Manifest& result);

}  // namespace aexcompat::worker_runtime::companions

