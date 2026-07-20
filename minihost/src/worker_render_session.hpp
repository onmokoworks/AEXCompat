#pragma once

// Render session transport (issue #98, docs/RENDER_SESSION_PROTOCOL_2026-07-19.md).
// Broker-created inherited handles only: two anonymous pipes carry
// length-prefixed JSON control messages, one anonymous file mapping carries
// pixels. The worker never opens a path for session transport, and plug-ins
// never receive a pointer into the mapping (copy-through slots).

#include <cstddef>
#include <cstdint>
#include <string>

namespace aexcompat::worker_render_session {

inline constexpr uint32_t kProtocolVersion = 1;
// render_frame v:2 carries a per-frame `parameters` payload replacing the
// launch assignments for that frame only (protocol §4.2.1, issue #107). Every
// other message, the frame_done schema, and the header layout stay v1.
inline constexpr uint32_t kRenderFrameParametersVersion = 2;
inline constexpr uint32_t kHeaderMagic = 0x53584541u;  // "AEXS" little-endian
inline constexpr std::size_t kHeaderBytes = 4096;
inline constexpr std::size_t kSlotAlignment = 4096;
inline constexpr std::size_t kMaxMessageBytes = 64 * 1024;

// SessionHeader field offsets, every field u32 little-endian.
inline constexpr std::size_t kHeaderMagicOffset = 0;
inline constexpr std::size_t kHeaderVersionOffset = 4;
inline constexpr std::size_t kHeaderDepthCodeOffset = 8;
inline constexpr std::size_t kHeaderMaxWidthOffset = 12;
inline constexpr std::size_t kHeaderMaxHeightOffset = 16;
inline constexpr std::size_t kHeaderLayerSlotCountOffset = 20;
inline constexpr std::size_t kHeaderInputGenerationOffset = 24;
inline constexpr std::size_t kHeaderOutputGenerationOffset = 28;
inline constexpr std::size_t kHeaderFrameWidthOffset = 32;
inline constexpr std::size_t kHeaderFrameHeightOffset = 36;
inline constexpr std::size_t kHeaderKnownBytes = 40;

struct SessionGeometry {
  int32_t max_width{};
  int32_t max_height{};
  // Output slot capacity, decoupled from the render dimensions (#261): the
  // output slot holds up to output_capacity_width*output_capacity_height so an
  // expand-output effect can render larger than max_width*max_height without
  // changing the render geometry. Defaults to the render dimensions.
  int32_t output_capacity_width{};
  int32_t output_capacity_height{};
  int32_t output_pixel_bytes{4};  // 4 (8bpc) / 8 (16bpc) / 16 (32f)
  int32_t layer_slot_count{};     // v1 rejects configurations with layers
};

// Input and layer slots are RGBA8 transport (4 bytes per pixel, matching the
// one-shot raw input contract); only the output slot scales with depth.
std::size_t input_slot_bytes(const SessionGeometry& geometry);
std::size_t output_slot_bytes(const SessionGeometry& geometry);
std::size_t input_slot_offset();
std::size_t output_slot_offset(const SessionGeometry& geometry);
// Layer slot `index` (0-based) follows the output slot; each is an RGBA8
// input-shaped slot (issue #98 W1-4). The caller bounds `index` by
// `layer_slot_count`.
std::size_t layer_slot_offset(const SessionGeometry& geometry, int32_t index);
std::size_t expected_section_bytes(const SessionGeometry& geometry);

// True when any of the session transport environment variables is set. Used
// to distinguish "session launch without channels" (admission failure) from
// ordinary one-shot launches.
bool session_environment_requested();

class SessionChannels {
 public:
  SessionChannels() = default;
  SessionChannels(const SessionChannels&) = delete;
  SessionChannels& operator=(const SessionChannels&) = delete;
  ~SessionChannels();

  // Parses the three AEXCOMPAT_RENDER_SESSION_*_HANDLE variables, validates
  // handle types, maps the section, and checks the mapped size covers the
  // geometry. Returns false (leaving the object unopened) on any mismatch.
  bool open_from_environment(const SessionGeometry& geometry);
  bool opened() const { return view_ != nullptr; }
  unsigned char* view() const { return view_; }

  // In-session output-slot grow (protocol §3, issue #262): the broker duplicates
  // a larger anonymous section into this process and passes its handle value in
  // a `grow` control message. Maps it, checks the mapped size covers
  // `new_geometry`, then unmaps and closes the previous section and adopts the
  // grown one. Returns false (leaving the previous section in place) on any
  // mismatch, so the caller can fail the session closed. Used so an expand that
  // overruns the launch slot re-uses the same worker and lifecycle instead of a
  // re-open that would replay SEQUENCE/FRAME setup and setdown.
  bool adopt_grown_section(unsigned long long section_handle_value,
                           const SessionGeometry& new_geometry);

  // Length-prefixed control framing: u32 LE byte count, then UTF-8 JSON.
  // A clean EOF on a frame boundary is the broker-side close signal and must
  // stay distinguishable from malformed framing (zero or oversized length,
  // EOF inside a frame), which is a protocol violation.
  enum class ReadResult { Message, Eof, Violation };
  ReadResult read_message(std::string& payload);
  bool write_message(const std::string& payload);

  uint32_t read_header_u32(std::size_t offset) const;
  void write_header_u32(std::size_t offset, uint32_t value);
  // Broker-owned static header fields must stay exactly as written at open;
  // a mutated header is a fail-closed session error.
  bool static_header_matches(const SessionGeometry& geometry) const;

 private:
  void* request_pipe_{};
  void* response_pipe_{};
  void* section_{};
  unsigned char* view_{};
  std::size_t view_bytes_{};
};

}  // namespace aexcompat::worker_render_session
