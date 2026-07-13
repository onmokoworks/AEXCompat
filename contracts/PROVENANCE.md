# Contract Provenance

Promoted contracts are copied from `imports/aviutlas-rust-contracts/` into
canonical `contracts/` paths. The imported source files remain frozen
provenance and should not be edited during promotion.

## AEX Contracts

| Canonical path | Source path | Notes |
| --- | --- | --- |
| `contracts/aex/image_probe_request.schema.json` | `imports/aviutlas-rust-contracts/analysis/AEX_IMAGE_PROBE_REQUEST_SCHEMA_2026-05-31.json` | Request/response shape for future image probe work. Current use remains no-load contract validation. |
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
