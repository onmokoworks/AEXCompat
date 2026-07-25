# Windows / macOS GUI parity

Issue: #494

## Decision

Treat the Windows harness as the functional reference for the basic AEX
editing workflow, not as a pixel-perfect layout specification. The shared
workflow is:

1. select an AEX and input image;
2. inspect editable parameters;
3. edit, reset, and optionally live-render those parameters;
4. inspect input, output, or both in a zoomable Full HD workspace;
5. retain the worker report for diagnosis.

The macOS worker boundary remains the existing generic `setup` / `render-png`
contract. No plug-in identity, parameter name, RVA, or effect algorithm is
encoded in the GUI.

`gui_state.rs` owns the platform-independent state introduced in this slice:
editable parameter defaults/reset, viewer mode, and the 500 ms live-render
debounce. Platform modules remain responsible for drawing widgets and invoking
their worker transports.

## Parity inventory

| Capability | Windows reference | macOS before #494 | #494 result |
| --- | --- | --- | --- |
| Select AEX and image | yes | yes, PNG only | retained |
| Dynamic parameter discovery | rich broker descriptors | guest setup descriptors | retained through generic guest setup |
| Effect Controls side panel | yes | no | added |
| Checkbox / popup / numeric control | yes | compact top grid | added to side panel |
| Reset one / Reset All | yes | no | added |
| Debounced live render | 500 ms | manual Render only | added, default on and user-toggleable |
| Input / Output / Compare modes | yes | fixed two-column view | added |
| Fit / wheel zoom / drag pan | yes | aspect-fit static preview | added |
| Full worker report | yes | yes | retained in collapsed diagnostics |
| Native carrier with fallback | macOS-only concern | yes | retained |
| Dependency approval / sealing | Windows worker security boundary | not applicable to current guest worker | Windows-only |
| Custom UI / AEGP probes | Windows compatibility laboratory | absent | deliberately Windows-only |
| Audio / timeline / compatibility matrix | Windows compatibility laboratory | absent | deferred; not required for image-effect editing |
| 8/16/32 bpc selection | Windows render stack | guest currently ARGB8 | deferred until the guest contract supports it |
| Rich layer/color/point/path parameters | Windows broker descriptors | guest numeric descriptor subset | deferred to guest descriptor/transport expansion |

## Acceptance evidence

- `gui_state` focused tests cover individual reset, reset-all, debounce
  replacement, render readiness, and disabling auto update.
- the GUI-to-worker argument test preserves discovered names and edited values.
- macOS harness tests retain native failure and deadline fallback coverage.
- the macOS Release harness and both guest carriers build.
- the unchanged benchmark AEX renders the Full HD input with default and edited
  numeric parameter values through the same worker arguments used by the GUI.

Observed on the issue #494 Release build:

- AEX SHA-256:
  `f0611785e7b14ac4fcfc75f23b8862beb4539eee52d25d472556849535e96e5b`
- Full HD input SHA-256:
  `9cb64466d3e0891df1b4885cf58c80082afa35794b9a9832d3f884d93c1d0c95`
- edited `Blur Amount=50` output SHA-256:
  `ea1594ad99258d56360737dfdfefab89c3513d8640dc2c56d073b5e388d49a9c`
- worker report: `1920x1080`, `smart-cpu`, applied value `50.0`
- GUI QA: five discovered controls, Auto Update off behavior, individual reset,
  Full HD Render completion, automatic Output selection, and Compare view all
  passed.

## Follow-up rule

Do not create parity issues merely because a Windows diagnostic button is
absent on macOS. Create a follow-up only when normal use of another unchanged
AEX exposes a missing generic descriptor, transport value, image format, or
editing operation.
