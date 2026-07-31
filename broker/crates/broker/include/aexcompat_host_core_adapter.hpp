#pragma once

#include <windows.h>

#include <array>
#include <cstdint>
#include <utility>

#include "aexcompat_host_core_abi.h"

#if !defined(_WIN32) || !defined(_MSC_VER)
#error \
    "aexcompat_host_core_adapter.hpp requires a Windows MSVC-compatible SEH compiler"
#endif

namespace aexcompat::host_core {

enum class AdapterLoadStatus : uint32_t {
  kOk = 0,
  kInvalidArgument = 1,
  kPathResolutionFailed = 2,
  kLoadLibraryFailed = 3,
  kMissingExport = 4,
  kMissingAbiDescriptor = 5,
  kIncompatibleAbiDescriptor = 6,
};

struct ApiV1 {
  AexHostCoreSessionCreateV1Fn session_create = nullptr;
  AexHostCoreSessionCallV1Fn session_open = nullptr;
  AexHostCoreSessionCallV1Fn session_begin_callback = nullptr;
  AexHostCoreSessionCallV1Fn session_end_callback = nullptr;
  AexHostCoreSessionCallV1Fn session_close = nullptr;
  AexHostCoreSessionCallV1Fn session_dispose = nullptr;

  bool complete() const noexcept {
    return session_create != nullptr && session_open != nullptr &&
           session_begin_callback != nullptr &&
           session_end_callback != nullptr && session_close != nullptr &&
           session_dispose != nullptr;
  }
};

struct SehCallResult {
  int32_t return_code = AEX_HOST_INVALID_STATE;
  uint32_t exception_code = 0;
};

struct Invocation {
  int32_t return_code = AEX_HOST_INVALID_STATE;
  uint32_t exception_code = 0;
  AexHostCallStatus status{};
  AexHostReportSnapshot report{};
  AexHostOpaqueHandle created_handle{};
};

// Owns one loaded host-core DLL and its complete v1 function table. Callers
// must dispose all DLL-owned handles and finish concurrent calls before this
// object is moved, replaced through Load, or destroyed.
class AdapterV1 {
 public:
  AdapterV1() noexcept = default;

  ~AdapterV1() noexcept { Reset(); }

  AdapterV1(const AdapterV1 &) = delete;
  AdapterV1 &operator=(const AdapterV1 &) = delete;

  AdapterV1(AdapterV1 &&other) noexcept { MoveFrom(std::move(other)); }

  AdapterV1 &operator=(AdapterV1 &&other) noexcept {
    if (this != &other) {
      Reset();
      MoveFrom(std::move(other));
    }
    return *this;
  }

  static bool IsCompatibleDescriptor(
      const AexHostCoreAbiDescriptorV1 &descriptor) noexcept {
    return descriptor.magic == AEXCOMPAT_HOST_CORE_ABI_DESCRIPTOR_MAGIC &&
           descriptor.abi_version == AEXCOMPAT_HOST_CORE_ABI_VERSION &&
           descriptor.struct_size == sizeof(AexHostCoreAbiDescriptorV1) &&
           descriptor.call_context_size == sizeof(AexHostCallContext) &&
           descriptor.call_context_alignment == alignof(AexHostCallContext) &&
           descriptor.call_status_size == sizeof(AexHostCallStatus) &&
           descriptor.call_status_alignment == alignof(AexHostCallStatus) &&
           descriptor.opaque_handle_size == sizeof(AexHostOpaqueHandle) &&
           descriptor.opaque_handle_alignment == alignof(AexHostOpaqueHandle) &&
           descriptor.report_snapshot_size == sizeof(AexHostReportSnapshot) &&
           descriptor.report_snapshot_alignment ==
               alignof(AexHostReportSnapshot) &&
           descriptor.capabilities ==
               AEXCOMPAT_HOST_CORE_CAPABILITY_SESSION_LIFECYCLE_V1;
  }

