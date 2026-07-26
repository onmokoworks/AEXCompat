#pragma once

#include <string>

namespace aexcompat::worker_runtime::openmp {

inline constexpr wchar_t kThreadCountVariable[] = L"OMP_NUM_THREADS";
inline constexpr wchar_t kDeterministicThreadCount[] = L"1";

// Installs the process policy before any plug-in or vcomp runtime is loaded.
// This keeps omp_get_max_threads(), fork width, and thread IDs consistent.
bool install_deterministic_policy(std::string& diagnostic) noexcept;

}  // namespace aexcompat::worker_runtime::openmp
