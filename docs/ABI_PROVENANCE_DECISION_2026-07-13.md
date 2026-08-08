# ABI Provenance Decision (2026-07-13)

Decision owner: repository owner, via direct Codex task response.

Decision: **public-document cleanroom for `minihost/`**.

The owner prefers cleanroom implementation and permits SDK use only where its
terms allow. AEXCompat therefore adopts the following hard boundary:

1. `minihost/` ABI definitions and behavior are derived from public
   documentation, independently authored contracts, synthetic fixtures, and
   black-box traces captured by human-operated instruments.
2. Adobe SDK headers and source may be read only by the instrument/build side
   under `instruments/`; they may not be copied, included, translated, or used
   as source material in `minihost/`.
3. Instrument output may cross the boundary only after A-5 redaction and A-4
   schema validation. It contains observed behavior and numeric metadata, not
   SDK source, header text, private paths, pointers, or payloads.
4. SDK-dependent plug-ins may be distributed only as permitted object code.
   SDK headers/source are never committed or redistributed by this repository.
5. Any future proposal to use SDK-derived ABI declarations in `minihost/`
   requires a new dated human/legal decision and cannot silently weaken this
   boundary.

This satisfies the H-3 provenance choice but does not open the Safety Gate.
The existing native-code guard continues to enforce the directory boundary.

## Observed Layout Facts

`instruments/abi-layout-probe` compiles against the external SDK and emits only
numeric ABI observations (`sizeof`, `offsetof`, and selector values). The
2026-07-13 x86_64 Windows observation is recorded in
`analysis/AE_ABI_LAYOUT_OBSERVATION_2026-07-13.json`. No header text, field
declaration, macro body, or Adobe source is copied into `minihost/`.

The cleanroom implementation may consume these numeric interoperability facts
as assertions. Any declaration needed by `minihost/` must still be independently
written from public documentation and validated against the observation.

The AEGP Stream Suite v11 implementation follows the same boundary. The
external SDK installation is used for instrument-side numeric/order auditing;
`minihost/` neither includes nor copies Adobe headers. Its 23-entry table and
local value/handle declarations are independently authored and guarded by
local size/count assertions plus native conformance tests.

AEGP Keyframe Suite v5 follows that boundary as well. Its 22-entry local table,
8-byte rational-time declaration, keyframe metadata, and transaction handles
are independently written. External SDK material remains outside `minihost/`;
only audited numeric/order facts and conformance observations cross the
instrument boundary.

AEGP Effect Suite v2/v3/v4 follows the same boundary. External SDK headers are
used only by a temporary conformance translation unit to audit the
`AEGP_ApplyEffect` signature, slot 9 / x64 offset 72, and table sizes
136/136/176 bytes. The 17/17/22-entry tables, instance registry, and opaque
lease representation in `minihost` are independently authored and do not
include or redistribute Adobe headers.

The same compiled probe pins SetEffectFlags, ReorderEffect,
DeleteLayerEffect, and DuplicateEffect at slots 5/6/10/16 (x64 offsets
40/48/80/128). Stream Suite v2 is independently represented as the SDK-sized
22-entry/176-byte table acquired at version 7; its legacy name buffer and
StreamValue signatures are compiled against the external SDK without importing
Adobe declarations into `minihost`.

AEGP Dynamic Stream Suite v4 uses the same cleanroom rule for its 26-entry
table, grouping/flag values, match-name buffers, and UTF-16 naming boundary.
The local property-tree and StreamValue declarations are independently
authored; no Adobe header is included or redistributed by `minihost/`.

AEGP Memory Suite v1 follows the same boundary for its eight-entry callback
table, 32-bit size/flag values, opaque handles, and UTF-16 payload ownership.
All storage and synchronization are independently implemented in `minihost/`.

The `in_data->utils` ANSI block crossed the same boundary twice. The 2026-07-13
observation carried eleven of its nineteen entries, so the host wired eleven and
left the rest null; issue #981 added the remaining eight offsets (`atan`,
`atan2`, `exp`, `floor`, `fmod`, `log`, `log10`, `tan`) by extending
`instruments/abi-layout-probe`, rebuilding it against the external SDK, and
folding the numbers it printed into the observation. The probe's full output was
diffed against the committed document first, so the only differences it
introduced are those eight entries. The functions themselves were already
independently written in `minihost/src/worker_pf_ansi_runtime.cpp`; nothing but
numeric offsets crossed.

That rerun is now the document's standing contract rather than a one-off:
`tests/test_abi_layout_observation_matches_probe.py` runs the compiled probe and
requires every value it prints to equal the committed observation, and CI builds
the probe on any run that provisioned the SDK. The document still carries two
things the probe does not print - `utils.app`, a legacy slot at
PF_UtilCallbacks+0xC8 that no SDK header declares (issue #362), and this
provenance note - and the test names them as the only permitted additions. What
neither covers is a PF_UtilCallbacks member nothing has taught the probe to
emit; issue #991 tracks the ones that are still outside it.
