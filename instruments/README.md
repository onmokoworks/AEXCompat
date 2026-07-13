# AEXCompat Instruments

This directory is the only repository boundary allowed to include reviewed
After Effects SDK headers. Set `AE_SDK_ROOT` to a locally reviewed SDK before
configuring CMake. If it is absent, the SDK-independent trace writer and its
selftest still build, while `pf-null-echo` is skipped.

At runtime, traces are written only when a human sets
`AEX_INSTRUMENT_TRACE_DIR` to an existing directory. If it is unset or invalid,
the writer performs no file output. Instruments never start After Effects,
load another plug-in, use the network, or write project files.

```powershell
cmake -S instruments -B target/instruments-build
cmake --build target/instruments-build --config Release
```

Running an instrument inside After Effects is a separate H-4 human-only step.
Built `.aex` binaries must not be committed.

`pf-callback-tracer` records selector order, numeric world descriptors, and a
small named suite census. `pf-crashkit` is a containment test instrument: its
fault popup defaults to `none`, and crash, hang, allocation pressure, or an
explicit PF error can occur only after a human selects that mode inside AE.
