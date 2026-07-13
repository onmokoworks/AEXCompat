# ScatterMap L2 Result (2026-07-13)

Status: **passed; rendering remains closed**.

Receipt `scattermap-l2-20260713-001` authorized the isolated selector sequence.
The broker revalidated SHA-256
`223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB`
and ran a disposable cleanroom worker under the Job Object boundary.

Observed selector results:

- `GLOBAL_SETUP`: error 0;
- `PARAMS_SETUP`: error 0;
- `GLOBAL_SETDOWN`: error 0;
- `ABOUT`: error 0 with deterministic ScatterMap v1.0 message;
- `SEQUENCE_SETUP`, `SEQUENCE_RESETUP`, `FRAME_SETUP`, `FRAME_SETDOWN`, and
  `SEQUENCE_SETDOWN`: error 0 in host order;
- sequence and frame data remained null, matching the stateless implementation;
- reported parameter count: 8 (implicit input plus 7 registered parameters);
- `out_flags`: 33554432;
- `out_flags2`: 167777280;
- `PF_OutFlag_SEND_UPDATE_PARAMS_UI`: not advertised;
- `PF_OutFlag2_SUPPORTS_QUERY_DYNAMIC_FLAGS`: not advertised;
- conditional `UPDATE_PARAMS_UI` and `QUERY_DYNAMIC_FLAGS` dispatch: correctly omitted;
- render performed: false;
- worker exit classification: `ok`.

Registered parameters, in order:

1. `Scatter Amount`, type 1;
2. `Direction`, type 7;
3. `Random Seed`, type 1;
4. `Repeat Edge Pixels`, type 4;
5. `Mix with Original`, type 10;
6. `Scatter Map`, type 0;
7. `Invert Map`, type 4.

The first attempt exposed two real host requirements rather than masking them:
worker crashes with absent PICA/handle services were contained as access
violations, and the broker was hardened to produce a report for empty or
crashed worker output. The passing host supplies only PICA `PF Handle Suite`
version 2; all other suite acquisition is default-deny.

Descriptor value decoding was subsequently re-run under the same receipt and
matched the self-authored source oracle:

- `Scatter Amount`: valid 0..500, slider 0..100, default 5;
- `Direction`: `Horizontal|Vertical|Both`, default 3 (`Both`);
- `Random Seed`: valid/slider 0..10000, default 0;
- `Repeat Edge Pixels`: default true, label `Repeat`, but raw current value false;
- `Mix with Original`: 0..100, default 100, precision 1;
- `Scatter Map`: layer parameter;
- `Invert Map`: default false, label `Invert`.

The Repeat Edge descriptor has `current=0`, `default=1`, while Invert Map has
`current=0`, `default=0`. Adobe SDK `PF_ADD_CHECKBOX` initializes both fields
to the requested default, but the Rust wrapper used by this fixture writes only
`dephault`. AE 25.2 consequently reports all eight low-level parameters while
omitting Repeat Edge from ExtendScript enumeration and AEPX parameter
templates. This mismatch is now retained as target behavior rather than
normalized away by the cleanroom host.

This result proves initialization, parameter registration, and descriptor
defaults/ranges for the observed types. The lifecycle extension also proves the
stateless sequence/frame contract in two independent isolated runs. Rendering
evidence is recorded separately.
