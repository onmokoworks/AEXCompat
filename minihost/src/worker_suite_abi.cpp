#include "worker_suite_abi.hpp"

namespace aexcompat::suite_abi {
namespace {

AegpWorldSuite3 g_aegp_world_suite3{};

}  // namespace

AegpWorldSuite3& aegp_world_suite3_table() noexcept {
  return g_aegp_world_suite3;
}

}  // namespace aexcompat::suite_abi
