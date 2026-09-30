#include "worker_companion_manifest.hpp"

#include <Windows.h>
#include <cassert>
#include <filesystem>
#include <fstream>
#include <string>
#include <system_error>

namespace {
std::filesystem::path temp_root() {
  wchar_t base[MAX_PATH]{};
  assert(GetTempPathW(MAX_PATH, base));
  const auto root = std::filesystem::path(base) /
      (L"aexcompat-companion-manifest-" + std::to_wstring(GetCurrentProcessId()));
  std::filesystem::create_directories(root);
  // GetTempPathW returns whatever TEMP holds, which on hosted CI runners is the
  // 8.3 short form (C:\Users\RUNNER~1\...). load_manifest requires a path whose
  // spelling already equals std::filesystem::canonical, so an un-canonicalised
  // fixture root fails the accept case below for a reason this test is not about
  // (#1467). Rejecting non-canonical spellings is what the alias case covers, and
  // that one builds its own alias explicitly.
  std::error_code error;
  const auto canonical = std::filesystem::canonical(root, error);
  assert(!error);
  return canonical;
}

void write(const std::filesystem::path& path, const std::string& text) {
  std::ofstream output(path, std::ios::binary | std::ios::trunc);
  output << text;
  assert(output.good());
}
}  // namespace

int main() {
  using namespace aexcompat::worker_runtime::companions;
  const auto root = temp_root();
  const auto companion = root / L"provider.aex";
  write(companion, "fixture");
  const auto manifest = root / L"companion-manifest-v1.json";
  const std::string prefix =
      "{\"schema\":\"companion-manifest-v1\",\"companions\":[{\"path\":\"" +
      companion.generic_string() +
      "\",\"sha256\":\"" + std::string(64, 'a') + "\",\"suites\":[";
  const std::string suite =
      "{\"name\":\"Fixture Suite\",\"api_version\":1,\"internal_version\":0}";
  write(manifest, prefix + suite + "]}]}");
  Manifest parsed;
  assert(load_manifest(manifest, parsed));
  assert(parsed.entries.size() == 1 && parsed.entries[0].suites.size() == 1);

  const auto alias_dir = root / L"alias";
  std::filesystem::create_directories(alias_dir);
  const auto noncanonical = alias_dir / L".." / L"provider.aex";
  write(manifest,
        "{\"schema\":\"companion-manifest-v1\",\"companions\":[{\"path\":\"" +
            noncanonical.generic_string() + "\",\"sha256\":\"" +
            std::string(64, 'a') + "\",\"suites\":[" + suite + "]}]}" );
  assert(!load_manifest(manifest, parsed));

  const auto record = [&](const std::filesystem::path& path, char sha,
                          const std::string& declared) {
    return "{\"path\":\"" + path.generic_string() +
        "\",\"sha256\":\"" + std::string(64, sha) +
        "\",\"suites\":[" + declared + "]}";
  };
  const auto verbatim = std::filesystem::path(L"\\\\?\\" + companion.wstring());
  const std::string other_suite =
      "{\"name\":\"Other Suite\",\"api_version\":1,\"internal_version\":0}";
  write(manifest,
        "{\"schema\":\"companion-manifest-v1\",\"companions\":[" +
            record(companion, 'a', suite) + "," +
            record(verbatim, 'b', other_suite) + "]}");
  assert(!load_manifest(manifest, parsed));

  write(manifest, prefix + suite + "," + suite + "]}]}");
  assert(!load_manifest(manifest, parsed));
  write(manifest, prefix +
                      "{\"name\":\"Fixture Suite\",\"api_version\":0,"
                      "\"internal_version\":0}] }]}");
  assert(!load_manifest(manifest, parsed));
  write(manifest,
        "{\"schema\":\"companion-manifest-v1\",\"companions\":[],"
        "\"unknown\":true}");
  assert(!load_manifest(manifest, parsed));
  std::filesystem::remove_all(root);
  return 0;
}
