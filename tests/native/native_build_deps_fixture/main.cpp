#include "shared.hpp"
#include <iostream>
int main() {
  std::cout << kVersion << ' ' << peer_version() << ' '
            << sizeof(Packet) << ' ' << peer_size() << '\n';
  return kVersion == peer_version() && sizeof(Packet) == peer_size() ? 0 : 1;
}
