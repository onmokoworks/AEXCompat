# X05: live macOS custom-control canvas

This standalone AppKit laboratory drives the original Windows AEX through a
resident Unicorn `ui-session`. It is deliberately separate from the image/chain
viewer: it does not pretend that edits here already update a render session.
A person can click, drag and release inside the window and see the guest's
Drawbot calls rasterized. ASCII/editing keydown is dispatched too; whether a
plugin handles a key is observed, never assumed.

## Run

From a space-free AEXCompat checkout, with X05's guest worker built:

```sh
./tools/mac-custom-ui/build.sh
"$HOME/.cache/olm-emu/build-X05/MacCustomUI.app/Contents/MacOS/mac-custom-ui" \
  /absolute/path/to/aex-guest-worker /absolute/path/to/OLMKiraKira.aex \
  tools/mac-custom-ui/kirakira.json /absolute/path/to/NEW-capture-directory
```

The directory must not already exist. Close the window to complete teardown and
save `capture.json`, `session.json`, raw `trace.jsonl`, `worker.log`, `first.png`
and `last.png`. `session.json` is an ordinary U03 batch request, not a new replay
schema. Images are diagnostic AppKit previews, not AE pixel or font oracles.
The template explicitly supplies slot, frame, context, theme and picker result;
it is not autodiscovery. Frame coordinates use logical points, independent of
Retina scaling. The included template selects KiraKira's slot 14.

The first NSView draw requests a guest DRAW. Input callbacks enqueue click,
drag, final-drag (mouse-up) or keydown, then request another DRAW. AppKit exposure
repaints the last raster without mutating guest state. Drag starts only if the
guest's click returns `send_drag`; it survives intervening DRAW callbacks.
Pending consecutive non-final mouse moves may be coalesced. Clicks, final drags,
keys and already executing operations are retained. The session closes cleanly
at the existing 256-operation bound, possibly in the middle of a gesture; start
a new process/session to continue from defaults. There is no unbounded history
or implicit transfer of state between sessions.

ASCII printable keys use PF's uppercase unshifted UTF16 plus `0x80000000` flag;
space/backspace/tab/return/enter/escape use PF control codes. Modifiers map to
PF's command/shift/caps-lock/option/Mac-control bits. IME, non-ASCII and function
keys are not implemented. Mouse modifiers and cursor negotiation remain open.
The configured color picker result remains injected (null means cancel); no
interactive OS picker is claimed. Unsupported drawing calls fail explicitly.

For repeatable event-path checks append `insert`, `delete` or `presets` to the
command. These create NSEvents and pass them to `NSWindow.sendEvent`, exercising
the same responder, coordinate conversion, queue, guest and rasterizer as real
input. Metadata labels these as injection. They are not human-operated evidence.
Desktop input through the computer-use tool was also exercised separately.

## Additive worker transport

`ui-session PLUGIN HEADER.json --stream` first runs the header's existing
operations, then reads one ordinary operation object per newline on stdin.
Each operation emits and flushes the same JSON row as batch mode; it does not
wait for stdin EOF. EOF performs normal guest teardown. Nonzero process exit
invalidates the capture even if complete stdout rows exist. Total operations,
including initial ones, remain limited to 256; each input line is at most 1 MiB
and must end in a newline. No header or recorded U03 fixture was changed.

EVENT 10 adds optional `key_code` and `modifiers`. A key code is mandatory for
10; key fields on other events are rejected. Absent fields remain absent in
legacy serialized output. Offsets match the native keydown dispatcher (20 x,
24 y, 28 code, 32 modifiers); a real emulated selector test reads those bytes.
Event operations explicitly re-enter the JSON deserializer for numeric theme
map keys, avoiding Serde's internally tagged enum content-map limitation.

The canvas uses one guest process on a serial queue. Child-only CPU (120 seconds),
file (64 MiB), descriptor (64) and core (0) limits are installed before exec;
resident memory is sampled every 100 ms with a 1 GiB limit. Process groups are
terminated on failure/close, and interactive exchanges have a 30-second deadline
plus a 5-second teardown deadline. Responses are bounded to 32 MiB, trace output
to 128 MiB, and queued input to 128 operations. These are containment bounds,
not approval or hash gates, and are not a confidentiality sandbox.
Launch hashes stay immutable in capture metadata; loaded plugin hashes come
from actual guest rows. Closing-time file hashes are separate drift observations.

## Drawbot census

FACT: source inventory is `x64/ui_draw.rs`, `x64/ui_session.rs`,
`minihost/src/worker_drawbot_runtime.cpp` and the actual slot wiring in
`worker_host_suite_catalog.cpp` at the X05 base `cb3bb768`. The native runtime
is itself bounded telemetry: it records counters/colors/last rectangle, not a
full reference rasterizer. Its family name being present is not full Drawbot.
The table lists zero-based ABI slots. Classification is INFERENCE, using the
main audit's (a) implementable gap, (b) platform mechanism, (c) intentional contract.

