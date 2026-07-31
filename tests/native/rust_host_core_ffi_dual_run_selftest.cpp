#include "aexcompat_host_core_adapter.hpp"

#include <cstdint>
#include <cstdio>
#include <thread>

namespace {

using aexcompat::host_core::AdapterLoadStatus;
using aexcompat::host_core::AdapterV1;
using aexcompat::host_core::ApiV1;
using aexcompat::host_core::Invocation;
using aexcompat::host_core::SehCallResult;

constexpr DWORD kSyntheticSehCode = 0xE0421001UL;

int32_t AEXCOMPAT_HOST_CORE_CALL SyntheticSehCreate(
    const AexHostCallContext *context, AexHostOpaqueHandle *session,
    AexHostCallStatus *status, AexHostReportSnapshot *report) {
  (void)context;
  if (session != nullptr) {
    session->value = ~uint64_t{0};
  }
  if (status != nullptr) {
    status->code = AEX_HOST_OK;
    status->report_id = ~uint64_t{0};
  }
  if (report != nullptr) {
    report->outcome = AEX_HOST_REPORT_OUTCOME_PASSED;
    report->report_id = ~uint64_t{0};
  }
#if defined(_MSC_VER)
  RaiseException(kSyntheticSehCode, EXCEPTION_NONCONTINUABLE, 0, nullptr);
#endif
  return AEX_HOST_SEH_FAULT;
}

Invocation InvokeRawCreate(const AdapterV1 &adapter,
                           const AexHostCallContext *context,
                           AexHostOpaqueHandle *session) {
  Invocation invocation;
  const SehCallResult result = AdapterV1::InvokeCreateRaw(
      adapter.api().session_create, context, session, &invocation.status,
      &invocation.report);
  invocation.return_code = result.return_code;
  invocation.exception_code = result.exception_code;
  if (session != nullptr) {
    invocation.created_handle = *session;
  }
  return invocation;
}

struct ExpectedCounters {
  uint64_t handles_created = 0;
  uint64_t handles_disposed = 0;
  uint64_t callbacks_attempted = 0;
  uint64_t callbacks_completed = 0;
};

struct ExpectedCall {
  int32_t code = AEX_HOST_OK;
  uint32_t session_state = AEX_HOST_SESSION_STATE_NONE;
  ExpectedCounters counters;
};

enum class OracleHandle {
  Invalid,
  Session,
};

enum class SessionOperation {
  Open,
  BeginCallback,
  EndCallback,
  Close,
};

class NativeSessionOracle {
 public:
  ExpectedCall Create(const AexHostCallContext *context, bool has_output) {
    if (!has_output || !ValidContext(context)) {
      return Failure(AEX_HOST_INVALID_ARGUMENT, AEX_HOST_SESSION_STATE_NONE);
    }
    owner_ = context->session_id;
    caller_thread_token_ = context->caller_thread_token;
    origin_thread_ = std::this_thread::get_id();
    known_ = true;
    live_ = true;
    state_ = AEX_HOST_SESSION_STATE_CREATED;
    ++counters_.handles_created;
    return Success(state_);
  }

  ExpectedCall Call(const AexHostCallContext *context, OracleHandle handle,
                    SessionOperation operation) {
    const ExpectedCall resolution = Resolve(context, handle);
    if (resolution.code != AEX_HOST_OK) {
      return resolution;
    }

    switch (operation) {
      case SessionOperation::Open:
        if (state_ != AEX_HOST_SESSION_STATE_CREATED) {
          return Failure(AEX_HOST_INVALID_STATE, state_);
        }
        state_ = AEX_HOST_SESSION_STATE_OPEN;
        break;
      case SessionOperation::BeginCallback:
        if (state_ != AEX_HOST_SESSION_STATE_OPEN) {
          return Failure(AEX_HOST_INVALID_STATE, state_);
        }
        state_ = AEX_HOST_SESSION_STATE_IN_CALLBACK;
        ++counters_.callbacks_attempted;
        break;
      case SessionOperation::EndCallback:
        if (state_ != AEX_HOST_SESSION_STATE_IN_CALLBACK) {
          return Failure(AEX_HOST_INVALID_STATE, state_);
        }
        state_ = AEX_HOST_SESSION_STATE_OPEN;
        ++counters_.callbacks_completed;
        break;
      case SessionOperation::Close:
        if (state_ != AEX_HOST_SESSION_STATE_OPEN) {
          return Failure(AEX_HOST_INVALID_STATE, state_);
        }
        state_ = AEX_HOST_SESSION_STATE_CLOSED;
        break;
    }
    return Success(state_);
  }

