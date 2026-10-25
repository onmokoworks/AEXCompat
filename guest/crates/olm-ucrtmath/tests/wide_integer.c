/* clang/GCC independent 128-bit oracle for the portable limb helpers. */
#include "wide_integer.h"
#include <assert.h>
static void check(uint64_t a,uint64_t b,uint64_t carry) {
    __uint128_t expected=(__uint128_t)a*b+carry;
    uint64_t high,low=ucrt_muladd64(a,b,carry,&high);
    assert(low==(uint64_t)expected && high==(uint64_t)(expected>>64));
    assert(ucrt_clz64(a)==(a?__builtin_clzll(a):64));
}
int main(void) {
    uint64_t values[]={0,1,0xffffffff,0x100000000,0x100000001,UINT64_MAX,UINT64_MAX-1,UINT64_C(1)<<63};
    for(unsigned a=0;a<8;a++)for(unsigned b=0;b<8;b++)for(unsigned c=0;c<8;c++)check(values[a],values[b],values[c]);
    uint64_t x=0x123456789abcdef;
    for(unsigned i=0;i<100000;i++) {
        x=x*6364136223846793005ULL+1;uint64_t a=x;
        x=x*6364136223846793005ULL+1;uint64_t b=x;
        x=x*6364136223846793005ULL+1;check(a,b,x);
    }
    return 0;
}
