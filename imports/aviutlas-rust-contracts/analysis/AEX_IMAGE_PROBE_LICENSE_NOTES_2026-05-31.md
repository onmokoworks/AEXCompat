# AEX Image Probe License Notes - 2026-05-31

Purpose: record dependency and licensing boundaries for the future
`aex-image-probe` implementation.

This is not legal advice. It is an engineering risk note for cleanroom and
publication boundaries.

## Immediate Contract Slice

The first `aex-image-probe` contract slice can use dependencies already present
in `aviutl-rs/Cargo.toml`:

- `serde`
- `serde_json`
- `image`
- `anyhow`
- `thiserror`

No new dependency is required for request/report validation and PNG fixture
generation.

Build-surface note:

- Run the first probe checks with `--no-default-features`.
- The crate default feature set includes GUI and native media dependencies that
  are not needed for the contract-first probe.
- Keeping the first probe minimal makes the cleanroom and publication review
  easier.

## Local AEX Candidate License Evidence

Local source trees inspected:

- `D:\Projects\01_Project\04_Tools\Ae_Plugins\AdaptiveFilterRust`
- `D:\Projects\01_Project\04_Tools\Ae_Plugins\MedianProRust`

Both contain a `LICENSE` file with MIT license text and copyright
`2026 onmokoworks`.

This supports local-only development candidate status for `AdaptiveFilter` and
`MedianPro`, but it does not automatically make compiled `.aex` binaries or
Adobe-SDK-derived build outputs publishable. Keep binary artifacts local unless
explicitly reviewed.

The fixture review gate
`analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json` uses path and size metadata
only. It does not embed hashes, base64 payloads, copied `.aex` binaries,
private images, or source text. Its current queue order recommends reviewing
`AdaptiveFilter` before `MedianPro`, but both compiled binaries remain
local-only and not approved for loading, rendering, or publication.

## After Effects Rust Bindings

Local adjacent AE plug-in projects use:

- `after-effects = { git = "https://github.com/virtualritz/after-effects.git" }`
- `pipl = { git = "https://github.com/virtualritz/after-effects.git" }`

Local cached manifests observed:

- `after-effects` license: `Apache-2.0 OR BSD-3-Clause OR MIT OR Zlib`
- `after-effects-sys` license: `Apache-2.0 OR BSD-3-Clause OR MIT OR Zlib`
- `pipl` license: `MIT OR Apache-2.0`

Risk notes:

- These bindings are useful context for local AE plug-in builds.
- Do not import them into `aviutl-rs` in the first image-probe slice.
- If imported later, perform exact license-file audit and NOTICE/attribution
  planning.
- AE SDK headers/bindings may carry Adobe SDK terms in addition to Rust crate
  license metadata. Treat executable host work as local-only until the SDK
  redistribution posture is reviewed.

## Adobe SDK / Plug-in Binary Boundary

Do not copy into public artifacts:

- Adobe SDK headers;
- generated bindings copied from Adobe SDK headers;
- `.aex` binaries;
- `.aep` / `.aepx` / `.ffx` user assets;
- private project images used as probe inputs.

The first public-candidate tests should use synthetic JSON and generated images
only.

## OFX Boundary

No OFX/OpenFX code is currently present in `aviutl-rs`.

For the OFX route:

- do not vendor OFX SDK/header code without license-file audit;
- do not make OFX a bypass around the AEX worker allowlist;
- do not load `.aex` inside an OFX host process;
- use OFX only as a later facade around the same out-of-process AEX worker.

## Development Chat Rules

For the first implementation:

- use existing dependencies only;
- run with `--no-default-features` where possible;
- do not add `after-effects`, `after-effects-sys`, `pipl`, or OFX dependencies;
- do not load `.aex`;
- do not copy local plug-in binaries into tests;
- do not publish local absolute paths except in local-only analysis artifacts;
- keep reports metadata-only.

## Follow-Up Audit Before Real Worker

Before any real `.aex` loading worker:

1. Confirm selected candidate plug-in source license and local-build status.
2. Confirm whether the worker must include Adobe SDK headers or generated
   bindings.
3. Confirm binary redistribution policy for the worker.
4. Confirm plug-in EULA / project license does not forbid non-Adobe hosting.
5. Confirm sandbox logs contain no private image or binary payloads.
