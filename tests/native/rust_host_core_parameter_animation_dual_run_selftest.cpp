#include "aexcompat_host_core_adapter.hpp"
#include "parameter_animation_transport.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <limits>
#include <vector>

namespace {

using aexcompat::host_core::AdapterLoadStatus;
using aexcompat::host_core::AdapterV1;
using aexcompat::parameter_animation::AnimationValueKind;
using aexcompat::parameter_animation::ParameterAnimationKey;
using aexcompat::parameter_animation::ParameterTimeline;

constexpr DWORD kSyntheticSehCode = 0xE04A6361UL;

class Verifier {
 public:
  void Require(const char* label, bool condition) noexcept {
    ++checks_;
    if (!condition) {
      std::fprintf(stderr, "%s\n", label);
      ++failures_;
    }
  }
  int checks() const noexcept { return checks_; }
  int failures() const noexcept { return failures_; }

 private:
  int checks_ = 0;
  int failures_ = 0;
};

bool Near(double left, double right) noexcept {
  return std::isfinite(left) && std::isfinite(right) &&
         std::abs(left - right) <=
             1e-12 * (1.0 + std::max(std::abs(left), std::abs(right)));
}

AexHostAnimationKey Encode(const ParameterAnimationKey& key) noexcept {
  AexHostAnimationKey result{};
  result.time = {key.time, key.scale};
  result.interpolation = key.hold ? AEX_HOST_ANIMATION_HOLD
                                  : AEX_HOST_ANIMATION_LINEAR;
  result.kind = key.kind == AnimationValueKind::Scalar
                    ? AEX_HOST_ANIMATION_KIND_SCALAR
                    : (key.kind == AnimationValueKind::Color
                           ? AEX_HOST_ANIMATION_KIND_COLOR
                           : AEX_HOST_ANIMATION_KIND_COMPONENTS);
  result.scalar = key.scalar;
  for (std::size_t index = 0; index < 3; ++index)
    result.components[index] = key.components[index];
  for (std::size_t index = 0; index < 4; ++index)
    result.color[index] = key.color[index];
  result.component_count = static_cast<uint32_t>(key.component_count);
  return result;
}

bool EqualValue(const ParameterAnimationKey& expected,
                const AexHostAnimationValue& actual) noexcept {
  const auto encoded = Encode(expected);
  if (encoded.kind != actual.kind ||
      encoded.component_count != actual.component_count)
    return false;
  if (actual.reserved[0] || actual.reserved[1] || actual.reserved[2] ||
      actual.reserved[3])
    return false;
  if (expected.kind == AnimationValueKind::Scalar)
    return Near(expected.scalar, actual.scalar);
  if (expected.kind == AnimationValueKind::Color)
    for (std::size_t index = 0; index < 4; ++index) {
      if (expected.color[index] != actual.color[index]) return false;
    }
  if (expected.kind == AnimationValueKind::Components)
    for (int index = 0; index < expected.component_count; ++index) {
      if (!Near(expected.components[index], actual.components[index]))
        return false;
    }
  return true;
}

bool IsZero(const AexHostAnimationValue& value) noexcept {
  const AexHostAnimationValue zero{};
  return std::memcmp(&value, &zero, sizeof(value)) == 0;
}

ParameterAnimationKey Key(int32_t time, uint32_t scale,
                          double scalar) noexcept {
  ParameterAnimationKey key;
  key.time = time;
  key.scale = scale;
  key.kind = AnimationValueKind::Scalar;
  key.scalar = scalar;
  return key;
}

void CompareTimeline(Verifier& verify, const AdapterV1& adapter,
                     const ParameterTimeline& timeline,
                     const std::vector<AexHostRationalTime>& times) {
  std::vector<AexHostAnimationKey> encoded;
  for (const auto& key : timeline.keys) encoded.push_back(Encode(key));
  for (const auto& time : times) {
    const auto expected = aexcompat::parameter_animation::evaluate_parameter_animation(
        timeline, time.value, time.scale);
    const auto actual = adapter.EvaluateParameterAnimation(
        time, encoded.data(), static_cast<uint32_t>(encoded.size()));
    verify.Require("dual-run success", actual.return_code == AEX_HOST_OK &&
                                            actual.exception_code == 0);
    verify.Require("dual-run value", EqualValue(expected, actual.value));
  }
}

int32_t __cdecl FaultingEvaluator(const AexHostRationalTime*,
                                  const AexHostAnimationKey*, uint32_t,
                                  AexHostAnimationValue* output) {
  output->kind = AEX_HOST_ANIMATION_KIND_SCALAR;
  RaiseException(kSyntheticSehCode, 0, 0, nullptr);
  return AEX_HOST_OK;
}

void Rejected(Verifier& verify, const AdapterV1& adapter,
              AexHostRationalTime now,
              const std::vector<AexHostAnimationKey>& keys,
              int32_t expected_code = AEX_HOST_INVALID_ARGUMENT) {
  const auto result = adapter.EvaluateParameterAnimation(
      now, keys.empty() ? nullptr : keys.data(),
      static_cast<uint32_t>(keys.size()));
  verify.Require("invalid input code", result.return_code == expected_code);
  verify.Require("invalid input zero output", IsZero(result.value));
}

}  // namespace

