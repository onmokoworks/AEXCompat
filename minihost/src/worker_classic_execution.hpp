#pragma once
#include "worker_suite_abi.hpp"
#include <cstdint>
#include <string>
#include <vector>
namespace aexcompat::worker_runtime::classic_execution {
struct LifecycleHooks {
  void* (*begin)(void* host);
  // What the plug-in returned from its own SEQUENCE_SETUP / FRAME_SETUP. `begin`
  // hands back an opaque lifecycle, so without this accessor begin_lifecycle
  // cannot see a setup refusal and reports success (issue #725).
  int32_t (*setup_error)(void* lifecycle);
  bool (*click)(void* host);
  bool (*interpolate)(void* host);
  bool (*roundtrip)(void* host);
  bool (*conditional_ui)(void* host);
  bool (*draw)(void* host);
  int32_t (*end)(void* host, void* lifecycle, int32_t error);
  void (*dispose_lifecycle)(void* lifecycle);
};
struct LifecycleResult { void* lifecycle{}; int32_t error{}; };
LifecycleResult begin_lifecycle(void* host, const LifecycleHooks& hooks);
int32_t finish_lifecycle(void* host, LifecycleResult& state,
                         const LifecycleHooks& hooks, bool draw);
struct RenderHooks {
  bool (*draw)(void* host);
  int32_t (*prepare_output)(void* host);
  int32_t (*dispatch_selector)(void* host);
  bool (*close_ui)(void* host);
};
int32_t dispatch_render(void* host, int32_t error, const RenderHooks& hooks);
struct Hooks {
  bool (*copy_packed)(const unsigned char*, int32_t, int32_t, int32_t, int32_t,
                      std::vector<unsigned char>&);
  std::string (*hash)(const unsigned char*, std::size_t);
  bool (*publish_stage)(suite_abi::AegpTime, suite_abi::AegpTime, int8_t,
                        int32_t, int32_t, int32_t, const void*);
  void (*dump)(const void*, int32_t, int32_t, int32_t);
  void (*set_pixel_format)(const char*);
};
struct Context {
  unsigned char* destination{}; int32_t rowbytes{}, width{}, height{}, pixel_bytes{};
  int32_t error{}; int32_t current_time{}, time_step{}; uint32_t time_scale{1};
  int32_t quality{}; int32_t pixel_format{};
  std::string* output_hash{}; bool* guards_intact{};
  std::vector<unsigned char>* captured{};
  bool sentinels_intact{};
};
int finalize(Context& context, const Hooks& hooks);
}
