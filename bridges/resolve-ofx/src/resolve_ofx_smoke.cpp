#include "resolve_ofx_abi.h"

#ifdef _WIN32
#include <windows.h>
#include <bcrypt.h>
#pragma comment(lib, "bcrypt.lib")
#endif

#include <cstdlib>
#include <cstring>
#include <iostream>
#include <iomanip>
#include <memory>
#include <sstream>
#include <string>
#include <unordered_map>
#include <vector>

struct Value {
  enum class Kind { String, Double, Int, Pointer } kind = Kind::String;
  std::string string_value;
  double double_value = 0.0;
  int int_value = 0;
  std::vector<int> int_values;
  void *pointer_value = nullptr;
};

struct FakeImageObject;

struct FakePropertySet {
  std::unordered_map<std::string, Value> values;
  void *owner = nullptr;
};

struct FakeParam {
  FakePropertySet properties;
  double value = 0.5;
  double last_time = -1.0;
};

struct FakeClip {
  FakeImageObject *owner = nullptr;
  bool source = false;
  FakePropertySet properties;
};

struct FakeImageObject {
  FakePropertySet properties;
  FakePropertySet params;
  FakeParam strength;
  std::vector<std::unique_ptr<FakePropertySet>> children;
  FakeClip source_clip;
  FakeClip output_clip;
  FakePropertySet source_image;
  FakePropertySet output_image;
  std::vector<unsigned char> source_pixels;
  std::vector<unsigned char> output_pixels;
  double last_source_time = -1.0;
  double last_output_time = -1.0;
};

static FakePropertySet *as_props(OfxPropertySetHandle handle) {
  return reinterpret_cast<FakePropertySet *>(handle);
}

static OfxStatus prop_set_pointer(OfxPropertySetHandle h, const char *name,
                                  int index, void *value) {
  if (!h || !name || index != 0) return kOfxStatErrBadIndex;
  auto &v = as_props(h)->values[name];
  v.kind = Value::Kind::Pointer;
  v.pointer_value = value;
  return kOfxStatOK;
}

static OfxStatus prop_set_string(OfxPropertySetHandle h, const char *name,
                                 int index, const char *value) {
  if (!h || !name || !value || index != 0) return kOfxStatErrBadIndex;
  auto &v = as_props(h)->values[name];
  v.kind = Value::Kind::String;
  v.string_value = value;
  return kOfxStatOK;
}

static OfxStatus prop_set_double(OfxPropertySetHandle h, const char *name,
                                 int index, double value) {
  if (!h || !name || index != 0) return kOfxStatErrBadIndex;
  auto &v = as_props(h)->values[name];
  v.kind = Value::Kind::Double;
  v.double_value = value;
  return kOfxStatOK;
}

static OfxStatus prop_set_int(OfxPropertySetHandle h, const char *name,
                              int index, int value) {
  if (!h || !name || index < 0) return kOfxStatErrBadIndex;
  auto &v = as_props(h)->values[name];
  v.kind = Value::Kind::Int;
  v.int_value = value;
  if (static_cast<int>(v.int_values.size()) <= index) {
    v.int_values.resize(static_cast<size_t>(index) + 1);
  }
  v.int_values[static_cast<size_t>(index)] = value;
  return kOfxStatOK;
}

static OfxStatus prop_set_string_n(OfxPropertySetHandle h, const char *name,
                                   int count, const char *const *value) {
  if (!h || !name || !value || count < 1) return kOfxStatErrBadIndex;
  return prop_set_string(h, name, 0, value[0]);
}

static OfxStatus prop_get_pointer(OfxPropertySetHandle h, const char *name,
                                  int index, void **value) {
  if (!h || !name || !value || index != 0) return kOfxStatErrBadIndex;
  auto it = as_props(h)->values.find(name);
  if (it == as_props(h)->values.end() || it->second.kind != Value::Kind::Pointer)
    return kOfxStatErrUnknown;
  *value = it->second.pointer_value;
  return kOfxStatOK;
}

static OfxStatus prop_get_string(OfxPropertySetHandle h, const char *name,
                                 int index, char **value) {
  if (!h || !name || !value || index != 0) return kOfxStatErrBadIndex;
  auto it = as_props(h)->values.find(name);
  if (it == as_props(h)->values.end() || it->second.kind != Value::Kind::String)
    return kOfxStatErrUnknown;
  *value = const_cast<char *>(it->second.string_value.c_str());
  return kOfxStatOK;
}

