# X07: Mac GUI image transport

Base: `cb3bb768` (origin/main). This is transport/export evidence, not native AE
numerical equivalence. No guest worker or plug-in arithmetic changed.

## Closed boundaries

- **Image admission:** the primary chooser, secondary layer chooser and drop
  target accept PNG, JPEG (`jpg`/`jpeg`), BMP, TIFF (`tif`/`tiff`) and WebP.
  These are the enabled `image` decoder features. Four committed 3x2 sample
  files pass the GUI drop/texture path and resident encoding at all three
  depths (`gui_admits_jpeg`, `gui_admits_bmp`, `gui_admits_tiff`,
  `gui_admits_webp`). Unsupported extensions and malformed encoded input
  remain rejected. Extension matching is case insensitive.
- **Negotiated output:** the single-effect GUI has an opt-in “Negotiate output
  size” checkbox. The default remains fixed input-sized output. The option is
  part of resident-session reuse identity and reaches `--negotiate-output 1`.
  Response dimensions, signed rectangle, packed rowbytes, depth, generation,
  guards and checksum are validated before export. Negotiated geometry is
  bounded to the existing GUI 3840x2160 boundary and origins to ±4096; the
  actual output dimensions feed PNG/raw/EXR export and origin feeds deep
  artifact metadata. Legacy replies without `rect` remain accepted only in
  fixed-size mode.
- **Deep preview/export:** ARGB16 produces a 16-bit `output.png` beside unchanged
  `output.bin`/`output.json`. FLOAT32 produces a display `output.png` and exact
  ARGB little-endian `output.argb` beside unchanged `output.exr`/`output.json`.
  The PNG is also loaded into the GUI viewer. Chains gain the same PNG export
  and viewer path while preserving their existing exact `output.argb` and
  raw/EXR artifacts. The Windows conversion functions moved unchanged into
  X01's shared `decoded_image` module and retain their old crate visibility.
  AE16 display expansion clamps over-white samples; float display clamps HDR
  and maps non-finite values to zero, exactly as Windows does. Raw/EXR samples
  retain their bits, including signed zero and NaN payloads; PNG is never a
  chain intermediate.

`gui_adapter_negotiates_geometry_and_exports_deep_preview` exercises the real
GUI admission/controller/export path with a process protocol double. The
worker changes 2x1 to 3x2 at origin (-1,2) only if the launch flag is present.
Twelve frames cover two modes, three depths and resident reuse. It verifies
PNG depth/sample values, raw bytes, dimensions, origin and unchanged default
raw/EXR payloads. This double establishes host transport behavior, not guest
negotiation or AEX arithmetic. Additional tests reject invalid geometry,
stale generations, corrupt lengths/checksums and preview overwrite; a two-stage
deep-chain test verifies PNG creation without quantizing raw intermediates.

## Audit corrections and remaining boundaries

The older parity table is a starting-boundary audit. Its “float EXR/raw
fixtures” wording must not be read as a raw sidecar already existing for the
single-effect GUI: that route previously emitted EXR plus JSON only. X07 adds
an explicitly named ARGB float sidecar. No decoder-support discrepancy was
observed in the four format samples; this is not exhaustive codec coverage.

Negotiated geometry across multiple effect stages remains outside X07: the
chain world has no origin field, so preserving origins between stages needs
a separate chain contract change. The checkbox is explicitly single-effect
and disabled for chains. Request rectangles, cropped checkouts, user-editable
origins and the paired native/guest ROI matrix remain separate work.

## Reproduction

Run from this AEXCompat worktree. All commands use one build/test process.
Set a space-free build directory and redirect output to ticket logs:

```sh
mkdir -p "$HOME/.cache/olm-emu/build-X07/tmp" orchestra/state/X07-gui-transport-gaps
export TMPDIR="$HOME/.cache/olm-emu/build-X07/tmp"
export CARGO_TARGET_DIR="$HOME/.cache/olm-emu/build-X07/target"
export CARGO_BUILD_JOBS=1 CARGO_PROFILE_DEV_DEBUG=0
cargo test --manifest-path broker/Cargo.toml -p aexcompat-harness --locked -- --test-threads=1
cargo test --manifest-path broker/Cargo.toml -p aexcompat-broker --lib --locked -- --test-threads=1
cargo check --manifest-path broker/Cargo.toml -p aexcompat-broker --target x86_64-pc-windows-msvc --locked
cargo build --release --manifest-path broker/Cargo.toml -p aexcompat-harness --locked --jobs 1
```

Measured: harness 96 passed / 0 failed / 5 opt-in real-worker tests ignored,
plus two Mac CLI integration tests passed; broker 217 passed / 0 failed.
The macOS Release build, Windows target check and whole tracked Rust formatting check passed.
Windows runtime execution was not performed.

With `AEXCOMPAT_GUEST_WORKER` pointing at the registered `worker-X01b-parity`
worker and `AEXCOMPAT_TEST_AEX` pointing at the original 2025 ColorKeep AEX:

```sh
export AEXCOMPAT_TEST_INPUT_PNG="$PWD/broker/crates/harness/tests/fixtures/black.bmp"
cargo test --manifest-path broker/Cargo.toml -p aexcompat-harness --bin aexcompat-harness real_resident_argb -- --ignored --test-threads=1
cargo test --manifest-path broker/Cargo.toml -p aexcompat-harness --bin aexcompat-harness real_chain_links -- --ignored --test-threads=1
```

Measured: two real resident deep export tests and one real three-depth chain
transport test passed. The retained environment variable name ends in PNG;
its actual test input here is BMP. No worker rebuild was needed.

Independent local review identified a Windows visibility regression in the
first shared-function extraction. Restoring `pub(crate) use` resolved it;
Windows compilation and a fresh review of the full diff passed. No rejected
or unaddressed review findings remain. No claim, push, PR or release-gate change
was made.
