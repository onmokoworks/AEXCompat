#include "worker_target.hpp"

#include <cstdio>
#include <cwchar>
#include <vector>

namespace {

// "This process was never told which route to serve." Distinct from every code
// the runtime returns because the runtime does not start: nothing has been
// loaded, dispatched or reported when this fires, so it cannot be confused
// with a plug-in outcome.
constexpr int kUnknownKindExitCode = 90;

int refuse(const wchar_t* detail) {
  std::fwprintf(stderr,
                L"aex_worker: %ls\n"
                L"usage: aex_worker --kind <discovery|classic|smart> <worker argv...>\n",
                detail);
  return kUnknownKindExitCode;
}

}  // namespace

int wmain(int argc, wchar_t** argv) {
  if (argc < 3 || !argv) return refuse(L"missing --kind");
  if (std::wcscmp(argv[1], L"--kind") != 0) {
    return refuse(L"first argument must be --kind");
  }
  aexcompat::worker_target::Kind kind;
  if (!aexcompat::worker_target::parse_kind(argv[2], kind)) {
    return refuse(L"unknown --kind value");
  }

  // Hand the runtime the vector it would have seen before the pair existed, so
  // the positional contract (argv[1] command, argv[2] plug-in, argv[3] sha256,
  // ...) and the auxiliary options stripped from the tail keep their indices.
  std::vector<wchar_t*> forwarded;
  forwarded.reserve(static_cast<std::size_t>(argc) - 2);
  forwarded.push_back(argv[0]);
  for (int index = 3; index < argc; ++index) forwarded.push_back(argv[index]);
  return aexcompat::worker_target::run(
      kind, static_cast<int>(forwarded.size()), forwarded.data());
}
