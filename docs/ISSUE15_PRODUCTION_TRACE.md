# Issue #15: Production Worker Trace

## 目的

`aex_worker.exe`は、discovery/classic/smartのどの`--kind`route実行でも、
`AEX_INSTRUMENT_TRACE_DIR`が設定されたbroker起動時だけ、既存のtrace event schemaに従うJSONLを出力します。環境変数がない通常実行ではwriterを有効化せず、追加のファイルI/OやJSON生成を行いません。

出力にはselector名、suite名・version・grant結果だけを含め、AEXのバイト列、ピクセル、ポインタ、private absolute pathは含めません。イベント数と各フィールド長にも上限があります。

## broker管理root

brokerは`AEX_INSTRUMENT_TRACE_DIR`を`target/worker-traces`配下の既存または新規ディレクトリに限定します。相対パス、`..`、root外、symlink/reparse pointはworker起動前に拒否されます。

workerへdirectory pathは渡しません。brokerがlaunchごとにcreate-newしたファイルを最終handle pathで再認証し、そのhandleだけを`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`と専用の子process環境で継承させます。workerは`AEX_INSTRUMENT_TRACE_HANDLE`を使用し、pathの再解決やファイルopenを行いません。handle継承・型検査に失敗した場合はnative load前にfail-closedで終了します。

PowerShellの例:

```powershell
$env:AEX_INSTRUMENT_TRACE_DIR = (Resolve-Path .\target\worker-traces).Path
```

brokerは必要な`target\worker-traces`を作成します。実行後のJSONLはそのroot内で扱い、外部ディレクトリを指定してworkerを直接起動する運用はbrokerのroot制約の対象外です。

## 再ビルドとtrust identity

この変更は単一の`aex_worker.exe`のリンク内容を変えるため、既存のローカルtrust receipt/allowlistを使用する場合は、承認済みのビルド手順でその実行ファイルを再ビルドし、サイズとSHA-256を再生成してください。生成した`.exe`、allowlist、receiptはローカル生成物であり、リポジトリへ追加しません。

```powershell
pwsh -File tools\build-native.ps1

Get-FileHash target\minihost-build\aex_worker.exe -Algorithm SHA256
```

trust identityの更新は、各環境で既存の承認・refresh手順を通して行います。ハッシュをソースへ手書きしたり、ビルド成果物をcommitしたりしないでください。
