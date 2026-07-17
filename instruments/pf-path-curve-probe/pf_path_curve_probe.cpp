#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "AE_EffectSuites.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <limits>
#include <new>
#include <vector>

namespace {
constexpr std::array<A_long, 6> kFrequencies{{-1, 0, 1, 1023, 1024, 1025}};
constexpr std::size_t kLengthCaseCount = 10;
constexpr std::size_t kHeaderSize = 24;
constexpr std::size_t kRecordSize = 112;
constexpr std::array<std::uint8_t, 8> kMagic{{'P', 'F', 'P', 'C', 'U', 'R', 'V', 0}};
constexpr std::uint16_t kFormatVersion = 1;
constexpr std::uint64_t kSentinelBits = UINT64_C(0x7ff4a5a5deadbeef);

void put_u16(std::uint8_t* out, std::uint16_t value) {
  out[0] = static_cast<std::uint8_t>(value);
  out[1] = static_cast<std::uint8_t>(value >> 8);
}
void put_u32(std::uint8_t* out, std::uint32_t value) {
  for (int i = 0; i != 4; ++i) out[i] = static_cast<std::uint8_t>(value >> (i * 8));
}
void put_u64(std::uint8_t* out, std::uint64_t value) {
  for (int i = 0; i != 8; ++i) out[i] = static_cast<std::uint8_t>(value >> (i * 8));
}
std::uint64_t raw_bits(double value) {
  std::uint64_t bits = 0;
  static_assert(sizeof(bits) == sizeof(value), "PF_FpLong must be binary64");
  std::memcpy(&bits, &value, sizeof(bits));
  return bits;
}
double from_bits(std::uint64_t bits) {
  double value = 0;
  std::memcpy(&value, &bits, sizeof(value));
  return value;
}
std::uint32_t crc32(const std::uint8_t* data, std::size_t size) {
  std::uint32_t crc = UINT32_C(0xffffffff);
  for (std::size_t i = 0; i != size; ++i) {
    crc ^= data[i];
    for (int bit = 0; bit != 8; ++bit) {
      crc = (crc >> 1) ^ (UINT32_C(0xedb88320) & (0u - (crc & 1u)));
    }
  }
  return ~crc;
}

struct Record {
  A_long frequency = 0;
  std::uint32_t length_case = 0;
  double requested_length = 0;
  PF_Err prepare_error = PF_Err_NONE;
  PF_Err get_length_error = PF_Err_NONE;
  double segment_length = from_bits(kSentinelBits);
  std::array<std::uint8_t, 5> prep_state{};
  PF_Err eval_error = PF_Err_NONE;
  double eval_x = from_bits(kSentinelBits);
  double eval_y = from_bits(kSentinelBits);
  PF_Err deriv_error = PF_Err_NONE;
  double deriv_x = from_bits(kSentinelBits);
  double deriv_y = from_bits(kSentinelBits);
  double deriv_dx = from_bits(kSentinelBits);
  double deriv_dy = from_bits(kSentinelBits);
  PF_Err cleanup_error = PF_Err_NONE;
};

void encode_record(const Record& r, std::uint8_t* out) {
  std::memset(out, 0, kRecordSize);
  put_u32(out + 0, static_cast<std::uint32_t>(r.frequency));
  put_u32(out + 4, r.length_case);
  put_u64(out + 8, raw_bits(r.requested_length));
  put_u32(out + 16, static_cast<std::uint32_t>(r.prepare_error));
  put_u32(out + 20, static_cast<std::uint32_t>(r.get_length_error));
  put_u64(out + 24, raw_bits(r.segment_length));
  std::copy(r.prep_state.begin(), r.prep_state.end(), out + 32);
  put_u64(out + 40, kSentinelBits);
  put_u32(out + 48, static_cast<std::uint32_t>(r.eval_error));
  put_u64(out + 52, raw_bits(r.eval_x));
  put_u64(out + 60, raw_bits(r.eval_y));
  put_u32(out + 68, static_cast<std::uint32_t>(r.deriv_error));
  put_u64(out + 72, raw_bits(r.deriv_x));
  put_u64(out + 80, raw_bits(r.deriv_y));
  put_u64(out + 88, raw_bits(r.deriv_dx));
  put_u64(out + 96, raw_bits(r.deriv_dy));
  put_u32(out + 104, static_cast<std::uint32_t>(r.cleanup_error));
}

PF_Err setup_parameters(PF_InData* in_data, PF_OutData* out_data) {
  PF_ParamDef definition{};
  definition.param_type = PF_Param_PATH;
  definition.uu.id = 1;
  constexpr char kParameterName[] = "Curved Path";
  std::memcpy(definition.name, kParameterName, sizeof(kParameterName));
  definition.u.path_d.dephault = 0;
  const PF_Err error = PF_ADD_PARAM(in_data, -1, &definition);
  if (!error) out_data->num_params = 2;
  return error;
}

void retain_first_error(PF_Err error, PF_Err* result) {
  if (error != PF_Err_NONE && *result == PF_Err_NONE) *result = error;
}

std::array<double, kLengthCaseCount> length_cases(double length) {
  const double inf = std::numeric_limits<double>::infinity();
  return {{-inf, -1.0, -0.0, 0.0, std::nextafter(0.0, inf),
           std::nextafter(length, -inf), length, std::nextafter(length, inf),
           inf, std::numeric_limits<double>::quiet_NaN()}};
}

template <typename Pixel, typename Component>
void paint_bytes(PF_LayerDef* output, const std::vector<std::uint8_t>& bytes) {
  std::size_t cursor = 0;
  for (A_long y = 0; y < output->height; ++y) {
    auto* row = reinterpret_cast<Pixel*>(reinterpret_cast<A_u_char*>(output->data) +
                                         y * output->rowbytes);
    for (A_long x = 0; x < output->width; ++x) {
      Component channels[3]{};
      for (int channel = 0; channel != 3; ++channel) {
        const std::uint8_t byte = cursor < bytes.size() ? bytes[cursor++] : 0;
        channels[channel] = static_cast<Component>(byte) *
                            static_cast<Component>(sizeof(Component) == 1 ? 1 : 257);
      }
      row[x].alpha = static_cast<Component>(sizeof(Component) == 1 ? 255 : 32768);
      row[x].red = channels[0];
      row[x].green = channels[1];
      row[x].blue = channels[2];
    }
  }
}

bool checked_output_capacity(const PF_LayerDef& output, std::size_t& capacity) {
  if (!output.data || output.width <= 0 || output.height <= 0 || output.rowbytes <= 0)
    return false;
  const std::size_t width = static_cast<std::size_t>(output.width);
  const std::size_t height = static_cast<std::size_t>(output.height);
  const std::size_t pixel_size =
      output.world_flags & PF_WorldFlag_DEEP ? sizeof(PF_Pixel16) : sizeof(PF_Pixel);
  if (width > std::numeric_limits<std::size_t>::max() / pixel_size ||
      static_cast<std::size_t>(output.rowbytes) < width * pixel_size ||
      width > std::numeric_limits<std::size_t>::max() / height ||
      width * height > std::numeric_limits<std::size_t>::max() / 3)
    return false;
  capacity = width * height * 3;
  return true;
}

PF_Err render(PF_InData* in_data, PF_ParamDef* params[], PF_LayerDef* output) {
  if (!in_data || !in_data->pica_basicP || !params || !params[1] || !output ||
      !output->data || output->width <= 0 || output->height <= 0 || output->rowbytes <= 0) {
    return PF_Err_BAD_CALLBACK_PARAM;
  }
  std::size_t capacity = 0;
  if (!checked_output_capacity(*output, capacity)) return PF_Err_BAD_CALLBACK_PARAM;
  const std::size_t record_count = kFrequencies.size() * kLengthCaseCount;
  const std::size_t encoded_size = kHeaderSize + record_count * kRecordSize;
  if (capacity < encoded_size) return PF_Err_BAD_CALLBACK_PARAM;

  const PF_PathQuerySuite1* query = nullptr;
  const PF_PathDataSuite1* data = nullptr;
  PF_PathOutlinePtr path = nullptr;
  bool query_acquired = false, data_acquired = false;
  PF_Err result = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
      kPFPathQuerySuite, kPFPathQuerySuiteVersion1, reinterpret_cast<const void**>(&query)));
  query_acquired = !result;
  if (!result) {
    result = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
        kPFPathDataSuite, kPFPathDataSuiteVersion1, reinterpret_cast<const void**>(&data)));
    data_acquired = !result;
  }
  const PF_PathID path_id = params[1]->u.path_d.path_id;
  if (!result && (!query || !data || path_id == PF_PathID_NONE)) result = PF_Err_BAD_CALLBACK_PARAM;
  if (!result) result = query->PF_CheckoutPath(in_data->effect_ref, path_id,
      in_data->current_time, in_data->time_step, in_data->time_scale, &path);
  if (!result && !path) result = PF_Err_BAD_CALLBACK_PARAM;

  std::vector<Record> records;
  records.reserve(record_count);
  if (!result) {
    for (A_long frequency : kFrequencies) {
      PF_PathSegPrepPtr prep = nullptr;
      const PF_Err prepare_error = data->PF_PathPrepareSegLength(
          in_data->effect_ref, path, 0, frequency, &prep);
      const bool prep_after_prepare = prep != nullptr;
      double segment_length = from_bits(kSentinelBits);
      const PF_Err length_error = prepare_error ? prepare_error : data->PF_PathGetSegLength(
          in_data->effect_ref, path, 0, &prep, &segment_length);
      const bool prep_after_get = prep != nullptr;
      const auto cases = length_cases(segment_length);
      const std::size_t first_record = records.size();
      for (std::size_t case_index = 0; case_index != cases.size(); ++case_index) {
        Record r;
        r.frequency = frequency;
        r.length_case = static_cast<std::uint32_t>(case_index);
        r.requested_length = cases[case_index];
        r.prepare_error = prepare_error;
        r.get_length_error = length_error;
        r.segment_length = segment_length;
        r.prep_state[0] = prep_after_prepare;
        r.prep_state[1] = prep_after_get;
        r.eval_error = length_error ? length_error : data->PF_PathEvalSegLength(
            in_data->effect_ref, path, &prep, 0, r.requested_length, &r.eval_x, &r.eval_y);
        r.prep_state[2] = prep != nullptr;
        r.deriv_error = length_error ? length_error : data->PF_PathEvalSegLengthDeriv1(
            in_data->effect_ref, path, &prep, 0, r.requested_length, &r.deriv_x,
            &r.deriv_y, &r.deriv_dx, &r.deriv_dy);
        r.prep_state[3] = prep != nullptr;
        records.push_back(r);
      }
      PF_Err cleanup_error = PF_Err_NONE;
      if (prep) cleanup_error = data->PF_PathCleanupSegLength(in_data->effect_ref, path, 0, &prep);
      for (std::size_t i = first_record; i != records.size(); ++i) {
        records[i].cleanup_error = cleanup_error;
        records[i].prep_state[4] = prep != nullptr;
      }
      // Prepare/eval/cleanup failures are observations, not render failures.
    }
  }

  if (!result) {
    std::vector<std::uint8_t> bytes(encoded_size, 0);
    std::copy(kMagic.begin(), kMagic.end(), bytes.begin());
    put_u16(bytes.data() + 8, kFormatVersion);
    put_u16(bytes.data() + 10, static_cast<std::uint16_t>(kHeaderSize));
    put_u32(bytes.data() + 12, static_cast<std::uint32_t>(kRecordSize));
    put_u32(bytes.data() + 16, static_cast<std::uint32_t>(records.size()));
    for (std::size_t i = 0; i != records.size(); ++i)
      encode_record(records[i], bytes.data() + kHeaderSize + i * kRecordSize);
    put_u32(bytes.data() + 20, crc32(bytes.data() + kHeaderSize, bytes.size() - kHeaderSize));
    if (output->world_flags & PF_WorldFlag_DEEP)
      paint_bytes<PF_Pixel16, A_u_short>(output, bytes);
    else
      paint_bytes<PF_Pixel, A_u_char>(output, bytes);
  }

  if (path) retain_first_error(query->PF_CheckinPath(in_data->effect_ref, path_id, FALSE, path), &result);
  if (data_acquired) retain_first_error(static_cast<PF_Err>(in_data->pica_basicP->ReleaseSuite(
      kPFPathDataSuite, kPFPathDataSuiteVersion1)), &result);
  if (query_acquired) retain_first_error(static_cast<PF_Err>(in_data->pica_basicP->ReleaseSuite(
      kPFPathQuerySuite, kPFPathQuerySuiteVersion1)), &result);
  return result;
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef* params[],
                                        PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      if (!out_data) return PF_Err_BAD_CALLBACK_PARAM;
      out_data->my_version = PF_VERSION(1, 1, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_DEEP_COLOR_AWARE;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      return in_data && out_data ? setup_parameters(in_data, out_data)
                                 : PF_Err_BAD_CALLBACK_PARAM;
    case PF_Cmd_RENDER:
      try {
        return render(in_data, params, output);
      } catch (const std::bad_alloc&) {
        return PF_Err_OUT_OF_MEMORY;
      } catch (...) {
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
      }
    default: return PF_Err_NONE;
  }
}
