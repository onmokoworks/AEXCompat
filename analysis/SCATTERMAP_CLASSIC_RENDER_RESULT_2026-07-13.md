# ScatterMap Classic Render Result (2026-07-13)

Status: **passed; classic default ARGB8 pixel parity proven**.

Receipt `scattermap-render-20260713-001` authorized two isolated classic CPU
render processes for the fixed ScatterMap fixture. Both completed selector 11
with error 0.

Measured case:

- dimensions: 16x12;
- rowbytes: 64;
- format: ARGB8;
- parameters: observed defaults;
- map layer: unconnected (checkout explicitly unavailable);
- input SHA-256:
  `863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7`;
- output SHA-256, run 1 and run 2:
  `19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9`;
- deterministic: true;
- guard bytes intact: true;
- broker survived: true;
- worker classifications: `ok`, `ok`;
- elapsed times observed: 84 ms and 24 ms.

`tools/scattermap_reference_oracle.py` independently implements the documented
self-authored integer hash, f32 conversion/rounding, edge clamp, and default
parameter behavior without loading native code. Its input and output hashes
match the worker report exactly.

This proves one classic CPU/default/no-map ARGB8 case. It does not yet prove
non-default parameters, connected map layers, other dimensions/strides, SmartFX,
GPU, 16-bpc/32-bpc, or After Effects reference-frame parity.