| Family | Native wired subset | Guest recorder / Mac canvas | Gap and class |
|---|---|---|---|
| Custom UI 1; Draw 1 | Drawing reference; GetSupplier/GetSurface 0/1 | Same context-bound references; real AppKit canvas | Multiple-control discovery/integration into main viewer remains (a) |
| Supplier 1 (13 slots) | NewPen 0, NewBrush 1, NewPath 6, Release 12 | Those plus NewDefaultFont 4, Retain 11; pen RGBA/width, brush, font size, persistent path objects | SupportsText 2, default font size 3, image 5, pixel-layout queries 7–10 unsupported in both (a) |
| Surface 2 (17 slots) | PaintRect 2, FillPath 3 (winding only), StrokePath 4; telemetry only | Same plus DrawString 9 and SetAntiAliasPolicy 13; AppKit paints rectangles, filled/stroked paths and text | Push/pop 0/1, clip/query 5–7, transform 8, image 10, interpolation 11/12, AA query 14, flush 15, screen scale 16 unsupported (a) |
| Path 1 (6 slots) | MoveTo/LineTo 0/1 share a point counter; AddRect 3 stores last rectangle | MoveTo 0, LineTo 1, AddRect 3, Close 5; NSBezierPath retains actual geometry | BezierTo 2, AddArc 4 unsupported in both; native Close 5 unsupported (a) |
| Text / font metrics | NewDefaultFont and DrawString not wired; AppGetFontStyle returns error 4 | Guest records exact float bits/UTF16, no metrics query; canvas uses NSFont system metrics, baseline and left/center/right alignment, end/end-ellipsis truncation | AE font selection, metrics, glyph/raster equality, path ellipsis missing (a). AppKit font engine is a platform mechanism (b); preview-only font choice is explicit (c) |
| Antialias / fill | Native only validates winding fill | Guest records policy; canvas even-odd/winding, no-AA vs AppKit AA; medium/high share AppKit AA | Exact AA kernel/quality parity missing (a); no raster-exact claim (c) |
| Pen/Image suites | Not published by this catalog | Not acquired by guest | Dashes/images and corresponding methods remain (a) |
| App/theme/picker/invalidation | Bounded synthetic colors, configured picker, invalidation telemetry | Exact supplied RGB16/ARGB bits; invalidation recorded; canvas redraw after input | Interactive picker, host theme discovery and cursor services remain (a); injected experiment inputs remain intentional (c) |

Drawbot numeric arguments and active arbitrary parameters are compared exactly
using U03's unchanged OLM comparator. AppKit raster output is outside that
semantic contract. CoreGraphics/AppKit versus native Windows API handles are
(b), not a reason to classify implementable missing primitives as platform-only.

## Measured scope and remaining work

FACT: three newly injected AppKit sessions (insert/delete/presets) and one desktop
CUA drag/key session completed with clean worker teardown: 9 + 9 + 13 + 9 = 40
operations. Batch replay matches every active field exactly. Insert changed the
original ramp from 3 to 4 points; delete changed it from 3 to 2; presets changed
active ramp bytes. Keys are present in all four traces; no plugin key-specific
behavior is inferred from a successful dispatch. PNGs were visually inspected
for usable rendering, not compared with tolerance or claimed AE exact.

FACT: the unchanged 17 U03 sessions (174 operations) still have zero semantic
mismatches. The same ten KiraKira sessions have raw inactive-capacity differences.
New captures also retain raw inactive tails; their differences are the known
serializer behavior, not a newly discovered plugin bug or an exactness waiver.
OLM's `tests/ui_oracle/canvas` adds four frozen recordings and a manifest;
`verify_X05_canvas.py` uses the original `tests/ui_oracle/verify.py` comparator.

Still replay/config-only: UPDATE parameter-assignment matrices, switching to
other custom slots/contexts, deterministic RNG seed injection, picker results,
negative contexts and all existing fixture inputs. The canvas has no general
parameter editor, clipboard transfer between independently opened sessions,
undo/project save, render-chain synchronization, AE lifecycle scheduling or full
Drawbot. Randomize/copy/paste buttons within the chosen live control are handled
by the original guest; deterministic randomize acceptance remains the seeded
U03 replay. No Windows/native AE execution was performed for this ticket.

## Checks

```sh
export TMPDIR="$HOME/.cache/olm-emu/build-X05/tmp"
export CARGO_TARGET_DIR="$HOME/.cache/olm-emu/build-X05/target"
export CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_INCREMENTAL=0
mkdir -p "$TMPDIR"
cargo build --release --locked --offline --manifest-path guest/Cargo.toml -p aex-guest-worker -j 1
cargo test --release --locked --offline --manifest-path guest/Cargo.toml -p aex-guest-worker ui_ -j 1 -- --test-threads=1
# 11 UI tests, including native-layout key bytes and legacy JSON preservation.
./tools/mac-custom-ui/build.sh
python3 tools/mac-custom-ui/check_stream.py --worker "$CARGO_TARGET_DIR/release/aex-guest-worker" \
  --plugin /absolute/path/to/OLMKiraKira.aex --config tools/mac-custom-ui/kirakira.json \
  --output /absolute/path/to/NEW-stream-check
# response before EOF; 256 succeeds, 257 fails after 256; malformed/oversize fail.
```

The OLM ticket report supplies exact registered paths, source/binary hashes,
full-suite counts, replay commands, screenshots and review status. No existing
worker entry, main checkout, original plugin, release gate or frozen fixture
was overwritten. No push, PR or claim was posted.
