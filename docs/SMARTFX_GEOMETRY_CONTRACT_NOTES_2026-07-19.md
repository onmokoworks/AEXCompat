# SmartFX Geometry Contract Notes (issue #8)

Time-ordered working log for generalizing the Smart Pre-Render / Smart Render
geometry semantics. Observations (facts read from code or SDK headers) are
marked **観察**; design decisions and inferences are marked **仮説/方針**.
Later entries may correct earlier ones; corrections are appended, never
rewritten.

## 2026-07-19 Initial survey

### 観察: SDK contract (ae25.2 SDK, `Examples/Headers/AE_Effect.h`)

- `PF_RenderRequest` = `{ PF_LRect rect; PF_Field field; PF_ChannelMask
  channel_mask; PF_Boolean preserve_rgb_of_zero_alpha; char unused[3];
  A_long reserved[4]; }` (AE_Effect.h:2374). The rect is the first 16 bytes.
- `PF_PreRenderOutput.flags` is a `short` at offset 34;
  `PF_RenderOutputFlag_RETURNS_EXTRA_PIXELS = 0x1` with the comment "if it's
  just as cheap to compute more pixels at once, set this to allow result >
  request rect" (AE_Effect.h:2395). `GPU_RENDER_POSSIBLE = 0x2` is already
  consumed by the host (l2_main.cpp:20146).
- `PF_PreRenderOutput.result_rect` is "the rectangle actually available from
  this request (can be empty)"; `max_result_rect` "must not vary depending on
  requested output size" (AE_Effect.h:2402-2404).
- `PF_CheckoutResult.result_rect` has the same "actually available from this
  request (can be empty)" wording (AE_Effect.h:2416); `max_result_rect` is the
  maximum obtainable; `par`, `ref_width/ref_height` follow.
- SDK sample `SmartyPants.cpp:334-397` copies `extra->input->output_request`
  into a local `PF_RenderRequest`, optionally widens `channel_mask`, checks the
  layer out with that request, then **unions the returned
  `PF_CheckoutResult.result_rect` / `max_result_rect` into its own
  `PF_PreRenderOutput` rects**. This is the canonical plug-in pattern: the
  host's checkout answer directly becomes the plug-in's declared geometry.

### 観察: current host behavior (`minihost/src/l2_main.cpp`)

- `pre_checkout_layer` (l2_main.cpp:1585) records the raw request rect into
  `g_input_checkout_request` / `g_map_checkout_request` but always answers
  full-layer availability: `write_checkout_result` (l2_main.cpp:1537) writes
  `result_rect = max_result_rect = [0,0,width,height]`. The request never
  shapes the answer.
- `smart_checkout_pixels` (l2_main.cpp:1696) returns the full-layer world
  regardless of the checkout request.
- `smart_render_once` (l2_main.cpp:19716) reads plug-in `result_rect` /
  `max_result_rect` back from `PF_PreRenderOutput` (20109-20111), validates
  them (`valid_rect` 20101-20108: inverted rects rejected, dimension/area
  bounds, `result ⊆ max_result`), then sizes the output world from
  `max_result_rect` (20117-20141) and sets the origin words at input+276/280
  to `-max_result_rect[0/1]`.
- `RETURNS_EXTRA_PIXELS` (pre_output flags bit 0x1) is read nowhere in the
  repository.
- The smart JSON report emits `result_rect` / `max_result_rect` twice
  (l2_main.cpp:24669 and 24706) — duplicate JSON keys; most parsers keep the
  last. Out of scope here; to be filed as its own issue.
- The report's `input_world.extent_hint` / `output_world.extent_hint` are
  hardcoded to the full world (24677-24682), not read back.
- `analysis/SCATTERMAP_SMARTFX_ROI_RESULT_2026-07-13.md` freezes the
  `partial_output_request` case: the fixture received the partial request,
  intersected **its own** full-world availability with it, and returned the
  partial rect. The doc explicitly does not claim the host supplied cropped
  pixels.

### 仮説/方針: host semantics to implement

1. **Checkout answer = intersection.** `PF_CheckoutResult.result_rect` should
   be `request.rect ∩ [0,0,layer_w,layer_h]` (empty allowed);
   `max_result_rect` stays the full layer extent. This is the only reading
   consistent with "actually available from this request" + "can be empty",
   and with the SmartyPants union pattern. A host that always answers
   full-frame silently erases every downstream geometry contraction.
2. **Pixel checkout worlds keep layer origin.** `PF_EffectWorld` has no origin
   field, so a world whose (0,0) is not the layer origin would be
   uninterpretable by the plug-in; the world handed out by
   `checkout_layer_pixels` keeps full-layer dimensions (world coordinates ==
   layer coordinates) and communicates the valid sub-region via
   `extent_hint = checkout result_rect` on a per-checkout view block, not by
   physically cropping pixels. Physical cropping would also change the frozen
   ScatterMap oracle SHA, and the frozen evidence explicitly leaves "cropped
   world" behavior unproven. If later AE observation (Direction 4) shows AE
   physically crops, that becomes a follow-up correction entry here.
