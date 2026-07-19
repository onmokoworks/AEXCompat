#pragma once
namespace aexcompat::pf_ansi {
double __cdecl ansi_atan(double);
double __cdecl ansi_atan2(double, double);
double __cdecl ansi_ceil(double);
double __cdecl ansi_cos(double);
double __cdecl ansi_exp(double);
double __cdecl ansi_fabs(double);
double __cdecl ansi_floor(double);
double __cdecl ansi_fmod(double, double);
double __cdecl ansi_hypot(double, double);
double __cdecl ansi_log(double);
double __cdecl ansi_log10(double);
double __cdecl ansi_pow(double, double);
double __cdecl ansi_sin(double);
double __cdecl ansi_sqrt(double);
double __cdecl ansi_tan(double);
int __cdecl ansi_sprintf(char*, const char*, ...);
char* __cdecl ansi_strcpy(char*, const char*);
double __cdecl ansi_asin(double);
double __cdecl ansi_acos(double);
}  // namespace aexcompat::pf_ansi