  ExpectedCall Dispose(const AexHostCallContext *context, OracleHandle handle) {
    const ExpectedCall resolution = Resolve(context, handle);
    if (resolution.code != AEX_HOST_OK) {
      return resolution;
    }
    if (state_ != AEX_HOST_SESSION_STATE_CLOSED &&
        state_ != AEX_HOST_SESSION_STATE_FAULTED) {
      return Failure(AEX_HOST_INVALID_STATE, state_);
    }
    const uint32_t terminal_state = state_;
    live_ = false;
    ++counters_.handles_disposed;
    return Success(terminal_state);
  }

 private:
  static bool ValidContext(const AexHostCallContext *context) {
    return context != nullptr &&
           context->abi_version == AEXCOMPAT_HOST_CORE_ABI_VERSION &&
           context->struct_size == sizeof(AexHostCallContext) &&
           context->session_id != 0 && context->caller_thread_token != 0;
  }

  ExpectedCall Resolve(const AexHostCallContext *context,
                       OracleHandle handle) const {
    if (!ValidContext(context)) {
      return Failure(AEX_HOST_INVALID_ARGUMENT, AEX_HOST_SESSION_STATE_NONE);
    }
    if (handle == OracleHandle::Invalid || !known_) {
      return Failure(AEX_HOST_INVALID_HANDLE, AEX_HOST_SESSION_STATE_NONE);
    }
    if (!live_) {
      return Failure(AEX_HOST_STALE_HANDLE, AEX_HOST_SESSION_STATE_NONE);
    }
    if (context->session_id != owner_) {
      return Failure(AEX_HOST_WRONG_OWNER, AEX_HOST_SESSION_STATE_NONE);
    }
    if (context->caller_thread_token != caller_thread_token_ ||
        std::this_thread::get_id() != origin_thread_) {
      return Failure(AEX_HOST_WRONG_THREAD, state_);
    }
    return Success(state_);
  }

  ExpectedCall Success(uint32_t state) const {
    return ExpectedCall{AEX_HOST_OK, state, counters_};
  }

  ExpectedCall Failure(int32_t code, uint32_t state) const {
    return ExpectedCall{code, state, counters_};
  }

