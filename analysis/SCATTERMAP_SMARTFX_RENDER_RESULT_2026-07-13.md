# ScatterMap SmartFX Render Result

## Result

Default SmartFX PreRender and Smart Render passed twice through the isolated
broker. Both fresh worker processes completed normally, the broker survived,
result rectangles remained within the 16x12 request, and guard bytes remained
intact.

## Equivalence

- Pixel format: ARGB8
- Dimensions: 16x12, rowbytes 64
- Output SHA-256: `19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9`
- Deterministic across two native executions: yes
- Exact match with classic default render and independent Python oracle: yes
- Fixture SHA-256: `223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB`

The evidence is limited to the fixed self-authored fixture and default SmartFX
case. It does not yet establish GPU rendering or full host compatibility.
