#include "worker_import_overrides.hpp"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>

namespace aexcompat::worker_runtime::imports {
namespace {

template <typename T>
T* image_pointer(std::byte* base, std::size_t image_size,
                 std::uint32_t rva, std::size_t count = 1) noexcept {
  if (!base || count > (std::numeric_limits<std::size_t>::max)() / sizeof(T))
    return nullptr;
  const std::size_t bytes = count * sizeof(T);
  if (rva > image_size || bytes > image_size - rva) return nullptr;
  return reinterpret_cast<T*>(base + rva);
}

const char* image_string(std::byte* base, std::size_t image_size,
                         std::uint32_t rva) noexcept {
  if (!base || rva >= image_size) return nullptr;
  const char* value = reinterpret_cast<const char*>(base + rva);
  return std::memchr(value, '\0', image_size - rva) ? value : nullptr;
}

bool ascii_equal_folded(const char* left, const char* right) noexcept {
  if (!left || !right) return false;
  while (*left && *right) {
    const auto fold = [](unsigned char value) {
      return value >= 'A' && value <= 'Z'
                 ? static_cast<unsigned char>(value + ('a' - 'A'))
                 : value;
    };
    if (fold(static_cast<unsigned char>(*left++)) !=
        fold(static_cast<unsigned char>(*right++)))
      return false;
  }
  return *left == '\0' && *right == '\0';
}

bool replace_iat_entry(IMAGE_THUNK_DATA64& entry,
                       std::string& diagnostic) noexcept {
  DWORD old_protection{};
  if (!VirtualProtect(&entry.u1.Function, sizeof(entry.u1.Function),
                      PAGE_READWRITE, &old_protection)) {
    diagnostic = "vcomp140 IAT write protection failed";
    return false;
  }
  entry.u1.Function = reinterpret_cast<ULONGLONG>(
      &deterministic_omp_get_max_threads);
  DWORD ignored{};
  if (!VirtualProtect(&entry.u1.Function, sizeof(entry.u1.Function),
                      old_protection, &ignored)) {
    diagnostic = "vcomp140 IAT protection restore failed";
    return false;
  }
  diagnostic = "vcomp140!omp_get_max_threads=1";
  return true;
}

}  // namespace

int __cdecl deterministic_omp_get_max_threads() {
  return kDeterministicOpenMpThreads;
}

bool install_deterministic_import_overrides(
    HMODULE module, std::string& diagnostic) noexcept {
  diagnostic.clear();
  if (!module) {
    diagnostic = "null module";
    return false;
  }
  auto* base = reinterpret_cast<std::byte*>(module);
  const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
  if (dos->e_magic != IMAGE_DOS_SIGNATURE || dos->e_lfanew <= 0) {
    diagnostic = "invalid DOS header";
    return false;
  }
  const auto nt_offset = static_cast<std::size_t>(dos->e_lfanew);
  if (nt_offset > 1024 * 1024) {
    diagnostic = "invalid NT header offset";
    return false;
  }
  const auto* nt = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + nt_offset);
  if (nt->Signature != IMAGE_NT_SIGNATURE ||
      nt->OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC ||
      nt->OptionalHeader.SizeOfImage < sizeof(IMAGE_DOS_HEADER)) {
    diagnostic = "invalid PE32+ header";
    return false;
  }
  const std::size_t image_size = nt->OptionalHeader.SizeOfImage;
  if (nt_offset > image_size ||
      sizeof(IMAGE_NT_HEADERS64) > image_size - nt_offset) {
    diagnostic = "NT headers outside image";
    return false;
  }
  const auto& directory =
      nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
  if (directory.VirtualAddress == 0 || directory.Size == 0) {
    diagnostic = "no import table";
    return true;
  }
  const std::size_t descriptor_count =
      directory.Size / sizeof(IMAGE_IMPORT_DESCRIPTOR);
  auto* descriptors = image_pointer<IMAGE_IMPORT_DESCRIPTOR>(
      base, image_size, directory.VirtualAddress, descriptor_count);
  if (!descriptors || descriptor_count == 0) {
    diagnostic = "invalid import directory";
    return false;
  }
  for (std::size_t descriptor_index = 0;
       descriptor_index < descriptor_count; ++descriptor_index) {
    const IMAGE_IMPORT_DESCRIPTOR& descriptor =
        descriptors[descriptor_index];
    if (descriptor.Name == 0 && descriptor.FirstThunk == 0 &&
        descriptor.OriginalFirstThunk == 0)
      break;
    const char* library =
        image_string(base, image_size, descriptor.Name);
    if (!library) {
      diagnostic = "invalid import library name";
      return false;
    }
    if (!ascii_equal_folded(library, "vcomp140.dll")) continue;
    if (descriptor.OriginalFirstThunk == 0 || descriptor.FirstThunk == 0) {
      diagnostic = "vcomp140 import has no named thunk";
      return false;
    }
    for (std::size_t thunk_index = 0;
         thunk_index < image_size / sizeof(IMAGE_THUNK_DATA64);
         ++thunk_index) {
      const std::size_t thunk_bytes = thunk_index * sizeof(IMAGE_THUNK_DATA64);
      if (thunk_bytes > (std::numeric_limits<std::uint32_t>::max)()) {
        diagnostic = "vcomp140 thunk offset overflow";
        return false;
      }
      const auto offset = static_cast<std::uint32_t>(thunk_bytes);
      if (descriptor.OriginalFirstThunk >
              (std::numeric_limits<std::uint32_t>::max)() - offset ||
          descriptor.FirstThunk >
              (std::numeric_limits<std::uint32_t>::max)() - offset) {
        diagnostic = "vcomp140 thunk RVA overflow";
        return false;
      }
      auto* source = image_pointer<IMAGE_THUNK_DATA64>(
          base, image_size, descriptor.OriginalFirstThunk + offset);
      auto* destination = image_pointer<IMAGE_THUNK_DATA64>(
          base, image_size, descriptor.FirstThunk + offset);
      if (!source || !destination) {
        diagnostic = "vcomp140 thunk outside image";
        return false;
      }
      if (source->u1.AddressOfData == 0) {
        diagnostic = "vcomp140 import present without omp_get_max_threads";
        return true;
      }
      if (IMAGE_SNAP_BY_ORDINAL64(source->u1.Ordinal)) continue;
      if (source->u1.AddressOfData >
          (std::numeric_limits<std::uint32_t>::max)()) {
        diagnostic = "vcomp140 import name RVA overflow";
        return false;
      }
      const auto name_rva =
          static_cast<std::uint32_t>(source->u1.AddressOfData);
      auto* import = image_pointer<IMAGE_IMPORT_BY_NAME>(
          base, image_size, name_rva);
      const char* name = import
          ? image_string(base, image_size,
                         name_rva + offsetof(IMAGE_IMPORT_BY_NAME, Name))
          : nullptr;
      if (!name) {
        diagnostic = "invalid vcomp140 import name";
        return false;
      }
      if (std::strcmp(name, "omp_get_max_threads") == 0)
        return replace_iat_entry(*destination, diagnostic);
    }
    diagnostic = "unterminated vcomp140 thunk table";
    return false;
  }
  diagnostic = "vcomp140 not imported";
  return true;
}

}  // namespace aexcompat::worker_runtime::imports
