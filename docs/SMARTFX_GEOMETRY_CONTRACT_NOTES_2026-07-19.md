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

## 2026-07-19 訂正: dispatch gate と empty extent (PR #86 Codex P2 ×2)

観察 (Codex review findings):
1. 空 result の report が output world の full-frame extent_hint をそのまま
   出しており、約束していない領域を主張していた。また broker の conformance
   経路 (`conformance.rs` normalize_success) は width==0 を `invalid_output`
   に分類するため、worker 側で合法化した空 result が bundle 側では不正扱いの
   まま。
2. pre-render 成功 + rects_valid=false (例: ±2^24 超の座標) でも render
   selector が stale な full-frame output world に dispatch されていた
   (従来からの挙動だが、検証強化後は明示 fail にすべき)。

訂正:
1. 空 result の `output_extent_hint` は `{0,0,0,0}` を報告する。conformance
   bundle 側の空 result 分類は schema (`classification` enum / world 非 null
   要求)・validator・進行中の issue #4 系列に跨る契約変更のため本 PR の
   scope 外とし、issue #88 として起票。
2. dispatch 条件に `result.rects_valid` を追加。invalid geometry は selector
   を dispatch せず `render_error=-6` で明示的に失敗する (空 result の合法
   スキップは別分岐で維持)。

## 2026-07-19 訂正: dispatch 判定の単一述語化 (PR #86 Codex P2 round 2)

観察 (Codex review findings): (1) `gpu_render_dispatched` が empty しか除外
しておらず、rects_valid=false の skip 時にも GPU transport を prepare/finish
して `gpu_render_dispatched=true` を報告していた。(2) report の
`smart_render_selector_dispatched` が NOP 判定のみで駆動され、empty skip や
invalid-geometry 拒否でも true になっていた。

訂正: `will_dispatch = pre_error==0 && rects_valid && !empty_result_rect` の
単一述語で selector 呼び出し・GPU transport・報告を駆動する。SmartResult に
`selector_dispatched` (実際に entry を呼んだ時のみ true) を追加し、report は
それを出す。既存 frozen evidence への影響は NOP case の false のみで不変。

## 2026-07-19 訂正: conformance に empty_result 分類を実装 (PR #86 Codex 再指摘 → issue #88)

観察 (Codex review round 3): worker 側で合法化した空 result を broker の
conformance 経路が `invalid_output` に分類したままでは end-to-end で成立
しない、との再指摘。issue #88 への deferral では通らないため #88 を claim
して同 PR で実装。

実装: `Classification::EmptyResult` ("empty_result") を追加。
`normalize_success` は smartfx report の `empty_result_rect==true` かつ
width==height==0 のとき input world の認証後に EmptyResult
(world: null, selector completed error 0, 空バイトの output_sha256) を返す。
非ゼロ寸法 + empty marker の不整合は invalid_output のまま、classic 経路に
empty 許容は無い。schema には classification enum への追加と
empty_result 用の allOf 制約 (world/raw_output null, smartfx, error 0) を追加。
validator (`tools/conformance_bundle_validator.py`) は classification != "ok"
を pixel 検証スキップとして扱うため変更不要。

追記 (Codex round 4 P1): 実 dispatch (`image_render.rs`
`render_experimental_image_at_time_with_format` 系) は worker report の
width/height を `validate_image_buffer_layout` (0 拒否) に通し、broker report
の転記フィールドに `empty_result_rect` が無かったため、実経路では空 result
が conformance の EmptyResult 分岐に到達できなかった。訂正: smartfx report が
`empty_result_rect==true` の場合は寸法 0 / raw 0 byte を検証した上で pixel
経路 (buffer 検証・raw 読出し・PNG 出力) をスキップし、`output_png: null` と
geometry フィールド群 (`empty_result_rect` / `returns_extra_pixels` /
`result_within_request` / `extra_pixels_contract_violation` /
`smart_render_selector_dispatched` / `input_checkout_result_rect`) を broker
report に転記する。非ゼロ寸法や非 0 byte 出力を伴う empty 主張は fail-closed。

## 2026-07-19 PR3 implementation record (geometry probe + 実 AEX 診断)

観察/実装 (branch `issue8-smartfx-geometry-probe-v2`, PR #86 merge 後の main 起点):

- 新規 SDK ビルド probe `instruments/pf-smart-geometry-probe`。mode は host が
  汎用に渡す render time (`current_time % 4`) で選択し、host 側に probe 固有
  分岐は無い:
  - mode 0: probe 自身の Smart Pre-Render 内で checkout 交差契約を検証
    (full/sub-rect/oversized/disjoint の各 request への応答を assert;
    pre_render_error==0 が全チェック通過を意味する)
  - mode 1: `RETURNS_EXTRA_PIXELS` + result > request (violation なし)
  - mode 2: flag なしの同じ超過 → `extra_pixels_contract_violation`
  - mode 3: 空 result → selector スキップ
- `tests/test_pf_smart_geometry_probe.py` (built-artifact gate): 4 mode の
  期待値表 + ARGB8/16/32F で geometry フィールドが同一であることを検証
  (issue #8 の「深度間同一契約」完了条件の behavioral 検証)。
- `tools/refresh-smartfx-geometry-evidence.ps1` が probe 12 run + 独立実 AEX
  3 run を実行し `analysis/SMARTFX_GEOMETRY_CONTRACT_RESULT_2026-07-19.json`
  を生成。検証テストは local-artifact gate
  (`tests/test_smartfx_geometry_contract_result.py`)。
- 観察 (ntsc-rs-ae 0.9.4, 64x48 full-frame, 3 深度): checkout request は
  full-frame をそのまま転送し交差応答も full-frame、result ==
  max_result == full、**`RETURNS_EXTRA_PIXELS` を宣言している**、violation
  なし、3 深度で geometry 完全一致。専用分岐なしで契約経路を通過した。

## AE 実機観測: output world sizing 基準 (issue #102、2026-07-19 追記)

保留していた「AE が output world を result_rect / max_result_rect の
どちらで sizing するか」を実機で観測した。

- 手段: `instruments/pf-selector-timeline-probe` (smart flavor) の
  Probe Mode 1。Smart Pre-Render で `result_rect = [8,4,632,356]`
  (inset)、`max_result_rect = [0,0,640,360]` (full) を返答し、
  SMART_RENDER で `checkout_output` が返す world を記録する。
  `tools/capture-selector-timeline.ps1` + aerender、AE 25.3.1x3、8bpc。
- **観察: AE の output world は 624x352、origin_x=8, origin_y=4、
  rowbytes=2496 (=624*4)。すなわち result_rect の寸法で確保され、origin が
  result_rect の左上を指す。max_result_rect (640x360) 基準ではない。**
- 観測は 8bpc・単一ケースの結果で、深度依存の可能性には未観測の留保を
  残す (ただし depth 依存にする理由は AE 側に見当たらない)。

含意: host (`minihost` smart pipeline) の現行 `max_result_rect` 基準の
output world 確保は AE と異なる。result_rect ≠ max_result_rect を返す
plugin では、AE 上と host 上で output world の寸法・origin が食い違う。
sizing の追随変更 (result_rect 基準 + origin 設定) は issue #102 の
残作業として、SmartFX geometry evidence の refresh と併せて別 PR で行う。
