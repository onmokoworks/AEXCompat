#include "worker_render_session.hpp"

#include <windows.h>

#include "render_pixel_transport.hpp"
#include "render_subsystem.h"
#include "strict_json.hpp"
#include "worker_invocation_orchestration.hpp"
#include "worker_pf_ae_channel_runtime.hpp"
#include "worker_request_parser.hpp"
#include "worker_smart_execution.hpp"
#include "worker_ui_event_execution.hpp"

#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <filesystem>
#include <string>
#include <variant>
#include <vector>

namespace aexcompat::worker_render_session {
namespace {

constexpr const wchar_t* kRequestVariable = L"AEXCOMPAT_RENDER_SESSION_REQUEST_HANDLE";
constexpr const wchar_t* kResponseVariable = L"AEXCOMPAT_RENDER_SESSION_RESPONSE_HANDLE";
constexpr const wchar_t* kSectionVariable = L"AEXCOMPAT_RENDER_SESSION_SECTION_HANDLE";

std::size_t align_slot(std::size_t bytes) {
  return (bytes + kSlotAlignment - 1) / kSlotAlignment * kSlotAlignment;
}

bool variable_present(const wchar_t* name) {
  wchar_t buffer[32];
  return GetEnvironmentVariableW(name, buffer, 32) != 0 ||
         GetLastError() != ERROR_ENVVAR_NOT_FOUND;
}

// Same shape as the AEX_INSTRUMENT_TRACE_HANDLE precedent in the shared
// trace writer: decimal handle number, strict tail, zero rejected, then a
// type check on the resulting handle.
HANDLE handle_from_variable(const wchar_t* name) {
  wchar_t buffer[32];
  const DWORD length = GetEnvironmentVariableW(name, buffer, 32);
  if (length == 0 || length >= 32) return nullptr;
  wchar_t* tail = nullptr;
  const unsigned long long value = std::wcstoull(buffer, &tail, 10);
  if (!tail || *tail != L'\0' || value == 0) return nullptr;
  const HANDLE handle = reinterpret_cast<HANDLE>(static_cast<uintptr_t>(value));
  DWORD flags = 0;
  if (!GetHandleInformation(handle, &flags)) return nullptr;
  return handle;
}

HANDLE pipe_from_variable(const wchar_t* name) {
  const HANDLE handle = handle_from_variable(name);
  if (!handle || GetFileType(handle) != FILE_TYPE_PIPE) return nullptr;
  return handle;
}

// Returns the byte count actually collected; a stop before `bytes` means the
// pipe hit EOF or an error mid-read.
std::size_t read_up_to(HANDLE pipe, unsigned char* destination, std::size_t bytes) {
  std::size_t collected = 0;
  while (collected < bytes) {
    DWORD read = 0;
    const DWORD request = static_cast<DWORD>(bytes - collected);
    if (!ReadFile(pipe, destination + collected, request, &read, nullptr) ||
        read == 0)
      break;
    collected += read;
  }
  return collected;
}

bool write_exact(HANDLE pipe, const unsigned char* source, std::size_t bytes) {
  std::size_t sent = 0;
  while (sent < bytes) {
    DWORD written = 0;
    const DWORD request = static_cast<DWORD>(bytes - sent);
    if (!WriteFile(pipe, source + sent, request, &written, nullptr) ||
        written == 0)
      return false;
    sent += written;
  }
  return true;
}

}  // namespace

std::size_t input_slot_bytes(const SessionGeometry& geometry) {
  return static_cast<std::size_t>(geometry.max_width) * geometry.max_height * 4;
}

std::size_t output_slot_bytes(const SessionGeometry& geometry) {
  return static_cast<std::size_t>(geometry.max_width) * geometry.max_height *
         geometry.output_pixel_bytes;
}

std::size_t input_slot_offset() { return kHeaderBytes; }

std::size_t output_slot_offset(const SessionGeometry& geometry) {
  return kHeaderBytes + align_slot(input_slot_bytes(geometry));
}

std::size_t layer_slot_offset(const SessionGeometry& geometry, int32_t index) {
  return output_slot_offset(geometry) + align_slot(output_slot_bytes(geometry)) +
         static_cast<std::size_t>(index) * align_slot(input_slot_bytes(geometry));
}

std::size_t expected_section_bytes(const SessionGeometry& geometry) {
  return output_slot_offset(geometry) + align_slot(output_slot_bytes(geometry)) +
         static_cast<std::size_t>(geometry.layer_slot_count) *
             align_slot(input_slot_bytes(geometry));
}

bool session_environment_requested() {
  return variable_present(kRequestVariable) ||
         variable_present(kResponseVariable) ||
         variable_present(kSectionVariable);
}

SessionChannels::~SessionChannels() {
  if (view_) UnmapViewOfFile(view_);
  if (section_) CloseHandle(section_);
  if (request_pipe_) CloseHandle(request_pipe_);
  if (response_pipe_) CloseHandle(response_pipe_);
}

bool SessionChannels::open_from_environment(const SessionGeometry& geometry) {
  if (opened()) return false;
  if (geometry.max_width <= 0 || geometry.max_height <= 0 ||
      geometry.layer_slot_count < 0)
    return false;
  const HANDLE request = pipe_from_variable(kRequestVariable);
  const HANDLE response = pipe_from_variable(kResponseVariable);
  const HANDLE section = handle_from_variable(kSectionVariable);
  if (!request || !response || !section) return false;
  void* view = MapViewOfFile(section, FILE_MAP_ALL_ACCESS, 0, 0, 0);
  if (!view) return false;
  MEMORY_BASIC_INFORMATION region{};
  if (VirtualQuery(view, &region, sizeof(region)) != sizeof(region) ||
      region.RegionSize < expected_section_bytes(geometry)) {
    UnmapViewOfFile(view);
    return false;
  }
  request_pipe_ = request;
  response_pipe_ = response;
  section_ = section;
  view_ = static_cast<unsigned char*>(view);
  view_bytes_ = region.RegionSize;
  return true;
}

SessionChannels::ReadResult SessionChannels::read_message(std::string& payload) {
  if (!request_pipe_) return ReadResult::Violation;
  unsigned char prefix[4];
  const std::size_t prefix_read = read_up_to(request_pipe_, prefix, sizeof(prefix));
  if (prefix_read == 0) return ReadResult::Eof;
  if (prefix_read != sizeof(prefix)) return ReadResult::Violation;
  const uint32_t length = static_cast<uint32_t>(prefix[0]) |
                          (static_cast<uint32_t>(prefix[1]) << 8) |
                          (static_cast<uint32_t>(prefix[2]) << 16) |
                          (static_cast<uint32_t>(prefix[3]) << 24);
  if (length == 0 || length > kMaxMessageBytes) return ReadResult::Violation;
  payload.resize(length);
  return read_up_to(request_pipe_,
                    reinterpret_cast<unsigned char*>(payload.data()),
                    length) == length
             ? ReadResult::Message
             : ReadResult::Violation;
}

bool SessionChannels::write_message(const std::string& payload) {
  if (!response_pipe_ || payload.empty() || payload.size() > kMaxMessageBytes)
    return false;
  const uint32_t length = static_cast<uint32_t>(payload.size());
  const unsigned char prefix[4] = {
      static_cast<unsigned char>(length & 0xFF),
      static_cast<unsigned char>((length >> 8) & 0xFF),
      static_cast<unsigned char>((length >> 16) & 0xFF),
      static_cast<unsigned char>((length >> 24) & 0xFF)};
  return write_exact(response_pipe_, prefix, sizeof(prefix)) &&
         write_exact(response_pipe_,
                     reinterpret_cast<const unsigned char*>(payload.data()),
                     payload.size());
}

uint32_t SessionChannels::read_header_u32(std::size_t offset) const {
  uint32_t value = 0;
  if (view_ && offset + sizeof(value) <= kHeaderBytes)
    std::memcpy(&value, view_ + offset, sizeof(value));
  return value;
}

void SessionChannels::write_header_u32(std::size_t offset, uint32_t value) {
  if (view_ && offset + sizeof(value) <= kHeaderBytes)
    std::memcpy(view_ + offset, &value, sizeof(value));
}

bool SessionChannels::static_header_matches(const SessionGeometry& geometry) const {
  const uint32_t depth_code = geometry.output_pixel_bytes == 16
                                  ? 32
                                  : (geometry.output_pixel_bytes == 8 ? 16 : 8);
  return opened() && read_header_u32(kHeaderMagicOffset) == kHeaderMagic &&
         read_header_u32(kHeaderVersionOffset) == kProtocolVersion &&
         read_header_u32(kHeaderDepthCodeOffset) == depth_code &&
         read_header_u32(kHeaderMaxWidthOffset) ==
             static_cast<uint32_t>(geometry.max_width) &&
         read_header_u32(kHeaderMaxHeightOffset) ==
             static_cast<uint32_t>(geometry.max_height) &&
         read_header_u32(kHeaderLayerSlotCountOffset) ==
             static_cast<uint32_t>(geometry.layer_slot_count);
}

}  // namespace aexcompat::worker_render_session

