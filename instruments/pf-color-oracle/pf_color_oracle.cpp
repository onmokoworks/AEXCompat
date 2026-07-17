#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"

#include <array>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iomanip>
#include <limits>
#include <new>
#include <sstream>
#include <string>

namespace {
template <class T> std::string hex_bits(const T& value) {
  static_assert(sizeof(T) == 4, "oracle values must be 32-bit");
  A_u_long bits = 0;
  std::memcpy(&bits, &value, sizeof(bits));
  std::ostringstream out;
  out << "0x" << std::hex << std::setw(8) << std::setfill('0') << bits;
  return out.str();
}

std::string fixed3(const PF_Fixed value[3]) {
  std::ostringstream out;
  out << "[\"" << hex_bits(value[0]) << "\",\"" << hex_bits(value[1])
      << "\",\"" << hex_bits(value[2]) << "\"]";
  return out.str();
}

template <class Pixel> std::string pixel_json(const Pixel& p) {
  std::ostringstream out;
  out << "[" << static_cast<unsigned long long>(p.alpha) << ","
      << static_cast<unsigned long long>(p.red) << ","
      << static_cast<unsigned long long>(p.green) << ","
      << static_cast<unsigned long long>(p.blue) << "]";
  return out.str();
}
template <> std::string pixel_json(const PF_PixelFloat& p) {
  return "[\"" + hex_bits(p.alpha) + "\",\"" + hex_bits(p.red) + "\",\"" +
      hex_bits(p.green) + "\",\"" + hex_bits(p.blue) + "\"]";
}

template <class Callbacks, class Pixel, class Scalar>
void append_case(std::ostringstream& out, const char* source, const char* name,
                 const Callbacks* cb, PF_ProgPtr ref, Pixel input, bool& first) {
  PF_HLS_Pixel hls{};
  PF_YIQ_Pixel yiq{};
  Pixel hls_rgb{}, yiq_rgb{};
  Scalar lum{}, hue{}, light{}, sat{};
  const PF_Err e_hls = cb->RGBtoHLS(ref, &input, hls);
  const PF_Err e_hls_rgb = cb->HLStoRGB(ref, hls, &hls_rgb);
  const PF_Err e_yiq = cb->RGBtoYIQ(ref, &input, yiq);
  const PF_Err e_yiq_rgb = cb->YIQtoRGB(ref, yiq, &yiq_rgb);
  const PF_Err e_lum = cb->Luminance(ref, &input, &lum);
  const PF_Err e_hue = cb->Hue(ref, &input, &hue);
  const PF_Err e_light = cb->Lightness(ref, &input, &light);
  const PF_Err e_sat = cb->Saturation(ref, &input, &sat);
  if (!first) out << ',';
  first = false;
  out << "{\"source\":\"" << source << "\",\"case\":\"" << name
      << "\",\"input_argb\":" << pixel_json(input)
      << ",\"errors\":[" << e_hls << ',' << e_hls_rgb << ',' << e_yiq << ','
      << e_yiq_rgb << ',' << e_lum << ',' << e_hue << ',' << e_light << ',' << e_sat
      << "],\"hls_bits\":" << fixed3(hls) << ",\"hls_rgb_argb\":" << pixel_json(hls_rgb)
      << ",\"yiq_bits\":" << fixed3(yiq) << ",\"yiq_rgb_argb\":" << pixel_json(yiq_rgb);
  if constexpr (sizeof(Scalar) == sizeof(float) && std::numeric_limits<Scalar>::is_iec559) {
    out << ",\"scalar_bits\":[\"" << hex_bits(lum) << "\",\"" << hex_bits(hue)
        << "\",\"" << hex_bits(light) << "\",\"" << hex_bits(sat) << "\"]";
  } else {
    out << ",\"scalars\":[" << lum << ',' << hue << ',' << light << ',' << sat << ']';
  }
  out << '}';
}

template <class Callbacks>
bool callbacks_complete(const Callbacks* cb) {
  return cb && cb->RGBtoHLS && cb->HLStoRGB && cb->RGBtoYIQ && cb->YIQtoRGB &&
      cb->Luminance && cb->Hue && cb->Lightness && cb->Saturation;
}

template <class Callbacks, class Pixel>
void append_inverse_case(std::ostringstream& out, const char* source, const char* name,
                         const Callbacks* cb, PF_ProgPtr ref, const PF_Fixed hls[3],
                         const PF_Fixed yiq[3], bool& first) {
  Pixel hls_rgb{}, yiq_rgb{};
  PF_HLS_Pixel hls_value{hls[0], hls[1], hls[2]};
  PF_YIQ_Pixel yiq_value{yiq[0], yiq[1], yiq[2]};
  const PF_Err hls_err = cb->HLStoRGB(ref, hls_value, &hls_rgb);
  const PF_Err yiq_err = cb->YIQtoRGB(ref, yiq_value, &yiq_rgb);
  if (!first) out << ',';
  first = false;
  out << "{\"source\":\"" << source << "\",\"case\":\"" << name
      << "\",\"kind\":\"manual_inverse\",\"hls_bits\":" << fixed3(hls)
      << ",\"hls_error\":" << hls_err << ",\"hls_rgb_argb\":" << pixel_json(hls_rgb)
      << ",\"yiq_bits\":" << fixed3(yiq) << ",\"yiq_error\":" << yiq_err
      << ",\"yiq_rgb_argb\":" << pixel_json(yiq_rgb) << '}';
}

template <class Callbacks, class Pixel>
void append_manual_inverses(std::ostringstream& out, const char* source,
                            const Callbacks* cb, PF_ProgPtr ref, bool& first) {
  const PF_Fixed hls_gray[3] = {0x00000000, 0x00008000, 0x00000000};
  const PF_Fixed yiq_gray[3] = {0x00008000, 0x00000000, 0x00000000};
  append_inverse_case<Callbacks, Pixel>(out, source, "manual_gray_half", cb, ref,
                                        hls_gray, yiq_gray, first);
  const PF_Fixed hls_color[3] = {0x00005555, 0x00008000, 0x00010000};
  const PF_Fixed yiq_color[3] = {0x00008000, 0x00004000, static_cast<PF_Fixed>(0xffffe000)};
  append_inverse_case<Callbacks, Pixel>(out, source, "manual_chroma", cb, ref,
                                        hls_color, yiq_color, first);
}

template <class Callbacks>
void append_8(std::ostringstream& out, const char* source, const Callbacks* cb,
              PF_ProgPtr ref, bool& first) {
  const std::array<std::pair<const char*, PF_Pixel>, 12> cases{{
      {"black", {255,0,0,0}}, {"white", {255,255,255,255}}, {"red", {255,255,0,0}},
      {"green", {255,0,255,0}}, {"blue", {255,0,0,255}},
      {"gray_127", {255,127,127,127}}, {"gray_128", {255,128,128,128}},
      {"achromatic_minus_1lsb", {255,127,128,128}},
      {"achromatic_plus_1lsb", {255,129,128,128}},
      {"halfway", {255,128,0,255}}, {"boundary_mix", {0,0,1,255}}}};
  for (const auto& item : cases)
    append_case<Callbacks, PF_Pixel, A_long>(out, source, item.first, cb, ref, item.second, first);
  append_manual_inverses<Callbacks, PF_Pixel>(out, source, cb, ref, first);
}

void append_16(std::ostringstream& out, const PF_ColorCallbacks16Suite1* cb,
               PF_ProgPtr ref, bool& first) {
  const std::array<std::pair<const char*, PF_Pixel16>, 12> cases{{
      {"black", {32768,0,0,0}}, {"white", {32768,32768,32768,32768}},
      {"red", {32768,32768,0,0}}, {"green", {32768,0,32768,0}},
      {"blue", {32768,0,0,32768}}, {"gray_16383", {32768,16383,16383,16383}},
      {"gray_16384", {32768,16384,16384,16384}},
      {"achromatic_minus_1lsb", {32768,16383,16384,16384}},
      {"achromatic_plus_1lsb", {32768,16385,16384,16384}},
      {"halfway", {32768,16384,0,32768}},
      {"boundary_mix", {0,0,1,32768}}}};
  for (const auto& item : cases)
    append_case<PF_ColorCallbacks16Suite1, PF_Pixel16, A_long>(out, "suite16", item.first,
        cb, ref, item.second, first);
  append_manual_inverses<PF_ColorCallbacks16Suite1, PF_Pixel16>(
      out, "suite16", cb, ref, first);
}

float from_bits(A_u_long bits) { float value; std::memcpy(&value, &bits, sizeof(value)); return value; }
void append_float(std::ostringstream& out, const PF_ColorCallbacksFloatSuite1* cb,
                  PF_ProgPtr ref, bool& first) {
  const float nan = from_bits(0x7fc00001u), inf = from_bits(0x7f800000u);
  const std::array<std::pair<const char*, PF_PixelFloat>, 18> cases{{
      {"black", {1,0,0,0}}, {"white", {1,1,1,1}}, {"red", {1,1,0,0}},
      {"green", {1,0,1,0}}, {"blue", {1,0,0,1}}, {"gray", {1,.5f,.5f,.5f}},
      {"negative", {1,-.25f,.25f,.75f}}, {"over_one", {1,1.5f,2,-1}},
      {"positive_zero", {1,from_bits(0x00000000u),0,0}},
      {"negative_zero", {1,from_bits(0x80000000u),0,0}},
      {"positive_subnormal", {1,from_bits(0x00000001u),0,0}},
      {"negative_subnormal", {1,from_bits(0x80000001u),0,0}},
      {"min_normal", {1,from_bits(0x00800000u),0,0}},
      {"nextafter_half_down", {1,from_bits(0x3effffffu),.5f,.5f}},
      {"nextafter_half_up", {1,from_bits(0x3f000001u),.5f,.5f}},
      {"max_finite", {1,from_bits(0x7f7fffffu),0,0}},
      {"nan", {1,nan,.5f,.25f}}, {"infinity", {1,inf,-inf,1}}}};
  for (const auto& item : cases)
    append_case<PF_ColorCallbacksFloatSuite1, PF_PixelFloat, float>(out, "suite_float",
        item.first, cb, ref, item.second, first);
  append_manual_inverses<PF_ColorCallbacksFloatSuite1, PF_PixelFloat>(
      out, "suite_float", cb, ref, first);
}

bool checked_world_bytes(const PF_LayerDef& world, size_t& bytes) {
  if (!world.data || world.rowbytes <= 0 || world.height <= 0) return false;
  const size_t rowbytes = static_cast<size_t>(world.rowbytes);
  const size_t height = static_cast<size_t>(world.height);
  if (height > std::numeric_limits<size_t>::max() / rowbytes) return false;
  bytes = rowbytes * height;
  return true;
}

bool write_atomic(const std::string& payload) {
  char* temp = nullptr;
  size_t temp_size = 0;
  if (_dupenv_s(&temp, &temp_size, "TEMP") || !temp)
    if (_dupenv_s(&temp, &temp_size, "TMP") || !temp) return false;
  const std::string final_path = std::string(temp) + "\\aexcompat-pf-color-oracle.json";
  std::free(temp);
  const std::string temp_path = final_path + ".tmp";
  FILE* file = nullptr;
  if (fopen_s(&file, temp_path.c_str(), "wb") || !file) return false;
  const bool ok = std::fwrite(payload.data(), 1, payload.size(), file) == payload.size() &&
      std::fclose(file) == 0;
  if (!ok) { std::remove(temp_path.c_str()); return false; }
  std::remove(final_path.c_str());
  return std::rename(temp_path.c_str(), final_path.c_str()) == 0;
}

struct ColorSuiteLeases {
  SPBasicSuite* basic = nullptr;
  bool suite8 = false;
  bool suite16 = false;
  bool suite_float = false;
  SPErr release8 = 0;
  SPErr release16 = 0;
  SPErr release_float = 0;

