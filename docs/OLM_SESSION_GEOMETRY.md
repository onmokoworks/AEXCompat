# Optional guest session geometry

The `aex-guest-worker session` route accepts these OLM-compatible options.
Existing invocations need no changes. Input and output raw files contain packed
active pixels; row padding is added and removed by the host.

| Option | Value / meaning |
|---|---|
| `--downsample-x`, `--downsample-y` | Positive `num/den`, at most one; sampled dimensions round up |
| `--rowbytes-pad` | 0–256 bytes per row |
| `--pixel-aspect` | Positive `num/den` |
| `--extent-hint` | `left,top,right,bottom` in sampled coordinates |
| `--request-rect` | Smart PreRender output request |
| `--negotiate-output` | `1` allocates the returned PreRender rectangle; default `0` |
| `--checkout-rect` | Explicit packed input rectangle, or `auto` for cropping to actual checkout requests; requires negotiation |

With geometry options, positional width and height describe the full layer.
Downsample determines the stored dimensions; an explicit checkout rectangle
further selects the packed payload dimensions. Automatic checkout starts with
the full sampled payload. The resident frame output metadata reports the actual
rectangle. These names match the OLM F52/F52c/F53 fuzz adapter mapping directly.
Geometry cannot replace an explicit resident fixture layout.

The render cap is 3840 by 2160. Allocations that exceed the original data region
map only the needed pages in a separate, bounded 2 GiB address range. That range
is an address-space bound, not a fixed physical allocation. Existing small
allocations retain their addresses. Negotiated output buffers reuse capacity;
cropped world descriptors are released after each checkout scope.

Geometry does not include the separate OLM UCRT or Unicorn NaN-order patches.
Host self-tests and comparisons against an OLM worker are distinct evidence;
matching tested frames does not establish general numerical equivalence.
