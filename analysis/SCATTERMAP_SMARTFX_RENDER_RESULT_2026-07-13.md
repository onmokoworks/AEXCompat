# ScatterMap SmartFX Render Result

## Result

Nine SmartFX PreRender and Smart Render cases passed twice each through the
isolated broker. All 18 fresh worker processes completed normally, the broker
survived, result rectangles stayed within each request, and guard bytes stayed
intact.

## Equivalence

- Pixel format: ARGB8
- Cases: default, identity, horizontal, vertical without repeat, mixed 37.5%,
  odd dimensions, padded stride, connected map, inverted map
- Deterministic across two native executions per case: yes
- Exact match with the corresponding classic render and independent Python oracle: yes
- Fixture SHA-256: `223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB`

The evidence is limited to the fixed self-authored fixture and the nine-case
ARGB8 matrix. It does not yet establish deep-color, GPU rendering, or full host
compatibility.
