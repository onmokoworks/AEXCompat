// SDK-free effect: all controls contribute to the pixel checksum, so a dropped
// or defaulted high slot is visible in both single and fresh-case batch renders.
#include "generated/aex_abi_contract.hpp"
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <cstdio>
namespace {
namespace abi = aexcompat::abi::x86_64_windows;
template<class T> T get(const void* p, std::size_t o) {
  T v{}; std::memcpy(&v, static_cast<const std::byte*>(p)+o, sizeof(v)); return v;
}
template<class T> void put(void* p, std::size_t o, T v) {
  std::memcpy(static_cast<std::byte*>(p)+o, &v, sizeof(v));
}
}
extern "C" __declspec(dllexport) int32_t __cdecl EffectMain(
    int32_t command, void* input, void* output, void** params, void* world, void*) {
  if (command == 4) {
    const auto add = get<int32_t(__cdecl*)(void*,int32_t,void*)>(input,abi::INTER_ADD_PARAM_OFFSET);
    for (int slot=1; slot<=MANY_PARAMETER_COUNT; ++slot) {
      std::array<std::byte,176> d{};
      put<int32_t>(d.data(),abi::PARAM_UU_OFFSET,slot);
      put<int32_t>(d.data(),abi::PARAM_PARAM_TYPE_OFFSET,1);
      std::snprintf(reinterpret_cast<char*>(d.data()+abi::PARAM_NAME_OFFSET),32,"Value %d",slot);
      put<int32_t>(d.data(),56+72,255);
      put<int32_t>(d.data(),56+80,255);
      put<int32_t>(d.data(),56+84,17);
      const auto error = add(get<void*>(input,abi::IN_EFFECT_REF_OFFSET),-1,d.data());
      if (error) return error;
    }
    put<int32_t>(output,abi::OUT_NUM_PARAMS_OFFSET,MANY_PARAMETER_COUNT+1);
  } else if (command == 11) {
    uint32_t sum=0;
    for (int slot=1; slot<=MANY_PARAMETER_COUNT; ++slot)
      sum += static_cast<uint32_t>(slot * get<int32_t>(params[slot],56));
    auto* bytes=get<unsigned char*>(world,abi::LAYER_DATA_OFFSET);
    const int w=get<int32_t>(world,abi::LAYER_WIDTH_OFFSET);
    const int h=get<int32_t>(world,abi::LAYER_HEIGHT_OFFSET);
    const int stride=get<int32_t>(world,abi::LAYER_ROWBYTES_OFFSET);
    for(int y=0;y<h;++y) for(int x=0;x<w;++x) {
      auto* p=bytes+y*stride+x*4;
      p[0]=255; p[1]=sum&255; p[2]=(sum>>8)&255; p[3]=(sum>>16)&255;
    }
  }
  return 0;
}