static OfxStatus prop_get_double(OfxPropertySetHandle h, const char *name,
                                 int index, double *value) {
  if (!h || !name || !value || index != 0) return kOfxStatErrBadIndex;
  auto it = as_props(h)->values.find(name);
  if (it == as_props(h)->values.end() || it->second.kind != Value::Kind::Double)
    return kOfxStatErrUnknown;
  *value = it->second.double_value;
  return kOfxStatOK;
}

static OfxStatus prop_get_int(OfxPropertySetHandle h, const char *name,
                              int index, int *value) {
  if (!h || !name || !value || index < 0) return kOfxStatErrBadIndex;
  auto it = as_props(h)->values.find(name);
  if (it == as_props(h)->values.end() || it->second.kind != Value::Kind::Int ||
      index >= static_cast<int>(it->second.int_values.size()))
    return kOfxStatErrUnknown;
  *value = it->second.int_values[static_cast<size_t>(index)];
  return kOfxStatOK;
}

static OfxStatus image_get_properties(OfxImageEffectHandle h,
                                      OfxPropertySetHandle *out) {
  if (!h || !out) return kOfxStatErrBadHandle;
  *out = reinterpret_cast<OfxPropertySetHandle>(
      &reinterpret_cast<FakeImageObject *>(h)->properties);
  return kOfxStatOK;
}

static OfxStatus image_get_params(OfxImageEffectHandle h,
                                  OfxParamSetHandle *out) {
  if (!h || !out) return kOfxStatErrBadHandle;
  auto *image = reinterpret_cast<FakeImageObject *>(h);
  image->params.owner = image;
  *out = reinterpret_cast<OfxParamSetHandle>(&image->params);
  return kOfxStatOK;
}

static OfxStatus image_clip_define(OfxImageEffectHandle h, const char *,
                                   OfxPropertySetHandle *out) {
  if (!h || !out) return kOfxStatErrBadHandle;
  auto *image = reinterpret_cast<FakeImageObject *>(h);
  image->children.emplace_back(std::make_unique<FakePropertySet>());
  *out = reinterpret_cast<OfxPropertySetHandle>(image->children.back().get());
  return kOfxStatOK;
}

static OfxStatus image_clip_get_handle(OfxImageEffectHandle h, const char *name,
                                        OfxImageClipHandle *clip,
                                        OfxPropertySetHandle *properties) {
  if (!h || !name || !clip || !properties) return kOfxStatErrBadHandle;
  auto *image = reinterpret_cast<FakeImageObject *>(h);
  FakeClip *selected = nullptr;
  if (std::strcmp(name, kOfxImageEffectSimpleSourceClipName) == 0) {
    selected = &image->source_clip;
  } else if (std::strcmp(name, kOfxImageEffectOutputClipName) == 0) {
    selected = &image->output_clip;
  }
  if (!selected) return kOfxStatErrUnknown;
  *clip = reinterpret_cast<OfxImageClipHandle>(selected);
  *properties = reinterpret_cast<OfxPropertySetHandle>(&selected->properties);
  return kOfxStatOK;
}

static OfxStatus image_clip_get_property_set(OfxImageClipHandle clip,
                                              OfxPropertySetHandle *out) {
  if (!clip || !out) return kOfxStatErrBadHandle;
  *out = reinterpret_cast<OfxPropertySetHandle>(
      &reinterpret_cast<FakeClip *>(clip)->properties);
  return kOfxStatOK;
}

static OfxStatus image_clip_get_image(OfxImageClipHandle clip, OfxTime time,
                                      const OfxRectD *,
                                      OfxPropertySetHandle *out) {
  if (!clip || !out) return kOfxStatErrBadHandle;
  auto *fake_clip = reinterpret_cast<FakeClip *>(clip);
  if (!fake_clip->owner) return kOfxStatErrBadHandle;
  if (fake_clip->source) fake_clip->owner->last_source_time = time;
  else fake_clip->owner->last_output_time = time;
  *out = reinterpret_cast<OfxPropertySetHandle>(
      fake_clip->source ? &fake_clip->owner->source_image
                        : &fake_clip->owner->output_image);
  return kOfxStatOK;
}

static OfxStatus image_clip_release_image(OfxPropertySetHandle) {
  return kOfxStatOK;
}

static OfxStatus parameter_define(OfxParamSetHandle h, const char *,
                                  const char *, OfxPropertySetHandle *out) {
  if (!h || !out) return kOfxStatErrBadHandle;
  auto *props = reinterpret_cast<FakePropertySet *>(h);
  props->values.emplace("defined", Value{});
  *out = reinterpret_cast<OfxPropertySetHandle>(props);
  return kOfxStatOK;
}

