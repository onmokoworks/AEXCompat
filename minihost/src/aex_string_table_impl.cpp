#include "aex_string_table.hpp"

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

// Bound for the non-LStr key form ($$$/path/key=value), whose "key" is a
// slash-delimited path rather than digits (issue #362: the VR family uses
// this form; the LStr form carries explicit digit ids).
constexpr std::size_t kMaxKeyPathBytes = 256;

// Entry values are free-form text: bundled effects ship empty values
// ($$$/AE/Colorama/Res/4154/LStr/0069=), embedded newlines
// ($$$/MediaCore/AEFilters/AELumetri/InvalidLUT/Message), and UTF-8 text
// ($$$/AE/Effect/Name/OCIOColorSpaceTransform/License/AgreementName), all of
// which the real host serves verbatim. Only the key path stays structural.
struct Candidate {
  bool accepted = false;
  bool lstr = false;
  int32_t id = 0;
  std::string group;
  std::string value;
};

Candidate parse_candidate(std::string_view candidate, int32_t next_ordinal) {
  constexpr std::string_view prefix = "$$$/";
  constexpr std::string_view marker = "/LStr/";
  Candidate result;
  if (candidate.size() < prefix.size() ||
      candidate.substr(0, prefix.size()) != prefix)
    return result;
  const std::size_t marker_offset = candidate.find(marker, prefix.size());
  if (marker_offset == std::string_view::npos) {
    // Non-LStr form ($$$/path/key=value, issue #362): the entry's id is its
    // ordinal position among all $$$ entries in the file, matching the
    // numbering the plug-in's lookup calls use (the digit-suffixed entries
    // such as .../0000= sit at the ordinal their digits name). Entries
    // without '=' (foreign record fragments such as a lone
    // "$$$/LocalizedFileNames/") are skipped without consuming an ordinal.
    const std::size_t equals = candidate.find('=', prefix.size());
    if (equals == std::string_view::npos || equals == prefix.size() ||
        equals - prefix.size() > kMaxKeyPathBytes)
      return result;
    if (!ascii_text(candidate.substr(prefix.size(), equals - prefix.size())))
      return result;
    result.accepted = true;
    result.lstr = false;
    result.id = next_ordinal;
    result.value.assign(candidate.substr(equals + 1));
    return result;
  }

  const std::size_t digits_begin = marker_offset + marker.size();
  const std::size_t equals = candidate.find('=', digits_begin);
  if (equals == std::string_view::npos || equals == digits_begin ||
      equals - digits_begin > kMaxKeyDigits ||
      marker_offset == prefix.size())
    return result;
  if (!ascii_text(candidate.substr(prefix.size(), marker_offset - prefix.size())))
    return result;

  int64_t parsed = 0;
  for (std::size_t index = digits_begin; index < equals; ++index) {
    const unsigned char byte = static_cast<unsigned char>(candidate[index]);
    if (byte < '0' || byte > '9') return result;
    parsed = parsed * 10 + static_cast<int64_t>(byte - '0');
    if (parsed > std::numeric_limits<int32_t>::max()) return result;
  }
  result.accepted = true;
  result.lstr = true;
  result.id = static_cast<int32_t>(parsed);
  result.group.assign(candidate.substr(prefix.size(), marker_offset - prefix.size()));
  result.value.assign(candidate.substr(equals + 1));
  return result;
}

StringTable invalid_table() {
  StringTable result;
  result.status = ParseStatus::Invalid;
  result.values.clear();
  return result;
}

// LStr tables coexist in one image: the effect's own about+params group
// (whose id 0 is the about string "<Name>, v%..."), the match-name/category
// group (id 0 is the bare display name), shared-library groups (CAMLIGHT,
// SOUP), and sibling effects sharing the binary ($$$/AE/Levels next to
// $$$/AE/Levels2). The lookup protocol carries only a bare integer id, so
// the host must serve the group the calling effect was built against: the
// unique group whose id 0 carries the ", v%" about-version pattern. This was
// verified against bundled effects (Card Dance wants Res/4147 "Rows &
// Columns" for id 1, not the match-name group's "Simulation").
bool has_about_version_id0(
    const std::map<int32_t, std::string>& group_values) {
  const auto id0 = group_values.find(0);
  return id0 != group_values.end() &&
         id0->second.find(", v%") != std::string::npos;
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
  // LStr entries are grouped by their directory path so the about-version
  // group can be selected after the whole image is scanned; non-LStr
  // entries keep their flat ordinal ids.
  std::map<std::string, std::map<int32_t, std::string>> lstr_groups;
  std::map<int32_t, std::string> ordinal_values;
  // Ordinal id source for non-LStr $$$ entries (issue #362), counted across
  // all accepted entries in file order.
  int32_t next_ordinal = 0;
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
      const Candidate candidate = parse_candidate(
          std::string_view(reinterpret_cast<const char*>(raw + offset),
                           end - offset),
          next_ordinal);
      if (!candidate.accepted) continue;
      if (candidate.lstr) {
        if (!lstr_groups[candidate.group]
                 .emplace(candidate.id, candidate.value)
                 .second)
          return invalid_table();
      } else {
        if (!ordinal_values.emplace(candidate.id, candidate.value).second)
          return invalid_table();
      }
      ++next_ordinal;
      offset = end;
    }
  }

  StringTable result;
  // Select the serving LStr group: a single group serves as-is; with
  // several, exactly one must carry the about-version id 0 (see
  // has_about_version_id0). Anything else is ambiguous and stays fail-closed.
  if (lstr_groups.size() == 1) {
    result.values = std::move(lstr_groups.begin()->second);
  } else if (lstr_groups.size() > 1) {
    const std::string* primary = nullptr;
    for (const auto& [group, values] : lstr_groups) {
      if (!has_about_version_id0(values)) continue;
      if (primary) return invalid_table();
      primary = &group;
    }
    if (!primary) return invalid_table();
    result.values = std::move(lstr_groups[*primary]);
  }
  // Mixed images (LStr groups plus match-name/category/error path entries)
  // serve only the LStr group: the path entries belong to the registration
  // layer and share the small integer id space, so merging them would
  // collide with the runtime lookup ids the effect actually uses.
  if (lstr_groups.empty()) {
    for (const auto& [id, value] : ordinal_values) {
      if (!result.values.emplace(id, value).second) return invalid_table();
    }
  }
  if (!result.values.empty()) result.status = ParseStatus::Valid;
  return result;
}

}  // namespace aexcompat::aex_strings
