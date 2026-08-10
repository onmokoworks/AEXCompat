#include "worker_session.hpp"

#include <windows.h>

#include <cassert>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>

namespace {
std::string read_file(const std::filesystem::path& path) {
  std::ifstream input(path, std::ios::binary);
  return {std::istreambuf_iterator<char>(input),
          std::istreambuf_iterator<char>()};
}

int child(const wchar_t* module_path, const wchar_t* detach_marker,
          bool prepared) {
  SetEnvironmentVariableW(L"AEXCOMPAT_DETACH_MARKER", detach_marker);
  HMODULE module = LoadLibraryW(module_path);
  assert(module);
  aexcompat::worker_runtime::RuntimeContext context;
  context.plugin_path = module_path;
  context.module = module;
  aexcompat::worker_runtime::WorkerSession session(context, nullptr, nullptr);
  if (prepared) {
    assert(session.prepare_protocol_report());
    std::cout << "{\"status\":\"complete-final-report\",\"padding\":\""
              << std::string(1024 * 1024, 'x') << "\"}\n";
  }
  session.terminate_after_protocol_report(37);
}

DWORD run_child(const std::filesystem::path& executable,
                const std::filesystem::path& module,
                const std::filesystem::path& detach_marker,
                const std::filesystem::path& report_marker, bool prepared) {
  std::wstring command = L"\"" + executable.wstring() + L"\" --child \"" +
      module.wstring() + L"\" \"" + detach_marker.wstring() + L"\" " +
      (prepared ? L"prepared" : L"unprepared");
  SECURITY_ATTRIBUTES security{sizeof(security), nullptr, TRUE};
  HANDLE report = CreateFileW(report_marker.c_str(), GENERIC_WRITE,
                              FILE_SHARE_READ, &security, CREATE_ALWAYS,
                              FILE_ATTRIBUTE_NORMAL, nullptr);
  assert(report != INVALID_HANDLE_VALUE);
  STARTUPINFOW startup{sizeof(startup)};
  startup.dwFlags = STARTF_USESTDHANDLES;
  startup.hStdOutput = report;
  startup.hStdError = report;
  startup.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
  PROCESS_INFORMATION process{};
  assert(CreateProcessW(nullptr, command.data(), nullptr, nullptr, TRUE, 0,
                        nullptr, nullptr, &startup, &process));
  CloseHandle(report);
  CloseHandle(process.hThread);
  assert(WaitForSingleObject(process.hProcess, 10000) == WAIT_OBJECT_0);
  DWORD exit_code{};
  assert(GetExitCodeProcess(process.hProcess, &exit_code));
  CloseHandle(process.hProcess);
  return exit_code;
}
}  // namespace

int wmain(int argc, wchar_t** argv) {
  if (argc == 5 && std::wstring(argv[1]) == L"--child")
    return child(argv[2], argv[3], std::wstring(argv[4]) == L"prepared");
  assert(argc == 2);
  const std::filesystem::path executable = argv[0];
  const std::filesystem::path module = argv[1];
  const auto root = std::filesystem::temp_directory_path() /
      (L"aexcompat-contained-exit-" + std::to_wstring(GetCurrentProcessId()));
  std::filesystem::create_directories(root);

  const auto detach = root / L"detach.txt";
  const auto report = root / L"report.txt";
  assert(run_child(executable, module, detach, report, true) == 37);
  const std::string completed = read_file(report);
  const std::string prefix =
      "{\"status\":\"complete-final-report\",\"padding\":\"";
  assert(completed.rfind(prefix, 0) == 0);
  assert(completed.size() >= 4 &&
         completed.compare(completed.size() - 4, 4, "\"}\r\n") == 0);
  assert(completed.size() > 1024 * 1024);
  assert(!std::filesystem::exists(detach));

  std::filesystem::remove(report);
  const DWORD rejected = run_child(executable, module, detach, report, false);
  assert(rejected != 37);
  assert(read_file(report).empty());
  assert(!std::filesystem::exists(detach));
  std::filesystem::remove_all(root);
  return 0;
}
