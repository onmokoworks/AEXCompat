# ScatterMap L1 Result (2026-07-13)

Status: **passed; L2 and later stages remain closed**.

The broker resolved local allowlist id `scattermap` to the approved fixture,
verified its 201216-byte identity, and launched the cleanroom L1 worker under
the existing kill-on-close Job Object boundary. The worker independently
recomputed SHA-256
`223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB`,
loaded the module with DLL-directory and System32-only dependency search,
resolved an AEX entrypoint export, and immediately unloaded it.

Observed result:

- broker exit classification: `ok`;
- worker status: `loaded_and_unloaded`;
- identity verified: true;
- entrypoint resolved: true;
- selectors executed: false;
- render performed: false;
- output truncation: false;
- private path exported in report: false.

Negative controls also passed: an incorrect hash was rejected before loading
with exit 10, and a hash-valid synthetic non-PE fixture was reported as
`load_failed` with Windows error 193 and exit 11.

The create-new local runtime report is under `target/l1-results/` and remains
unpublished. This result is evidence only for L1. It is not initialization,
parameter, suite, pixel, or render compatibility evidence.

