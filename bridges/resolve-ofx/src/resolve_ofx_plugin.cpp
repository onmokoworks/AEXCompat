#include "resolve_ofx_abi.h"

#include <algorithm>
#include <cstdint>
#include <cmath>
#include <new>
#include <string>

namespace {

OfxHost g_host{};
const OfxPropertySuiteV1 *g_properties = nullptr;
const OfxImageEffectSuiteV1 *g_image_effect = nullptr;
const OfxParameterSuiteV1 *g_parameters = nullptr;

constexpr std::uint64_t kBytesPerPixel = 4;
constexpr std::uint64_t kMaxImageDimension = 16384;
constexpr std::uint64_t kMaxRenderPixels = 64ull * 1024 * 1024;
constexpr std::uint64_t kMaxImageSpan = 512ull * 1024 * 1024;

struct ImageGeometry {
  std::uint64_t width = 0;
  std::uint64_t height = 0;
  std::uint64_t row_bytes = 0;
  std::uint64_t span = 0;
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
                    ImageGeometry *geometry) {
  if (!geometry || row_bytes <= 0) return false;

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
  if (!checked_multiply(static_cast<std::uint64_t>(width), kBytesPerPixel,
                        &minimum_row_bytes) ||
      !checked_multiply(static_cast<std::uint64_t>(width),
                        static_cast<std::uint64_t>(height), &pixel_count) ||
      !checked_multiply(pixel_count, kBytesPerPixel, &pixel_span) ||
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
      !checked_multiply(last_column, kBytesPerPixel, &pixel_offset) ||
      !checked_add(row_offset, pixel_offset, &pixel_end) ||
      !checked_add(pixel_end, kBytesPerPixel, &pixel_end)) {
    return false;
  }
  return pixel_end <= geometry.span;
}

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
                const char *value) {
  return g_properties && g_properties->propSetString && properties &&
         g_properties->propSetString(properties, name, 0, value) == kOfxStatOK;
}

bool set_double(OfxPropertySetHandle properties, const char *name,
                double value) {
  return g_properties && g_properties->propSetDouble && properties &&
         g_properties->propSetDouble(properties, name, 0, value) == kOfxStatOK;
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
  if (g_image_effect->clipDefine(descriptor, kOfxImageEffectSimpleSourceClipName,
                                  &source) != kOfxStatOK ||
      g_image_effect->clipDefine(descriptor, kOfxImageEffectOutputClipName,
                                  &output) != kOfxStatOK ||
      !set_string(source, kOfxImageEffectPropComponents, kOfxImageComponentRGBA) ||
      !set_string(source, kOfxImageEffectPropPixelDepth, kOfxBitDepthByte) ||
      !set_string(output, kOfxImageEffectPropComponents, kOfxImageComponentRGBA) ||
      !set_string(output, kOfxImageEffectPropPixelDepth, kOfxBitDepthByte) ||
      !set_string(output, kOfxImageEffectPropSupportsTiles, "false") ||
      !set_string(output, kOfxImageEffectPropPreMultiplication,
                  kOfxImagePreMultiplied)) {
    return kOfxStatErrMissingHostFeature;
  }

  // This bounded control effect exposes a standard double parameter. The
  // instance reads it with paramGetValueAtTime during render and keeps that
  // path separate from the AEX RenderSession gate below.
  if (g_parameters && g_image_effect->getParamSet && g_parameters->paramDefine) {
    OfxParamSetHandle param_set = nullptr;
    OfxPropertySetHandle param_properties = nullptr;
    if (g_image_effect->getParamSet(descriptor, &param_set) == kOfxStatOK &&
        g_parameters->paramDefine(param_set, kOfxParamTypeDouble, "strength",
                                   &param_properties) == kOfxStatOK) {
      set_string(param_properties, kOfxPropLabel, "Strength");
      set_double(param_properties, kOfxParamPropDefault, 0.5);
      set_double(param_properties, kOfxParamPropDisplayMin, 0.0);
      set_double(param_properties, kOfxParamPropDisplayMax, 1.0);
    }
  }
  return kOfxStatOK;
}

