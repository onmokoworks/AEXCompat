// Selector timeline probe (issue #98 stage 0 items 1/2/6, issue #102).
//
// Logs every EffectMain invocation as one JSON line so a multi-frame aerender
// run records the real host's selector order, per-frame timing, sequence-data
// continuity (a counter that lives in sequence data), parameter values seen at
// render time, and - in the smart flavor - the checkout answers and the output
// world the host supplies. Two build flavors share this source:
//
//   pf_selector_timeline_classic.aex  - Classic RENDER path only.
//   pf_selector_timeline_smart.aex    - advertises SUPPORTS_SMART_RENDER
//                                       (define SELTIMELINE_SMART).
//
// The log path comes from AEXCOMPAT_SELECTOR_TIMELINE_LOG (absolute file
// path); without it, %TEMP%\aexcompat-selector-timeline.jsonl. Records are
// appended, one JSON object per line, and never contain image contents or
// machine paths. "Probe Mode" (slider, smart flavor) selects the pre-render
// answer: 0 = result_rect == max_result_rect == full frame, 1 = result_rect
// strictly inside max_result_rect (issue #102: which of the two sizes the
// host's output world uses).

#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"

#include <windows.h>

#include <atomic>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>

namespace {

// Bumped to "SEL2" when the audio counters were appended (#239): an old-layout
// handle from a previously loaded build carries the former magic, so lock_counters
// rejects it (magic mismatch -> nullptr) and the RESETUP null-restore path
// reallocates a full-size block, instead of reading/writing past the smaller old
// allocation. Any layout change to SequenceCounters must bump this.
constexpr std::uint32_t kSequenceMagic = 0x53454C32;  // "SEL2"

struct SequenceCounters {
  std::uint32_t magic;
  std::uint32_t setup_count;
  std::uint32_t resetup_count;
  std::uint32_t render_count;
  std::uint32_t smart_pre_render_count;
  std::uint32_t smart_render_count;
  std::uint32_t flatten_count;
  std::uint32_t frame_setup_count;
  std::uint32_t frame_setdown_count;
  // Audio observation (issue #98 W4 / #239 stage 0): the AUDIO selector
  // issuance relative to the image RENDER path is the open question for the
  // resident-session audio design. The probe advertises AUDIO_EFFECT_TOO so
  // the host routes these when the comp carries audio.
  std::uint32_t audio_setup_count;
  std::uint32_t audio_render_count;
  std::uint32_t audio_setdown_count;
};

std::atomic<std::uint32_t> event_counter{0};

const char* selector_name(PF_Cmd cmd) {
  switch (cmd) {
    case PF_Cmd_ABOUT: return "ABOUT";
    case PF_Cmd_GLOBAL_SETUP: return "GLOBAL_SETUP";
    case PF_Cmd_GLOBAL_SETDOWN: return "GLOBAL_SETDOWN";
    case PF_Cmd_PARAMS_SETUP: return "PARAMS_SETUP";
    case PF_Cmd_SEQUENCE_SETUP: return "SEQUENCE_SETUP";
    case PF_Cmd_SEQUENCE_RESETUP: return "SEQUENCE_RESETUP";
    case PF_Cmd_SEQUENCE_FLATTEN: return "SEQUENCE_FLATTEN";
    case PF_Cmd_SEQUENCE_SETDOWN: return "SEQUENCE_SETDOWN";
    case PF_Cmd_FRAME_SETUP: return "FRAME_SETUP";
    case PF_Cmd_RENDER: return "RENDER";
    case PF_Cmd_FRAME_SETDOWN: return "FRAME_SETDOWN";
    case PF_Cmd_USER_CHANGED_PARAM: return "USER_CHANGED_PARAM";
    case PF_Cmd_UPDATE_PARAMS_UI: return "UPDATE_PARAMS_UI";
    case PF_Cmd_EVENT: return "EVENT";
    case PF_Cmd_QUERY_DYNAMIC_FLAGS: return "QUERY_DYNAMIC_FLAGS";
    case PF_Cmd_SMART_PRE_RENDER: return "SMART_PRE_RENDER";
    case PF_Cmd_SMART_RENDER: return "SMART_RENDER";
    case PF_Cmd_GET_FLATTENED_SEQUENCE_DATA: return "GET_FLATTENED_SEQUENCE_DATA";
    case PF_Cmd_AUDIO_SETUP: return "AUDIO_SETUP";
    case PF_Cmd_AUDIO_RENDER: return "AUDIO_RENDER";
    case PF_Cmd_AUDIO_SETDOWN: return "AUDIO_SETDOWN";
    default: return "OTHER";
  }
}

std::string log_path() {
  char configured[1024]{};
  DWORD length = GetEnvironmentVariableA("AEXCOMPAT_SELECTOR_TIMELINE_LOG",
                                         configured, sizeof(configured));
  if (length > 0 && length < sizeof(configured)) return configured;
  char temp_dir[1024]{};
  length = GetTempPathA(sizeof(temp_dir), temp_dir);
  if (length == 0 || length >= sizeof(temp_dir)) return {};
  return std::string(temp_dir) + "aexcompat-selector-timeline.jsonl";
}

void append_line(const std::string& line) {
  static std::string path = log_path();
  if (path.empty()) return;
  const HANDLE file =
      CreateFileA(path.c_str(), FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
                  nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
  if (file == INVALID_HANDLE_VALUE) return;
  DWORD written = 0;
  WriteFile(file, line.data(), static_cast<DWORD>(line.size()), &written, nullptr);
  CloseHandle(file);
}

SequenceCounters* lock_counters(PF_InData* in_data, PF_Handle handle) {
  if (!handle || !in_data || !in_data->utils) return nullptr;
  auto* counters = static_cast<SequenceCounters*>(
      (*in_data->utils->host_lock_handle)(handle));
  if (counters && counters->magic != kSequenceMagic) {
    (*in_data->utils->host_unlock_handle)(handle);
    return nullptr;
  }
  return counters;
}

// One record per selector; numeric-only payload plus fixed enum-like strings,
// so no JSON escaping is needed.
void log_event(PF_Cmd cmd, PF_InData* in_data, const SequenceCounters* counters,
               double drive_value, bool drive_valid, const char* extra_json) {
  char buffer[1024];
  const std::uint32_t sequence = event_counter.fetch_add(1) + 1;
  int written = std::snprintf(
      buffer, sizeof(buffer),
      "{\"seq\":%u,\"pid\":%lu,\"tid\":%lu,\"cmd\":%d,\"cmd_name\":\"%s\"",
      sequence, GetCurrentProcessId(), GetCurrentThreadId(), static_cast<int>(cmd),
      selector_name(cmd));
  std::string line(buffer, written > 0 ? static_cast<size_t>(written) : 0);
  if (in_data) {
    written = std::snprintf(
        buffer, sizeof(buffer),
        ",\"current_time\":%d,\"time_step\":%d,\"time_scale\":%u,"
        "\"total_time\":%d,\"width\":%d,\"height\":%d,\"field\":%d,"
        "\"sequence_data\":%s",
        in_data->current_time, in_data->time_step, in_data->time_scale,
        in_data->total_time, in_data->width, in_data->height, in_data->field,
        in_data->sequence_data ? "\"present\"" : "null");
    line.append(buffer, written > 0 ? static_cast<size_t>(written) : 0);
  }
  if (counters) {
    written = std::snprintf(
        buffer, sizeof(buffer),
        ",\"seq_counters\":{\"setup\":%u,\"resetup\":%u,\"render\":%u,"
        "\"smart_pre_render\":%u,\"smart_render\":%u,\"flatten\":%u,"
        "\"frame_setup\":%u,\"frame_setdown\":%u,\"audio_setup\":%u,"
        "\"audio_render\":%u,\"audio_setdown\":%u}",
        counters->setup_count, counters->resetup_count, counters->render_count,
        counters->smart_pre_render_count, counters->smart_render_count,
        counters->flatten_count, counters->frame_setup_count,
        counters->frame_setdown_count, counters->audio_setup_count,
        counters->audio_render_count, counters->audio_setdown_count);
    line.append(buffer, written > 0 ? static_cast<size_t>(written) : 0);
  }
  if (drive_valid) {
    written = std::snprintf(buffer, sizeof(buffer), ",\"drive\":%.6f", drive_value);
    line.append(buffer, written > 0 ? static_cast<size_t>(written) : 0);
  }
  if (extra_json && *extra_json) {
    line.push_back(',');
    line.append(extra_json);
  }
  line.append("}\n");
  append_line(line);
}

PF_Err add_slider(PF_InData* in_data, const char* name, double maximum,
                  A_long id) {
  PF_ParamDef def{};
  def.param_type = PF_Param_FLOAT_SLIDER;
  std::snprintf(def.name, sizeof(def.name), "%s", name);
  def.u.fs_d.valid_min = 0;
  def.u.fs_d.slider_min = 0;
  def.u.fs_d.valid_max = static_cast<PF_FpShort>(maximum);
  def.u.fs_d.slider_max = static_cast<PF_FpShort>(maximum);
  def.u.fs_d.value = 0;
  def.u.fs_d.dephault = 0;
  def.u.fs_d.precision = 2;
  def.uu.id = id;
  return (*in_data->inter.add_param)(in_data->effect_ref, -1, &def);
}

PF_Err sequence_setup(PF_InData* in_data, PF_OutData* out_data) {
  const PF_Handle handle =
      (*in_data->utils->host_new_handle)(sizeof(SequenceCounters));
  if (!handle) return PF_Err_OUT_OF_MEMORY;
  auto* counters = static_cast<SequenceCounters*>(
      (*in_data->utils->host_lock_handle)(handle));
  if (!counters) return PF_Err_OUT_OF_MEMORY;
  *counters = SequenceCounters{kSequenceMagic, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0};
  (*in_data->utils->host_unlock_handle)(handle);
  out_data->sequence_data = handle;
  return PF_Err_NONE;
}

template <typename Update>
void update_counters(PF_InData* in_data, Update update) {
  const auto handle = static_cast<PF_Handle>(in_data->sequence_data);
  if (auto* counters = lock_counters(in_data, handle)) {
    update(counters);
    (*in_data->utils->host_unlock_handle)(handle);
  }
}

SequenceCounters snapshot_counters(PF_InData* in_data, bool* valid) {
  SequenceCounters copy{};
  *valid = false;
  const auto handle =
      in_data ? static_cast<PF_Handle>(in_data->sequence_data) : nullptr;
  if (auto* counters = lock_counters(in_data, handle)) {
    copy = *counters;
    *valid = true;
    (*in_data->utils->host_unlock_handle)(handle);
  }
  return copy;
}

PF_Err classic_render(PF_InData* in_data, PF_ParamDef* params[],
                      PF_LayerDef* output) {
  if (!output || !output->data) return PF_Err_BAD_CALLBACK_PARAM;
  auto* base = reinterpret_cast<A_u_char*>(output->data);
  for (A_long y = 0; y < output->height; ++y) {
    auto* row = reinterpret_cast<PF_Pixel8*>(base + y * output->rowbytes);
    for (A_long x = 0; x < output->width; ++x)
      row[x] = PF_Pixel8{255, 32, 128, 224};
  }
  char extra[256];
  std::snprintf(extra, sizeof(extra),
                "\"output_world\":{\"width\":%d,\"height\":%d,\"rowbytes\":%ld,"
                "\"origin_x\":%d,\"origin_y\":%d},"
                "\"extent_hint\":[%d,%d,%d,%d]",
                output->width, output->height, static_cast<long>(output->rowbytes),
                output->origin_x, output->origin_y, in_data->extent_hint.left,
                in_data->extent_hint.top, in_data->extent_hint.right,
                in_data->extent_hint.bottom);
  bool counters_valid = false;
  const SequenceCounters counters = snapshot_counters(in_data, &counters_valid);
  const bool drive_valid = params && params[1];
  log_event(PF_Cmd_RENDER, in_data, counters_valid ? &counters : nullptr,
            drive_valid ? params[1]->u.fs_d.value : 0.0, drive_valid, extra);
  return PF_Err_NONE;
}

// Audio selectors (issue #98 W4 / #239 stage 0). Counts the selector, passes
// the input audio through unchanged on AUDIO_RENDER (the host allocates
// dest_snd; a same-format copy keeps the render valid without altering the
// signal), and logs the sample range plus the negotiated format so the JSONL
// timeline shows how AUDIO selectors interleave with the image RENDER path.
PF_Err handle_audio(PF_Cmd cmd, PF_InData* in_data, PF_OutData* out_data) {
  update_counters(in_data, [cmd](SequenceCounters* counters) {
    if (cmd == PF_Cmd_AUDIO_SETUP)
      counters->audio_setup_count += 1;
    else if (cmd == PF_Cmd_AUDIO_RENDER)
      counters->audio_render_count += 1;
    else
      counters->audio_setdown_count += 1;
  });
  if (cmd == PF_Cmd_AUDIO_RENDER) {
    const PF_SoundWorld& src = in_data->src_snd;
    PF_SoundWorld& dst = out_data->dest_snd;
    if (src.dataP && dst.dataP &&
        src.fi.num_channels == dst.fi.num_channels &&
        src.fi.sample_size == dst.fi.sample_size &&
        src.num_samples == dst.num_samples) {
      const std::size_t bytes = static_cast<std::size_t>(src.num_samples) *
                                src.fi.num_channels * src.fi.sample_size;
      std::memcpy(dst.dataP, src.dataP, bytes);
    }
  }
  char extra[512];
  if (cmd == PF_Cmd_AUDIO_RENDER) {
    // The SDK only guarantees the audio PF_InData fields (sample range and
    // src_snd sound world) for AUDIO_RENDER. AUDIO_SETUP requests an input span
    // and AUDIO_SETDOWN frees setup memory; reading src_snd there yields stale
    // or undefined data, so only log the sound world for AUDIO_RENDER.
    const PF_SoundWorld& src = in_data->src_snd;
    std::snprintf(
        extra, sizeof(extra),
        "\"audio\":{\"start_samp\":%ld,\"dur_samp\":%ld,\"total_samp\":%ld,"
        "\"src_rate\":%.3f,\"src_channels\":%d,\"src_sample_size\":%d,"
        "\"src_samples\":%ld}",
        static_cast<long>(in_data->start_sampL),
        static_cast<long>(in_data->dur_sampL),
        static_cast<long>(in_data->total_sampL), static_cast<double>(src.fi.rateF),
        static_cast<int>(src.fi.num_channels), static_cast<int>(src.fi.sample_size),
        static_cast<long>(src.num_samples));
  } else {
    // Explicit null so the timeline records that no sound-world data was read
    // for setup/setdown, rather than emitting stale fields.
    std::snprintf(extra, sizeof(extra), "\"audio\":null");
  }
  bool counters_valid = false;
  const SequenceCounters counters = snapshot_counters(in_data, &counters_valid);
  log_event(cmd, in_data, counters_valid ? &counters : nullptr, 0.0, false, extra);
  return PF_Err_NONE;
}

#if defined(SELTIMELINE_SMART)

double checkout_slider(PF_InData* in_data, A_long index, bool* valid) {
  PF_ParamDef def{};
  *valid = false;
  if ((*in_data->inter.checkout_param)(in_data->effect_ref, index,
                                       in_data->current_time, in_data->time_step,
                                       in_data->time_scale, &def))
    return 0.0;
  const double value = def.u.fs_d.value;
  *valid = true;
  (*in_data->inter.checkin_param)(in_data->effect_ref, &def);
  return value;
}

PF_Err smart_pre_render(PF_InData* in_data, PF_PreRenderExtra* extra) {
  if (!in_data || !extra || !extra->input || !extra->output || !extra->cb)
    return PF_Err_BAD_CALLBACK_PARAM;
  const A_long width = in_data->width;
  const A_long height = in_data->height;
  const PF_LRect full{0, 0, width, height};
  bool mode_valid = false;
  const double mode = checkout_slider(in_data, 2, &mode_valid);
  bool drive_valid = false;
  const double drive = checkout_slider(in_data, 1, &drive_valid);

  PF_RenderRequest request = extra->input->output_request;
  request.rect = full;
  PF_CheckoutResult checkout{};
  const PF_Err err = extra->cb->checkout_layer(
      in_data->effect_ref, 0, 0, &request, in_data->current_time,
      in_data->time_step, in_data->time_scale, &checkout);
  if (err) return err;

  // Mode 1 (issue #102): promise result_rect strictly inside max_result_rect
  // so the render can observe which of the two the host sized the output
  // world with. Every other mode answers the full frame for both.
  const bool inset_mode = mode_valid && mode >= 0.5 && mode < 1.5 &&
                          width > 16 && height > 8;
  const PF_LRect inset{8, 4, width - 8, height - 4};
  extra->output->result_rect = inset_mode ? inset : full;
  extra->output->max_result_rect = full;

  char extra_json[512];
  std::snprintf(
      extra_json, sizeof(extra_json),
      "\"probe_mode\":%d,"
      "\"output_request\":[%d,%d,%d,%d],"
      "\"checkout\":{\"result_rect\":[%d,%d,%d,%d],\"max_result_rect\":[%d,%d,%d,%d]},"
      "\"answered\":{\"result_rect\":[%d,%d,%d,%d],\"max_result_rect\":[%d,%d,%d,%d]}",
      inset_mode ? 1 : 0, extra->input->output_request.rect.left,
      extra->input->output_request.rect.top, extra->input->output_request.rect.right,
      extra->input->output_request.rect.bottom, checkout.result_rect.left,
      checkout.result_rect.top, checkout.result_rect.right,
      checkout.result_rect.bottom, checkout.max_result_rect.left,
      checkout.max_result_rect.top, checkout.max_result_rect.right,
      checkout.max_result_rect.bottom, extra->output->result_rect.left,
      extra->output->result_rect.top, extra->output->result_rect.right,
      extra->output->result_rect.bottom, extra->output->max_result_rect.left,
      extra->output->max_result_rect.top, extra->output->max_result_rect.right,
      extra->output->max_result_rect.bottom);
  update_counters(in_data, [](SequenceCounters* counters) {
    counters->smart_pre_render_count += 1;
  });
  bool counters_valid = false;
  const SequenceCounters counters = snapshot_counters(in_data, &counters_valid);
  log_event(PF_Cmd_SMART_PRE_RENDER, in_data,
            counters_valid ? &counters : nullptr, drive, drive_valid, extra_json);
  return PF_Err_NONE;
}

template <typename Pixel, typename Channel>
void fill_world(PF_EffectWorld* world, Channel opaque) {
  for (A_long y = 0; y < world->height; ++y) {
    auto* row = reinterpret_cast<Pixel*>(
        reinterpret_cast<A_u_char*>(world->data) + y * world->rowbytes);
    for (A_long x = 0; x < world->width; ++x) {
      auto* channels = reinterpret_cast<Channel*>(&row[x]);
      channels[0] = opaque;
      channels[1] = opaque;
      channels[2] = Channel(0);
      channels[3] = opaque;
    }
  }
}

PF_Err smart_render(PF_InData* in_data, PF_SmartRenderExtra* extra) {
  if (!in_data || !extra || !extra->cb) return PF_Err_BAD_CALLBACK_PARAM;
  PF_EffectWorld* input = nullptr;
  PF_EffectWorld* output = nullptr;
  PF_Err err = extra->cb->checkout_layer_pixels(in_data->effect_ref, 0, &input);
  if (!err) err = extra->cb->checkout_output(in_data->effect_ref, &output);
  if (err) return err;
  if (!output || !output->data) return PF_Err_BAD_CALLBACK_PARAM;
  if (output->rowbytes >= output->width * static_cast<A_long>(sizeof(PF_PixelFloat)))
    fill_world<PF_PixelFloat, PF_FpShort>(output, 1.0f);
  else if (output->rowbytes >= output->width * static_cast<A_long>(sizeof(PF_Pixel16)))
    fill_world<PF_Pixel16, A_u_short>(output, PF_MAX_CHAN16);
  else
    fill_world<PF_Pixel8, A_u_char>(output, PF_MAX_CHAN8);

  // The output world's dimensions against the pre-render's inset result_rect
  // versus full max_result_rect are the issue #102 observation.
  char extra_json[512];
  std::snprintf(extra_json, sizeof(extra_json),
                "\"output_world\":{\"width\":%d,\"height\":%d,\"rowbytes\":%ld,"
                "\"origin_x\":%d,\"origin_y\":%d},"
                "\"input_world\":{\"width\":%d,\"height\":%d}",
                output->width, output->height,
                static_cast<long>(output->rowbytes), output->origin_x,
                output->origin_y, input ? input->width : -1,
                input ? input->height : -1);
  update_counters(in_data, [](SequenceCounters* counters) {
    counters->smart_render_count += 1;
  });
  bool counters_valid = false;
  const SequenceCounters counters = snapshot_counters(in_data, &counters_valid);
  log_event(PF_Cmd_SMART_RENDER, in_data, counters_valid ? &counters : nullptr,
            0.0, false, extra_json);
  return extra->cb->checkin_layer_pixels(in_data->effect_ref, 0);
}

#endif  // SELTIMELINE_SMART

}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                       PF_OutData* out_data, PF_ParamDef* params[],
                                       PF_LayerDef* output, void* extra) {
  // Selectors with per-selector handlers log inside those handlers (they can
  // attach richer payloads); everything else logs generically here.
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
#if defined(SELTIMELINE_SMART)
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      out_data->out_flags2 = PF_OutFlag2_SUPPORTS_SMART_RENDER;
#else
      // AUDIO_EFFECT_TOO (not just I_USE_AUDIO) is what makes the host issue
      // AUDIO_SETUP/RENDER/SETDOWN to this effect, so a multi-frame comp with
      // audio records the audio selector timeline alongside the image path
      // (issue #98 W4 / #239 stage 0). Classic flavor only; the flag must also
      // be declared in the PiPL OutFlags (pf_selector_timeline_classic.rc) to
      // match, or the host rejects the GLOBAL_SETUP out_flags as inconsistent.
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_AUDIO_EFFECT_TOO;
      out_data->out_flags2 = 0;
#endif
      log_event(cmd, in_data, nullptr, 0.0, false, nullptr);
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP: {
      PF_Err err = add_slider(in_data, "Drive", 100.0, 1);
      if (!err) err = add_slider(in_data, "Probe Mode", 10.0, 2);
      out_data->num_params = 3;
      log_event(cmd, in_data, nullptr, 0.0, false, nullptr);
      return err;
    }
    case PF_Cmd_SEQUENCE_SETUP: {
      const PF_Err err = sequence_setup(in_data, out_data);
      bool counters_valid = false;
      PF_InData probe = *in_data;
      probe.sequence_data = out_data->sequence_data;
      const SequenceCounters counters = snapshot_counters(&probe, &counters_valid);
      log_event(cmd, in_data, counters_valid ? &counters : nullptr, 0.0, false,
                nullptr);
      return err;
    }
    case PF_Cmd_SEQUENCE_RESETUP: {
      // No flattening is advertised, so an existing (unflat) handle is kept;
      // a null sequence_data (the documented flat-restore path) allocates a
      // fresh counter block whose setup/resetup history restarts.
      PF_Err err = PF_Err_NONE;
      if (in_data->sequence_data) {
        update_counters(in_data, [](SequenceCounters* counters) {
          counters->resetup_count += 1;
        });
        out_data->sequence_data = static_cast<PF_Handle>(in_data->sequence_data);
      } else {
        err = sequence_setup(in_data, out_data);
      }
      PF_InData probe = *in_data;
      probe.sequence_data = out_data->sequence_data;
      bool counters_valid = false;
      const SequenceCounters counters = snapshot_counters(&probe, &counters_valid);
      log_event(cmd, in_data, counters_valid ? &counters : nullptr, 0.0, false,
                nullptr);
      return err;
    }
    case PF_Cmd_SEQUENCE_SETDOWN: {
      bool counters_valid = false;
      const SequenceCounters counters = snapshot_counters(in_data, &counters_valid);
      log_event(cmd, in_data, counters_valid ? &counters : nullptr, 0.0, false,
                nullptr);
      if (in_data->sequence_data && in_data->utils)
        (*in_data->utils->host_dispose_handle)(
            static_cast<PF_Handle>(in_data->sequence_data));
      out_data->sequence_data = nullptr;
      return PF_Err_NONE;
    }
    case PF_Cmd_SEQUENCE_FLATTEN: {
      // The counter block is plain data, so it is its own flat form; count
      // the flatten and hand the same handle back.
      update_counters(in_data, [](SequenceCounters* counters) {
        counters->flatten_count += 1;
      });
      out_data->sequence_data = static_cast<PF_Handle>(in_data->sequence_data);
      bool counters_valid = false;
      const SequenceCounters counters = snapshot_counters(in_data, &counters_valid);
      log_event(cmd, in_data, counters_valid ? &counters : nullptr, 0.0, false,
                nullptr);
      return PF_Err_NONE;
    }
    case PF_Cmd_FRAME_SETUP:
    case PF_Cmd_FRAME_SETDOWN: {
      update_counters(in_data, [cmd](SequenceCounters* counters) {
        if (cmd == PF_Cmd_FRAME_SETUP)
          counters->frame_setup_count += 1;
        else
          counters->frame_setdown_count += 1;
      });
      bool counters_valid = false;
      const SequenceCounters counters = snapshot_counters(in_data, &counters_valid);
      const bool drive_valid = params && params[1];
      log_event(cmd, in_data, counters_valid ? &counters : nullptr,
                drive_valid ? params[1]->u.fs_d.value : 0.0, drive_valid, nullptr);
      return PF_Err_NONE;
    }
    case PF_Cmd_RENDER:
      update_counters(in_data, [](SequenceCounters* counters) {
        counters->render_count += 1;
      });
      return classic_render(in_data, params, output);
    case PF_Cmd_AUDIO_SETUP:
    case PF_Cmd_AUDIO_RENDER:
    case PF_Cmd_AUDIO_SETDOWN:
      return handle_audio(cmd, in_data, out_data);
#if defined(SELTIMELINE_SMART)
    case PF_Cmd_SMART_PRE_RENDER:
      return smart_pre_render(in_data, static_cast<PF_PreRenderExtra*>(extra));
    case PF_Cmd_SMART_RENDER:
      return smart_render(in_data, static_cast<PF_SmartRenderExtra*>(extra));
#endif
    default: {
      (void)extra;
      bool counters_valid = false;
      const SequenceCounters counters = snapshot_counters(in_data, &counters_valid);
      log_event(cmd, in_data, counters_valid ? &counters : nullptr, 0.0, false,
                nullptr);
      return PF_Err_NONE;
    }
  }
}
