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

## 2026-07-19 訂正: GPU mode では view を返さない (PR #83 Codex P1)

観察 (Codex review finding, PR #83): GPU Smart Render では
`prepare_cuda_render_transport` が **base の** input/output world の data
pointer を device 割り当てに書き換え、`is_active_gpu_world` は base
input/output world と GPU-created world しか受理しない。初版実装のように GPU
mode でも checkout view を返すと、plug-in は stale な host pointer を持つ
非 active world を受け取り、GPU Device Suite 呼び出しが失敗する。

訂正: `smart_checkout_pixels` は `g_gpu_world_mode` の間は view を返さず
base world を返す (従来動作)。view の GpuBgra128 登録も撤回。空応答の
pixel denial は GPU mode でも維持。extent_hint による checkout 答えの伝達は
CPU 経路の契約とし、GPU 経路の geometry 伝達は PR2 以降で transport の
promote と併せて扱う。

## 2026-07-19 訂正: view は dispatch world として登録しない (PR #83 Codex P2)

観察 (Codex review finding): `resolve_dispatch_world_format` の copied-struct
fallback は (data, rowbytes, width, height) の内容一致で登録済み world を探し、
**別 world pointer で2件一致すると ambiguous として error 4 を返す**
(l2_main.cpp の fallback ループ、`unique->world != entry.world` 判定)。view は
base とまったく同じ data/寸法を持つため、view を別エントリとして登録すると、
効果が world struct を値コピーして suite (Pixel Data / World Transform) に渡す
既存の正当な経路が全て ambiguous 化して壊れる。

訂正: view (input/map/hosted とも) は登録しない。未登録の view が suite に
渡された場合は exact-pointer lookup を外れて content fallback に落ち、base
エントリに一意解決される — これは copied-struct fallback の設計どおりの経路
であり、view はまさに base のコピーなので正しい形式に解決される。

## 2026-07-19 訂正: view 抑止は「GPU render が実際に dispatch された間」のみ (PR #83 Codex P2 round 3)

観察 (Codex review finding): 最初の GPU 訂正で gate に使った
`g_gpu_world_mode` は「GPU negotiation が有効」を意味し、plug-in が
`GPU_RENDER_POSSIBLE` を立てなかった場合は negotiation 有効のまま CPU の
`kSmartRender` が dispatch される。このとき CUDA transport は走らず base
world は promote されないのに、view が抑止されて交差契約が失われていた。

訂正: 新フラグ `g_smart_gpu_render_dispatched` (kSmartRenderGpu dispatch の
間のみ true) で gate する。GPU negotiation からの CPU fallback では view と
交差契約が維持される。self-test も新フラグでの gating を検証するよう更新。

## 2026-07-19 PR2 implementation record (RETURNS_EXTRA_PIXELS / rect 検証強化)

観察/実装 (branch `issue8-smartfx-extra-pixels`, PR1 merge 後の main 起点):

- `RETURNS_EXTRA_PIXELS` (pre_output flags @34 bit 0x1) を読み取り、
  `result_rect ⊆ output_request.rect` の包含を検証。flag なしで超過した場合は
  `extra_pixels_contract_violation` として **診断のみ** (render 失敗にはしない)。
  理由: AE は黙って clip する挙動であり、ここで hard fail にすると実 AEX の
  観測 (Project Direction 1) が止まる。broker 側 gate での強制は fixture 側の
  証拠が揃ってから判断する。
- 空 `result_rect` は SDK の "can be empty" どおり合法扱いに変更: render
  selector を dispatch せず `empty_result_rect: true` を報告し、0 byte 出力を
  正常 (output_pixels_valid=true) とする。従来は rects_valid=false でも
  pre_error==0 なら render が走っていた (max_result が空のときのみ暗黙に
  失敗)。
- rect 検証を inline lambda から `smart_geometry_rect_valid` に切り出し、
  絶対座標の上限 `kMaxSmartRectMagnitude = 1<<24` を追加 (負座標そのものは
  buffer expansion で合法なので下限も −2^24)。既存の 4096 辺長・面積上限は
  維持。
- output world の `extent_hint` を render 後に読み戻して report の
  `output_world.extent_hint` に反映 (従来はフルフレームをハードコード)。
  plug-in が extent_hint を設定した場合のみ値が変わる。
- 新 self-test `--self-test-pf-smart-geometry-rects` で
  `smart_geometry_rect_valid` / `smart_rect_contained` の純関数を検証。
- 未実施 (PR3 送り): output world sizing の result_rect 基準への変更は
  probe/oracle 証拠が出るまで保留 (max_result_rect 基準を維持)。probe で
  AE 実挙動を観測してから確定する。
