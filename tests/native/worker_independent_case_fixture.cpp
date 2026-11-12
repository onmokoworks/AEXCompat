// SDK-free behavioral fixture: pixels expose instance, parameter and global
// state, so accidentally retaining a sequence or resetting global setup fails.
#include "generated/aex_abi_contract.hpp"
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <stdexcept>
#include <windows.h>

namespace {
namespace abi = aexcompat::abi::x86_64_windows;
template <typename T> T get(const void* p, std::size_t offset) {
  T value{}; std::memcpy(&value, static_cast<const std::byte*>(p)+offset, sizeof(value)); return value;
}
template <typename T> void put(void* p, std::size_t offset, T value) {
  std::memcpy(static_cast<std::byte*>(p)+offset, &value, sizeof(value));
}
using Handle = void**;
struct Basic { int32_t(__cdecl* acquire)(const char*,int32_t,const void**);
               int32_t(__cdecl* release)(const char*,int32_t); };
struct Handles { Handle(__cdecl* create)(uint64_t); void*(__cdecl* lock)(Handle);
                 void(__cdecl* unlock)(Handle); void(__cdecl* dispose)(Handle); };
const Handles* handles{};
int global_setups{};
Handle declaration_default{};
}

extern "C" __declspec(dllexport) int32_t __cdecl EffectMain(
    int32_t command, void* input, void* output, void** params, void* world, void* extra) {
  const auto* basic = get<const Basic*>(input, abi::IN_PICA_BASICP_OFFSET);
  if (command == 1) {
    const void* suite{};
    if (basic->acquire("PF Handle Suite",2,&suite)) return 4;
    handles = static_cast<const Handles*>(suite);
    auto global = handles->create(sizeof(int));
    if (!global) return 4;
    *static_cast<int*>(handles->lock(global)) = ++global_setups;
    handles->unlock(global);
    put(output, abi::OUT_GLOBAL_DATA_OFFSET, global);
  } else if (command == 3) {
    handles->dispose(get<Handle>(input,abi::IN_GLOBAL_DATA_OFFSET));
    return basic->release("PF Handle Suite",2);
  } else if (command == 4) {
    std::array<std::byte,176> definition{};
    put<int32_t>(definition.data(),abi::PARAM_UU_OFFSET,1);
    put<int32_t>(definition.data(),abi::PARAM_PARAM_TYPE_OFFSET,1);
    std::memcpy(definition.data()+abi::PARAM_NAME_OFFSET,"Value",6);
    put<int32_t>(definition.data(),56+68,0);
    put<int32_t>(definition.data(),56+72,100);
    put<int32_t>(definition.data(),56+76,0);
    put<int32_t>(definition.data(),56+80,100);
    put<int32_t>(definition.data(),56+84,17);
    const auto add = get<int32_t(__cdecl*)(void*,int32_t,void*)>(input,abi::INTER_ADD_PARAM_OFFSET);
    const auto effect = get<void*>(input,abi::IN_EFFECT_REF_OFFSET);
    if (const auto error = add(effect,-1,definition.data())) return error;
    // Keep a declaration handle, as real plug-ins may do. Re-running PARAMS_SETUP
    // after disposing this source is invalid; instance values require ARB_COPY.
    if (!declaration_default) {
      declaration_default = handles->create(sizeof(int));
      *static_cast<int*>(handles->lock(declaration_default)) = 23;
      handles->unlock(declaration_default);
    }
    definition.fill(std::byte{});
    put<int32_t>(definition.data(),abi::PARAM_UU_OFFSET,2);
    put<int32_t>(definition.data(),abi::PARAM_PARAM_TYPE_OFFSET,11);
    std::memcpy(definition.data()+abi::PARAM_NAME_OFFSET,"Owned value",12);
    put<int16_t>(definition.data(),56,2);
    put<Handle>(definition.data(),64,declaration_default);
    put<int32_t>(output,abi::OUT_NUM_PARAMS_OFFSET,3);
    return add(effect,-1,definition.data());
  } else if (command == 22) {
    const auto operation = get<int32_t>(extra,0);
    if (operation == 1) { handles->dispose(get<Handle>(extra,16)); return 0; }
    if (operation == 2) {
      auto source = get<Handle>(extra,16);
      auto* value = static_cast<int*>(handles->lock(source));
      if (!value) return 4;
      auto copy = handles->create(sizeof(int));
      *static_cast<int*>(handles->lock(copy)) = *value;
      handles->unlock(copy); handles->unlock(source);
      *get<Handle*>(extra,24) = copy;
      return 0;
    }
    return 4; // no editable/printable arbitrary representation
  } else if (command == 5) {
    auto sequence = handles->create(sizeof(int));
    if (!sequence) return 4;
    *static_cast<int*>(handles->lock(sequence)) = 0;
    handles->unlock(sequence);
    put(output,abi::OUT_SEQUENCE_DATA_OFFSET,sequence);
  } else if (command == 8) {
    handles->dispose(get<Handle>(input,abi::IN_SEQUENCE_DATA_OFFSET));
    put<Handle>(output,abi::OUT_SEQUENCE_DATA_OFFSET,nullptr);
    if (get<int32_t>(input,224)==14) return 9;
  } else if (command == 11) {
    const auto sequence = get<Handle>(input,abi::IN_SEQUENCE_DATA_OFFSET);
    auto* state = static_cast<int*>(handles->lock(sequence));
    const auto ordinal = ++*state;
    handles->unlock(sequence);
    if (get<int32_t>(input,224)==13) return 7;
    if (get<int32_t>(input,224)==15) RaiseException(0xE0182601,0,0,nullptr);
    if (get<int32_t>(input,224)==16) Sleep(1000);
    if (get<int32_t>(input,224)==18) { (void)handles->create(4); return 7; }
    if (get<int32_t>(input,224)==19) throw std::runtime_error("independent case fixture");
    const auto arbitrary = get<Handle>(params[2],72);
    auto* owned = static_cast<int*>(handles->lock(arbitrary));
    if (!owned) return 4;
    const int drift = (*owned)++ - 23;
    handles->unlock(arbitrary);
    const auto global = get<Handle>(input,abi::IN_GLOBAL_DATA_OFFSET);
    const int global_count = *static_cast<int*>(handles->lock(global));
    handles->unlock(global);
    const int value = get<int32_t>(params[1],56);
    auto* bytes = get<unsigned char*>(world,abi::LAYER_DATA_OFFSET);
    const int width = get<int32_t>(world,abi::LAYER_WIDTH_OFFSET);
    const int height = get<int32_t>(world,abi::LAYER_HEIGHT_OFFSET);
    const int stride = get<int32_t>(world,abi::LAYER_ROWBYTES_OFFSET);
    for (int y=0;y<height;++y) for(int x=0;x<width;++x) {
      auto* pixel=bytes+y*stride+x*4;
      pixel[0]=255; pixel[1]=static_cast<unsigned char>(ordinal + drift);
      pixel[2]=static_cast<unsigned char>(value); pixel[3]=static_cast<unsigned char>(global_count);
    }
  }
  return 0;
}
