#include "worker_smart_finalize.hpp"
#include "render_pixel_transport.hpp"
#include "worker_render_report.hpp"

#include <cstring>
#include <sstream>

namespace aexcompat::worker_runtime::smart_finalize {
bool verify_nonfinite_output() {
  // Run the production finalizer with a padded output and lifecycle hooks that
  // preserve the supplied selector error. No plug-in or SDK fixture is needed.
  parameter_execution::BufferIn input{};
  parameter_execution::BufferOut output{};
  smart_setup::ParameterState parameters(1, 0);
  world_safety::EffectWorldStorage world{};
  render_lifecycle::RenderLifecycle lifecycle;
  std::array<std::byte, 56> pre_output{};
  constexpr int32_t width = 3, height = 2, rowbytes = 64;
  render_safety::InputPixelBuffer source(rowbytes * height);
  render_safety::OutputPixelBuffer guarded(rowbytes * height);
  if (!source || !guarded) return false;
  std::memset(source.data(), 0, rowbytes * height);
  std::vector<unsigned char> captured;
  smart_execution::SessionFrame session{&captured};
  const auto entry = +[](int32_t, void*, void*, void**, void*, void*) -> int32_t {
    return 0;
  };
  const Hooks hooks{
      +[](parameter_execution::EffectEntry, parameter_execution::BufferIn&,
          parameter_execution::BufferOut&, parameter_execution::Definitions&) {
        return true;
      },
      +[](parameter_execution::EffectEntry, parameter_execution::BufferIn&,
          parameter_execution::BufferOut&, void**, void*,
          const render_lifecycle::RenderLifecycle&, int32_t error) { return error; },
      +[](const std::string&, const unsigned char*, int32_t, int32_t, int32_t) {},
      +[](const unsigned char*, std::size_t) { return std::string{}; },
      +[]() { return false; }};
  Request request{entry, &input, &output, &parameters, &world, &lifecycle,
      &source, &guarded, guarded.data(), width, height, rowbytes, 16,
      &pre_output, &session};
  const auto fresh = [&]() {
    smart_execution::Result result;
    result.pre_error = result.selector_error = result.render_error = 0;
    result.output_width = width;
    result.output_height = height;
    result.output_rowbytes = rowbytes;
    return result;
  };
  // Quiet/signalling NaNs of both signs with distinct payloads, infinities,
  // signed zeros, subnormals and finite HDR words. Rotate through every channel.
  constexpr uint32_t words[] = {0x7fc12345, 0xffc54321, 0x7f812345, 0xff812346,
      0x7f800000, 0xff800000, 0x00000000, 0x80000000,
      0x00000001, 0x80000001, 0x3f800000, 0x40800000};
  for (int rotation = 0; rotation < 4; ++rotation) {
    std::vector<unsigned char> expected(width * height * 16);
    std::memset(guarded.data(), 0xA5, rowbytes * height);
    for (int pixel = 0; pixel < width * height; ++pixel) {
      for (int channel = 0; channel < 4; ++channel) {
        const uint32_t word = words[(pixel * 4 + channel + rotation) % 12];
        std::memcpy(expected.data() + pixel * 16 + channel * 4, &word, 4);
      }
      std::memcpy(guarded.data() + (pixel / width) * rowbytes +
                      (pixel % width) * 16, expected.data() + pixel * 16, 16);
    }
    auto result = fresh();
    if (!finalize(request, hooks, result) || result.render_error != 0 ||
        !result.output_pixels_valid || result.output_samples_finite ||
        !result.guards_intact || result.output_coverage.unwritten_pixels != 0 ||
        captured != expected) return false;
    // The actual session-slot permutation must preserve payloads too.
    for (int pixel = 0; pixel < width * height; ++pixel) {
      uint32_t rgba[4]{};
      render_pixel_transport::argb_to_rgba_native(
          rgba, captured.data() + pixel * 16, 16);
      for (int channel = 0; channel < 4; ++channel) {
        uint32_t want{};
        std::memcpy(&want, expected.data() + pixel * 16 + ((channel + 1) % 4) * 4, 4);
        if (rgba[channel] != want) return false;
      }
    }
    // Empty-result passthrough retains the same words and diagnostic.
    result = fresh();
    result.empty_result_rect = result.empty_result_passthrough = true;
    if (!finalize(request, hooks, result) || result.render_error != 0 ||
        !result.output_pixels_valid || result.output_samples_finite ||
        captured != expected) return false;
    result = fresh();
    result.output_coverage_external_world = true;
    if (!finalize(request, hooks, result) || result.render_error != 0 ||
        !result.output_pixels_valid || result.output_samples_finite ||
        captured != expected) return false;
  }
  // A caller must still see the diagnostic in the serialized worker report.
  std::ostringstream serialized;
  worker_render_report::ReportSnapshot snapshot(serialized);
  worker_render_report::SmartReport::Context context;
  context.validity = {true, true, true};
  context.output_samples_finite = false;
  worker_render_report::append_smart_context(snapshot, context);
  worker_render_report::emit(snapshot, serialized);
  if (serialized.str().find("\"output_samples_finite\":false") == std::string::npos)
    return false;

  // Non-finite contents do not mask a selector failure or damaged guards.
  auto result = fresh();
  result.render_error = 37;
  if (!finalize(request, hooks, result) || result.render_error != 37) return false;
  guarded.data()[guarded.size()] ^= 1;
  result = fresh();
  if (!finalize(request, hooks, result) || result.guards_intact) return false;
  guarded.data()[guarded.size()] ^= 1;
  result = fresh();
  result.output_rowbytes = width * 16 - 1;
  if (finalize(request, hooks, result)) return false;
  result = fresh();
  result.output_width = -1;
  if (finalize(request, hooks, result)) return false;
  // Seeded or partially unwritten pixels remain a transport failure at every
  // depth. Finite zero output succeeds; lossy non-finite narrowing does not.
  for (const int32_t depth : {4, 8, 16}) {
    request.pixel_bytes = depth;
    if (!output_coverage::seed(guarded.data(), guarded.size(), width, height,
                              rowbytes, depth)) return false;
    result = fresh();
    if (!finalize(request, hooks, result) || result.render_error != -6 ||
        result.output_pixels_valid || !result.output_samples_finite ||
        result.output_coverage.unwritten_pixels != width * height) return false;
    std::memset(guarded.data(), 0, depth);
    result = fresh();
    if (!finalize(request, hooks, result) || result.render_error != -6 ||
        result.output_coverage.unwritten_pixels != width * height - 1) return false;
    if (depth == 16) {
      std::memcpy(guarded.data(), words, depth);
      result = fresh();
      if (!finalize(request, hooks, result) || result.render_error != -6 ||
          result.output_pixels_valid || result.output_samples_finite ||
          result.output_coverage.unwritten_pixels != width * height - 1) return false;
    }
    std::memset(guarded.data(), 0, guarded.size());
    result = fresh();
    if (!finalize(request, hooks, result) || result.render_error != 0 ||
        !result.output_pixels_valid || !result.output_samples_finite) return false;
    if (depth != 16) {
      result = fresh();
      result.output_non_finite = true;
      if (!finalize(request, hooks, result) || result.render_error != -6 ||
          result.output_pixels_valid || result.output_samples_finite) return false;
    }
    std::memset(guarded.data(), 0xCC, guarded.size());
    result = fresh();
    result.output_coverage_external_world = true;
    if (!finalize(request, hooks, result) || result.render_error != -6 ||
        result.output_pixels_valid || !result.output_untouched) return false;
  }
  return true;
}
}  // namespace aexcompat::worker_runtime::smart_finalize
