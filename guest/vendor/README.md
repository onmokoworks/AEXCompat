# Pinned Unicorn source

`unicorn-engine-sys` is copied from the crates.io 2.1.5 release. Original licensing and notices remain in its source directory. Cargo uses this copy through `[patch.crates-io]`; the global Cargo cache is unchanged.

Local change: `qemu/exec.c` indexes RAM block offsets in a temporary sorted array after the first free, replacing the inner full scan with lower_bound. The outer traversal, alignment, best-fit comparison, tie-breaking, maps, unmaps, and permissions are unchanged. This targets measured small-allocation overhead in the Sapphire corpus.

Validation: `python3 tools/tests/test_unicorn_ram_offsets.py` compiles the actual selector under ASan/UBSan and compares 12,000 fragmented layouts against the original, including nonfatal index-allocation failure. The `aex-unicorn-buffer` tests execute map/free/remap and protected-memory access checks; the worker library tests cover the existing allocation ownership and execution behavior.

In the same S_Gamma run, the allocation at 1,916 regions decreased from 23.6 ms to 1.8 ms. The previous 300-second observation ended without a result; the patched run reported a subsequent unsupported C++ exception after 84.8 seconds. These are runtime-progress measurements, not rendering success.

The second local change is in `qemu/softmmu/memory.c`: a temporary address-sorted array is used only for disjoint sibling regions. Overlapping siblings and allocation failure retain original priority traversal. A binary search skips flat ranges preceding the current region. Guest visibility, permissions, and the stored region list are unchanged. `tools/tests/test_unicorn_topology.py` compiles the actual renderer under ASan/UBSan and compares 5,000 nested, clipped, overlapping/disjoint layouts (including disabled/readonly regions and allocation failure) against the retained upstream renderer.

The standalone differential harness also measures 4,096 disjoint regions: on the development Mac, the original renderer took 10.934 ms and the indexed renderer 0.757 ms (ASan/UBSan enabled). This is a focused topology measurement. The real S_Gamma run still reached its 180-second observation deadline; setup and rendering completion remain unverified.