OfxStatus create_instance(OfxImageEffectHandle instance) {
  auto *state = new (std::nothrow) InstanceState();
  if (!state || !g_image_effect || !g_image_effect->clipGetHandle ||
      !set_pointer(property_set(instance), kOfxPropInstanceData, state)) {
    delete state;
    return kOfxStatErrMemory;
  }
  OfxPropertySetHandle source_properties = nullptr;
  OfxPropertySetHandle output_properties = nullptr;
  if (g_image_effect->clipGetHandle(
          instance, kOfxImageEffectSimpleSourceClipName, &state->source_clip,
          &source_properties) != kOfxStatOK ||
      g_image_effect->clipGetHandle(instance, kOfxImageEffectOutputClipName,
                                    &state->output_clip, &output_properties) !=
          kOfxStatOK) {
    set_pointer(property_set(instance), kOfxPropInstanceData, nullptr);
    delete state;
    return kOfxStatErrMissingHostFeature;
  }
  OfxParamSetHandle param_set = nullptr;
  if (!g_parameters || !g_parameters->paramGetHandle ||
      !g_image_effect->getParamSet ||
      g_image_effect->getParamSet(instance, &param_set) != kOfxStatOK ||
      g_parameters->paramGetHandle(param_set, "strength", &state->strength_param,
                                   nullptr) != kOfxStatOK) {
    set_pointer(property_set(instance), kOfxPropInstanceData, nullptr);
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
  delete static_cast<InstanceState *>(raw);
  set_pointer(properties, kOfxPropInstanceData, nullptr);
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
  for (int index = 0; index < 4; ++index) {
    if (!get_int(in_args, kOfxImageEffectPropRenderWindow, index,
                 &((&render_window.x1)[index]))) {
      return kOfxStatErrMissingHostFeature;
    }
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
  if (!image_properties_ok || source_bounds.x1 != output_bounds.x1 ||
      source_bounds.y1 != output_bounds.y1 || source_bounds.x2 != output_bounds.x2 ||
      source_bounds.y2 != output_bounds.y2 ||
      !image_geometry(source_bounds, source_row_bytes, &source_geometry) ||
      !image_geometry(output_bounds, output_row_bytes, &output_geometry)) {
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

  // Bounded, deterministic control effect: darken premultiplied RGB by the
  // time-evaluated strength parameter, preserve alpha, and honor host rowbytes.
  // This is an actual OFX pixel render, but it is not an AEX render claim.
  for (std::uint64_t row = 0; row < render_height; ++row) {
    const auto source_row_offset = (first_row + row) * source_geometry.row_bytes;
    const auto output_row_offset = (first_row + row) * output_geometry.row_bytes;
    auto *source_row = static_cast<unsigned char *>(source_data) +
                       static_cast<size_t>(source_row_offset);
    auto *output_row = static_cast<unsigned char *>(output_data) +
                       static_cast<size_t>(output_row_offset);
    for (std::uint64_t column = 0; column < render_width; ++column) {
      const auto source_offset = static_cast<size_t>(first_column + column) *
                                 static_cast<size_t>(kBytesPerPixel);
      const auto output_offset = static_cast<size_t>(first_column + column) *
                                 static_cast<size_t>(kBytesPerPixel);
      const auto attenuation = 1.0 - strength;
      output_row[output_offset + 0] = static_cast<unsigned char>(
          source_row[source_offset + 0] * attenuation + 0.5);
      output_row[output_offset + 1] = static_cast<unsigned char>(
          source_row[source_offset + 1] * attenuation + 0.5);
      output_row[output_offset + 2] = static_cast<unsigned char>(
          source_row[source_offset + 2] * attenuation + 0.5);
      output_row[output_offset + 3] = source_row[source_offset + 3];
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
