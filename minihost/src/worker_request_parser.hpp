#pragma once

#include "l2_cli_dispatch.h"

#include <array>
#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

namespace aexcompat::worker_runtime::request_parser {

enum class Kind { Render, Smart };

struct LayerInput {
  int32_t slot{};
  int32_t time{};
  uint32_t time_scale{1};
  bool timed{};
  int32_t width{};
  int32_t height{};
  std::vector<unsigned char> rgba;
  // Session transport only (#268): the inherited read HANDLE value carrying this
  // layer's RGBA8 file. Zero for the one-shot layered path, which loads `rgba`
  // directly from a file path. The session frame loop reads w*h*4 bytes from
  // this handle into `rgba` once at open.
  uint64_t rgba_handle{};
};

struct Hooks {
  aexcompat::l2cli::AuxiliaryOptionHooks auxiliary;
  bool (*parse_layer_key)(const wchar_t*, LayerInput&){};
  bool (*parse_mask_context)(const wchar_t*){};
  bool (*parse_spatial_context)(const wchar_t*){};
  bool (*parse_render_environment)(const wchar_t*){};
  void* parameter_context{};
  bool (*parse_parameters)(const wchar_t*, void*){};
  bool (*configure_mask_scene)(const std::string&){};
};

struct WorkerInvocation {
  aexcompat::l2cli::WorkerMode mode;
  int effective_argc{};
  std::vector<unsigned char> rgba;
  std::vector<LayerInput> layers;
  std::filesystem::path output;
  std::vector<float> audio;
  std::filesystem::path audio_output;
  int32_t width{};
  int32_t height{};
  int32_t current_time{};
  int32_t time_step{1};
  int32_t total_time{1};
  uint32_t time_scale{1};
  int32_t audio_samples{};
  int32_t audio_rate{};
  int32_t audio_session_max_samples{};
  int32_t audio_session_channels{1};
  int32_t click_x{};
  int32_t click_y{};
  std::array<float, 4> picker_color{};
};

struct ParseResult {
  WorkerInvocation invocation;
  int error{};  // 0 success, 2 unsupported command, 3 malformed/bounds.
};

ParseResult parse(Kind kind, int argc, wchar_t** argv, const Hooks& hooks);

}  // namespace aexcompat::worker_runtime::request_parser
