#include "worker_parameter_execution.hpp"

#include <cmath>
#include <cstring>
#include <limits>

namespace aexcompat::l2_detail {
bool parse_parameter_payload(const wchar_t*, worker_runtime::parameters::RequestedAssignments&);
}

namespace aexcompat::worker_runtime::parameter_execution {
bool verify_declared_defaults() {
  auto& records = parameters::state().records;
  const auto saved = records;
  bool ok = true;
  // Parse the real wire format, validate against the loaded declaration, and
  // inspect the ABI bytes a plug-in reads, including reset after an edit.
  for (int type : {1, 2, 7, 10}) {
    for (int scenario = 0; scenario < 6; ++scenario) {
      parameters::ParamRecord record{};
      record.type = type;
      record.has_numeric = true;
      record.valid_min = 1;
      record.valid_max = 100;
      const double below = type == 10 ? double(0.1f) : type == 2 ? 0.125 : 0;
      const double declared[] = {below, 101, 2, 1, 100, -3};
      const double expected[] = {1, 100, 2, 1, 100, -2};
      record.default_value = declared[scenario];
      if (scenario == 5) record.valid_min = record.valid_max = -2;
      // Slider UI range must not replace the valid range.
      record.slider_min = 20;
      record.slider_max = 80;
      records = {record};
      Definitions definitions(2);
      const auto check_applied = [&]() {
        double observed = 0;
        if (type == 10) std::memcpy(&observed, definitions[1].data() + 56, 8);
        else {
          int32_t word = -1;
          std::memcpy(&word, definitions[1].data() + 56, 4);
          observed = type == 2 ? word / 65536.0 : word;
        }
        return observed == expected[scenario] && records[0].has_current &&
            records[0].current_value == expected[scenario] &&
            records[0].default_value == declared[scenario] && records[0].raw == record.raw;
      };
      initialize_parameter_definitions(definitions);
      ok = check_applied() && ok;
      const bool integer = type == 1 || type == 7;
      parameters::RequestedAssignments defaults, explicit_value, edit;
      ok = l2_detail::parse_parameter_payload(integer ? L"v7|p@1:i32=default" :
          L"v7|p@1:f64=default", defaults) && ok;
      ok = l2_detail::parse_parameter_payload(integer ? L"v7|p@1:i32=0" :
          L"v7|p@1:f64=0.125", explicit_value) && ok;
      ok = l2_detail::parse_parameter_payload(integer ? L"v2|p@1:i32=2" :
          L"v2|p@1:f64=2", edit) && ok;
      if (defaults.size() != 1 || explicit_value.size() != 1 || edit.size() != 1) {
        records = saved;
        return false;
      }
      explicit_value[0].value = record.default_value;
      ok = validate_requested_assignments(explicit_value) == (scenario >= 2 && scenario <= 4) && ok;
      edit[0].value = scenario == 5 ? -2 : 2;
      ok = apply_requested_assignments(definitions, edit) && ok;
      ok = records[0].current_value == edit[0].value && ok;
      ok = apply_requested_assignments(definitions, defaults) && ok;
      ok = check_applied() && ok;
      // No caller value can be smuggled through default mode.
      defaults[0].value = 99;
      ok = apply_requested_assignments(definitions, defaults) &&
          check_applied() && ok;
      const auto unchanged = definitions;
      auto invalid = defaults;
      invalid[0].index = 2;
      ok = !apply_requested_assignments(definitions, invalid) && definitions == unchanged && ok;
      invalid = defaults;
      invalid[0].kind = integer ? parameters::RequestedKind::Float : parameters::RequestedKind::Integer;
      ok = !apply_requested_assignments(definitions, invalid) && definitions == unchanged && ok;
      for (double bad : {std::numeric_limits<double>::infinity(),
                         std::numeric_limits<double>::quiet_NaN(),
                         integer ? 2147483648.0 : type == 2 ? 32768.0 :
                             std::numeric_limits<double>::infinity()}) {
        records[0].default_value = bad;
        ok = !apply_requested_assignments(definitions, defaults) && definitions == unchanged && ok;
      }
      records[0].default_value = declared[scenario];
      for (double bad_min : {101.0, std::numeric_limits<double>::infinity(),
                             std::numeric_limits<double>::quiet_NaN()}) {
        records[0].valid_min = bad_min;
        ok = !apply_requested_assignments(definitions, defaults) && definitions == unchanged && ok;
      }
    }
  }
  for (const wchar_t* text : {L"v6|p@1:f64=default", L"v7|p@1:f64=default0",
      L"v7|p@0:f64=default", L"v7|p@1:argb8=default", L"v7|p@1:point=default",
      L"v7|p@1:f64=default;q@1:f64=2", L"v7|p@1:i32=2147483648"}) {
    parameters::RequestedAssignments assignments;
    ok = !l2_detail::parse_parameter_payload(text, assignments) && ok;
  }
  records = saved;
  return ok;
}
}  // namespace aexcompat::worker_runtime::parameter_execution