  uint64_t owner_ = 0;
  uint64_t caller_thread_token_ = 0;
  std::thread::id origin_thread_;
  bool known_ = false;
  bool live_ = false;
  uint32_t state_ = AEX_HOST_SESSION_STATE_NONE;
  ExpectedCounters counters_;
};

class Verifier {
 public:
  void Verify(const char *label, const Invocation &actual,
              const ExpectedCall &expected) {
    Equal(label, "SEH exception", actual.exception_code, 0);
    Equal(label, "return code", actual.return_code, expected.code);
    Equal(label, "status ABI", actual.status.abi_version,
          AEXCOMPAT_HOST_CORE_ABI_VERSION);
    Equal(label, "status size", actual.status.struct_size,
          sizeof(AexHostCallStatus));
    Equal(label, "status code", actual.status.code, expected.code);
    Equal(label, "status reserved", actual.status.reserved, 0);
    Equal(label, "report ABI", actual.report.abi_version,
          AEXCOMPAT_HOST_CORE_ABI_VERSION);
    Equal(label, "report size", actual.report.struct_size,
          sizeof(AexHostReportSnapshot));
    Equal(label, "report schema", actual.report.schema_version, 1);
    Equal(label, "report phase", actual.report.phase,
          AEX_HOST_REPORT_PHASE_SESSION);
    Equal(label, "report outcome", actual.report.outcome,
          expected.code == AEX_HOST_OK ? AEX_HOST_REPORT_OUTCOME_PASSED
                                       : AEX_HOST_REPORT_OUTCOME_REJECTED);
    Equal(label, "report error", actual.report.error_code, expected.code);
    Equal(label, "handle kind", actual.report.handle_kind,
          AEX_HOST_HANDLE_KIND_SESSION);
    Equal(label, "session state", actual.report.session_state,
          expected.session_state);
    Equal(label, "created count", actual.report.handles_created,
          expected.counters.handles_created);
    Equal(label, "disposed count", actual.report.handles_disposed,
          expected.counters.handles_disposed);
    Equal(label, "callbacks attempted", actual.report.callbacks_attempted,
          expected.counters.callbacks_attempted);
    Equal(label, "callbacks completed", actual.report.callbacks_completed,
          expected.counters.callbacks_completed);
    if (actual.status.report_id == 0 ||
        actual.status.report_id != actual.report.report_id ||
        actual.status.report_id <= last_report_id_) {
      std::fprintf(stderr,
                   "%s: report id is zero, uncorrelated, or non-monotonic "
                   "(status=%llu report=%llu previous=%llu)\n",
                   label,
                   static_cast<unsigned long long>(actual.status.report_id),
                   static_cast<unsigned long long>(actual.report.report_id),
                   static_cast<unsigned long long>(last_report_id_));
      ++failures_;
    }
    if (actual.status.report_id > last_report_id_) {
      last_report_id_ = actual.status.report_id;
    }
  }

  void Require(const char *label, bool condition) {
    if (!condition) {
      std::fprintf(stderr, "%s\n", label);
      ++failures_;
    }
  }

  int failures() const { return failures_; }

 private:
  template <typename Actual, typename Expected>
  void Equal(const char *label, const char *field, Actual actual,
             Expected expected) {
    const long long actual_value = static_cast<long long>(actual);
    const long long expected_value = static_cast<long long>(expected);
    if (actual_value != expected_value) {
      std::fprintf(stderr, "%s: %s mismatch (actual=%lld expected=%lld)\n",
                   label, field, actual_value, expected_value);
      ++failures_;
    }
  }

  int failures_ = 0;
  uint64_t last_report_id_ = 0;
};

}  // namespace

