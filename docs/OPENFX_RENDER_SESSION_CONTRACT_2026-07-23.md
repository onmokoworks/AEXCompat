# OpenFX -> RenderSession 共通契約

Issue #385 の共通部分として、Blender/DaVinci 固有のコードから分離した
host-neutral な frame 契約を `contracts/openfx/render_session_bridge.schema.json`
に固定する。

## 対応関係

| 共通契約 | broker `RenderSession` |
| --- | --- |
| `session_open.plugin.relative_path` + `sha256` | `SessionOpenRequest.plugin_path` + `plugin_sha256`。実パス解決と trust admission は broker 側で行う |
| `session_open.worker.sha256` | `RenderSession` が起動する authenticated Render worker の identity |
| `geometry` / `time` | `SessionOpenRequest.width`, `height`, `time_step`, `total_time`, `time_scale` |
| `frame_exchange.request.input` | `render_frame_with_parameters(frame_index, current_time, rgba, ...)` の RGBA8入力 |
| `frame_exchange.response.output` | `FrameStatus::Rendered { pixels, checksum, width, height }` |
| `response.identity` | plugin/worker SHAの再確認。違えば `identity_mismatch` |

## 第一slice

- pixel format: RGBA8
- channel order: RGBA
- alpha: straight または premultiplied を明示
- row-major、rowbytesは1行のバイト数として検証
- payloadはbase64で運ぶが、sha256と `rowbytes * height` を必ず照合
- frame timeは `current_time` と `time_scale` を分離して保持

出力は同じ寸法に限定していない。`RenderSession`のshrink/expand結果を表現できるよう、
requestとresponseのframe geometryは個別に検証する。ただし各frameは4096x4096、
64MiB相当のbounded transportを超えない。

## 失敗分類

`unsupported`、`timeout`、`worker_crash`、`identity_mismatch`、`protocol_error` は
いずれもfail-closedで、outputを持たない。rendered responseだけがoutputとidentityを
持ち、健康な `close.status=closed` を許される。

この契約はhost adapterとbroker APIの境界を固定するものであり、実AEXのロード、
実Blender/Resolveでのdiscovery、実pixel renderを成功扱いにはしない。#387/#388は
それぞれのhost固有artifactからこの共通契約へ接続する。
