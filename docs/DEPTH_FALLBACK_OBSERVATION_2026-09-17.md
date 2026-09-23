# advertise していない depth の session dispatch (2026-09-17)

`PF_OutFlag_DEEP_COLOR_AWARE` を立てていない effect を 16 bpc session に流すと、
host は exit 22 (`depth_supported == false`) で拒否していた。AE は同じ状況で
**拒否せずレンダリングする**ので、session route を AE 側に合わせた記録。

## 1. 観察

対象は `KO_Foil.aex` (yama-ko.net、matchName `KO_Foil`、sha256
`b7ee86faa9824a81f00fadcf1bc52252abb609ca8391f5a725dca6dc5de9b705`)。
SmartFX で、out_flags `0x04000024` / out_flags2 `0x08001408`:

- `PF_OutFlag2_FLOAT_COLOR_AWARE` (bit 12) **立っている**
- `PF_OutFlag_DEEP_COLOR_AWARE` (bit 25) **立っていない**
- `PF_OutFlag2_SUPPORTS_SMART_RENDER` (bit 10) 立っている

### 1.1 変更前の host

`render_sweep --depth 8 / 16 / 32`、256x144、`--plugin-defaults --no-layer`:

| depth | 結果 |
| --- | --- |
| 8 | `rendered` |
| 16 | `render_frame_failed:worker_exited` (exit 22) |
| 32 | `rendered` |

16 bpc の worker trace は selector に入る前で終わっている:

```
stage:smart_render_begin
stage:smart_render_end pre_error=-1 render_error=-1
```

`worker_effect_bootstrap.cpp` の `depth_supported` が false になり、
orchestration の session branch にそもそも入らなかった。

### 1.2 AE のふるまい

`tools/capture-ae-reference.ps1 -Bpc 16`、AE 26.3x87、同じ 256x144 単色
RGBA(32,64,128,255) 入力、default parameter、frame 0。**AE は普通にレンダリング
する**。AE の 16 bpc 出力を 8 bit に落として AE の 8 bpc 出力と比べると:

```
AE16 (→8bit) vs AE8 : 差のある画素 254 / 36864、maxdiff [1 0 1 0]
```

1 段以内。つまり AE は 16 bpc project でもこの effect を 8 bit 精度で回して
いて、差は depth 変換の丸めだけ。(§4 で否定: AE の 16 bpc 出力は 8 bit の格子に
乗っていない)**観察できるのはここまで**で、AE が内部で
何を 8 bit に落としているか (world 全体か、effect の入出力だけか) はこの計測
では区別できない。

### 1.3 変更後の host

| depth | 結果 |
| --- | --- |
| 8 | `rendered` |
| 16 | `rendered` |
| 32 | `rendered` |

```
host8  vs AE8   : 差のある画素 0 / 36864     (バイト一致)
host16 (→8bit) vs AE16 (→8bit) : 254 px、maxdiff [1 0 1 0]
host16 (→8bit) vs host8 : 差のある画素 0
```

AE の 16 bpc 結果との距離は **AE 自身の 8 bpc 結果と AE の 16 bpc 結果の距離と
同じ** (254 px / maxdiff 1)。丸めの経路が違うだけで、それ以上ずれていない。

3 行目は独立した証拠ではない。host は 8 bit で回して広げているので、
一致するのは実装の同義反復。書いてあるのは「広げる経路に余計な丸めが
入っていない」ことの確認までで、AE との一致の根拠は 2 行目のみ。
(§4 で訂正: 2 行目も独立した証拠ではない。host8 == AE8 かつ host16→8 == host8
なので、2 行目は「AE8 vs AE16→8」の言い換えになっている)

## 2. 実装

- `effect_bootstrap::dispatch_pixel_bytes(session_pixel_bytes, out_flags,
  out_flags2)`: session が dispatch する depth。advertise していればその depth、
  していなければ **session depth より浅い側で** advertise している中の一番深い
  depth。FLOAT_COLOR_AWARE だけ持つ plug-in を 16 bpc session に流すと float32
  ではなく **8 bit** になる (深い側の advertise は session の上なので関係ない)。
  これが §1 で計測した KO_Foil のケースそのもの。(§4 で変更: この組み合わせは
  float32 で dispatch する)8 bit が床。session depth が
  {4,8,16} の外なら**そのまま返す** (caller の契約違反を握り潰さない)。
  self-test `--self-test-dispatch-pixel-depth-rule`。
