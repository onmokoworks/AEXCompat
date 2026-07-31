#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"

#include <Windows.h>

#include <array>
#include <cstdio>
#include <cwchar>
#include <cstring>

namespace {
constexpr wchar_t kRuntimeBasename[] = L"aexcompat_wgpu_dx12_runtime.dll";
constexpr char kSetupMarker[] = "AEXCOMPAT_WGPU_DX12_SETUP=";
constexpr char kSetdownMarker[] = "AEXCOMPAT_WGPU_DX12_SETDOWN=";
constexpr size_t kDiagnosticCapacity = 16 * 1024;
constexpr size_t kPathCapacity = 32768;

using RuntimeFunction = int(__cdecl*)(char*, size_t);

HMODULE g_runtime = nullptr;
RuntimeFunction g_setup = nullptr;
RuntimeFunction g_setdown = nullptr;

void emit(const char* marker, const char* json) {
  std::fputs(marker, stderr);
  std::fputs(json, stderr);
  std::fputc('\n', stderr);
  std::fflush(stderr);
}

void emit_failure(const char* marker, const char* error) {
  std::array<char, 512> json{};
  std::snprintf(json.data(), json.size(),
                "{\"schema_version\":1,\"stage\":\"failed\","
                "\"backend\":\"dx12\",\"wgpu_version\":\"0.19.4\","
                "\"wgpu_compute_ready\":false,\"error\":\"%s\"}",
                error);
  emit(marker, json.data());
}

void clear_runtime() {
  g_setup = nullptr;
  g_setdown = nullptr;
  // Keep the companion module pinned until this one-shot worker exits. The
  // native worker intentionally pins the AEX for the same lifetime so its
  // final module audit can observe both sealed images. GPU objects themselves
  // are dropped by the Rust global-setdown call above.
}

HMODULE load_sealed_runtime() {
  HMODULE self = nullptr;
  if (!GetModuleHandleExW(
          GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
              GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
          reinterpret_cast<LPCWSTR>(&load_sealed_runtime), &self)) {
    return nullptr;
  }
  std::array<wchar_t, kPathCapacity> path{};
  const DWORD length =
      GetModuleFileNameW(self, path.data(), static_cast<DWORD>(path.size()));
  if (length == 0 || length >= path.size()) {
    return nullptr;
  }
  wchar_t* separator = std::wcsrchr(path.data(), L'\\');
  if (!separator) {
    return nullptr;
  }
  ++separator;
  const size_t remaining =
      path.size() - static_cast<size_t>(separator - path.data());
  if (wcsncpy_s(separator, remaining, kRuntimeBasename, _TRUNCATE) != 0) {
    return nullptr;
  }
  return LoadLibraryExW(path.data(), nullptr,
                        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR |
                            LOAD_LIBRARY_SEARCH_SYSTEM32);
}

PF_Err global_setup(PF_OutData* out_data) {
  if (!out_data || g_runtime) {
    return PF_Err_INTERNAL_STRUCT_DAMAGED;
  }
  out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
  out_data->out_flags = PF_OutFlag_NOP_RENDER | PF_OutFlag_PIX_INDEPENDENT;
  out_data->out_flags2 = 0;

  g_runtime = load_sealed_runtime();
  if (!g_runtime) {
    emit_failure(kSetupMarker, "sealed runtime artifact could not be loaded");
    return PF_Err_NONE;
  }
  g_setup = reinterpret_cast<RuntimeFunction>(
      GetProcAddress(g_runtime, "aexcompat_wgpu_dx12_global_setup"));
  g_setdown = reinterpret_cast<RuntimeFunction>(
      GetProcAddress(g_runtime, "aexcompat_wgpu_dx12_global_setdown"));
  if (!g_setup || !g_setdown) {
    emit_failure(kSetupMarker, "sealed runtime exports are missing");
    return PF_Err_NONE;
  }

  std::array<char, kDiagnosticCapacity> report{};
  const int status = g_setup(report.data(), report.size());
  if (status != 0 || report[0] == '\0') {
    emit_failure(kSetupMarker, "wgpu runtime setup diagnostic failed");
    return PF_Err_NONE;
  }
  emit(kSetupMarker, report.data());
  return PF_Err_NONE;
}

PF_Err global_setdown() {
  if (!g_runtime) {
    emit_failure(kSetdownMarker, "sealed runtime was not loaded");
    return PF_Err_NONE;
  }
  if (!g_setdown) {
    emit_failure(kSetdownMarker, "sealed runtime setdown export is missing");
    clear_runtime();
    return PF_Err_NONE;
  }
  std::array<char, kDiagnosticCapacity> report{};
  const int status = g_setdown(report.data(), report.size());
  if (status != 0 || report[0] == '\0') {
    emit_failure(kSetdownMarker, "wgpu runtime setdown diagnostic failed");
  } else {
    emit(kSetdownMarker, report.data());
  }
  clear_runtime();
  return PF_Err_NONE;
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData*,
                                        PF_OutData* out_data,
                                        PF_ParamDef*[], PF_LayerDef*, void*) {
  switch (cmd) {
    case PF_Cmd_ABOUT:
      if (out_data) {
        strncpy_s(out_data->return_msg, sizeof(out_data->return_msg),
                  "AEXCompat wgpu 0.19.4 DX12 compute probe", _TRUNCATE);
      }
      return PF_Err_NONE;
    case PF_Cmd_GLOBAL_SETUP:
      return global_setup(out_data);
    case PF_Cmd_PARAMS_SETUP:
      if (!out_data) return PF_Err_BAD_CALLBACK_PARAM;
      out_data->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_GLOBAL_SETDOWN:
      return global_setdown();
    case PF_Cmd_RENDER:
    case PF_Cmd_SMART_PRE_RENDER:
    case PF_Cmd_SMART_RENDER:
      return PF_Err_INTERNAL_STRUCT_DAMAGED;
    default:
      return PF_Err_NONE;
  }
}
