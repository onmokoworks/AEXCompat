#include "worker_companion_manifest.hpp"

#include "strict_json.hpp"

#include <algorithm>
#include <fstream>
#include <set>
#include <system_error>

namespace aexcompat::worker_runtime::companions {
namespace {
using aexcompat::strict_json::JsonValue;
using aexcompat::strict_json::StrictJsonParser;
using aexcompat::strict_json::json_exact_keys;
using aexcompat::strict_json::json_member;
using aexcompat::strict_json::json_string;
using aexcompat::strict_json::json_u64;

std::wstring lowercase(std::wstring value) {
  std::transform(value.begin(), value.end(), value.begin(), towlower);
  return value;
}

std::wstring comparable_canonical_path(const std::filesystem::path& path) {
  std::wstring value = path.wstring();
  if (value.rfind(LR"(\\?\UNC\)", 0) == 0)
    value = L"\\\\" + value.substr(8);
  else if (value.rfind(LR"(\\?\)", 0) == 0)
    value.erase(0, 4);
  std::replace(value.begin(), value.end(), L'/', L'\\');
  return lowercase(std::move(value));
}

bool valid_sha256(const std::string& value) {
  if (value.size() != 64) return false;
  return std::all_of(value.begin(), value.end(), [](const unsigned char ch) {
    return (ch >= '0' && ch <= '9') || (ch >= 'a' && ch <= 'f') ||
           (ch >= 'A' && ch <= 'F');
  });
}

bool canonical_exact(const std::filesystem::path& path,
                     std::filesystem::path& canonical) {
  if (path.empty() || !path.is_absolute()) return false;
  if (comparable_canonical_path(path) !=
      comparable_canonical_path(path.lexically_normal())) return false;
  std::error_code error;
  const auto absolute = std::filesystem::absolute(path, error);
  if (error) return false;
  const auto input_spelling = comparable_canonical_path(absolute);
  const auto normalized_spelling = comparable_canonical_path(
      std::filesystem::path(absolute).lexically_normal());
  if (input_spelling != normalized_spelling) return false;
  canonical = std::filesystem::canonical(path, error);
  return !error && input_spelling ==
      comparable_canonical_path(canonical);
}
}  // namespace

bool load_manifest(const std::filesystem::path& path, Manifest& result) {
  std::filesystem::path canonical_manifest;
  if (!canonical_exact(path, canonical_manifest)) return false;
  std::error_code error;
  const uint64_t size = std::filesystem::file_size(canonical_manifest, error);
  if (error || size == 0 || size > kMaximumManifestBytes) return false;
  std::ifstream input(canonical_manifest, std::ios::binary);
  if (!input) return false;
  std::string text((std::istreambuf_iterator<char>(input)), {});
  JsonValue root;
  if (input.bad() || !StrictJsonParser(std::move(text)).parse(root) ||
      !std::holds_alternative<JsonValue::Object>(root.value))
    return false;
  const auto& object = std::get<JsonValue::Object>(root.value);
  std::string schema;
  if (!json_exact_keys(object, {"schema", "companions"}) ||
      !json_string(object, "schema", schema) ||
      schema != "companion-manifest-v1")
    return false;
  const auto* companions = json_member(object, "companions");
  if (!companions ||
      !std::holds_alternative<JsonValue::Array>(companions->value))
    return false;
  const auto& array = std::get<JsonValue::Array>(companions->value);
  if (array.empty() || array.size() > kMaximumCompanions) return false;

  Manifest parsed;
  parsed.manifest_path = canonical_manifest;
  std::set<std::wstring> paths;
  std::set<std::tuple<std::string, uint64_t, uint64_t>> suite_identities;
  std::size_t suite_count = 0;
  for (const auto& value : array) {
    if (!std::holds_alternative<JsonValue::Object>(value.value)) return false;
    const auto& companion = std::get<JsonValue::Object>(value.value);
    if (!json_exact_keys(companion, {"path", "sha256", "suites"})) return false;
    std::string path_text;
    Entry entry;
    if (!json_string(companion, "path", path_text) || path_text.empty() ||
        !json_string(companion, "sha256", entry.sha256) ||
        !valid_sha256(entry.sha256))
      return false;
    if (!canonical_exact(std::filesystem::u8path(path_text), entry.path) ||
        !paths.insert(comparable_canonical_path(entry.path)).second)
      return false;
    const auto* suites = json_member(companion, "suites");
    if (!suites || !std::holds_alternative<JsonValue::Array>(suites->value))
      return false;
    const auto& declared = std::get<JsonValue::Array>(suites->value);
    if (declared.empty() || suite_count + declared.size() > kMaximumDeclaredSuites)
      return false;
    for (const auto& suite_value : declared) {
      if (!std::holds_alternative<JsonValue::Object>(suite_value.value)) return false;
      const auto& suite = std::get<JsonValue::Object>(suite_value.value);
      if (!json_exact_keys(
              suite, {"name", "api_version", "internal_version"}))
        return false;
      SuiteIdentity identity;
      uint64_t api_version{}, internal_version{};
      if (!json_string(suite, "name", identity.name) || identity.name.empty() ||
          identity.name.size() > 255 ||
          !json_u64(suite, "api_version", api_version) || api_version == 0 ||
          api_version > INT32_MAX ||
          !json_u64(suite, "internal_version", internal_version) ||
          internal_version > INT32_MAX ||
          !suite_identities
               .insert({identity.name, api_version, internal_version})
               .second)
        return false;
      identity.api_version = static_cast<int32_t>(api_version);
      identity.internal_version = static_cast<int32_t>(internal_version);
      entry.suites.push_back(std::move(identity));
      ++suite_count;
    }
    parsed.entries.push_back(std::move(entry));
  }
  result = std::move(parsed);
  return true;
}

}  // namespace aexcompat::worker_runtime::companions
