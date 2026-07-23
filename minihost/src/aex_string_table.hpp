≠rá^—f•ñÿ¶{MÏy 'v√Æ∂õ≠#pragma once

#include <cstddef>
#include <cstdint>
#include <map>
#include <string>

namespace aexcompat::aex_strings {

enum class ParseStatus {
  NoEntries,
  Valid,
  Invalid,
};

struct StringTable {
  ParseStatus status = ParseStatus::NoEntries;
  std::map<int32_t, std::string> values;

  const char* lookup(int32_t id) const;
};

// Parses only PE sections that are readable and neither writable nor
// executable. The input is a bounded file image, not a mapped module.
StringTable parse_readonly_pe_strings(const unsigned char* bytes,
                                      std::size_t size);

}  // namespace aexcompat::aex_strings
