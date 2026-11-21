# macOS `.plugin` の調査（2026-11-21、#1847）

Windows の AEXCompat で Mac 用の `.plugin`（Mach-O）を読み込んで描画できるかを考えるため、手元の After Effects 2026 と共通プラグインフォルダーにある `.plugin` の中身を数えた。プラグインを実行はしていない。集計は `tools/macho_plugin_survey.py` で取り、名前・パス・ハッシュは出していない。

```sh
python3 -I tools/macho_plugin_survey.py "/Applications/Adobe After Effects 2026/Plug-ins" --json
python3 -I tools/macho_plugin_survey.py "/Library/Application Support/Adobe/Common/Plug-ins" --json
```

## 結果

| 項目 | AE 2026 同梱（359） | 共通フォルダーのサードパーティ製（6） |
|---|---:|---:|
| x86_64 スライスあり | 359（全部 arm64 との universal） | 3（2 は arm64 だけ、1 は実行ファイルなし） |
| `Contents/Resources/*.rsrc`（PiPL）あり | 139 | 3 |
| `EffectMain` などの名前付き入口を export | 64 | 3 |
| `LC_DYLD_CHAINED_FIXUPS` | 254 | 0（3 とも `LC_DYLD_INFO_ONLY`） |
| 初期化子（`__mod_init_func` / `__init_offsets`） | 143 | 1 |
| スレッドローカル（`__thread_vars`） | 13 | 2 |
| `__cxa_throw` を import | 355 | 1 |
| `objc_msgSend` を import | 16 | 3 |
| `dispatch_*` を import | 13 | 3 |
| 未定義シンボル数 | 25 以下 80、26〜100 199、101 以上 80 | 98、180、212 |

AE 同梱のエフェクトの多くは、PiPL を `.rsrc` に持たず `EffectMainExtra` などの独自の入口を出している。また `@rpath` で AE 内部のライブラリを読み込む（多い順に `dvacore` 254、`libtbb` 232、`boost_thread` 228、`AdobeARE` 228、`U.dylib` 170、`PF.dylib` 92）。AE の外で読み込む前提で作られていないので、対象から外す。

## 分かったこと

- 対象はサードパーティ製の SDK プラグインになる。ただし手元で x86_64 を持つものは 3 本しかなく、何を実装すればよいかを決める標本としては足りない
- その 3 本でも、AppKit・Foundation・CoreFoundation を全部リンクし、`objc_msgSend` と `dispatch_*` を import している。設計メモ（下記）が後回しにした Objective-C と dispatch は、少なくとも import の解決としては最初から要る。リンクしているだけで描画中には呼ばない可能性はあるが、それは実行して確かめるしかない
- C++ 例外（`__cxa_throw`、`__gxx_personality_v0`、`_Unwind_Resume`）は、同梱でもサードパーティ製でもほぼ全部が import している。Windows の guest のように「投げられたら明示的に失敗」で始められるが、投げる AEX は多いと見ておく
- fixup の形式は 2 通りある。新しいリンカーの出力は chained fixups（同梱の 71%）、古いものは `LC_DYLD_INFO_ONLY`（サードパーティ製 3 本すべて）。最初からどちらも読む必要がある
- AE 2026 の Mac 版は universal なので、x86_64 スライスを選んで Unicorn の x86_64 で動かす道は残っている。arm64 だけのサードパーティ製も既に 2 本あり、いずれ arm64 の guest が要る

## 次にすること

1. SDK のサンプル（Skeleton など）を Mac でビルドし、import と load command を同じスクリプトで数える。依存が libSystem と libc++ だけで済むなら、最初の 1 本にする
2. サードパーティ製の標本を増やす。手元にない市販品は、持っている人に `tools/macho_plugin_survey.py` を回してもらい、集計の JSON だけを受け取る
3. 設計は scratchpad の設計メモを元に別 Issue に起こす。要点は、PE の読み込みと分けた Mach-O の読み込み、System V の呼び出し規約、Darwin の import 表（libSystem / libc++ / CoreFoundation から、実際に呼ばれたものだけ）、PiPL の `mi64` / `ma64` から入口を引くこと
