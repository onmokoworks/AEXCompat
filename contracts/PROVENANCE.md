# Contract Provenance

Promoted contracts are copied from `imports/aviutlas-rust-contracts/` into
canonical `contracts/` paths. The imported source files remain frozen
provenance and should not be edited during promotion.

## AEX Contracts

| Canonical path | Source path | Notes |
| --- | --- | --- |
| `contracts/aex/image_probe_request.schema.json` | `imports/aviutlas-rust-contracts/analysis/AEX_IMAGE_PROBE_REQUEST_SCHEMA_2026-05-31.json` | Request/response shape for future image probe work. Current use remains no-load contract validation. |
| `contracts/aex/render_parameter_request.schema.json` | Local cleanroom contract, 2026-07-13 | Strict caller value request resolved through a broker-owned descriptor profile; v4 adds bounded host-owned mask context. |
| `contracts/aex/parameter_descriptor_manifest.schema.json` | Local cleanroom contract, 2026-07-13 | Promoted L2 parameter observations bound to a reviewed plug-in digest and receipt. |
| `contracts/aex/descriptor_manifest_promotion_report.schema.json` | Local cleanroom contract, 2026-07-13 | No-native comparison evidence for regenerated and reviewed descriptor manifests. |
| `contracts/aex/render_parameter_gate_report.schema.json` | Local cleanroom contract, 2026-07-13 | Pre-dispatch decision report; this revision proves that no native process starts. |
| `contracts/aex/parameterized_classic_render_report.schema.json` | Local cleanroom contract, 2026-07-13 | Profile-neutral fixed-hash isolated execution report with descriptor-bound worker echoes. |
| `contracts/aex/parameterized_smartfx_render_report.schema.json` | Local cleanroom contract, 2026-07-13 | Profile-neutral Smart PreRender/Render report with descriptor-bound parameter and host-context echoes. |
| `contracts/aex/worker_capability_report.schema.json` | `imports/aviutlas-rust-contracts/analysis/AEX_WORKER_CAPABILITY_REPORT_SCHEMA_2026-05-31.json` | Capability report vocabulary for future worker results and compatibility claims. |
| `contracts/aex/loader_readiness_gate.schema.json` | `imports/aviutlas-rust-contracts/analysis/AEX_LOADER_READINESS_GATE_SCHEMA_2026-06-03.json` | Final readiness gate contract that keeps native loading closed. |
| `contracts/aex/image_probe_allowlist.example.json` | `imports/aviutlas-rust-contracts/analysis/AEX_IMAGE_PROBE_ALLOWLIST.example.json` | Sanitized example. Local absolute plugin paths from the source file are replaced with placeholders. |

## Promotion Rules

- Preserve source meaning and record the source path here.
- Keep `schema_version` or `schema_name` metadata intact when present.
- Sanitize local absolute paths before adding examples under `contracts/`.
- Keep native loading closed; these files are contracts, not approvals.

## Trace Contracts

| Canonical path | Source | Notes |
| --- | --- | --- |
| `contracts/trace/host_trace_event.schema.json` | `docs/PROJECT_DESIGN_2026-07-03.md`, Host Behavior Oracle design | Locally authored machine-readable event contract; not imported and contains no observed AE data. |
| `contracts/trace/host_trace_session.schema.json` | `docs/PROJECT_DESIGN_2026-07-03.md`, Host Behavior Oracle design | Session envelope for validating trace identity and completeness metadata. |
| `contracts/trace/conformance_rules.json` | `docs/PROJECT_DESIGN_2026-07-03.md`, Host Behavior Oracle design | Initial comparison policy for future AE-versus-minihost traces. |
| `contracts/trace/examples/synthetic_session.jsonl` | Locally generated synthetic example | Contains no native plug-in payload or measured After Effects behavior. |

## Compatibility Oracle Contract

| Canonical path | Source | Notes |
| --- | --- | --- |
| `contracts/aex/compat_oracle_report.schema.json` | `docs/IMPLEMENTATION_ROADMAP_2026-07-06.md`, A-8 | Locally authored statistics-only report contract; no pixel values or native payloads are serialized. |

## AEPX Edge Fixtures

Files under `tests/fixtures/aepx/` are byte-identical copies of the same-named
files under `imports/aviutlas-rust-contracts/aviutl-rs/tests/fixtures/`.
They cover BOM/CRLF, Unicode, duplicate IDs, ambiguous selections, multiple
compositions, scanner edges, and preservation sentinels. Tests enforce the
byte-for-byte provenance boundary; the imported originals remain unmodified.

## Broker Contract

| Canonical path | Source | Notes |
| --- | --- | --- |
| `contracts/broker/broker_selftest_report.schema.json` | `docs/IMPLEMENTATION_ROADMAP_2026-07-06.md`, B-1 | Locally authored process-isolation selftest contract using synthetic workers only. |
`aex/l1_worker_report.schema.json` is an AEXCompat-authored runtime contract
derived from the public Windows loader behavior boundary. It contains no Adobe
SDK declarations. Added 2026-07-13 for the cleanroom L1 worker.

`aex/classic_render_report.schema.json` is an AEXCompat-authored staged render
evidence contract. It requires deterministic double execution, pixel hashes,
buffer guards, and broker survival. It contains no Adobe SDK declarations.

`aex/smartfx_suite_fault_report.schema.json` is an AEXCompat-authored fault
conformance contract. It records only fixed fault ids, process classifications,
pixel hashes, guard status, and broker survival; it contains no Adobe SDK
declarations or native payloads.

`aex/smartfx_mask_scene_report.schema.json` is an AEXCompat-authored host-context
conformance contract. It records fixed scene identities, mask counts, independent
pixel hashes, guard status, and broker survival; it contains no Adobe SDK
declarations or native payloads.