int wmain(int argc, wchar_t** argv) {
  Verifier verify;
  if (argc != 2) return 2;
  AdapterV1 adapter;
  verify.Require("load", AdapterV1::Load(argv[1], &adapter) ==
                             AdapterLoadStatus::kOk && adapter.loaded());
  if (!adapter.loaded()) return 2;

  const auto descriptor = adapter.parameter_animation_descriptor();
  verify.Require("descriptor", AdapterV1::IsCompatibleParameterAnimationDescriptor(
                                   descriptor));
  auto altered = descriptor;
  altered.magic ^= 1;
  verify.Require("descriptor magic", !AdapterV1::IsCompatibleParameterAnimationDescriptor(altered));
  altered = descriptor;
  altered.key_alignment += 1;
  verify.Require("descriptor alignment", !AdapterV1::IsCompatibleParameterAnimationDescriptor(altered));
  altered = descriptor;
  altered.capabilities = 0;
  verify.Require("descriptor capability", !AdapterV1::IsCompatibleParameterAnimationDescriptor(altered));

  ParameterTimeline scalar;
  scalar.keys = {Key(-3, 2, -8.0), Key(1, 2, 12.0), Key(2, 1, -4.0)};
  CompareTimeline(verify, adapter, scalar,
                  {{-4, 1}, {-3, 2}, {-1, 2}, {0, 1}, {1, 2},
                   {3, 2}, {2, 1}, {5, 2}});
  scalar.keys[0].hold = true;
  CompareTimeline(verify, adapter, scalar, {{-1, 2}, {0, 1}});

  ParameterTimeline color;
  auto first = Key(0, 1, 0.0);
  first.kind = AnimationValueKind::Color;
  first.color = {0, 1, 100, 255};
  auto second = Key(2, 1, 0.0);
  second.kind = AnimationValueKind::Color;
  second.color = {1, 2, 201, 0};
  color.keys = {first, second};
  CompareTimeline(verify, adapter, color,
                  {{-1, 1}, {0, 1}, {1, 1}, {2, 1}, {3, 1}});
  color.keys[0].hold = true;
  CompareTimeline(verify, adapter, color, {{1, 1}});

  for (int count = 1; count <= 3; ++count) {
    ParameterTimeline components;
    first = Key(-1, 2, 0.0);
    first.kind = AnimationValueKind::Components;
    first.component_count = count;
    first.components = {1.5, -3.0, 10.0};
    second = first;
    second.time = 3;
    second.scale = 2;
    second.components = {-2.5, 7.0, 2.0};
    components.keys = {first, second};
    CompareTimeline(verify, adapter, components,
                    {{-1, 1}, {-1, 2}, {0, 1}, {1, 2}, {3, 2}, {2, 1}});
    if (count < 3) {
      components.keys[1].component_count = count + 1;
      CompareTimeline(verify, adapter, components, {{0, 1}});
    }
  }
  color.keys[0].hold = false;
  color.keys[1].kind = AnimationValueKind::Scalar;
  CompareTimeline(verify, adapter, color, {{1, 1}});

  // Distinct rational times can round to one MSVC double. Both evaluators
  // classify the interval as unrepresentable and leave no usable value.
  const AexHostRationalTime narrow_now{1999999999, 2000000001};
  for (const auto kind : {AnimationValueKind::Scalar,
                          AnimationValueKind::Color,
                          AnimationValueKind::Components}) {
    ParameterTimeline narrow;
    auto left = Key(999999999, 1000000000, 1.0);
    left.kind = kind;
    left.component_count = kind == AnimationValueKind::Components ? 1 : 0;
    auto right = left;
    right.time = 1000000000;
    right.scale = 1000000001;
    right.scalar = 2.0;
    narrow.keys = {left, right};
    const auto cpp = aexcompat::parameter_animation::evaluate_parameter_animation(
        narrow, narrow_now.value, narrow_now.scale);
    verify.Require("collapsed rational C++ fails closed", !std::isfinite(cpp.scalar));
    const std::array<AexHostAnimationKey, 2> encoded = {Encode(left), Encode(right)};
    const auto rust = adapter.EvaluateParameterAnimation(narrow_now,
                                                          encoded.data(), 2);
    verify.Require("collapsed rational Rust code", rust.return_code == AEX_HOST_INVALID_ARGUMENT);
    verify.Require("collapsed rational Rust zero", IsZero(rust.value));
  }

  auto one = Encode(Key(0, 1, 1.0));
  const AexHostRationalTime midpoint{1, 1};
  Rejected(verify, adapter, {0, 0}, {one});
  Rejected(verify, adapter, midpoint, {});
  Rejected(verify, adapter, midpoint, std::vector<AexHostAnimationKey>(257, one));
  auto bad = one;
  bad.kind = 99;
  Rejected(verify, adapter, midpoint, {bad});
  bad = one;
  bad.interpolation = 99;
  Rejected(verify, adapter, midpoint, {bad});
  bad = one;
  bad.kind = AEX_HOST_ANIMATION_KIND_COMPONENTS;
  bad.component_count = 4;
  Rejected(verify, adapter, midpoint, {bad});
  bad = one;
  bad.scalar = std::numeric_limits<double>::quiet_NaN();
  Rejected(verify, adapter, midpoint, {bad});
  bad = one;
  bad.components[0] = std::numeric_limits<double>::infinity();
  Rejected(verify, adapter, midpoint, {bad});
  bad = one;
  bad.time.scale = 0;
  Rejected(verify, adapter, midpoint, {bad});
  bad = one;
  Rejected(verify, adapter, midpoint, {one, bad});
  bad.time = {-1, 1};
  Rejected(verify, adapter, midpoint, {one, bad});

  HMODULE library = LoadLibraryW(argv[1]);
  verify.Require("direct export module", library != nullptr);
  if (library != nullptr) {
    const auto direct = reinterpret_cast<AexHostCoreParameterAnimationEvaluateV1Fn>(
        GetProcAddress(library, "aex_host_core_parameter_animation_evaluate_v1"));
    verify.Require("direct export", direct != nullptr);
    if (direct != nullptr) {
      AexHostAnimationValue direct_output{};
      direct_output.kind = 99;
      verify.Require("null time code",
                     direct(nullptr, &one, 1, &direct_output) ==
                         AEX_HOST_INVALID_ARGUMENT);
      verify.Require("null time zero", IsZero(direct_output));
      direct_output.kind = 99;
      verify.Require("null keys code",
                     direct(&midpoint, nullptr, 1, &direct_output) ==
                         AEX_HOST_INVALID_ARGUMENT);
      verify.Require("null keys zero", IsZero(direct_output));
      auto alias_key = one;
      auto* alias_output = reinterpret_cast<AexHostAnimationValue*>(&alias_key);
      verify.Require("aliased output code",
                     direct(&midpoint, &alias_key, 1, alias_output) == AEX_HOST_OK);
      AexHostAnimationValue alias_value{};
      std::memcpy(&alias_value, &alias_key, sizeof(alias_value));
      verify.Require("aliased output value",
                     alias_value.kind == AEX_HOST_ANIMATION_KIND_SCALAR &&
                         Near(alias_value.scalar, 1.0));
    }
    FreeLibrary(library);
  }

  AexHostAnimationValue output{};
  output.kind = 99;
  const auto null_result = AdapterV1::InvokeParameterAnimationRaw(
      nullptr, &midpoint, &one, 1, &output);
  verify.Require("null function code", null_result.return_code == AEX_HOST_INVALID_STATE);
  verify.Require("null function zero", IsZero(output));
  output.kind = 99;
  const auto fault = AdapterV1::InvokeParameterAnimationRaw(
      &FaultingEvaluator, &midpoint, &one, 1, &output);
  verify.Require("SEH code", fault.return_code == AEX_HOST_SEH_FAULT &&
                                 fault.exception_code == kSyntheticSehCode);
  verify.Require("SEH zero", IsZero(output));

  std::printf("{\"rust_host_core_parameter_animation_dual_run\":\"%s\",\"checks\":%d}\n",
              verify.failures() == 0 ? "passed" : "failed", verify.checks());
  return verify.failures() == 0 ? 0 : 1;
}