static OfxStatus parameter_get_handle(OfxParamSetHandle h, const char *name,
                                      OfxParamHandle *param,
                                      OfxPropertySetHandle *properties) {
  if (!h || !name || !param || std::strcmp(name, "strength") != 0) {
    return kOfxStatErrBadHandle;
  }
  auto *set = reinterpret_cast<FakePropertySet *>(h);
  auto *image = static_cast<FakeImageObject *>(set->owner);
  if (!image) return kOfxStatErrBadHandle;
  image->strength.properties.owner = image;
  *param = reinterpret_cast<OfxParamHandle>(&image->strength);
  if (properties) {
    *properties = reinterpret_cast<OfxPropertySetHandle>(
        &image->strength.properties);
  }
  return kOfxStatOK;
}

static OfxStatus parameter_get_value_at_time(OfxParamHandle handle,
                                             OfxTime time, ...) {
  if (!handle || !std::isfinite(time)) return kOfxStatErrBadHandle;
  auto *param = reinterpret_cast<FakeParam *>(handle);
  va_list args;
  va_start(args, time);
  auto *value = va_arg(args, double *);
  va_end(args);
  if (!value) return kOfxStatErrBadHandle;
  param->last_time = time;
  *value = param->value;
  return kOfxStatOK;
}

static OfxPropertySuiteV1 g_properties = {prop_set_pointer,
                                          prop_set_string,
                                          prop_set_double,
                                          prop_set_int,
                                          nullptr,
                                          prop_set_string_n,
                                          prop_get_pointer,
                                          prop_get_string,
                                          prop_get_double,
                                          prop_get_int};
static OfxImageEffectSuiteV1 g_image_effect = {image_get_properties,
                                                image_get_params,
                                                image_clip_define,
                                                image_clip_get_handle,
                                                image_clip_get_property_set,
                                                image_clip_get_image,
                                                image_clip_release_image};
static OfxParameterSuiteV1 g_parameters = {parameter_define, parameter_get_handle, nullptr, nullptr, nullptr, parameter_get_value_at_time};

static const void *fetch_suite(OfxPropertySetHandle, const char *name, int) {
  if (std::strcmp(name, kOfxPropertySuite) == 0) return &g_properties;
  if (std::strcmp(name, kOfxImageEffectSuite) == 0) return &g_image_effect;
  if (std::strcmp(name, kOfxParameterSuite) == 0) return &g_parameters;
  return nullptr;
}

static bool has_string(FakePropertySet &props, const char *name,
                       const char *expected) {
  auto it = props.values.find(name);
  return it != props.values.end() && it->second.kind == Value::Kind::String &&
         it->second.string_value == expected;
}

static void set_image_property(FakePropertySet &properties, const char *name,
                               void *data, int row_bytes) {
  prop_set_pointer(reinterpret_cast<OfxPropertySetHandle>(&properties),
                   kOfxImagePropData, 0, data);
  for (int index = 0; index < 4; ++index) {
    const int bounds[] = {0, 0, 3, 2};
    prop_set_int(reinterpret_cast<OfxPropertySetHandle>(&properties),
                 kOfxImagePropBounds, index, bounds[index]);
  }
  prop_set_int(reinterpret_cast<OfxPropertySetHandle>(&properties),
               kOfxImagePropRowBytes, 0, row_bytes);
  prop_set_string(reinterpret_cast<OfxPropertySetHandle>(&properties),
                  kOfxImageEffectPropPixelDepth, 0, kOfxBitDepthByte);
  prop_set_string(reinterpret_cast<OfxPropertySetHandle>(&properties),
                  kOfxImageEffectPropComponents, 0, kOfxImageComponentRGBA);
  prop_set_string(reinterpret_cast<OfxPropertySetHandle>(&properties),
                  kOfxImageEffectPropPreMultiplication, 0,
                  kOfxImagePreMultiplied);
}

