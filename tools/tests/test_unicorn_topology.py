#!/usr/bin/env python3
"""Execute the actual vendored renderer against its retained original version."""
import pathlib
import subprocess
import tempfile
ROOT = pathlib.Path(__file__).resolve().parents[2]
s = (ROOT/'guest/vendor/unicorn-engine-sys/qemu/softmmu/memory.c').read_text()
a = s.index('static int compare_subregion_addresses(')
b = s.index('\n}', s.index('static void render_memory_region(', a)) + 2
patched = s[a:b]
original = (ROOT/'tools/tests/fixtures/unicorn_render_region_original.c').read_text()
fixture = r'''
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <assert.h>
#include <time.h>
typedef __int128 Int128;
typedef uint64_t hwaddr;
#define int128_make64(x) ((Int128)(uint64_t)(x))
#define int128_add(a,b) ((a)+(b))
#define int128_sub(a,b) ((a)-(b))
#define int128_addto(a,b) (*(a)+=(b))
#define int128_subfrom(a,b) (*(a)-=(b))
#define int128_ge(a,b) ((a)>=(b))
#define int128_gt(a,b) ((a)>(b))
#define int128_lt(a,b) ((a)<(b))
#define int128_nz(a) ((a)!=0)
#define int128_get64(a) ((uint64_t)(a))
#define int128_min(a,b) ((a)<(b)?(a):(b))
typedef struct { Int128 start,size; } AddrRange;
static AddrRange addrrange_make(Int128 a,Int128 b) { return (AddrRange){a,b}; }
static Int128 addrrange_end(AddrRange a) { return a.start+a.size; }
static bool addrrange_intersects(AddrRange a,AddrRange b) { return a.start<addrrange_end(b)&&b.start<addrrange_end(a); }
static AddrRange addrrange_intersection(AddrRange a,AddrRange b) {
 Int128 start=a.start>b.start?a.start:b.start;return addrrange_make(start,int128_min(addrrange_end(a),addrrange_end(b))-start);
}
typedef struct MemoryRegion MemoryRegion;
struct Head { MemoryRegion *first; };
struct MemoryRegion { uint64_t addr;Int128 size;bool enabled,readonly,terminates;struct Head subregions;MemoryRegion *next; };
#define QTAILQ_FOREACH(child,head,unused) for((child)=(head)->first;(child);(child)=(child)->next)
typedef struct { MemoryRegion *mr;bool readonly;hwaddr offset_in_region;AddrRange addr; } FlatRange;
typedef struct { FlatRange ranges[8192];unsigned nr; } FlatView;
static void flatview_insert(FlatView *v,unsigned i,FlatRange *r) { assert(v->nr<8192);memmove(v->ranges+i+1,v->ranges+i,(v->nr-i)*sizeof(*r));v->ranges[i]=*r;v->nr++; }
static int deny_alloc;
static void *g_try_malloc(size_t n) { return deny_alloc ? NULL : malloc(n); }
#define g_free free
'''
main = r'''
static uint64_t seed=2026;
static uint64_t rng(void) { seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;return seed; }
int main(void) {
 for(int test=0;test<5000;test++) {
  MemoryRegion root={0},nodes[64]={0};
  root.enabled=true;root.size=4096;root.terminates=test%2;root.readonly=test%7==0;
  for(int i=0;i<32;i++) {
   nodes[i].enabled=rng()%5!=0;nodes[i].readonly=rng()%3==0;nodes[i].terminates=true;
   nodes[i].addr=test%2 ? (rng()%64)*16 : (uint64_t)i*64;
   nodes[i].size=1+rng()%(test%2?256:32);
   nodes[i].next=root.subregions.first;root.subregions.first=&nodes[i];
   if(i%2==0) {
    MemoryRegion *child=&nodes[32+i/2];child->enabled=true;child->terminates=true;
    child->addr=rng()%32;child->size=1+rng()%64;child->readonly=rng()%2;
    nodes[i].subregions.first=child;
   }
  }
  for(deny_alloc=0;deny_alloc<=1;deny_alloc++) {
   FlatView before={0},after={0};AddrRange clip=addrrange_make(test%19,2048+test%2048);
   render_original(&before,&root,0,clip,false);render_memory_region(&after,&root,0,clip,false);
   assert(before.nr==after.nr);
   for(unsigned i=0;i<before.nr;i++) {
    FlatRange *a=&before.ranges[i],*b=&after.ranges[i];
    assert(a->mr==b->mr&&a->readonly==b->readonly&&a->offset_in_region==b->offset_in_region&&a->addr.start==b->addr.start&&a->addr.size==b->addr.size);
   }
  }
 }
 MemoryRegion root={0};MemoryRegion *nodes=calloc(4096,sizeof(*nodes));
 FlatView *old=calloc(1,sizeof(*old)),*new=calloc(1,sizeof(*new));
 assert(nodes&&old&&new);root.enabled=true;root.size=(Int128)4096*4096;
 for(int i=0;i<4096;i++) { nodes[i].addr=(uint64_t)i*4096;nodes[i].size=4096;nodes[i].enabled=true;nodes[i].terminates=true;nodes[i].next=root.subregions.first;root.subregions.first=&nodes[i]; }
 deny_alloc=0;AddrRange clip=addrrange_make(0,root.size);
 clock_t begin=clock();render_original(old,&root,0,clip,false);clock_t middle=clock();
 render_memory_region(new,&root,0,clip,false);clock_t end=clock();
 assert(old->nr==new->nr);
 for(unsigned i=0;i<old->nr;i++) assert(old->ranges[i].mr==new->ranges[i].mr&&old->ranges[i].addr.start==new->ranges[i].addr.start&&old->ranges[i].addr.size==new->ranges[i].addr.size);
 printf("4096 disjoint regions: original %.3f ms, indexed %.3f ms\n",1000.0*(middle-begin)/CLOCKS_PER_SEC,1000.0*(end-middle)/CLOCKS_PER_SEC);
 free(nodes);free(old);free(new);
 puts("5000 nested, clipped, disjoint/overlapping layouts and OOM fallback matched");
}
'''
with tempfile.TemporaryDirectory(prefix='aex-topology-') as temp:
    p=pathlib.Path(temp);(p/'test.c').write_text(fixture+original+patched+main)
    subprocess.run(['cc','-std=c11','-O2','-fsanitize=address,undefined',str(p/'test.c'),'-o',str(p/'test')],check=True)
    subprocess.run([str(p/'test')],check=True)
