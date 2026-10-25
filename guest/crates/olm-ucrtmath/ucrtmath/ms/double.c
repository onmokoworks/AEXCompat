/* OLM-internal interoperability implementation; see README-double.md.
 * Round-to-nearest, gradual underflow, contraction off, explicit FMA only. */
#include "ucrtmath.h"
#include <math.h>
#include <stdint.h>
#include <string.h>
#include "double_tables.h"
static uint64_t bits(double x) { uint64_t u; memcpy(&u,&x,8); return u; }
static double value(uint64_t u) { double x; memcpy(&x,&u,8); return x; }
#define SIGN UINT64_C(0x8000000000000000)
#define INF UINT64_C(0x7ff0000000000000)
#define QUIET UINT64_C(0x0008000000000000)

double ucrt_exp(double x, ucrt_mode mode) {
    uint64_t u=bits(x), a=u&~SIGN;
    if (a>=INF) return a>INF?value(u|QUIET):(u&SIGN?0.0:x);
    if (x>0x1.62e42fefa39efp+9) return value(INF);
    if (x< -0x1.74046dfefd9d0p+9)
        return x< -0x1.74910d52d3051p+9?0.0:value(1);
    if (a<=bits(0x1p-26)) return 1.0+x;
    int n=(int)(x*0x1.71547652b82fep+6), j=n&63, k=(n-j)/64;
    double lead=mode==UCRT_FMA?fma(n,-0x1.62e42fefa0000p-7,x):x+n*(-0x1.62e42fefa0000p-7);
    double r=lead+n*(-0x1.cf79abc9e3b39p-46), q=r*r, p;
    if (mode==UCRT_FMA) {
        p=fma(0x1.6c16c16c16c17p-10,r,0x1.1111111111111p-7);
        p=fma(p,r,0x1.5555555555555p-5);
        p=fma(p,r,0x1.5555555555555p-3);
        p=fma(p,r,0.5);
        p=fma(q,p,r);
    } else {
        double upper=(0x1.6c16c16c16c17p-10*r+0x1.1111111111111p-7)*r+0x1.5555555555555p-5;
        p=((0x1.5555555555555p-3*r+0.5)*q+r)+upper*(q*q);
    }
    double y=(p*exp_full[j]+exp_tail[j])+exp_head[j];
    if (k> -1022 || (k== -1022 && y>=1.0))
        return value(bits(y)+((uint64_t)(int64_t)k<<52));
    return y*value(UINT64_C(1)<<(k+1074));
}