int wmain(int argc, wchar_t **argv) {
  if (argc != 2) {
    std::fprintf(stderr,
                 "usage: rust_host_core_ffi_dual_run_selftest <dll-path>\n");
    return 2;
  }

  Verifier verifier;
  AdapterV1 adapter;
  const AdapterLoadStatus load_status = AdapterV1::Load(argv[1], &adapter);
  if (load_status != AdapterLoadStatus::kOk) {
    std::fprintf(stderr,
                 "host-core adapter load failed: status=%u native=%lu\n",
                 static_cast<unsigned>(load_status), adapter.native_error());
    return 2;
  }
  verifier.Require("adapter must own a complete API", adapter.loaded());
  const AexHostCoreAbiDescriptorV1 descriptor = adapter.descriptor();
  verifier.Require(
      "adapter must copy the exact compatible ABI descriptor",
      AdapterV1::IsCompatibleDescriptor(descriptor) &&
          descriptor.magic == AEXCOMPAT_HOST_CORE_ABI_DESCRIPTOR_MAGIC &&
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
              AEXCOMPAT_HOST_CORE_CAPABILITY_SESSION_LIFECYCLE_V1);
  const auto require_descriptor_rejection =
      [&verifier](const char *label,
                  AexHostCoreAbiDescriptorV1 candidate) {
        verifier.Require(label,
                         !AdapterV1::IsCompatibleDescriptor(candidate));
      };
  AexHostCoreAbiDescriptorV1 incompatible = descriptor;
  ++incompatible.magic;
  require_descriptor_rejection("wrong descriptor magic must fail closed",
                               incompatible);
  incompatible = descriptor;
  ++incompatible.abi_version;
  require_descriptor_rejection("wrong descriptor ABI version must fail closed",
                               incompatible);
  incompatible = descriptor;
  ++incompatible.struct_size;
  require_descriptor_rejection("wrong descriptor size must fail closed",
                               incompatible);
  incompatible = descriptor;
  ++incompatible.call_context_size;
  require_descriptor_rejection("wrong context size must fail closed",
                               incompatible);
  incompatible = descriptor;
  ++incompatible.call_context_alignment;
  require_descriptor_rejection("wrong context alignment must fail closed",
                               incompatible);
  incompatible = descriptor;
  ++incompatible.call_status_size;
  require_descriptor_rejection("wrong status size must fail closed",
                               incompatible);
  incompatible = descriptor;
  ++incompatible.call_status_alignment;
  require_descriptor_rejection("wrong status alignment must fail closed",
                               incompatible);
  incompatible = descriptor;
  ++incompatible.opaque_handle_size;
  require_descriptor_rejection("wrong handle size must fail closed",
                               incompatible);
  incompatible = descriptor;
  ++incompatible.opaque_handle_alignment;
  require_descriptor_rejection("wrong handle alignment must fail closed",
                               incompatible);
  incompatible = descriptor;
  ++incompatible.report_snapshot_size;
  require_descriptor_rejection("wrong report size must fail closed",
                               incompatible);
  incompatible = descriptor;
  ++incompatible.report_snapshot_alignment;
  require_descriptor_rejection("wrong report alignment must fail closed",
                               incompatible);
  incompatible = descriptor;
  incompatible.capabilities = 0;
  require_descriptor_rejection("missing capability must fail closed",
                               incompatible);
  incompatible = descriptor;
  incompatible.capabilities |= UINT64_C(2);
  require_descriptor_rejection("unknown capability must fail closed",
                               incompatible);
  const wchar_t *loaded_path = adapter.absolute_path();
  verifier.Require("adapter must retain an absolute DLL path",
                   loaded_path[0] != L'\0' &&
                       (loaded_path[1] == L':' ||
                        (loaded_path[0] == L'\\' && loaded_path[1] == L'\\')));

  AdapterV1 invalid_adapter;
  verifier.Require("null DLL path must fail closed",
                   AdapterV1::Load(nullptr, &invalid_adapter) ==
                           AdapterLoadStatus::kInvalidArgument &&
                       !invalid_adapter.loaded());

  wchar_t system_directory[MAX_PATH]{};
  wchar_t kernel32_path[MAX_PATH]{};
  const UINT system_directory_length =
      GetSystemDirectoryW(system_directory, MAX_PATH);
  verifier.Require(
      "GetSystemDirectoryW failed",
      system_directory_length != 0 && system_directory_length < MAX_PATH);
  if (system_directory_length != 0 && system_directory_length < MAX_PATH) {
    const int written =
        swprintf_s(kernel32_path, L"%ls\\kernel32.dll", system_directory);
    AdapterV1 incomplete_adapter;
    verifier.Require(
        "DLL without the host-core ABI descriptor must fail closed",
        written > 0 &&
            AdapterV1::Load(kernel32_path, &incomplete_adapter) ==
                AdapterLoadStatus::kMissingAbiDescriptor &&
            incomplete_adapter.native_error() == ERROR_PROC_NOT_FOUND &&
            !incomplete_adapter.loaded());
  }
  ApiV1 incomplete_api = adapter.api();
  incomplete_api.session_dispose = nullptr;
  verifier.Require(
      "descriptor-compatible API missing one function must fail closed",
      AdapterV1::ValidateApi(incomplete_api) ==
          AdapterLoadStatus::kMissingExport);

  NativeSessionOracle oracle;
  const AexHostCallContext context{
      AEXCOMPAT_HOST_CORE_ABI_VERSION,
      sizeof(AexHostCallContext),
      41001,
      91001,
  };

  const Invocation unloaded = invalid_adapter.Create(context);
  verifier.Require(
      "unloaded adapter invocation must fail closed",
      unloaded.return_code == AEX_HOST_INVALID_STATE &&
          unloaded.exception_code == 0 &&
          unloaded.created_handle.value == 0 &&
          unloaded.status.code == AEX_HOST_INVALID_STATE &&
          unloaded.status.report_id == 0 &&
          unloaded.report.phase == AEX_HOST_REPORT_PHASE_BOUNDARY &&
          unloaded.report.outcome == AEX_HOST_REPORT_OUTCOME_REJECTED &&
          unloaded.report.error_code == AEX_HOST_INVALID_STATE &&
          unloaded.report.report_id == 0);

  AexHostOpaqueHandle untouched{0x55aa55aa55aa55aaULL};
  AexHostReportSnapshot ignored_report{};
  const SehCallResult null_status =
      AdapterV1::InvokeCreateRaw(adapter.api().session_create, &context,
                                 &untouched, nullptr, &ignored_report);
  verifier.Require("null status must fail before mutation",
                   null_status.return_code == AEX_HOST_INVALID_ARGUMENT &&
                       null_status.exception_code == 0 &&
                       untouched.value == 0x55aa55aa55aa55aaULL);
  AexHostCallStatus ignored_status{};
  const SehCallResult null_report =
      AdapterV1::InvokeCreateRaw(adapter.api().session_create, &context,
                                 &untouched, &ignored_status, nullptr);
  verifier.Require("null report must fail before mutation",
                   null_report.return_code == AEX_HOST_INVALID_ARGUMENT &&
                       null_report.exception_code == 0 &&
                       untouched.value == 0x55aa55aa55aa55aaULL);

  Invocation actual = InvokeRawCreate(adapter, &context, nullptr);
  verifier.Verify("create/null-output", actual, oracle.Create(&context, false));

  AexHostCallContext bad_version = context;
  bad_version.abi_version += 1;
  actual = adapter.Create(bad_version);
  verifier.Verify("create/bad-version", actual,
                  oracle.Create(&bad_version, true));
  verifier.Require("bad version must zero the output token",
                   actual.created_handle.value == 0);

  AexHostCallContext bad_size = context;
  bad_size.struct_size = 0;
  actual = adapter.Create(bad_size);
  verifier.Verify("create/bad-size", actual, oracle.Create(&bad_size, true));
  verifier.Require("bad size must zero the output token",
                   actual.created_handle.value == 0);

  AexHostOpaqueHandle invalid_create_handle{~uint64_t{0}};
  actual = InvokeRawCreate(adapter, nullptr, &invalid_create_handle);
  verifier.Verify("create/null-context", actual, oracle.Create(nullptr, true));
  verifier.Require("null context must zero the output token",
                   invalid_create_handle.value == 0 &&
                       actual.created_handle.value == 0);

  actual = adapter.Create(context);
  const AexHostOpaqueHandle session = actual.created_handle;
  verifier.Verify("create/valid", actual, oracle.Create(&context, true));
  verifier.Require("create must return an opaque non-pointer token",
                   session.value != 0);

  actual = adapter.Open(context, AexHostOpaqueHandle{0});
  verifier.Verify(
      "open/invalid-handle", actual,
      oracle.Call(&context, OracleHandle::Invalid, SessionOperation::Open));

  AexHostCallContext wrong_token = context;
  ++wrong_token.caller_thread_token;
  actual = adapter.Open(wrong_token, session);
  verifier.Verify(
      "open/wrong-token", actual,
      oracle.Call(&wrong_token, OracleHandle::Session, SessionOperation::Open));

  Invocation foreign_actual;
  ExpectedCall foreign_expected;
  std::thread foreign_thread([&]() {
    foreign_actual = adapter.Open(context, session);
    foreign_expected =
        oracle.Call(&context, OracleHandle::Session, SessionOperation::Open);
  });
  foreign_thread.join();
  verifier.Verify("open/foreign-thread", foreign_actual, foreign_expected);

  actual = adapter.Open(context, session);
  verifier.Verify(
      "open/valid", actual,
      oracle.Call(&context, OracleHandle::Session, SessionOperation::Open));

  actual = adapter.EndCallback(context, session);
  verifier.Verify("end/invalid-state", actual,
                  oracle.Call(&context, OracleHandle::Session,
                              SessionOperation::EndCallback));

  actual = adapter.BeginCallback(context, session);
  verifier.Verify("begin/valid", actual,
                  oracle.Call(&context, OracleHandle::Session,
                              SessionOperation::BeginCallback));

  actual = adapter.Close(context, session);
  verifier.Verify(
      "close/in-callback", actual,
      oracle.Call(&context, OracleHandle::Session, SessionOperation::Close));

  actual = adapter.EndCallback(context, session);
  verifier.Verify("end/valid", actual,
                  oracle.Call(&context, OracleHandle::Session,
                              SessionOperation::EndCallback));

  actual = adapter.Close(context, session);
  verifier.Verify(
      "close/valid", actual,
      oracle.Call(&context, OracleHandle::Session, SessionOperation::Close));

  AexHostCallContext wrong_owner = context;
  ++wrong_owner.session_id;
  actual = adapter.Dispose(wrong_owner, session);
  verifier.Verify("dispose/wrong-owner", actual,
                  oracle.Dispose(&wrong_owner, OracleHandle::Session));

  actual = adapter.Dispose(context, session);
  verifier.Verify("dispose/valid", actual,
                  oracle.Dispose(&context, OracleHandle::Session));

  actual = adapter.Open(context, session);
  verifier.Verify(
      "open/stale", actual,
      oracle.Call(&context, OracleHandle::Session, SessionOperation::Open));

#if defined(_MSC_VER)
  AexHostOpaqueHandle seh_handle{0x55aa55aa55aa55aaULL};
  AexHostCallStatus seh_status{};
  AexHostReportSnapshot seh_report{};
  const SehCallResult seh_result = AdapterV1::InvokeCreateRaw(
      SyntheticSehCreate, &context, &seh_handle, &seh_status, &seh_report);
  verifier.Require(
      "synthetic SEH must be normalized into stable boundary values",
      seh_result.return_code == AEX_HOST_SEH_FAULT &&
          seh_result.exception_code == kSyntheticSehCode &&
          seh_handle.value == 0 &&
          seh_status.abi_version == AEXCOMPAT_HOST_CORE_ABI_VERSION &&
          seh_status.struct_size == sizeof(AexHostCallStatus) &&
          seh_status.code == AEX_HOST_SEH_FAULT && seh_status.report_id == 0 &&
          seh_report.abi_version == AEXCOMPAT_HOST_CORE_ABI_VERSION &&
          seh_report.struct_size == sizeof(AexHostReportSnapshot) &&
          seh_report.schema_version == 1 &&
          seh_report.phase == AEX_HOST_REPORT_PHASE_BOUNDARY &&
          seh_report.outcome == AEX_HOST_REPORT_OUTCOME_FAULTED &&
          seh_report.error_code == AEX_HOST_SEH_FAULT &&
          seh_report.handle_kind == AEX_HOST_HANDLE_KIND_NONE &&
          seh_report.session_state == AEX_HOST_SESSION_STATE_NONE &&
          seh_report.report_id == 0);
#endif

  if (verifier.failures() != 0) {
    return 1;
  }
  std::puts("rust_host_core_ffi_dual_run_selftest: ok");
  return 0;
}
