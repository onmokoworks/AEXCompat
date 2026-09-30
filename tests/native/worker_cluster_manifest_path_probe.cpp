#include "worker_cluster_manifest.hpp"

#include <iostream>

int wmain(int argc, wchar_t** argv) {
  if (argc != 2) return 64;
  aexcompat::worker_runtime::cluster::Manifest manifest;
  if (!aexcompat::worker_runtime::cluster::load_manifest(argv[1], manifest))
    return 3;
  std::cout << "{\"plugins\":" << manifest.plugins.size()
            << ",\"search_dirs\":" << manifest.search_dirs.size()
            << ",\"module_bound\":" << manifest.module_bound << "}\n";
  return 0;
}
