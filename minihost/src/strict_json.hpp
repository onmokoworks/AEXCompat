#pragma once

#include <cstdint>
#include <initializer_list>
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

const JsonValue* json_member(const JsonValue::Object& object, const char* key);
bool json_exact_keys(const JsonValue::Object& object,
                     std::initializer_list<const char*> keys);
bool json_i32(const JsonValue::Object& object, const char* key, int32_t& value);
bool json_u64(const JsonValue::Object& object, const char* key, uint64_t& value);
bool json_string(const JsonValue::Object& object, const char* key,
                 std::string& value);
bool json_number(const JsonValue::Object& object, const char* key, double& value);

}  // namespace aexcompat::strict_json
