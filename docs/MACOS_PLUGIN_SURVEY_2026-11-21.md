# macOS `.plugin` の調査（2026-11-21、#1847）

Windows の AEXCompat で Mac 用の `.plugin`（Mach-O）を読み込んで描画できるかを考えるため、手元の After Effects 2026 と共通プラグインフォルダーにある `.plugin` の中身を数えた。プラグインは実行していない。数字はすべて `tools/macho_plugin_survey.py` の出力で、名前・パス・ハッシュは出していない。

```sh
python3 -I tools/macho_plugin_survey.py "/Applications/Adobe After Effects 2026/Plug-ins" --json --detail
python3 -I tools/macho_plugin_survey.py "/Library/Application Support/Adobe/Common/Plug-ins" --json
```

## 結果

| 項目 | AE 2026 同梱（359） | 共通フォルダーのサードパーティ製（6） |
|---|---:|---:|
| x86_64 スライスあり | 359（全部 arm64 との universal） | 3（2 は arm64 だけ、1 は実行ファイルなし） |
| 集計できた数（x86_64 あり、ツールの失敗を除く） | 358（otool の失敗 1） | 3 |
| Mach-O の種類 | BUNDLE 358 | BUNDLE 1、DYLIB 2 |
| PiPL あり（`.rsrc` の中に `PiPL`） | 137（`.rsrc` はあるが PiPL なし 2、見つからない 220） | 3 |
| `EffectMain` / `EntryPointFunc` / `PluginMain` を export | 64 | 3 |
| `LC_DYLD_CHAINED_FIXUPS` | 254 | 0（3 とも `LC_DYLD_INFO_ONLY`） |
| 初期化子（`__mod_init_func` / `__init_offsets`） | 143 | 1 |
| スレッドローカル（`__thread_vars`） | 13 | 2 |
| `__cxa_throw` / `__gxx_personality_v0` を import | 354 / 354 | 1 / 1 |
| `_Unwind_Resume` を import | 354 | 3 |
| `objc_msgSend` を import | 16 | 3 |
| `dispatch_*` を import | 13 | 3 |
| 未定義シンボル数 | 25 以下 80、26〜100 199、101〜400 60、400 超 19 | 51〜100 1、101〜200 1、201〜400 1 |

同梱のエフェクトで PiPL が見つからない 220 個は、`EffectMainExtra`（106）、`FilterMain`（91）、`MainEntry`（76）などの独自の入口を export している（`--detail`）。さらに `@rpath` で AE 内部のライブラリを読み込む。多い順に `dvacore` 254、`libtbb` 232、`AdobeARE` 228、`boost_thread` 228、`U.dylib` 170、`PF.dylib` 92。AE の外で読み込む前提で作られていないので、対象から外す。

## 分かったこと

- 対象はサードパーティ製の SDK プラグインになる。ただし手元で x86_64 を持つものは 3 本しかなく、何を実装すればよいかを決める標本としては足りない
- その 3 本は AppKit・Foundation・CoreFoundation をリンクし、`objc_msgSend` と `dispatch_*` を import している。import の解決（最初は呼ばれたら明示的に失敗するトラップで足りる）は最初から要る。描画中に実際に呼ばれて実装が要るかどうかは、実行して確かめる
- C++ 例外の関数は、同梱ではほぼ全部（354/358）が import している。サードパーティ製では `__cxa_throw` が 1/3 で、`_Unwind_Resume` は 3/3。Windows の guest と同じく、投げられたら明示的に失敗する形で始められる
- fixup の形式は 2 通りある。新しいリンカーの出力は chained fixups（同梱の 71%）、古いものは `LC_DYLD_INFO_ONLY`（サードパーティ製 3 本すべて）。最初からどちらも読む必要がある
- `.plugin` の実行ファイルは `MH_BUNDLE` とは限らない。サードパーティ製 3 本のうち 2 本は `MH_DYLIB` だったので、読み込みは両方を受け付ける
- AE 2026 の Mac 版は universal なので、x86_64 スライスを選んで Unicorn の x86_64 で動かす道は残っている。arm64 だけのサードパーティ製も既に 2 本あり、いずれ arm64 の guest が要る

## 次にすること

1. SDK のサンプル（Skeleton など）を Mac でビルドし、同じスクリプトで数える。依存が libSystem と libc++ だけで済むなら、最初の 1 本にする
2. サードパーティ製の標本を増やす。手元にない市販品は、持っている人にこのスクリプトを回してもらい、集計の JSON だけを受け取る
3. 設計は別 Issue に起こす。骨子は次のとおり
   - PE とは別の Mach-O の読み込み（universal のスライス選択、`MH_BUNDLE` と `MH_DYLIB`、2 通りの fixup、初期化子、TLV）
   - Win64 とは別の System V の呼び出し規約（引数のレジスター、home 領域なし、red zone）
   - 名前で引く Darwin の import 表（libSystem / libc++ / CoreFoundation / libobjc。実際に呼ばれたものから実装し、ほかは明示的に失敗するトラップ）
   - PiPL の `mi64` / `ma64` から入口を引く
