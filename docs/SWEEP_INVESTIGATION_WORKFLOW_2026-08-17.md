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
- report JSON の build fingerprint (sweep 実行ファイルと worker exe の hash) か、
  最低でも計測した build の commit
- baseline との比較なら baseline 側も同じ項目

環境によって列挙件数が変わる例: PR #1215 の環境では full-corpus が 576 AEX
(304 の上位集合と記載) だった。件数を書かない比較は照合できない。

## 2. sweep と trace の取り方

- `AEXCOMPAT_MULTIFILTER_REPOSITORY=<repo>` は worker 実行ファイル
  (`aex_worker.exe`、discovery/classic/smart を `--kind` で切替) を
  `<repo>\target\minihost-build\` から解決するための変数。sweep はこの値を
  そのまま使い (AviUtl2 DLL 側にあるプラグイン隣接への降格は sweep には無い)、
  その hash を報告の build fingerprint に入れる。fingerprint は
  `l2_worker` / `classic_worker` / `smart_worker` の 3 フィールドを持つが
  (issue #1495 前の 3 exe 構成の名残)、いずれも同じ `aex_worker.exe` を指すため
  常に同一の値になる。指した先に exe が無ければ fingerprint に `open_failed`
  が記録され worker 起動が失敗する。sweep 前に指した先の exe を確認する。
- 実行例:
  ```powershell
  $env:AEXCOMPAT_MULTIFILTER_REPOSITORY = "<worktree>"
  cargo run --release --manifest-path bridges\aviutl2-multifilter\Cargo.toml `
    --example render_sweep -- --json sweep.json "<AE>\Support Files\Plug-ins\Effects"
  ```
- `--filter <substr>`: ファイル名部分一致で絞る。修正後の対象数本の再計測はこれで、
  full-corpus は回帰確認用に別途 1 回。
- `--depth` (省略時 8) で経路が変わる効果がある。`xGPUFilterEntry` を export する
  AEX (AE 2026 Effects folder では VR 12 本 + Bilateral / Box_Blur /
  DirectionalBlur / FractalNoise / Levels2 / Lumetri / Transform / VideoLimiter
  の 8 本、dumpbin /EXPORTS で列挙できる) は、全 depth でまず PF path を通り、
  PF CPU selector が 512 または 516 を返したときだけ frame loop の retry で
  pr-gpu 経路 (`worker_smart_dispatch.cpp` の `run_pr_gpu_filter`) に入る
  (#1271, #1272。trace では `smart_render_cpu_end error=512` →
  `frame_setdown_*` → `frame_setup_*` → `pr_gpu_startup_end` →
  `pr_gpu_render_begin` の順に見える。retry は session frame loop だけで、
  one-shot `--smart-image*` は session-loop retry を持たない。8/16bpc は
  PF の 512/516 のまま、float32 は従来どおり export-first になる。512 が plug-in 自身の
  戻り値でない frame (selector が SEH で落ちた / C++ 例外が抜けた / module
  audit を通らなかった、いずれも host が 512 に置換する) や、#1072 の
  14 → GPU transport retry を通った frame では retry しない)。
  1 depth の結果だけで「render する / しない」を
  判定しない。経路に入った frame があれば、その終わり方が各 record の
  `worker.pr_gpu_route` に 1 つ入る (`committed` / `startup_fault` /
  `no_output_frame` など。EXTENDED_DIAG 不要)。frame ごとには入らず、記録に
  残った最後の 1 つで、stage event list は session 単位で上限があるため
  多 frame session では早い frame の結果になりうる。containment により経路の fault は worker を落とさず PF path への
  fall-through になるので、`rendered` でもこの key を見ないと「GPU 経路を試して
  降りた」ことに気付けない。
