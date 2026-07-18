#pragma once

#include <cstdint>
#include <map>
#include <string>
#include <variant>
#include <vector>

namespace aexcompat::strict_json {

struct JsonValue {
  using Object = std::map<std::string, JsonValue>;
  using Array = std::vector<JsonValue>;

  std::variant<std::nullptr_t, bool, int64_t, double, std::string, Object, Array> value;
};

class StrictJsonParser {
 public:
  explicit StrictJsonParser(std::string text);
  bool parse(JsonValue& out);

 private:
  void skip();
  bool string(std::string& out);
  bool value(JsonValue& out);

  std::string text_;
  std::size_t pos_{};
};

}  // namespace aexcompat::strict_json
