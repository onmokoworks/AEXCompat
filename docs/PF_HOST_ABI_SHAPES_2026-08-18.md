# AE の PF ホスト側 ABI の実体 (PF_Birth / PF_ProgressInfo / PF_World) と非VR 512 の 5 本 (2026-08-18)

issue #1274 / #1275 / #1276 の観測記録。#1212 の per-member fault fingerprint 表で
「PF.dll 内部 fault」「execute@null (呼び出し元未確定)」に分類されていた
Echo / Transform / Spill2 / CannedWarp / Glow の 5 本は、いずれも **host が
`in_data` に渡している値の「形」が AE と違う** ことが原因で、SDK ヘッダが
opaque と書いているもの (`PF_ProgPtr`、`PF_LayerDef.reserved_long4`) を Adobe
同梱 plug-in と PF.dll 自身が具体的な object として deref していた。
観測 (逆アセンブル・cdb・AE 実機) と推論を分けて書く。

## 1. 手順 (再現可能)

- trace: worktree で 3 worker exe を build、`AEXCOMPAT_EXTENDED_DIAG=1` +
  `render_sweep --filter <name> --close-report`
  (`docs/SWEEP_INVESTIGATION_WORKFLOW_2026-08-17.md` §2)。
- 静的: Ghidra `AEXCompat.gpr` に 8 本の .aex と `VideoFrame.dll` を import
  (headless、`tools/ghidra/DumpDecomp.java` / `DumpFromSymbols.java` /
  `DumpVtable.java` / `DumpListing.java`)。PF.dll / BEE.dll は import 済み。
- 動的: `cdb -o` で `render_sweep --filter` を子プロセスごと debug
  (`sxe -c "... " av` で AV 発生時に `r` / `k` / `ub rip`)。
- AE oracle: `tools/capture-ae-reference.ps1`。入力は 256x144 の単色 RGBA
  (32,64,128,255) PNG (sha256 `b90064f380218150b09114abd5c38cb687bb54dea49d985cd84663da73e96a59`)、
  AE 26.3x87、bpc 8、default param、frame 0。effect の matchName は AEX 内の
  `ADBE ...` 文字列から (`ADBE Geometry2` / `ADBE Glo2` / `ADBE Echo` /
  `ADBE Spill2` / `ADBE WRPMESH`)。
- host 出力: `render_sweep --dump-frames <dir>` の raw pixel を AE の PNG と比較。

## 2. 観測

### 2.1 PF.dll の dispatch table は `PF_Birth` が埋める (#1274)

- PF.dll (AE 2026) の export `PF_TransferRect` (0x57bf0) は
  `jmp [DAT_18044da98]` の thunk、`PF_TransformWorld` (0x61140) は
  `(*DAT_18044daa0)(...)` を call する。`DAT_18044da98` /
  `DAT_18044daa0` は `PFp_G_OverFuncs` (0x18044d9b0..) 内の slot。
- これらに値を書く関数は export `PF_Birth` (ordinal 288、0x67d40) だけ
  (`ref:` 検索で参照は各 thunk と `PF_Birth` のみ):
  `DAT_18044da98 = PF_TransferRect_S; DAT_18044daa0 = PF_TransformWorld_S; ...`
  約 70 slot を、U.dll の CPU feature block (`U_G_CPU_DataP`) で SIMD 版を
  選びながら埋める。
