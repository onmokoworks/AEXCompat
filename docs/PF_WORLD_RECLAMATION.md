# PF world lifetime and reclamation (#1834)

`PF_UtilCallbacks.new_world`, PF World Suite and legacy variants share the PF
world registry. Their worlds may remain live across frames and independent
cases, including in sequence/global data. They continue to count against the
64-world / 256 MiB live allocation ceilings. Allocation refusal and invalid
foreign, host-owned or double disposal retain their error and fault accounting.
Input/output and checked-out worlds are registered elsewhere; GPU fallback
allocations have explicit transport ownership and cannot be disposed through
the PF callback or reclaimed as plug-in leaks.

GLOBAL_SETDOWN records completion; it does not free residual PF worlds. The
terminal provider hook first shuts down companions, COR and BIB, and verifies
Compute Cache quiescence. Only successful global setdown and successful terminal
quiescence permit reclamation. Cluster swaps retain residual worlds until this
terminal hook, so provider callbacks cannot observe freed storage. Failed
setdown is sticky and prevents reclamation. Process exit remains containment
for failed shutdown. This is host policy: **AE's own reclamation timing has not
been verified with an AE oracle**.

Final reports keep `world_lifetimes_balanced` meaning no live registry world
remains: `worlds_created == worlds_disposed + plugin_leaked_worlds_reclaimed`
and live count/bytes are zero. `worlds_disposed` counts plug-in/transport disposal,
not host reclamation. Reports add:

- `plugin_leaked_worlds_reclaimed`: cumulative reclaimed allocation count.
- `plugin_leaked_world_bytes_reclaimed`: cumulative reclaimed pixel bytes.
- `plugin_world_leak_warning`: true exactly when the reclaimed count is nonzero.

Intermediate case reports can have unbalanced live worlds without failing the
frame. They do not claim reclamation before teardown. Real invalid operations,
output guards, other ownership registries and existing selector failures remain
failures. Native fault-injection probes still test their explicit expected errors.

Broker final acceptance uses the common world-teardown validator (session normal
and abandoned-Smart-output validation, classic/Smart probe operations, parameterized
requests). Missing or contradictory reclamation metadata, nonzero live count or
bytes, and invalid world operations fail. Legacy reports without the extension
retain the previous balance contract. Image results and parameterized summaries
retain the warning fields; session close exposes them alongside the final report.
Performance diagnostics retain a per-session warning list. Bridges use the same
broker close/fallback validation, so no separate counter-equality rule is needed.

Tests exercise registry allocation/disposal, retained worlds, two-phase teardown,
GPU exclusion, stale/foreign disposal, count/byte ceilings, independent case
retention, and broker warning propagation and malformed final reports. Third-party
binaries and machine paths are not test fixtures or committed evidence.
