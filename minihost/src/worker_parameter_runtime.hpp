#pragma once

#include "parameter_animation_transport.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <map>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

namespace aexcompat::worker_runtime::parameters {

inline constexpr std::size_t kDefinitionSize = 176;
using Definition = std::array<std::byte, kDefinitionSize>;

struct ParamRecord {
  int32_t index{};
  int32_t disk_id{};
  int32_t type{};
  uint32_t flags{};
  std::string name;
  bool has_numeric{};
  double valid_min{};
  double valid_max{};
  double slider_min{};
  double slider_max{};
  double default_value{};
  double current_value{};
  bool has_current{};
  bool has_color{};
  std::array<unsigned char, 4> default_color{};
  std::array<unsigned char, 4> current_color{};
  std::array<float, 4> default_float_color{};
  std::array<float, 4> current_float_color{};
  std::array<double, 3> default_components{};
  std::array<double, 3> current_components{};
  int32_t component_count{};
  int32_t precision{-1};
  std::string choices;
  std::string label;
  std::string arbitrary_summary;
  int32_t layer_default{};
  Definition raw{};
};

enum class RequestedKind { Integer, Float, Color, Angle, Point, Point3D, ArbitraryText };
struct RequestedAssignment {
  std::wstring id;
  int32_t index{};
  RequestedKind kind{};
  double value{};
  std::array<unsigned char, 4> color{};
  std::array<double, 3> components{};
  std::string text;
};
using RequestedAssignments = std::vector<RequestedAssignment>;

struct ArbitraryTelemetry {
  uint32_t copy_calls{};
  uint32_t dispose_calls{};
  uint32_t invalid_operations{};
  uint32_t print_calls{};
  uint32_t print_failures{};
  uint32_t roundtrip_calls{};
  uint32_t roundtrip_failures{};
  uint32_t scan_calls{};
  uint32_t scan_failures{};
  uint32_t compare_disagreements{};
  uint32_t new_calls{};
  uint32_t interpolation_calls{};
  uint32_t interpolation_failures{};
  double last_interpolation_amount{};
};

struct UiState {
  std::string options_button_name;
  uint32_t options_button_name_calls{};
  bool update_advertised{};
  bool dynamic_flags_advertised{};
  bool conditional_selectors_dispatched{};
  int32_t update_error{-1};
  int32_t dynamic_flags_error{-1};
  void** active_params{};
  std::size_t active_param_count{};
  bool update_active{};
  bool user_changed_active{};
  uint32_t update_calls{};
  bool user_changed_requested{};
  int32_t user_changed_slot{-1};
  int32_t user_changed_error{-1};
};

struct CheckoutState {
  std::map<int32_t, Definition> definitions;
  std::mutex mutex;
  std::unordered_map<void*, uint32_t> live;
  uint32_t checkout_calls{};
  uint32_t checkin_calls{};
  uint32_t automatic_checkins{};
  uint32_t invalid_checkins{};
  uint32_t rejected_temporal{};
  bool wide_time_allowed{};
  int32_t current_time{};
  uint32_t current_time_scale{1};
  int32_t last_index{-1};
  int32_t last_time{};
  int32_t last_time_step{};
  uint32_t last_time_scale{};
};

struct State {
  std::vector<ParamRecord> records;
  std::vector<parameter_animation::ParameterTimeline> timelines;
  std::unordered_map<void*, Definition> keyframe_checkout_ledger;
  std::mutex keyframe_checkout_mutex;
  ArbitraryTelemetry arbitrary;
  UiState ui;
  RequestedAssignments user_changed_parameters;
  CheckoutState checkout;
};

State& state() noexcept;
const parameter_animation::ParameterTimeline* timeline(int32_t slot) noexcept;
bool copy_definition_at_time(int32_t slot, int32_t time, uint32_t scale,
                             const Definition& hosted, Definition& result);

}  // namespace aexcompat::worker_runtime::parameters
