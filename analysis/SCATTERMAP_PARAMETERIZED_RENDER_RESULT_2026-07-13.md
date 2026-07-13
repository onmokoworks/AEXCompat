# ScatterMap Parameterized Render Result

The fixed-hash classic worker now accepts five broker-validated parameter
values through a dedicated `--render-request` mode. It independently rechecks
integer syntax, finiteness, and the production-observed ranges before loading
the AEX, writes the values into the discovered PF parameter definitions, and
echoes every bound value in its report.

The Rust broker computes an independent ARGB8 oracle for arbitrary valid
Amount, Direction, Seed, and Mix combinations. Invert Map is still bound to the
effect parameter but has no pixel effect while the optional map layer is
unconnected. An accepted request runs twice in disposable Job Object-isolated
workers and requires exact value echo, selector success, intact guards,
determinism, and oracle hash parity.

The native CLI rendered the non-table endpoint combination Amount 500,
Direction 1, Seed 10000, Mix 0, Invert 1. Both runs returned error 0 and SHA-256
`863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7`,
exactly matching the dynamic oracle. The broker survived and reported
`passed: true`.

A second non-table case used Amount 13, Direction 2, Seed 1234, and Mix
33.333333333. It exposed an oracle precision detail hidden by the earlier 37.5
case: the plug-in converts Mix to f32 before dividing by 100. Updating both
independent oracles to that operation order produced SHA-256
`3905F287DBF3042CD73527154B8B6DA89E21A86C3ECDB6902D51A67F2CB79CF1`,
which matched both isolated native runs exactly. The worker reports Mix with 17
digits so the broker can verify a lossless double roundtrip before accepting
the render.

The same execution route rejected Direction 4 with exit 3, recorded
`native_process_started: false`, and did not load the AEX. Direct worker
defense-in-depth also rejected Amount 501 with exit 3 before hash verification
or native loading.

This establishes a generalized classic ARGB8 parameter path for an unconnected
map. Generalized source images, connected map payloads, Repeat Edge exposure,
and parameterized SmartFX dispatch remain separate compatibility increments.

## SmartFX Parity

The same strict request ABI is now implemented for SmartFX CPU rendering. The
worker revalidates all five values before loading the AEX, binds them before
Smart PreRender, and reports the exact values with a 17-digit Mix roundtrip.
The broker uses the separate SmartFX allowlist and receipt, then requires two
Job Object-isolated runs with successful PreRender and Render selectors, valid
result rectangles, intact guards, deterministic output, and the same dynamic
oracle used by classic rendering.

Amount 13, Direction 2, Seed 1234, Mix 33.333333333, Invert 0 produced
`3905F287DBF3042CD73527154B8B6DA89E21A86C3ECDB6902D51A67F2CB79CF1`
in both SmartFX runs, exactly equal to classic and the independent oracle.
Direction 4 was rejected by the SmartFX execution route with exit 3 and
`native_process_started: false`; direct worker validation also returned exit 3.

Parameterized SmartFX is currently proven for CPU ARGB8 with an unconnected map.
Connected map request payloads, partial output requests combined with arbitrary
values, deep/float parameterized worlds, and GPU execution remain separate
compatibility increments.