static void initialize_fixture(FakeImageObject &instance) {
  instance.strength.value = 0.25;
  instance.source_clip.owner = &instance;
  instance.source_clip.source = true;
  instance.output_clip.owner = &instance;
  instance.output_clip.source = false;
  prop_set_string(reinterpret_cast<OfxPropertySetHandle>(
                      &instance.source_clip.properties),
                  kOfxImageEffectPropPixelDepth, 0, kOfxBitDepthByte);
  prop_set_string(reinterpret_cast<OfxPropertySetHandle>(
                      &instance.source_clip.properties),
                  kOfxImageEffectPropComponents, 0, kOfxImageComponentRGBA);
  prop_set_string(reinterpret_cast<OfxPropertySetHandle>(
                      &instance.output_clip.properties),
                  kOfxImageEffectPropPixelDepth, 0, kOfxBitDepthByte);
  prop_set_string(reinterpret_cast<OfxPropertySetHandle>(
                      &instance.output_clip.properties),
                  kOfxImageEffectPropComponents, 0, kOfxImageComponentRGBA);
  instance.source_pixels.assign(2 * 20, 0xEE);
  instance.output_pixels.assign(2 * 24, 0xCD);
  const unsigned char pixels[] = {
      10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120,
      130, 140, 150, 160, 170, 180, 190, 200, 210, 220, 230, 240};
  std::memcpy(instance.source_pixels.data(), pixels, sizeof(pixels));
  set_image_property(instance.source_image, kOfxImagePropData,
                     instance.source_pixels.data(), 20);
  set_image_property(instance.output_image, kOfxImagePropData,
                     instance.output_pixels.data(), 24);
}

static bool rendered_fixture_ok(const FakeImageObject &instance,
                                int *changed_bytes) {
  int changed = 0;
  for (int y = 0; y < 2; ++y) {
    const auto *source = instance.source_pixels.data() + y * 20;
    const auto *output = instance.output_pixels.data() + y * 24;
    for (int x = 0; x < 3; ++x) {
      const auto source_offset = x * 4;
      const auto output_offset = x * 4;
      if (x == 0) {
        for (int channel = 0; channel < 4; ++channel) {
          if (output[output_offset + channel] != 0xCD) return false;
        }
        continue;
      }
      for (int channel = 0; channel < 3; ++channel) {
        const auto expected = static_cast<unsigned char>(
            source[source_offset + channel] * 0.75 + 0.5);
        if (output[output_offset + channel] != expected) return false;
        ++changed;
      }
      if (output[output_offset + 3] != source[source_offset + 3]) return false;
    }
    for (int offset = 12; offset < 24; ++offset) {
      if (output[offset] != 0xCD) return false;
    }
  }
  if (changed_bytes) *changed_bytes = changed;
  return instance.last_source_time == 7.0 && instance.last_output_time == 7.0 && instance.strength.last_time == 7.0;
}

static std::string sha256(const std::vector<unsigned char> &bytes) {
#ifdef _WIN32
  BCRYPT_ALG_HANDLE algorithm = nullptr;
  if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr,
                                  0) < 0) {
    return "";
  }
  DWORD object_length = 0;
  DWORD result_length = 0;
  if (BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                        reinterpret_cast<PUCHAR>(&object_length),
                        sizeof(object_length), &result_length, 0) < 0) {
    BCryptCloseAlgorithmProvider(algorithm, 0);
    return "";
  }
  std::vector<unsigned char> object(object_length);
  BCRYPT_HASH_HANDLE hash = nullptr;
  if (BCryptCreateHash(algorithm, &hash, object.data(), object_length, nullptr, 0,
                       0) < 0 ||
      BCryptHashData(hash, const_cast<PUCHAR>(bytes.data()),
                     static_cast<ULONG>(bytes.size()), 0) < 0) {
    if (hash) BCryptDestroyHash(hash);
    BCryptCloseAlgorithmProvider(algorithm, 0);
    return "";
  }
  unsigned char digest[32] = {};
  if (BCryptFinishHash(hash, digest, sizeof(digest), 0) < 0) {
    BCryptDestroyHash(hash);
    BCryptCloseAlgorithmProvider(algorithm, 0);
    return "";
  }
  BCryptDestroyHash(hash);
  BCryptCloseAlgorithmProvider(algorithm, 0);
  std::ostringstream result;
  result << std::uppercase << std::hex << std::setfill('0');
  for (unsigned char byte : digest) result << std::setw(2) << static_cast<int>(byte);
  return result.str();
#else
  (void)bytes;
  return "";
#endif
}

#ifdef _WIN32
template <typename T> static T load_symbol(HMODULE module, const char *name) {
  return reinterpret_cast<T>(GetProcAddress(module, name));
}
#endif