  explicit ColorSuiteLeases(SPBasicSuite* basic_suite) noexcept : basic(basic_suite) {}
  ColorSuiteLeases(const ColorSuiteLeases&) = delete;
  ColorSuiteLeases& operator=(const ColorSuiteLeases&) = delete;

  void release_all() noexcept {
    if (suite_float) {
      release_float = basic->ReleaseSuite(
          kPFColorCallbacksFloatSuite, kPFColorCallbacksFloatSuiteVersion1);
      suite_float = false;
    }
    if (suite16) {
      release16 = basic->ReleaseSuite(
          kPFColorCallbacks16Suite, kPFColorCallbacks16SuiteVersion1);
      suite16 = false;
    }
    if (suite8) {
      release8 = basic->ReleaseSuite(
          kPFColorCallbacksSuite, kPFColorCallbacksSuiteVersion1);
      suite8 = false;
    }
  }

  ~ColorSuiteLeases() noexcept { release_all(); }
};

PF_Err render(PF_InData* in, PF_LayerDef* output) {
  size_t output_bytes = 0;
  if (!in || !in->pica_basicP || !in->utils || !output ||
      !checked_world_bytes(*output, output_bytes))
    return PF_Err_BAD_CALLBACK_PARAM;
  const PF_ColorCallbacksSuite1* suite8 = nullptr;
  const PF_ColorCallbacks16Suite1* suite16 = nullptr;
  const PF_ColorCallbacksFloatSuite1* suite_float = nullptr;
  ColorSuiteLeases leases(in->pica_basicP);
  const SPErr a8 = in->pica_basicP->AcquireSuite(kPFColorCallbacksSuite,
      kPFColorCallbacksSuiteVersion1, reinterpret_cast<const void**>(&suite8));
  leases.suite8 = !a8;
  const SPErr a16 = in->pica_basicP->AcquireSuite(kPFColorCallbacks16Suite,
      kPFColorCallbacks16SuiteVersion1, reinterpret_cast<const void**>(&suite16));
  leases.suite16 = !a16;
  const SPErr af = in->pica_basicP->AcquireSuite(kPFColorCallbacksFloatSuite,
      kPFColorCallbacksFloatSuiteVersion1, reinterpret_cast<const void**>(&suite_float));
  leases.suite_float = !af;
  std::ostringstream json;
  const bool legacy_ok = callbacks_complete(&in->utils->colorCB);
  const bool suite8_ok = !a8 && callbacks_complete(suite8);
  const bool suite16_ok = !a16 && callbacks_complete(suite16);
  const bool float_ok = !af && callbacks_complete(suite_float);
  json << "{\"schema_version\":2,\"status\":\"captured\",\"mfr_policy\":\"not_declared_supported\",\"acquire_errors\":{" 
       << "\"PF Color Suite v1\":" << a8 << ",\"PF Color16 Suite v1\":" << a16
       << ",\"PF ColorFloat Suite v1\":" << af << "},\"error_order\":["
       << "\"RGBtoHLS\",\"HLStoRGB\",\"RGBtoYIQ\",\"YIQtoRGB\",\"Luminance\","
       << "\"Hue\",\"Lightness\",\"Saturation\"],\"callbacks_complete\":{" 
       << "\"legacy8\":" << (legacy_ok ? "true" : "false")
       << ",\"suite8\":" << (suite8_ok ? "true" : "false")
       << ",\"suite16\":" << (suite16_ok ? "true" : "false")
       << ",\"suite_float\":" << (float_ok ? "true" : "false") << "},\"records\":[";
  bool first = true;
  if (legacy_ok) append_8(json, "legacy8", &in->utils->colorCB, in->effect_ref, first);
  if (suite8_ok) append_8(json, "suite8", suite8, in->effect_ref, first);
  if (suite16_ok) append_16(json, suite16, in->effect_ref, first);
  if (float_ok) append_float(json, suite_float, in->effect_ref, first);
  const bool release_float_attempted = leases.suite_float;
  const bool release16_attempted = leases.suite16;
  const bool release8_attempted = leases.suite8;
  leases.release_all();
  const SPErr rf = leases.release_float;
  const SPErr r16 = leases.release16;
  const SPErr r8 = leases.release8;
  json << "],\"release\":{\"suite8\":{" << "\"attempted\":" << (release8_attempted ? "true" : "false")
       << ",\"error\":" << r8 << "},\"suite16\":{\"attempted\":" << (release16_attempted ? "true" : "false")
       << ",\"error\":" << r16 << "},\"suite_float\":{\"attempted\":" << (release_float_attempted ? "true" : "false")
       << ",\"error\":" << rf << "}}}";
  const bool wrote = write_atomic(json.str());
  std::memset(output->data, 0, output_bytes);
  return (wrote && !r8 && !r16 && !rf) ? PF_Err_NONE : PF_Err_INTERNAL_STRUCT_DAMAGED;
}
}

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in, PF_OutData* out,
    PF_ParamDef*[], PF_LayerDef* output, void*) {
  try {
    if (cmd == PF_Cmd_GLOBAL_SETUP) {
      if (!out) return PF_Err_BAD_CALLBACK_PARAM;
      out->my_version = PF_VERSION(1,0,0,PF_Stage_DEVELOP,0);
      out->out_flags = PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_DEEP_COLOR_AWARE;
      // Deliberately omit PF_OutFlag2_SUPPORTS_THREADED_RENDERING: one capture file per render.
      out->out_flags2 = PF_OutFlag2_FLOAT_COLOR_AWARE;
      return PF_Err_NONE;
    }
    if (cmd == PF_Cmd_PARAMS_SETUP) {
      if (!out) return PF_Err_BAD_CALLBACK_PARAM;
      out->num_params = 1; return PF_Err_NONE;
    }
    return cmd == PF_Cmd_RENDER ? render(in, output) : PF_Err_NONE;
  } catch (const std::bad_alloc&) {
    return PF_Err_OUT_OF_MEMORY;
  } catch (...) {
    return PF_Err_INTERNAL_STRUCT_DAMAGED;
  }
}
