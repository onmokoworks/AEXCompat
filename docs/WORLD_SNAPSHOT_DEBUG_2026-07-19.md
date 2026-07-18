# Intermediate World Snapshots and Row/Channel Checksums (2026-07-19)

Opt-in debugging for pixel differences (issue #19): dump the worlds a render
passes through, and record row/channel checksums of the final output, so a
difference can be narrowed to a stage, a row range, and a channel without
re-running under a debugger. Both features default off and only apply to
broker-dispatched image renders (`--render-experimental*` and the typed
request routes).

## Enabling

```powershell
$env:AEXCOMPAT_DUMP_WORLDS_DIR = 'target/world-dumps'   # snapshots
$env:AEXCOMPAT_CHECKSUM_DETAIL = '1'                    # row/channel checksums
broker\target\release\aexcompat-harness.exe --render-experimental-smart-16 `
    <plugin.aex> <input.png> <output.png>
```

Constraints (fail-closed, enforced by the broker before dispatch):

- The dump directory must resolve under `<repository>/target/`; traversal
  components and locations outside the target tree are refused.
- The directory must start empty, so stale snapshots can never be mistaken
  for the current run's output.
- The worker writes at most 32 snapshots and 1 GiB in total per run; anything
  beyond is counted in `world_dumps.skipped`, not silently dropped.

## Snapshot files

Named `NNN-<stage>-<width>x<height>.<format>`, where `NNN` is the write
order and `<format>` is `rgba8`, `rgba16le`, or `rgba32f-le` (RGBA order,
row-major, no stride padding, little-endian). Stages:

- `classic-input` / `classic-output`: the packed input world immediately
  before the Classic RENDER dispatch, and the output world after it.
- `smart-input`: the packed input world immediately before SmartFX
  Pre-Render (the same buffer stays pinned through Smart Render).
- `smart-output`: the output world after Smart Render (also written on the
  NOP-render path).
- `classic-layer-slot<N>` / `smart-layer-slot<N>`: each hosted secondary
  layer world offered for checkout, at its built state.

Output-stage snapshots are written even when the render errors, so partial
output is inspectable. The formats match `tools/compare-pixel-oracles.py`:

```powershell
python tools/compare-pixel-oracles.py --raw target/world-dumps/001-smart-output-1920x1080.rgba16le `
    --render <reference.png> --width 1920 --height 1080 `
    --raw-format rgba16le --raw-integer-max 32768
```

(Worker worlds carry AE-range 16-bpc samples, white = 32768.)

## Report fields

With dumps enabled, the render report gains:

```json
"world_dumps": {"directory": "target/world-dumps", "written": 2,
                 "skipped": 0, "bytes": 33177600}
```

With `AEXCOMPAT_CHECKSUM_DETAIL`, it also gains `output_row_crc32` (one
CRC-32 per output row, computed over the RGBA-ordered transport bytes, so
they recompute directly from the raw sidecar) and `output_channel_sha256`
(SHA-256 per channel plane, R/G/B/A). Raw pixel contents never enter the
report; only counts, CRCs, and digests do.

## Notes

- On the SmartFX GPU-to-CPU fallback retry, the broker deletes the failed
  attempt's snapshot files (only names matching the owned pattern) before
  re-dispatching, so the directory reflects the run the report describes.
- The dump directory is passed to the worker as the `--dump-worlds-v1` argv
  trailer; `--output-checksum-detail-v1 1` enables the checksums. Neither
  trailer is accepted twice.
