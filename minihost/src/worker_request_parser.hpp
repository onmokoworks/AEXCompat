#pragma once

#include "l2_cli_dispatch.h"

#include <array>
#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

namespace aexcompat::worker_runtime::request_parser {

enum class Kind { Classic, Smart };

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
  // this handle into `rgba` once at open - or before every frame when the layer
  // is `dynamic`.
  uint64_t rgba_handle{};
  // The broker rewrites this layer's file between frames (issue #674: AviUtl2's
  // virtual buffer as an animated displacement map). Its handle is kept open and
  // re-read before each frame instead of being consumed at open. Geometry is
  // still fixed at open, so only the bytes may change.
  bool dynamic{};
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
  std::vector<LayerInput> layers;
  std::vector<float> audio;
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
};

struct ParseResult {
  WorkerInvocation invocation;
  int error{};  // 0 success, 2 unsupported command, 3 malformed/bounds.
};

ParseResult parse(Kind kind, int argc, wchar_t** argv, const Hooks& hooks);

}  // namespace aexcompat::worker_runtime::request_parser
