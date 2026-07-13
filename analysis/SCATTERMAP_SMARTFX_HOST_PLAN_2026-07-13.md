# ScatterMap SmartFX Host Plan (2026-07-13)

Status: **implemented and verified for the matrix below**.

The first SmartFX CPU case uses the same 16x12 ARGB8 gradient and default
parameters as the proven classic case. The worker will:

1. initialize and register parameters through the proven L2 path;
2. dispatch Smart PreRender selector 23 with a 24-byte extra block;
3. provide bounded checkout-layer results for input id 0 while returning
   unavailable for the unconnected map layer;
4. validate result/max-result rectangles remain within 16x12;
5. dispatch Smart Render selector 24 with a 16-byte extra block;
6. expose input and output worlds only through the observed 24-byte callback
   table;
7. require output SHA-256
   `19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9`,
   equal to both the classic default result and independent oracle;
8. repeat in a second disposable process and verify guards/determinism.

GPU callbacks and frameworks remain disabled in this first SmartFX case.
