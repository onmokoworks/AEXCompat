#!/usr/bin/env python3
"""Compile the vendored gap selector and compare it with its original algorithm."""
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
source = (ROOT / 'guest/vendor/unicorn-engine-sys/qemu/exec.c').read_text()
start = source.index('static int compare_ram_offsets(')
end = source.index('\nvoid *qemu_ram_get_host_addr', start)
selector = source[start:end]
fixture = r'''
#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>
#include <stdio.h>
#include <assert.h>
typedef uint64_t ram_addr_t;
typedef struct RAMBlock { uint64_t offset, max_length; struct RAMBlock *next; } RAMBlock;
struct uc_struct { struct { RAMBlock *blocks; int freed; } ram_list; };
#define RAM_ADDR_MAX UINT64_MAX
#define PRIu64 "llu"
#define BITS_PER_LONG 64
#define TARGET_PAGE_BITS 12
#define ROUND_UP(n,a) (((n)+(a)-1)&~((a)-1))
#define MIN(a,b) ((a)<(b)?(a):(b))
#define QLIST_EMPTY_RCU(p) (*(p)==NULL)
#define RAMBLOCK_FOREACH(b) for ((b)=uc->ram_list.blocks;(b);(b)=(b)->next)
static int deny_alloc;
static void *g_try_malloc(size_t n) { return deny_alloc ? NULL : malloc(n); }
#define g_free free
static ram_addr_t find_ram_offset_last(struct uc_struct *uc, ram_addr_t size) {
    RAMBlock *b; uint64_t end=0; (void)size;
    RAMBLOCK_FOREACH(b) if (b->offset+b->max_length>end) end=b->offset+b->max_length;
    return ROUND_UP(end,UINT64_C(262144));
}
static ram_addr_t original(struct uc_struct *uc, ram_addr_t size) {
    RAMBlock *block,*next_block; ram_addr_t offset=RAM_ADDR_MAX,mingap=RAM_ADDR_MAX;
    if (!uc->ram_list.blocks) return 0;
    if (!uc->ram_list.freed) return find_ram_offset_last(uc,size);
    RAMBLOCK_FOREACH(block) {
        ram_addr_t candidate=ROUND_UP(block->offset+block->max_length,UINT64_C(262144));
        ram_addr_t next=RAM_ADDR_MAX;
        RAMBLOCK_FOREACH(next_block) if(next_block->offset>=candidate) next=MIN(next,next_block->offset);
        if(next-candidate>=size && next-candidate<mingap) { offset=candidate;mingap=next-candidate; }
    }
    assert(offset!=RAM_ADDR_MAX); return offset;
}
'''
main = r'''
static uint64_t seed=42;
static uint64_t random64(void) { seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;return seed; }
int main(void) {
    RAMBlock blocks[128]; struct uc_struct uc={0};
    assert(find_ram_offset(&uc,4096)==0);
    for(int test=0;test<12000;test++) {
        int count=1+random64()%128;uint64_t offset=0;
        for(int i=0;i<count;i++) {
            offset+=(random64()%128)*4096;
            blocks[i].offset=offset;blocks[i].max_length=(1+random64()%128)*4096;
            offset+=blocks[i].max_length;
        }
        for(int i=count-1;i>0;i--) { int j=random64()%(i+1);RAMBlock temp=blocks[i];blocks[i]=blocks[j];blocks[j]=temp; }
        for(int i=0;i<count;i++) blocks[i].next=i+1<count ? &blocks[i+1]:NULL;
        uc.ram_list.blocks=blocks;uc.ram_list.freed=test%3!=0;
        uint64_t size=(1+random64()%512)*4096;
        uint64_t expected=original(&uc,size);
        for(deny_alloc=0;deny_alloc<=1;deny_alloc++) assert(find_ram_offset(&uc,size)==expected);
    }
    puts("12000 fragmented layouts matched, including allocation-failure fallback");
}
'''
with tempfile.TemporaryDirectory(prefix='aex-ram-offsets-') as temp:
    temp = pathlib.Path(temp)
    c = temp / 'check.c'
    c.write_text(fixture + selector + main)
    exe = temp / 'check'
    subprocess.run(['cc', '-std=c11', '-O2', '-fsanitize=address,undefined', str(c), '-o', str(exe)], check=True)
    subprocess.run([str(exe)], check=True)
