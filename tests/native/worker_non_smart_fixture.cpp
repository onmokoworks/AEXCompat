// SDK-free classic effect: GLOBAL_SETUP advertises unrelated bits, while
// PARAMS_SETUP overwrites flags. Dispatch must use the GLOBAL_SETUP snapshot.
#include "generated/aex_abi_contract.hpp"
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>

#ifndef AEXCOMPAT_FIXTURE_PARAMS_ERROR
#define AEXCOMPAT_FIXTURE_PARAMS_ERROR 0
#endif

namespace {
namespace abi = aexcompat::abi::x86_64_windows;
template<class T> T get(const void* p, std::size_t offset) {
  T value{};
  std::memcpy(&value, static_cast<const std::byte*>(p) + offset, sizeof(value));
  return value;
}
template<class T> void put(void* p, std::size_t offset, T value) {
  std::memcpy(static_cast<std::byte*>(p) + offset, &value, sizeof(value));
}
int sequence_calls{}, classic_calls{}, pre_calls{}, smart_calls{};
}

extern "C" __declspec(dllexport) int32_t __cdecl EffectMain(
    int32_t command, void*, void* output, void**, void* world, void*) {
  if (command == 1) {
    put<uint32_t>(output, abi::OUT_OUT_FLAGS2_OFFSET, 0x00800009u);
  } else if (command == 4) {
    put<int32_t>(output, abi::OUT_NUM_PARAMS_OFFSET, 1);
    put<uint32_t>(output, abi::OUT_OUT_FLAGS2_OFFSET, 1u << 10);
    return AEXCOMPAT_FIXTURE_PARAMS_ERROR;
  } else if (command == 5) {
    ++sequence_calls;
  } else if (command == 11) {
    ++classic_calls;
    auto* data = get<unsigned char*>(world, abi::LAYER_DATA_OFFSET);
    const auto width = get<int32_t>(world, abi::LAYER_WIDTH_OFFSET);
    const auto height = get<int32_t>(world, abi::LAYER_HEIGHT_OFFSET);
    const auto stride = get<int32_t>(world, abi::LAYER_ROWBYTES_OFFSET);
    for (int y = 0; y < height; ++y) for (int x = 0; x < width; ++x) {
      const unsigned char pixel[] = {255, 19, 37, 73};
      std::memcpy(data + y * stride + x * 4, pixel, 4);
    }
  } else if (command == 23) {
    ++pre_calls;
    return 123;
  } else if (command == 24) {
    ++smart_calls;
    return 124;
  } else if (command == 3) {
    std::fprintf(stderr, "fixture_dispatch_counts:%d,%d,%d,%d\n",
                 sequence_calls, classic_calls, pre_calls, smart_calls);
  }
  return 0;
}
