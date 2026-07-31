#include "aexcompat_host_core_abi.h"

#include <windows.h>

#include <cstdint>
#include <cstdio>
#include <thread>

namespace {

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
      return Failure(AEX_HOST_INVALID_ARGUMENT,
                     AEX_HOST_SESSION_STATE_NONE);
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

  ExpectedCall Call(const AexHostCallContext *context,
                    OracleHandle handle,
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

  ExpectedCall Dispose(const AexHostCallContext *context,
                       OracleHandle handle) {
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
      return Failure(AEX_HOST_INVALID_ARGUMENT,
                     AEX_HOST_SESSION_STATE_NONE);
    }
    if (handle == OracleHandle::Invalid || !known_) {
      return Failure(AEX_HOST_INVALID_HANDLE,
                     AEX_HOST_SESSION_STATE_NONE);
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

struct SehCallResult {
  int32_t return_code;
  uint32_t exception_code;
};

SehCallResult CallCreateInsideSeh(
    AexHostCoreSessionCreateV1Fn function,
    const AexHostCallContext *context,
    AexHostOpaqueHandle *session,
    AexHostCallStatus *status,
    AexHostReportSnapshot *report) noexcept {
  SehCallResult result{AEX_HOST_SEH_FAULT, 0};
#if defined(_MSC_VER)
  __try {
    result.return_code = function(context, session, status, report);
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    result.return_code = AEX_HOST_SEH_FAULT;
    result.exception_code = static_cast<uint32_t>(GetExceptionCode());
  }
#else
  result.return_code = function(context, session, status, report);
#endif
  return result;
}

SehCallResult CallSessionInsideSeh(
    AexHostCoreSessionCallV1Fn function,
    const AexHostCallContext *context,
    AexHostOpaqueHandle session,
    AexHostCallStatus *status,
    AexHostReportSnapshot *report) noexcept {
  SehCallResult result{AEX_HOST_SEH_FAULT, 0};
#if defined(_MSC_VER)
  __try {
    result.return_code = function(context, session, status, report);
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    result.return_code = AEX_HOST_SEH_FAULT;
    result.exception_code = static_cast<uint32_t>(GetExceptionCode());
  }
#else
  result.return_code = function(context, session, status, report);
#endif
  return result;
}

struct Invocation {
  int32_t return_code = AEX_HOST_INVALID_STATE;
  uint32_t exception_code = 0;
  AexHostCallStatus status{};
  AexHostReportSnapshot report{};
};

Invocation InvokeCreate(AexHostCoreSessionCreateV1Fn function,
                        const AexHostCallContext *context,
                        AexHostOpaqueHandle *session) {
  Invocation invocation;
  const SehCallResult result =
      CallCreateInsideSeh(function, context, session, &invocation.status,
                          &invocation.report);
  invocation.return_code = result.return_code;
  invocation.exception_code = result.exception_code;
  return invocation;
}

Invocation InvokeSession(AexHostCoreSessionCallV1Fn function,
                         const AexHostCallContext *context,
                         AexHostOpaqueHandle session) {
  Invocation invocation;
  const SehCallResult result =
      CallSessionInsideSeh(function, context, session, &invocation.status,
                           &invocation.report);
  invocation.return_code = result.return_code;
  invocation.exception_code = result.exception_code;
  return invocation;
}

class Verifier {
 public:
  void Verify(const char *label,
              const Invocation &actual,
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
          expected.code == AEX_HOST_OK
              ? AEX_HOST_REPORT_OUTCOME_PASSED
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
    Equal(label, "callbacks attempted",
          actual.report.callbacks_attempted,
          expected.counters.callbacks_attempted);
    Equal(label, "callbacks completed",
          actual.report.callbacks_completed,
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
  void Equal(const char *label,
             const char *field,
             Actual actual,
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

template <typename Function>
Function LoadFunction(HMODULE module, const char *name, Verifier *verifier) {
  const FARPROC address = GetProcAddress(module, name);
  verifier->Require(name, address != nullptr);
  return reinterpret_cast<Function>(address);
}

}  // namespace

int wmain(int argc, wchar_t **argv) {
  if (argc != 2) {
    std::fprintf(stderr,
                 "usage: rust_host_core_ffi_dual_run_selftest <dll-path>\n");
    return 2;
  }

  Verifier verifier;
  HMODULE module = LoadLibraryW(argv[1]);
  if (module == nullptr) {
    std::fprintf(stderr, "LoadLibraryW failed: %lu\n", GetLastError());
    return 2;
  }

  const auto create = LoadFunction<AexHostCoreSessionCreateV1Fn>(
      module, "aex_host_core_session_create_v1", &verifier);
  const auto open = LoadFunction<AexHostCoreSessionCallV1Fn>(
      module, "aex_host_core_session_open_v1", &verifier);
  const auto begin_callback = LoadFunction<AexHostCoreSessionCallV1Fn>(
      module, "aex_host_core_session_begin_callback_v1", &verifier);
  const auto end_callback = LoadFunction<AexHostCoreSessionCallV1Fn>(
      module, "aex_host_core_session_end_callback_v1", &verifier);
  const auto close = LoadFunction<AexHostCoreSessionCallV1Fn>(
      module, "aex_host_core_session_close_v1", &verifier);
  const auto dispose = LoadFunction<AexHostCoreSessionCallV1Fn>(
      module, "aex_host_core_session_dispose_v1", &verifier);
  if (verifier.failures() != 0) {
    FreeLibrary(module);
    return 1;
  }

  NativeSessionOracle oracle;
  const AexHostCallContext context{
      AEXCOMPAT_HOST_CORE_ABI_VERSION,
      sizeof(AexHostCallContext),
      41001,
      91001,
  };

  AexHostOpaqueHandle untouched{0x55aa55aa55aa55aaULL};
  AexHostReportSnapshot ignored_report{};
  const SehCallResult null_status =
      CallCreateInsideSeh(create, &context, &untouched, nullptr,
                          &ignored_report);
  verifier.Require("null status must fail before mutation",
                   null_status.return_code == AEX_HOST_INVALID_ARGUMENT &&
                       null_status.exception_code == 0 &&
                       untouched.value == 0x55aa55aa55aa55aaULL);
  AexHostCallStatus ignored_status{};
  const SehCallResult null_report =
      CallCreateInsideSeh(create, &context, &untouched, &ignored_status,
                          nullptr);
  verifier.Require("null report must fail before mutation",
                   null_report.return_code == AEX_HOST_INVALID_ARGUMENT &&
                       null_report.exception_code == 0 &&
                       untouched.value == 0x55aa55aa55aa55aaULL);

  Invocation actual =
      InvokeCreate(create, &context, static_cast<AexHostOpaqueHandle *>(nullptr));
  verifier.Verify("create/null-output", actual, oracle.Create(&context, false));

  AexHostCallContext bad_version = context;
  bad_version.abi_version += 1;
  AexHostOpaqueHandle invalid_create_handle{~uint64_t{0}};
  actual = InvokeCreate(create, &bad_version, &invalid_create_handle);
  verifier.Verify("create/bad-version", actual,
                  oracle.Create(&bad_version, true));
  verifier.Require("bad version must zero the output token",
                   invalid_create_handle.value == 0);

  AexHostCallContext bad_size = context;
  bad_size.struct_size = 0;
  invalid_create_handle.value = ~uint64_t{0};
  actual = InvokeCreate(create, &bad_size, &invalid_create_handle);
  verifier.Verify("create/bad-size", actual, oracle.Create(&bad_size, true));
  verifier.Require("bad size must zero the output token",
                   invalid_create_handle.value == 0);

  invalid_create_handle.value = ~uint64_t{0};
  actual = InvokeCreate(create, nullptr, &invalid_create_handle);
  verifier.Verify("create/null-context", actual,
                  oracle.Create(nullptr, true));
  verifier.Require("null context must zero the output token",
                   invalid_create_handle.value == 0);

  AexHostOpaqueHandle session{};
  actual = InvokeCreate(create, &context, &session);
  verifier.Verify("create/valid", actual, oracle.Create(&context, true));
  verifier.Require("create must return an opaque non-pointer token",
                   session.value != 0);

  actual = InvokeSession(open, &context, AexHostOpaqueHandle{0});
  verifier.Verify(
      "open/invalid-handle", actual,
      oracle.Call(&context, OracleHandle::Invalid, SessionOperation::Open));

  AexHostCallContext wrong_token = context;
  ++wrong_token.caller_thread_token;
  actual = InvokeSession(open, &wrong_token, session);
  verifier.Verify(
      "open/wrong-token", actual,
      oracle.Call(&wrong_token, OracleHandle::Session, SessionOperation::Open));

  Invocation foreign_actual;
  ExpectedCall foreign_expected;
  std::thread foreign_thread([&]() {
    foreign_actual = InvokeSession(open, &context, session);
    foreign_expected =
        oracle.Call(&context, OracleHandle::Session, SessionOperation::Open);
  });
  foreign_thread.join();
  verifier.Verify("open/foreign-thread", foreign_actual, foreign_expected);

  actual = InvokeSession(open, &context, session);
  verifier.Verify(
      "open/valid", actual,
      oracle.Call(&context, OracleHandle::Session, SessionOperation::Open));

  actual = InvokeSession(end_callback, &context, session);
  verifier.Verify(
      "end/invalid-state", actual,
      oracle.Call(&context, OracleHandle::Session,
                  SessionOperation::EndCallback));

  actual = InvokeSession(begin_callback, &context, session);
  verifier.Verify(
      "begin/valid", actual,
      oracle.Call(&context, OracleHandle::Session,
                  SessionOperation::BeginCallback));

  actual = InvokeSession(close, &context, session);
  verifier.Verify(
      "close/in-callback", actual,
      oracle.Call(&context, OracleHandle::Session, SessionOperation::Close));

  actual = InvokeSession(end_callback, &context, session);
  verifier.Verify(
      "end/valid", actual,
      oracle.Call(&context, OracleHandle::Session,
                  SessionOperation::EndCallback));

  actual = InvokeSession(close, &context, session);
  verifier.Verify(
      "close/valid", actual,
      oracle.Call(&context, OracleHandle::Session, SessionOperation::Close));

  AexHostCallContext wrong_owner = context;
  ++wrong_owner.session_id;
  actual = InvokeSession(dispose, &wrong_owner, session);
  verifier.Verify("dispose/wrong-owner", actual,
                  oracle.Dispose(&wrong_owner, OracleHandle::Session));

  actual = InvokeSession(dispose, &context, session);
  verifier.Verify("dispose/valid", actual,
                  oracle.Dispose(&context, OracleHandle::Session));

  actual = InvokeSession(open, &context, session);
  verifier.Verify(
      "open/stale", actual,
      oracle.Call(&context, OracleHandle::Session, SessionOperation::Open));

  const BOOL unloaded = FreeLibrary(module);
  verifier.Require("FreeLibrary failed", unloaded != FALSE);
  if (verifier.failures() != 0) {
    return 1;
  }
  std::puts("rust_host_core_ffi_dual_run_selftest: ok");
  return 0;
}
