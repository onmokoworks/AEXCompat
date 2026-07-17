#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_AdvEffectSuites.h"

#include <array>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <new>
#include <sstream>
#include <string>
#include <windows.h>

namespace {
constexpr A_u_char kGuard = 0xa7;
constexpr A_long kSentinel = static_cast<A_long>(0x5a5aa55aUL);

struct GuardedText {
  std::array<A_u_char, 16> before{};
  std::array<A_char, PF_MAX_TIME_LEN + 1> text{};
  std::array<A_u_char, 16> after{};
  GuardedText() { before.fill(kGuard); text.fill(static_cast<A_char>(0x5d)); after.fill(kGuard); }
  bool intact() const {
    for (auto b : before) if (b != kGuard) return false;
    for (auto b : after) if (b != kGuard) return false;
    return true;
  }
};

std::string escaped(const GuardedText& value) {
  std::ostringstream out;
  out << '"';
  for (std::size_t i = 0; i < value.text.size() && value.text[i]; ++i) {
    const unsigned char c = static_cast<unsigned char>(value.text[i]);
    if (c == '"' || c == '\\') out << '\\' << static_cast<char>(c);
    else if (c >= 0x20 && c < 0x7f) out << static_cast<char>(c);
    else { char hex[8]{}; std::snprintf(hex, sizeof(hex), "\\u%04x", c); out << hex; }
  }
  out << '"';
  return out.str();
}

struct Lease {
  SPBasicSuite* basic{};
  const PF_AdvTimeSuite4* suite{};
  bool owns{};
  SPErr acquire_err{};
  SPErr release_err{kSentinel};
  A_long acquire_count{};
  A_long release_count{};
  explicit Lease(PF_InData* in) : basic(in ? in->pica_basicP : nullptr) {
    if (!basic) { acquire_err = PF_Err_BAD_CALLBACK_PARAM; return; }
    const void* raw = nullptr;
    acquire_err = basic->AcquireSuite(kPFAdvTimeSuite, kPFAdvTimeSuiteVersion4, &raw);
    if (!acquire_err) {
      owns = true;
      suite = static_cast<const PF_AdvTimeSuite4*>(raw);
      acquire_count = 1;
    }
  }
  void release() noexcept {
    if (owns && !release_count) {
      release_err = basic->ReleaseSuite(kPFAdvTimeSuite, kPFAdvTimeSuiteVersion4);
      release_count = 1; owns = false; suite = nullptr;
    }
  }
  ~Lease() noexcept { release(); }
};

LONG record_exception(EXCEPTION_POINTERS* info, A_long* code) {
  *code = (info && info->ExceptionRecord)
      ? static_cast<A_long>(info->ExceptionRecord->ExceptionCode) : kSentinel;
  return EXCEPTION_EXECUTE_HANDLER;
}

template <class F> PF_Err guarded(F fn, A_long& exception_code) {
  PF_Err err = kSentinel; exception_code = 0;
  __try { err = fn(); }
  __except(record_exception(GetExceptionInformation(), &exception_code)) {}
  return err;
}

template <class F>
void text_case(std::ostringstream& out, const char* name, const F& fn) {
  GuardedText value; A_long exception = 0;
  const PF_Err err = guarded([&]() { return fn(value.text.data()); }, exception);
  out << "{\"case\":\"" << name << "\",\"err\":" << err
      << ",\"exception\":" << exception << ",\"guard_intact\":"
      << (value.intact() ? "true" : "false") << ",\"raw\":" << escaped(value) << '}';
}

void count_case(std::ostringstream& out, const PF_AdvTimeSuite4* s, const char* name,
                const A_Time* start, const A_Time* step, A_Boolean partial, A_long* result) {
  A_long exception = 0;
  const PF_Err err = guarded([&]() { return s->PF_TimeCountFrames(start, step, partial, result); }, exception);
  out << "{\"case\":\"" << name << "\",\"err\":" << err
      << ",\"exception\":" << exception << ",\"raw_output\":"
      << (result ? *result : kSentinel) << '}';
}

std::string observe(PF_InData* in, PF_EffectWorld* world, const char* phase) {
  Lease lease(in); std::ostringstream out;
  out << "{\"schema\":1,\"phase\":\"" << phase << "\",\"suite\":\""
      << kPFAdvTimeSuite << "\",\"version\":" << kPFAdvTimeSuiteVersion4
      << ",\"acquire_err\":" << lease.acquire_err << ",\"acquire_count\":" << lease.acquire_count;
  if (!lease.suite) {
    lease.release();
    out << ",\"observation_error\":\"acquired_null_suite\",\"release_err\":"
        << lease.release_err << ",\"release_count\":" << lease.release_count
        << ",\"lease_balanced\":" << (lease.acquire_count == lease.release_count ? "true" : "false") << '}';
    return out.str();
  }
  const auto* s = lease.suite;
  out << ",\"format_cases\":[";
  text_case(out, "active_positive", [&](A_char* b){ return s->PF_FormatTimeActiveItem(1001, 30000, FALSE, b); }); out << ',';
  text_case(out, "active_negative_duration", [&](A_char* b){ return s->PF_FormatTimeActiveItem(-1001, 30000, TRUE, b); }); out << ',';
  text_case(out, "active_scale_zero", [&](A_char* b){ return s->PF_FormatTimeActiveItem(1, 0, FALSE, b); }); out << ',';
  text_case(out, "world_time", [&](A_char* b){ return s->PF_FormatTime(in, world, in->current_time, in->time_scale, FALSE, b); }); out << ',';
  text_case(out, "world_duration", [&](A_char* b){ return s->PF_FormatTime(in, world, in->time_step, in->time_scale, TRUE, b); }); out << ',';
  text_case(out, "plus_layer_time", [&](A_char* b){ return s->PF_FormatTimePlus(in, world, in->current_time, in->time_scale, FALSE, FALSE, b); }); out << ',';
  text_case(out, "plus_comp_duration", [&](A_char* b){ return s->PF_FormatTimePlus(in, world, in->time_step, in->time_scale, TRUE, TRUE, b); }); out << ',';
  text_case(out, "active_null_output", [&](A_char*){ return s->PF_FormatTimeActiveItem(1, 1, FALSE, nullptr); });
  PF_TimeDisplayPrefVersion3 pref{}; std::memset(&pref, 0x5d, sizeof(pref)); A_long starting = kSentinel, pref_exception = 0;
  const PF_Err pref_err = guarded([&](){ return s->PF_GetTimeDisplayPref(&pref, &starting); }, pref_exception);
  A_long null_pref_exception = 0; const PF_Err null_pref_err = guarded([&](){ return s->PF_GetTimeDisplayPref(nullptr, nullptr); }, null_pref_exception);
  out << "],\"display_pref\":{\"err\":" << pref_err << ",\"exception\":" << pref_exception
      << ",\"raw_bytes\":\"";
  const auto* bytes = reinterpret_cast<const A_u_char*>(&pref);
  for (std::size_t i=0; i<sizeof(pref); ++i) { char h[3]{}; std::snprintf(h,sizeof(h),"%02x",bytes[i]); out << h; }
  out << "\",\"starting_frame\":" << starting << ",\"null_err\":" << null_pref_err
      << ",\"null_exception\":" << null_pref_exception << "},\"count_cases\":[";
  const A_Time exact_start{100, 100}, exact_step{25, 100}, partial_start{101, 100};
  const A_Time invalid_step{1, 0}, overflow_start{0x7fffffff, 1}, overflow_step{1, 0x7fffffff};
  A_long counts[5]{kSentinel,kSentinel,kSentinel,kSentinel,kSentinel};
  count_case(out,s,"exact",&exact_start,&exact_step,FALSE,&counts[0]); out << ',';
  count_case(out,s,"partial_excluded",&partial_start,&exact_step,FALSE,&counts[1]); out << ',';
  count_case(out,s,"partial_included",&partial_start,&exact_step,TRUE,&counts[2]); out << ',';
  count_case(out,s,"invalid_scale_zero",&exact_start,&invalid_step,FALSE,&counts[3]); out << ',';
  count_case(out,s,"overflow",&overflow_start,&overflow_step,TRUE,&counts[4]); out << ',';
  count_case(out,s,"null_inputs",nullptr,nullptr,FALSE,nullptr);
  lease.release();
  out << "],\"release_err\":" << lease.release_err << ",\"release_count\":" << lease.release_count
      << ",\"lease_balanced\":" << (lease.acquire_count == lease.release_count ? "true" : "false") << '}';
  return out.str();
}

void emit(const std::string& json) {
  std::fprintf(stdout, "AEXCOMPAT_PF_ADV_TIME_V4 %s\n", json.c_str()); std::fflush(stdout);
  OutputDebugStringA(("AEXCOMPAT_PF_ADV_TIME_V4 " + json + "\n").c_str());
}

struct SidecarWriteResult {
  bool ok{};
  const char* stage{"none"};
  DWORD win32_error{};
};

SidecarWriteResult write_atomic_sidecar(const std::string& payload) {
  char* temp_dir = nullptr;
  std::size_t temp_size = 0;
  if ((_dupenv_s(&temp_dir, &temp_size, "TEMP") || !temp_dir) &&
      (_dupenv_s(&temp_dir, &temp_size, "TMP") || !temp_dir))
    return {false, "resolve_temp", ERROR_PATH_NOT_FOUND};
  const std::string final_path = std::string(temp_dir) + "\\aexcompat-pf-adv-time-v4.json";
  std::free(temp_dir);
  std::ostringstream temp_name;
  temp_name << final_path << '.' << GetCurrentProcessId() << '.' << GetCurrentThreadId() << ".tmp";
  const std::string temp_path = temp_name.str();
  HANDLE file = CreateFileA(temp_path.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS,
                            FILE_ATTRIBUTE_NORMAL, nullptr);
  if (file == INVALID_HANDLE_VALUE) return {false, "create_temp", GetLastError()};
  const char* cursor = payload.data();
  std::size_t remaining = payload.size();
  while (remaining) {
    const DWORD chunk = remaining > MAXDWORD ? MAXDWORD : static_cast<DWORD>(remaining);
    DWORD written = 0;
    if (!WriteFile(file, cursor, chunk, &written, nullptr) || written != chunk) {
      const DWORD error = GetLastError(); CloseHandle(file); DeleteFileA(temp_path.c_str());
      return {false, "write_temp", error ? error : ERROR_WRITE_FAULT};
    }
    cursor += written; remaining -= written;
  }
  if (!FlushFileBuffers(file)) {
    const DWORD error = GetLastError(); CloseHandle(file); DeleteFileA(temp_path.c_str());
    return {false, "flush_temp", error};
  }
  if (!CloseHandle(file)) {
    const DWORD error = GetLastError(); DeleteFileA(temp_path.c_str());
    return {false, "close_temp", error};
  }
  if (!MoveFileExA(temp_path.c_str(), final_path.c_str(),
                   MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) {
    const DWORD error = GetLastError(); DeleteFileA(temp_path.c_str());
    return {false, "rename_temp", error};
  }
  return {true, "complete", ERROR_SUCCESS};
}

void emit_sidecar_status(const SidecarWriteResult& result) {
  std::ostringstream status;
  status << "AEXCOMPAT_PF_ADV_TIME_V4_SIDECAR {\"ok\":" << (result.ok ? "true" : "false")
         << ",\"stage\":\"" << result.stage << "\",\"win32_error\":" << result.win32_error << "}\n";
  std::fputs(status.str().c_str(), stdout); std::fflush(stdout);
  OutputDebugStringA(status.str().c_str());
}

PF_Err render(PF_InData* in, PF_EffectWorld* output) {
  const std::string json = observe(in, output, "render"); emit(json);
  emit_sidecar_status(write_atomic_sidecar(json));
  constexpr std::size_t kMaxImageCapacity = 512u * 1024u * 1024u;
  if (!output || !output->data || output->width < 0 || output->height < 0 || output->rowbytes < 0)
    return PF_Err_BAD_CALLBACK_PARAM;
  const auto width = static_cast<std::size_t>(output->width);
  const auto height = static_cast<std::size_t>(output->height);
  const auto rowbytes = static_cast<std::size_t>(output->rowbytes);
  if (width > (std::numeric_limits<std::size_t>::max)() / sizeof(PF_Pixel8) ||
      width * sizeof(PF_Pixel8) > rowbytes ||
      (height && rowbytes > (std::numeric_limits<std::size_t>::max)() / height) ||
      rowbytes * height > kMaxImageCapacity)
    return PF_Err_BAD_CALLBACK_PARAM;
  A_u_long hash = 2166136261u;
  for (const unsigned char c : json) { hash ^= c; hash *= 16777619u; }
  for (std::size_t y=0; y<height; ++y) {
    auto* row = reinterpret_cast<PF_Pixel8*>(reinterpret_cast<A_u_char*>(output->data) + y*rowbytes);
    for (std::size_t x=0; x<width; ++x) row[x] = PF_Pixel8{255, static_cast<A_u_char>(hash>>16), static_cast<A_u_char>(hash>>8), static_cast<A_u_char>(hash)};
  }
  return PF_Err_NONE;
}
} // namespace

PF_Err effect_main_impl(PF_Cmd cmd, PF_InData* in, PF_OutData* out, PF_LayerDef* output) {
  if (!in || !out) return PF_Err_BAD_CALLBACK_PARAM;
  if (cmd == PF_Cmd_GLOBAL_SETUP) {
    out->my_version = PF_VERSION(1,0,0,PF_Stage_DEVELOP,0); out->out_flags = PF_OutFlag_PIX_INDEPENDENT;
    emit(observe(in, nullptr, "global_setup")); return PF_Err_NONE;
  }
  if (cmd == PF_Cmd_PARAMS_SETUP) { out->num_params = 1; return PF_Err_NONE; }
  if (cmd == PF_Cmd_RENDER) return render(in, output);
  return PF_Err_NONE;
}

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in, PF_OutData* out,
                                         PF_ParamDef*[], PF_LayerDef* output, void*) {
  try {
    return effect_main_impl(cmd, in, out, output);
  } catch (const std::bad_alloc&) {
    return PF_Err_OUT_OF_MEMORY;
  } catch (...) {
    return PF_Err_INTERNAL_STRUCT_DAMAGED;
  }
}
