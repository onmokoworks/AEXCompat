#pragma once

// Env-gated diagnostics (AEXCOMPAT_EXTENDED_DIAG=1) for the extended-inter
// lookup protocol and host-callback traffic, added while diagnosing the
// issue #362 selector families. Off by default and read-only: with the
// variable unset every helper collapses to a single cached environment
// check.

#include <windows.h>

#include <cstddef>
#include <iostream>
#include <string>

namespace aexcompat::l2_detail {

inline bool extended_diag_enabled() {
  static const bool enabled = [] {
    wchar_t value[8]{};
    const DWORD length = GetEnvironmentVariableW(
        L"AEXCOMPAT_EXTENDED_DIAG", value, static_cast<DWORD>(std::size(value)));
    return length > 0 && value[0] == L'1';
  }();
  return enabled;
}

inline void diag_probe_arg(const char* name, const void* arg) {
  std::cerr << " " << name << "=" << arg;
  if (!arg) return;
  MEMORY_BASIC_INFORMATION info{};
  if (VirtualQuery(arg, &info, sizeof(info)) == 0 || info.State != MEM_COMMIT ||
      (info.Protect & (PAGE_READONLY | PAGE_READWRITE | PAGE_EXECUTE_READ |
                       PAGE_EXECUTE_READWRITE | PAGE_WRITECOPY |
                       PAGE_EXECUTE_WRITECOPY)) == 0) {
    std::cerr << "(unreadable)";
    return;
  }
  const auto* bytes = static_cast<const unsigned char*>(arg);
  std::string text;
  for (std::size_t i = 0; i < 64; ++i) {
    const unsigned char byte = bytes[i];
    if (byte == 0) break;
    if (byte < 0x20 || byte > 0x7e) {
      text.clear();
      break;
    }
    text.push_back(static_cast<char>(byte));
  }
  if (!text.empty()) std::cerr << "(\"" << text << "\")";
}

}  // namespace aexcompat::l2_detail
