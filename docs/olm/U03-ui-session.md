# Explicit Unicorn UI sessions

`aex-guest-worker ui-session plugin.aex session.json` runs original setup and
sequence setup, materializes declared parameter defaults, copies arbitrary
values through the original callback, then dispatches explicit UPDATE (14) or
EVENT (15). Rendering does not enter this path. This command uses Unicorn,
not the native carrier and not an Adobe UI scheduling oracle.

A minimal update request is:

```json
{"current_time":7,"time_scale":24,"operations":[{"type":"update","parameters":["Amount@1=2"]}]}
```

Use exact parameter slots. `operations` preserves order and accepts
`{"type":"event","event":{...}}` entries. Alternatively, `updates` is an array
of assignment arrays and `events` an array of event objects; these convenience
arrays execute updates first and cannot be combined with `operations`.
Sessions are bounded to 256 operations. Each event object requires:

- `event`: 2 click, 3 drag, 4 draw, or 9 adjust cursor.
- `slot`, `area` (CONTROL is 2), `frame` ([left, top, right, bottom]).
- `x`, `y` in window coordinates; `clicks`; `last` for final drag.
- `context`: logical context identity; `window_type` (ECW is 2).
- `theme`: role number to three exact unsigned 16-bit RGB components.
- `picker_argb_bits`: null for cancel, or four unsigned float32 bit patterns
  in PF_PixelFloat alpha/red/green/blue order.

Context identity and drag continuation persist through intervening draws.
Dragging without a click in that context or against another slot fails.
The Win64 PF_Point layout is x/h first, y/v second: click x at extra+20,
y at +24; hover x at +16, y at +20. The original OLM survey's y-first claim
and older native minihost construction must not be reused here.

Optional `initial_state_u32` maps RVAs to integer values, with at most sixteen
entries. It permits only readable/writable original-image data, records the
initial bytes and values before/after every operation, and can initialize an
original RNG without replacing its algorithm. This is an explicit experiment
input, not a fixture-dependent patch in the host.

Each completed operation emits one JSON line with loaded-plugin SHA256,
setup metadata, request/time, before/after parameter snapshots, raw arbitrary
FLATTEN bytes, and ordered suite calls. Numeric drawing data is recorded as
float32 bit patterns, strings as exact UTF16 units or byte strings, and
Drawbot objects as stable session IDs. Recorders support the original OLM
paths: ParamUtils, PF Interface, Layer current time, Effect disposal,
Stream values/disposal, DynamicStream HIDDEN, CustomUI drawing references,
Drawbot Supplier/Surface/Path, App colors/picker/invalidation. Unknown suite
acquisition or unsupported calls stop execution; they are not successful
no-ops. App colors have no approximate fallback.

The guest serializes arbitrary values through FLAT_SIZE and FLATTEN; it cannot
infer an arbitrary plug-in's payload schema. KiraKira returns 325 bytes but
initializes only its version, count and active records; inactive capacity can
contain changing stack contents. Raw bytes are retained. OLM's separately
versioned verifier compares the active records exactly and reports raw tail
differences separately. It does not claim full raw-buffer equality.

`ui_lifetimes_pre_teardown` on stderr records outstanding effect/stream/value/
Drawbot handles. These are observations, not automatically repaired leaks.
ColorKeep leaves one stream per UPDATE outstanding in the measured sessions.
The following `ui_session_close` diagnostic describes original PF lifecycle
setdown only. It must not be read as a claim that UI handles were leak-free.
Completed stdout lines do not override a nonzero process/teardown result.

## Validation and build

Run from a space-free checkout; QEMU configure rejects physical paths with
spaces (including a symlink whose target contains a space). Check
`memory_pressure` and proceed only with at least 30% free. Use one build job:

```sh
mkdir -p "$HOME/.cache/olm-emu/build-U03/tmp"
export TMPDIR="$HOME/.cache/olm-emu/build-U03/tmp"
export CARGO_TARGET_DIR="$HOME/.cache/olm-emu/build-U03/target"
export CARGO_BUILD_JOBS=1
cargo build --release --locked --manifest-path guest/Cargo.toml -p aex-guest-worker -j 1
cargo test --release --locked --manifest-path guest/Cargo.toml -p aex-guest-worker ui_ -- --test-threads=1
```

The focused suite has ten tests, including UI scope rejection, invalid slots,
continuation across redraw, high suite-slot isolation, invalidation completeness
and stale Drawbot release. Original witnesses live in the OLM repository's
`tests/ui_oracle`: seventeen sessions, 123 UPDATE and 51 EVENT dispatches.
No original plug-in binaries are distributed in that corpus. Native AE UI
scheduling, font rasterization, undo/redo and project persistence require their
own acceptance work; these traces establish handler behavior and drawing calls.
