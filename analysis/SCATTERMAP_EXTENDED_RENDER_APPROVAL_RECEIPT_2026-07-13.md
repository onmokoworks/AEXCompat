# ScatterMap Extended Render Approval Receipt (2026-07-13)

- receipt id: `scattermap-extended-render-20260713-001`;
- authority: repository owner, direct response and continuous authorization;
- decision: `approve_extended_classic_cpu_render`;
- fixture SHA-256:
  `223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB`;
- approved cases: identity, horizontal, vertical-no-repeat, mixed,
  odd-dimensions, padded-stride, connected-map, inverted-map;
- execution: two disposable broker workers per case, 5-second timeout each;
- expires: `2026-08-12T23:59:59+09:00`.

SmartFX/GPU probing is covered by the continuous authorization but remains a
later implementation stage with its own capability evidence.

