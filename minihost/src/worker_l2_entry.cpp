#include "worker_target.hpp"

int wmain(int argc, wchar_t** argv) {
  return aexcompat::worker_target::run(aexcompat::worker_target::Kind::L2, argc, argv);
}
