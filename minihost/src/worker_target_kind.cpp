#include "worker_target.hpp"

#include <cwchar>

namespace aexcompat::worker_target {

bool parse_kind(const wchar_t* value, Kind& kind) {
  if (!value) return false;
  if (std::wcscmp(value, L"discovery") == 0) {
    kind = Kind::Discovery;
    return true;
  }
  if (std::wcscmp(value, L"classic") == 0) {
    kind = Kind::Classic;
    return true;
  }
  if (std::wcscmp(value, L"smart") == 0) {
    kind = Kind::Smart;
    return true;
  }
  return false;
}

}  // namespace aexcompat::worker_target
