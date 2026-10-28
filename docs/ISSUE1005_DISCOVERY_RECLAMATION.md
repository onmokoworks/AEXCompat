# #1005: 共有 discovery の handle 回収境界

## 事実

起点 `dbf836f6427cab445c03298963765b91bb50da8a` の Release worker で、以下の3本だけを
shipping `discover_records_for_diagnostics` に渡した。configured corpus の全体走査ではない。
discovery はパラメーター検査のみ（深度中立、frame render なし）。AE は起動・操作していない。

|AEX|SHA-256|パラメーター数|
|---|---|---:|
|OLMToonDilate|c05db8c118029ff3216d3cae8e6423e2eb41ca8f56de2fb3668db81b9b8c32b3|1|
|OLMRadialBlur|ffbb1d0109671e3ea9b1a12cd1126f2c72f965197577a57cc602fb096414ccdb|30|
|OLMColorKey|9c6cca226a52d35ce7833fcc4c0f914f6b15b3abe0202e0957ba97ba3bb2cf2c|223|

修正前 worker SHA-256: `d338f087da407dbdeb94733dc018177388badeb20d8eedaa4c7f95064d654722`。
3本とも表面の `ok=true` の内側に `cleanup_crash_contained` があった。
直接共有 session は先頭 ToonDilate の checkpoint 後に `0xC0000374` で終了し、後続2要求が
invalidated。一方、同じ worker の通常 one-shot discovery は全3本が exit 0、
`GLOBAL_SETDOWN=0` で終了した。

`8bdfde1d` が通常 one-shot/render に追加した `HandleReclamationScope` は、共有 discovery の
別入口を覆っていなかった。その既存スコープを共有 session に適用した結果、全3本が
`ok=true / failure_diagnostics=null / cluster_fallback=null`。直接共有 session も
`inspects_ok=3 / inspects_errored=0 / session_clean=true` となった。パラメーター数は不変。

## 変更と検証境界

スコープは最初の setup 前から swap・再検査をまたぎ、最後の GLOBAL_SETDOWN と COR/BIB teardown
の後に回収する。論理的な dispose、stale/foreign handle の拒否、live+quarantined の既存個数・容量上限を
変更しない。プラグイン名・hash による分岐、selector 省略、crash の成功扱いは追加しない。

SDK 不要の fixture は実際の PF Handle Suite を呼び、4要求で既存の quarantine 個数予算を満たす。
swap・swap-back・同一 member 再検査を含み、GLOBAL_SETDOWN で破棄済み handle が拒否されること、
5要求目が割当失敗になること、失敗を報告した session が正常終了できることを検査する。
解放済み backing の読み取りや allocator のアドレス再利用には依存しない。
スコープを除去すると5要求目が誤って成功し、テストは exit 101 で失敗した。
既存 `worker_handle_runtime_selftest` は物理解放、容量予算、locked/stale/foreign handle 境界を検査する。

```powershell
& tools/build-native.ps1 -Target @('aex_worker','worker_handle_runtime_selftest')
& target/minihost-build/worker_handle_runtime_selftest.exe
cargo test --manifest-path broker/Cargo.toml -p aexcompat-broker --test discovery_session_native discovery_reclamation -- --nocapture
cargo test --manifest-path broker/Cargo.toml -p aexcompat-broker --test trusted_worker_stage
```

推論: 今回の比較は共有 discovery の回収境界欠落がこの3本の現行 crash の原因であることを支持する。
独自の AE lifecycle 契約やプラグイン内部の誤り全般を確定するものではない。
画像を比較していないため、この結果単独では AE 画素等価性を主張しない。
