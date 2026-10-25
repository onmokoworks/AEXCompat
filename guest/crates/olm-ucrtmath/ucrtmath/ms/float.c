/* OLM-internal interoperability implementation. See README.md. */
#include "ucrtmath.h"
#include <stdint.h>
#include <string.h>
#include <math.h>
#include "tables.h"
static uint32_t fb(float x) { uint32_t u; memcpy(&u,&x,4); return u; }
static float fv(uint32_t u) { float x; memcpy(&x,&u,4); return x; }
static uint64_t db(double x) { uint64_t u; memcpy(&u,&x,8); return u; }
static double dv(uint64_t u) { double x; memcpy(&x,&u,8); return x; }
float ucrt_log2f(float x, ucrt_mode mode) {
    (void)mode; /* AVX and SSE paths both use separate multiply/add. */
    uint32_t u=fb(x), a=u&0x7fffffff;
    if (a>0x7f800000) return fv(u|0x400000);
    if (!a) return fv(0xff800000);
    if (u>>31) return fv(0xffc00000);
    if (a==0x7f800000) return x;
    if (u==0x3f800000) return 0.0f;
    if (u<0x800000) u=fb(x*0x1p23f)-0x0b800000;
    uint32_t t=u-0x3f330000;
    int k=(int32_t)t>>23;
    unsigned i=(t>>19)&15;
    double z=fv(u-(t&0xff800000));
    double r=z*log_table[i][0]-1.0, r2=r*r;
    double y=(k+log_table[i][1])+r*0x1.715475f35c8b8p+0;
    double p=(r*0x1.ecabf496832e0p-2-0x1.715479ffae3dep-1)-r2*0x1.712b6f70a7e4dp-2;
    return (float)(y+p*r2);
}

static double sine_poly(double x, int fused) {
    double q=x*x, cube=x*q;
    const double a=-0x1.5555555555555p-3, b=0x1.1111111111111p-7;
    const double c=-0x1.a01a01a01a01ap-13, d=0x1.71de3a556c734p-19;
    if (fused) return fma(fma(fma(fma(d,q,c),q,b),q,a),cube,x);
    return x+((d*q+c)*(q*q)+(b*q+a))*cube;
}
static double cosine_poly(double x, int fused) {
    double q=x*x, q2=q*q;
    const double a=0x1.5555555555555p-5, b=-0x1.6c16c16c16c16p-10;
    const double c=0x1.a01a01a01a019p-16, d=-0x1.27e4fb7789f5cp-22;
    double base=1.0-0.5*q;
    if (fused) return fma(fma(fma(fma(d,q,c),q,b),q,a),q2,base);
    return base+((d*q+c)*q2+(b*q+a))*q2;
}

/* Three little-endian limbs of the fractional product. Extract the leading
 * 53 bits by truncation, then optionally the next 53; rounding here would
 * change the residual returned by the reference reduction. */
