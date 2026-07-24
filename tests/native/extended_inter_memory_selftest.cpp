#include "extended_inter_memory.hpp"

#include <cstdint>
#include <cstdio>

int main() {
  using aexcompat::extended_inter::allocate;
  using aexcompat::extended_inter::release;

  void* zero = reinterpret_cast<void*>(static_cast<std::uintptr_t>(0x1));
  const bool zero_allocated = allocate(&zero, 0) == 0 && zero != nullptr;
  const bool zero_released = release(&zero) == 0 && zero == nullptr;

  void* positive = nullptr;
  const bool positive_allocated = allocate(&positive, 8) == 0 && positive != nullptr;
  const bool positive_released = release(&positive) == 0 && positive == nullptr;

  void* oversize = reinterpret_cast<void*>(static_cast<std::uintptr_t>(0x2));
  const bool oversize_rejected = allocate(&oversize, (std::size_t{1} << 24) + 1) != 0 &&
      oversize == nullptr;

  int foreign_storage = 0;
  void* foreign = &foreign_storage;
  const bool foreign_rejected = release(&foreign) != 0 && foreign == nullptr;

  void* null_out = nullptr;
  const bool null_release = release(&null_out) == 0;
  const bool null_argument = release(nullptr) == 0 && allocate(nullptr, 0) != 0;

  const bool passed = zero_allocated && zero_released && positive_allocated &&
      positive_released && oversize_rejected && foreign_rejected &&
      null_release && null_argument;
  std::printf(
      "{\"extended_inter_memory\":\"%s\",\"zero_size\":%s,"
      "\"foreign_free_rejected\":%s,\"failed_alloc_clears\":%s}\n",
      passed ? "passed" : "failed", zero_allocated && zero_released ? "true" : "false",
      foreign_rejected ? "true" : "false", oversize_rejected ? "true" : "false");
  return passed ? 0 : 1;
}
