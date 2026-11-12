// SDK-free defaults fixture. Every current value differs from its declaration.
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
    const int types[] = {5,1,2,10,7,4,3,6,18};
    for (int slot=1; slot<=9; ++slot) {
      std::array<std::byte,176> d{};
      put<int32_t>(d.data(),abi::PARAM_UU_OFFSET,slot);
      put<int32_t>(d.data(),abi::PARAM_PARAM_TYPE_OFFSET,types[slot-1]);
      std::snprintf(reinterpret_cast<char*>(d.data()+abi::PARAM_NAME_OFFSET),32,"Default %d",slot);
      if (slot == 1) {
        const unsigned char current[]={255,200,201,202}, declared[]={255,19,37,73};
        std::memcpy(d.data()+56,current,4); std::memcpy(d.data()+60,declared,4);
      } else if (slot == 2 || slot == 3) {
        const int unit=slot==2 ? 1 : 65536;
        put<int32_t>(d.data(),56,99*unit);
        put<int32_t>(d.data(),128,100*unit); put<int32_t>(d.data(),136,100*unit);
        put<int32_t>(d.data(),140,slot==2 ? 17 : 80960); // 1.2353515625, not display-rounded
        put<int16_t>(d.data(),144,1);
      } else if (slot == 4) {
        put<double>(d.data(),56,99.0);
        put<float>(d.data(),108,100.f); put<float>(d.data(),116,100.f);
        put<float>(d.data(),120,1.234567f); put<int16_t>(d.data(),124,1);
      } else if (slot == 5) {
        put<int32_t>(d.data(),56,3); put<int16_t>(d.data(),60,3); put<int16_t>(d.data(),62,2);
        put<const char*>(d.data(),64,"One|Two|Three");
      } else if (slot == 6) {
        put<int32_t>(d.data(),56,0); put<uint8_t>(d.data(),60,1);
      } else if (slot == 7) {
        put<int32_t>(d.data(),56,65536); put<int32_t>(d.data(),60,45*65536);
      } else if (slot == 8) {
        put<int32_t>(d.data(),56,65536); put<int32_t>(d.data(),60,65536);
        put<int32_t>(d.data(),68,50*65536); put<int32_t>(d.data(),72,25*65536);
      } else {
        for (int c=0;c<3;++c) { put<double>(d.data(),56+c*8,99.); put<double>(d.data(),80+c*8,25.*(c+1)); }
      }
      if (auto error=add(get<void*>(input,abi::IN_EFFECT_REF_OFFSET),-1,d.data())) return error;
    }
    put<int32_t>(output,abi::OUT_NUM_PARAMS_OFFSET,10);
  } else if (command == 11) {
    // 4x8 input: point percentages materialize to pixel values. Verify every
    // omitted scalar/component kind, including precision-preserving floats.
    if (get<int32_t>(params[3],56)!=80960 || get<double>(params[4],56)!=double(1.234567f) ||
        get<int32_t>(params[5],56)!=2 || get<int32_t>(params[6],56)!=1 ||
        get<int32_t>(params[7],56)!=45*65536 ||
        get<int32_t>(params[8],56)!=2*65536 || get<int32_t>(params[8],60)!=2*65536 ||
        get<double>(params[9],56)!=1. || get<double>(params[9],64)!=4. || get<double>(params[9],72)!=6.) return 4;
    const auto* color=static_cast<const unsigned char*>(params[1])+56;
    const int delta=get<int32_t>(params[2],56)-17;
    auto* bytes=get<unsigned char*>(world,abi::LAYER_DATA_OFFSET);
    const int w=get<int32_t>(world,abi::LAYER_WIDTH_OFFSET),h=get<int32_t>(world,abi::LAYER_HEIGHT_OFFSET);
    const int stride=get<int32_t>(world,abi::LAYER_ROWBYTES_OFFSET);
    for(int y=0;y<h;++y) for(int x=0;x<w;++x) {
      auto* p=bytes+y*stride+x*4; p[0]=color[0]; p[1]=color[1]+delta; p[2]=color[2]; p[3]=color[3];
    }
  }
  return 0;
}