static double take_fraction(uint64_t w[3], int point) {
    int limb=2;
    while (limb>=0 && !w[limb]) --limb;
    if (limb<0) return 0;
    int top=64*limb+63-__builtin_clzll(w[limb]);
    int shift=top-52;
    uint64_t sig;
    if (shift<0) { sig=w[0]<<(-shift); w[0]=0; }
    else {
        int j=shift/64, b=shift%64;
        sig=w[j]>>b;
        if (b && j<2) sig|=w[j+1]<<(64-b);
        for (int i=j+1;i<3;i++) w[i]=0;
        w[j]&=b?((UINT64_C(1)<<b)-1):0;
    }
    return dv(((uint64_t)(top-point+1023)<<52)|(sig&UINT64_C(0xfffffffffffff)));
}
static double large_reduce(double x, int *quadrant, int compensated) {
    uint64_t bits=db(x), mantissa=(bits&UINT64_C(0xfffffffffffff))|UINT64_C(0x10000000000000);
    int e=(int)(bits>>52)-1023;
    unsigned offset=134-e/8-119;
    uint64_t w[3], carry=0;
    for (int i=0;i<3;i++) {
        uint64_t digit; memcpy(&digit,invpio2+offset+8*i,8);
        __uint128_t p=(__uint128_t)digit*mantissa+carry;
        w[i]=(uint64_t)p; carry=(uint64_t)(p>>64);
    }
    int fraction_bits=54-e%8;
    unsigned up=(unsigned)((w[2]>>(fraction_bits-1))&1);
    *quadrant=(int)(((w[2]>>fraction_bits)+up)&3);
    if (up) for (int i=0;i<3;i++) w[i]=~w[i];
    w[2]&=(UINT64_C(1)<<fraction_bits)-1;
    double hi=take_fraction(w,128+fraction_bits);
    double lo=compensated?take_fraction(w,128+fraction_bits):0;
    if (up) {hi=-hi;lo=-lo;}
    double y=hi*0x1.921fb54442d18p+0;
    if (!compensated) return y;
    double h=dv(db(hi)&UINT64_C(0xfffffffff8000000)), l=hi-h;
    double correction=h*0x1.921fb50000000p+0-y;
    correction=fma(l,0x1.921fb50000000p+0,correction);
    correction=fma(h,0x1.110b460000000p-26,correction);
    correction=fma(l,0x1.110b460000000p-26,correction);
    double tail=fma(hi,0x1.1a62633145c06p-54,lo*0x1.921fb54442d18p+0);
    return y+(correction+tail);
}

static float trig(float input, int cosine, int fused) {
    uint32_t raw=fb(input), a=raw&0x7fffffff;
    if (a>=0x7f800000) return fv(a>0x7f800000 ? raw|0x400000 : 0xffc00000);
    double x=input;
    if (a<=0x3f490fdb) {
        if (!cosine) {
            if (a<0x39000000) return input;
            if (a<0x3c000000) {
                double cube=(x*x)*x;
                return (float)(fused?fma(-cube,0x1.5555555555555p-3,x):x-cube*0x1.5555555555555p-3);
            }
            return (float)sine_poly(x,fused);
        }
        if (fused && a<0x3c000000) return a<0x39000000?1.0f:(float)fma(-0.5*x,x,1.0);
        return (float)cosine_poly(x,fused);
    }
    x=fabs(x);
    uint32_t limit=fused?(cosine?0x4f490fdb:0x4b800456):(cosine?0x48f42400:0x4b742400);
    int n, reduced_exp=1023;
    double r;
    if (a>=limit) r=large_reduce(x,&n,cosine&&fused);
    else {
        double nd=fused?fma(x,0x1.45f306dc9c883p-1,0.5):x*0x1.45f306dc9c883p-1+0.5;
        n=(int)nd;
        double head=fused?fma(-(double)n,0x1.921fb54400000p+0,x):x-n*0x1.921fb54400000p+0;
        r=head-n*0x1.0b4611a626331p-34;
        reduced_exp=(int)((db(r)>>52)&2047);
        if (!fused && (int)(db(x)>>52)-reduced_exp>15) {
            double part=n*0x1.0b4611a600000p-34;
            double rem=head-part;
            double tail=n*0x1.3198a2e037073p-69-((head-rem)-part);
            r=rem-tail;
        }
    }
    int use_cos=(n&1)^cosine;
    double y;
    if (!fused && reduced_exp<0x3f2) {
        if (reduced_exp<=0x3de) y=use_cos?1.0:r;
        else y=use_cos?1.0-(r*r)*0.5:r-(0x1.5555555555555p-3*r)*(r*r);
    } else y=use_cos?cosine_poly(r,fused):sine_poly(r,fused);
    int negative=cosine?((n+1)&2):((n&2)^((raw>>31)*2));
    if (negative) y=-y;
    return (float)y;
}
float ucrt_sinf(float x, ucrt_mode mode) { return trig(x,0,mode==UCRT_FMA); }
float ucrt_cosf(float x, ucrt_mode mode) { return trig(x,1,mode==UCRT_FMA); }