// Cross-TU declarations into worker_main's private renderers and state
// (issue #169, same shape as worker_invocation_orchestration.cpp). The
// constants mirror l2_main's frozen protocol offsets and selector codes;
// every definition stays in l2_main.
namespace aexcompat::l2_detail {

using EffectEntry = aexcompat::worker_runtime::parameter_execution::EffectEntry;
using RequestedAssignments = aexcompat::worker_runtime::parameters::RequestedAssignments;
using ExternalLayerInput = aexcompat::worker_runtime::request_parser::LayerInput;
using aexcompat::render_pixel_transport::argb_to_rgba_native;
using aexcompat::pf_ae_channel::activate_external_aux;
using aexcompat::pf_ae_channel::deactivate_external_aux;
using aexcompat::pf_ae_channel::clear_native_aux_provider;

constexpr std::size_t kInSize = 408;
constexpr std::size_t kOutSize = 408;
constexpr std::size_t kInSequenceData = 320;
constexpr std::size_t kOutSequenceData = 56;
constexpr int32_t kSequenceSetup = 5;
constexpr int32_t kSequenceSetdown = 8;

int32_t invoke_sequence_selector(EffectEntry entry, int32_t selector, void* input,
                                 void* output, uint32_t* exception_code = nullptr);
int32_t render_once(EffectEntry entry, std::array<std::byte, kInSize>& input,
                    std::array<std::byte, kOutSize>& output,
                    const std::string& case_id, int32_t& width, int32_t& height,
                    int32_t& rowbytes, std::string& input_hash, std::string& output_hash,
                    bool& guards_intact, const RequestedAssignments* requested = nullptr,
                    const std::vector<unsigned char>* external_rgba = nullptr,
                    const std::filesystem::path* external_output = nullptr,
                    int32_t external_width = 0, int32_t external_height = 0,
                    const std::vector<ExternalLayerInput>* external_layers = nullptr,
                    int32_t external_current_time = 0, int32_t external_time_step = 1,
                    int32_t external_total_time = 1, uint32_t external_time_scale = 1,
                    int32_t external_pixel_bytes = 4, bool manage_sequence = true,
                    std::vector<unsigned char>* captured_argb = nullptr,
                    bool* output_validation_failed = nullptr);
worker_runtime::smart_execution::Result smart_render_once(
    EffectEntry entry, std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, const std::string& case_id,
    const RequestedAssignments* requested = nullptr,
    const std::vector<unsigned char>* external_rgba = nullptr,
    const std::filesystem::path* external_output = nullptr,
    int32_t external_width = 0, int32_t external_height = 0,
    const std::vector<ExternalLayerInput>* external_layers = nullptr,
    int32_t external_current_time = 0, int32_t external_time_step = 1,
    int32_t external_total_time = 1, uint32_t external_time_scale = 1,
    int32_t external_pixel_bytes = 4,
    worker_runtime::smart_execution::SessionFrame* session = nullptr);
std::string sha256_bytes(const unsigned char* data, std::size_t size);
void record_output_checksum_detail(const unsigned char* rgba, int32_t width,
                                   int32_t height, int32_t pixel_bytes);
// Launch-payload parser reused verbatim for the v:2 per-frame `parameters`
// field (protocol §4.2.1): the message rides the exact argv encoding.
bool parse_parameter_payload(const wchar_t* text, RequestedAssignments& output);

namespace {
// Spatial-context full-resolution override read through its render-subsystem
// owner; these references keep the g_* spellings the session body was
// written with.
auto& g_render_context_state = aexcompat::render::render_context_state();
auto& g_full_resolution_width = g_render_context_state.full_resolution_width;
auto& g_full_resolution_height = g_render_context_state.full_resolution_height;

template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{};
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}

template <typename T, std::size_t N>
void write(std::array<std::byte, N>& bytes, std::size_t offset, T value) {
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}

// One frame's decoded v:2 `ui_action` (protocol §4.2.1, issue #238): a click
// with its coordinates and picker color, or a draw. Empty (neither flag) is
// never constructed; a frame with no `ui_action` passes a null pointer.
struct SessionUiAction {
  bool click{false};
  bool draw{false};
  int32_t x{0};
  int32_t y{0};
  std::array<float, 4> color{};
};

// Decodes the v:2 `ui_action` string, reusing the one-shot custom-UI trailer
// grammar so the session and one-shot admit/reject the identical set:
// "click:v1|x|y|r|g|b|a" (worker_request_parser.cpp:177-186) or "draw:v1"
// (l2_cli_dispatch.cpp:180-181). Bounds mirror the argv path: x/y in [0,8192],
// each color component finite in [0,1]. Returns false on any malformed value,
// which the caller escalates to a protocol violation (the broker validates the
// same grammar before sending, so a bad value is a defect or tampering).
bool parse_session_ui_action(const std::string& text, SessionUiAction& action) {
  if (text == "draw:v1") {
    action.draw = true;
    return true;
  }
  constexpr std::size_t kClickPrefixLen = 9;  // "click:v1|"
  if (text.rfind("click:v1|", 0) != 0) return false;
  float red{}, green{}, blue{}, alpha{};
  int consumed = 0;
  if (std::sscanf(text.c_str() + kClickPrefixLen, "%d|%d|%f|%f|%f|%f%n",
                  &action.x, &action.y, &red, &green, &blue, &alpha,
                  &consumed) != 6)
    return false;
  // Reject trailing bytes after the seventh field: a well-formed broker message
  // ends exactly here.
  if (text[kClickPrefixLen + static_cast<std::size_t>(consumed)] != '\0')
    return false;
  if (action.x < 0 || action.x > 8192 || action.y < 0 || action.y > 8192)
    return false;
  for (const float value : {red, green, blue, alpha})
    if (!std::isfinite(value) || value < 0.0f || value > 1.0f) return false;
  action.click = true;
  action.color = {red, green, blue, alpha};
  return true;
}

// Applies one frame's v:2 `ui_action` to the process-lifetime custom-UI
// telemetry the render path reads (the same singleton the one-shot ApplyHooks
// setters drive from argv, l2_main.cpp:1505-1509). The per-frame semantic is a
// complete replacement (protocol §4.2.1): the enabled flags are cleared first
// so a frame with no ui_action renders with no custom-UI event, then set from
// this frame's action. Frame N's click never leaks into frame N+1.
void apply_session_ui_action(const SessionUiAction* action) {
  namespace ui = worker_runtime::ui_event_execution;
  ui::CustomUiTelemetry& telemetry = ui::custom_ui_telemetry();
  // Reset the per-render custom-UI observation before every frame so a
  // multi-frame session starts each frame from the same state a fresh one-shot
  // process would, then apply this frame's action. Without this the counters
  // the click dispatcher checks for an exact value (app_color_picker_calls /
  // app_invalidate_rect_calls == 1, the lifecycle/error fields) accumulate
  // across frames and the second click reports a frame error. Only the
  // setup-time registration fields (captured once at GLOBAL_SETUP) are
  // preserved; every other field resets to its default (a fresh render's
  // starting point, e.g. render_click_error == -1). The enabled flags default
  // to false, so a frame with no ui_action renders with no custom-UI event.
  const auto register_ui_calls = telemetry.register_ui_calls;
  const auto registration = telemetry.registration;
  const auto invalid_registrations = telemetry.invalid_custom_ui_registrations;
  telemetry = ui::CustomUiTelemetry{};
  telemetry.register_ui_calls = register_ui_calls;
  telemetry.registration = registration;
  telemetry.invalid_custom_ui_registrations = invalid_registrations;
  if (!action) return;
  if (action->click) {
    telemetry.render_click_x = action->x;
    telemetry.render_click_y = action->y;
    telemetry.app_picker_color = action->color;
    telemetry.render_click_enabled = true;
  }
  if (action->draw) telemetry.render_draw_enabled = true;
}
}  // namespace