3. **Fail-closed request validation.** A malformed request rect (inverted
   edges) is rejected with error 4 plus a diagnostic counter instead of being
   silently accepted. An empty intersection is a *legal* answer at checkout
   time (empty `result_rect`), but a subsequent `checkout_layer_pixels` on
   that checkout is refused (error 4 + counter) — pixels that were never
   promised are never handed out.
4. **RETURNS_EXTRA_PIXELS (PR2).** Read pre_output flags bit 0x1. Without the
   flag, `result_rect ⊄ request.rect` is a contract violation surfaced as a
   diagnostic; with the flag, `result > request` is admitted (still bounded by
   `max_result_rect` and the existing size caps). Output world sizing moves
   from `max_result_rect` to `result_rect` only after probe/oracle evidence
   pins which one AE uses; until then sizing stays as-is and the flag is
   recorded, validated, and reported.
5. **Depth invariance.** The geometry path is already depth-independent
   (single code path for ARGB8/16/32F); the work is to add tests asserting it
   stays that way, not to change code.

### 方針: PR split

- **PR1** (this branch): checkout request → intersection answer, per-checkout
  view worlds with `extent_hint`, malformed-request/empty-checkout fail-closed
  counters, JSON report fields, native self-test cases, focused pytest.
  Fixture oracles must stay byte-identical (full-frame requests intersect to
  full frame; ScatterMap partial case returns the same rects it already
  returned).
- **PR2**: RETURNS_EXTRA_PIXELS validation + result/request containment
  diagnostics + `valid_rect` absolute-coordinate hardening + extent_hint
  readback in the report + empty-result_rect legal-skip semantics.
- **PR3**: new SDK-built geometry probe (sub-rect checkout, extra-pixels,
  empty-result) + ntsc-rs (or another independent SmartFX AEX) geometry
  diagnostics + ARGB8/16/32F same-contract tests.

## 2026-07-19 PR1 implementation record

観察/実装 (branch `issue8-smartfx-checkout-geometry`):

- `write_checkout_result` was replaced by `write_checkout_result_rects`, which
  takes explicit result / max_result rects; all three pre-checkout success
  paths (input, secondary map, hosted layers) now answer
  `result_rect = request ∩ layer extent`, `max_result_rect = full layer
  extent`. A null request pointer still means "everything".
- Malformed requests are defined as inverted rects only (`right < left` or
  `bottom < top`) and are rejected with error 4 plus
  `g_malformed_checkout_requests`. Huge or fully off-layer rects are *not*
  malformed: plug-ins legitimately pass "give me everything" rects, and the
  intersection clamps them. A disjoint or degenerate request intersects to
  the canonical empty rect `{0,0,0,0}`.
- Each checkout target now has a paired "view world": a copy of the base
  120-byte world block whose `extent_hint` (offset 44) is rewritten to the
  intersected answer at checkout time. `checkout_layer_pixels` hands out the
  view instead of the shared base world, so checkout state never leaks into
  the parameter worlds. Pixels are not physically cropped (see the 方針 entry
  above: world coordinates stay layer coordinates; PF_EffectWorld has no
  origin field, and the frozen ScatterMap evidence leaves host-side cropping
  unproven).
- A checkout whose answer was empty refuses `checkout_layer_pixels` with
  error 4 plus `g_empty_checkout_pixel_denials` — pixels never promised are
  never handed out. The `{-1,-1,-1,-1}` no-checkout sentinel preserves the
  historic permissive full-world answer for render-only dispatch paths.
- The smart JSON report gains `input_checkout_result_rect`,
  `map_checkout_result_rect`, `malformed_checkout_request_count`, and
  `empty_checkout_pixel_denial_count`.
- New native self-test `--self-test-pf-checkout-intersection`
  (`verify_checkout_request_intersection_contract`): null/sub-rect/oversized/
  disjoint/degenerate/inverted request cases on the input path, a hosted-layer
  intersection case against the layer's own extent, and the empty-answer
  pixel-denial + sentinel cases. Wired into
  `tests/test_smart_checkout_intersection.py` (source markers + all three
  workers) and `tests/built_artifact_tests.txt`.
- 仮説: fixture oracles stay byte-identical because full-frame requests
  intersect to the full frame and ScatterMap's partial case already returned
  the partial rect itself; the view world only changes `extent_hint`, which
  the fixtures are not known to read. To be confirmed by the broker gates
  before merge; if a frozen SHA shifts, that is evidence the fixture reads
  `extent_hint` and belongs in this log as a correction.
