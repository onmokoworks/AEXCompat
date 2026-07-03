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