// One rendered session frame as the shared loop below consumes it: the
// renderer-specific callback (classic render_once or smart_render_once)
// reports its result in this shape so the transport, message grammar, and
// fail-closed decisions stay identical across both session flavors.
struct SessionFrameOutput {
  int32_t frame_error{0};
  int32_t width{0};
  int32_t height{0};
  int32_t rowbytes{0};
  std::string input_hash;
  std::string output_hash;
  bool guard_violation{false};
  bool output_validation_failed{false};
};

// Resident render session frame loop (docs/RENDER_SESSION_PROTOCOL_2026-07-19.md).
// SEQUENCE_SETUP is hoisted once around per-frame renders following the
// persistent_sequence precedent; pixels move through the inherited anonymous
// section (copy-through slots, the plug-in never sees the mapping) and control
// messages over the inherited pipe pair with strict exact-key validation.
// render_frame(current_time, frame_rgba, captured, frame_layers,
// frame_override, frame_ui) runs one frame under the hoisted sequence and
// returns the SessionFrameOutput above with the packed ARGB output in
// `captured`. frame_override is a v:2 per-frame parameter set (protocol
// §4.2.1) or null; the wrappers fall back to their launch payload when it is
// null. frame_ui is a v:2 per-frame custom-UI action (§4.2.1) or null; the
// callback drives the click/draw event sequence for that frame only.
// RenderSessionOutcome is defined in worker_invocation_orchestration.hpp so
// the final dispatch owner can call the wrappers below across TUs.
template <typename FrameFn>
RenderSessionOutcome run_session_frame_loop(
    EffectEntry entry, std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output,
    int32_t max_width, int32_t max_height, int32_t time_step, int32_t total_time,
    uint32_t time_scale, int32_t pixel_bytes,
    const std::vector<ExternalLayerInput>* external_layers, FrameFn&& render_frame) {
  using aexcompat::strict_json::JsonValue;
  using aexcompat::strict_json::StrictJsonParser;
  using aexcompat::strict_json::json_exact_keys;
  using aexcompat::strict_json::json_i32;
  using aexcompat::strict_json::json_member;
  using aexcompat::strict_json::json_string;
  namespace wrs = aexcompat::worker_render_session;
  constexpr int32_t kSessionTimeScaleMismatch = -40;
  constexpr int32_t kSessionGenerationMismatch = -41;
  constexpr int32_t kSessionOutputCaptureError = -42;
  constexpr int32_t kSessionGuardViolation = -43;
  constexpr int32_t kSessionDimensionMismatch = -44;
  constexpr int32_t kSessionOutputValidationError = -45;
  constexpr int32_t kSessionTimeOutOfRange = -46;
  // Deferred SEQUENCE_SETUP failed: the session can never render, so the
  // response must read as continuation-impossible, not frame-local (the
  // plug-in's own setup error is preserved in the final report).
  constexpr int32_t kSessionSequenceSetupFailed = -47;

  RenderSessionOutcome outcome;
  const int32_t layer_slot_count =
      external_layers ? static_cast<int32_t>(external_layers->size()) : 0;
  const wrs::SessionGeometry geometry{max_width, max_height, pixel_bytes,
                                      layer_slot_count};
  wrs::SessionChannels channels;
  if (!channels.open_from_environment(geometry) ||
      !channels.static_header_matches(geometry)) {
    outcome.protocol_violation = true;
    return outcome;
  }
  // Layers are static for the whole session: copy each layer's RGBA out of
  // its shared slot once, into a worker-private vector the render loop reuses
  // (the plug-in never sees the mapping). A layer whose declared geometry
  // overflows its slot is a fail-closed launch error.
  std::vector<ExternalLayerInput> session_layers;
  if (external_layers) {
    session_layers = *external_layers;
    for (int32_t index = 0; index < layer_slot_count; ++index) {
      auto& layer = session_layers[index];
      const std::size_t bytes =
          static_cast<std::size_t>(layer.width) * layer.height * 4;
      if (bytes == 0 || bytes > wrs::input_slot_bytes(geometry)) {
        outcome.protocol_violation = true;
        return outcome;
      }
      layer.rgba.resize(bytes);
      std::memcpy(layer.rgba.data(),
                  channels.view() + wrs::layer_slot_offset(geometry, index), bytes);
    }
  }
  const std::vector<ExternalLayerInput>* frame_layers =
      session_layers.empty() ? nullptr : &session_layers;
  // Static in_data geometry and timing fields for the whole session. The
  // per-frame current_time is seeded when SEQUENCE_SETUP actually runs:
  // setup is deferred to the first rendered frame so effects that
  // initialize persistent sequence state from in_data->current_time observe
  // that frame's time, exactly like the one-shot render lifecycle seeds the
  // requested time before SEQUENCE_SETUP.
  write<int32_t>(input, 228, time_step);
  write<int32_t>(input, 232, total_time);
  write<int32_t>(input, 236, time_step);
  write<uint32_t>(input, 240, time_scale);
  // Same full-resolution override the per-frame render applies: a spatial
  // context can declare the true composition size, and the deferred
  // SEQUENCE_SETUP must observe it exactly like the one-shot lifecycle.
  write<int32_t>(input, 252,
                 g_full_resolution_width > 0 ? g_full_resolution_width : max_width);
  write<int32_t>(input, 256,
                 g_full_resolution_height > 0 ? g_full_resolution_height : max_height);
  const int32_t session_extent[4] = {0, 0, max_width, max_height};
  std::memcpy(input.data() + 260, session_extent, sizeof(session_extent));
  bool sequence_attempted = false;
  bool sequence_started = false;

  const std::size_t input_offset = wrs::input_slot_offset();
  const std::size_t output_offset = wrs::output_slot_offset(geometry);
  const char* pixel_format = pixel_bytes == 16 ? "argb32f" :
      (pixel_bytes == 8 ? "argb16" : "argb8");
  std::vector<unsigned char> frame_rgba(wrs::input_slot_bytes(geometry));
  std::vector<unsigned char> captured;
  std::string message;
  for (;;) {
    const auto read_result = channels.read_message(message);
    if (read_result == wrs::SessionChannels::ReadResult::Eof) break;
    if (read_result != wrs::SessionChannels::ReadResult::Message) {
      // Malformed framing is invalid control input, not a close signal.
      outcome.protocol_violation = true;
      break;
    }
    JsonValue root;
    if (!StrictJsonParser(std::move(message)).parse(root) ||
        !std::holds_alternative<JsonValue::Object>(root.value)) {
      outcome.protocol_violation = true;
      break;
    }
    const auto& object = std::get<JsonValue::Object>(root.value);
    std::string type;
    int32_t version{};
    if (!json_string(object, "type", type) || !json_i32(object, "v", version) ||
        (version != static_cast<int32_t>(wrs::kProtocolVersion) &&
         version != static_cast<int32_t>(wrs::kRenderFrameParametersVersion))) {
      outcome.protocol_violation = true;
      break;
    }
    if (type == "close") {
      if (version != static_cast<int32_t>(wrs::kProtocolVersion) ||
          !json_exact_keys(object, {"v", "type"}))
        outcome.protocol_violation = true;
      break;
    }
    int32_t frame_index{};
    const auto* time_value = json_member(object, "current_time");
    // v:2 carries per-frame dynamic attributes (protocol §4.2.1): `parameters`
    // (#107) and/or `ui_action` (#238) as presence-driven optional fields. At
    // least one must be present; a v:2 with neither is a protocol violation
    // (senders use v:1 for a plain frame). The exact-key set is built from the
    // present optionals so unknown keys stay rejected.
    const bool with_attributes =
        version == static_cast<int32_t>(wrs::kRenderFrameParametersVersion);
    const bool has_parameters = json_member(object, "parameters") != nullptr;
    const bool has_ui_action = json_member(object, "ui_action") != nullptr;
    bool exact_keys;
    if (with_attributes) {
      if (has_parameters && has_ui_action)
        exact_keys = json_exact_keys(
            object, {"v", "type", "frame_index", "current_time", "parameters",
                     "ui_action"});
      else if (has_parameters)
        exact_keys = json_exact_keys(
            object, {"v", "type", "frame_index", "current_time", "parameters"});
      else if (has_ui_action)
        exact_keys = json_exact_keys(
            object, {"v", "type", "frame_index", "current_time", "ui_action"});
      else
        exact_keys = false;  // v:2 must carry at least one dynamic attribute.
    } else {
      exact_keys = json_exact_keys(object, {"v", "type", "frame_index", "current_time"});
    }
    if (type != "render_frame" || !exact_keys ||
        !json_i32(object, "frame_index", frame_index) || frame_index < 0 ||
        !time_value || !std::holds_alternative<JsonValue::Object>(time_value->value)) {
      outcome.protocol_violation = true;
      break;
    }
    const auto& time_object = std::get<JsonValue::Object>(time_value->value);
    int32_t current_time{};
    int32_t current_scale{};
    if (!json_exact_keys(time_object, {"value", "scale"}) ||
        !json_i32(time_object, "value", current_time) ||
        !json_i32(time_object, "scale", current_scale) || current_scale <= 0) {
      outcome.protocol_violation = true;
      break;
    }
    // The v:2 `parameters` attribute replaces the launch payload's assignments
    // for this frame only (protocol §4.2.1). The payload rides the message in
    // the argv encoding, ASCII only; a payload the broker's pre-send validation
    // would have rejected is a protocol violation, not a frame-local
    // diagnostic. The loop only produces the override; the launch payload
    // itself stays with the flavor wrappers, which fall back to it when this is
    // null.
    RequestedAssignments frame_assignments;
    const RequestedAssignments* frame_override = nullptr;
    if (has_parameters) {
      std::string parameters_text;
      std::wstring widened;
      bool widened_ok = json_string(object, "parameters", parameters_text);
      if (widened_ok) {
        widened.reserve(parameters_text.size());
        for (const unsigned char byte : parameters_text) {
          if (byte < 0x20 || byte > 0x7E) {
            widened_ok = false;
            break;
          }
          widened.push_back(static_cast<wchar_t>(byte));
        }
      }
      if (!widened_ok || !parse_parameter_payload(widened.c_str(), frame_assignments)) {
        outcome.protocol_violation = true;
        break;
      }
      frame_override = &frame_assignments;
    }
    // The v:2 `ui_action` attribute drives the one-shot custom-UI event
    // sequence for this frame only (protocol §4.2.1). Same grammar and bounds
    // as the argv trailer; a malformed value is a protocol violation.
    SessionUiAction frame_ui_action;
    const SessionUiAction* frame_ui = nullptr;
    if (has_ui_action) {
      std::string ui_action_text;
      if (!json_string(object, "ui_action", ui_action_text) ||
          !parse_session_ui_action(ui_action_text, frame_ui_action)) {
        outcome.protocol_violation = true;
        break;
      }
      frame_ui = &frame_ui_action;
    }
    const uint32_t expected_generation = static_cast<uint32_t>(frame_index) + 1;
    // Error responses carry no output or generation: a frame rejected before
    // or during rendering never updates the output slot, so there is no slot
    // metadata to report (protocol §4.3).
    const auto respond_error = [&](int32_t frame_error) {
      std::string reply = "{\"v\":1,\"type\":\"frame_done\",\"frame_index\":" +
          std::to_string(frame_index) + ",\"status\":\"error\",\"render_error\":" +
          std::to_string(frame_error) + "}";
      return channels.write_message(reply);
    };
    const auto respond_ok = [&](int32_t frame_width, int32_t frame_height,
                                int32_t frame_rowbytes, const std::string& checksum) {
      std::string reply;
      reply.reserve(256);
      reply += "{\"v\":1,\"type\":\"frame_done\",\"frame_index\":";
      reply += std::to_string(frame_index);
      reply += ",\"status\":\"ok\",\"output\":{\"width\":";
      reply += std::to_string(frame_width);
      reply += ",\"height\":";
      reply += std::to_string(frame_height);
      reply += ",\"rowbytes\":";
      reply += std::to_string(frame_rowbytes);
      reply += ",\"pixel_format\":\"";
      reply += pixel_format;
      reply += "\",\"checksum\":\"";
      reply += checksum;
      reply += "\",\"guards_intact\":true},\"render_error\":0,\"generation\":";
      reply += std::to_string(expected_generation);
      reply += "}";
      return channels.write_message(reply);
    };
    // A resize-output effect whose result overruns the launch output slot: the
    // worker cannot write it here, so it reports the required dimensions and the
    // broker re-opens the session with a larger slot (protocol §3, issue #261),
    // rather than the render falling back to the one-shot transport. No slot
    // write, no generation advance; the session stays usable.
    const auto respond_resize_needed = [&](int32_t frame_width, int32_t frame_height) {
      std::string reply = "{\"v\":1,\"type\":\"frame_done\",\"frame_index\":" +
          std::to_string(frame_index) + ",\"status\":\"resize_needed\",\"width\":" +
          std::to_string(frame_width) + ",\"height\":" + std::to_string(frame_height) +
          "}";
      return channels.write_message(reply);
    };
    if (static_cast<uint32_t>(current_scale) != time_scale) {
      // Frame-local diagnostic: the session continues, the broker decides.
      if (!respond_error(kSessionTimeScaleMismatch)) {
        outcome.protocol_violation = true;
        break;
      }
      continue;
    }
    if (current_time < 0 || current_time > total_time) {
      // Same range contract the one-shot parser enforces on its launch time:
      // frames outside the declared timeline are rejected before rendering.
      if (!respond_error(kSessionTimeOutOfRange)) {
        outcome.protocol_violation = true;
        break;
      }
      continue;
    }
    if (channels.read_header_u32(wrs::kHeaderInputGenerationOffset) !=
            expected_generation ||
        !channels.static_header_matches(geometry)) {
      // Stale slot or mutated header: host-protection invariant, fail closed.
      respond_error(kSessionGenerationMismatch);
      outcome.invariant_failure = true;
      break;
    }
    std::memcpy(frame_rgba.data(), channels.view() + input_offset, frame_rgba.size());
    outcome.frames_attempted += 1;
    if (!sequence_started) {
      write<int32_t>(input, 224, current_time);
      sequence_attempted = true;
      outcome.setup_error = invoke_sequence_selector(entry, kSequenceSetup,
                                                     input.data(), output.data());
      if (outcome.setup_error != 0) {
        // The session can never render; answer with the reserved
        // continuation-impossible code so the broker invalidates instead of
        // treating this as a reusable frame-local diagnostic. The plug-in's
        // setup error itself reaches the final report.
        respond_error(kSessionSequenceSetupFailed);
        break;
      }
      sequence_started = true;
      // Mirrors begin_render for the hoisted sequence: an --aux-manifest-v1
      // manifest becomes visible to the channel suite once SEQUENCE_SETUP
      // succeeds, and stays active for every session frame.
      activate_external_aux();
      write<void*>(input, kInSequenceData, read<void*>(output, kOutSequenceData));
    }
    captured.clear();
    const SessionFrameOutput frame =
        render_frame(current_time, frame_rgba, captured, frame_layers, frame_override,
                     frame_ui);
    // Aux channel chunks are host-owned and cannot outlive one frame's render
    // lifecycle (end_render's cleanup for the one-shot path); the manifest
    // itself stays active across frames.
    aexcompat::pf_ae_channel::reclaim_layer_channels();
    outcome.width = frame.width;
    outcome.height = frame.height;
    outcome.rowbytes = frame.rowbytes;
    outcome.input_hash = frame.input_hash;
    outcome.output_hash = frame.output_hash;
    // Corruption evidence outranks the render error: a plug-in that wrote
    // outside its guarded private buffer invalidates the session even when it
    // also reported a nonzero error. Host-protection invariant, fail closed.
    if (frame.guard_violation) {
      outcome.guards_intact = false;
      respond_error(kSessionGuardViolation);
      outcome.invariant_failure = true;
      break;
    }
    // Host-side output validation failures (rejected or failed resize) share
    // numeric codes with selector errors, so the dispatch owner reports them
    // out of band; they are output-bounds invariant failures, not frame-local
    // diagnostics (protocol §4.3).
    if (frame.output_validation_failed) {
      respond_error(kSessionOutputValidationError);
      outcome.invariant_failure = true;
      break;
    }
    if (frame.frame_error != 0) {
      // Frame-local compatibility diagnostic; the sequence state is still
      // owned by the host, so the session may continue.
      if (!respond_error(frame.frame_error)) {
        outcome.protocol_violation = true;
        break;
      }
      continue;
    }
    // A resize-output effect may render at dimensions other than the launch max
    // (protocol §3, issue #261). The captured pixels must match the reported
    // frame dimensions exactly (a mismatch is a capture invariant failure), and
    // must be positive.
    if (frame.width <= 0 || frame.height <= 0) {
      respond_error(kSessionDimensionMismatch);
      outcome.invariant_failure = true;
      break;
    }
    const std::size_t expected_pixels =
        static_cast<std::size_t>(frame.width) * frame.height;
    if (captured.size() != expected_pixels * pixel_bytes) {
      respond_error(kSessionOutputCaptureError);
      outcome.invariant_failure = true;
      break;
    }
    // Shrink, or an expand that still fits the launch output slot, is written at
    // its actual dimensions. An expand that overruns the slot cannot be written
    // here: report resize_needed so the broker re-opens at these dimensions
    // (the session stays usable; no slot write, no generation advance).
    if (captured.size() > wrs::output_slot_bytes(geometry)) {
      if (!respond_resize_needed(frame.width, frame.height)) {
        outcome.protocol_violation = true;
        break;
      }
      continue;
    }
    unsigned char* slot = channels.view() + output_offset;
    for (std::size_t pixel = 0; pixel < expected_pixels; ++pixel)
      argb_to_rgba_native(slot + pixel * pixel_bytes,
                          captured.data() + pixel * pixel_bytes, pixel_bytes);
    // Mirrors the one-shot finalize checksum hook (gated on the opt-in aux
    // option): detail is computed from the same transferred RGBA bytes the
    // broker reads. The final report carries the last rendered frame's
    // detail; frame_done stays the per-frame truth.
    record_output_checksum_detail(slot, frame.width, frame.height, pixel_bytes);
    channels.write_header_u32(wrs::kHeaderFrameWidthOffset,
                              static_cast<uint32_t>(frame.width));
    channels.write_header_u32(wrs::kHeaderFrameHeightOffset,
                              static_cast<uint32_t>(frame.height));
    channels.write_header_u32(wrs::kHeaderOutputGenerationOffset,
                              expected_generation);
    // The frame checksum covers the transferred slot bytes, the exact bytes
    // the broker reads; the final report's output_hash keeps the one-shot
    // internal-ARGB definition (protocol §4.3).
    const std::string slot_checksum = sha256_bytes(slot, captured.size());
    if (!respond_ok(frame.width, frame.height, frame.rowbytes, slot_checksum)) {
      outcome.protocol_violation = true;
      break;
    }
  }
  if (sequence_started) {
    outcome.setdown_error =
        invoke_sequence_selector(entry, kSequenceSetdown, input.data(), output.data());
    write<void*>(input, kInSequenceData, nullptr);
  } else if (!sequence_attempted) {
    // No frame ever reached rendering, so there is no sequence to tear
    // down; an empty session is vacuously clean. (An attempted-but-failed
    // setup keeps its error and the missing setdown fails the session.)
    outcome.setup_error = 0;
    outcome.setdown_error = 0;
  }
  aexcompat::pf_ae_channel::reclaim_layer_channels();
  deactivate_external_aux();
  clear_native_aux_provider();
  // Frame-local errors were already reported through frame_done and the
  // broker owned the continue/stop decision, so a clean close after them is
  // still a successful session; only session mechanics count here.
  outcome.render_error =
      outcome.setup_error == 0 && outcome.setdown_error == 0 &&
              !outcome.protocol_violation && !outcome.invariant_failure
          ? 0
          : -1;
  return outcome;
}

