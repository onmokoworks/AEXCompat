# X04 paired geometry and native instruction watches

The geometry runner executes original AEX bytes in the registered guest. Its
native column is a reviewed structural mapping, **not Windows execution**.
Compile-checking Rust for Windows does not compile the C++ minihost or execute
Windows. `tests/portable/geometry_contract_shape.cpp` separately checks the native
header's typed shape, not its runtime behavior.

Run `python tools/olm/geometry_matrix.py --worker <F54-worker> --worker
<F54b-worker> --worker <F54c-worker> --aex <ColorKeep.aex> --output <new-directory>`.
It runs one process at a time, writes packed typed input by row, streams process
output to files, records output hashes and world headers, and removes pixel dumps.
The matrix covers identity, non-divisible per-axis downsample, pixel aspect,
negotiated output, inset/negative requests, extent, explicit/automatic checkout,
odd/max row padding and UHD at 8/16/32 bpc, plus three admission negatives.
No pixel tolerance or plugin implementation changes are involved.

Structural findings at cb3bb768:

- `render_subsystem.h::SpatialRatio` and `RenderContextState` carry both ratios,
  aspect, full dimensions and pre-effect source origin. Guest and native require
  already sampled input; native flags are not aliases for guest flags.
- `SmartOutputBounds` derives dimensions/origin from result_rect; Classic resize
  uses `ClassicFrameOutput` with the opposite origin convention. This matrix uses
  SmartFX and does not establish Classic resize parity.
- `DiagnosticWorldLayout` supplies input/output padding and extent/request rects.
  Its parser rejects negative requests; the separate smart_output_request path
  accepts signed coordinates. Guest accepts negative negotiated requests, but
  ColorKeep clips the tested request to zero: negative *output* origin remains
  unobserved with this AEX, even though positive inset output is observed.
- `worker_smart_runtime.cpp::pre_checkout_layer` intersects requested rectangles
  with the source bounds, while `checkout_pixels` returns a view of the original
  world. It does not implement F54's cropped allocation, dimensions, stride and
  rebased data pointer. Explicit and automatic checkout rows are a structural gap.
- Native source admits 4096-edge / 16777216-pixel Smart worlds. UHD acceptance is
  structurally comparable; resource budgets remain separate and unchanged.

Existing OLM refs/conformance contains real Windows AE renders and old debugger
captures. Those records do not bind these F54 cases to this minihost revision;
there is no observed Windows execution of this matrix. Do not relabel historical
AE captures as native-minihost fixture runs.
