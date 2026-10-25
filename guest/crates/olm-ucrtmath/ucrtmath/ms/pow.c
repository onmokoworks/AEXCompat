/* OLM-internal interoperability implementation. See pow.md for provenance.
 * Requires round-to-nearest, gradual underflow and -ffp-contract=off. */
#include "../include/ucrtmath.h"
#include <stdint.h>
#include <string.h>
#include <math.h>
#include "pow_tables.h"
static uint64_t word(double x) { uint64_t u; memcpy(&u,&x,8); return u; }
static double value(uint64_t u) { double x; memcpy(&x,&u,8); return x; }
#define SIGN UINT64_C(0x8000000000000000)
#define ABS UINT64_C(0x7fffffffffffffff)
#define INF UINT64_C(0x7ff0000000000000)
#define ONE UINT64_C(0x3ff0000000000000)
#define QUIET UINT64_C(0x0008000000000000)
typedef struct { double hi,lo; } extended;
/* 0: noninteger, 1: odd, 2: even. Large doubles are even integers. */
static int parity(uint64_t u) {
 unsigned e=(u>>52)&2047;
 if(e<1023) return 0;
 if(e>1075) return 2;
 uint64_t unit=UINT64_C(1)<<(1075-e);
 return (u&(unit-1)) ? 0 : (u&unit) ? 1 : 2;
}
static int snan(uint64_t u) { return (u&ABS)>INF && !(u&QUIET); }
/* x+y on x86 chooses the first NaN, independent of signaling status.
 * Spell out the payload rule so ARM's signaling-NaN priority cannot differ. */
static double nan_sum(uint64_t x,uint64_t y) {
 return value(((x&ABS)>INF ? x : y)|QUIET);
}
/* Normalize about 0x1.69555p-1, then remove the table center.
 * The high/low sum preserves the log error for the exponent product. */
static extended logarithm(uint64_t x, ucrt_mode mode) {
 uint64_t delta=x-UINT64_C(0x3fe6955500000000);
 int k=(int)((int64_t)delta>>52);
 unsigned i=(delta>>45)&127;
 double z=value(x-(delta&UINT64_C(0xfff0000000000000)));
 double inv=pow_log[i][0], r, q, correction;
 if(mode==UCRT_FMA) {
  r=fma(z,inv,-1.0);
  double half=-0.5*r;
  q=half*r;
  correction=fma(half,r,-q);
 } else {
  double zh=value((word(z)+UINT64_C(0x80000000))&UINT64_C(0xffffffff00000000));
  double rh=zh*inv-1.0, rl=(z-zh)*inv;
  r=rh+rl;
  q=(-0.5*rh)*rh;
  correction=((-0.5*rh)+(-0.5*r))*rl;
 }
 double a=k*0x1.62e42fefa3800p-1+pow_log[i][1];
 double b=k*0x1.ef35793c76730p-45+pow_log[i][2];
 double s=a+r, e=(a-s)+r;
 double t=s+q, f=(s-t)+q;
 /* Preserve mode-specific accumulation order of the compensated quadratic. */
 double tail=mode==UCRT_FMA ? correction+(b+e) : (b+e)+correction;
 tail=tail+f;
 double h=-0.5*r*r;
 double p=((0x1.999999959554ep-1-r*0x1.555555529a47ap-1)
          +(r*0x1.0002b8b263fc3p+0-0x1.2495b9b4845e9p+0)*h);
 p=(p*h+(r*0x1.0000000000006p-1-0x1.5555555555560p-1))*(r*h);
 tail=tail+p;
 double hi=t+tail;
 return (extended){hi,(t-hi)+tail};
}
static extended product(extended l,double y,ucrt_mode mode) {
 if(mode==UCRT_FMA) {
  double hi=l.hi*y;
  return (extended){hi,fma(l.hi,y,-hi)+l.lo*y};
 }
 double lh=value(word(l.hi)&UINT64_C(0xfffffffff8000000));
 double yh=value(word(y)&UINT64_C(0xfffffffff8000000));
 return (extended){lh*yh,((l.hi-lh)+l.lo)*y+(y-yh)*lh};
}
/* exp(high+low) with a 256-entry scale table; the table scale has the
 * index's exponent contribution pre-subtracted. No FMA in this stage. */