- `render_pixel_transport::conform_pixel_depth(captured, pixels, pixel_bytes,
  dispatched_pixel_bytes)`: captured frame を slot の depth に合わせる。変換は
  既存の `argb_to_argb32f` / `argb32f_to_argb` 経由 (丸め・clamp・NaN の規則を
  もう一つ作らないため)。到着時の depth を返し、dispatch した depth でも
  float32 でもない stride、あるいはピクセル数に対して端数が出る buffer は 0 を
  返して caller が capture invariant failure として扱う。float32 を無条件に
  認めるのは **GPU negotiation transport** (#1072) のため: この経路は world を
  session の depth から plan するので、plug-in を 8 bit に narrow しても
  float32 のまま capture が返る (32bpc の OpenCL session + shallow plug-in)。
  (§4.5 で訂正: 「session の depth から plan」は case_id / retry 経路の話で、
  自動判定は dispatch 後の depth を見る)
  Premiere GPU-filter route (#1271) の方は download を自分で plan の depth に
  narrow してから publish するので、広いまま届くことはない。self-test
  `--self-test-pixel-depth-conform`。
- frame loop が session 単位で `dispatch_bytes` を持ち、frame callback に渡す。
  callback はその depth で world を組み、loop は capture を slot の depth へ広げる。
  変換が走った frame では報告する rowbytes も slot の stride に直す。world を
  一度も作っていない run では `pixel_format` は空のままにする (存在しない frame の
  depth を名乗るのは診断への捏造)。transport の
  入力スロットは元から全 depth で RGBA8 なので、入力側の変換はこの経路では増えない。
- **session の report は slot を記述する**。plug-in が浅く dispatch された場合、
  plug-in 自身の world 深度は caller が受け取らないバッファの記述になる。
  `pixel_format` / `bytes_written_per_row` を world 側から、`rowbytes` を slot 側から
  取ると、書き込み済みの行の半分を `undefined_tail_bytes_per_row` として報告する
  (= plug-in が行を書き残したという偽の証拠になる)。classic / smart どちらの
  report も session mode では session depth で揃える。
- `dispatch_pixel_bytes` は **frame が実際に到着した depth** を記録する
  (`conform_pixel_depth` の戻り値)。GPU negotiation transport (#1072) は
  dispatch した depth に関係なく float32 の world を plan するので、session の
  「決定」を記録すると走ったものと違う。
  frame が 1 枚も完走していない間は session の決定値。
- `Result::depth_dispatchable` を足し、**session route の gate だけ**をこちらに
  切り替えた。
- report の `depth_supported` は **「この run は要求された depth を出せたか」**に
  意味を寄せた。下流はこの false を「depth 交渉で断られた」と読む:
  `render_contract.rs` は `unsupported_pixel_depth` /
  `pixel_depth_negotiation` に分類した上で **selector error を捨てる**
  (`selector_error: if unsupported_render_path || unsupported_depth { None }`)。
  `run-conformance-bundle.py` は `"unsupported"` と分類するだけで、
  `meaningful_selector_error` を先に見るので selector error は残る。
  いずれにせよ narrow して描けた session を false のままにすると、実際の
  selector エラーが depth 交渉の拒否に化ける。advertise の生の事実は
  `advertised_depth_supported`、実際に dispatch した depth は
  `dispatch_pixel_bytes` として report に併記する (record, never enforce)。
- dispatch depth は **live な `out_data` からは読まない**。bootstrap が
  GLOBAL_SETUP 直後に取った snapshot (`advertised_out_flags` / `_2`) を session に
  渡し、cluster swap では `SwapPluginResult` が incoming member の snapshot を
  運ぶ。`out_data` は PARAMS_SETUP 以降のあらゆる selector が書き込む共有バッファで、
  frame 間で復元もされない (#843)。そこから読むと、flags を OR ではなく**代入**する
  plug-in が session の途中 (あるいは PARAMS_SETUP の時点) で depth を動かし、
  診断も出ない。selector が effect について変えうることと、host がその effect に
  world を何で渡すかは別。
- `worker_smart_setup.cpp` の plan 受理条件に `force_cpu_image`
  (`case_id == "request_cpu"`) を足した。従来この session case は
  `deep16`/`float32` 経由で**偶然**受理されていて、narrow すると両方 false に
  なり plan 自体が無効になる。一度「外部画像なら何でも受理」(`has_external_rgba`)
  にしたが、それだと未知の case_id まで valid になり classic 側
  (`prepare_image_request` は未知 case_id を -2 で拒否) と食い違うので、
  session case を名指しする形に絞り直した。

## 3. スコープ外 / 残り

- **one-shot route は触っていない**。ただし「`depth_supported` で拒否し続ける」
  という説明は正しくない: #365 が deep な one-shot を運ぶ argv transport を
  消したので、残っている one-shot arm は全部 `external_pixel_bytes == 4` で
  走る。そこでは `dispatch_pixel_bytes` が無条件に 4 を返すので
  `depth_supported` は常に true。つまり**現状 depth で拒否できる経路は
  どこにも無い**。gate を 2 つに分けてあるのは問うている内容が違うからで、
  どちらかが今 fire するからではない。
- #1072 の float32→8bit narrowing はこの変更で**削除した**。同じ clamp/丸めを
  手書きした 2 つ目の規則になっていて、NaN の扱いだけが `argb32f_to_argb` と
  食い違っていた (手書き側は NaN が比較を両方すり抜けて cast に届くので UB)。
  今は全 session depth で conform が narrowing を担当する。副次的に、session が
  16 bpc で GPU route が走った場合はこれまで capture サイズ不一致で invariant
  failure になっていたのが argb16 に narrow されるようになった (未観測)。
- **classic session 側は計測していない**。計測した plug-in が SmartFX 専用なので、
  `run_render_session` の経路は self-test と同じ conform を通る、という以上の
  根拠が無い。Classic の AE oracle は別途。
- session 内で実際に narrow/widen が起きることは smart / classic 両方で
  押さえるようになった。`pf_smart_geometry_probe` と `pf_sampling_probe` に
  advertise だけを変える variant を 2 つ足し、**選択は probe 自身のファイル名の
  マーカー**で行う (`GetModuleFileNameA` で自分のパスを読む):
  - `...-shallow.aex`: GLOBAL_SETUP で DEEP / FLOAT を出さない。
    `test_smart_session_narrows_...` (32bpc) と
    `test_classic_session_narrows_...` (16bpc) が使う。
  - `...-rewrite.aex`: GLOBAL_SETUP では両方出して PARAMS_SETUP で**代入**して
    消す。`test_smart_session_depth_follows_global_setup_not_a_later_rewrite`
    が使い、live な `out_data` を読む実装だとここで 8 bit に落ちる。

  テスト側は `probe_variant()` (`tests/test_render_session_worker.py`) で probe を
  マーカー付きの名前にコピーし、worker にそのコピーを渡す。

  **環境変数でやってはいけない** (最初そうして差し戻した)。ambient なので同じ
  probe を harness 経由で起動する他のテスト
  (`tests/_render_session.py`、`test_pf_smart_geometry_probe.py`、
  `test_pf_sampling_probe.py`、`test_render_video_batch_cli.py`、
  `test_smart_output_reset.py`) と
  `tools/refresh-smartfx-geometry-evidence.ps1` が全部継承する。しかもこの変更で
  session の report が world ではなく slot を記述するようになったため、
  `test_geometry_contract_is_identical_across_depths` の
  `pixel_format == [argb8, argb16, argb32f]` という guard は narrowing を検出
  できない: `AEXCOMPAT_PROBE_SHALLOW=1` をシェルに置くと、3 つの depth を確かめた
  つもりで全部 8 bit で走りながら緑のまま通ることを実測した。ファイル名なら
  variant が「実際に load された plug-in」に付いて回り、ambient な状態が無くなる。
  古い環境変数を両方セットして suite を回し、何も変わらないことも確認済み。

  これまで tree にある probe は全部 DEEP+FLOAT を無条件に出していたので、
  session の dispatch depth が常に session depth と一致し、narrowing のコードが
  一度も走っていなかった。

  この変更を検証したときの baseline (probe は native build の**後**に rebuild
  すること。`tests/_render_session.py` の freshness gate は probe の `.aex` が
  `aex_worker.exe` より新しいことを要求するので、順番を逆にすると stale で
  落ちて regression に見える):

  ```
  uv run python -m pytest \
    tests/test_worker_selftest_routes.py tests/test_render_session_worker.py \
    tests/test_smart_session_worker.py tests/test_classic_render_contract.py \
    tests/test_smartfx_render_report_contract.py \
    tests/test_minihost_render_output_safety.py \
    tests/test_worker_session_component.py \
    tests/test_openfx_render_session_contract.py \
    tests/test_pf_smart_geometry_probe.py tests/test_pf_sampling_probe.py \
    -q --run-built-artifact-tests
  # 79 passed, 3 skipped

  cargo test --manifest-path broker\Cargo.toml --test render_session_wrapper -q
  # 27 passed. pytest と同時に走らせないこと (aex_worker.exe を取り合う)。
  ```

  赤くなることを確認した mutation: `dispatch_pixel_bytes` を恒等にする /
  snapshot ではなく live `out_data` を読む / 報告 rowbytes を plug-in の stride に
  戻す / classic report の `session_pixel_bytes` を落とす / smart report の同上 /
  plan 受理から `force_cpu_image` を外す / `conform_pixel_depth` の受理を
  認識できる stride 全部に広げる / marker 判定を両方向に定数化する。
- **まだ押さえられていない配線**:
  - cluster swap 後の depth 再計算。`swap_plugin` を実 AEX 相手に動かすテストが
    無い (swap の既存カバレッジは broker 側の `dummy-workers` だけ)。variant が
    ファイル名で決まるようになったので、深い member と浅い member を同じ manifest
    に並べること自体は可能になった。
  - GPU negotiation transport (#1072) の float32 capture。
    `dispatch_pixel_bytes` が session の決定ではなく capture 深度を記録する理由が
    これで、`conform_pixel_depth` が stride 16 を無条件に受ける理由でもあるが、
    GPU route を走らせるテストが無い。なお report と per-frame message の
    食い違いはこの変更で解消している: `plan.float32` が立つと
    `runtime->pixel_format` は `argb32f` になるので
    `world_pixel_bytes != frame_pixel_bytes` となり、smart report は
    `width * session depth` を報告して `frame_done` と一致する。main ではここが
    非対称だった。
  - 早期 exit 経路の `outcome.rowbytes`。ここは振る舞いも変わっている: 以前は
    plug-in 自身の stride を報告していたのが slot の stride になる。session が
    取りうる case_id に padded stride は無いので、実際に差が出るのは narrow した
    session が transfer 前に落ちた場合 (`width * dispatch_bytes` ではなく
    `width * pixel_bytes` を報告する)。
  - `conform_pixel_depth` が 0 を返したときの capture 拒否。後段のサイズ検査が
    ほぼ全部を捕まえるので、残るのは「slot の depth とは一致するが dispatch した
    depth とは一致しない stride」(まさにこの拒否が存在する理由のケース)
    だけで、それを作る fixture が無い。ブロックを削除しても suite は緑のまま。
  - `-rewrite` の PARAMS_SETUP ブロックが**常に off でも常に on でも**検出できない。
    host は snapshot を使うので、probe が rewrite してもしなくても
    `dispatch_pixel_bytes: 16` になる。つまり両 probe の rewrite ブロックを
    削除しても、逆に無条件実行にしても suite は緑のまま。
    `test_smart_session_depth_follows_global_setup_not_a_later_rewrite` が赤に
    できるのは「host が live な `out_data` を読む」mutation だけ。マーカー解析
    そのものは `-shallow` 側のテストが両方向で押さえている。
  - narrowing を挟む複数フレーム、および narrowing と in-session output grow
    (#262) の組み合わせ。新しいテストはどちらも 1 フレームしか送っていない。
- probe は今や `out_flags =` の代入を 2 つ持つので、
  `tests/test_probe_pipl_contract.py` が拒否する形になっている (同テストは
  「per-variant flags には per-variant source が要る」と明記している)。
  両 probe ともその `PROBES` tuple に入っていないので落ちないが、唯一認められた
  `.rc`↔`.cpp` チェックの射程からさらに外れた。
- shallow 版 probe は **AE oracle の fixture としては使えない**。`.rc` の PiPL は
  `DEEP_COLOR_AWARE` を無条件に宣言したままで、runtime 側だけが条件付きで落とす。
  AEXCompat は GLOBAL_SETUP の flags を読むので host 側の試験には足りるが、
  AE は PiPL も見る。Direction 4 で使うなら別に建てること。
- `depth_supported` が **false になりうる経路はもう無い**。one-shot は全部 8bit で
  `dispatch_pixel_bytes` が無条件に 4 を返し、session は `depth_dispatchable`
  (常に true) を報告する。結果として
  `broker/crates/harness/src/windows/render_contract.rs` の
  `unsupported_pixel_depth` 分類と `matrix_error_summary` の
  "AEX did not advertise support for the requested pixel depth" は**到達不能**に
  なり、新しい `advertised_depth_supported` を読むものは下流に無い。
  Direction 4 の sweep を撮り直すと、これまで `unsupported_pixel_depth` だった
  16bpc の行が `rendered` に変わり、raw report を開かない限り本物の 16bit
  レンダリングと区別できない。broker 側の追従は別途。
- `dispatch_pixel_bytes` の規則は `PF_OutFlag2_SUPPORTS_GPU_RENDER_F32` を見ない。
  GPU F32 を出すが `FLOAT_COLOR_AWARE` を出さない plug-in は 32bpc session で
  8 bit に narrow される。ただし GPU route に入らなくなるわけではない:
  - `--smart-session32-v1` の **1 回目の attempt** は skip する
    (`gpu_negotiation` の該当項が `external_pixel_bytes == 16` = narrow 後の
    depth を見るため)。
  - その attempt が CPU SMART_RENDER で 14 を返すと #1072 の retry が走る。
    この retry は depth を一切見ず `force_gpu_retry` で `gpu_negotiation` を
    立てるので、結局 advertise していない float32 world を受け取る。GPU 専用
    effect が 14 を返すのはまさにこの fallback が存在する理由なので、
    このクラスの plug-in では現実的な経路。
  - `--smart-session32-opencl-v1` / `-directx-v1` は case_id が直接
    `gpu_negotiation` を立てるので 1 回目から route に入る。

  変更前はそもそも拒否されていたので main に対する regression ではないが、
  この規則が黙って決めているケースではある。
- **同じ「live な `out_data` を読むな」という問題が隣にある**。
  `worker_smart_setup.cpp` の `advertised_gpu_support` は
  `read<uint32_t>(output, 400) & (1u << 25)` で、この変更が depth について
  「信用してはいけない」と論じているのと同じ共有バッファを読んでいる。flags を
  代入する plug-in は GPU negotiation も同じように動かせる。この変更以前からある
  もので今回は触っていないが、同じ関数を編集して黙っているのは筋が悪いので記録する。
- broker 側には **到達不能になったコードにテストが付いている**。
  `render_contract.rs` の `unsupported_pixel_depth` / `pixel_depth_negotiation`
  分岐と `matrix_error_summary` の depth 文言はもう発火しないのに、
  `broker/crates/harness/src/windows/tests.rs` が手書き JSON からそれを assert
  し続けている。追従は別途。
- `conform_pixel_depth` が受け入れる到着 depth は「dispatch した depth」と
  「float32 (GPU route)」の 2 つだけにしてある。認識できる stride を全部
  受け入れると、誰も dispatch していない depth で capture した malformed output
  を黙って広げて成功扱いにしてしまう (capture invariant が無効化される)。
- AE が 16 bpc project で内部的にどこを 8 bit に落としているかは未確認 (§1.2)。
  host の実装は「world を落として dispatch し、結果を広げる」で、observable な
  結果が AE と 1 段以内で一致することまでしか主張しない。
  (§4 で訂正: 8 bit に落としているという前提自体が否定された)
- `output_hash` / `input_hash` は dispatch 時の narrow buffer のハッシュなので、
  narrow した 16 bpc run と同じ plug-in の 8 bpc run は同じ値になる
  (§4 以降: float で dispatch する float-only plug-in では、16 bpc run と同じに
  なるのは 32 bpc run の方)。Direction 4
  の corpus で depth 回帰をハッシュ比較で見る場合はこれでは捕まらない。
  report の `dispatch_pixel_bytes` で条件を付けて読むこと。一方
  `output_checksum_detail` (opt-in) は conform 後の slot から取るので session
  depth 側。同じ frame について 2 つの depth の記述が並ぶ状態で、この変更前は
  一致していた。
- per-frame の provenance が無い。`frame_done` は session depth の
  `pixel_format` しか持たず、`dispatch_pixel_bytes` は final report に
  last-frame-wins で 1 つあるだけ。swap を挟む multi-frame batch では frame ごとの
  depth を言えない。`docs/RENDER_SESSION_PROTOCOL_2026-07-19.md` §4.3/§5 も
  「plug-in は `depth_code` より浅く dispatch されうる」を書いていない。
- `worker_invocation_orchestration.cpp` の `-6` sentinel (depth 未対応) は今も
  advertise 側の `depth_supported` から作っている。session mode で
  `params_error != 0` かつ advertise していない depth だと、report に
  `depth_supported: true` と `render_error: -6` が並ぶ。両 consumer とも他の
  field を先に見るので分類は狂わないが、記録としては矛盾している。

## 4. レビューでの訂正と規則の変更 (2026-09-23)

PR 前のローカルレビューで、§1.2 の「AE は 8 bit 精度で回している」が計測から
言えていないと指摘された。上の記述は消さず、ここで訂正する。

### 4.1 観察

2026-09-17 のセッションが撮った AE の出力 (8 bpc と 16 bpc の PNG、256x144、
KO_Foil、default parameter、単色 RGBA(32,64,128,255) 入力) をそのまま使って
再解析した。AE は再実行していない。

- AE 16 bpc 出力の R/G/B の distinct 値は 340 / 396 / 246。AE 8 bpc 出力は
  142 / 131 / 102。
- 画素ごとに見ると、AE 8 bpc で同じ値を持つ画素が AE 16 bpc では 2〜3 通りの値に
  分かれる (8 bit 値 211 種のうち 179 種)。
- 8 bit → AE 内部 0..32768 → 16 bit PNG → 8 bit の往復は、戻しが丸め
  (`(y*255+32767)//65535`) なら全 256 値で元に戻る (机上計算。書き出し側は
  round / floor / x2 のいずれを仮定しても同じ)。§1.2 の 254 px の差が何の丸めで
  出たかは記録が無く、特定していない。
- 2026-09-17 の host 32 bpc dump (float32) を `round(v*65535)` で 16 bit にした
  ものと AE 16 bpc 出力の差は最大 2/65535。AE 8 bpc を `v*257` で広げたものとの
  差は最大 129/65535。
- 規則変更後の host (16 bpc session で float32 dispatch) の 16 bpc 出力を
  `(v*65535)//32768` で PNG 領域に写すと、AE 16 bpc 出力との差は最大 2/65535、
  差のある画素 13494 / 36864。`(v*65535+16384)//32768` だと最大 3/65535、
  25059 px。変更前の host (8 bit dispatch) は全 36864 px で最大 128/65535。
  host の R/G/B distinct 値は 338 / 395 / 245。
- host 8 bpc と AE 8 bpc は変更後もバイト一致 (差 0 px)。

### 4.2 訂正と解釈

- **否定 (§1.2)**: 「AE は 16 bpc project でもこの effect を 8 bit 精度で回している」。
  8 bit で描いて広げたなら 8 bit 値から 16 bit 値への対応は一意になるはずで、
  4.1 の観察と合わない。
- **訂正 (§1.3)**: 2 行目 (host16→8 vs AE16→8) は AE との一致の独立した根拠では
  なかった。3 行目と合わせて、AE8 vs AE16→8 の言い換えになっている。
- 仮説: AE はこの effect (FLOAT_COLOR_AWARE のみ) を 16 bpc project で float か
  それに近い精度で dispatch している可能性が高い。host の float32 dispatch が
  最大 2/65535 まで寄ることがその傍証。16 bit で渡しているのか float で渡して
  いるのかは、この計測では区別していない。
- 残差 (最大 2〜3/65535) が float→16 bit の丸め、AE の 0..32768 → PNG 16 bit の
  書き出し、effect 内部の精度差のどれに由来するかは未確認。byte-exact の主張は
  しない。

### 4.3 規則の変更

`dispatch_pixel_bytes` を「session の depth を advertise していなければ、それより
**上**で一番近い advertise 済み depth、上に無ければ下で一番深いもの」に変えた。
{8, 16, 32} の中で上に advertise があるのは「16 bpc session で FLOAT のみ」だけで、
計測したのもこの組み合わせだけ。下に落とす側 (32 bpc で DEEP のみ、16 bpc で
どちらも無し) は AE と照合していない。

probe に `-floatonly` variant (FLOAT を出し DEEP を出さない) を足し、classic と
smart の 16 bpc session で `dispatch_pixel_bytes == 16` を確かめるテストを足した。
規則を旧版に戻すと、この 2 本と self-test route が落ちることを確認済み。

### 4.4 同じレビューで直したもの / 起票したもの

- probe の variant 判定が `GetModuleFileNameA` のフルパスに `strstr` していた。
  checkout や worktree のディレクトリ名に `-shallow` などが入ると素の probe が
  variant になる。ファイル名部分だけを見るように直した。
- 範囲外として issue にした: 下流 (matrix 分類、到達不能分岐とそのテスト、
  protocol doc、report の depth provenance、`conform_pixel_depth` の float32 無条件
  受理) は #1538、`advertised_gpu_support` の live `out_data` 読みは #1539。§3 の
  「broker 側の追従は別途」は #1538 を指す。

### 4.5 2 回目のレビューで直したもの (2026-09-23)

- 新しい e2e テスト 2 本を `tests/built_artifact_tests.txt` に登録した。
- **訂正 (§2)**: GPU negotiation transport が「world を session の depth から
  plan する」は不正確だった。case_id (`gpu_opencl_float32` / `gpu_directx_float32`、
  32 bpc の `--smart-session32-opencl-v1` / `-directx-v1` だけが持つ) と frame loop の
  `force_gpu_retry` で入る経路は dispatch depth と無関係に float32 world を渡す。
  自動判定 (`worker_smart_setup.cpp` の `external_pixel_bytes == 16 &&
  advertised_gpu_support`) は dispatch 後の depth を見る。§3 の GPU の項の方が正しい。
- **挙動の変化 (未観測)**: 上の自動判定が dispatch depth を見るため、FLOAT のみを
  advertise し `SUPPORTS_GPU_RENDER_F32` も advertise する SmartFX は、16 bpc
  session でも 1 回目の attempt から GPU negotiation に入る。DEEP も advertise する
  plug-in は 16 bpc では 8 bytes で dispatch されるので CPU のまま。GPU context が
  立たない機械では frame が落ちる。main ではこの組み合わせは 16 bpc session ごと
  拒否されていたので regression ではない (扱いは 32 bpc session と同じ) が、AE が
  この組み合わせで GPU を使うかは確認していない。16 bpc には
  `--smart-session32-cpu-v1` に当たる CPU 固定のコマンドが無いので、GPU の無い
  機械では caller がこの経路を避ける手段も無い。
  KO_Foil は GPU F32 を advertise していないので §4.1 の計測には影響しない。
  追従は #1538 に追記した。
- 「narrow して広げる」前提で書かれていた worker 側のコメント
  (`l2_main_entry.inc`、`worker_invocation_orchestration.*`、report の
  `session_pixel_bytes`、`depth_dispatchable`) を「別の depth で dispatch して
  変換する」に直した。
- §3 の variant 一覧 (2 つ) と baseline (79 passed) は 2026-09-17 時点のもの。
  今は `-floatonly` を含めて 3 つで、同じ baseline は 81 passed, 3 skipped。