  static AdapterLoadStatus ValidateApi(const ApiV1 &api) noexcept {
    return api.complete() ? AdapterLoadStatus::kOk
                          : AdapterLoadStatus::kMissingExport;
  }

  static AdapterLoadStatus Load(const wchar_t *dll_path,
                                 AdapterV1 *output) noexcept {
    if (output == nullptr) {
      return AdapterLoadStatus::kInvalidArgument;
    }
    if (dll_path == nullptr || dll_path[0] == L'\0') {
      output->Reset();
      return AdapterLoadStatus::kInvalidArgument;
    }

    std::array<wchar_t, 32768> absolute_path{};
    const DWORD written = GetFullPathNameW(
        dll_path, static_cast<DWORD>(absolute_path.size()),
        absolute_path.data(), nullptr);
    const DWORD path_error =
        written == 0 ? GetLastError() : ERROR_INSUFFICIENT_BUFFER;
    output->Reset();
    if (written == 0 ||
        written >= static_cast<DWORD>(absolute_path.size())) {
      output->native_error_ = path_error;
      return AdapterLoadStatus::kPathResolutionFailed;
    }
    output->absolute_path_ = absolute_path;

    HMODULE module = LoadLibraryExW(
        output->absolute_path_.data(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (module == nullptr) {
      output->native_error_ = GetLastError();
      return AdapterLoadStatus::kLoadLibraryFailed;
    }

    const FARPROC descriptor_symbol =
        GetProcAddress(module, "aex_host_core_abi_descriptor_v1");
    if (descriptor_symbol == nullptr) {
      output->native_error_ = ERROR_PROC_NOT_FOUND;
      FreeLibrary(module);
      return AdapterLoadStatus::kMissingAbiDescriptor;
    }
    const auto *published_descriptor =
        reinterpret_cast<const AexHostCoreAbiDescriptorV1 *>(
            descriptor_symbol);
    AexHostCoreAbiDescriptorV1 descriptor{};
    DWORD descriptor_error = ERROR_SUCCESS;
    if (!CopyDescriptor(published_descriptor, &descriptor,
                        &descriptor_error)) {
      output->native_error_ = descriptor_error;
      FreeLibrary(module);
      return AdapterLoadStatus::kIncompatibleAbiDescriptor;
    }
    if (!IsCompatibleDescriptor(descriptor)) {
      output->native_error_ = ERROR_BAD_FORMAT;
      FreeLibrary(module);
      return AdapterLoadStatus::kIncompatibleAbiDescriptor;
    }

    ApiV1 api;
    api.session_create = Resolve<AexHostCoreSessionCreateV1Fn>(
        module, "aex_host_core_session_create_v1");
    api.session_open = Resolve<AexHostCoreSessionCallV1Fn>(
        module, "aex_host_core_session_open_v1");
    api.session_begin_callback = Resolve<AexHostCoreSessionCallV1Fn>(
        module, "aex_host_core_session_begin_callback_v1");
    api.session_end_callback = Resolve<AexHostCoreSessionCallV1Fn>(
        module, "aex_host_core_session_end_callback_v1");
    api.session_close = Resolve<AexHostCoreSessionCallV1Fn>(
        module, "aex_host_core_session_close_v1");
    api.session_dispose = Resolve<AexHostCoreSessionCallV1Fn>(
        module, "aex_host_core_session_dispose_v1");
    if (ValidateApi(api) != AdapterLoadStatus::kOk) {
      output->native_error_ = ERROR_PROC_NOT_FOUND;
      FreeLibrary(module);
      return AdapterLoadStatus::kMissingExport;
    }

    output->module_ = module;
    output->api_ = api;
    output->descriptor_ = descriptor;
    output->native_error_ = ERROR_SUCCESS;
    return AdapterLoadStatus::kOk;
  }

  bool loaded() const noexcept {
    return module_ != nullptr && IsCompatibleDescriptor(descriptor_) &&
           api_.complete();
  }

  DWORD native_error() const noexcept { return native_error_; }

  const wchar_t *absolute_path() const noexcept {
    return absolute_path_.data();
  }

  const ApiV1 &api() const noexcept { return api_; }

  const AexHostCoreAbiDescriptorV1 &descriptor() const noexcept {
    return descriptor_;
  }

  // Normal callers use value copies so the pointers passed to Rust always
  // refer to adapter-owned storage inside the SEH frame.
  Invocation Create(AexHostCallContext context) const noexcept {
    Invocation invocation;
    const SehCallResult result =
        InvokeCreateRaw(api_.session_create, &context,
                        &invocation.created_handle,
                        &invocation.status, &invocation.report);
    invocation.return_code = result.return_code;
    invocation.exception_code = result.exception_code;
    return invocation;
  }

  Invocation Open(AexHostCallContext context,
                  AexHostOpaqueHandle session) const noexcept {
    return InvokeSession(api_.session_open, context, session);
  }

  Invocation BeginCallback(AexHostCallContext context,
                           AexHostOpaqueHandle session) const noexcept {
    return InvokeSession(api_.session_begin_callback, context, session);
  }

  Invocation EndCallback(AexHostCallContext context,
                         AexHostOpaqueHandle session) const noexcept {
    return InvokeSession(api_.session_end_callback, context, session);
  }

  Invocation Close(AexHostCallContext context,
                   AexHostOpaqueHandle session) const noexcept {
    return InvokeSession(api_.session_close, context, session);
  }

  Invocation Dispose(AexHostCallContext context,
                     AexHostOpaqueHandle session) const noexcept {
    return InvokeSession(api_.session_dispose, context, session);
  }

  // Low-level ABI conformance hooks. Each non-null pointer must remain valid;
  // production callers should prefer the value-taking methods above.
  static SehCallResult InvokeCreateRaw(AexHostCoreSessionCreateV1Fn function,
                                       const AexHostCallContext *context,
                                       AexHostOpaqueHandle *session,
                                       AexHostCallStatus *status,
                                       AexHostReportSnapshot *report) noexcept {
    if (function == nullptr) {
      NormalizeBoundaryFailure(AEX_HOST_INVALID_STATE, false, status, report);
      return SehCallResult{AEX_HOST_INVALID_STATE, 0};
    }

    SehCallResult result{AEX_HOST_SEH_FAULT, 0};
    __try {
      result.return_code = function(context, session, status, report);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
      result.return_code = AEX_HOST_SEH_FAULT;
      result.exception_code = static_cast<uint32_t>(GetExceptionCode());
      if (session != nullptr) {
        session->value = 0;
      }
      NormalizeBoundaryFailure(AEX_HOST_SEH_FAULT, true, status, report);
    }
    return result;
  }

  static SehCallResult InvokeSessionRaw(
      AexHostCoreSessionCallV1Fn function, const AexHostCallContext *context,
      AexHostOpaqueHandle session, AexHostCallStatus *status,
      AexHostReportSnapshot *report) noexcept {
    if (function == nullptr) {
      NormalizeBoundaryFailure(AEX_HOST_INVALID_STATE, false, status, report);
      return SehCallResult{AEX_HOST_INVALID_STATE, 0};
    }

    SehCallResult result{AEX_HOST_SEH_FAULT, 0};
    __try {
      result.return_code = function(context, session, status, report);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
      result.return_code = AEX_HOST_SEH_FAULT;
      result.exception_code = static_cast<uint32_t>(GetExceptionCode());
      NormalizeBoundaryFailure(AEX_HOST_SEH_FAULT, true, status, report);
    }
    return result;
  }

 private:
  static bool CopyDescriptor(
      const AexHostCoreAbiDescriptorV1 *source,
      AexHostCoreAbiDescriptorV1 *destination,
      DWORD *native_error) noexcept {
    if (source == nullptr || destination == nullptr || native_error == nullptr) {
      return false;
    }
    bool copied = false;
    __try {
      // Read only the fixed identity prefix until the publisher proves that
      // the complete v1 descriptor is present.
      destination->magic = source->magic;
      destination->abi_version = source->abi_version;
      destination->struct_size = source->struct_size;
      if (destination->magic == AEXCOMPAT_HOST_CORE_ABI_DESCRIPTOR_MAGIC &&
          destination->abi_version == AEXCOMPAT_HOST_CORE_ABI_VERSION &&
          destination->struct_size == sizeof(AexHostCoreAbiDescriptorV1)) {
        *destination = *source;
      }
      copied = true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
      *native_error = static_cast<DWORD>(GetExceptionCode());
    }
    return copied;
  }

  static Invocation InvokeSession(AexHostCoreSessionCallV1Fn function,
                                  AexHostCallContext context,
                                  AexHostOpaqueHandle session) noexcept {
    Invocation invocation;
    const SehCallResult result = InvokeSessionRaw(
        function, &context, session, &invocation.status, &invocation.report);
    invocation.return_code = result.return_code;
    invocation.exception_code = result.exception_code;
    return invocation;
  }

  static void NormalizeBoundaryFailure(int32_t code, bool faulted,
                                       AexHostCallStatus *status,
                                       AexHostReportSnapshot *report) noexcept {
    // report_id zero identifies an adapter-local failure for which Rust could
    // not allocate or publish a report. Status and snapshot remain correlated.
    if (status != nullptr) {
      *status = AexHostCallStatus{
          AEXCOMPAT_HOST_CORE_ABI_VERSION,
          static_cast<uint32_t>(sizeof(AexHostCallStatus)),
          code,
          0,
          0,
      };
    }
    if (report != nullptr) {
      *report = AexHostReportSnapshot{
          AEXCOMPAT_HOST_CORE_ABI_VERSION,
          static_cast<uint32_t>(sizeof(AexHostReportSnapshot)),
          1,
          static_cast<uint32_t>(AEX_HOST_REPORT_PHASE_BOUNDARY),
          static_cast<uint32_t>(faulted ? AEX_HOST_REPORT_OUTCOME_FAULTED
                                        : AEX_HOST_REPORT_OUTCOME_REJECTED),
          code,
          static_cast<uint32_t>(AEX_HOST_HANDLE_KIND_NONE),
          static_cast<uint32_t>(AEX_HOST_SESSION_STATE_NONE),
          0,
          0,
          0,
          0,
          0,
      };
    }
  }

  template <typename Function>
  static Function Resolve(HMODULE module, const char *name) noexcept {
    return reinterpret_cast<Function>(GetProcAddress(module, name));
  }

  void Reset() noexcept {
    if (module_ != nullptr) {
      FreeLibrary(module_);
    }
    module_ = nullptr;
    api_ = ApiV1{};
    descriptor_ = AexHostCoreAbiDescriptorV1{};
    absolute_path_.fill(L'\0');
    native_error_ = ERROR_SUCCESS;
  }

  void MoveFrom(AdapterV1 &&other) noexcept {
    module_ = other.module_;
    api_ = other.api_;
    descriptor_ = other.descriptor_;
    absolute_path_ = other.absolute_path_;
    native_error_ = other.native_error_;
    other.module_ = nullptr;
    other.api_ = ApiV1{};
    other.descriptor_ = AexHostCoreAbiDescriptorV1{};
    other.absolute_path_.fill(L'\0');
    other.native_error_ = ERROR_SUCCESS;
  }

  HMODULE module_ = nullptr;
  ApiV1 api_;
  AexHostCoreAbiDescriptorV1 descriptor_{};
  std::array<wchar_t, 32768> absolute_path_{};
  DWORD native_error_ = ERROR_SUCCESS;
};

}  // namespace aexcompat::host_core
