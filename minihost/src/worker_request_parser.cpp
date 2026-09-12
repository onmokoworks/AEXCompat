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
      kind == Kind::Classic ? aexcompat::l2cli::WorkerKind::Classic :
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
    if (mode.audio_session_mode) {
      // [command, plugin, sha256, payload, max_samples, channels, time_scale]
      // (protocol §10.2). v1 audio is mono; channels stays for the extension.
      auto& invocation = result.invocation;
      invocation.audio_session_max_samples = std::stoi(argv[5]);
      invocation.audio_session_channels = std::stoi(argv[6]);
      invocation.time_scale = std::stoul(argv[7]);
      if (invocation.audio_session_max_samples <= 0 ||
          invocation.audio_session_max_samples > 16 * 1024 * 1024 ||
          invocation.audio_session_channels != 1 ||
          invocation.time_scale == 0 || invocation.time_scale > 0x7FFFFFFFu) throw 1;
    }
    if (mode.render_session_mode) {
      auto& invocation = result.invocation;
      invocation.width = std::stoi(argv[5]); invocation.height = std::stoi(argv[6]);
      invocation.time_step = std::stoi(argv[7]);
      invocation.total_time = std::stoi(argv[8]);
      invocation.time_scale = std::stoul(argv[9]);
      // The per-frame protocol carries current_time.scale as a signed 32-bit
      // value, so a launch scale above INT32_MAX could never be matched by
      // any frame; keep the launch and frame contracts on the same domain.
      // A zero-duration session (total_time == 0) is valid and renders the
      // single current_time == 0 frame, matching the one-shot worker (#272);
      // only a negative total_time is rejected. The per-frame loop still rejects
      // current_time > total_time (worker_render_session.cpp), so total_time == 0
      // admits exactly the t=0 frame.
      if (invocation.width <= 0 || invocation.height <= 0 ||
          invocation.width > 4096 || invocation.height > 4096 ||
          invocation.time_step <= 0 || invocation.total_time < 0 ||
          invocation.time_scale == 0 ||
          invocation.time_scale > 0x7FFFFFFFu) throw 1;
      // Static context trailers, one-shot order and hooks (the classifier
      // stored each trailer's index in the matching *_argc field).
      if (mode.image_mask_context && (!hooks.parse_mask_context ||
          !hooks.parse_mask_context(argv[mode.image_argc]))) throw 1;
      if (mode.image_spatial_context && (!hooks.parse_spatial_context ||
          !hooks.parse_spatial_context(argv[mode.image_trailer_argc]))) throw 1;
      if (mode.image_render_environment && (!hooks.parse_render_environment ||
          !hooks.parse_render_environment(argv[mode.image_environment_argc]))) throw 1;
      // Audio source for the session (`session-audio:v1|<samples>|<rate>|<path>`,
      // issue #339). The one-shot spends argv[13..15] on the same three values
      // under --render-image-audio; a session packs them into one marked trailer
      // because its tail is shared. The path is last so a separator inside it
      // cannot shift the numeric fields. Same load_audio validation as the
      // one-shot, so a malformed span fails the launch rather than the frame.
      if (mode.session_audio) {
        const std::wstring trailer(argv[mode.session_audio_argc]);
        const std::wstring body = trailer.substr(std::wcslen(L"session-audio:v1|"));
        const std::size_t rate_at = body.find(L'|');
        if (rate_at == std::wstring::npos) throw 1;
        const std::size_t path_at = body.find(L'|', rate_at + 1);
        if (path_at == std::wstring::npos) throw 1;
        result.invocation.audio_samples = std::stoi(body.substr(0, rate_at));
        result.invocation.audio_rate =
            std::stoi(body.substr(rate_at + 1, path_at - rate_at - 1));
        if (!load_audio(body.substr(path_at + 1).c_str(),
                        result.invocation.audio_samples,
                        result.invocation.audio_rate,
                        result.invocation.audio)) throw 1;
      }
      // Secondary layer metadata:
      // `session-layers:v2|slot,w,h,handle;slot,w,h,time,scale,handle;...`.
      // The pixels travel as inherited per-layer file HANDLEs (#268); the slot,
      // geometry, optional rational time, and the read handle value travel here
      // (issue #98 W1-4, timed layers are v1.1). The frame loop reads each
      // layer's bytes from its handle once at open.
      if (mode.session_layers) {
        const std::wstring trailer(argv[mode.image_argc - 1]);
        const std::wstring body = trailer.substr(std::wcslen(L"session-layers:v2|"));
        std::size_t offset = 0;
        while (offset < body.size()) {
          const std::size_t separator = body.find(L';', offset);
          const std::size_t end = separator == std::wstring::npos ? body.size() : separator;
          const std::wstring field = body.substr(offset, end - offset);
          LayerInput layer;
          int consumed = 0;
          // Field count selects the form. `slot,w,h,time,scale,handle` (6) is a
          // timed layer (issue #98 W1-4b), `slot,w,h,handle,1` (5) a dynamic
          // secondary whose bytes the broker rewrites between frames (#674),
          // and `slot,w,h,handle` (4) the static secondary (W1-4). The handle
          // is the last field of the static forms and of the timed one (#268).
          const auto commas = std::count(field.begin(), field.end(), L',');
          if (commas == 5) {
            if (swscanf_s(field.c_str(), L"%d,%d,%d,%d,%u,%llu%n", &layer.slot,
                    &layer.width, &layer.height, &layer.time, &layer.time_scale,
                    &layer.rgba_handle, &consumed) != 6 ||
                layer.time_scale == 0)
              throw 1;
            layer.timed = true;
          } else if (commas == 4) {
            int32_t dynamic_flag = 0;
            // Only the literal 1 marks a dynamic layer: anything else would be
            // a sender the worker does not understand, and admitting it would
            // silently pick the static reading of a field that means otherwise.
            if (swscanf_s(field.c_str(), L"%d,%d,%d,%llu,%d%n", &layer.slot,
                    &layer.width, &layer.height, &layer.rgba_handle,
                    &dynamic_flag, &consumed) != 5 ||
                dynamic_flag != 1)
              throw 1;
            layer.dynamic = true;
          } else if (swscanf_s(field.c_str(), L"%d,%d,%d,%llu%n", &layer.slot,
                         &layer.width, &layer.height, &layer.rgba_handle,
                         &consumed) != 4) {
            throw 1;
          }
          if (static_cast<std::size_t>(consumed) != field.size() ||
              layer.slot < 0 || (layer.slot == 0 && !layer.timed) || layer.slot > 1024 ||
              layer.width <= 0 || layer.width > 4096 ||
              layer.height <= 0 || layer.height > 4096 ||
              // A zero handle is never a valid inherited layer file (#268).
              layer.rgba_handle == 0 ||
              std::any_of(invocation.layers.begin(), invocation.layers.end(),
                  [&](const auto& existing) {
                    // A slot rejects only a second static entry or a timed
                    // entry at a rational time already present. A static plus
                    // timed entries at one slot is the valid representation of
                    // a layer parameter sampled at current_time and at other
                    // times, so both must be admitted.
                    if (existing.slot != layer.slot) return false;
                    if (!existing.timed || !layer.timed) return !existing.timed && !layer.timed;
                    return same_time(existing, layer);
                  }))
            throw 1;
          invocation.layers.push_back(std::move(layer));
          if (separator == std::wstring::npos) break;
          offset = separator + 1;
        }
        if (invocation.layers.empty() || invocation.layers.size() > 64) throw 1;
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