/* Truncate successive 53-bit significands of a 192-bit fixed-point fraction. */
static double fraction(uint64_t w[3], int point) {
    int limb=2;
    while (limb>=0 && !w[limb]) --limb;
    if (limb<0) return 0.0;
    int top=64*limb+63-__builtin_clzll(w[limb]), shift=top-52;
    uint64_t sig;
    if (shift<0) {sig=w[0]<<(-shift);w[0]=0;}
    else {
        int j=shift/64,b=shift%64;
        sig=w[j]>>b;
        if(b && j<2) sig|=w[j+1]<<(64-b);
        for(int i=j+1;i<3;i++) w[i]=0;
        w[j]&=b?((UINT64_C(1)<<b)-1):0;
    }
    return value(((uint64_t)(top-point+1023)<<52)|(sig&UINT64_C(0xfffffffffffff)));
}
typedef struct { double hi,lo; int quadrant; } reduced;
static reduced large_reduce(double x,int fused) {
    uint64_t u=bits(x), m=(u&UINT64_C(0xfffffffffffff))|UINT64_C(0x10000000000000);
    int e=(int)(u>>52)-1023, offset=134-e/8-7, point=54-e%8;
    uint64_t w[3],carry=0;
    for(int i=0;i<3;i++) {
        uint64_t digit;memcpy(&digit,double_invpio2+offset+8*i,8);
        __uint128_t p=(__uint128_t)digit*m+carry;
        w[i]=(uint64_t)p;carry=p>>64;
    }
    unsigned up=(w[2]>>(point-1))&1;
    int n=((w[2]>>point)+up)&3;
    if(up)for(int i=0;i<3;i++)w[i]=~w[i];
    w[2]&=(UINT64_C(1)<<point)-1;
    double a=fraction(w,128+point),b=fraction(w,128+point);
    if(up){a=-a;b=-b;}
    double h=value(bits(a)&UINT64_C(0xfffffffff8000000)),l=a-h;
    double product=a*0x1.921fb54442d18p+0;
    double error=h*0x1.921fb50000000p+0-product;
    if(fused) {
        error=fma(l,0x1.921fb50000000p+0,error);
        error=fma(h,0x1.110b460000000p-26,error);
        error=fma(l,0x1.110b460000000p-26,error);
        error+=fma(a,0x1.1a62633145c06p-54,b*0x1.921fb54442d18p+0);
    } else {
        error+=l*0x1.921fb50000000p+0;
        error+=h*0x1.110b460000000p-26;
        error+=l*0x1.110b460000000p-26;
        error+=a*0x1.1a62633145c06p-54+b*0x1.921fb54442d18p+0;
    }
    double hi=product+error;
    return (reduced){hi,(product-hi)+error,n};
}
static reduced reduce(double x,int fused) {
    if(x>=(fused?0x1.312d000000000p+24:0x1.e848p+18))return large_reduce(x,fused);
    if(fused) {
        double n=fma(x,0x1.45f306dc9c883p-1,0x1.8p52)-0x1.8p52;
        double r=fma(-n,0x1.921fb54442d18p+0,x);
        double t=n*0x1.1a62633145c00p-54;
        double te=fma(n,0x1.1a62633145c00p-54,-t);
        double s=r-t, se=(r-s)-t;
        double hi=fma(-n,0x1.1a62633145c00p-54,r);
        double lo=fma(-n,0x1.b839a252049c0p-104,((s-hi)+se)-te);
        return (reduced){hi,lo,(int)n&3};
    }
    int n=(int)(x*0x1.45f306dc9c883p-1+0.5);
    double h=x-n*0x1.921fb54400000p+0,t=n*0x1.0b4611a626331p-34,r=h-t;
    if ((int)(bits(x)>>52)-(int)((bits(r)>>52)&2047)>15) {
        double part=n*0x1.0b4611a600000p-34, rem=h-part;
        t=n*0x1.3198a2e037073p-69-((h-rem)-part);
        h=rem;r=h-t;
    }
    return (reduced){r,(h-r)-t,n&3};
}
static const double S[]={-0x1.5555555555555p-3,0x1.1111111110bb3p-7,-0x1.a01a019e83e5cp-13,0x1.71de3796cde01p-19,-0x1.ae600b42fdfa7p-26,0x1.5e0b2f9a43bb8p-33};
static const double C[]={0x1.5555555555555p-5,-0x1.6c16c16c16967p-10,0x1.a01a019f4ec91p-16,-0x1.27e4fa17f667bp-22,0x1.1eeb690382eecp-29,-0x1.907db47258aa7p-37};
static double sine(double x,double tail,int fused,int direct) {
    double q=x*x,cube=x*q;
    if(fused) {
        double p=S[5];for(int i=4;i>=1;i--)p=fma(p,q,S[i]);
        if(direct)return fma(cube,fma(p,q,S[0]),x);
        double correction=q*(tail*0.5-cube*p)-tail;
        return x-fma(-cube,S[0],correction);
    }
    double p=((S[5]*q+S[4])*q+S[3])*((q*q)*q)+((S[2]*q+S[1])*q+S[0]);
    if(direct)return x+cube*p;
    return (tail+(cube*p-(q*0.5)*tail))+x;
}
static double cosine(double x,double tail,int fused,int direct) {
    double q=x*x,q2=q*q;
    if(fused) {
        double p=C[5];for(int i=4;i>=0;i--)p=fma(p,q,C[i]);
        if(direct)return fma(fma(p,q,-0.5),q,1.0);
        double half=q*0.5,base=1.0-half;
        double correction=fma(-x,tail,(1.0-base)-half);
        return fma(p,q2,correction)+base;
    }
    if(direct) {
        double q4=q2*q2;
        double p=((C[1]*q+C[0])*q2+(C[3]*q+C[2])*q4)+(q2*q4)*(C[5]*q+C[4]);
        double half=-0.5*q,base=half+1.0;
        return ((1.0-base)+half+p)+base;
    }
    double half=q*0.5;
    double p=((C[2]*q+C[1])*q+C[0])+((C[5]*q+C[4])*q+C[3])*((q*q)*q);
    double correction=(((half-1.0)+1.0)-half)-tail*x;
    return (p*q2+correction)-(half-1.0);
}
static double trig(double x,int cosfn,int fused) {
    uint64_t u=bits(x),a=u&~SIGN;
    if(a>=INF)return value(a>INF?u|QUIET:INF|SIGN|QUIET);
    int direct=cosfn&&fused?a<=bits(0x1.921fb54442d18p-1):a<bits(0x1.921fb54442d18p-1);
    if(direct) {
        if(cosfn) {
            if(a<bits(0x1p-27))return 1.0;
            if(a<bits(0x1p-13))return fused?fma(-x*0.5,x,1.0):1.0-(x*x)*0.5;
            return cosine(x,0,fused,1);
        }
        if(fused) {
            if(a<bits(0x1p-27))return x;
            if(a<bits(0x1p-13))return fma(-((x*x)*x),0x1.5555555555555p-3,x);
        } else if(a<=bits(0x1p-27))return x;
        return sine(x,0,fused,1);
    }
    reduced r=reduce(value(a),fused);
    double y=((r.quadrant&1)^cosfn)?cosine(r.hi,r.lo,fused,0):sine(r.hi,r.lo,fused,0);
    int neg=cosfn?((r.quadrant+1)&2):((r.quadrant&2)^((u>>63)*2));
    if(neg)return fused?value(bits(y)^SIGN):0.0-y;
    return y;
}
double ucrt_sin(double x,ucrt_mode mode){return trig(x,0,mode==UCRT_FMA);}
double ucrt_cos(double x,ucrt_mode mode){return trig(x,1,mode==UCRT_FMA);}
