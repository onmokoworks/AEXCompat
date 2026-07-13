# ScatterMap SmartFX Partial Output Request Result

The fixed, owner-authored ScatterMap fixture was executed twice in fresh
Job Object-isolated workers through the fixed-hash SmartFX broker route. The
host supplied a partial output request with rectangle `[left=3, top=2,
right=11, bottom=8]` inside the 16x12 world.

Both the source-layer checkout (parameter 0, checkout ID 0) and optional map
checkout (parameter 6, checkout ID 1) received the exact request. ScatterMap's
Smart PreRender intersected the returned full-world availability with that
request and returned the same partial `result_rect` and `max_result_rect`.

Both Smart Render calls completed with error 0, preserved all guard bytes, and
produced ARGB SHA-256
`19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9`.
This is the fixed default oracle, so introducing a partial PreRender request
did not alter the full-world pixel contract used by this harness.

The broker accepted no arbitrary plug-in or rectangle input. It retained the
existing fixed SHA-256 allowlist, five-second timeout, disposable worker, and
create-new result policy. The broker survived both runs and reported
`roi_contract_valid: true`, `deterministic: true`, `oracle_match: true`, and
`passed: true`.

This proves the target's Smart PreRender request forwarding and rectangle
intersection behavior. It does not yet claim that production AE supplied a
physically cropped pixel world during Smart Render; that remains a separate
host-observation case.
