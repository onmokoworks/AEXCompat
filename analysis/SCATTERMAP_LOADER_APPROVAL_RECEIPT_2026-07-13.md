# ScatterMap Loader Approval Receipt (2026-07-13)

- receipt id: `scattermap-l1-20260713-001`;
- authority: repository owner, direct Codex task response;
- decision: `approve_native_load_l1`;
- approved stage: L1 (`LoadLibraryExW`, export lookup, immediate unload; no
  selector invocation);
- fixture byte size: `201216`;
- fixture SHA-256:
  `223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB`;
- issued: `2026-07-13T00:00:00+09:00`;
- expires: `2026-08-12T23:59:59+09:00`;
- dependency review:
  `analysis/SCATTERMAP_DEPENDENCY_REVIEW_2026-07-13.md`;
- execution boundary: broker-owned allowlist, disposable worker, Job Object,
  timeout kill, fixed DLL search, redacted bounded output.

The owner's explicit response to the `native load` permission request was
`許可します`. This receipt does not approve L2 selector dispatch, L3 parameter
discovery, or L4 rendering. It becomes executable only when every remaining
Safety Gate condition is implemented and re-audited.

