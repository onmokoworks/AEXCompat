#include "worker_minidump_runtime.hpp"

// MiniDumpWriteDump types only; dbghelp.dll is loaded from System32 on crash.
#include <DbgHelp.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <filesystem>

namespace aexcompat::worker_runtime::minidump {
namespace {

// Opt-in crash minidumps (issue #18) use a broker-created inherited pipe. The
// worker never receives a directory path or dump-file handle. DbgHelp runs on
// a pre-started dedicated thread because calling it from the faulting SEH
// thread can deadlock in an unstable process.
constexpr uint64_t kMaxMinidumpFileBytes = 64ull * 1024ull * 1024ull;
constexpr DWORD kMinidumpWriterWaitMs = 2'000;
constexpr std::array<unsigned char, 16> kMinidumpCompletionMarker{
    'A', 'E', 'X', 'D', 'U', 'M', 'P', '-',
    'C', 'O', 'M', 'P', 'L', 'E', 'T', 'E'};
HANDLE g_minidump_handle{};
HANDLE g_minidump_ack_handle{};
HANDLE g_minidump_request_event{};
HANDLE g_minidump_complete_event{};
std::atomic<bool> g_minidump_ready{false};
// A plug-in can crash several of its own threads at once, and the top-level
// filter runs on each, so the writer must admit only the first.
std::atomic<bool> g_minidump_attempted{false};
std::atomic<uint64_t> g_minidump_written_bytes{0};
std::atomic<bool> g_minidump_broker_rejected{false};
std::atomic<bool> g_minidump_handle_configured{false};
EXCEPTION_RECORD g_minidump_exception_record{};
CONTEXT g_minidump_context{};
EXCEPTION_POINTERS g_minidump_exception_pointers{};
DWORD g_minidump_thread_id{};
unsigned char* g_minidump_buffer{};
uint64_t g_minidump_page_size{};

struct MinidumpBufferContext {
  unsigned char* buffer{};
  uint64_t capacity{};
  uint64_t extent{};
  uint64_t committed{};
  uint64_t page_size{};
  bool started{false};
  bool finished{false};
  bool failed{false};
};

using MiniDumpWriteDumpFn = BOOL(WINAPI*)(HANDLE, DWORD, HANDLE,
                                          MINIDUMP_TYPE,
                                          PMINIDUMP_EXCEPTION_INFORMATION,
                                          PMINIDUMP_USER_STREAM_INFORMATION,
                                          PMINIDUMP_CALLBACK_INFORMATION);
HMODULE g_dbghelp{};
MiniDumpWriteDumpFn g_minidump_write_dump{};

bool preload_minidump_writer() {
  if (g_minidump_write_dump) return true;
  g_dbghelp = LoadLibraryExW(L"dbghelp.dll", nullptr,
                             LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (!g_dbghelp) return false;
  g_minidump_write_dump = reinterpret_cast<MiniDumpWriteDumpFn>(
      GetProcAddress(g_dbghelp, "MiniDumpWriteDump"));
  return g_minidump_write_dump != nullptr;
}

BOOL CALLBACK minidump_limit_callback(
    PVOID parameter, const PMINIDUMP_CALLBACK_INPUT input,
    PMINIDUMP_CALLBACK_OUTPUT output) {
  if (!parameter || !input || !output) return FALSE;
  auto* sink = static_cast<MinidumpBufferContext*>(parameter);
  if (input->CallbackType == IoStartCallback) {
    sink->started = true;
    // S_FALSE opts out of DbgHelp's normal hFile writes. All payload writes
    // arrive through IoWriteAllCallback with their authoritative offsets.
    output->Status = S_FALSE;
    return TRUE;
  }
  if (input->CallbackType == IoWriteAllCallback) {
    const auto& io = input->Io;
    if (!sink->started || !sink->buffer ||
        io.Offset > sink->capacity ||
        io.BufferBytes > sink->capacity - io.Offset ||
        (io.BufferBytes != 0 && !io.Buffer)) {
      sink->failed = true;
      output->Status = E_FAIL;
      return FALSE;
    }
    if (io.BufferBytes != 0) {
      const uint64_t end = io.Offset + static_cast<uint64_t>(io.BufferBytes);
      const uint64_t commit_end =
          (end + sink->page_size - 1) / sink->page_size * sink->page_size;
      if (commit_end > sink->committed) {
        const SIZE_T commit_bytes = static_cast<SIZE_T>(
            commit_end - sink->committed);
        void* const committed = VirtualAlloc(
            sink->buffer + sink->committed, commit_bytes, MEM_COMMIT,
            PAGE_READWRITE);
        if (committed != sink->buffer + sink->committed) {
          sink->failed = true;
          output->Status = E_OUTOFMEMORY;
          return FALSE;
        }
        sink->committed = commit_end;
      }
      std::memcpy(sink->buffer + static_cast<std::size_t>(io.Offset),
                  io.Buffer, io.BufferBytes);
    }
    sink->extent = std::max(
        sink->extent, io.Offset + static_cast<uint64_t>(io.BufferBytes));
    output->Status = S_OK;
    return TRUE;
  }
  if (input->CallbackType == IoFinishCallback) {
    sink->finished = true;
    output->Status = sink->failed ? E_FAIL : S_OK;
    return sink->failed ? FALSE : TRUE;
  }
  return TRUE;
}

bool write_minidump_transport(const unsigned char* bytes, uint64_t size) {
  if (!g_minidump_handle || !bytes || size == 0 ||
      size > kMaxMinidumpFileBytes) return false;
  uint64_t offset = 0;
  while (offset < size) {
    const DWORD requested = static_cast<DWORD>(
        std::min<uint64_t>(size - offset, 64 * 1024));
    DWORD written = 0;
    if (!WriteFile(g_minidump_handle, bytes + offset, requested, &written,
                   nullptr) || written == 0)
      return false;
    offset += written;
  }
  DWORD marker_bytes = 0;
  return WriteFile(g_minidump_handle, kMinidumpCompletionMarker.data(),
                   static_cast<DWORD>(kMinidumpCompletionMarker.size()),
                   &marker_bytes, nullptr) &&
         marker_bytes == kMinidumpCompletionMarker.size();
}

struct MinidumpBrokerAck {
  bool valid{false};
  uint64_t bytes{};
  bool rejected{false};
};

MinidumpBrokerAck finish_minidump_transport() {
  // Prevent a new faulting thread from claiming the transport before the
  // writer mutates the global handles. A thread that observed the previous
  // ready state still has to win g_minidump_attempted before reading them.
  g_minidump_ready.store(false, std::memory_order_release);
  if (g_minidump_handle) {
    CloseHandle(g_minidump_handle);
    g_minidump_handle = nullptr;
  }
  MinidumpBrokerAck result;
  if (!g_minidump_ack_handle) return result;
  std::array<unsigned char, 9> ack{};
  DWORD read = 0;
  const BOOL ok = ReadFile(g_minidump_ack_handle, ack.data(),
                           static_cast<DWORD>(ack.size()), &read, nullptr);
  CloseHandle(g_minidump_ack_handle);
  g_minidump_ack_handle = nullptr;
  if (ok && read == ack.size()) {
    std::memcpy(&result.bytes, ack.data(), sizeof(result.bytes));
    result.rejected = ack[8] != 0;
    result.valid = true;
  }
  g_minidump_written_bytes.store(result.bytes);
  g_minidump_broker_rejected.store(result.rejected);
  return result;
}

void write_minidump_on_dedicated_thread() {
  if (!g_minidump_write_dump) {
    finish_minidump_transport();
    std::fprintf(stderr, "stage:minidump_failed reason=%s\n",
                 g_dbghelp ? "entry_unavailable" : "dbghelp_unavailable");
    return;
  }
  MINIDUMP_EXCEPTION_INFORMATION exception_info{};
  exception_info.ThreadId = g_minidump_thread_id;
  exception_info.ExceptionPointers = &g_minidump_exception_pointers;
  exception_info.ClientPointers = FALSE;
  MinidumpBufferContext sink{
      g_minidump_buffer, kMaxMinidumpFileBytes, 0, 0,
      g_minidump_page_size};
  MINIDUMP_CALLBACK_INFORMATION callback_info{};
  callback_info.CallbackParam = &sink;
  callback_info.CallbackRoutine = minidump_limit_callback;
  const BOOL written = g_minidump_write_dump(
      GetCurrentProcess(), GetCurrentProcessId(), INVALID_HANDLE_VALUE,
      MiniDumpNormal, &exception_info, nullptr, &callback_info);
  const DWORD error = GetLastError();
  const bool transported = written && sink.started && sink.finished &&
      !sink.failed && write_minidump_transport(sink.buffer, sink.extent);
  const MinidumpBrokerAck broker = finish_minidump_transport();
  if (transported && broker.valid && !broker.rejected && broker.bytes > 0 &&
      broker.bytes <= kMaxMinidumpFileBytes) {
    std::fprintf(stderr, "stage:minidump_written bytes=%lld\n",
                 static_cast<long long>(broker.bytes));
  } else if (broker.rejected) {
    std::fprintf(stderr, "stage:minidump_failed reason=capacity_exceeded\n");
  } else {
    std::fprintf(stderr, "stage:minidump_failed reason=write_failed code=%lu\n",
                 error);
  }
}

DWORD WINAPI minidump_writer_thread(void*) {
  if (WaitForSingleObject(g_minidump_request_event, INFINITE) == WAIT_OBJECT_0)
    write_minidump_on_dedicated_thread();
  SetEvent(g_minidump_complete_event);
  return 0;
}

bool start_minidump_writer(HANDLE handle, HANDLE ack_handle) {
  g_minidump_handle = handle;
  g_minidump_ack_handle = ack_handle;
  g_minidump_request_event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
  g_minidump_complete_event = CreateEventW(nullptr, TRUE, FALSE, nullptr);
  if (!g_minidump_request_event || !g_minidump_complete_event) {
    if (g_minidump_request_event) CloseHandle(g_minidump_request_event);
    if (g_minidump_complete_event) CloseHandle(g_minidump_complete_event);
    g_minidump_request_event = nullptr;
    g_minidump_complete_event = nullptr;
    g_minidump_handle = nullptr;
    g_minidump_ack_handle = nullptr;
    return false;
  }
  HANDLE thread = CreateThread(nullptr, 0, minidump_writer_thread, nullptr, 0,
                               nullptr);
  if (!thread) {
    CloseHandle(g_minidump_request_event);
    CloseHandle(g_minidump_complete_event);
    g_minidump_request_event = nullptr;
    g_minidump_complete_event = nullptr;
    g_minidump_handle = nullptr;
    g_minidump_ack_handle = nullptr;
    return false;
  }
  CloseHandle(thread);
  g_minidump_ready.store(true, std::memory_order_release);
  return true;
}

HANDLE inherited_minidump_handle(const wchar_t* name) {
  wchar_t buffer[64]{};
  const DWORD length = GetEnvironmentVariableW(
      name, buffer, static_cast<DWORD>(std::size(buffer)));
  if (length == 0 || length >= std::size(buffer)) return nullptr;
  wchar_t* end = nullptr;
  const unsigned long long value = _wcstoui64(buffer, &end, 10);
  if (!end || *end != L'\0' || value == 0) return nullptr;
  const HANDLE handle = reinterpret_cast<HANDLE>(static_cast<uintptr_t>(value));
  DWORD flags{};
  if (!GetHandleInformation(handle, &flags) ||
      !(flags & HANDLE_FLAG_INHERIT) || GetFileType(handle) != FILE_TYPE_PIPE)
    return nullptr;
  return handle;
}

void clear_minidump_env() {
  SetEnvironmentVariableW(L"AEXCOMPAT_MINIDUMP_HANDLE", nullptr);
  SetEnvironmentVariableW(L"AEXCOMPAT_MINIDUMP_ACK_HANDLE", nullptr);
}

}  // namespace

bool attempted() { return g_minidump_attempted.load(); }
uint64_t written_bytes() { return g_minidump_written_bytes.load(); }
bool broker_rejected() { return g_minidump_broker_rejected.load(); }
bool handle_configured() { return g_minidump_handle_configured.load(); }

void classify_seh_exception(EXCEPTION_POINTERS* information,
                            SehDiagnosticsSink diagnostics) {
  diagnostics.code = information && information->ExceptionRecord
      ? information->ExceptionRecord->ExceptionCode : 0;
  const void* address = information && information->ExceptionRecord
      ? information->ExceptionRecord->ExceptionAddress : nullptr;
  diagnostics.address = reinterpret_cast<uint64_t>(address);
  diagnostics.module.clear();
  HMODULE module{};
  if (address && GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
          GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
          reinterpret_cast<LPCWSTR>(address), &module)) {
    std::array<wchar_t, MAX_PATH> path{};
    if (GetModuleFileNameW(module, path.data(),
                           static_cast<DWORD>(path.size())) > 0) {
      const std::wstring filename =
          std::filesystem::path(path.data()).filename().wstring();
      for (wchar_t ch : filename)
        diagnostics.module.push_back(ch >= 0x20 && ch <= 0x7e
            ? static_cast<char>(ch) : '?');
    }
  }
}

void request_crash_minidump(EXCEPTION_POINTERS* information) {
  if (!information ||
      !g_minidump_ready.load(std::memory_order_acquire)) return;
  // Claim the one-shot transport before reading any mutable global handle.
  // The writer may close/null those handles only after this claimant signals
  // it; every concurrent fault returns here without touching them.
  if (g_minidump_attempted.exchange(true, std::memory_order_acq_rel)) return;
  if (!g_minidump_handle || !g_minidump_request_event) return;
  if (information->ExceptionRecord)
    g_minidump_exception_record = *information->ExceptionRecord;
  else
    std::memset(&g_minidump_exception_record, 0,
                sizeof(g_minidump_exception_record));
  g_minidump_exception_record.ExceptionRecord = nullptr;
  if (information->ContextRecord)
    g_minidump_context = *information->ContextRecord;
  else
    std::memset(&g_minidump_context, 0, sizeof(g_minidump_context));
  g_minidump_exception_pointers.ExceptionRecord =
      &g_minidump_exception_record;
  g_minidump_exception_pointers.ContextRecord = information->ContextRecord
      ? &g_minidump_context
      : nullptr;
  g_minidump_thread_id = GetCurrentThreadId();
  SetEvent(g_minidump_request_event);
  const DWORD result = WaitForSingleObject(g_minidump_complete_event,
                                           kMinidumpWriterWaitMs);
  if (result == WAIT_TIMEOUT)
    std::fprintf(stderr, "stage:minidump_failed reason=writer_timeout\n");
}

int capture_seh_exception(EXCEPTION_POINTERS* information,
                          SehDiagnosticsSink diagnostics) {
  request_crash_minidump(information);
  classify_seh_exception(information, diagnostics);
  return EXCEPTION_EXECUTE_HANDLER;
}

// Best-effort coverage for crashes that never reach an __except filter
// (e.g. on foreign threads). Continue the search so default handling and the
// nonzero exit code are unchanged.
LONG WINAPI top_level_crash_filter(EXCEPTION_POINTERS* information) {
  request_crash_minidump(information);
  return EXCEPTION_CONTINUE_SEARCH;
}

bool configure_from_inherited_handle() {
  wchar_t probe[2]{};
  if (GetEnvironmentVariableW(L"AEXCOMPAT_MINIDUMP_HANDLE", probe,
                              static_cast<DWORD>(std::size(probe))) == 0)
    return true;
  const HANDLE handle = inherited_minidump_handle(L"AEXCOMPAT_MINIDUMP_HANDLE");
  const HANDLE ack_handle =
      inherited_minidump_handle(L"AEXCOMPAT_MINIDUMP_ACK_HANDLE");
  if (!handle || !ack_handle) {
    if (handle) CloseHandle(handle);
    if (ack_handle) CloseHandle(ack_handle);
    clear_minidump_env();
    std::fprintf(stderr, "stage:minidump_failed reason=handle_invalid\n");
    return false;
  }
  if (SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) == 0 ||
      SetHandleInformation(ack_handle, HANDLE_FLAG_INHERIT, 0) == 0) {
    CloseHandle(handle);
    CloseHandle(ack_handle);
    clear_minidump_env();
    std::fprintf(stderr, "stage:minidump_failed reason=handle_invalid\n");
    return false;
  }
  // Reserve address space for the bounded alternate-I/O sink before any
  // plug-in code runs, but do not consume the worker's 512 MiB commit budget.
  // The crash-only callback commits pages on demand through the maximum
  // validated write offset. DbgHelp and the plug-in still receive no seekable
  // broker-owned file handle.
  SYSTEM_INFO system_info{};
  GetSystemInfo(&system_info);
  g_minidump_page_size = system_info.dwPageSize;
  if (g_minidump_page_size == 0 ||
      kMaxMinidumpFileBytes % g_minidump_page_size != 0) {
    CloseHandle(handle);
    CloseHandle(ack_handle);
    clear_minidump_env();
    std::fprintf(stderr, "stage:minidump_failed reason=writer_unavailable\n");
    return false;
  }
  g_minidump_buffer = static_cast<unsigned char*>(VirtualAlloc(
      nullptr, static_cast<SIZE_T>(kMaxMinidumpFileBytes),
      MEM_RESERVE, PAGE_READWRITE));
  if (!g_minidump_buffer) {
    CloseHandle(handle);
    CloseHandle(ack_handle);
    clear_minidump_env();
    std::fprintf(stderr, "stage:minidump_failed reason=writer_unavailable\n");
    return false;
  }
  preload_minidump_writer();
  if (!start_minidump_writer(handle, ack_handle)) {
    VirtualFree(g_minidump_buffer, 0, MEM_RELEASE);
    g_minidump_buffer = nullptr;
    CloseHandle(handle);
    CloseHandle(ack_handle);
    clear_minidump_env();
    std::fprintf(stderr, "stage:minidump_failed reason=writer_unavailable\n");
    return false;
  }
  clear_minidump_env();
  g_minidump_handle_configured.store(true);
  SetUnhandledExceptionFilter(top_level_crash_filter);
  return true;
}

}  // namespace aexcompat::worker_runtime::minidump
