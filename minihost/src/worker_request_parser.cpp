#include "worker_request_parser.hpp"

#include <algorithm>
#include <cmath>
#include <fstream>
#include <string>

namespace aexcompat::worker_runtime::request_parser {
namespace {

bool same_time(const LayerInput& left, const LayerInput& right) {
  return left.time_scale != 0 && right.time_scale != 0 &&
      static_cast<int64_t>(left.time) * right.time_scale ==
          static_cast<int64_t>(right.time) * left.time_scale;
}

bool load_rgba(const wchar_t* path, int32_t width, int32_t height,
               std::vector<unsigned char>& output) {
  if (width <= 0 || height <= 0 || width > 4096 || height > 4096) return false;
  const auto bytes = static_cast<std::size_t>(width) * height * 4;
  std::ifstream file(path, std::ios::binary | std::ios::ate);
  if (!file || static_cast<std::size_t>(file.tellg()) != bytes) return false;
  output.resize(bytes); file.seekg(0);
  return static_cast<bool>(file.read(reinterpret_cast<char*>(output.data()), bytes));
}

bool load_audio(const wchar_t* path, int32_t samples, int32_t rate,
                std::vector<float>& output) {
  if (samples <= 0 || samples > 10'000'000 || rate != 44100) return false;
  const auto bytes = static_cast<std::size_t>(samples) * sizeof(float);
  std::ifstream file(path, std::ios::binary | std::ios::ate);
  if (!file || static_cast<std::size_t>(file.tellg()) != bytes) return false;
  output.resize(static_cast<std::size_t>(samples) + 1, 0.0f); file.seekg(0);
  return file.read(reinterpret_cast<char*>(output.data()), bytes) &&
      std::none_of(output.begin(), output.end() - 1,
                   [](float value) { return !std::isfinite(value); });
}

}  // namespace

ParseResult parse(Kind kind, int argc, wchar_t** argv, const Hooks& hooks) {
  ParseResult result;
  const auto auxiliary = aexcompat::l2cli::strip_auxiliary_options(
      argc, argv, hooks.auxiliary);
  if (!auxiliary.accepted) { result.error = 3; return result; }
  result.invocation.effective_argc = auxiliary.effective_argc;
  result.invocation.mode = aexcompat::l2cli::classify_worker_mode(
      kind == Kind::Render ? aexcompat::l2cli::WorkerKind::Render :
                             aexcompat::l2cli::WorkerKind::Smart,
      argc, argv, auxiliary.effective_argc);
  const auto& mode = result.invocation.mode;
  if (!mode.command_accepted) { result.error = 2; return result; }
  if (mode.request_mode && (!hooks.parse_parameters ||
      !hooks.parse_parameters(argv[4], hooks.parameter_context))) {
    result.error = 3;
    return result;
  }
  try {
    if (mode.audio_mode) {
      result.invocation.audio_samples = std::stoi(argv[7]);
      result.invocation.audio_rate = std::stoi(argv[8]);
      if (!load_audio(argv[5], result.invocation.audio_samples,
                      result.invocation.audio_rate, result.invocation.audio)) throw 1;
      result.invocation.audio_output = argv[6];
      if (std::filesystem::exists(result.invocation.audio_output)) throw 1;
    }
    if (mode.image_audio_mode) {
      result.invocation.audio_samples = std::stoi(argv[14]);
      result.invocation.audio_rate = std::stoi(argv[15]);
      if (!load_audio(argv[13], result.invocation.audio_samples,
                      result.invocation.audio_rate, result.invocation.audio)) throw 1;
    }
    if (mode.render_session_mode) {
      auto& invocation = result.invocation;
      invocation.width = std::stoi(argv[5]); invocation.height = std::stoi(argv[6]);
      invocation.time_step = std::stoi(argv[7]);
      invocation.total_time = std::stoi(argv[8]);
      invocation.time_scale = std::stoul(argv[9]);
      if (invocation.width <= 0 || invocation.height <= 0 ||
          invocation.width > 4096 || invocation.height > 4096 ||
          invocation.time_step <= 0 || invocation.total_time <= 0 ||
          invocation.time_scale == 0) throw 1;
    }
    if (mode.image_mode) {
      auto& invocation = result.invocation;
      invocation.width = std::stoi(argv[7]); invocation.height = std::stoi(argv[8]);
      if (!load_rgba(argv[5], invocation.width, invocation.height, invocation.rgba)) throw 1;
      invocation.output = argv[6];
      if (std::filesystem::exists(invocation.output)) throw 1;
      invocation.current_time = std::stoi(argv[9]); invocation.time_step = std::stoi(argv[10]);
      invocation.total_time = std::stoi(argv[11]); invocation.time_scale = std::stoul(argv[12]);
      if (invocation.current_time < 0 || invocation.time_step <= 0 ||
          invocation.total_time < invocation.current_time || invocation.time_scale == 0) throw 1;
      if (mode.layered_image_mode) for (int argument = 13; argument < mode.image_argc; argument += 4) {
        LayerInput layer;
        if (!hooks.parse_layer_key || !hooks.parse_layer_key(argv[argument], layer)) throw 1;
        layer.width = std::stoi(argv[argument + 2]); layer.height = std::stoi(argv[argument + 3]);
        if (layer.slot <= 0 || layer.slot > 1024 || layer.width <= 0 || layer.height <= 0 ||
            layer.width > 4096 || layer.height > 4096 ||
            std::any_of(invocation.layers.begin(), invocation.layers.end(), [&](const auto& existing) {
              if (existing.slot != layer.slot) return false;
              if (!existing.timed || !layer.timed) return !existing.timed && !layer.timed;
              return same_time(existing, layer);
            }) || !load_rgba(argv[argument + 1], layer.width, layer.height, layer.rgba)) throw 1;
        invocation.layers.push_back(std::move(layer));
      }
      const int click_argc = mode.image_click_argc;
      const int environment_argc = mode.image_environment_argc;
      const int trailer_argc = mode.image_trailer_argc;
      if (mode.image_mask_context && (!hooks.parse_mask_context ||
          !hooks.parse_mask_context(argv[trailer_argc - 1]))) throw 1;
      if (mode.image_spatial_context && (!hooks.parse_spatial_context ||
          !hooks.parse_spatial_context(argv[environment_argc - 1]))) throw 1;
      if (mode.image_render_environment && (!hooks.parse_render_environment ||
          !hooks.parse_render_environment(argv[click_argc - 1]))) throw 1;
      if (mode.image_click_context) {
        auto& color = invocation.picker_color;
        if (swscanf_s(argv[auxiliary.effective_argc - 1] + 9, L"%d|%d|%f|%f|%f|%f",
              &invocation.click_x, &invocation.click_y, &color[0], &color[1], &color[2], &color[3]) != 6 ||
            invocation.click_x < 0 || invocation.click_x > 8192 ||
            invocation.click_y < 0 || invocation.click_y > 8192 ||
            std::any_of(color.begin(), color.end(), [](float value) {
              return !std::isfinite(value) || value < 0 || value > 1;
            })) throw 1;
      }
    }
    if (kind == Kind::Smart && mode.mask_model_enabled) {
      std::string scene_id = "rectangle";
      if (mode.mask_context_request_mode) {
        if (!hooks.parse_mask_context || !hooks.parse_mask_context(argv[5])) throw 1;
      } else if (mode.mask_scene_request_mode) {
        scene_id.clear();
        for (const wchar_t* character = argv[5]; *character; ++character) {
          if (*character > 0x7f) throw 1;
          scene_id.push_back(static_cast<char>(*character));
        }
      }
      if (!mode.mask_context_request_mode && (!hooks.configure_mask_scene ||
          !hooks.configure_mask_scene(scene_id))) throw 1;
    }
  } catch (...) { result.error = 3; }
  return result;
}

}  // namespace aexcompat::worker_runtime::request_parser
