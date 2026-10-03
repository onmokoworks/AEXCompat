#pragma once

#include <filesystem>

namespace aexcompat::transport_path {

// A build target may be a directory junction. Permit aliases in the parent
// chain, but not a symlink/reparse alias for the file itself. Callers must also
// check the appropriate broker-owned canonical directory.
inline bool parent_alias_only(const std::filesystem::path& requested,
                              const std::filesystem::path& resolved,
                              const std::filesystem::path& absolute) {
  if (!requested.is_absolute() ||
      absolute.lexically_normal().filename() != resolved.filename())
    return false;
  std::error_code error;
  const auto parent = std::filesystem::canonical(
      absolute.lexically_normal().parent_path(), error);
  return !error && parent == resolved.parent_path();
}

}  // namespace aexcompat::transport_path
