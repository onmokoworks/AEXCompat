// SDK-free behavioral fixture: pixels expose instance, parameter and global
// state, so accidentally retaining a sequence or resetting global setup fails.
#include "generated/aex_abi_contract.hpp"
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <cstdio>
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

int32_t scene_case(const Basic* basic, void* effect_ref, bool balanced) {
  const void* pf{}; const void* effects{};
  if (basic->acquire("AEGP PF Interface Suite",1,&pf) ||
      basic->acquire("AEGP Effect Suite",4,&effects)) return 101;
  auto p = static_cast<void* const*>(pf);
  auto e = static_cast<void* const*>(effects);
  void* effect{};
  auto error = reinterpret_cast<int32_t(__cdecl*)(int32_t,void*,void**)>(p[1])(7,effect_ref,&effect);
  if (!error && balanced)
    error = reinterpret_cast<int32_t(__cdecl*)(void*)>(e[8])(effect);
  basic->release("AEGP Effect Suite",4);
  basic->release("AEGP PF Interface Suite",1);
  return error ? 100 + error : 0;
}
}

#ifdef INDEPENDENT_SMART_FIXTURE
// Availability alone enables the ordinary-session Premiere retry. Decline the
// GPU route so its second PF selector invocation returns a different error.
extern "C" __declspec(dllexport) int32_t __cdecl xGPUFilterEntry() { return 1; }
#endif

extern "C" __declspec(dllexport) int32_t __cdecl EffectMain(
    int32_t command, void* input, void* output, void** params, void* world, void* extra) {
  const auto* basic = get<const Basic*>(input, abi::IN_PICA_BASICP_OFFSET);
  if (command == 1) {
#ifdef INDEPENDENT_SMART_FIXTURE
    put<uint32_t>(output,abi::OUT_OUT_FLAGS2_OFFSET,(1u << 10) | (1u << 25));
#endif
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
  } else if (command == 23) {
    const auto pre_input = get<void*>(extra,abi::PRE_EXTRA_INPUT_OFFSET);
    const auto pre_output = get<void*>(extra,abi::PRE_EXTRA_OUTPUT_OFFSET);
    std::memcpy(static_cast<std::byte*>(pre_output)+abi::PRE_OUTPUT_RESULT_RECT_OFFSET,pre_input,16);
    std::memcpy(static_cast<std::byte*>(pre_output)+abi::PRE_OUTPUT_MAX_RESULT_RECT_OFFSET,pre_input,16);
  } else if (command == 24 || command == 31) {
    const auto sequence = get<Handle>(input,abi::IN_SEQUENCE_DATA_OFFSET);
    auto* state = static_cast<int*>(handles->lock(sequence));
    const auto ordinal = ++*state;
    handles->unlock(sequence);
    const auto time = get<int32_t>(input,224);
    char call_name[64]{};
    std::snprintf(call_name,sizeof(call_name),"Independent smart selector %d",ordinal);
    const void* unused{};
    basic->acquire(call_name,1,&unused);
    return ordinal == 1 ? (time == 22 ? 512 : time == 23 ? 516 : 14) : 7;
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
    if (get<int32_t>(input,224)==20) {
      const void* memory{};
      if (basic->acquire("AEGP Memory Suite",1,&memory)) return 4;
      using NewMemory = int32_t(__cdecl*)(int32_t,const char*,uint32_t,int32_t,void**);
      void* leaked{};
      const auto error = (*static_cast<const NewMemory*>(memory))(0,"case leak",4,0,&leaked);
      basic->release("AEGP Memory Suite",1);
      return error ? error : 7;
    }
    if (get<int32_t>(input,224)==21) {
      const void* suite{};
      basic->acquire("Independent case missing suite",1,&suite);
      if (!basic->acquire("AE Timecode Helper Suite",1,&suite)) {
        (*static_cast<int32_t(__cdecl* const*)()>(suite))();
        basic->release("AE Timecode Helper Suite",1);
      }
      return 7;
    }
    const auto time = get<int32_t>(input,224);
    if (time >= 24 && time <= 26) {
      const void* query{};
      const void* data{};
      if (basic->acquire("PF Path Query Suite",1,&query)) return 4;
      if (basic->acquire("PF Path Data Suite",1,&data)) return 4;
      auto q = static_cast<void* const*>(query);
      auto d = static_cast<void* const*>(data);
      auto effect = get<void*>(input,abi::IN_EFFECT_REF_OFFSET);
      int32_t id{};
      auto error = reinterpret_cast<int32_t(__cdecl*)(void*,int32_t,int32_t*)>(q[1])(effect,0,&id);
      void* path{};
      if (!error) error = reinterpret_cast<int32_t(__cdecl*)(void*,int32_t,int32_t,int32_t,uint32_t,void**)>(q[2])(effect,id,0,1,30,&path);
      void* prep{};
      if (!error && time != 24) error = reinterpret_cast<int32_t(__cdecl*)(void*,void*,int32_t,int32_t,void**)>(d[3])(effect,path,0,4,&prep);
      if (!error && time == 26) error = reinterpret_cast<int32_t(__cdecl*)(void*,void*,int32_t,void**)>(d[7])(effect,path,0,&prep);
      if (!error && time != 24) error = reinterpret_cast<int32_t(__cdecl*)(void*,int32_t,int32_t,void*)>(q[3])(effect,id,0,path);
      basic->release("PF Path Data Suite",1);
      basic->release("PF Path Query Suite",1);
      return error ? error : 7;
    }
    if (time == 27 || time == 28) {
      const auto error = scene_case(basic,get<void*>(input,abi::IN_EFFECT_REF_OFFSET),
          time == 28);
      if (error) return error;
    }
    if (time == 29) {
      const auto checkout = get<int32_t(__cdecl*)(void*,int32_t,int32_t,int32_t,uint32_t,uint32_t,int32_t,int32_t,int32_t,void**)>(input,abi::INTER_CHECKOUT_LAYER_AUDIO_OFFSET);
      void* audio{};
      const auto error = checkout(get<void*>(input,abi::IN_EFFECT_REF_OFFSET),0,0,1,30,44100u<<16,2,1,1,&audio);
      if (error || !audio) return 102;
      // Classic render cleanup owns the automatic check-in.
    }
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