- `stage:selector_seh` の `unwind=` は fault した `CONTEXT` を x64 unwind data で
  辿った call chain (innermost first、frame 0 は fault site 自身。最大 12 frame、
  #1312)。同じ行の `stackN=` は RSP 先頭の数 qword を module 解決しただけの
  **ヒューリスティック**で、既に return した関数の残骸が呼び出し元と同じ形で出る
  ことがある (実例: ShapeBlur の `stack4=module:BEE.dll+0xc8d146` は呼び出し元では
  ない)。両方あるときは `unwind=` を読む。
  - `?` が付いた frame だけは unwind table 由来ではない: fault site 自身に unwind
    entry が無いとき (null slot への call、あるいは .pdata を持たない leaf) に
    RSP から読んだ戻り番地で、fault が本当に call だった場合にのみ呼び出し元を
    指す。付くのは frame 1 だけ。
  - **`unwind_stop=` を必ず見る**。一番外側に出ている frame が stack の頂上だと
    言えるのは `end_of_chain` のときだけ:
    - `end_of_chain`: unwind が null の instruction pointer に行き着いた =
      thread の frame chain の端。
    - `frame_cap`: 12 frame の上限で切れた。**plug-in の奥で fault したときは
      これが普通**で、外側はまだ続いている。
    - `chain_lost`: table 由来の前進が止まった — unwind が stack base の方向に
      進まなくなった (壊れた stack / 壊れた unwind data)、または fault site の
      戻り番地スロットが null だった。外側の frame は頂上ではない。
    - `no_unwind_entry`: fault site 以外の frame に unwind entry が無く、
      そこで chain が切れた。
    - `return_slot_unreadable`: fault site に unwind entry が無く、push された
      戻り番地がそもそも読めなかった (frame 0 だけ。読めて中身が null なら
      `chain_lost`)。
    - `walk_faulted`: 走査中に stack が読めなくなった。それまでの frame は有効。
    - `low_stack`: stack limit に近すぎて走査していない。
  - worker は `AEXCOMPAT_EXTENDED_DIAG` なしでもこの行を出すが、sweep の record で
    読むには `stderr_tail` が要る = `AEXCOMPAT_EXTENDED_DIAG=1` が要る。
  - l2 worker の report には
    `worker_report.selector_invocations.records[].unwind_frames` /
    `.unwind_stop` として入る。ただし broker の `propagate_selector_invocations`
    は key allowlist で record を組み直すので、`inspect` や render session の
    diagnostics には伝わらない (worker report をそのまま埋め込む `broker l2`
    route だけが持ち、そこは登録済み observation profile 専用)。任意の AEX の
    fault を読むときは stderr の行が唯一の経路 (#1314)。
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
  生 stderr が要るときは one-shot の discovery route を直接叩く:
  ```powershell
  $env:AEXCOMPAT_EXTENDED_DIAG = "1"
  $sha = (Get-FileHash $aex -Algorithm SHA256).Hash.ToLower()
  & target\minihost-build\aex_worker.exe --kind discovery --l2-params-only $aex $sha `
      --dependency-dirs-v1 "<AE>\Support Files\Plug-ins\Effects;<AE>\Support Files"
  ```
  exit 20 = selector が非 0 (report の status=selector_error)、exit 11 = LoadLibrary
  失敗。`--dependency-dirs-v1` に AE の `Support Files` (Effects の 2 つ上) を
  含めないと AE 同梱 AEX は依存 DLL 不足で全部 exit 11 になる (sweep は
  multifilter の configured dependency dirs でこれを補っている)。one-shot で通るが
  full sweep で落ちる AEX は cluster の同居 member 依存なので、one-shot の結果だけで
  「再現しない」と判定しない (Reshape_New が実例)。

## 2.1 並行セッションと sweep プロセス

複数セッションがそれぞれの worktree で同時に sweep を回すことがある
(2026-08-18 に #1271 (VR) と #1274-#1276 (非VR) が同時に走った)。自分の
sweep を止めるときに `Get-Process render_sweep | Stop-Process` を使うと
**他セッションの sweep も落ちる**。実際に 2 回巻き込んだ。path で絞ること:

```powershell
Get-CimInstance Win32_Process -Filter "Name='render_sweep.exe'" |
  Where-Object { $_.ExecutablePath -like '*<自分の CARGO_TARGET_DIR>*' } |
  ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
```

同じ理由で、sweep 中に `<worktree>\target\minihost-build\` の worker exe を
上書きしない (§4 のコピー運用)。走っている sweep が測る exe が途中で入れ替わり、
その計測は build fingerprint と一致しなくなる。exe を差し替えたら sweep を
やり直す。

## 3. worktree 運用

- main working tree には他セッションの未コミット変更が乗っていることがある
  (そこで stash やコミットをすると他セッションの作業を巻き込む)。調査 / 実装は
  `git worktree add` した専用 worktree で行う。
- worktree のパスは短くする。scratchpad 配下のような長いパスに切ると Rust
  ビルドが依存クレートの build script で `LNK1104: cannot open file
  '...\build_script_build-*.exe'` を出して全滅する (4 セッション全部が踏んだ)。
  回避は worktree 自体を短いパスに置くか、`CARGO_TARGET_DIR` を短いパスに逃がす。
- C++ (minihost) の build dir も短いパスに置いてよい。生成した exe を
  `<worktree>\target\minihost-build\` にコピーすれば sweep はそこから解決する
  (§2)。コピーし忘れると別 build の exe で測る。
- vcvars64 が要る点は `CLAUDE.md` Canonical Verification と
  `docs/BUILD_REQUIREMENTS.md` 「C++ worker (minihost)」を参照。

## 4. worker 再ビルドの確認手順

issue #1495 で worker は `aex_worker.exe` 1 本 (discovery/classic/smart を
`--kind` で切替) に統合され、以前あった「3 exe 個別にリンクされ、1 target だけ
ビルドすると他 2 つがステールのまま残る」罠は無くなった (link は 1 回だけ)。
ただしビルドは 1 回でも失敗しうる (commit charge 枯渇で rustc / cl が落ちるなど)。
失敗したまま sweep を回すと古い exe で測り、「修正が効いていない」ように見える
(08-17 に実例あり)。

1. `pwsh -File tools\build-native.ps1` の exit code を見る (`$LASTEXITCODE`)。
2. exe の mtime が編集より新しいことを見る:
   ```powershell
   Get-ChildItem target\minihost-build\aex_worker.exe | Select-Object Name, LastWriteTime
   ```
3. §3 のコピー運用なら、コピー先の exe で同じ確認をする。
4. それから `--filter` 再計測 → full-corpus。

## 5. CI runner

- CI (`Windows clean clone`) は GitHub-hosted の `windows-latest` で走る
  (#1457)。self-hosted runner `windows-real` は廃止したので、CI が AE と機械を
  共有することはもう無い。owner が AfterFX を開いていても CI 側は影響を受けない。
  - 訂正前の記述 (2026-08-20 まで): CI は `vars.USE_SELF_HOSTED_RUNNER` により
    self-hosted runner `windows-real` で走り AE と機械を共有していた。AfterFX
    起動中は `test_ae_reference_capture_automation` が fail-closed で red になり
    ("After Effects is already running; refusing to touch an existing user
    session")、AfterFX 終了後の `gh run rerun <run-id> --failed` で回復していた
    (PR #1215 の初回 run がこれ)。ローカルで pytest を回すときの AE 排他は
    §6 のとおり今も有効。
- run が queued のまま長時間動かないのは GitHub 側の事情 (無料枠、同時実行上限、
  runner 障害) になった。担当セッションから打てる手は無いので、blocked として
  報告する。CI が動かない間のマージ可否は owner 判断 (PR #1255 で CI を待たずに
  マージしたのは、明示の owner 指示による 1 回限りの例外)。

## 6. local pytest の既知の環境要因 fail

`uv run python -m pytest -q` を worktree で回したときに出る、diff と無関係な fail:

| test | 件数 | 原因 |
| --- | --- | --- |
| `test_ae_reference_capture_automation` | 3 | AfterFX 起動中 (AE 排他ガード) |
| `test_pf_smart_geometry_probe[4]` | 2 | `pf_smart_geometry_probe.aex` をその worktree で build していない |
| `test_rust_host_core_phase5` 〜 `phase8` | 4 | `CARGO_TARGET_DIR` を分離しているため `broker\target\release` に FFI dll (`aexcompat_host_core_ffi.dll`) が無い |
| worker stderr を読む test (`test_active_plugin_context` / `test_worker_effect_bootstrap_timing` など) | 環境次第 | cp932 locale で worker stderr の decode が `UnicodeDecodeError` になる |
| `build aex_l2_worker.exe before running the native test` などで落ちる native test 群 (テストが pre-#1495 の `aex_l2_worker.exe` / `aex_render_worker.exe` / `aex_smart_worker.exe` という個別ファイル名をまだ探している) | 2026-08-17 の文書のみ worktree で 5 | issue #1495 で worker は単一の `aex_worker.exe` に統合され、現行の CMake ビルドはこの 3 ファイル名を生成しない。ビルドし直しても解消せず、テスト側の追随が必要 |
| `missing self-test binary` で落ちる selftest 系 test | 同 worktree で 3 | selftest exe (`worker_*_selftest.exe`) を build していない。worker だけ build した worktree ではこちらだけ残る |
| `missing VS2022 worker` (`test_pf_parameter_animation_transport`) | 同 worktree で 1 | 別 build dir `target\minihost-build-v18\` の worker を build していない |

扱いは共通: 自分の diff がその test / 経路に非接触なことを確認し、PR body に
件数と内訳を明記し、CI を権威にする。fail が上の表に無い、または diff に接触する
なら環境要因扱いにしない。

---

この文書は実装状態と運用の記述で、方針の正本は `CLAUDE.md`。
