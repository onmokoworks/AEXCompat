#include "strict_json.hpp"

#include <cctype>
#include <cmath>
#include <utility>

namespace aexcompat::strict_json {

StrictJsonParser::StrictJsonParser(std::string text) : text_(std::move(text)) {}

bool StrictJsonParser::parse(JsonValue& out) {
  skip();
  return value(out) && (skip(), pos_ == text_.size());
}

void StrictJsonParser::skip() {
  while (pos_ < text_.size() &&
         (text_[pos_] == ' ' || text_[pos_] == '\n' || text_[pos_] == '\r' ||
          text_[pos_] == '\t'))
    ++pos_;
}

bool StrictJsonParser::string(std::string& out) {
  skip();
  if (pos_ >= text_.size() || text_[pos_++] != '"') return false;
  while (pos_ < text_.size()) {
    const unsigned char c = text_[pos_++];
    if (c == '"') return true;
    if (c < 0x20 || c >= 0x80) return false;
    if (c != '\\') {
      out.push_back(static_cast<char>(c));
      continue;
    }
    if (pos_ >= text_.size()) return false;
    const char escaped = text_[pos_++];
    if (escaped == '"' || escaped == '\\' || escaped == '/')
      out.push_back(escaped);
    else if (escaped == 'b')
      out.push_back('\b');
    else if (escaped == 'f')
      out.push_back('\f');
    else if (escaped == 'n')
      out.push_back('\n');
    else if (escaped == 'r')
      out.push_back('\r');
    else if (escaped == 't')
      out.push_back('\t');
    else
      return false;
  }
  return false;
}

bool StrictJsonParser::value(JsonValue& out) {
  skip();
  if (pos_ >= text_.size()) return false;
  if (text_[pos_] == '"') {
    std::string parsed;
    if (!string(parsed)) return false;
    out.value = std::move(parsed);
    return true;
  }
  if (text_[pos_] == '{') {
    ++pos_;
    JsonValue::Object object;
    skip();
    if (pos_ < text_.size() && text_[pos_] == '}') {
      ++pos_;
      out.value = std::move(object);
      return true;
    }
    for (;;) {
      std::string key;
      if (!string(key) || object.count(key)) return false;
      skip();
      if (pos_ >= text_.size() || text_[pos_++] != ':') return false;
      JsonValue child;
      if (!value(child)) return false;
      object.emplace(std::move(key), std::move(child));
      skip();
      if (pos_ >= text_.size()) return false;
      const char c = text_[pos_++];
      if (c == '}') break;
      if (c != ',') return false;
    }
    out.value = std::move(object);
    return true;
  }
  if (text_[pos_] == '[') {
    ++pos_;
    JsonValue::Array array;
    skip();
    if (pos_ < text_.size() && text_[pos_] == ']') {
      ++pos_;
      out.value = std::move(array);
      return true;
    }
    for (;;) {
      JsonValue child;
      if (!value(child)) return false;
      array.push_back(std::move(child));
      skip();
      if (pos_ >= text_.size()) return false;
      const char c = text_[pos_++];
      if (c == ']') break;
      if (c != ',') return false;
    }
    out.value = std::move(array);
    return true;
  }
  if (text_.compare(pos_, 4, "null") == 0) {
    pos_ += 4;
    out.value = nullptr;
    return true;
  }
  if (text_.compare(pos_, 4, "true") == 0) {
    pos_ += 4;
    out.value = true;
    return true;
  }
  if (text_.compare(pos_, 5, "false") == 0) {
    pos_ += 5;
    out.value = false;
    return true;
  }

  const std::size_t begin = pos_;
  if (text_[pos_] == '-') ++pos_;
  if (pos_ >= text_.size() ||
      !std::isdigit(static_cast<unsigned char>(text_[pos_])))
    return false;
  if (text_[pos_] == '0')
    ++pos_;
  else
    while (pos_ < text_.size() &&
           std::isdigit(static_cast<unsigned char>(text_[pos_])))
      ++pos_;

  bool real = false;
  if (pos_ < text_.size() && text_[pos_] == '.') {
    real = true;
    ++pos_;
    if (pos_ >= text_.size() ||
        !std::isdigit(static_cast<unsigned char>(text_[pos_])))
      return false;
    while (pos_ < text_.size() &&
           std::isdigit(static_cast<unsigned char>(text_[pos_])))
      ++pos_;
  }
  if (pos_ < text_.size() && (text_[pos_] == 'e' || text_[pos_] == 'E')) {
    real = true;
    ++pos_;
    if (pos_ < text_.size() && (text_[pos_] == '+' || text_[pos_] == '-'))
      ++pos_;
    if (pos_ >= text_.size() ||
        !std::isdigit(static_cast<unsigned char>(text_[pos_])))
      return false;
    while (pos_ < text_.size() &&
           std::isdigit(static_cast<unsigned char>(text_[pos_])))
      ++pos_;
  }
  try {
    std::size_t used{};
    const auto token = text_.substr(begin, pos_ - begin);
    if (real) {
      const double number = std::stod(token, &used);
      if (used != token.size() || !std::isfinite(number)) return false;
      out.value = number;
    } else {
      const auto number = std::stoll(token, &used);
      if (used != token.size()) return false;
      out.value = number;
    }
    return true;
  } catch (...) {
    return false;
  }
}

}  // namespace aexcompat::strict_json
