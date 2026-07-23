≠rá^—f•ñÿ¶{MÏy 'v√Æ∂õ≠#include "aex_string_table.hpp"

#include <windows.h>

#include <cstring>
#include <iostream>
#include <string>
#include <vector>

namespace {

constexpr std::size_t kPeOffset = 0x80;
constexpr std::size_t kSectionTableOffset = 0x188;
constexpr std::size_t kCodeOffset = 0x300;
constexpr std::size_t kDataOffset = 0x500;
constexpr std::size_t kRawSize = 0x100;

void write_u16(std::vector<unsigned char>& bytes, std::size_t offset,
               uint16_t value) {
  bytes[offset] = static_cast<unsigned char>(value);
  bytes[offset + 1] = static_cast<unsigned char>(value >> 8);
}

void write_u32(std::vector<unsigned char>& bytes, std::size_t offset,
               uint32_t value) {
  for (unsigned shift = 0; shift < 32; shift += 8)
    bytes[offset + shift / 8] = static_cast<unsigned char>(value >> shift);
}

std::vector<unsigned char> make_pe(const std::vector<std::string>& strings,
                                   bool duplicate_data = false,
                                   bool invalid_data_range = false) {
  std::vector<unsigned char> bytes(0x700, 0);
  write_u16(bytes, 0, IMAGE_DOS_SIGNATURE);
  write_u32(bytes, 0x3c, static_cast<uint32_t>(kPeOffset));
  write_u32(bytes, kPeOffset, IMAGE_NT_SIGNATURE);
  write_u16(bytes, kPeOffset + 4 + 2, 2);
  write_u16(bytes, kPeOffset + 4 + 16, 0xf0);
  write_u16(bytes, kPeOffset + 4 + 20, IMAGE_NT_OPTIONAL_HDR64_MAGIC);

  const std::size_t code_section = kSectionTableOffset;
  const std::size_t data_section = kSectionTableOffset + 40;
  write_u32(bytes, code_section + 16, kRawSize);
  write_u32(bytes, code_section + 20, kCodeOffset);
  write_u32(bytes, code_section + 36,
            IMAGE_SCN_CNT_CODE | IMAGE_SCN_MEM_READ | IMAGE_SCN_MEM_EXECUTE);
  write_u32(bytes, data_section + 16, invalid_data_range ? 0x1000 : kRawSize);
  write_u32(bytes, data_section + 20, kDataOffset);
  write_u32(bytes, data_section + 36,
            IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ);

  const std::string executable_string = "$$$/AE/Ignore/LStr/0001=Executable";
  std::memcpy(bytes.data() + kCodeOffset, executable_string.c_str(),
              executable_string.size() + 1);
  std::size_t cursor = kDataOffset;
  for (const auto& string : strings) {
    std::memcpy(bytes.data() + cursor, string.c_str(), string.size() + 1);
    cursor += string.size() + 1;
  }
  if (duplicate_data) {
    const std::string duplicate = "$$$/AE/Other/LStr/0003=Duplicate";
    std::memcpy(bytes.data() + cursor, duplicate.c_str(), duplicate.size() + 1);
  }
  return bytes;
}

bool valid_case() {
  const auto bytes = make_pe({"$$$/AE/Arithmetic/LStr/0003=Operator",
                              "$$$/AE/Arithmetic/LStr/0004=Amount"});
  const auto table = aexcompat::aex_strings::parse_readonly_pe_strings(
      bytes.data(), bytes.size());
  return table.status == aexcompat::aex_strings::ParseStatus::Valid &&
         table.values.size() == 2 &&
         std::string(table.lookup(3)) == "Operator" &&
         std::string(table.lookup(4)) == "Amount" &&
         table.lookup(1) == nullptr;
}

bool negative_cases() {
  const auto duplicate = make_pe({"$$$/AE/Arithmetic/LStr/0003=Operator"}, true);
  const auto non_ascii = make_pe({std::string("$$$/AE/Arithmetic/LStr/0003=Op") +
                                  std::string({static_cast<char>(0xc3),
                                               static_cast<char>(0xa9)})});
  const auto out_of_range = make_pe({}, false, true);
  const auto executable_only = make_pe({});
  const auto duplicate_result =
      aexcompat::aex_strings::parse_readonly_pe_strings(duplicate.data(),
                                                        duplicate.size());
  const auto non_ascii_result =
      aexcompat::aex_strings::parse_readonly_pe_strings(non_ascii.data(),
                                                        non_ascii.size());
  const auto out_of_range_result =
      aexcompat::aex_strings::parse_readonly_pe_strings(out_of_range.data(),
                                                        out_of_range.size());
  const auto executable_result =
      aexcompat::aex_strings::parse_readonly_pe_strings(executable_only.data(),
                                                        executable_only.size());
  return duplicate_result.status == aexcompat::aex_strings::ParseStatus::Invalid &&
         non_ascii_result.status == aexcompat::aex_strings::ParseStatus::Invalid &&
         out_of_range_result.status == aexcompat::aex_strings::ParseStatus::Invalid &&
         executable_result.status == aexcompat::aex_strings::ParseStatus::NoEntries;
}

}  // namespace

int main() {
  const bool passed = valid_case() && negative_cases();
  std::cout << "{\"aex_string_table\":\""
            << (passed ? "passed" : "failed")
            << "\",\"readonly_sections_only\":true,\"duplicate_fail_closed\":"
            << (passed ? "true" : "false") << "}\n";
  return passed ? 0 : 1;
}