// Classic resident session: render_once(manage_sequence=false) per frame
// under the hoisted sequence, the persistent_sequence precedent.
RenderSessionOutcome run_render_session(
    EffectEntry entry, std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, const RequestedAssignments* requested,
    int32_t max_width, int32_t max_height, int32_t time_step, int32_t total_time,
    uint32_t time_scale, int32_t pixel_bytes,
    const std::vector<ExternalLayerInput>* external_layers) {
  return run_session_frame_loop(
      entry, input, output, max_width, max_height, time_step, total_time,
      time_scale, pixel_bytes, external_layers,
      [&](int32_t current_time, const std::vector<unsigned char>& frame_rgba,
          std::vector<unsigned char>& captured,
          const std::vector<ExternalLayerInput>* frame_layers,
          const RequestedAssignments* frame_override,
          const SessionUiAction* frame_ui) {
        apply_session_ui_action(frame_ui);
        SessionFrameOutput frame;
        // Initialized true so it means "finalize observed corruption" when
        // false: render_once's early host-side failures return before touching
        // it, while every path that allocates the guarded buffer overwrites it.
        bool frame_guards = true;
        bool output_validation_failed = false;
        frame.frame_error = render_once(
            entry, input, output, "request", frame.width, frame.height,
            frame.rowbytes, frame.input_hash, frame.output_hash, frame_guards,
            frame_override ? frame_override : requested, &frame_rgba, nullptr,
            max_width, max_height, frame_layers,
            current_time, time_step, total_time, time_scale, pixel_bytes, false,
            &captured, &output_validation_failed);
        frame.guard_violation = !frame_guards;
        frame.output_validation_failed = output_validation_failed;
        return frame;
      });
}

