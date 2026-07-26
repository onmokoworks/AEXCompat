#include "worker_openmp_policy.hpp"

#include <windows.h>

#include <cstdlib>

namespace aexcompat::worker_runtime::openmp {

bool install_deterministic_policy(std::string& diagnostic) noexcept {
  diagnostic.clear();
  if (_wputenv_s(kThreadCountVariable, kDeterministicThreadCount) != 0) {
    diagnostic = "failed to update CRT environment";
    return false;
  }

  wchar_t value[2]{};
  const DWORD length =
      GetEnvironmentVariableW(kThreadCountVariable, value, 2);
  if (length != 1 || value[0] != L'1') {
    diagnostic = "process environment did not retain OMP_NUM_THREADS=1";
    return false;
  }

  diagnostic = "OMP_NUM_THREADS=1";
  return true;
}

}  // namespace aexcompat::worker_runtime::openmp
