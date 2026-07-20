#pragma once

// Resident audio render session transport (issue #239,
// docs/RENDER_SESSION_PROTOCOL_2026-07-19.md §10). Independent of the image
// render session (§2-§8): audio is bulk (one AUDIO_SETUP/RENDER/SETDOWN over a
// sample span, on its own thread and sequence, per the stage-0 observations),
// so it gets a separate control message (audio_render with
// start_sample/duration_samples), a separate f32 audio sample buffer channel,
// and its own worker lifecycle that hoists run_audio_span across requests.
//
// Broker-created inherited handles only: two anonymous pipes carry
// length-prefixed JSON control messages, one anonymous file mapping carries the
// f32 audio samples. The worker never opens a path for session transport, and
// the plug-in never receives a pointer into the mapping (copy-through slots,
// matching the image session's host-protection).

#include <cstddef>
#include <cstdint>
#include <string>

namespace aexcompat::worker_audio_session {

inline constexpr uint32_t kProtocolVersion = 1;
inline constexpr uint32_t kHeaderMagic = 0x53554141u;  // "AAUS" little-endian
inline constexpr std::size_t kHeaderBytes = 4096;
inline constexpr std::size_t kSlotAlignment = 4096;
inline constexpr std::size_t kMaxMessageBytes = 64 * 1024;

// AudioSessionHeader field offsets, every field u32 little-endian.
inline constexpr std::size_t kHeaderMagicOffset = 0;
inline constexpr std::size_t kHeaderVersionOffset = 4;
inline constexpr std::size_t kHeaderMaxSamplesOffset = 8;
inline constexpr std::size_t kHeaderChannelsOffset = 12;
inline constexpr std::size_t kHeaderInputGenerationOffset = 16;
inline constexpr std::size_t kHeaderOutputGenerationOffset = 20;
inline constexpr std::size_t kHeaderRequestSamplesOffset = 24;
inline constexpr std::size_t kHeaderOutputSamplesOffset = 28;
inline constexpr std::size_t kHeaderKnownBytes = 32;

// Reserved exit codes shared with the image session's contract (protocol §7).
inline constexpr int kExitProtocolViolation = 23;
inline constexpr int kExitInvariantFailure = 24;

struct AudioSessionGeometry {
  // Upper bound on the samples any single audio_render span may carry, per
  // channel; the input and output slots are each max_samples * channels f32.
  int32_t max_samples{};
  int32_t channels{1};
};

// Both slots are f32 interleaved (matching host_audio::Runtime's float
// contract). rate/channels/format are host-negotiated at AUDIO_RENDER time and
// reported per span, not fixed by the layout beyond the channel bound.
std::size_t slot_bytes(const AudioSessionGeometry& geometry);
std::size_t input_slot_offset();
std::size_t output_slot_offset(const AudioSessionGeometry& geometry);
std::size_t expected_section_bytes(const AudioSessionGeometry& geometry);

// True when any AUDIO session transport environment variable is set (used to
// tell "audio session launch without channels" from an ordinary one-shot).
bool session_environment_requested();

class AudioSessionChannels {
 public:
  AudioSessionChannels() = default;
  AudioSessionChannels(const AudioSessionChannels&) = delete;
  AudioSessionChannels& operator=(const AudioSessionChannels&) = delete;
  ~AudioSessionChannels();

  // Parses the three AEXCOMPAT_AUDIO_SESSION_*_HANDLE variables, validates
  // handle types, maps the section, and checks the mapped size covers the
  // geometry. Returns false (leaving the object unopened) on any mismatch.
  bool open_from_environment(const AudioSessionGeometry& geometry);
  bool opened() const { return view_ != nullptr; }
  unsigned char* view() const { return view_; }

  // Length-prefixed control framing (identical to the image session's §4.1):
  // u32 LE byte count then UTF-8 JSON. A clean EOF on a frame boundary is the
  // close signal and must stay distinguishable from malformed framing.
  enum class ReadResult { Message, Eof, Violation };
  ReadResult read_message(std::string& payload);
  bool write_message(const std::string& payload);

  uint32_t read_header_u32(std::size_t offset) const;
  void write_header_u32(std::size_t offset, uint32_t value);
  bool static_header_matches(const AudioSessionGeometry& geometry) const;

 private:
  void* request_pipe_{};
  void* response_pipe_{};
  void* section_{};
  unsigned char* view_{};
  std::size_t view_bytes_{};
};

}  // namespace aexcompat::worker_audio_session
