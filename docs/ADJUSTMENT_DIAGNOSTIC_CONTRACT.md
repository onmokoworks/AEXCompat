# Direct / Adjustment diagnostic contract

Issue #420 の診断は、Issue #4 の `tools/run-conformance-bundle.py` を一回の
render runnerとして再利用する。`tools/run-adjustment-diagnostic.py` は同じ
条件で `direct` と `adjustment` の2 bundleを生成し、bundle-local raw artifact
だけを比較する orchestration 層である。Adjustment Layer のruntime意味論を
この診断層で推測したり修正したりしない。

## Manifest and standard scenes

入力は `schemas/adjustment-diagnostic-manifest.schema.json` に従う。manifest は
AEX、依存DLL、runner、2つの独立した RGBA source artifact、time、typed
parameters、resolution、depth、renderer、downsample、alpha policy を固定する。

runner は次の2 sceneを同じ生成経路で作る。

- `opaque_full_frame`: 不透明な全面 source と effect/adjustment layer。
- `alpha_extent_roi`: alpha/transparent region を持つ source、ROI/extent を持つ
  foreground、effect/adjustment layer。

各 scene は composited PNG を再生成し、`inputs/` に保存する。pair report の
`layer_stacks.direct` と `layer_stacks.adjustment` は、同じ order/extent/ROI を
保ったまま effect layer の kind/flags だけを application mode に合わせて
固定する。

## Evidence and classification

各経路の conformance report は `pairs/<scene>-<mode>/` に保存し、失敗結果を
成功へ丸めない。pair report は次を保持する。

- AEX/parameter/time/resolution/depth/renderer/alpha/downsample の common identity
- layer order、flags、coverage、extent、ROI
- selector、Suite timeline、session mode、worker identity
- composited PNG の SHA-256 と、#4 runner の pixel-depth conversion で再計算した
  expected raw input。Adjustment 経路の `raw_input` と hash/size を比較する。
- direct/adjustment の raw output、pixel mismatch、alpha mismatch、
  premultiplication/world mismatch

classification は排他的に `equivalent`、`pixel_diverged`、
`direct_ok_adjustment_failed`、`direct_failed_adjustment_ok`、`both_failed`、
`identity_mismatch`、`blocked_external` のいずれかである。原因候補は事実の
断定ではなく、後続 Issue を切り出すための bounded candidates である。

`--adapter-command` は focused test 用の非権威経路で、report の
`execution_evidence` は `adapter` とする。これは実AEX成功を意味しない。
scene-capable な外部実AEX実行環境が無い通常実行は `blocked_external` とし、
再開条件は独立fixture AEXとscene-aware worker/AE実行が利用可能になることで
ある。GPU/UI/runtimeの修正は本契約の範囲外で、診断結果から #25/#26/#29/#98
または独立原因Issueへ分離する。

## Commands

```powershell
$env:PYTHONUTF8 = "1"
python tools\run-adjustment-diagnostic.py `
  --manifest <diagnostic-manifest.json> `
  --out <new-diagnostic-bundle>
```

Focused adapter validation is run with `--adapter-command`; its output is useful for
schema, hash, diff, and identity tests only. A native worker build and real AEX run
are separate gates and must be recorded as native evidence only when they actually
execute the pinned AEX and both application modes.
