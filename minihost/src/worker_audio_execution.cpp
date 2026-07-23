#include "worker_audio_execution.hpp"

#include <windows.h>

#include "host_audio_runtime.hpp"
#include "runtime_module_audit.hpp"
#include "strict_json.hpp"
#include "worker_handle_runtime.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <iostream>
#include <string>
#include <vector>

namespace aexcompat::l2_detail {

using namespace aexcompat::worker_runtime::parameter_execution;
using aexcompat::worker_runtime::handles::handle_lifetimes_balanced;
using aexcompat::worker_runtime::parameters::kDefinitionSize;

// Worker-entry owned selector plumbing and host-audio accounting stay in
// l2_main with the dispatch that mutates them; the audio mode reads them
// cross-TU.
int32_t invoke_global_setdown(EffectEntry entry, void* input, void* output);
const aexcompat::host_audio::Telemetry& audio_telemetry();
bool audio_handle_lifetimes_balanced();
// Defined in l2_main (the render subsystem owner); the audio session's
// audio_done checksum is the sha256 of the rendered output bytes the broker
// reads, matching the image session's frame_done.output.checksum contract.
std::string sha256_bytes(const unsigned char* data, std::size_t size);

namespace {
// Private protocol constants mirrored from worker_main's selector table; the
// audio selectors and buffer offsets are part of the observed legacy ABI.
constexpr int32_t kAudioRender = 19;
constexpr int32_t kAudioSetup = 20;
constexpr int32_t kAudioSetdown = 21;
constexpr std::size_t kInTimeScale = 240;

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

AudioSpanOutcome run_audio_span(
    EffectEntry entry, BufferIn& input, BufferOut& output,
    std::vector<float>* external_audio, int32_t external_audio_samples,
    const worker_runtime::parameters::RequestedAssignments& requested_parameters,
    std::vector<float>* captured_output, uint32_t rate) {
  AudioSpanOutcome outcome;
  constexpr std::size_t kAudioGuardSamples = 8;
  constexpr float kAudioGuardValue = 1234567.0f;
  std::vector<std::array<std::byte, kDefinitionSize>> audio_definitions(
      aexcompat::worker_runtime::parameters::state().records.size() + 1);
  initialize_parameter_definitions(audio_definitions);
  const bool assignments_applied =
      apply_requested_assignments(audio_definitions, requested_parameters);
  outcome.assignments_applied = assignments_applied;
  std::vector<std::array<std::byte, kDefinitionSize>> audio_values(audio_definitions.size() * 2);
  for (std::size_t index = 0; index < audio_definitions.size(); ++index) {
    audio_values[index] = audio_definitions[index];
    audio_values[index + audio_definitions.size()] = audio_definitions[index];
  }
  std::vector<void*> audio_params(audio_values.size());
  for (std::size_t index = 0; index < audio_values.size(); ++index)
    audio_params[index] = audio_values[index].data();

  write<int32_t>(input, 336, 0);
  write<int32_t>(input, 340, external_audio_samples);
  write<int32_t>(input, 344, external_audio_samples);
  write<uint32_t>(input, kInTimeScale, rate);
  write<double>(input, 352, static_cast<double>(rate));
  write<int16_t>(input, 360, 1);
  write<int16_t>(input, 362, 2);
  write<int16_t>(input, 364, 4);
  write<int32_t>(input, 368, external_audio_samples);
  write<void*>(input, 376, external_audio->data());
  aexcompat::host_audio::runtime().set_source(external_audio, external_audio_samples);

  // The audio selectors are invoked as raw entry() calls rather than through
  // guarded_effect_call, so capture the loaded-module set at each selector
  // boundary here exactly as audited_effect_call does after every guarded call
  // (its audit_capture hook is capture_module_audit_phase). Without this, a
  // transient DLL loaded only during AUDIO_SETUP/RENDER/SETDOWN and unloaded
  // before GLOBAL_SETDOWN would never enter the module audit, and the broker's
  // secure dispatch would accept a report that never observed the audio render
  // phase (Codex #258).
  std::cerr << "stage:audio_setup_begin\n" << std::flush;
  const int32_t audio_setup_error = assignments_applied
      ? entry(kAudioSetup, input.data(), output.data(), audio_params.data(), nullptr, nullptr)
      : -1;
  aexcompat::worker_runtime::capture_module_audit_phase();
  std::cerr << "stage:audio_setup_end error=" << audio_setup_error << "\n" << std::flush;
  const int32_t output_start = read<int32_t>(output, 356);
  const int32_t output_samples = read<int32_t>(output, 360);
  const bool setup_range_valid = output_start >= 0 && output_samples >= 0 &&
      output_start <= external_audio_samples &&
      output_samples <= external_audio_samples - output_start;

  std::vector<float> guarded_output(
      kAudioGuardSamples + static_cast<std::size_t>(external_audio_samples) +
      kAudioGuardSamples, kAudioGuardValue);
  auto* audio_destination = guarded_output.data() + kAudioGuardSamples;
  if (setup_range_valid) {
    std::fill_n(audio_destination, external_audio_samples, 0.0f);
    write<double>(output, 368, static_cast<double>(rate));
    write<int16_t>(output, 376, 1);
    write<int16_t>(output, 378, 2);
    write<int16_t>(output, 380, 4);
    write<int32_t>(output, 384, output_samples);
    write<void*>(output, 392, audio_destination);
  }
  std::cerr << "stage:audio_render_begin\n" << std::flush;
  const int32_t audio_render_error = audio_setup_error == 0 && setup_range_valid
      ? entry(kAudioRender, input.data(), output.data(), audio_params.data(), nullptr, nullptr)
      : -1;
  aexcompat::worker_runtime::capture_module_audit_phase();
  std::cerr << "stage:audio_render_end error=" << audio_render_error << "\n" << std::flush;
  std::cerr << "stage:audio_setdown_begin\n" << std::flush;
  const int32_t audio_setdown_error = audio_setup_error == 0
      ? entry(kAudioSetdown, input.data(), output.data(), audio_params.data(), nullptr, nullptr)
      : -1;
  aexcompat::worker_runtime::capture_module_audit_phase();
  std::cerr << "stage:audio_setdown_end error=" << audio_setdown_error << "\n" << std::flush;

  const bool guards_intact = std::all_of(guarded_output.begin(),
      guarded_output.begin() + kAudioGuardSamples,
      [=](float value) { return value == kAudioGuardValue; }) &&
      std::all_of(guarded_output.end() - kAudioGuardSamples, guarded_output.end(),
      [=](float value) { return value == kAudioGuardValue; });
  const bool samples_finite = setup_range_valid && std::all_of(
      audio_destination, audio_destination + output_samples,
      [](float value) { return std::isfinite(value); });

  // Capture the rendered samples while the guarded buffer is still alive, so
  // the caller (one-shot file write or session output slot) has them after the
  // guard buffer is torn down.
  if (captured_output && setup_range_valid)
    captured_output->assign(audio_destination, audio_destination + output_samples);

  outcome.audio_setup_error = audio_setup_error;
  outcome.audio_render_error = audio_render_error;
  outcome.audio_setdown_error = audio_setdown_error;
  outcome.output_start = output_start;
  outcome.output_samples = output_samples;
  outcome.setup_range_valid = setup_range_valid;
  outcome.guards_intact = guards_intact;
  outcome.samples_finite = samples_finite;
  outcome.audio_lifetimes_balanced = audio_handle_lifetimes_balanced();
  return outcome;
}

AudioSessionOutcome run_audio_render_session(
    EffectEntry entry, BufferIn& input, BufferOut& output, int32_t global_error,
    int32_t params_error,
    const worker_runtime::parameters::RequestedAssignments& requested_parameters,
    const worker_audio_session::AudioSessionGeometry& geometry, uint32_t rate) {
  namespace was = aexcompat::worker_audio_session;
  using aexcompat::strict_json::JsonValue;
  using aexcompat::strict_json::StrictJsonParser;
  using aexcompat::strict_json::json_exact_keys;
  using aexcompat::strict_json::json_i32;
  using aexcompat::strict_json::json_string;

  AudioSessionOutcome outcome;
  // Fail-closed teardown tail shared by every exit: GLOBAL_SETUP/PARAMS_SETUP
  // already ran at launch, so even a transport-open failure must dispose the
  // arbitrary defaults and run GLOBAL_SETDOWN before returning (Codex #252),
  // and the report must carry the module audit like the image session's.
  const auto finish_session = [&]() -> AudioSessionOutcome {
    const bool arbitrary_defaults_disposed =
        dispose_arbitrary_defaults(entry, input, output);
    outcome.global_setdown_error = (global_error == 0 && params_error == 0)
        ? invoke_global_setdown(entry, input.data(), output.data())
        : -1;
    aexcompat::worker_runtime::capture_module_audit_phase();
    if (outcome.global_setdown_error != 0 || !arbitrary_defaults_disposed ||
        !handle_lifetimes_balanced())
      outcome.clean = false;
    return outcome;
  };

  was::AudioSessionChannels channels;
  if (!channels.open_from_environment(geometry) ||
      !channels.static_header_matches(geometry)) {
    outcome.clean = false;
    outcome.protocol_violation = true;
    return finish_session();
  }
  // v1 audio session is mono (host-negotiated to channels==1, matching the
  // one-shot audio report); the channel bound stays in the geometry for the
  // multi-channel extension. input_samples counts f32 samples in the input
  // slot; the slot holds at most max_samples of them.
  const std::size_t max_slot_samples = static_cast<std::size_t>(geometry.max_samples);
  const auto* input_slot =
      reinterpret_cast<const float*>(channels.view() + was::input_slot_offset());
  auto* output_slot =
      reinterpret_cast<float*>(channels.view() + was::output_slot_offset(geometry));

  // Sentinel returned when a span fails host-protection validation (range,
  // finiteness, lifetime balance) without any selector reporting an error code.
  // A zero "error" status would let the broker accept the span as a success
  // (Codex #252): every error reply must carry a non-zero code.
  constexpr int32_t kAudioSpanRejected = -48;

  uint32_t last_output_generation = 0;
  std::string message;
  const auto respond_error = [&](int32_t request_index, int32_t render_error) {
    const std::string reply =
        "{\"v\":1,\"type\":\"audio_done\",\"request_index\":" +
        std::to_string(request_index) +
        ",\"status\":\"error\",\"audio_render_error\":" +
        std::to_string(render_error) + "}";
    return channels.write_message(reply);
  };
  for (;;) {
    const auto read = channels.read_message(message);
    if (read == was::AudioSessionChannels::ReadResult::Eof) break;  // broker close
    if (read != was::AudioSessionChannels::ReadResult::Message) {
      outcome.clean = false;
      outcome.protocol_violation = true;
      break;
    }
    JsonValue root;
    if (!StrictJsonParser(std::move(message)).parse(root) ||
        !std::holds_alternative<JsonValue::Object>(root.value)) {
      outcome.clean = false;
      outcome.protocol_violation = true;
      break;
    }
    const auto& object = std::get<JsonValue::Object>(root.value);
    std::string type;
    int32_t version = 0;
    if (!json_string(object, "type", type) || !json_i32(object, "v", version) ||
        version != static_cast<int32_t>(was::kProtocolVersion)) {
      outcome.clean = false;
      outcome.protocol_violation = true;
      break;
    }
    if (type == "close") {
      if (!json_exact_keys(object, {"v", "type"})) {
        outcome.clean = false;
        outcome.protocol_violation = true;
      }
      break;
    }
    int32_t request_index = 0;
    int32_t input_samples = 0;
    if (type != "audio_render" ||
        !json_exact_keys(object, {"v", "type", "request_index", "input_samples"}) ||
        !json_i32(object, "request_index", request_index) || request_index < 0 ||
        !json_i32(object, "input_samples", input_samples) || input_samples < 0 ||
        static_cast<std::size_t>(input_samples) > max_slot_samples) {
      outcome.clean = false;
      outcome.protocol_violation = true;
      break;
    }
    const uint32_t expected_generation = static_cast<uint32_t>(request_index) + 1;
    if (expected_generation <= last_output_generation ||
        channels.read_header_u32(was::kHeaderInputGenerationOffset) != expected_generation) {
      // A stale or foreign generation is a host-protection failure, not a
      // per-request diagnostic (§7): invalidate the session.
      outcome.clean = false;
      outcome.invariant_failure = true;
      break;
    }
    // Copy the input span out of the shared slot into a private buffer so the
    // plug-in never sees the mapping (copy-through, §10.4).
    std::vector<float> span_input(input_slot, input_slot + input_samples);
    std::vector<float> captured;
    const AudioSpanOutcome span = run_audio_span(
        entry, input, output, &span_input, input_samples, requested_parameters,
        &captured, rate);
    // run_audio_span left host_audio's source_ pointing at span_input, which is
    // destroyed at the end of this iteration. Clear it now so a plug-in that
    // checks out audio between requests or during GLOBAL_SETDOWN fails closed
    // (no source) instead of dereferencing the freed buffer.
    aexcompat::host_audio::runtime().set_source(nullptr, 0);
    // A guard breach is corruption evidence and outranks the render error:
    // invalidate the session (§4.3 host-protection). Send NO audio_done reply:
    // a `status:"error"` response is a per-span diagnostic the broker keeps the
    // session valid on, which would let host-protection corruption look like a
    // frame-local compatibility error. Breaking here runs the fail-closed
    // teardown (GLOBAL_SETDOWN, report, exit 24); the broker observes the
    // worker leave and invalidates the session.
    if (!span.guards_intact) {
      outcome.clean = false;
      outcome.invariant_failure = true;
      break;
    }
    const bool span_ok = span.assignments_applied && span.audio_setup_error == 0 &&
        span.audio_render_error == 0 && span.audio_setdown_error == 0 &&
        span.setup_range_valid && span.samples_finite &&
        span.audio_lifetimes_balanced && audio_telemetry().invalid_operations == 0;
    // Collapse the failed selector into a single non-zero code so the broker
    // never accepts a zero-error "error" status as a success (Codex #252):
    // prefer the actual selector error, else the range/finiteness sentinel.
    const int32_t span_error =
        span.audio_setup_error != 0    ? span.audio_setup_error
        : span.audio_render_error != 0 ? span.audio_render_error
        : span.audio_setdown_error != 0 ? span.audio_setdown_error
                                        : kAudioSpanRejected;
    if (!span_ok) {
      // Frame-local compatibility diagnostic: the session stays usable and the
      // broker decides whether to continue.
      if (!respond_error(request_index, span_error)) {
        outcome.clean = false;
        outcome.protocol_violation = true;
        break;
      }
      continue;
    }
    const std::size_t output_count = static_cast<std::size_t>(span.output_samples);
    if (output_count > max_slot_samples) {
      respond_error(request_index, kAudioSpanRejected);
      outcome.clean = false;
      outcome.invariant_failure = true;
      break;
    }
    std::memcpy(output_slot, captured.data(), output_count * sizeof(float));
    channels.write_header_u32(was::kHeaderOutputSamplesOffset,
                              static_cast<uint32_t>(span.output_samples));
    channels.write_header_u32(was::kHeaderOutputGenerationOffset, expected_generation);
    last_output_generation = expected_generation;
    outcome.requests_ok += 1;
    const std::string checksum = sha256_bytes(
        reinterpret_cast<const unsigned char*>(captured.data()),
        output_count * sizeof(float));
    const std::string reply =
        "{\"v\":1,\"type\":\"audio_done\",\"request_index\":" +
        std::to_string(request_index) +
        ",\"status\":\"ok\",\"output\":{\"start_sample\":" +
        std::to_string(span.output_start) + ",\"sample_count\":" +
        std::to_string(span.output_samples) + ",\"rate\":" + std::to_string(rate) +
        ",\"channels\":1,\"sample_size\":4,\"checksum\":\"" +
        checksum + "\",\"guards_intact\":true},\"audio_render_error\":0,\"generation\":" +
        std::to_string(expected_generation) + "}";
    if (!channels.write_message(reply)) {
      outcome.clean = false;
      outcome.protocol_violation = true;
      break;
    }
  }
  // Dispose arbitrary-data parameter defaults, run GLOBAL_SETDOWN, and capture
  // the pre-unload module audit through the shared teardown tail (finish_session):
  // a plug-in that declares arbitrary defaults leaves those handles live
  // otherwise, and the broker's SecureSessionProcess::finish fails a clean-exit
  // report closed if `module_audit` is missing (worker_module_audit.rs), so a
  // session report must carry it exactly like the image session's final report.
  return finish_session();
}

void emit_audio_session_report(int32_t global_error, int32_t params_error,
                               const AudioSessionOutcome& outcome) {
  std::cout << "{\"schema_version\":1,\"stage\":\"audio_session\",\"status\":\""
            << (outcome.clean ? "session_completed" : "session_failed")
            << "\",\"global_setup_error\":" << global_error
            << ",\"params_setup_error\":" << params_error
            << ",\"session_requests_ok\":" << outcome.requests_ok
            << ",\"session_protocol_violation\":"
            << (outcome.protocol_violation ? "true" : "false")
            << ",\"session_invariant_failure\":"
            << (outcome.invariant_failure ? "true" : "false")
            << ",\"global_setdown_error\":" << outcome.global_setdown_error
            // Audio telemetry parity with the one-shot audio report
            // (emit_audio_render_report): the SDK audio contract consumers read
            // these, and the length-1 session wrapper surfaces them on the
            // session-routed public report. For a session they are the
            // aggregate (last span's) values.
            << ",\"audio_checkout_calls\":" << audio_telemetry().checkout_calls
            << ",\"audio_usage_advertised\":" << (audio_telemetry().usage_advertised ? "true" : "false")
            << ",\"audio_checkout_allowed\":" << (audio_telemetry().checkout_allowed ? "true" : "false")
            << ",\"rejected_unadvertised_audio_checkouts\":" << audio_telemetry().rejected_unadvertised_checkouts
            << ",\"rejected_audio_format_requests\":" << audio_telemetry().rejected_format_requests
            << ",\"audio_handle_exhaustions\":" << audio_telemetry().handle_exhaustions
            << ",\"peak_live_audio_handles\":" << audio_telemetry().peak_live_handles
            << ",\"audio_checkin_calls\":" << audio_telemetry().checkin_calls
            << ",\"audio_get_data_calls\":" << audio_telemetry().get_data_calls
            << ",\"last_audio_checkout_start_time\":" << audio_telemetry().last_checkout_start_time
            << ",\"last_audio_checkout_duration\":" << audio_telemetry().last_checkout_duration
            << ",\"last_audio_checkout_time_scale\":" << audio_telemetry().last_checkout_time_scale
            << ",\"last_audio_window_start_sample\":" << audio_telemetry().last_window_start_sample
            << ",\"last_audio_window_sample_count\":" << audio_telemetry().last_window_sample_count
            << ",\"last_audio_window_silence_samples\":" << audio_telemetry().last_window_silence_samples
            << ",\"last_audio_output_rate_fixed\":" << audio_telemetry().last_output_rate
            << ",\"last_audio_output_bytes_per_sample\":" << audio_telemetry().last_output_bytes_per_sample
            << ",\"last_audio_output_channels\":" << audio_telemetry().last_output_channels
            << ",\"last_audio_output_format\":" << audio_telemetry().last_output_format
            << ",\"last_audio_returned_sample_frames\":" << audio_telemetry().last_returned_sample_frames
            << ",\"audio_lifetimes_balanced\":"
            << (audio_handle_lifetimes_balanced() ? "true" : "false")
            << ",\"invalid_audio_operations\":" << audio_telemetry().invalid_operations
            << ",\"module_audit\":" << aexcompat::worker_runtime::module_audit_json()
            << ",\"session_clean\":" << (outcome.clean ? "true" : "false") << "}\n";
}

}  // namespace aexcompat::l2_detail
