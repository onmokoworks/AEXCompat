#ifndef UCRT_WIDE_INTEGER_H
#define UCRT_WIDE_INTEGER_H
#include <stdint.h>
/* Portable unsigned 64x64+64 -> 128, using base-2^32 limbs. */
static inline uint64_t ucrt_muladd64(uint64_t a, uint64_t b, uint64_t add, uint64_t *high) {
    uint64_t a0=(uint32_t)a, a1=a>>32, b0=(uint32_t)b, b1=b>>32;
    uint64_t p0=a0*b0, p1=a0*b1, p2=a1*b0, p3=a1*b1;
    uint64_t middle=(p0>>32)+(uint32_t)p1+(uint32_t)p2;
    uint64_t low=(middle<<32)|(uint32_t)p0;
    *high=p3+(p1>>32)+(p2>>32)+(middle>>32);
    uint64_t result=low+add;
    *high += result<low;
    return result;
}
static inline int ucrt_clz64(uint64_t x) {
    int count=0;
    if (!x) return 64;
    while (!(x & (UINT64_C(1)<<63))) { ++count; x<<=1; }
    return count;
}
#endif
