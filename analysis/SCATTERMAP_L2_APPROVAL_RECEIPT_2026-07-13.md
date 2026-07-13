# ScatterMap L2 Approval Receipt (2026-07-13)

- receipt id: `scattermap-l2-20260713-001`;
- authority: repository owner, direct Codex task response;
- decision: `approve_native_selector_dispatch_l2`;
- approved selectors: `GLOBAL_SETUP`, `PARAMS_SETUP`, `GLOBAL_SETDOWN`;
- fixture byte size: `201216`;
- fixture SHA-256:
  `223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB`;
- issued: `2026-07-13T00:00:00+09:00`;
- expires: `2026-08-12T23:59:59+09:00`;
- prerequisite L1 result: `analysis/SCATTERMAP_L1_RESULT_2026-07-13.md`;
- execution boundary: broker-owned allowlist, disposable worker, Job Object,
  timeout kill, fixed DLL search, bounded path-free JSON output.

The owner's direct response was `L2許可します`. This receipt does not approve
render selectors, image payloads, project access, arbitrary suite behavior, or
L3/L4 execution. Missing callbacks must return an explicit unsupported result;
they must not be silently emulated.

