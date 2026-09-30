#include "worker_system_sound_suppression.hpp"

#include <Windows.h>

#include <atomic>
#include <cstring>

namespace aexcompat::worker_runtime::system_sound_suppression {
namespace {

std::atomic<std::uint64_t> g_intercepted_calls{};
std::atomic<bool> g_installed{};

BOOL WINAPI silent_message_beep(UINT) {
  g_intercepted_calls.fetch_add(1, std::memory_order_relaxed);
  return TRUE;
}

bool fail(std::string& diagnostic, const char* reason) {
  diagnostic = reason;
  return false;
}

}  // namespace

bool install(std::string& diagnostic) {
  diagnostic.clear();
  if (g_installed.load(std::memory_order_acquire)) return true;

  static_assert(sizeof(void*) == 8,
                "the process-local USER32 redirect is defined for x64 workers");
  const auto user32 = GetModuleHandleW(L"user32.dll");
  if (!user32) return fail(diagnostic, "user32_not_loaded");
  const auto message_beep = reinterpret_cast<const std::uint8_t*>(
      GetProcAddress(user32, "MessageBeep"));
  if (!message_beep) return fail(diagnostic, "message_beep_export_missing");

  // On supported x64 Windows, USER32's public MessageBeep export and its
  // internal MessageBox paths share this RIP-relative import slot for the
  // underlying Win32U system-alert call. Redirecting the slot is process-local:
  // it changes neither the user's sound scheme nor another process's audio.
  if (message_beep[0] != 0x48 || message_beep[1] != 0xff ||
      message_beep[2] != 0x25)
    return fail(diagnostic, "message_beep_thunk_shape_changed");
  std::int32_t displacement{};
  std::memcpy(&displacement, message_beep + 3, sizeof(displacement));
  const auto slot_address = reinterpret_cast<std::intptr_t>(message_beep) + 7 +
      displacement;
  auto slot = reinterpret_cast<void**>(slot_address);
  const auto win32u = GetModuleHandleW(L"win32u.dll");
  const auto nt_user_message_beep = win32u
      ? reinterpret_cast<void*>(GetProcAddress(win32u, "NtUserMessageBeep"))
      : nullptr;
  if (!nt_user_message_beep || *slot != nt_user_message_beep)
    return fail(diagnostic, "message_beep_target_changed");

  DWORD old_protection{};
  if (!VirtualProtect(slot, sizeof(*slot), PAGE_READWRITE, &old_protection))
    return fail(diagnostic, "message_beep_slot_unprotect_failed");
  InterlockedExchangePointer(
      reinterpret_cast<void* volatile*>(slot),
      reinterpret_cast<void*>(&silent_message_beep));
  DWORD ignored{};
  if (!VirtualProtect(slot, sizeof(*slot), old_protection, &ignored))
    return fail(diagnostic, "message_beep_slot_reprotect_failed");
  g_installed.store(true, std::memory_order_release);
  return true;
}

bool selftest() {
  if (!g_installed.load(std::memory_order_acquire)) return false;
  const auto before = intercepted_calls();
  const auto result = MessageBeep(MB_ICONWARNING);
  return result != FALSE && intercepted_calls() == before + 1;
}

std::uint64_t intercepted_calls() {
  return g_intercepted_calls.load(std::memory_order_relaxed);
}

}  // namespace aexcompat::worker_runtime::system_sound_suppression
