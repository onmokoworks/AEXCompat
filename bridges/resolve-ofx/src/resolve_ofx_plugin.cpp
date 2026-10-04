#include "resolve_ofx_abi.h"

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <new>
#include <string>
#include <thread>
#include <vector>

#ifdef __APPLE__
#include <signal.h>
#include <spawn.h>
#include <sys/wait.h>
#include <unistd.h>
extern char **environ;
#endif

namespace {

OfxHost g_host{};
const OfxPropertySuiteV1 *g_properties = nullptr;
const OfxImageEffectSuiteV1 *g_image_effect = nullptr;
const OfxParameterSuiteV1 *g_parameters = nullptr;

constexpr std::uint64_t kRgba8BytesPerPixel = 4;
constexpr std::uint64_t kRgbaFloatBytesPerPixel = 16;
constexpr std::uint64_t kMaxImageDimension = 16384;
constexpr std::uint64_t kMaxRenderPixels = 64ull * 1024 * 1024;
constexpr std::uint64_t kMaxImageSpan = 512ull * 1024 * 1024;

struct ImageGeometry {
  std::uint64_t width = 0;
  std::uint64_t height = 0;
  std::uint64_t row_bytes = 0;
  std::uint64_t span = 0;
  std::uint64_t bytes_per_pixel = 0;
};

bool checked_add(std::uint64_t left, std::uint64_t right,
                 std::uint64_t *result) {
  if (!result || left > UINT64_MAX - right) return false;
  *result = left + right;
  return true;
}

bool checked_multiply(std::uint64_t left, std::uint64_t right,
                      std::uint64_t *result) {
  if (!result || (right != 0 && left > UINT64_MAX / right)) return false;
  *result = left * right;
  return true;
}

bool image_geometry(const OfxRectI &bounds, int row_bytes,
                    std::uint64_t bytes_per_pixel,
                    ImageGeometry *geometry) {
  if (!geometry || row_bytes <= 0 ||
      (bytes_per_pixel != kRgba8BytesPerPixel &&
       bytes_per_pixel != kRgbaFloatBytesPerPixel)) return false;

  const auto width = static_cast<std::int64_t>(bounds.x2) -
                     static_cast<std::int64_t>(bounds.x1);
  const auto height = static_cast<std::int64_t>(bounds.y2) -
                      static_cast<std::int64_t>(bounds.y1);
  if (width <= 0 || height <= 0 ||
      static_cast<std::uint64_t>(width) > kMaxImageDimension ||
      static_cast<std::uint64_t>(height) > kMaxImageDimension) {
    return false;
  }

  std::uint64_t minimum_row_bytes = 0;
  std::uint64_t pixel_count = 0;
  std::uint64_t pixel_span = 0;
  std::uint64_t span = 0;
  if (!checked_multiply(static_cast<std::uint64_t>(width), bytes_per_pixel,
                        &minimum_row_bytes) ||
      !checked_multiply(static_cast<std::uint64_t>(width),
                        static_cast<std::uint64_t>(height), &pixel_count) ||
      !checked_multiply(pixel_count, bytes_per_pixel, &pixel_span) ||
      pixel_span > kMaxImageSpan ||
      static_cast<std::uint64_t>(row_bytes) < minimum_row_bytes ||
      !checked_multiply(static_cast<std::uint64_t>(row_bytes),
                        static_cast<std::uint64_t>(height), &span) ||
      span > kMaxImageSpan) {
    return false;
  }

  geometry->width = static_cast<std::uint64_t>(width);
  geometry->height = static_cast<std::uint64_t>(height);
  geometry->row_bytes = static_cast<std::uint64_t>(row_bytes);
  geometry->span = span;
  geometry->bytes_per_pixel = bytes_per_pixel;
  return true;
}

bool render_extent(const OfxRectI &bounds, const OfxRectI &render_window,
                   const ImageGeometry &geometry, std::uint64_t *first_row,
                   std::uint64_t *first_column, std::uint64_t *pixel_count) {
  if (!first_row || !first_column || !pixel_count ||
      render_window.x1 < bounds.x1 || render_window.y1 < bounds.y1 ||
      render_window.x2 > bounds.x2 || render_window.y2 > bounds.y2 ||
      render_window.x2 <= render_window.x1 ||
      render_window.y2 <= render_window.y1) {
    return false;
  }

  const auto width = static_cast<std::int64_t>(render_window.x2) -
                     static_cast<std::int64_t>(render_window.x1);
  const auto height = static_cast<std::int64_t>(render_window.y2) -
                      static_cast<std::int64_t>(render_window.y1);
  const auto row = static_cast<std::int64_t>(render_window.y1) -
                   static_cast<std::int64_t>(bounds.y1);
  const auto column = static_cast<std::int64_t>(render_window.x1) -
                      static_cast<std::int64_t>(bounds.x1);
  if (width <= 0 || height <= 0 || row < 0 || column < 0 ||
      static_cast<std::uint64_t>(width) > geometry.width ||
      static_cast<std::uint64_t>(height) > geometry.height ||
      !checked_multiply(static_cast<std::uint64_t>(width),
                        static_cast<std::uint64_t>(height), pixel_count) ||
      *pixel_count > kMaxRenderPixels) {
    return false;
  }

  *first_row = static_cast<std::uint64_t>(row);
  *first_column = static_cast<std::uint64_t>(column);
  return true;
}

bool checked_last_pixel_end(const ImageGeometry &geometry,
                            std::uint64_t first_row,
                            std::uint64_t first_column,
                            std::uint64_t render_width,
                            std::uint64_t render_height) {
  std::uint64_t last_row = 0;
  std::uint64_t last_column = 0;
  std::uint64_t row_offset = 0;
  std::uint64_t pixel_offset = 0;
  std::uint64_t pixel_end = 0;
  if (!checked_add(first_row, render_height - 1, &last_row) ||
      !checked_add(first_column, render_width - 1, &last_column) ||
      last_row >= geometry.height || last_column >= geometry.width ||
      !checked_multiply(last_row, geometry.row_bytes, &row_offset) ||
      !checked_multiply(last_column, geometry.bytes_per_pixel, &pixel_offset) ||
      !checked_add(row_offset, pixel_offset, &pixel_end) ||
      !checked_add(pixel_end, geometry.bytes_per_pixel, &pixel_end)) {
    return false;
  }
  return pixel_end <= geometry.span;
}

#ifdef __APPLE__
unsigned char byte_from_unit(double value) {
  return static_cast<unsigned char>(std::lround(std::clamp(value, 0.0, 1.0) * 255.0));
}

// The child alone owns the AEX worker and validates its complete bridge packet.
// A private directory carries binary pixels without embedding large JSON in
// Resolve. Nothing touches the host output until the child exits successfully.
OfxStatus render_aex_macos(const void *source_data, void *output_data,
                           const ImageGeometry &source,
                           const ImageGeometry &output,
                           const OfxRectI &bounds,
                           const OfxRectI &render_window,
                           bool rgba_float, double time_seconds) {
  try {
  const char *python = std::getenv("AEXCOMPAT_RESOLVE_PYTHON");
  const char *runner = std::getenv("AEXCOMPAT_RESOLVE_RUNNER");
  if (!python || !runner || python[0] != '/' || runner[0] != '/' ||
      !std::isfinite(time_seconds) || time_seconds < 0.0 || time_seconds > 3600.0 ||
      source.width > 4096 || source.height > 4096 ||
      source.width * source.height * 4 > 64ull * 1024 * 1024) {
    return kOfxStatErrUnsupported;
  }
  const auto millis = static_cast<long long>(std::llround(time_seconds * 1000.0));
  const auto frame_bytes = static_cast<size_t>(source.width * source.height * 4);
  std::vector<unsigned char> input(frame_bytes);
  for (size_t row = 0; row < source.height; ++row) {
    const auto *source_row = static_cast<const unsigned char *>(source_data) +
                             row * source.row_bytes;
    for (size_t col = 0; col < source.width; ++col) {
      auto *destination = input.data() + (row * source.width + col) * 4;
      const auto *pixel = source_row + col * source.bytes_per_pixel;
      if (rgba_float) {
        float channels[4];
        std::memcpy(channels, pixel, sizeof(channels));
        for (float channel : channels) {
          if (!std::isfinite(channel)) return kOfxStatErrValue;
        }
        const double alpha = std::clamp(static_cast<double>(channels[3]), 0.0, 1.0);
        for (int channel = 0; channel < 3; ++channel) {
          destination[channel] = byte_from_unit(
              alpha > 0.0 ? static_cast<double>(channels[channel]) / alpha : 0.0);
        }
        destination[3] = byte_from_unit(alpha);
      } else {
        const unsigned char alpha = pixel[3];
        for (int channel = 0; channel < 3; ++channel) {
          destination[channel] = alpha
              ? byte_from_unit(static_cast<double>(pixel[channel]) / alpha)
              : 0;
        }
        destination[3] = alpha;
      }
    }
  }

  struct TempDirectory {
    char path[sizeof("/tmp/aexcompat-resolve-XXXXXX")] =
        "/tmp/aexcompat-resolve-XXXXXX";
    bool active = false;
    ~TempDirectory() noexcept {
      if (!active) return;
      for (const char *name : {"input.rgba", "output.rgba", "evidence.json"}) {
        char file[sizeof(path) + sizeof("/evidence.json")];
        std::snprintf(file, sizeof(file), "%s/%s", path, name);
        unlink(file);
      }
      rmdir(path);
    }
  } temp;
  char *directory = mkdtemp(temp.path);
  if (!directory) return kOfxStatErrMemory;
  temp.active = true;
  const std::string root(directory);
  const std::string input_path = root + "/input.rgba";
  const std::string output_path = root + "/output.rgba";
  const std::string evidence_path = root + "/evidence.json";
  {
    std::ofstream file(input_path, std::ios::binary);
    file.write(reinterpret_cast<const char *>(input.data()),
               static_cast<std::streamsize>(input.size()));
    file.close();
    if (!file) {
      return kOfxStatErrMemory;
    }
  }
  std::string width = std::to_string(source.width);
  std::string height = std::to_string(source.height);
  std::string time = std::to_string(millis);
  char *arguments[] = {const_cast<char *>(python), const_cast<char *>(runner),
                       const_cast<char *>(input_path.c_str()),
                       const_cast<char *>(output_path.c_str()),
                       const_cast<char *>(evidence_path.c_str()),
                       const_cast<char *>(width.c_str()),
                       const_cast<char *>(height.c_str()),
                       const_cast<char *>(time.c_str()), nullptr};
  pid_t child = -1;
  const int spawn_status = posix_spawn(&child, python, nullptr, nullptr,
                                       arguments, environ);
  if (spawn_status != 0) {
    return kOfxStatErrUnsupported;
  }
  int wait_status = 0;
  bool completed = false;
  const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(90);
  while (std::chrono::steady_clock::now() < deadline) {
    const pid_t result = waitpid(child, &wait_status, WNOHANG);
    if (result == child) {
      completed = true;
      break;
    }
    if (result < 0) break;
    std::this_thread::sleep_for(std::chrono::milliseconds(20));
  }
  if (!completed) {
    // The Python transport's SIGTERM handler kills and reaps its harness
    // process group. Give that handler a bounded chance before forcing exit.
    kill(child, SIGTERM);
    const auto grace = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (std::chrono::steady_clock::now() < grace) {
      if (waitpid(child, &wait_status, WNOHANG) == child) {
        completed = true;
        break;
      }
      std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    if (!completed) {
      kill(child, SIGKILL);
      waitpid(child, &wait_status, 0);
    }
    return kOfxStatErrUnsupported;
  }
  if (!WIFEXITED(wait_status) || WEXITSTATUS(wait_status) != 0) {
    return kOfxStatErrUnsupported;
  }
  std::vector<unsigned char> rendered(frame_bytes);
  {
    std::ifstream file(output_path, std::ios::binary | std::ios::ate);
    if (!file || file.tellg() != static_cast<std::streamoff>(frame_bytes)) {
      return kOfxStatErrFormat;
    }
    file.seekg(0);
    file.read(reinterpret_cast<char *>(rendered.data()),
              static_cast<std::streamsize>(rendered.size()));
    if (!file) {
      return kOfxStatErrFormat;
    }
  }
  for (int y = render_window.y1; y < render_window.y2; ++y) {
    auto *output_row = static_cast<unsigned char *>(output_data) +
        static_cast<size_t>(y - bounds.y1) * output.row_bytes;
    for (int x = render_window.x1; x < render_window.x2; ++x) {
      const auto *pixel = rendered.data() +
          (static_cast<size_t>(y - bounds.y1) * source.width +
           static_cast<size_t>(x - bounds.x1)) * 4;
      auto *destination = output_row +
          static_cast<size_t>(x - bounds.x1) * output.bytes_per_pixel;
      if (rgba_float) {
        const float alpha = static_cast<float>(pixel[3]) / 255.0f;
        float channels[4] = {
            static_cast<float>(pixel[0]) / 255.0f * alpha,
            static_cast<float>(pixel[1]) / 255.0f * alpha,
            static_cast<float>(pixel[2]) / 255.0f * alpha, alpha};
        std::memcpy(destination, channels, sizeof(channels));
      } else {
        const unsigned char alpha = pixel[3];
        for (int channel = 0; channel < 3; ++channel) {
          destination[channel] = static_cast<unsigned char>(
              (static_cast<unsigned int>(pixel[channel]) * alpha + 127) / 255);
        }
        destination[3] = alpha;
      }
    }
  }
  return kOfxStatOK;
  } catch (const std::bad_alloc &) {
    return kOfxStatErrMemory;
  }
}
#endif

struct InstanceState {
  unsigned int lifecycle_cookie = 0xA3E0388u;
  OfxImageClipHandle source_clip = nullptr;
  OfxImageClipHandle output_clip = nullptr;
  OfxParamHandle strength_param = nullptr;
};

OfxPropertySetHandle property_set(OfxImageEffectHandle handle) {
  if (!g_image_effect || !g_image_effect->getPropertySet || !handle) {
    return nullptr;
  }
  OfxPropertySetHandle properties = nullptr;
  if (g_image_effect->getPropertySet(handle, &properties) != kOfxStatOK) {
    return nullptr;
  }
  return properties;
}

const char *property_string(OfxPropertySetHandle properties, const char *name) {
  if (!g_properties || !g_properties->propGetString || !properties) {
    return nullptr;
  }
  char *value = nullptr;
  if (g_properties->propGetString(properties, name, 0, &value) != kOfxStatOK) {
    return nullptr;
  }
  return value;
}

bool set_string(OfxPropertySetHandle properties, const char *name,
                const char *value, int index = 0) {
  return g_properties && g_properties->propSetString && properties &&
         g_properties->propSetString(properties, name, index, value) == kOfxStatOK;
}

bool set_double(OfxPropertySetHandle properties, const char *name,
                double value) {
  return g_properties && g_properties->propSetDouble && properties &&
         g_properties->propSetDouble(properties, name, 0, value) == kOfxStatOK;
}

bool set_int(OfxPropertySetHandle properties, const char *name, int value) {
  return g_properties && g_properties->propSetInt && properties &&
         g_properties->propSetInt(properties, name, 0, value) == kOfxStatOK;
}

bool set_pointer(OfxPropertySetHandle properties, const char *name,
                 void *value) {
  return g_properties && g_properties->propSetPointer && properties &&
         g_properties->propSetPointer(properties, name, 0, value) == kOfxStatOK;
}

bool get_pointer(OfxPropertySetHandle properties, const char *name,
                 void **value) {
  return g_properties && g_properties->propGetPointer && properties && value &&
         g_properties->propGetPointer(properties, name, 0, value) == kOfxStatOK;
}

bool get_int(OfxPropertySetHandle properties, const char *name, int index,
             int *value) {
  return g_properties && g_properties->propGetInt && properties && value &&
         g_properties->propGetInt(properties, name, index, value) == kOfxStatOK;
}

OfxStatus describe(OfxImageEffectHandle descriptor) {
  const auto properties = property_set(descriptor);
  if (!properties || !set_string(properties, kOfxImageEffectPropSupportedContexts,
                                 kOfxImageEffectContextFilter) ||
      !set_string(properties, kOfxImageEffectPropSupportedPixelDepths,
                  kOfxBitDepthByte) ||
      !set_string(properties, kOfxImageEffectPropSupportedPixelDepths,
                  kOfxBitDepthFloat, 1) ||
      !set_int(properties, kOfxImageEffectPropSupportsTiles, 0) ||
      !set_string(properties, kOfxPropLabel, "AEXCompat Resolve OFX")) {
    return kOfxStatErrMissingHostFeature;
  }
  return kOfxStatOK;
}

OfxStatus describe_in_context(OfxImageEffectHandle descriptor,
                              OfxPropertySetHandle in_args) {
  const char *context = property_string(in_args, kOfxImageEffectPropContext);
  if (!context || std::string(context) != kOfxImageEffectContextFilter ||
      !g_image_effect || !g_image_effect->clipDefine) {
    return kOfxStatErrUnsupported;
  }

  OfxPropertySetHandle source = nullptr;
  OfxPropertySetHandle output = nullptr;
  const auto source_status = g_image_effect->clipDefine(
      descriptor, kOfxImageEffectSimpleSourceClipName, &source);
  const auto output_status = source_status == kOfxStatOK
                                 ? g_image_effect->clipDefine(
                                       descriptor, kOfxImageEffectOutputClipName,
                                       &output)
                                 : kOfxStatErrMissingHostFeature;
  if (source_status != kOfxStatOK || output_status != kOfxStatOK ||
      !set_string(source, kOfxImageEffectPropSupportedComponents,
                  kOfxImageComponentRGBA) ||
      !set_string(output, kOfxImageEffectPropSupportedComponents,
                  kOfxImageComponentRGBA) ||
      !set_int(source, kOfxImageEffectPropSupportsTiles, 0) ||
      !set_int(output, kOfxImageEffectPropSupportsTiles, 0)) {
    return kOfxStatErrMissingHostFeature;
  }

  // This bounded control effect exposes a standard double parameter. The
  // instance reads it with paramGetValueAtTime during render and keeps that
  // path separate from the AEX RenderSession gate below.
  OfxParamSetHandle param_set = nullptr;
  OfxPropertySetHandle param_properties = nullptr;
  if (!g_parameters || !g_image_effect->getParamSet ||
      !g_parameters->paramDefine ||
      g_image_effect->getParamSet(descriptor, &param_set) != kOfxStatOK ||
      g_parameters->paramDefine(param_set, kOfxParamTypeDouble, "strength",
                                &param_properties) != kOfxStatOK ||
      !set_string(param_properties, kOfxPropLabel, "Strength") ||
      !set_double(param_properties, kOfxParamPropDefault, 0.5) ||
      !set_double(param_properties, kOfxParamPropDisplayMin, 0.0) ||
      !set_double(param_properties, kOfxParamPropDisplayMax, 1.0)) {
    return kOfxStatErrMissingHostFeature;
  }
  return kOfxStatOK;
}

OfxStatus create_instance(OfxImageEffectHandle instance) {
  auto *state = new (std::nothrow) InstanceState();
  if (!state) return kOfxStatErrMemory;
  if (!g_image_effect || !g_image_effect->clipGetHandle) {
    delete state;
    return kOfxStatErrMissingHostFeature;
  }
  OfxPropertySetHandle source_properties = nullptr;
  OfxPropertySetHandle output_properties = nullptr;
  if (g_image_effect->clipGetHandle(
          instance, kOfxImageEffectSimpleSourceClipName, &state->source_clip,
          &source_properties) != kOfxStatOK ||
      g_image_effect->clipGetHandle(instance, kOfxImageEffectOutputClipName,
                                    &state->output_clip, &output_properties) !=
          kOfxStatOK) {
    delete state;
    return kOfxStatErrMissingHostFeature;
  }
  OfxParamSetHandle param_set = nullptr;
  if (!g_parameters || !g_parameters->paramGetHandle ||
      !g_image_effect->getParamSet ||
      g_image_effect->getParamSet(instance, &param_set) != kOfxStatOK ||
      g_parameters->paramGetHandle(param_set, "strength", &state->strength_param,
                                   nullptr) != kOfxStatOK) {
    delete state;
    return kOfxStatErrMissingHostFeature;
  }
  if (!set_pointer(property_set(instance), kOfxPropInstanceData, state)) {
    delete state;
    return kOfxStatErrMissingHostFeature;
  }
  return kOfxStatOK;
}

OfxStatus destroy_instance(OfxImageEffectHandle instance) {
  auto properties = property_set(instance);
  if (!g_properties || !g_properties->propGetPointer || !properties) {
    return kOfxStatErrMissingHostFeature;
  }
  void *raw = nullptr;
  if (g_properties->propGetPointer(properties, kOfxPropInstanceData, 0, &raw) !=
      kOfxStatOK) {
    return kOfxStatErrBadHandle;
  }
  if (!raw) return kOfxStatErrBadHandle;
  if (!set_pointer(properties, kOfxPropInstanceData, nullptr)) {
    return kOfxStatErrMissingHostFeature;
  }
  delete static_cast<InstanceState *>(raw);
  return kOfxStatOK;
}

OfxStatus render(OfxImageEffectHandle instance, OfxPropertySetHandle in_args) {
  double frame_time = 0.0;
  if (!g_properties || !g_properties->propGetDouble || !in_args ||
      g_properties->propGetDouble(in_args, kOfxPropTime, 0, &frame_time) !=
          kOfxStatOK) {
    return kOfxStatErrMissingHostFeature;
  }

  auto instance_properties = property_set(instance);
  void *raw_state = nullptr;
  if (!get_pointer(instance_properties, kOfxPropInstanceData, &raw_state) ||
      !raw_state || !g_image_effect || !g_image_effect->clipGetImage ||
      !g_image_effect->clipReleaseImage) {
    return kOfxStatErrMissingHostFeature;
  }
  auto *state = static_cast<InstanceState *>(raw_state);
  double strength = 0.0;
  if (!g_parameters || !g_parameters->paramGetValueAtTime ||
      !state->strength_param ||
      g_parameters->paramGetValueAtTime(state->strength_param, frame_time,
                                         &strength) != kOfxStatOK) {
    return kOfxStatErrMissingHostFeature;
  }
  if (!std::isfinite(strength) || strength < 0.0 || strength > 1.0) {
    return kOfxStatErrValue;
  }

  OfxRectI render_window{};
  if (!get_int(in_args, kOfxImageEffectPropRenderWindow, 0, &render_window.x1) ||
      !get_int(in_args, kOfxImageEffectPropRenderWindow, 1, &render_window.y1) ||
      !get_int(in_args, kOfxImageEffectPropRenderWindow, 2, &render_window.x2) ||
      !get_int(in_args, kOfxImageEffectPropRenderWindow, 3, &render_window.y2)) {
    return kOfxStatErrMissingHostFeature;
  }
  if (render_window.x2 <= render_window.x1 ||
      render_window.y2 <= render_window.y1 || !state->source_clip ||
      !state->output_clip) {
    return kOfxStatErrValue;
  }

  OfxPropertySetHandle source_image = nullptr;
  OfxPropertySetHandle output_image = nullptr;
  const OfxRectD *region = nullptr;
  const auto source_status = g_image_effect->clipGetImage(
      state->source_clip, frame_time, region, &source_image);
  const auto output_status = g_image_effect->clipGetImage(
      state->output_clip, frame_time, region, &output_image);
  if (source_status != kOfxStatOK || output_status != kOfxStatOK ||
      !source_image || !output_image) {
    if (source_image) g_image_effect->clipReleaseImage(source_image);
    if (output_image) g_image_effect->clipReleaseImage(output_image);
    return kOfxStatErrMissingHostFeature;
  }

  void *source_data = nullptr;
  void *output_data = nullptr;
  OfxRectI source_bounds{};
  OfxRectI output_bounds{};
  int source_row_bytes = 0;
  int output_row_bytes = 0;
  const bool image_properties_ok =
      get_pointer(source_image, kOfxImagePropData, &source_data) &&
      get_pointer(output_image, kOfxImagePropData, &output_data) &&
      get_int(source_image, kOfxImagePropBounds, 0, &source_bounds.x1) &&
      get_int(source_image, kOfxImagePropBounds, 1, &source_bounds.y1) &&
      get_int(source_image, kOfxImagePropBounds, 2, &source_bounds.x2) &&
      get_int(source_image, kOfxImagePropBounds, 3, &source_bounds.y2) &&
      get_int(output_image, kOfxImagePropBounds, 0, &output_bounds.x1) &&
      get_int(output_image, kOfxImagePropBounds, 1, &output_bounds.y1) &&
      get_int(output_image, kOfxImagePropBounds, 2, &output_bounds.x2) &&
      get_int(output_image, kOfxImagePropBounds, 3, &output_bounds.y2) &&
      get_int(source_image, kOfxImagePropRowBytes, 0, &source_row_bytes) &&
      get_int(output_image, kOfxImagePropRowBytes, 0, &output_row_bytes) &&
      source_data && output_data;
  ImageGeometry source_geometry{};
  ImageGeometry output_geometry{};
  const char *source_depth = property_string(
      source_image, kOfxImageEffectPropPixelDepth);
  const char *output_depth = property_string(
      output_image, kOfxImageEffectPropPixelDepth);
  const char *source_components = property_string(
      source_image, kOfxImageEffectPropComponents);
  const char *output_components = property_string(
      output_image, kOfxImageEffectPropComponents);
  const bool rgba_components = source_components && output_components &&
      std::strcmp(source_components, kOfxImageComponentRGBA) == 0 &&
      std::strcmp(output_components, kOfxImageComponentRGBA) == 0;
  const bool rgba8 = source_depth && output_depth &&
      std::strcmp(source_depth, kOfxBitDepthByte) == 0 &&
      std::strcmp(output_depth, kOfxBitDepthByte) == 0;
  const bool rgba_float = source_depth && output_depth &&
      std::strcmp(source_depth, kOfxBitDepthFloat) == 0 &&
      std::strcmp(output_depth, kOfxBitDepthFloat) == 0;
  const auto bytes_per_pixel = rgba_float ? kRgbaFloatBytesPerPixel
                                          : kRgba8BytesPerPixel;
  if (!image_properties_ok || !rgba_components || (!rgba8 && !rgba_float) ||
      source_bounds.x1 != output_bounds.x1 ||
      source_bounds.y1 != output_bounds.y1 || source_bounds.x2 != output_bounds.x2 ||
      source_bounds.y2 != output_bounds.y2 ||
      !image_geometry(source_bounds, source_row_bytes, bytes_per_pixel,
                      &source_geometry) ||
      !image_geometry(output_bounds, output_row_bytes, bytes_per_pixel,
                      &output_geometry)) {
    g_image_effect->clipReleaseImage(source_image);
    g_image_effect->clipReleaseImage(output_image);
    return kOfxStatErrFormat;
  }

  std::uint64_t first_row = 0;
  std::uint64_t first_column = 0;
  std::uint64_t pixel_count = 0;
  if (!render_extent(source_bounds, render_window, source_geometry, &first_row,
                     &first_column, &pixel_count)) {
    g_image_effect->clipReleaseImage(source_image);
    g_image_effect->clipReleaseImage(output_image);
    return kOfxStatErrValue;
  }

  const auto render_width = static_cast<std::uint64_t>(
      static_cast<std::int64_t>(render_window.x2) - render_window.x1);
  const auto render_height = static_cast<std::uint64_t>(
      static_cast<std::int64_t>(render_window.y2) - render_window.y1);
  if (!checked_last_pixel_end(source_geometry, first_row, first_column,
                              render_width, render_height) ||
      !checked_last_pixel_end(output_geometry, first_row, first_column,
                              render_width, render_height)) {
    g_image_effect->clipReleaseImage(source_image);
    g_image_effect->clipReleaseImage(output_image);
    return kOfxStatErrFormat;
  }

  // A configured source enters the verified AEX route. Host output remains
  // untouched if the worker, its identity, or the transport fails.
#ifdef __APPLE__
  const char *aex_source = std::getenv("AEXCOMPAT_RESOLVE_AEX_PATH");
  if (aex_source && *aex_source) {
    double frame_rate = 0.0;
    if (!g_properties->propGetDouble ||
        g_properties->propGetDouble(instance_properties,
            kOfxImageEffectPropFrameRate, 0, &frame_rate) != kOfxStatOK ||
        !std::isfinite(frame_rate) || frame_rate <= 0.0) {
      g_image_effect->clipReleaseImage(source_image);
      g_image_effect->clipReleaseImage(output_image);
      return kOfxStatErrFormat;
    }
    const char *source_alpha = property_string(
        source_image, kOfxImageEffectPropPreMultiplication);
    const char *output_alpha = property_string(
        output_image, kOfxImageEffectPropPreMultiplication);
    if (!source_alpha || !output_alpha ||
        std::strcmp(source_alpha, kOfxImagePreMultiplied) != 0 ||
        std::strcmp(output_alpha, kOfxImagePreMultiplied) != 0) {
      g_image_effect->clipReleaseImage(source_image);
      g_image_effect->clipReleaseImage(output_image);
      return kOfxStatErrFormat;
    }
    const auto status = render_aex_macos(source_data, output_data,
                                         source_geometry, output_geometry,
                                         source_bounds, render_window,
                                         rgba_float, frame_time / frame_rate);
    g_image_effect->clipReleaseImage(source_image);
    g_image_effect->clipReleaseImage(output_image);
    return status;
  }
#endif

  // Bounded control effect: darken RGB by the time-evaluated strength, preserve
  // alpha, and honor host rowbytes. This is not an AEX render claim.
  for (std::uint64_t row = 0; row < render_height; ++row) {
    const auto source_row_offset = (first_row + row) * source_geometry.row_bytes;
    const auto output_row_offset = (first_row + row) * output_geometry.row_bytes;
    auto *source_row = static_cast<unsigned char *>(source_data) +
                       static_cast<size_t>(source_row_offset);
    auto *output_row = static_cast<unsigned char *>(output_data) +
                       static_cast<size_t>(output_row_offset);
    for (std::uint64_t column = 0; column < render_width; ++column) {
      const auto source_offset = static_cast<size_t>(first_column + column) *
                                 static_cast<size_t>(bytes_per_pixel);
      const auto output_offset = static_cast<size_t>(first_column + column) *
                                 static_cast<size_t>(bytes_per_pixel);
      if (rgba8) {
        const auto attenuation = 1.0 - strength;
        for (int channel = 0; channel < 3; ++channel) {
          output_row[output_offset + channel] = static_cast<unsigned char>(
              source_row[source_offset + channel] * attenuation + 0.5);
        }
        output_row[output_offset + 3] = source_row[source_offset + 3];
      } else {
        const float attenuation = static_cast<float>(1.0 - strength);
        for (int channel = 0; channel < 3; ++channel) {
          float source_value = 0.0f;
          std::memcpy(&source_value,
                      source_row + source_offset + channel * sizeof(float),
                      sizeof(float));
          const float output_value = source_value * attenuation;
          std::memcpy(output_row + output_offset + channel * sizeof(float),
                      &output_value, sizeof(float));
        }
        std::memcpy(output_row + output_offset + 3 * sizeof(float),
                    source_row + source_offset + 3 * sizeof(float),
                    sizeof(float));
      }
    }
  }
  g_image_effect->clipReleaseImage(source_image);
  g_image_effect->clipReleaseImage(output_image);
  return kOfxStatOK;
}

OfxStatus plugin_main(const char *action, const void *handle,
                      OfxPropertySetHandle in_args,
                      OfxPropertySetHandle /*out_args*/) {
  if (!action) {
    return kOfxStatErrValue;
  }
  if (std::string(action) == kOfxActionLoad ||
      std::string(action) == kOfxActionUnload) {
    return kOfxStatOK;
  }
  if (std::string(action) == kOfxActionDescribe) {
    return describe(static_cast<OfxImageEffectHandle>(const_cast<void *>(handle)));
  }
  if (std::string(action) == kOfxImageEffectActionDescribeInContext) {
    return describe_in_context(
        static_cast<OfxImageEffectHandle>(const_cast<void *>(handle)), in_args);
  }
  if (std::string(action) == kOfxActionCreateInstance) {
    return create_instance(
        static_cast<OfxImageEffectHandle>(const_cast<void *>(handle)));
  }
  if (std::string(action) == kOfxActionDestroyInstance) {
    return destroy_instance(
        static_cast<OfxImageEffectHandle>(const_cast<void *>(handle)));
  }
  if (std::string(action) == kOfxImageEffectActionRender) {
    return render(static_cast<OfxImageEffectHandle>(const_cast<void *>(handle)),
                  in_args);
  }
  return kOfxStatReplyDefault;
}

void set_host(OfxHost *host) {
  if (host) {
    g_host = *host;
    g_properties = static_cast<const OfxPropertySuiteV1 *>(
        g_host.fetchSuite(g_host.host, kOfxPropertySuite, 1));
    g_image_effect = static_cast<const OfxImageEffectSuiteV1 *>(
        g_host.fetchSuite(g_host.host, kOfxImageEffectSuite, 1));
    g_parameters = static_cast<const OfxParameterSuiteV1 *>(
        g_host.fetchSuite(g_host.host, kOfxParameterSuite, 1));
  }
}

OfxPlugin g_plugin = {kOfxImageEffectPluginApi,
                      kOfxImageEffectPluginApiVersion,
                      "com.aexcompat.resolve.ofx",
                      0,
                      1,
                      set_host,
                      plugin_main};

} // namespace

AEXCOMPAT_OFX_EXPORT int OfxGetNumberOfPlugins(void) { return 1; }

AEXCOMPAT_OFX_EXPORT OfxPlugin *OfxGetPlugin(int nth) {
  return nth == 0 ? &g_plugin : nullptr;
}

AEXCOMPAT_OFX_EXPORT OfxStatus OfxSetHost(const OfxHost *host) {
  if (!host || !host->fetchSuite) {
    return kOfxStatErrBadHandle;
  }
  g_plugin.setHost(const_cast<OfxHost *>(host));
  return kOfxStatOK;
}

AEXCOMPAT_OFX_EXPORT OfxStatus OfxPluginMain(const char *action,
                                             const void *handle,
                                             OfxPropertySetHandle in_args,
                                             OfxPropertySetHandle out_args) {
  return g_plugin.mainEntry(action, handle, in_args, out_args);
}
