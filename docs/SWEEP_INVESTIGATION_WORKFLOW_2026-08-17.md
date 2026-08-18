# render sweep 調査セッションの運用メモ (2026-08-17)

2026-08-12〜17 の render sweep コホート調査 (#1211 / #1213 / #1215 / #1255) で
複数セッションが同じ場所で二度手間を踏んだ運用知識をまとめる (issue #1256)。
観察と手順のみを書き、方針は追加しない。

## 1. 母集団の定義と計測の必須記載事項

`render_sweep` (`bridges/aviutl2-multifilter/examples/render_sweep.rs`) は
引数にフォルダを渡すとそのフォルダだけを、渡さないと configured/default scan
folders (AviUtl2 の登録が見るのと同じ集合) を掃く。この 2 つは別母集団:

| 呼び方 | 母集団 | 件数 (2026-08 時点) |
| --- | --- | --- |
| AE 2026 `Support Files\Plug-ins\Effects` を引数に渡す | #980 系コホート (frame_error 512 / 516 / 4、not_discovered など) の baseline | 304 AEX |
| 引数なし (configured scan) | AviUtl2 側の scan folder。MediaCore 主体 | 970〜975 AEX |

同じ効果名でも configured scan には無い、あるいは別ベンダーの同名ファイルに
当たることがある。#1037 / #1052 / #1069 / #1079 では過去セッションが configured
scan を見て「対象が corpus に無い」として claim を撤回しており、いずれも Effects
folder を引数に渡せば再現した (PR #1211 / #1255)。#980 の close 判定
(859/859 rendered) も configured scan 基準で、Effects folder には 2026-08-17 時点で
まだ失敗が残っている (#1256 本文の内訳を参照)。

計測を issue / PR に書くときは次を必ず併記する:

- 引数に渡したフォルダ (無ければ「configured scan」と明記) と件数
- `--depth` (省略時 8) と、`--size` / `--time` / `--frames` など既定から変えたもの
- report JSON の build fingerprint (sweep 実行ファイルと worker 3 exe の hash) か、
  最低でも計測した build の commit
- baseline との比較なら baseline 側も同じ項目

環境によって列挙件数が変わる例: PR #1215 の環境では full-corpus が 576 AEX
(304 の上位集合と記載) だった。件数を書かない比較は照合できない。

## 2. sweep と trace の取り方

- `AEXCOMPAT_MULTIFILTER_REPOSITORY=<repo>` は worker 3 exe
  (`aex_l2_worker.exe` / `aex_render_worker.exe` / `aex_smart_worker.exe`) を
  `<repo>\target\minihost-build\` から解決するための変数。sweep はこの値を
  そのまま使い (AviUtl2 DLL 側にあるプラグイン隣接への降格は sweep には無い)、
  3 exe の hash を報告の build fingerprint に入れる。指した先に exe が無ければ
  fingerprint に `open_failed` が記録され worker 起動が失敗する。sweep 前に
  指した先の 3 exe を確認する。
- 実行例:
  ```powershell
  $env:AEXCOMPAT_MULTIFILTER_REPOSITORY = "<worktree>"
  cargo run --release --manifest-path bridges\aviutl2-multifilter\Cargo.toml `
    --example render_sweep -- --json sweep.json "<AE>\Support Files\Plug-ins\Effects"
  ```
- `--filter <substr>`: ファイル名部分一致で絞る。修正後の対象数本の再計測はこれで、
  full-corpus は回帰確認用に別途 1 回。
- `AEXCOMPAT_EXTENDED_DIAG=1`: worker の host-callback trace を stderr に出し、
  各 record の `stderr_tail` (末尾 64 KB) に worker の生 stderr が入る。fault site
  (`stage:selector_seh`) や拒否痕跡 (`stage:callback_denied`) はここで読める。
  生 stderr は絶対パスを含みうるので、この変数を立てた出力は共有しない
  (`--include-scan-paths` も既定 off のまま)。issue / PR に貼るのは該当行の抜粋。
- `--close-report`: session の close report 全体を各 record に載せる。1 バケツの
  drill-down で close report の他フィールドまで要るときだけで、full-corpus では
  大きすぎる。
- 変数なしの既定 report は共有前提で stderr を持たない。
- `--dump-frames <dir>`: rendered した frame の raw pixel を
  `<dir>/<plugin>.<sha8>.f<n>.<W>x<H>.<format>` に落とす (#1253)。`rendered` が AE の
  参照 (mask 無しの Scribble は全 pixel 透明、Inner/Outer Key / Reshape は
  passthrough、など) と合っているかを見るためのもので、生の画像なので report と
  一緒に共有しない。record 側には `pixel_sha256` だけが常に乗る。
- host 拒否痕跡が出ないまま 4 / 512 になる plug-in は、trace に出ていない
  callback を疑う。`PF Path Query Suite` の checkout / checkin と
  `PF_CHECKOUT_LAYER_AUDIO` は #1253 で trace 行を足した
  (`extended_diag:path_checkout ...` / `extended_diag:checkout_layer_audio ...`)。
  それでも見えないときは cdb で worker ごと debug する (`cdb -o` で
  `render_sweep --filter` を子プロセスごと debug、`sxe -c "bu <mod>!<sym>
  ..." cpr` で子に deferred bp、C++ 例外は `sxe eh` + `k`、Adobe DLL 内部の
  戻り値は `bp /1 @$ra "r rax"`。手順と観測例は
  `docs/MASKLESS_PATH_EFFECTS_OBSERVATION_2026-08-17.md` §1)。
- AE の PNG は premultiplied alpha、host の dump は straight。alpha が 255 で
  ない pixel を含む出力を AE と比べるときは host 側を premultiply してから
  比較する (#1253 の AudWave、#1276 の CannedWarp)。また host は expand buffer
  を返すので、出力の extent が AE の comp より大きいことがある
  (record の `width`/`height`/`origin_x`/`origin_y` で AE 側の座標に写す)。
- `tools/capture-ae-reference.ps1` の `-EffectName` は matchName。AEX の PiPL が
  読めない (AE 同梱は `no_pipl` になる) ので、AEX のバイト列から `ADBE ...`
  文字列を拾うのが早い (`Spill2.aex` → `ADBE Spill2`、`CannedWarp.aex` →
  `ADBE WRPMESH`)。失敗すると `.result.json` が残り、次の実行が
  `Reference result file already exists.` で止まるので消してから再実行する。
- discovery 失敗 (`not_discovered:*`) は render と違って worker の stderr が record に
  乗らない。Effects folder の discovery は in-place cluster session (1 worker が
  複数 AEX を順に inspect) で走り、session の stderr は close 時の末尾 4 KB しか
  broker に戻らず、member 単位には割り付けられないため。record の
  `detail.discovery_diagnostics` (#1063 以降) には worker の partial report の
  selector 別 error code (`global_setup_error` / `params_setup_error` /
  `global_setdown_error`、`missing_suites`) が乗るのでまずそれを見る。1 本の
  生 stderr が要るときは one-shot の l2 worker を直接叩く:
  ```powershell
  $env:AEXCOMPAT_EXTENDED_DIAG = "1"
  $sha = (Get-FileHash $aex -Algorithm SHA256).Hash.ToLower()
  & target\minihost-build\aex_l2_worker.exe --l2-params-only $aex $sha `
      --dependency-dirs-v1 "<AE>\Support Files\Plug-ins\Effects;<AE>\Support Files"
  ```
  exit 20 = selector が非 0 (report の status=selector_error)、exit 11 = LoadLibrary
  失敗。`--dependency-dirs-v1` に AE の `Support Files` (Effects の 2 つ上) を
  含めないと AE 同梱 AEX は依存 DLL 不足で全部 exit 11 になる (sweep は
  multifilter の configured dependency dirs でこれを補っている)。one-shot で通るが
  full sweep で落ちる AEX は cluster の同居 member 依存なので、one-shot の結果だけで
  「再現しない」と判定しない (Reshape_New が実例)。

## 3. worktree 運用

- main working tree には他セッションの未コミット変更が乗っていることがある
  (そこで stash やコミットをすると他セッションの作業を巻き込む)。調査 / 実装は
  `git worktree add` した専用 worktree で行う。
- worktree のパスは短くする。scratchpad 配下のような長いパスに切ると Rust
  ビルドが依存クレートの build script で `LNK1104: cannot open file
  '...\build_script_build-*.exe'` を出して全滅する (4 セッション全部が踏んだ)。
  回避は worktree 自体を短いパスに置くか、`CARGO_TARGET_DIR` を短いパスに逃がす。
- C++ (minihost) の build dir も短いパスに置いてよい。生成した 3 exe を
  `<worktree>\target\minihost-build\` にコピーすれば sweep はそこから解決する
  (§2)。コピーし忘れると別 build の exe で測る。
- vcvars64 が要る点は `CLAUDE.md` Canonical Verification と
  `docs/BUILD_REQUIREMENTS.md` 「C++ worker (minihost)」を参照。

## 4. worker 再ビルドの確認手順

CLAUDE.md の「3 exe 全部を再ビルドする」に加えて、ビルドは 1 回でも失敗しうる
(commit charge 枯渇で rustc / cl が落ちるなど)。失敗したまま sweep を回すと古い
exe で測り、「修正が効いていない」ように見える (08-17 に実例あり)。

1. `cmake --build target\minihost-build --target aex_smart_worker aex_render_worker aex_l2_worker`
   の exit code を見る (`$LASTEXITCODE`)。
2. 3 exe の mtime が編集より新しいことを見る:
   ```powershell
   Get-ChildItem target\minihost-build\aex_*_worker.exe | Select-Object Name, LastWriteTime
   ```
3. §3 のコピー運用なら、コピー先の 3 exe で同じ確認をする。
4. それから `--filter` 再計測 → full-corpus。

## 5. CI runner

- CI (`Windows clean clone`) は現状の設定 (`vars.USE_SELF_HOSTED_RUNNER`) では
  self-hosted runner `windows-real` で走り、AE と機械を共有する。owner が AfterFX
  を開いている間は `test_ae_reference_capture_automation` が fail-closed で red に
  なり ("After Effects is already running; refusing to touch an existing user
  session")、AfterFX 終了後の `gh run rerun <run-id> --failed` で回復する
  (PR #1215 の初回 run がこれ)。
- run が queued のまま数十分動かないなら runner が落ちている。担当セッションから
  runner の再起動はできないので owner に依頼する (runner を触らない)。CI が
  動かない間のマージ可否は owner 判断で、担当セッションは blocked として報告する
  (PR #1255 で CI を待たずにマージしたのは、明示の owner 指示による 1 回限りの
  例外)。

## 6. local pytest の既知の環境要因 fail

`uv run python -m pytest -q` を worktree で回したときに出る、diff と無関係な fail:

| test | 件数 | 原因 |
| --- | --- | --- |
| `test_ae_reference_capture_automation` | 3 | AfterFX 起動中 (AE 排他ガード) |
| `test_pf_smart_geometry_probe[4]` | 2 | `pf_smart_geometry_probe.aex` をその worktree で build していない |
| `test_rust_host_core_phase5` 〜 `phase8` | 4 | `CARGO_TARGET_DIR` を分離しているため `broker\target\release` に FFI dll (`aexcompat_host_core_ffi.dll`) が無い |
| worker stderr を読む test (`test_active_plugin_context` / `test_worker_effect_bootstrap_timing` など) | 環境次第 | cp932 locale で worker stderr の decode が `UnicodeDecodeError` になる |
| `build aex_l2_worker.exe before running the native test` で落ちる native test 群 | 2026-08-17 の文書のみ worktree で 5 | その worktree の `target\minihost-build\` に worker 3 exe が無い |
| `missing self-test binary` で落ちる selftest 系 test | 同 worktree で 3 | selftest exe (`worker_*_selftest.exe`) を build していない。worker だけ build した worktree ではこちらだけ残る |
| `missing VS2022 worker` (`test_pf_parameter_animation_transport`) | 同 worktree で 1 | 別 build dir `target\minihost-build-v18\` の worker を build していない |

扱いは共通: 自分の diff がその test / 経路に非接触なことを確認し、PR body に
件数と内訳を明記し、CI を権威にする。fail が上の表に無い、または diff に接触する
なら環境要因扱いにしない。

---

この文書は実装状態と運用の記述で、方針の正本は `CLAUDE.md`。
