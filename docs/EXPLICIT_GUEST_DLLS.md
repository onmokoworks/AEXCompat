# Explicit PE dependencies in the Unicorn worker

`AEXCOMPAT_GUEST_LIBRARIES` selects a local JSON manifest for the Unicorn
backend. Paths in `path` are resolved relative to that manifest; `name` is the
Windows-visible path or basename. No DLL runs natively in the macOS host.

```json
{
  "libraries": [
    {"name": "C:/runtime/example.dll", "path": "runtime/example.dll"}
  ]
}
```

Run the ordinary worker command with this environment variable set. The library
set is loaded before the primary AEX's process initialization. Unset the variable
to retain primary-only loading. Do not include private runtime files in the repo.

Each library is parsed as PE32+, rebased at a separate guest address, linked to
supplied named exports or existing emulated imports, protected, and initialized
in dependency order. Static TLS templates receive distinct indices and slots.
`LoadLibraryA` and `GetProcAddress` can return these actual guest modules/exports.
Module name, address, and filename queries consult the same registry. Basenames
must be unique; a full-path request must match its manifest alias. Actual input
SHA-256 values are available through `library_reports()` and trace module data.
These identify bytes, not permission to execute or proof of rendering success.

Bounds: 1 MiB manifest, 64 libraries, 128 MiB per input file, 1 GiB total mapped
images. Existing PE parser limits also apply. Guest mapping collisions, missing
exports from supplied libraries, cycles, and failed initializers are errors.
Large DLLs synchronize AVX state by decoding executed instructions with bounded
scratch space, without creating an unbounded table of candidate instruction PCs.

This is an initial pinned library set, not a complete Windows loader. Ordinal and
forwarded exports, dependency cycles, dynamic unloading/detach, and per-thread
static TLS propagation remain unsupported. Dependencies not supplied can still
trap at an unsupported import. Loading a DLL is not evidence that its GPU,
licensing, file, or other runtime services work. The Sapphire 2026.5 experiment
reaches its runtime initializer; no successful Sapphire renders are established.
