# ScatterMap Render Host Plan (2026-07-13)

Status: **implementation preparation only; render not approved or executed**.

The first deterministic classic-render case will use a small ARGB8 image with
fixed dimensions, stride, time, and default parameters. The cleanroom host must:

- provide `PF_LayerDef` input/output worlds using the observed 120-byte layout;
- use ARGB byte order (alpha, red, green, blue at offsets 0..3);
- construct all eight parameter pointers in registered order;
- preserve the raw descriptor defaults captured during `PARAMS_SETUP`;
- provide checkout/checkin callbacks that return an explicit unavailable result
  for the unconnected `Scatter Map` layer instead of exposing a null callback;
- dispatch classic render selector 11 only after a separate render receipt;
- hash input/output pixels and save bounded path-free JSON through the broker;
- run the same input/default case twice and require byte-identical output;
- keep SmartFX, GPU, project access, and external layer checkout closed.

No target AEX render selector or image payload was executed while preparing this
plan.
