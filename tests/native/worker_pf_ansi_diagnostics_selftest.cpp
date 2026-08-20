#include "worker_callback_diagnostics.hpp"
#include "worker_pf_ansi_runtime.hpp"

#include <iostream>
#include <limits>

int main() {
  using namespace aexcompat::pf_ansi;
  aexcompat::callback_diagnostics::reset();

  char formatted[32]{};
  char copied[32]{};
  if (ansi_atan(0.5) == 0.0 || ansi_atan2(1.0, 1.0) == 0.0 ||
      ansi_ceil(1.5) != 2.0 || ansi_cos(0.0) != 1.0 ||
      ansi_exp(0.0) != 1.0 || ansi_fabs(-2.0) != 2.0 ||
      ansi_floor(1.5) != 1.0 || ansi_fmod(5.0, 2.0) != 1.0 ||
      ansi_hypot(3.0, 4.0) != 5.0 || ansi_log(1.0) != 0.0 ||
      ansi_log10(1.0) != 0.0 || ansi_pow(2.0, 3.0) != 8.0 ||
      ansi_sin(0.0) != 0.0 || ansi_sqrt(4.0) != 2.0 ||
      ansi_tan(0.0) != 0.0 || ansi_sprintf(formatted, "%s", "ok") != 2 ||
      ansi_strcpy(copied, "ok") != copied || ansi_asin(0.0) != 0.0 ||
      ansi_acos(1.0) != 0.0)
    return 1;

  if (ansi_atan(std::numeric_limits<double>::infinity()) != 0.0 ||
      ansi_atan2(std::numeric_limits<double>::quiet_NaN(), 1.0) != 0.0 ||
      ansi_exp(1000.0) != 0.0 || ansi_fmod(1.0, 0.0) != 0.0 ||
      ansi_log(0.0) != 0.0 || ansi_sqrt(-1.0) != 0.0 ||
      ansi_asin(2.0) != 0.0 || ansi_acos(-2.0) != 0.0)
    return 2;

  if (ansi_sprintf(nullptr, "%s", "bad") != -1 ||
      ansi_strcpy(nullptr, "bad") != nullptr)
    return 3;

  std::cout << "{\"schema_version\":1"
            << aexcompat::callback_diagnostics::report_field_json() << "}\n";
  return 0;
}
