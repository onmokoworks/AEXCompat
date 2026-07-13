# ScatterMap Threaded Render Result

ScatterMap advertises `PF_OutFlag2_SUPPORTS_THREADED_RENDERING` (bit 27) in
GLOBAL_SETUP. Sequential fresh-process determinism was already covered, but
that does not prove the advertised same-process concurrency contract.

The fixed-hash classic worker now loads and initializes the AEX once, then
starts two native threads simultaneously. Each thread owns separate input,
output, parameter, padding, and guard storage while both share the loaded
module and plug-in global data, matching the relevant AE threaded-render
contract. The harness map-availability flag is atomic so harness state does not
introduce a data race.

The broker repeated the complete worker run twice in fresh Job Object-isolated
processes. All four renders returned error 0, preserved their individual guard
regions, and produced ARGB SHA-256
`19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9`.
Both outer runs were classified `ok`; the broker reported
`threaded_render_valid: true`, `deterministic: true`, `oracle_match: true`,
`broker_survived: true`, and `passed: true`.

The route remains limited to the approved owner-authored fixture, fixed default
case, fixed SHA-256 allowlist, five-second timeout, disposable worker, and
create-new result path. It does not expose arbitrary thread counts, plug-in
paths, parameters, or output destinations.
