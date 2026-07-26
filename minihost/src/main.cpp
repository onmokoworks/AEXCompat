#include <windows.h>
#include <bcrypt.h>

#include "worker_openmp_policy.hpp"

#include <array>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <string>
#include <string_view>
#include <vector>

namespace {
constexpr int kUsage = 2;
constexpr int kIdentityMismatch = 10;
constexpr int kLoadFailed = 11;
constexpr int kEntrypointMissing = 12;
constexpr int kInternalError = 13;

std::string hex(const std::array<unsigned char, 32>& digest) {
  std::ostringstream out;
  out << std::hex << std::setfill('0');
  for (const auto byte : digest) out << std::setw(2) << static_cast<unsigned>(byte);
  return out.str();
}

bool sha256(const std::filesystem::path& path,
            std::array<unsigned char, 32>& digest) {
  BCRYPT_ALG_HANDLE algorithm = nullptr;
  BCRYPT_HASH_HANDLE hash = nullptr;
  DWORD object_size = 0;
  DWORD returned = 0;
  if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr,
                                  0) < 0 ||
      BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                        reinterpret_cast<PUCHAR>(&object_size),
                        sizeof(object_size), &returned, 0) < 0) {
    if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0);
    return false;
  }
  std::vector<unsigned char> object(object_size);
  if (BCryptCreateHash(algorithm, &hash, object.data(), object_size, nullptr, 0,
                       0) < 0) {
    BCryptCloseAlgorithmProvider(algorithm, 0);
    return false;
  }
  std::ifstream input(path, std::ios::binary);
  std::array<unsigned char, 64 * 1024> buffer{};
  while (input) {
    input.read(reinterpret_cast<char*>(buffer.data()), buffer.size());
    const auto count = input.gcount();
    if (count > 0 && BCryptHashData(hash, buffer.data(),
                                    static_cast<ULONG>(count), 0) < 0) {
      BCryptDestroyHash(hash);
      BCryptCloseAlgorithmProvider(algorithm, 0);
      return false;
    }
  }
  const bool ok = input.eof() &&
                  BCryptFinishHash(hash, digest.data(), digest.size(), 0) >= 0;
  BCryptDestroyHash(hash);
  BCryptCloseAlgorithmProvider(algorithm, 0);
  return ok;
}

void report(const char* status, bool identity, bool loaded, bool entrypoint,
            DWORD error = 0) {
  std::cout << "{\"schema_version\":1,\"stage\":\"L1\",\"status\":\""
            << status << "\",\"identity_verified\":"
            << (identity ? "true" : "false")
            << ",\"module_loaded\":" << (loaded ? "true" : "false")
            << ",\"entrypoint_resolved\":" << (entrypoint ? "true" : "false")
            << ",\"selectors_executed\":false,\"render_performed\":false"
            << ",\"win32_error\":" << error << "}\n";
}
}  // namespace

int wmain(int argc, wchar_t** argv) {
  std::string openmp_diagnostic;
  if (!aexcompat::worker_runtime::openmp::install_deterministic_policy(
          openmp_diagnostic)) {
    report("openmp_policy_failed", false, false, false);
    return kInternalError;
  }
  if (argc != 4 || std::wstring_view(argv[1]) != L"--l1") {
    report("invalid_request", false, false, false);
    return kUsage;
  }
  const std::filesystem::path candidate(argv[2]);
  std::string expected;
  for (const wchar_t* cursor = argv[3]; *cursor; ++cursor) {
    if (*cursor > 0x7f) {
      report("invalid_request", false, false, false);
      return kUsage;
    }
    expected.push_back(static_cast<char>(*cursor));
  }
  std::array<unsigned char, 32> digest{};
  std::error_code ec;
  if (!std::filesystem::is_regular_file(candidate, ec) || ec ||
      !sha256(candidate, digest)) {
    report("identity_read_failed", false, false, false);
    return kInternalError;
  }
  if (hex(digest) != expected) {
    report("identity_mismatch", false, false, false);
    return kIdentityMismatch;
  }

  if (!SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_USER_DIRS)) {
    report("dll_policy_failed", true, false, false, GetLastError());
    return kInternalError;
  }
  const HMODULE module = LoadLibraryExW(
      candidate.c_str(), nullptr,
      LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (!module) {
    report("load_failed", true, false, false, GetLastError());
    return kLoadFailed;
  }
  SetLastError(ERROR_SUCCESS);
  const FARPROC effect_main = GetProcAddress(module, "EffectMain");
  const DWORD effect_main_error = GetLastError();
  SetLastError(ERROR_SUCCESS);
  const FARPROC entry_point_func = GetProcAddress(module, "EntryPointFunc");
  const DWORD entry_point_func_error = GetLastError();
  const bool entrypoint = effect_main != nullptr || entry_point_func != nullptr;
  if (!entrypoint) {
    FreeLibrary(module);
    const DWORD error = entry_point_func_error != ERROR_SUCCESS
                            ? entry_point_func_error
                            : (effect_main_error != ERROR_SUCCESS
                                   ? effect_main_error
                                   : ERROR_PROC_NOT_FOUND);
    report("entrypoint_missing", true, true, false, error);
    return kEntrypointMissing;
  }
  FreeLibrary(module);
  report("loaded_and_unloaded", true, true, true);
  return 0;
}
