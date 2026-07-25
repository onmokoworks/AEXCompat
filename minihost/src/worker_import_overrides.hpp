#pragma once

#include <windows.h>

#include <string>

namespace aexcompat::worker_runtime::imports {

constexpr int kDeterministicOpenMpThreads = 1;

int __cdecl deterministic_omp_get_max_threads();

// Rewrites only the named vcomp140 import after Windows has admitted the
// module. A module without that import is accepted unchanged. A malformed
// import table or a failed page-protection transition fails closed.
bool install_deterministic_import_overrides(HMODULE module,
                                            std::string& diagnostic) noexcept;

}  // namespace aexcompat::worker_runtime::imports
