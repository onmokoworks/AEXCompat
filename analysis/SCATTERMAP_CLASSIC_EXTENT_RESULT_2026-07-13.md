# ScatterMap Classic Partial Extent Result

The external Adobe SDK ABI instrument measured `PF_InData.extent_hint` at
offset 260 with size 16 in the x64 `PF_InData` layout. SDK headers remain
outside the minihost and no native AEX was loaded by the instrument.

The fixed, owner-authored ScatterMap fixture was then rendered twice in fresh
Job Object-isolated classic workers. Each worker received a 16x12 ARGB8 world
and the partial extent `[left=3, top=2, right=11, bottom=8]` while all default
effect parameters remained fixed.

Both renders returned error 0, preserved all guard bytes, and produced ARGB
SHA-256
`19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9`.
That is the full-world default oracle. The result proves that this target's
classic render path does not restrict processing to `in_data.extent_hint` and
continues to write the complete output world.

The broker retained the fixed fixture hash, approved receipt, five-second
timeout, disposable worker, and create-new result policy. It reported
`deterministic: true`, `oracle_match: true`, `guard_bytes_intact: true`,
`broker_survived: true`, and `passed: true`.

## Production AEGP Follow-up

An SDK-based diagnostic AEGP was built successfully outside the repository and
`tools/ae_scattermap_roi_aegp_probe.jsx` provides a new-project-only launcher.
AE 25.2 did not load a General AEGP from the shared MediaCore location, and the
AE application Plug-ins directory requires administrator write access.
No diagnostic AEX remains installed. Therefore this result does not claim a
production AE cropped-world observation; that requires an explicitly installed
diagnostic AEGP or another supported General AEGP search path.
