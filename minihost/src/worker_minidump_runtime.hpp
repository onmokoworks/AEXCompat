#pragma once

#include <windows.h>

#include <filesystem>

namespace aexcompat::worker_runtime::minidump {

// Configures the broker-approved local dump directory. Empty and non-directory
// paths are rejected; crash dumping remains disabled until this succeeds.
bool configure_directory(const std::filesystem::path& directory);

std::filesystem::path current_process_dump_path();
bool attempted();

void write_crash_minidump(EXCEPTION_POINTERS* information);
LONG WINAPI top_level_crash_filter(EXCEPTION_POINTERS* information);

}  // namespace aexcompat::worker_runtime::minidump