static double exponential(extended t,int negative) {
 unsigned e=(word(t.hi)>>52)&2047;
 if(e<969) return negative ? -(1.0+t.hi) : 1.0+t.hi;
 if(e>=1033) return value((negative?SIGN:0)|((word(t.hi)&SIGN)?0:INF));
 double rounded=t.hi*0x1.71547652b82fep+8+0x1.8000000008000p+36;
 uint64_t index=word(rounded)>>16;
 double n=(int32_t)(uint32_t)index;
 double r=((t.hi-n*0x1.62e42fefc0000p-9)+n*0x1.c610ca86c3899p-45)+t.lo;
 unsigned j=index&255;
 uint64_t scale=pow_exp[j].scale+((index+(negative?UINT64_C(0x80000):0))<<44);
 double r2=r*r;
 double correction=((r*0x1.555555555543cp-3+0x1.ffffffffffdbdp-2)*r2
                     +(r+pow_exp[j].tail))
                    +(r*0x1.1111167a4b553p-7+0x1.55555cf16e1edp-5)*(r2*r2);
 if(e<1032) { double s=value(scale); return s*correction+s; }
 if(!(index&UINT64_C(0x80000000))) {
  double s=value(scale-UINT64_C(0x3f10000000000000));
  return (s*correction+s)*0x1p1009;
 }
 double s=value(scale+UINT64_C(0x3fe0000000000000));
 double p=s*correction, z=s+p;
 if(fabs(z)<1.0) {
  /* Round on the subnormal output grid before the final exact scale.
   * Recover both additions' residuals to avoid double rounding. */
  double unit=z<0 ? -1.0 : 1.0;
  double hi=z+unit;
  double lo=((s-z)+p)+((unit-hi)+z);
  z=(lo+hi)-unit;
  if(z==0) z=value(word(s)&SIGN);
 }
 return z*0x1p-1022;
}
double ucrt_pow(double x,double y,ucrt_mode mode) {
 uint64_t ux=word(x),uy=word(y), ax=ux&ABS,ay=uy&ABS;
 if(ay==0) return snan(ux)?nan_sum(ux,uy):1.0;
 if(ux==ONE) return snan(uy)?nan_sum(ux,uy):1.0;
 if(ay>=INF && (ax>INF || ay>INF)) return nan_sum(ux,uy);
 if(ay==INF) {
  if(ax==ONE) return 1.0;
  return ((ax<ONE)==!(uy&SIGN)) ? 0.0 : value(INF);
 }
 int odd=parity(uy)==1;
 if(ax>INF) return value((ux|QUIET)^(((ux&SIGN)&&odd)?SIGN:0));
 if(ax==0 || ax==INF) {
  uint64_t sign=(ux&SIGN)&&odd ? SIGN:0;
  return value(sign|(((ax==0)==!(uy&SIGN)) ? 0:INF));
 }
 if((ux&SIGN)&&!parity(uy)) return value(SIGN|INF|QUIET);
 int negative=(ux&SIGN)&&odd;
 unsigned ye=ay>>52;
 if(ye<958 || ye>=1086) {
  if(ax==ONE) return 1.0;
  if(ye<958) return ax>ONE ? 1.0+y : 1.0-y;
  return ((ax>ONE)==!(uy&SIGN)) ? value(INF) : 0.0;
 }
 if(uy==ONE) return x;
 if(ax==ONE) return negative?-1.0:1.0;
 if(ax<UINT64_C(0x0010000000000000)) ax=(word(x*0x1p52)&ABS)-UINT64_C(0x0340000000000000);
 return exponential(product(logarithm(ax,mode),y,mode),negative);
}