- host は PF.dll を plug-in の import として map するだけで `PF_Birth` を
  呼んでいなかった (U.dll の `U_Birth` は #362 / #1063 で呼んでいる。同型)。
  結果、`PF_TransferRect` を import する Echo は
  `Echo+0x4005: CALL [PF.DLL::PF_TransferRect]` の先で address 0 に飛び
  (`stage:selector_seh ... access=execute fault=null stack0=plugin:Echo.aex+0x400b`)、
  `PF_TransformWorld` を import する Transform は PF.dll 内 (+0x6128c) で同じ形、
  `PF_Blend` を import する Spill2 は read@low になっていた。
- host で `PF_Birth()` を呼ぶと戻り値 21249 (0x5301)。decompile 上、table
  書き込みはその手前で完了し、21249 は末尾の COR/BIB 初期化段の戻り値。
  同じ 21249 は `not_discovered:exit_20_global_setup:21249` (PSL_Adjustments)
  にも出るが関連は未確認。

### 2.2 `effect_ref` は PF_ProgressInfo (#1275)

SDK は `PF_ProgPtr` を opaque と書き、abort/progress は `in_data->inter`
経由と書いているが、PF.dll 自身のシグネチャは `PF_ProgressInfo*` を名乗る
(`PFp_Convolve<T>(PF_ProgressInfo*, ...)`、
`PF_AreaSample_CPlusPlus<T,N>(PF_ProgressInfo*, ...)`)。実体の読み方:

| 誰が | 何を |
| --- | --- |
| PF.dll `FUN_180055960` (`PF_TransferRect_S` の本体) | `effect_ref == NULL` なら組み込み no-op、それ以外は `(*effect_ref[2])(*effect_ref, cur, total)` を行ごとに call。`effect_ref[2]` の null check は無い |
| PF.dll `PFp_Convolve<T>` | 同じ +0x10 slot を progress として call、+0 を refcon |
| CannedWarp.aex RENDER `FUN_180001700` (#1212 の `CannedWarp+0x176f`) | `in_data->effect_ref` を受け `(*effect_ref[2])(effect_ref[0], row, height)` を出力 1 行ごとに call |
| Echo.aex SMART_RENDER `FUN_180003bd0` | `*(effect_ref+0x10) = *(effect_ref+8)` を PF_TransferRect ループの間だけ書き、後で復元する |

- つまり `{ void* refcon (+0); fn (+8); PF_Err (*progress)(refcon, cur, total) (+0x10); }`。
- host はここに 4 byte の tag object を渡していたので、+0x10 の読みは worker の
  隣接 static に落ち、Echo の書き込みは host memory を書いていた。
- 推論: +8 は Echo が progress の代わりに差し込む関数なので呼び出し形が互換な
  もの (abort poll と読むのが自然)。AE の live な effect_ref を Frida で dump
  して slot の実体 (module+RVA) を確かめてはいない。

### 2.3 `PF_LayerDef` は `PF_World` に埋め込まれている (#1276)

- PF.dll `PF_WorldX<T>::PF_WorldX(PF_LayerDef*, bool)` (0xb7f0) は
  `this+0 = vftable`、`+0x18 = world_flags`、`+0x20 = data`、`+0x28 = rowbytes`、
  `+0x2c = width`、`+0x30 = height`、`+0x34.. = extent_hint`、
  **`+0x58 = this`**、`+0x88 = 0` を書く。LayerDef は PF_World の +8 に埋まって
  いて、`reserved_long4` (LayerDef+0x50 = object+0x58) は **その PF_World 自身**
  を指す。#1090 が Channel Blur で見た `reserved_long4+0x70/+0x74` は
  LayerDef+0x68/+0x6c = origin_x/y そのもの。
- vftable (`PF_WorldX<PF_Pixel8>::vftable` 0x1803cd148、19 slot):
  slot 0 = scalar deleting dtor、**slot 1 = depth (8/16/32 を返す short)**、
  slot 2 = `+0x88 == 0`、4 Clear、5 ClearExceptRect、6/7 Fill、8 FillAlpha、
  9 InvertAlpha、10 Premultiply、11 Unmultiply、12 PremulColor、13 UnmulColor、
  **14 CopyRect**、15 CopyRectPossiblyDifferentBitdepth、16 ...WithField、
  17 SubWorldSafetyGutterHasBeenOverWritten、18 (未解読)。depth 別に 3 table。
- `PFp_WorldDepth(PF_LayerDef*, bool)` (0xf640) も `reserved_long4 != 0` なら
  slot 1、無ければ `world_flags` の byte+3 で 8/16/32。
- 読む側:
  - Glow.aex `FUN_180009f90` (+0xa0b3): `MOV RCX,[RSI+0x50]` → `MOV RAX,[RCX]`
    → `CALL [RAX+8]` → `SAR AX,3` (= bytes/channel)。null check 無し。
  - Spill2.aex `FUN_180005210`: checkout した input / output の
    `PF_LayerDef*` から 8 を引いて `PF_World*` とし、PF.dll の
    `PF_World::CopyWorld(dst, src)` (0x4550) に渡す。CopyWorld は
    `dst->vtable[0x70/8 = 14]` (`CopyRect(const PF_World*, const M_LRect&, int, int)`)
    を call する。cdb の実測: `PF!PF_World::CopyWorld+0x39  call qword ptr [rax+70h]`
    で `rax = 0` (host の world の +0x50 が 0 だったため)。
  - Curl_Noise.aex `FUN_180003430` も `world - 8` を PF_World として渡す。
- `ae::pf::VideoFrameFactory::IsEffectWorldGPUBased(PF_LayerDef*)` (0x6140) は
  `data == 0 && platform_ref != 0` で GPU world を判定する。

### 2.4 ShapeBlur は AEGP handle を BEE object として渡す (#1264 の範囲)

- ShapeBlur (Camera Lens Blur) の `FUN_180019a40` は
  `AEGP PF Interface Suite` v1 slot 0 (`AEGP_GetEffectLayer`) →
  `AEGP Layer Suite` v11 slot 6 (`AEGP_GetLayerParentComp`) の戻り値を
  そのまま `BEE_CompItem*` として BEE.dll の
  `GetColorSettings@BEE_CompItem` に渡す。BEE.dll 側は
  `BEE_Item::GetParentProject` (this+0x38) → project+0xe8/+0xf0 の
  `shared_ptr<PF_ColorSettings>` を copy する。
- host は registry の borrowed token を返していたので `this+0x38` の読みで AV
  (`selector_seh ... module=BEE.dll ... stack0=module:BEE.dll+0x3bcefc`
  = `GetColorSettings` 内の call の戻り番地)。
- 修正後 (effect layer の parent comp = BEE facade の comp item、project に
  空 shared_ptr): この AV は解消し、より深い BEE.dll 内の null vtable call
  (`stack0=module:BEE.dll+0xc55a00`) に進んだ。残りは #1264。

## 3. host 側の実装 (要旨)

- `PF_Birth` を U_Birth と同じ latch (PF.dll の mapping 単位、SEH guard) で
  一度だけ呼ぶ。`stage:pf_host_layer_init status=called result=N`。
- `g_effect` を PF_ProgressInfo 形の object にし、+8/+0x10 を host の
  abort / progress callback に forward。plug-in bootstrap ごとに再 publish
  (Echo が書き換えるため)。
- host が渡す world storage を `world_safety::EffectWorldStorage`
  (8 byte の vtable prefix + 120 byte の LayerDef + 0x10 の tail = 0x90、
  tail は AE の `PF_WorldX` が +0x88 まで書くぶん) にし、`prepare_world_layout`
  が prefix に depth 別 vtable を書いて `reserved_long4` をそこに向ける
  (= `world - 8` が PF_World)。bare な 120 byte struct (PF_NewWorld の
  呼び出し側 struct、selftest) には mirror object を付ける。slot 1 = depth、
  slot 14 = CopyRect (同 depth・両 world に bound した copy)、他は識別 trap。
  GPU world (§2.3 の判定) には付けない。
- `AEGP_GetLayerParentComp` は effect layer に対して BEE facade の comp item
  を返し、comp accessor 側はその pointer を composition として解決する。

## 4. 結果 (AE oracle との突合)

`render_sweep --filter <name> --dump-frames`、AE 2026 26.3x87、同じ単色入力、
default param、frame 0:

| plug-in | before | after | AE との差 |
| --- | --- | --- | --- |
| Transform (`ADBE Geometry2`) | 512 | rendered | **byte 一致** (256x144) |
| Glow (`ADBE Glo2`) | 512 | rendered | **byte 一致** |
| Echo (`ADBE Echo`) | 512 | rendered | **byte 一致** |
| Spill2 (`ADBE Spill2`) | 512 | rendered | **byte 一致** (この capture のみ `loaded_aex_identity` verified) |
| CannedWarp (`ADBE WRPMESH`) | 512 | rendered | AE の comp 範囲 (256x144) 全 36864 pixel が一致。ただし host は straight alpha、AE の PNG は premultiplied なので、host 値を premultiply してから比較 (差は ±1 以内)。host は expand buffer を返すので出力自体は 461x199 @ origin (-102,-54) |

full-corpus (AE 2026 `Support Files\Plug-ins\Effects` 304 AEX、depth 8、
既定 size/time/frames): rendered 273 → 278、frame_error:512 20 → 15
(VR 12 本 + ColorAndContrast + Curl_Noise + ShapeBlur)、他の bucket は不変。

## 5. 未解決 / 記録のみ

- **ColorAndContrast / Curl_Noise**: どちらも「GPU 必須」を自分で表明する
  color family。CPU 経路では `$$$/MediaCore/AEFilters/.../CPUNotSupported=Color
  And Contrast effect requires GPU acceleration` /
  `$$$/AE/Curl_Noise/GPUWarning=Curl Noise Requires Mercury GPU Acceleration`
  の文字列を作って 0x200 (512) を返す = **plug-in 自身の戻り値**で、host の
  SEH 代替ではない。GPU 経路 (#1072 / #1157 の err14-retry gate) の軸。
- **ShapeBlur**: §2.4 の通り BEE.dll のより深い場所へ進んだだけ (#1264)。
- PF_World facade の未観測 slot はすべて識別 trap。full-corpus では 1 件も
  取られていない (取られれば `unsupported_suite_calls` に `PF_World vtable`
  slot N として出る)。
- GPU world に facade を付けると VideoFrame の `CreateGPUVideoFrame` 内で
  AV する (観測)。原因は未特定で、除外で回避している。
- AE の live な `effect_ref` / `reserved_long4` の中身を Frida で dump しては
  いない (実装は PF.dll / plug-in 側の逆アセンブルと AE 出力の一致による)。
- Glow の AE capture は `loaded_aex_identity` が 2 回とも `unverified`
  (`reason=loaded_module_not_observed`)。`-RequireLoadedAexIdentity` を付けた
  2 回目も同じで、AE の process tree に `Glow.aex` が mapped な瞬間を観測
  できていない (Spill2 では verified になる)。ただし 2 回の capture の PNG は
  byte 一致 (`ecc8ddde…`) で、host 出力とも byte 一致なので、突合の再現性は
  ある。Glow の AE 側 provenance は「未検証」のまま記録する。

## 6. main (#1278 / VR depth) を取り込んだ後の再測定 (2026-08-18)

作業中に main が進み (`175c4844`、PR #1278 = issue #1271)、VR family を depth
8/16 でも Premiere GPU 経路に載せる変更と、8/16↔32f 変換、`xGPUFilterEntry` の
SEH/C++ 例外封じ込めが入った。非VR でも `xGPUFilterEntry` を export する
plug-in がある (#1272) ため、この 5 本が #1278 側で既に直っていないか、
また逆に #1278 と衝突していないかを測り直した。

- 取り込みは merge (`019e031b`)。conflict は 3 つとも「両側が同じ配列/リストに
  要素を足した」だけで、両方を残して解決した (selftest route の
  `std::array` は 39 + main 1 + 本ブランチ 2 = 42)。
- merge 後に 3 exe を再ビルドし、mtime が編集より新しいことを確認してから
  再測定した。main の新 route `--self-test-argb32f-depth-conversion` は
  本ブランチの `EffectWorldStorage` (合計 0x90 = 先頭 8 byte の vtable prefix
  + 120 byte の LayerDef + 末尾 0x10 の tail。tail は AE の `PF_WorldX` が
  +0x88 まで書くぶんの余白) の上でも 3 worker すべて pass する。
- 再測定 (merge 済みブランチ、`--filter` 単位): Spill2 / Transform / Glow /
  Echo / CannedWarp は rendered、`pixel_sha256` は merge 前と同一。
  ColorAndContrast / Curl_Noise / ShapeBlur は 512 のまま (§5 の通り別軸)。
- 新 main 単体 (`175c4844` を別 worktree で build して full-corpus sweep):
  rendered 284 / 512 9 (CannedWarp, ColorAndContrast, Curl_Noise, Echo, Glow,
  ShapeBlur, Spill2, Transform, VRSphereToPlane) / not_discovered 7 /
  worker_invariant_failure 2 / rendered_empty 2。本ブランチは同じ引数で
  rendered 289 / 512 4 (ColorAndContrast, Curl_Noise, ShapeBlur,
  VRSphereToPlane — 最後の 1 本は VR 側の残りで #1084 の範囲) で、plug-in
  単位の差は上の 5 本だけ、他 299 本は bucket も `pixel_sha256` も一致する。
