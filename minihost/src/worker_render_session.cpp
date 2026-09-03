#include "worker_render_session.hpp"

#include "worker_bee_scene_facade.hpp"

#include <windows.h>

#include "render_pixel_transport.hpp"
#include "render_subsystem.h"
#include "strict_json.hpp"
#include "worker_classic_render_entry.hpp"
#include "worker_invocation_orchestration.hpp"
#include "worker_pf_ae_channel_runtime.hpp"
#include "worker_request_parser.hpp"
#include "worker_selector_dispatch.hpp"
#include "worker_smart_dispatch.hpp"
#include "worker_smart_execution.hpp"
#include "worker_smart_setup.hpp"
#include "worker_ui_event_execution.hpp"

#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
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
  const int32_t capacity_width = geometry.output_capacity_width > 0
      ? geometry.output_capacity_width : geometry.max_width;
  const int32_t capacity_height = geometry.output_capacity_height > 0
      ? geometry.output_capacity_height : geometry.max_height;
  return static_cast<std::size_t>(capacity_width) * capacity_height *
         geometry.output_pixel_bytes;
}

std::size_t input_slot_offset() { return kHeaderBytes; }

std::size_t output_slot_offset(const SessionGeometry& geometry) {
  return kHeaderBytes + align_slot(input_slot_bytes(geometry));
}

std::size_t expected_section_bytes(const SessionGeometry& geometry) {
  // Header + input + output only (#268); layer pixels travel as inherited file
  // HANDLEs, so nothing beyond the output slot lives in the section.
  return output_slot_offset(geometry) + align_slot(output_slot_bytes(geometry));
}

bool session_environment_requested() {
  return variable_present(kRequestVariable) ||
         variable_present(kResponseVariable) ||
         variable_present(kSectionVariable);
}

SessionChannels::~SessionChannels() {
  close();
}