int main(int argc, char **argv) {
#ifndef _WIN32
  std::cerr << "windows_only" << std::endl;
  return 2;
#else
  if (argc != 2) {
    std::cerr << "usage: resolve_ofx_smoke <plugin.ofx>" << std::endl;
    return 2;
  }
  HMODULE module = LoadLibraryA(argv[1]);
  if (!module) {
    std::cerr << "load_failed" << std::endl;
    return 3;
  }
  using GetNumber = int (*)();
  using GetPlugin = OfxPlugin *(*)(int);
  using SetHost = OfxStatus (*)(const OfxHost *);
  auto get_number = load_symbol<GetNumber>(module, "OfxGetNumberOfPlugins");
  auto get_plugin = load_symbol<GetPlugin>(module, "OfxGetPlugin");
  auto set_host = load_symbol<SetHost>(module, "OfxSetHost");
  auto plugin_main = load_symbol<OfxPluginEntryPoint *>(module, "OfxPluginMain");
  if (!get_number || !get_plugin || !set_host || !plugin_main ||
      get_number() != 1) {
    std::cerr << "exports_failed" << std::endl;
    FreeLibrary(module);
    return 4;
  }

  OfxHost host{nullptr, fetch_suite};
  if (set_host(&host) != kOfxStatOK) {
    std::cerr << "set_host_failed" << std::endl;
    FreeLibrary(module);
    return 5;
  }
  OfxPlugin *plugin = get_plugin(0);
  FakeImageObject descriptor;
  FakePropertySet context;
  prop_set_string(reinterpret_cast<OfxPropertySetHandle>(&context),
                  kOfxImageEffectPropContext, 0,
                  kOfxImageEffectContextFilter);
  const auto load_status = plugin->mainEntry(kOfxActionLoad, nullptr, nullptr, nullptr);
  const auto describe_status = plugin->mainEntry(
      kOfxActionDescribe, &descriptor, nullptr, nullptr);
  const auto context_status = plugin->mainEntry(
      kOfxImageEffectActionDescribeInContext, &descriptor,
      reinterpret_cast<OfxPropertySetHandle>(&context), nullptr);
  FakeImageObject instance;
  initialize_fixture(instance);
  const auto create_status = plugin->mainEntry(
      kOfxActionCreateInstance, &instance, nullptr, nullptr);
  FakePropertySet render_args;
  prop_set_double(reinterpret_cast<OfxPropertySetHandle>(&render_args),
                  kOfxPropTime, 0, 7.0);
  const int render_window[] = {1, 0, 3, 2};
  for (int index = 0; index < 4; ++index) {
    prop_set_int(reinterpret_cast<OfxPropertySetHandle>(&render_args),
                 kOfxImageEffectPropRenderWindow, index, render_window[index]);
  }
  const auto render_status = plugin->mainEntry(
      kOfxImageEffectActionRender, &instance,
      reinterpret_cast<OfxPropertySetHandle>(&render_args), nullptr);
  int changed_bytes = 0;
  const bool render_verified = rendered_fixture_ok(instance, &changed_bytes);
  const auto input_sha256 = sha256(instance.source_pixels);
  const auto output_sha256 = sha256(instance.output_pixels);
  const auto destroy_status = plugin->mainEntry(
      kOfxActionDestroyInstance, &instance, nullptr, nullptr);
  const auto unload_status = plugin->mainEntry(kOfxActionUnload, nullptr, nullptr, nullptr);
  const bool lifecycle_ok = load_status == kOfxStatOK &&
                            describe_status == kOfxStatOK &&
                            context_status == kOfxStatOK &&
                            create_status == kOfxStatOK &&
                            render_status == kOfxStatOK && render_verified &&
                            destroy_status == kOfxStatOK &&
                            unload_status == kOfxStatOK &&
                            has_string(descriptor.properties, kOfxPropLabel,
                                       "AEXCompat Resolve OFX");
  std::cout << "{\"schema_version\":1,\"plugin_identifier\":\""
            << plugin->pluginIdentifier << "\",\"load\":" << load_status
            << ",\"describe\":" << describe_status << ",\"describe_in_context\":"
            << context_status << ",\"create_instance\":" << create_status
            << ",\"render\":" << render_status
            << ",\"render_claim\":\"builtin_rgba8_control\""
            << ",\"aex_render_claim\":\"blocked_missing_aex_rendersession\""
            << ",\"destroy_instance\":" << destroy_status
            << ",\"unload\":" << unload_status
            << ",\"rgba8_contract\":true,\"stride_checked\":"
            << (render_verified ? "true" : "false")
            << ",\"alpha_preserved\":" << (render_verified ? "true" : "false")
            << ",\"frame_time\":7.0,\"parameter\":{\"name\":\"strength\",\"time\":7.0,\"value\":0.25},\"input_sha256\":\"" << input_sha256
            << "\",\"output_sha256\":\"" << output_sha256
            << "\",\"pixel_diff\":" << changed_bytes
            << ",\"lifecycle_ok\":"
            << (lifecycle_ok ? "true" : "false") << "}" << std::endl;
  FreeLibrary(module);
  return lifecycle_ok ? 0 : 6;
#endif
}
