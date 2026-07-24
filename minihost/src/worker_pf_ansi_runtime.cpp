#include "worker_pf_ansi_runtime.hpp"
#include <cerrno>
#include <cmath>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <limits>
namespace aexcompat::pf_ansi {
template <typename Operation>
double finite_ansi_unary(double value, Operation operation) noexcept {
  if (!std::isfinite(value)) return 0.0;
  const double result = operation(value);
  return std::isfinite(result) ? result : 0.0;
}

template <typename Operation>
double finite_ansi_binary(double left, double right, Operation operation) noexcept {
  if (!std::isfinite(left) || !std::isfinite(right)) return 0.0;
  const double result = operation(left, right);
  return std::isfinite(result) ? result : 0.0;
}

double __cdecl ansi_atan(double value) {
  return finite_ansi_unary(value, [](double x) { return std::atan(x); });
}

double __cdecl ansi_atan2(double y, double x) {
  return finite_ansi_binary(y, x, [](double a, double b) { return std::atan2(a, b); });
}

double __cdecl ansi_ceil(double value) {
  return finite_ansi_unary(value, [](double x) { return std::ceil(x); });
}

double __cdecl ansi_cos(double value) {
  return finite_ansi_unary(value, [](double x) { return std::cos(x); });
}

double __cdecl ansi_exp(double value) {
  return finite_ansi_unary(value, [](double x) { return std::exp(x); });
}

double __cdecl ansi_fabs(double value) {
  return finite_ansi_unary(value, [](double x) { return std::fabs(x); });
}

double __cdecl ansi_floor(double value) {
  return finite_ansi_unary(value, [](double x) { return std::floor(x); });
}

double __cdecl ansi_fmod(double value, double divisor) {
  if (divisor == 0.0) return 0.0;
  return finite_ansi_binary(value, divisor, [](double x, double y) { return std::fmod(x, y); });
}

double __cdecl ansi_hypot(double x, double y) {
  return finite_ansi_binary(x, y, [](double a, double b) { return std::hypot(a, b); });
}

double __cdecl ansi_log(double value) {
  if (!(value > 0.0)) return 0.0;
  return finite_ansi_unary(value, [](double x) { return std::log(x); });
}

double __cdecl ansi_log10(double value) {
  if (!(value > 0.0)) return 0.0;
  return finite_ansi_unary(value, [](double x) { return std::log10(x); });
}

double __cdecl ansi_pow(double base, double exponent) {
  return finite_ansi_binary(base, exponent, [](double x, double y) { return std::pow(x, y); });
}

double __cdecl ansi_sin(double value) {
  return finite_ansi_unary(value, [](double x) { return std::sin(x); });
}

double __cdecl ansi_sqrt(double value) {
  if (value < 0.0) return 0.0;
  return finite_ansi_unary(value, [](double x) { return std::sqrt(x); });
}

double __cdecl ansi_tan(double value) {
  return finite_ansi_unary(value, [](double x) { return std::tan(x); });
}

int __cdecl ansi_sprintf(char* destination, const char* format, ...) {
  if (!destination || !format || strnlen_s(format, 256) == 256) return -1;
  va_list arguments;
  va_start(arguments, format);
  va_list measure;
  va_copy(measure, arguments);
  const int required = _vscprintf(format, measure);
  va_end(measure);
  const int written = required >= 0 && required <= 4096
      ? vsprintf_s(destination, static_cast<std::size_t>(required) + 1, format, arguments)
      : -1;
  va_end(arguments);
  return written;
}

char* __cdecl ansi_strcpy(char* destination, const char* source) {
  if (!destination || !source) return nullptr;
  const std::size_t length = strnlen_s(source, 4096);
  if (length == 4096) return nullptr;
  std::memmove(destination, source, length + 1);
  return destination;
}

// PF ANSI Suite v2 entry at index 20 (issue #362): bounded string copy in the
// (destination, size, source) shape plug-ins use to fill fixed-size name
// fields. Truncates and always NUL-terminates; 0 on success, 4 on a
// malformed call, matching the suite's error convention.
int32_t __cdecl ansi_strcpy_bounded(char* destination, std::size_t destination_size,
                                    const char* source) {
  if (!destination || !source || destination_size == 0) return 4;
  const std::size_t length = strnlen_s(source, 4096);
  if (length == 4096) return 4;
  const std::size_t copied = length < destination_size - 1 ? length : destination_size - 1;
  std::memmove(destination, source, copied);
  destination[copied] = '\0';
  return 0;
}

double __cdecl ansi_asin(double value) {
  if (value < -1.0 || value > 1.0) return 0.0;
  return finite_ansi_unary(value, [](double x) { return std::asin(x); });
}

double __cdecl ansi_acos(double value) {
  if (value < -1.0 || value > 1.0) return 0.0;
  return finite_ansi_unary(value, [](double x) { return std::acos(x); });
}
}  // namespace aexcompat::pf_ansi