void SessionChannels::close() {
  if (view_) UnmapViewOfFile(view_);
  view_ = nullptr;
  if (section_) CloseHandle(section_);
  section_ = nullptr;
  if (request_pipe_) CloseHandle(request_pipe_);
  request_pipe_ = nullptr;
  if (response_pipe_) CloseHandle(response_pipe_);
  response_pipe_ = nullptr;
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

bool SessionChannels::adopt_grown_section(unsigned long long section_handle_value,
                                          const SessionGeometry& new_geometry) {
  if (!opened() || section_handle_value == 0) return false;
  const HANDLE grown =
      reinterpret_cast<HANDLE>(static_cast<uintptr_t>(section_handle_value));
  DWORD flags = 0;
  if (!GetHandleInformation(grown, &flags)) return false;
  void* view = MapViewOfFile(grown, FILE_MAP_ALL_ACCESS, 0, 0, 0);
  if (!view) return false;
  MEMORY_BASIC_INFORMATION region{};
  if (VirtualQuery(view, &region, sizeof(region)) != sizeof(region) ||
      region.RegionSize < expected_section_bytes(new_geometry)) {
    UnmapViewOfFile(view);
    return false;
  }
  // Adopt only after the grown mapping is validated: unmap and close the
  // previous section, then take the grown one (the broker duplicated its handle
  // into this process). On any earlier failure the previous section stays live.
  if (view_) UnmapViewOfFile(view_);
  if (section_) CloseHandle(section_);
  section_ = grown;
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
         read_header_u32(kHeaderVersionOffset) == kSessionHeaderVersion &&
         read_header_u32(kHeaderDepthCodeOffset) == depth_code &&
         read_header_u32(kHeaderMaxWidthOffset) ==
             static_cast<uint32_t>(geometry.max_width) &&
         read_header_u32(kHeaderMaxHeightOffset) ==
             static_cast<uint32_t>(geometry.max_height) &&
         read_header_u32(kHeaderLayerSlotCountOffset) ==
             static_cast<uint32_t>(geometry.layer_slot_count);
}

SessionPipes::~SessionPipes() {
  if (request_pipe_) CloseHandle(request_pipe_);
  if (response_pipe_) CloseHandle(response_pipe_);
}

bool SessionPipes::open_from_environment() {
  if (opened()) return false;
  const HANDLE request = pipe_from_variable(kRequestVariable);
  const HANDLE response = pipe_from_variable(kResponseVariable);
  if (!request || !response) return false;
  request_pipe_ = request;
  response_pipe_ = response;
  return true;
}

SessionPipes::ReadResult SessionPipes::read_message(std::string& payload) {
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

bool SessionPipes::write_message(const std::string& payload) {
  if (!response_pipe_ || payload.empty() || payload.size() > max_write_bytes_)
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

}  // namespace aexcompat::worker_render_session

// Cross-TU declarations into worker_main's private renderers and state
// (issue #169, same shape as worker_invocation_orchestration.cpp). The
// constants mirror l2_main's frozen protocol offsets and selector codes;
// every definition stays in l2_main.
namespace aexcompat::l2_detail {

// Owned by worker_l2_shared_helpers.cpp.
std::string escape(const std::string& input);

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
worker_runtime::smart_execution::Result smart_render_once(
    EffectEntry entry, std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, const std::string& case_id,
    const RequestedAssignments* requested = nullptr,
    const std::vector<unsigned char>* external_rgba = nullptr,
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

// Decodes the v:2 `ui_action` string. The grammar came from the one-shot
// custom-UI argv trailer (deleted in #365) and is unchanged:
// "click:v1|x|y|r|g|b|a" or "draw:v1", with x/y in [0,8192] and each color
// component finite in [0,1]. Returns false on any malformed value, which the
// caller escalates to a protocol violation (the broker validates the same
// grammar before sending, so a bad value is a defect or tampering).
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
// telemetry the render path reads (the singleton the deleted one-shot
// ApplyHooks setters drove from argv). The per-frame semantic is a complete
// replacement (protocol §4.2.1): the enabled flags are cleared first so a frame
// with no ui_action renders with no custom-UI event, then set from this frame's
// action. Frame N's click never leaks into frame N+1.
void apply_session_ui_action(const SessionUiAction* action) {
  namespace ui = worker_runtime::ui_event_execution;
  ui::CustomUiTelemetry& telemetry = ui::custom_ui_telemetry();
  // Reset the per-render custom-UI observation before every frame so a
  // multi-frame session starts each frame from the same state a fresh worker
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
  ui::reset_info_text_telemetry();
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
  // True only when a Smart selector returned success but the host's guarded
  // output remained invalid/untouched. This is distinct from a plug-in
  // returning the same numeric -6 itself.
  bool smart_output_untouched{false};
  int32_t width{0};
  int32_t height{0};
  int32_t rowbytes{0};
  // Where the frame sits relative to the layer's own origin. A SmartFX
  // effect that grows its output - a glow reaching past the layer - answers
  // with a result_rect whose top-left is negative, and the pixels start
  // there rather than at the layer's (0,0). A caller that has to place the
  // frame back into a fixed-size image needs this to know which part of it
  // covers the layer (issue #914). A Classic effect that expands its buffer
  // states the same geometry the other way round - PF_OutData::origin is where
  // the input's (0,0) landed inside the grown output, so positive - and the
  // classic callback negates it into this field rather than reporting it raw
  // (issue #984). Zero when nothing resized, which is where the output starts.
  int32_t origin_x{0};
  int32_t origin_y{0};
  std::string input_hash;
  std::string output_hash;
  bool guard_violation{false};
  bool output_validation_failed{false};
  // A SmartFX frame whose PreRender returned a legally empty result_rect (#278):
  // the render selector was skipped and there are no output pixels. This is a
  // valid contract the one-shot path reports as a zero-dimension output, so the
  // session must report it as a valid empty frame instead of a dimension
  // invariant failure. Only smart frames ever set this; classic frames leave it
  // false and a zero dimension stays an invariant failure.
  bool empty_result{false};
};

// Resident render session frame loop (docs/RENDER_SESSION_PROTOCOL_2026-07-19.md).
// SEQUENCE_SETUP is hoisted once around per-frame renders following the
// persistent_sequence precedent; pixels move through the inherited anonymous
// section (copy-through slots, the plug-in never sees the mapping) and control
// messages over the inherited pipe pair with strict exact-key validation.
// render_frame(current_entry, current_time, frame_rgba, captured, frame_layers,
// frame_override, frame_ui) runs one frame under the hoisted sequence and
// returns the SessionFrameOutput above with the packed ARGB output in
// `captured`. frame_override is a v:2 per-frame parameter set (protocol
// §4.2.1) or null; the wrappers fall back to their launch payload when it is
// null. frame_ui is a v:2 per-frame custom-UI action (§4.2.1) or null; the
// callback drives the click/draw event sequence for that frame only.
// RenderSessionOutcome is defined in worker_invocation_orchestration.hpp so
// the final dispatch owner can call the wrappers below across TUs.
template <typename FrameFn>
void run_session_frame_loop(
    RenderSessionOutcome& outcome, EffectEntry entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output,
    int32_t max_width, int32_t max_height, int32_t output_capacity_width,
    int32_t output_capacity_height, int32_t time_step, int32_t total_time,
    uint32_t time_scale, int32_t pixel_bytes,
    const std::vector<ExternalLayerInput>* external_layers, FrameFn&& render_frame,
    const aexcompat::worker_render_session::SwapPluginHook* swap_hook = nullptr,
    bool audio_passthrough = false) {
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
  // A ui_action frame sent to an AUDIO_EFFECT_ONLY passthrough session
  // (issue #1048): the event sequence has no render to ride on, and
  // swallowing it silently would hide the gap. Frame-local; the session
  // continues.
  constexpr int32_t kSessionUiActionUnsupported = -48;

  const int32_t layer_slot_count =
      external_layers ? static_cast<int32_t>(external_layers->size()) : 0;
  // Non-const: an in-session grow (protocol §3, issue #262) raises the output
  // capacity in place when an expand overruns the launch slot, without changing
  // the render dimensions (max_width/max_height) or the input/output offsets.
  // The section holds only header + input + output (#268); layer pixels no
  // longer occupy it, so no layer region feeds the geometry.
  wrs::SessionGeometry geometry{
      max_width, max_height,
      output_capacity_width > 0 ? output_capacity_width : max_width,
      output_capacity_height > 0 ? output_capacity_height : max_height,
      pixel_bytes, layer_slot_count};
  wrs::SessionChannels channels;
  if (!channels.open_from_environment(geometry) ||
      !channels.static_header_matches(geometry)) {
    outcome.protocol_violation = true;
    return;
  }
  // Refills every dynamic layer's private vector from its retained handle
  // (issue #674). Returns false on a short read or an unreadable handle, which
  // the caller turns into the same fail-closed protocol violation a bad layer
  // is at open: rendering a frame against half-updated pixels would be a
  // silently wrong image rather than a diagnostic.
  const auto refresh_dynamic_layers = [](std::vector<ExternalLayerInput>& layers) {
    for (auto& layer : layers) {
      if (!layer.dynamic || layer.rgba_handle == 0) continue;
      const HANDLE handle =
          reinterpret_cast<HANDLE>(static_cast<uintptr_t>(layer.rgba_handle));
      if (SetFilePointer(handle, 0, nullptr, FILE_BEGIN) == INVALID_SET_FILE_POINTER)
        return false;
      std::size_t collected = 0;
      while (collected < layer.rgba.size()) {
        DWORD read = 0;
        const DWORD request = static_cast<DWORD>(layer.rgba.size() - collected);
        if (!ReadFile(handle, layer.rgba.data() + collected, request, &read, nullptr) ||
            read == 0)
          return false;
        collected += read;
      }
    }
    return true;
  };
  // Layers are static for the whole session and travel as inherited per-layer
  // read HANDLEs (#268), not section slots: read each layer's RGBA8 once from
  // its handle into a worker-private vector the render loop reuses (the plug-in
  // never sees a mapping), then close the handle. A short read, a zero-size
  // layer, or a handle that is not an inherited disk file is a fail-closed
  // launch error.
  std::vector<ExternalLayerInput> session_layers;
  if (external_layers) {
    session_layers = *external_layers;
    for (int32_t index = 0; index < layer_slot_count; ++index) {
      auto& layer = session_layers[index];
      const std::size_t bytes =
          static_cast<std::size_t>(layer.width) * layer.height * 4;
      const HANDLE handle =
          reinterpret_cast<HANDLE>(static_cast<uintptr_t>(layer.rgba_handle));
      DWORD flags = 0;
      if (bytes == 0 || layer.rgba_handle == 0 ||
          !GetHandleInformation(handle, &flags) ||
          GetFileType(handle) != FILE_TYPE_DISK) {
        outcome.protocol_violation = true;
        return;
      }
      layer.rgba.resize(bytes);
      std::size_t collected = 0;
      bool read_ok = true;
      while (collected < bytes) {
        DWORD read = 0;
        const DWORD request = static_cast<DWORD>(bytes - collected);
        if (!ReadFile(handle, layer.rgba.data() + collected, request, &read,
                      nullptr) ||
            read == 0) {
          read_ok = false;
          break;
        }
        collected += read;
      }
      if (layer.dynamic) {
        // Kept open on purpose (issue #674): the broker rewrites this file
        // between frames, and the frame loop below re-reads it. Everything
        // else about the layer - slot, geometry, the private vector the
        // plug-in sees - is unchanged.
        SetFilePointer(handle, 0, nullptr, FILE_BEGIN);
      } else {
        CloseHandle(handle);
        layer.rgba_handle = 0;  // consumed; never reused
      }
      if (!read_ok || collected != bytes) {
        outcome.protocol_violation = true;
        return;
      }
    }
  }
  const std::vector<ExternalLayerInput>* frame_layers =
      session_layers.empty() ? nullptr : &session_layers;
  // Static in_data geometry and timing fields for the whole session. The
  // per-frame current_time is seeded when SEQUENCE_SETUP actually runs:
  // setup is deferred to the first rendered frame so effects that
  // initialize persistent sequence state from in_data->current_time observe
  // that frame's time, exactly like the one-shot render lifecycle seeds the
  // requested time before SEQUENCE_SETUP. Hoisted into a lambda because a
  // cluster-session swap re-bootstraps the buffers: the static fields must be
  // re-applied before the swapped plug-in's first frame.
  const auto write_session_static_fields = [&] {
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
  };
  write_session_static_fields();
  bool sequence_attempted = false;
  bool sequence_started = false;
  // Cluster-session swap state (closure-session design §4.1): the manifest
  // index of the plug-in currently loaded, and whether its GLOBAL_SETUP /
  // PARAMS_SETUP failed (plug-in-local; frames get the
  // continuation-impossible response while the session stays alive for the
  // broker's continue/stop decision).
  int32_t current_plugin_index = swap_hook ? 0 : -1;
  bool current_audio_passthrough = audio_passthrough;
  bool swapped_plugin_setup_failed = false;

  const std::size_t input_offset = wrs::input_slot_offset();
  const std::size_t output_offset = wrs::output_slot_offset(geometry);
  const char* pixel_format = pixel_bytes == 16 ? "argb32f" :
      (pixel_bytes == 8 ? "argb16" : "argb8");
  std::vector<unsigned char> frame_rgba(wrs::input_slot_bytes(geometry));
  std::vector<unsigned char> captured;
  std::string message;
  // In-session output-slot grow (protocol §3, issue #262). After the worker
  // reports resize_needed for an expand that overran the launch slot, the broker
  // duplicates a larger anonymous section into this process and replies with a
  // `grow` control message carrying the handle value and new capacity. This
  // adopts the grown section (updating `geometry`), so the same worker transfers
  // the already-rendered frame into the larger slot. The render lifecycle
  // (SEQUENCE/FRAME setup, RENDER, setdown) is not re-run: it already completed
  // once into the private buffer, exactly like the one-shot path. `required`
  // is the byte count of the rendered output that must fit the grown slot.
  const auto grow_output_slot = [&](std::size_t required) -> bool {
    std::string grow_message;
    if (channels.read_message(grow_message) !=
        wrs::SessionChannels::ReadResult::Message)
      return false;
    JsonValue grow_root;
    if (!StrictJsonParser(std::move(grow_message)).parse(grow_root) ||
        !std::holds_alternative<JsonValue::Object>(grow_root.value))
      return false;
    const auto& grow_object = std::get<JsonValue::Object>(grow_root.value);
    int32_t grow_version{};
    std::string grow_type;
    std::string handle_text;
    int32_t capacity_width{};
    int32_t capacity_height{};
    if (!json_exact_keys(grow_object,
                         {"v", "type", "section_handle",
                          "output_capacity_width", "output_capacity_height"}) ||
        !json_i32(grow_object, "v", grow_version) ||
        grow_version != static_cast<int32_t>(wrs::kProtocolVersion) ||
        !json_string(grow_object, "type", grow_type) || grow_type != "grow" ||
        !json_string(grow_object, "section_handle", handle_text) ||
        !json_i32(grow_object, "output_capacity_width", capacity_width) ||
        !json_i32(grow_object, "output_capacity_height", capacity_height) ||
        capacity_width <= 0 || capacity_height <= 0)
      return false;
    // Parse the duplicated section handle value (decimal, strict tail).
    char* tail = nullptr;
    const unsigned long long handle_value =
        std::strtoull(handle_text.c_str(), &tail, 10);
    if (!tail || *tail != '\0' || handle_value == 0) return false;
    // The granted capacity must cover the rendered pixels; the broker bounds it
    // too, this is defense in depth against a slot copy overrunning the section.
    wrs::SessionGeometry grown = geometry;
    grown.output_capacity_width = capacity_width;
    grown.output_capacity_height = capacity_height;
    if (wrs::output_slot_bytes(grown) < required) return false;
    if (!channels.adopt_grown_section(handle_value, grown) ||
        !channels.static_header_matches(grown))
      return false;
    geometry = grown;
    return true;
  };
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
    // Cluster-session plug-in swap (closure-session design §4.1): only an
    // authenticated manifest index travels here; the path/hash stay with the
    // launch-time trust decision. Without a cluster hook the message type is
    // unknown and stays a protocol violation.
    if (type == "swap_plugin") {
      int32_t plugin_index{};
      if (version != static_cast<int32_t>(wrs::kProtocolVersion) ||
          !json_exact_keys(object, {"v", "type", "plugin_index"}) ||
          !json_i32(object, "plugin_index", plugin_index) ||
          !swap_hook || !swap_hook->invoke || plugin_index < 0 ||
          plugin_index >= swap_hook->plugin_count ||
          plugin_index == current_plugin_index) {
        outcome.protocol_violation = true;
        break;
      }
      // The outgoing plug-in's words do not belong to the incoming one
      // (issue #707).
      aexcompat::worker_runtime::reset_selector_return_message();
      // 1. SEQUENCE_SETDOWN when a sequence is up (design §4.1 step 1).
      if (sequence_started) {
        const int32_t sequence_setdown_error = invoke_sequence_selector(
            entry, kSequenceSetdown, input.data(), output.data());
        write<void*>(input, kInSequenceData, nullptr);
        sequence_started = false;
        sequence_attempted = false;
        if (sequence_setdown_error != 0) {
          // Setdown failure leaves possibly-contaminated plug-in state; the
          // session cannot safely continue (design §4.1 step 3).
          outcome.swap_failure = true;
          break;
        }
      }
      // 2.-6. GLOBAL_SETDOWN, quiescence, plug-in-only unload, authenticated
      // load of plugins[N], GLOBAL_SETUP/PARAMS_SETUP, and the payload swap
      // all belong to the dispatch owner, which holds the WorkerSession, the
      // bootstrap wiring, and the launch payload parser. The BEE facade's
      // attribution window (issue #1264) is opened inside that hook, between
      // the outgoing plug-in's unload and the incoming plug-in's bootstrap,
      // so each side's facade calls count against the plug-in that made them.
      wrs::SwapPluginResult swap =
          swap_hook->invoke(swap_hook->context, plugin_index);
      if (swap.hard_failure || !swap.entry) {
        outcome.swap_failure = true;
        break;
      }
      entry = swap.entry;
      current_plugin_index = plugin_index;
      current_audio_passthrough = swap.audio_effect_only;
      swapped_plugin_setup_failed =
          swap.global_setup_error != 0 || swap.params_setup_error != 0;
      // Re-apply the session-static in_data fields the re-bootstrap cleared.
      write_session_static_fields();
      std::string reply;
      if (swap.global_setup_error == 0) {
        reply = "{\"v\":1,\"type\":\"swap_done\",\"plugin_index\":" +
            std::to_string(plugin_index) + ",\"status\":\"ok\"}";
      } else {
        // Plug-in-local GLOBAL_SETUP failure: structured swap_done error, the
        // session stays alive and later frames answer with the
        // continuation-impossible code (design §4.1).
        reply = "{\"v\":1,\"type\":\"swap_done\",\"plugin_index\":" +
            std::to_string(plugin_index) +
            ",\"status\":\"error\",\"global_setup_error\":" +
            std::to_string(swap.global_setup_error) + "}";
      }
      if (!channels.write_message(reply)) {
        outcome.protocol_violation = true;
        break;
      }
      continue;
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
    // Each frame starts with no message: what the plug-in said about a previous
    // frame is not this frame's diagnosis (issue #707).
    aexcompat::worker_runtime::reset_selector_return_message();
    const uint64_t seh_sequence_at_frame_start =
        aexcompat::worker_runtime::selector_dispatch_telemetry().seh_sequence;
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
    const auto respond_error = [&](int32_t frame_error,
                                   bool smart_output_untouched = false) {
      std::string reply = "{\"v\":1,\"type\":\"frame_done\",\"frame_index\":" +
          std::to_string(frame_index) + ",\"status\":\"error\",\"render_error\":" +
          std::to_string(frame_error);
      if (smart_output_untouched)
        reply += ",\"smart_output_untouched\":true";
      const auto& telemetry =
          aexcompat::worker_runtime::selector_dispatch_telemetry();
      const auto& missing = telemetry.missing_dependency;
      if (!missing.empty()) reply += ",\"missing_dependency\":\"" + missing + "\"";
      // A selector SEH used to be flattened into kAuditFailure == 512, which
      // collides with PF_Err_INTERNAL_STRUCT_DAMAGED. Carry a frame-local,
      // structured discriminator instead. The sequence comparison prevents a
      // crash from startup or a previous frame being attached to this error,
      // and the error check keeps the field meaning exactly "this 512 is the
      // host's substitute for a fault": a fault this frame that the reported
      // error did not come from (a GPU setdown or custom-UI event fault behind
      // an untouched-output -6, say) stays on the always-on
      // `stage:selector_seh` stderr line (issue #1212) rather than being
      // attached to a number it does not explain (issue #983).
      constexpr int32_t kSelectorFaultSubstitute = 512;  // kAuditFailure
      if (frame_error == kSelectorFaultSubstitute &&
          telemetry.seh_sequence != seh_sequence_at_frame_start &&
          telemetry.seh_code != 0 && !telemetry.selector.empty()) {
        reply += ",\"selector_crash\":{\"selector\":\"" +
            escape(telemetry.selector) + "\",\"exception_code\":" +
            std::to_string(telemetry.seh_code) + "}";
      }
      // What the plug-in itself said about the failure. The SDK writes
      // "Couldn't load suite." here when a suite is missing, and plug-ins write
      // their own reason, so this is often the whole diagnosis (issue #707).
      if (!telemetry.return_message.empty()) {
        reply += ",\"return_message\":{\"selector\":\"" +
            escape(telemetry.return_message.selector) + "\",\"text\":\"" +
            escape(telemetry.return_message.text) + "\",\"error\":" +
            std::to_string(telemetry.return_message.error) +
            ",\"display_requested\":" +
            (telemetry.return_message.display_requested ? "true" : "false") + "}";
      }
      reply += "}";
      return channels.write_message(reply);
    };
    const auto respond_ok = [&](int32_t frame_width, int32_t frame_height,
                                int32_t frame_rowbytes, std::size_t packed_bytes,
                                int32_t frame_origin_x, int32_t frame_origin_y) {
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
      // The byte count the worker actually packed into the slot. The broker
      // computes the same extent from the reported dimensions and refuses a
      // disagreement, which is the layout cross-check the per-frame SHA-256
      // used to carry (issue #690).
      reply += "\",\"packed_bytes\":";
      reply += std::to_string(packed_bytes);
      reply += ",\"origin_x\":";
      reply += std::to_string(frame_origin_x);
      reply += ",\"origin_y\":";
      reply += std::to_string(frame_origin_y);
      reply += ",\"guards_intact\":true},\"render_error\":0,\"generation\":";
      reply += std::to_string(expected_generation);
      reply += "}";
      return channels.write_message(reply);
    };
    // A SmartFX frame whose PreRender returned a legally empty result_rect (#278):
    // no pixels were rendered. Report a valid zero-dimension ok frame carrying
    // an explicit "empty_result":true so the broker accepts the empty geometry
    // (a zero dimension without this flag stays a dimension invariant failure).
    const auto respond_empty = [&]() {
      std::string reply;
      reply.reserve(256);
      reply += "{\"v\":1,\"type\":\"frame_done\",\"frame_index\":";
      reply += std::to_string(frame_index);
      reply += ",\"status\":\"ok\",\"output\":{\"width\":0,\"height\":0,\"rowbytes\":0,";
      reply += "\"pixel_format\":\"";
      reply += pixel_format;
      reply += "\",\"packed_bytes\":0,\"guards_intact\":true,\"empty_result\":true},";
      reply += "\"render_error\":0,\"generation\":";
      reply += std::to_string(expected_generation);
      reply += "}";
      return channels.write_message(reply);
    };
    // A resize-output effect whose result overruns the launch output slot: the
    // worker cannot write it here, so it reports the required dimensions and the
    // broker grows the shared section in place (protocol §3, issue #262), after
    // which the same worker transfers the frame, rather than the render falling
    // back to the one-shot transport. No slot write, no generation advance; the
    // session stays usable.
    const auto respond_resize_needed = [&](int32_t frame_width, int32_t frame_height) {
      std::string reply = "{\"v\":1,\"type\":\"frame_done\",\"frame_index\":" +
          std::to_string(frame_index) + ",\"status\":\"resize_needed\",\"width\":" +
          std::to_string(frame_width) + ",\"height\":" + std::to_string(frame_height) +
          ",\"render_error\":0}";
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
    // A swapped-in plug-in whose GLOBAL/PARAMS setup failed can never render;
    // answer every frame with the reserved continuation-impossible code and
    // let the broker own the stop decision (closure-session design §4.1).
    if (swapped_plugin_setup_failed) {
      if (!respond_error(kSessionSequenceSetupFailed)) {
        outcome.protocol_violation = true;
        break;
      }
      continue;
    }
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
    // AUDIO_EFFECT_ONLY passthrough (issue #1048): an audio-only effect has no
    // video selector to dispatch, and AE leaves its video untouched, so the
    // frame is the input. The input slot is RGBA8 transport at every session
    // depth, so a deep session expands each channel with the same scaling
    // `build_argb_input` uses for the render input; the output slot is native
    // RGBA at the session depth either way. The sequence lifecycle above
    // still runs - AE opens a sequence for every applied effect, and a setup
    // failure stays the genuine diagnostic it is for any other plug-in. No
    // FRAME pair, no layers, no capture buffer.
    if (current_audio_passthrough) {
      // A custom-UI event is an explicit broker-requested action, not a
      // render input; a passthrough that swallowed it would be a silent
      // compatibility gap. Explicit frame-local refusal instead.
      if (frame_ui) {
        if (!respond_error(kSessionUiActionUnsupported)) {
          outcome.protocol_violation = true;
          break;
        }
        continue;
      }
      unsigned char* slot = channels.view() + output_offset;
      const std::size_t channel_count =
          static_cast<std::size_t>(max_width) * max_height * 4;
      if (pixel_bytes == 4) {
        std::memcpy(slot, frame_rgba.data(), frame_rgba.size());
      } else if (pixel_bytes == 8) {
        auto* deep = reinterpret_cast<uint16_t*>(slot);
        for (std::size_t index = 0; index < channel_count; ++index)
          deep[index] = static_cast<uint16_t>(
              (static_cast<uint32_t>(frame_rgba[index]) * 32768u + 127u) / 255u);
      } else {
        auto* deep = reinterpret_cast<float*>(slot);
        for (std::size_t index = 0; index < channel_count; ++index)
          deep[index] = frame_rgba[index] / 255.0f;
      }
      record_output_checksum_detail(slot, max_width, max_height, pixel_bytes);
      channels.write_header_u32(wrs::kHeaderFrameWidthOffset,
                                static_cast<uint32_t>(max_width));
      channels.write_header_u32(wrs::kHeaderFrameHeightOffset,
                                static_cast<uint32_t>(max_height));
      channels.write_header_u32(wrs::kHeaderOutputGenerationOffset,
                                expected_generation);
      outcome.width = max_width;
      outcome.height = max_height;
      outcome.rowbytes = max_width * pixel_bytes;
      if (!respond_ok(max_width, max_height, max_width * pixel_bytes,
                      static_cast<std::size_t>(max_width) * max_height * pixel_bytes,
                      0, 0)) {
        outcome.protocol_violation = true;
        break;
      }
      continue;
    }
    captured.clear();
    // Re-read every dynamic layer for this frame (issue #674). The broker
    // rewrites the file before it sends the frame message and waits for the
    // reply, so a write is never in flight while this reads. Geometry was fixed
    // at open, so only the bytes may differ; a short read is the same
    // fail-closed protocol violation it is at open.
    if (!refresh_dynamic_layers(session_layers)) {
      outcome.protocol_violation = true;
      break;
    }
    const SessionFrameOutput frame =
        render_frame(entry, current_time, frame_rgba, captured, frame_layers,
                     frame_override, frame_ui);
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
      if (!respond_error(frame.frame_error, frame.smart_output_untouched)) {
        outcome.protocol_violation = true;
        break;
      }
      continue;
    }
    // A legally empty SmartFX result (#278): PreRender skipped the render
    // selector, so there are no pixels and the reported geometry is 0x0. This is
    // a valid contract (the one-shot path reports it as a zero-dimension output),
    // not a dimension invariant failure. Advance the output generation like a
    // normal frame (no slot write) and report the empty ok frame. Only smart
    // frames set this; a classic zero dimension falls through to the invariant
    // failure below.
    if (frame.empty_result) {
      if (!captured.empty()) {
        respond_error(kSessionOutputCaptureError);
        outcome.invariant_failure = true;
        break;
      }
      // Record the empty checksum detail (no rows, sha256-of-empty channels) so
      // an opt-in final report does not carry a previous frame's stale detail
      // for this zero-byte output (#278).
      record_output_checksum_detail(channels.view() + output_offset, 0, 0, pixel_bytes);
      channels.write_header_u32(wrs::kHeaderFrameWidthOffset, 0);
      channels.write_header_u32(wrs::kHeaderFrameHeightOffset, 0);
      channels.write_header_u32(wrs::kHeaderOutputGenerationOffset, expected_generation);
      if (!respond_empty()) {
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
    // Shrink, or an expand that still fits the launch output slot, is written
    // below at its actual dimensions. An expand that overruns the slot needs a
    // larger slot: the render already ran exactly once into the private buffer
    // (captured), so instead of tearing the session down and replaying the whole
    // SEQUENCE/FRAME setup+setdown lifecycle in a fresh worker, report the
    // required dimensions and let the broker grow the shared section in place
    // (protocol §3, issue #262). After the grow the same slot copy below lands in
    // the enlarged slot; FRAME_SETUP/RENDER/FRAME_SETDOWN ran once, matching the
    // one-shot lifecycle. A malformed or insufficient grant fails closed.
    if (captured.size() > wrs::output_slot_bytes(geometry)) {
      if (!respond_resize_needed(frame.width, frame.height)) {
        outcome.protocol_violation = true;
        break;
      }
      if (!grow_output_slot(captured.size())) {
        outcome.invariant_failure = true;
        break;
      }
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
    // Report what was packed rather than hashing it: the broker derives the
    // same extent from the reported dimensions, so a mismatch catches the
    // layout disagreement the hash existed to catch, at no per-frame cost
    // (issue #690). The final report's output_hash keeps the one-shot
    // internal-ARGB definition (protocol §4.3).
    if (!respond_ok(frame.width, frame.height, frame.rowbytes, captured.size(),
                    frame.origin_x, frame.origin_y)) {
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
  channels.close();
  return;
}

// Classic resident session: render_once(manage_sequence=false) per frame
// under the hoisted sequence, the persistent_sequence precedent.
RenderSessionOutcome run_render_session(
    EffectEntry entry, std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, const RequestedAssignments* requested,
    int32_t max_width, int32_t max_height, int32_t time_step, int32_t total_time,
    uint32_t time_scale, int32_t pixel_bytes,
    const std::vector<ExternalLayerInput>* external_layers,
    const aexcompat::worker_render_session::SwapPluginHook* swap_hook,
    bool audio_passthrough) {
  // The output slot starts at the render dimensions; an expand grows it in place
  // mid-session (#262), so the initial output capacity equals max_width/height.
  RenderSessionOutcome outcome;
  run_session_frame_loop(
      outcome, entry, input, output, max_width, max_height, max_width,
      max_height, time_step, total_time,
      time_scale, pixel_bytes, external_layers,
      [&](EffectEntry current_entry, int32_t current_time,
          const std::vector<unsigned char>& frame_rgba,
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
        aexcompat::render::ClassicFrameOutput classic_output;
        frame.frame_error = render_once(
            current_entry, input, output, "request", frame.width, frame.height,
            frame.rowbytes, frame.input_hash, frame.output_hash, frame_guards,
            frame_override ? frame_override : requested, &frame_rgba,
            max_width, max_height, frame_layers,
            current_time, time_step, total_time, time_scale, pixel_bytes, false,
            &captured, &classic_output);
        frame.guard_violation = !frame_guards;
        frame.output_validation_failed = classic_output.validation_failed;
        // Sign conversion, not a copy: `ClassicFrameOutput::input_origin_*` is
        // PF_OutData::origin, and this field is the layer-relative origin the
        // smart path feeds from `result_rect[0]`. The rule itself lives in
        // render_subsystem so the self-test can pin it (issue #984).
        frame.origin_x =
            aexcompat::render::layer_origin_from_input_origin(classic_output.input_origin_x);
        frame.origin_y =
            aexcompat::render::layer_origin_from_input_origin(classic_output.input_origin_y);
        return frame;
      },
      swap_hook, audio_passthrough);
  return outcome;
}

// SmartFX resident session frame loop (protocol v1.1): each frame runs
// PreRender→SmartRender (and any GPU device setup/setdown the plan selects)
// inside the per-frame FRAME pair, while the shared loop above owns the
// hoisted SEQUENCE pair, transport, and fail-closed decisions. The GPU
// backend is fixed at launch through the session command word's case_id.
// Smart sessions carry the same static secondary layer slots as the classic
// session (issue #294): the loop hands each frame its layers and
// smart_render_once checks them out for the SmartFX render exactly as the
// one-shot --smart-image-layer path does.
SmartRenderSessionOutcome run_smart_render_session(
    EffectEntry entry, std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, const RequestedAssignments* requested,
    const std::string& case_id, int32_t max_width, int32_t max_height,
    int32_t time_step, int32_t total_time, uint32_t time_scale,
    int32_t pixel_bytes, const std::vector<ExternalLayerInput>* external_layers) {
  SmartRenderSessionOutcome outcome;
  run_session_frame_loop(
      outcome.session, entry, input, output, max_width, max_height,
      // Smart sessions (v1.1) render at fixed dimensions; no output expansion.
      max_width, max_height, time_step, total_time,
      time_scale, pixel_bytes, external_layers,
      [&](EffectEntry current_entry, int32_t current_time,
          const std::vector<unsigned char>& frame_rgba,
          std::vector<unsigned char>& captured,
          const std::vector<ExternalLayerInput>* frame_layers,
          const RequestedAssignments* frame_override,
          const SessionUiAction* frame_ui) {
        apply_session_ui_action(frame_ui);
        SessionFrameOutput frame;
        worker_runtime::smart_execution::SessionFrame session_frame{&captured};
        // A retry below renders into its own SessionFrame; `verdict` always
        // names the frame that produced `frame_result`, so the guard verdict
        // read at the end is the one from the pass whose pixels are reported.
        worker_runtime::smart_execution::SessionFrame retry_frame{&captured};
        const worker_runtime::smart_execution::SessionFrame* verdict = &session_frame;
        const auto render_attempt = [&](worker_runtime::smart_execution::SessionFrame* attempt_frame) {
          return worker_runtime::smart_setup::run_pr_gpu_pf_first_session_attempt(
              [&] {
                return smart_render_once(
                    current_entry, input, output, case_id,
                    frame_override ? frame_override : requested, &frame_rgba,
                    max_width, max_height, frame_layers, current_time, time_step,
                    total_time, time_scale, pixel_bytes, attempt_frame);
              });
        };
        worker_runtime::smart_execution::Result frame_result =
            render_attempt(&session_frame);
        // GPU-required fallback (#1072): an effect advertising GPU F32 render
        // (out_flags2 bit25) that answers PF_Err 14 at the start of CPU
        // SMART_RENDER only implements the GPU path (the color family). out_flags2
        // does not separate those from CPU-capable effects, so the runtime 14 is
        // the only signal. The 14 arrives before the plug-in touches suites,
        // worlds, or checkouts, so re-run the frame once through the GPU transport.
        if (frame_result.render_error == 14 &&
            (read<uint32_t>(output, 400) & (1u << 25)) != 0 &&
            !worker_runtime::smart_setup::force_gpu_retry_requested()) {
          const worker_runtime::smart_setup::ForceGpuRetryScope force_gpu;
          captured.clear();
          retry_frame = worker_runtime::smart_execution::SessionFrame{&captured};
          verdict = &retry_frame;
          frame_result = render_attempt(&retry_frame);
        }
        // Premiere GPU-filter fallback (#1271): an effect exporting
        // xGPUFilterEntry whose SMART_RENDER selector refused the frame only
        // implements the GPU path (the VR family draws a "requires GPU
        // acceleration" notice and gives up). The export alone does not
        // separate those from exporters whose PF CPU path works, so the
        // selector's own refusal is the signal (the selector, not a
        // FRAME_SETUP / SETDOWN error folded into render_error), and only the
        // plug-in's own: the host substitutes 512 - never 516 - whenever it
        // stands in for the plug-in's return (a caught fault, an escaped C++
        // exception, a failed module audit), and a plug-in that just faulted
        // is not re-entered on a GPU route. That guard therefore covers the
        // 512 half only; what bounds the 516 half is the `xGPUFilterEntry`
        // export test below, and the `stage:pr_gpu_route_begin reason=cpu_*`
        // line the route now carries, which keeps a papered-over host refusal
        // visible in the record. A dispatch that
        // already offered the route is not retried: the route declined once
        // and would again. Re-run the frame once with the route enabled; if it
        // declines, the PF path answers the same again.
        //
        // Both 512 and 516 count (issue #1283). The VR family's CPU path
        // answered 512 only because it gave up before reaching a host
        // callback; once `dvacore::config::Localizer` is installed the same
        // effects get one step further, ask the host to transform a world
        // with a matrix they had no field of view to build (the
        // `AE VR Effects Video Attributes Suite` they want is not
        // implemented), and pass the host's PF_Err_BAD_CALLBACK_PARAM out as
        // their own. Which of the two codes a GPU-only effect reaches the end
        // of its CPU path with is incidental; that it could not serve the
        // frame is the property this retry is for.
        if (frame_result.selector_dispatched &&
            (frame_result.selector_error == 512 ||
             frame_result.selector_error == 516) &&
            !frame_result.selector_failure_substituted &&
            !frame_result.pr_gpu_route_attempted &&
            worker_runtime::smart_dispatch::pr_gpu_filter_route_available() &&
            !worker_runtime::smart_setup::force_pr_gpu_retry_requested()) {
          const worker_runtime::smart_setup::ForcePrGpuRetryScope force_pr_gpu(
              frame_result.selector_error);
          captured.clear();
          retry_frame = worker_runtime::smart_execution::SessionFrame{&captured};
          verdict = &retry_frame;
          frame_result = render_attempt(&retry_frame);
        }
        outcome.last = frame_result;
        // The GPU transport captures float32 ARGB; when the session output is
        // 8-bit (pixel_bytes == 4) narrow it to 8-bit ARGB the way an 8-bit comp
        // in AE would receive it (#1072).
        bool downconverted_8bit = false;
        if (pixel_bytes == 4 && frame_result.output_width > 0 &&
            frame_result.output_height > 0) {
          const std::size_t px = static_cast<std::size_t>(frame_result.output_width) *
                                 static_cast<std::size_t>(frame_result.output_height);
          if (captured.size() == px * 16) {
            std::vector<unsigned char> narrowed(px * 4);
            const float* src = reinterpret_cast<const float*>(captured.data());
            for (std::size_t i = 0; i < px * 4; ++i) {
              float v = src[i];
              v = v < 0.0f ? 0.0f : (v > 1.0f ? 1.0f : v);
              narrowed[i] = static_cast<unsigned char>(v * 255.0f + 0.5f);
            }
            captured.swap(narrowed);
            downconverted_8bit = true;
          }
        }
        frame.width = frame_result.output_width;
        frame.height = frame_result.output_height;
        frame.rowbytes = downconverted_8bit ? frame_result.output_width * 4
                                            : frame_result.output_rowbytes;
        // result_rect's top-left is where these pixels sit relative to the
        // layer origin; a grown output starts at a negative coordinate. Only
        // once the rects passed validation: `result_rect` is copied out of the
        // PreRender output before it is checked, so on a rejected geometry it
        // still holds whatever the plug-in wrote, and a caller placing a frame
        // by it would be placing it by an unvalidated number.
        if (frame_result.rects_valid) {
          // The plug-in's own `result_rect` places a frame it rendered. It
          // cannot place the empty-result passthrough: that rect is empty and
          // its top-left need not be (0,0) (an effect may answer, say,
          // {5,5,5,5}), while the copied frame sits at the request rect
          // (issue #1285).
          frame.origin_x = frame_result.empty_result_passthrough
              ? frame_result.output_origin_x : frame_result.result_rect[0];
          frame.origin_y = frame_result.empty_result_passthrough
              ? frame_result.output_origin_y : frame_result.result_rect[1];
        }
        frame.input_hash = frame_result.input_hash;
        frame.output_hash = frame_result.output_hash;
        // A legally empty PreRender result_rect (#278): no pixels were rendered,
        // so this is a valid empty frame, not a zero-dimension invariant failure.
        // ... unless the host emitted the input in its place, which is a real
        // frame with real pixels (issue #1285).
        frame.empty_result = frame_result.empty_result_rect &&
            !frame_result.empty_result_passthrough;
        // Sentinel evidence only exists once the guarded output buffer was
        // built; refusals before that point are frame-local diagnostics, not
        // corruption.
        frame.guard_violation =
            verdict->output_buffer_allocated && !verdict->guards_intact;
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
        frame.smart_output_untouched = frame_result.selector_error == 0 &&
            frame_result.pre_error == 0 && frame_result.render_error == -6 &&
            frame_result.output_untouched && !frame_result.empty_result_rect;
        return frame;
      },
      // Smart sessions are out of cluster-swap scope (design §1): no hook.
      nullptr);
  return outcome;
}

}  // namespace aexcompat::l2_detail
