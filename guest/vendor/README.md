# Pinned Unicorn source

`unicorn-engine-sys` is copied from the crates.io 2.1.5 release. Original licensing and notices remain in its source directory. Cargo uses this copy through `[patch.crates-io]`; the global Cargo cache is unchanged.

Local change: `qemu/exec.c` indexes RAM block offsets in a temporary sorted array after the first free, replacing the inner full scan with lower_bound. The outer traversal, alignment, best-fit comparison, tie-breaking, maps, unmaps, and permissions are unchanged. This targets measured small-allocation overhead in the Sapphire corpus.

Validation: `python3 tools/tests/test_unicorn_ram_offsets.py` compiles the actual selector under ASan/UBSan and compares 12,000 fragmented layouts against the original, including nonfatal index-allocation failure. The `aex-unicorn-buffer` tests execute map/free/remap and protected-memory access checks; the worker library tests cover the existing allocation ownership and execution behavior.

In the same S_Gamma run, the allocation at 1,916 regions decreased from 23.6 ms to 1.8 ms. The previous 300-second observation ended without a result; the patched run reported a subsequent unsupported C++ exception after 84.8 seconds. These are runtime-progress measurements, not rendering success.
