#pragma once
#include "worker_suite_abi.hpp"
#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>
namespace aexcompat::worker_runtime::classic_execution {
struct Hooks {
  bool (*copy_packed)(const unsigned char*, int32_t, int32_t, int32_t, int32_t,
                      std::vector<unsigned char>&);
  std::string (*hash)(const unsigned char*, std::size_t);
  bool (*publish_stage)(suite_abi::AegpTime, suite_abi::AegpTime, int8_t,
                        int32_t, int32_t, int32_t, const void*);
  void (*dump)(const void*, int32_t, int32_t, int32_t);
  void (*argb_to_rgba)(unsigned char*, const unsigned char*, int32_t);
  void (*checksum)(const unsigned char*, int32_t, int32_t, int32_t);
  void (*set_pixel_format)(const char*);
};
struct Context {
  unsigned char* destination{}; int32_t rowbytes{}, width{}, height{}, pixel_bytes{};
  int32_t error{}; int32_t current_time{}, time_step{}; uint32_t time_scale{1};
  int32_t quality{}; int32_t pixel_format{};
  std::string* output_hash{}; bool* guards_intact{};
  std::vector<unsigned char>* captured{};
  const std::filesystem::path* external_output{};
  bool sentinels_intact{};
};
int finalize(Context& context, const Hooks& hooks);
}