// SmartFX resident session frame loop (protocol v1.1): each frame runs
// PreRender→SmartRender (and any GPU device setup/setdown the plan selects)
// inside the per-frame FRAME pair, while the shared loop above owns the
// hoisted SEQUENCE pair, transport, and fail-closed decisions. The GPU
// backend is fixed at launch through the session command word's case_id.
// Smart sessions carry no layer slots in v1.1 (the broker rejects the
// combination at open), so the loop's frame_layers stay unused here.
SmartRenderSessionOutcome run_smart_render_session(
    EffectEntry entry, std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, const RequestedAssignments* requested,
    const std::string& case_id, int32_t max_width, int32_t max_height,
    int32_t time_step, int32_t total_time, uint32_t time_scale,
    int32_t pixel_bytes) {
  SmartRenderSessionOutcome outcome;
  outcome.session = run_session_frame_loop(
      entry, input, output, max_width, max_height, time_step, total_time,
      time_scale, pixel_bytes, nullptr,
      [&](int32_t current_time, const std::vector<unsigned char>& frame_rgba,
          std::vector<unsigned char>& captured,
          const std::vector<ExternalLayerInput>*,
          const RequestedAssignments* frame_override,
          const SessionUiAction* frame_ui) {
        apply_session_ui_action(frame_ui);
        SessionFrameOutput frame;
        worker_runtime::smart_execution::SessionFrame session_frame{&captured};
        const worker_runtime::smart_execution::Result frame_result = smart_render_once(
            entry, input, output, case_id, frame_override ? frame_override : requested,
            &frame_rgba, nullptr,
            max_width, max_height, nullptr, current_time, time_step, total_time,
            time_scale, pixel_bytes, &session_frame);
        outcome.last = frame_result;
        frame.width = frame_result.output_width;
        frame.height = frame_result.output_height;
        frame.rowbytes = frame_result.output_rowbytes;
        frame.input_hash = frame_result.input_hash;
        frame.output_hash = frame_result.output_hash;
        // Sentinel evidence only exists once the guarded output buffer was
        // built; refusals before that point are frame-local diagnostics, not
        // corruption.
        frame.guard_violation =
            session_frame.output_buffer_allocated && !session_frame.guards_intact;
        // The per-frame diagnostic keeps the one-shot error priority: GPU
        // device setup, then PreRender, then the render/finalize error, then
        // GPU device setdown. ROI/rect diagnostics stay in the final report;
        // host-protection stays with the shared loop's geometry checks.
        frame.frame_error = frame_result.gpu_setup_error != 0
            ? frame_result.gpu_setup_error
            : frame_result.pre_error != 0
                ? frame_result.pre_error
                : frame_result.render_error != 0 ? frame_result.render_error
                                                 : frame_result.gpu_setdown_error;
        return frame;
      });
  return outcome;
}

}  // namespace aexcompat::l2_detail
