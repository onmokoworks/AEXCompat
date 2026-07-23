≠rá^—f•ñÿ¶{MÏy 'v√Æ∂õ≠#include "aex_string_table.hpp"

#include <windows.h>

#include <cstring>
#include <limits>
#include <string_view>

namespace aexcompat::aex_strings {
namespace {

constexpr std::size_t kDosHeaderOffset = 0x3c;
constexpr std::size_t kPeSignatureBytes = 4;
constexpr std::size_t kFileHeaderBytes = 20;
constexpr std::size_t kSectionHeaderBytes = 40;
constexpr std::size_t kMaxSections = 96;
constexpr std::size_t kMaxKeyDigits = 10;

uint16_t read_u16(const unsigned char* bytes) {
  return static_cast<uint16_t>(bytes[0]) |
         (static_cast<uint16_t>(bytes[1]) << 8);
}

uint32_t read_u32(const unsigned char* bytes) {
  return static_cast<uint32_t>(bytes[0]) |
         (static_cast<uint32_t>(bytes[1]) << 8) |
         (static_cast<uint32_t>(bytes[2]) << 16) |
         (static_cast<uint32_t>(bytes[3]) << 24);
}

bool range_within(std::size_t offset, std::size_t length, std::size_t size) {
  return offset <= size && length <= size - offset;
}

bool ascii_text(std::string_view text) {
  if (text.empty()) return false;
  for (const unsigned char byte : text) {
    if (byte < 0x20 || byte > 0x7e) return false;
  }
  return true;
}

bool parse_candidate(std::string_view candidate, int32_t& id,
                     std::string& value, bool& is_lstr_candidate) {
  constexpr std::string_view prefix = "$$$/";
  constexpr std::string_view marker = "/LStr/";
  is_lstr_candidate = false;
  if (candidate.size() < prefix.size() ||
      candidate.substr(0, prefix.size()) != prefix)
    return true;
  const std::size_t marker_offset = candidate.find(marker, prefix.size());
  if (marker_offset == std::string_view::npos) return true;
  is_lstr_candidate = true;

  const std::size_t digits_begin = marker_offset + marker.size();
  const std::size_t equals = candidate.find('=', digits_begin);
  if (equals == std::string_view::npos || equals == digits_begin ||
      equals - digits_begin > kMaxKeyDigits ||
      marker_offset == prefix.size())
    return false;
  if (!ascii_text(candidate.substr(prefix.size(), marker_offset - prefix.size())))
    return false;

  int64_t parsed = 0;
  for (std::size_t index = digits_begin; index < equals; ++index) {
    const unsigned char byte = static_cast<unsigned char>(candidate[index]);
    if (byte < '0' || byte > '9') return false;
    parsed = parsed * 10 + static_cast<int64_t>(byte - '0');
    if (parsed > std::numeric_limits<int32_t>::max()) return false;
  }
  if (!ascii_text(candidate.substr(equals + 1))) return false;
  id = static_cast<int32_t>(parsed);
  value.assign(candidate.substr(equals + 1));
  return true;
}

StringTable invalid_table() {
  StringTable result;
  result.status = ParseStatus::Invalid;
  result.values.clear();
  return result;
}

}  // namespace

const char* StringTable::lookup(int32_t id) const {
  const auto found = values.find(id);
  return found == values.end() ? nullptr : found->second.c_str();
}

StringTable parse_readonly_pe_strings(const unsigned char* bytes,
                                      std::size_t size) {
  if (!bytes || !range_within(0, kDosHeaderOffset + 4, size) ||
      read_u16(bytes) != IMAGE_DOS_SIGNATURE)
    return invalid_table();

  const uint32_t pe_offset = read_u32(bytes + kDosHeaderOffset);
  if (pe_offset > size ||
      !range_within(pe_offset, kPeSignatureBytes + kFileHeaderBytes, size) ||
      read_u32(bytes + pe_offset) != IMAGE_NT_SIGNATURE)
    return invalid_table();

  const unsigned char* file_header = bytes + pe_offset + kPeSignatureBytes;
  const uint16_t sections = read_u16(file_header + 2);
  const uint16_t optional_size = read_u16(file_header + 16);
  if (sections == 0 || sections > kMaxSections || optional_size < 2 ||
      !range_within(pe_offset + kPeSignatureBytes + kFileHeaderBytes,
                    optional_size + sections * kSectionHeaderBytes, size))
    return invalid_table();
  const unsigned char* optional = file_header + kFileHeaderBytes;
  const uint16_t optional_magic = read_u16(optional);
  if (optional_magic != IMAGE_NT_OPTIONAL_HDR32_MAGIC &&
      optional_magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC)
    return invalid_table();

  const unsigned char* section = optional + optional_size;
  StringTable result;
  for (uint16_t section_index = 0; section_index < sections;
       ++section_index, section += kSectionHeaderBytes) {
    const uint32_t characteristics = read_u32(section + 36);
    const bool readable = (characteristics & IMAGE_SCN_MEM_READ) != 0;
    const bool writable = (characteristics & IMAGE_SCN_MEM_WRITE) != 0;
    const bool executable = (characteristics & IMAGE_SCN_MEM_EXECUTE) != 0;
    if (!readable || writable || executable) continue;

    const uint32_t raw_offset = read_u32(section + 20);
    const uint32_t raw_size = read_u32(section + 16);
    if (raw_size == 0) continue;
    if (!range_within(raw_offset, raw_size, size)) return invalid_table();
    const auto* raw = bytes + raw_offset;
    for (std::size_t offset = 0; offset + 4 <= raw_size; ++offset) {
      if (std::memcmp(raw + offset, "$$$/", 4) != 0) continue;
      std::size_t end = offset + 4;
      while (end < raw_size && raw[end] != 0) ++end;
      if (end == raw_size) return invalid_table();
      int32_t id = 0;
      std::string value;
      bool is_lstr_candidate = false;
      if (!parse_candidate(
              std::string_view(reinterpret_cast<const char*>(raw + offset),
                               end - offset),
              id, value, is_lstr_candidate))
        return invalid_table();
      if (!is_lstr_candidate) continue;
      if (!result.values.emplace(id, std::move(value)).second)
        return invalid_table();
      result.status = ParseStatus::Valid;
      offset = end;
    }
  }
  return result;
}

}  // namespace aexcompat::aex_strings
