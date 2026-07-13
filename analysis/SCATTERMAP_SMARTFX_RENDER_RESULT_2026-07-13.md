# ScatterMap SmartFX Render Result

## Result

Ten SmartFX PreRender and Smart Render cases passed twice each through the
isolated broker. All 20 fresh worker processes completed normally, the broker
survived, result rectangles stayed within each request, and guard bytes stayed
intact.

## Equivalence

- Pixel format: ARGB8
- Cases: default, identity, horizontal, vertical without repeat, mixed 37.5%,
  odd dimensions, padded stride, connected map, inverted map
- Deep-color observation: default 16-bpc world
- Deterministic across two native executions per case: yes
- Exact match with the corresponding classic render and independent Python oracle: yes
- Fixture SHA-256: `223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB`

The 16-bpc case exactly reproduces a fixture limitation: although the plug-in
declares deep-color awareness, its current layer helpers copy only `width*4`
bytes per row. The first half of each 16-bpc row is processed as byte pixels and
the second half remains unwritten. Native output and the source-derived oracle
both hash to `FDC0BC732683E9353F9A855D6EA2589B17D43D29D7B538093B474BEC6D5AD026`.
This records faithful host behavior; it is not evidence of correct deep-color
processing. 32-bpc float and GPU rendering remain unverified.
