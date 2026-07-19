#include "worker_render_session.hpp"

#include <windows.h>

#include "render_pixel_transport.hpp"
#include "render_subsystem.h"
#include "strict_json.hpp"
#include "worker_invocation_orchestration.hpp"
#include "worker_pf_ae_channel_runtime.hpp"
#include "worker_request_parser.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
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
std::string sha256_bytes(const unsigned char* data, std::size_t size);
void record_output_checksum_detail(const unsigned char* rgba, int32_t width,
                                   int32_t height, int32_t pixel_bytes);

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
}  // namespace

// Resident render session frame loop (docs/RENDER_SESSION_PROTOCOL_2026-07-19.md).
// SEQUENCE_SETUP is hoisted once around render_once(manage_sequence=false)
// following the persistent_sequence precedent; pixels move through the
// inherited anonymous section (copy-through slots, the plug-in never sees the
// mapping) and control messages over the inherited pipe pair with strict
// exact-key validation. RenderSessionOutcome is defined in
// worker_invocation_orchestration.hpp so the final dispatch owner can call
// this across TUs.
RenderSessionOutcome run_render_session(
    EffectEntry entry, std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, const RequestedAssignments* requested,
    int32_t max_width, int32_t max_height, int32_t time_step, int32_t total_time,
    uint32_t time_scale, int32_t pixel_bytes,
    const std::vector<ExternalLayerInput>* external_layers) {
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
        version != static_cast<int32_t>(wrs::kProtocolVersion)) {
      outcome.protocol_violation = true;
      break;
    }
    if (type == "close") {
      if (!json_exact_keys(object, {"v", "type"})) outcome.protocol_violation = true;
      break;
    }
    int32_t frame_index{};
    const auto* time_value = json_member(object, "current_time");
    if (type != "render_frame" ||
        !json_exact_keys(object, {"v", "type", "frame_index", "current_time"}) ||
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
    int32_t frame_width = 0, frame_height = 0, frame_rowbytes = 0;
    std::string frame_input_hash, frame_output_hash;
    // Initialized true so it means "finalize observed corruption" when false:
    // render_once's early host-side failures return before touching it, while
    // every path that allocates the guarded buffer overwrites it.
    bool frame_guards = true;
    bool output_validation_failed = false;
    captured.clear();
    const int32_t frame_error = render_once(
        entry, input, output, "request", frame_width, frame_height, frame_rowbytes,
        frame_input_hash, frame_output_hash, frame_guards, requested, &frame_rgba,
        nullptr, max_width, max_height, frame_layers, current_time, time_step, total_time,
        time_scale, pixel_bytes, false, &captured, &output_validation_failed);
    // Aux channel chunks are host-owned and cannot outlive one frame's render
    // lifecycle (end_render's cleanup for the one-shot path); the manifest
    // itself stays active across frames.
    aexcompat::pf_ae_channel::reclaim_layer_channels();
    outcome.width = frame_width;
    outcome.height = frame_height;
    outcome.rowbytes = frame_rowbytes;
    outcome.input_hash = frame_input_hash;
    outcome.output_hash = frame_output_hash;
    // Corruption evidence outranks the render error: a plug-in that wrote
    // outside its guarded private buffer invalidates the session even when it
    // also reported a nonzero error. Host-protection invariant, fail closed.
    if (!frame_guards) {
      outcome.guards_intact = false;
      respond_error(kSessionGuardViolation);
      outcome.invariant_failure = true;
      break;
    }
    // Host-side output validation failures (rejected or failed resize) share
    // numeric codes with selector errors, so the dispatch owner reports them
    // out of band; they are output-bounds invariant failures, not frame-local
    // diagnostics (protocol §4.3).
    if (output_validation_failed) {
      respond_error(kSessionOutputValidationError);
      outcome.invariant_failure = true;
      break;
    }
    if (frame_error != 0) {
      // Frame-local compatibility diagnostic; the sequence state is still
      // owned by the host, so the session may continue.
      if (!respond_error(frame_error)) {
        outcome.protocol_violation = true;
        break;
      }
      continue;
    }
    // v1 fixes every frame to the launch max dimensions (protocol §3); an
    // expand/shrink-output effect changing them would publish dimensions the
    // broker cannot trust against the slot layout. Fail closed.
    if (frame_width != max_width || frame_height != max_height) {
      respond_error(kSessionDimensionMismatch);
      outcome.invariant_failure = true;
      break;
    }
    const std::size_t expected_pixels =
        static_cast<std::size_t>(frame_width) * frame_height;
    if (captured.size() != expected_pixels * pixel_bytes ||
        output_offset + captured.size() > wrs::expected_section_bytes(geometry)) {
      respond_error(kSessionOutputCaptureError);
      outcome.invariant_failure = true;
      break;
    }
    unsigned char* slot = channels.view() + output_offset;
    for (std::size_t pixel = 0; pixel < expected_pixels; ++pixel)
      argb_to_rgba_native(slot + pixel * pixel_bytes,
                          captured.data() + pixel * pixel_bytes, pixel_bytes);
    // Mirrors the one-shot finalize checksum hook (gated on the opt-in aux
    // option): detail is computed from the same transferred RGBA bytes the
    // broker reads. The final report carries the last rendered frame's
    // detail; frame_done stays the per-frame truth.
    record_output_checksum_detail(slot, frame_width, frame_height, pixel_bytes);
    channels.write_header_u32(wrs::kHeaderFrameWidthOffset,
                              static_cast<uint32_t>(frame_width));
    channels.write_header_u32(wrs::kHeaderFrameHeightOffset,
                              static_cast<uint32_t>(frame_height));
    channels.write_header_u32(wrs::kHeaderOutputGenerationOffset,
                              expected_generation);
    // The frame checksum covers the transferred slot bytes, the exact bytes
    // the broker reads; the final report's output_hash keeps the one-shot
    // internal-ARGB definition (protocol §4.3).
    const std::string slot_checksum = sha256_bytes(slot, captured.size());
    if (!respond_ok(frame_width, frame_height, frame_rowbytes, slot_checksum)) {
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

}  // namespace aexcompat::l2_detail
