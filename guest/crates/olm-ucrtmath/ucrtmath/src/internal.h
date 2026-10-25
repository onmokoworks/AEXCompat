#ifndef UCRTMATH_INTERNAL_H
#define UCRTMATH_INTERNAL_H
#include "ucrtmath.h"
#include <stdint.h>
#include <string.h>
#include <math.h>
#define ALIGN(n) __attribute__((aligned(n)))
#define unlikely(x) __builtin_expect(!!(x), 0)
typedef union { float f; uint32_t u; } flt32_t;
typedef union { double d; uint64_t u; } flt64_t;
static inline uint32_t asuint32(float x) { uint32_t u; memcpy(&u,&x,4); return u; }
static inline uint64_t asuint64(double x) { uint64_t u; memcpy(&u,&x,8); return u; }
static inline float asfloat(uint32_t u) { float x; memcpy(&x,&u,4); return x; }
static inline double asdouble(uint64_t u) { double x; memcpy(&x,&u,8); return x; }
static inline double madd(ucrt_mode m, double a, double b, double c) {
    return m == UCRT_FMA ? fma(a,b,c) : a*b+c;
}
#define GET_BITS_SP32(x,u) ((u)=asuint32(x))
#define GET_BITS_DP64(x,u) ((u)=asuint64(x))
#define PUT_BITS_DP64(u,x) ((x)=asdouble(u))
#define SIGNBIT_SP32 0x80000000u
#define PINFBITPATT_SP32 0x7f800000u
#define NEG_ZERO_F32 0x80000000u
#define POS_ZERO_F32 0u
#define AMD_F_INEXACT 0
#define AMD_F_UNDERFLOW 0
#define ALM_E_IN_X_INF 0
#define ALM_E_IN_X_ZERO 0
static inline float alm_expf_special(float x, int flag) { (void)flag; return x; }
static inline float __alm_handle_errorf(uint32_t u, int flag) { (void)flag; return asfloat(u); }
/* Here input and scaled result are normal binary64 values; float range is
 * strictly inside binary64 range, including the intermediate factor 2^100. */
static inline double scaleDouble_1(double x, int n) { return scalbn(x,n); }
static inline void scaleDownDouble(uint64_t u, int n, uint64_t *v) {
    *v=asuint64(scalbn(asdouble(u),-n));
}
#endif
