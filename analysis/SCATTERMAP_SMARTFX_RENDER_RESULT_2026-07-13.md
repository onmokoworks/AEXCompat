# ScatterMap SmartFX Render Result

## Result

Eleven SmartFX PreRender and Smart Render cases passed twice each through the
isolated broker. All 22 fresh worker processes completed normally, the broker
survived, result rectangles stayed within each request, and guard bytes stayed
intact.

## Equivalence

- Pixel format: ARGB8
- Cases: default, identity, horizontal, vertical without repeat, mixed 37.5%,
  odd dimensions, padded stride, connected map, inverted map
- Deep-color observation: default 16-bpc world
- Float-color observation: default 32-bpc ARGB128 world
- Deterministic across two native executions per case: yes
- Exact match with the corresponding classic render and independent Python oracle: yes
- Fixture SHA-256: `223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB`

The 16-bpc case exactly reproduces a fixture limitation: although the plug-in
declares deep-color awareness, its current layer helpers copy only `width*4`
bytes per row. The first half of each 16-bpc row is processed as byte pixels and
the second half remains unwritten. Native output and the source-derived oracle
both hash to `FDC0BC732683E9353F9A855D6EA2589B17D43D29D7B538093B474BEC6D5AD026`.
The 32-bpc case likewise supplies a correctly laid out ARGB128 world while the
fixture copies only `width*4` bytes per row. Native and source-derived oracle
outputs hash to `D707B9B7BD7C923182A0BEFCA60985896E473AFF3D0191FC310FC07CAD3FE90B`.
These results record faithful host behavior; they are not evidence of correct
deep/float processing.

## GPU Negotiation

A DirectX/device-0 negotiation case executed GPU device setup, Smart PreRender,
CPU Smart Render fallback, and GPU device setdown twice in isolated workers.
Setup and setdown returned zero. The fixture did not set
`PF_RenderOutputFlag_GPU_RENDER_POSSIBLE`, so the host correctly did not dispatch
`PF_Cmd_SMART_RENDER_GPU`. Both fallback outputs matched the float oracle. This
proves the observable fallback decision, not GPU pixel execution.

## Error Propagation

A fixed missing-input negative control made checkout id 0 return error 4. Both
isolated runs completed without a process crash, returned Smart Render error 4,
left the guarded output entirely at its `0xCC` sentinel value, and exited
nonzero. The broker treated this as the expected error contract rather than a
successful render or an internal failure. Output SHA-256 was
`346790DBFE3BE4137B9B0B36606E504BCE22FF351867A183DE2FE8481BE1006E` in
both runs.

## Fixture-Specific Crash Isolation

A fixed malformed frame contract supplied valid parameters but a null output
world to `PF_Cmd_FRAME_SETUP`. The fixture's Rust wrapper aborted in both fresh
workers. Both exits were classified as `crashed`; neither crash escaped the Job
Object boundary, and the broker survived to write the create-new result report.
This case is accepted only when both workers crash. It is never treated as a
render success or ordinary nonzero error.
