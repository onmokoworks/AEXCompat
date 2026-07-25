#include "worker_import_overrides.hpp"

#include <windows.h>

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>

using namespace aexcompat::worker_runtime::imports;

int main() {
  constexpr std::size_t kImageSize = 0x4000;
  auto* image = static_cast<std::byte*>(
      VirtualAlloc(nullptr, kImageSize, MEM_RESERVE | MEM_COMMIT,
                   PAGE_READWRITE));
  if (!image) return 1;

  auto* dos = reinterpret_cast<IMAGE_DOS_HEADER*>(image);
  dos->e_magic = IMAGE_DOS_SIGNATURE;
  dos->e_lfanew = 0x100;
  auto* nt = reinterpret_cast<IMAGE_NT_HEADERS64*>(image + dos->e_lfanew);
  nt->Signature = IMAGE_NT_SIGNATURE;
  nt->OptionalHeader.Magic = IMAGE_NT_OPTIONAL_HDR64_MAGIC;
  nt->OptionalHeader.SizeOfImage = static_cast<DWORD>(kImageSize);
  nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT] =
      {0x400, 2 * sizeof(IMAGE_IMPORT_DESCRIPTOR)};

  auto* descriptor =
      reinterpret_cast<IMAGE_IMPORT_DESCRIPTOR*>(image + 0x400);
  descriptor->Name = 0x500;
  descriptor->OriginalFirstThunk = 0x600;
  descriptor->FirstThunk = 0x700;
  std::memcpy(image + 0x500, "VCOMP140.DLL", sizeof("VCOMP140.DLL"));

  auto* names = reinterpret_cast<IMAGE_THUNK_DATA64*>(image + 0x600);
  names[0].u1.AddressOfData = 0x800;
  auto* addresses = reinterpret_cast<IMAGE_THUNK_DATA64*>(image + 0x700);
  addresses[0].u1.Function = 0x12345678;
  auto* import = reinterpret_cast<IMAGE_IMPORT_BY_NAME*>(image + 0x800);
  import->Hint = 0;
  std::memcpy(import->Name, "omp_get_max_threads",
              sizeof("omp_get_max_threads"));

  std::string diagnostic;
  const bool installed = install_deterministic_import_overrides(
      reinterpret_cast<HMODULE>(image), diagnostic);
  const auto function = reinterpret_cast<int(__cdecl*)()>(
      addresses[0].u1.Function);
  const bool passed = installed && function &&
      function() == kDeterministicOpenMpThreads &&
      diagnostic == "vcomp140!omp_get_max_threads=1";

  descriptor->OriginalFirstThunk = 0;
  std::string malformed_diagnostic;
  const bool malformed_rejected = !install_deterministic_import_overrides(
      reinterpret_cast<HMODULE>(image), malformed_diagnostic);

  VirtualFree(image, 0, MEM_RELEASE);
  return passed && malformed_rejected &&
          malformed_diagnostic == "vcomp140 import has no named thunk"
      ? 0
      : 2;
}
