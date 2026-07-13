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
- reported parameter count: 8 (implicit input plus 7 registered parameters);
- `out_flags`: 33554432;
- `out_flags2`: 167777280;
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

This result proves initialization and parameter registration only. Parameter
union defaults/ranges and image rendering remain future compatibility work.

