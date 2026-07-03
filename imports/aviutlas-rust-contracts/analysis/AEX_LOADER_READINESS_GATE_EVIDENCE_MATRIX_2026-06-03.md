# AEX Loader Readiness Gate Evidence Matrix - 2026-06-03

Purpose: summarize what the current no-load AEX loader readiness gate proves,
what it does not prove, and what remains blocked until Native Oracle/operator
approval. This is a local-only analysis artifact.

## Inputs Reviewed

- `analysis/AVIUTLAS_DEVELOPMENT_CHAT_HANDOFF_2026-05-31.md`
- `analysis/AEX_LOADER_READINESS_GATE_SCHEMA_2026-06-03.json`
- `aviutl-rs/examples/aex_loader_readiness_gate.rs`
- `aviutl-rs/tests/aex_loader_readiness_gate_contract.rs`
- `analysis/AEX_LOADER_PREFLIGHT_REPORT_SCHEMA_2026-06-01.json`
- `analysis/AEX_NO_LOAD_PROVENANCE_AUDIT_SCHEMA_2026-06-01.json`
- `analysis/AEX_PROBE_FIXTURE_IDENTITY_SMOKE_SCHEMA_2026-06-01.json`
- `analysis/AEX_LOADER_SLICE_REVIEW_SCHEMA_2026-06-01.json`
- `analysis/AEX_LOADER_APPROVAL_RECEIPT_SCHEMA_2026-06-01.json`

## Evidence Matrix

| Item | Classification | Evidence | Current meaning |
| --- | --- | --- | --- |
| Final readiness gate report shape | Merged/Ready | `analysis/AEX_LOADER_READINESS_GATE_SCHEMA_2026-06-03.json`; `aviutl-rs/examples/aex_loader_readiness_gate.rs`; `aviutl-rs/tests/aex_loader_readiness_gate_contract.rs` | The gate can emit a schema-versioned local-only JSON report with required summaries, checks, notes, and statuses. |
| Complete no-load evidence status | Measured | `aviutl-rs/examples/aex_loader_readiness_gate.rs` sets `status=loader_readiness_evidence_complete_gate_closed` only when all five checks pass; `aviutl-rs/tests/aex_loader_readiness_gate_contract.rs` covers ready and blocked cases. | The current gate can prove that required metadata evidence is present and internally consistent enough to close the final no-load readiness check. |
| Gate remains closed after evidence completion | Measured | `analysis/AEX_LOADER_READINESS_GATE_SCHEMA_2026-06-03.json` requires `final_gate_closed=true`, `may_load_aex=false`, `native_load_performed=false`, `selectors_executed=false`, and `render_performed=false`; the contract asserts these values. | "Evidence complete" does not unlock loading. The final readiness output is intentionally a closed gate. |
| Loader preflight no-load invariant | Measured | `analysis/AEX_LOADER_PREFLIGHT_REPORT_SCHEMA_2026-06-01.json`; `aviutl-rs/examples/aex_loader_readiness_gate.rs` checks `preflight_passed`, `native_load_performed=false`, `broker_may_load_plugin=false`, selected fixture presence, and selected loader entry presence. | The gate proves that the consumed preflight report claims a selected fixture and loader entry while still forbidding broker/native plug-in loading. |
| Provenance chain no-load invariant | Measured | `analysis/AEX_NO_LOAD_PROVENANCE_AUDIT_SCHEMA_2026-06-01.json`; `aviutl-rs/examples/aex_loader_readiness_gate.rs` checks `status=no_load_provenance_chain_ready`, `native_load_performed=false`, `selectors_executed=false`, `render_performed=false`, and `ofx_route_allowed=false`. | The gate proves that the consumed provenance audit reports a ready metadata chain while preserving no-load, no-selector, no-render, and no-OFX-route boundaries. |
| Forbidden-token contamination check | Measured | `analysis/AEX_LOADER_READINESS_GATE_SCHEMA_2026-06-03.json` lists forbidden source tokens; `aviutl-rs/examples/aex_loader_readiness_gate.rs` blocks when provenance says `evidence_contains_forbidden_tokens=true`. | The gate depends on upstream provenance token scanning and refuses a report that declares contaminated evidence. |
| Fixture identity smoke presence | Measured | `analysis/AEX_PROBE_FIXTURE_IDENTITY_SMOKE_SCHEMA_2026-06-01.json`; `analysis/AEX_NO_LOAD_PROVENANCE_AUDIT_SCHEMA_2026-06-01.json`; `aviutl-rs/examples/aex_loader_readiness_gate.rs` requires `fixture_identity_smoke_summary.provided=true`. | The gate proves that sanitized fixture identity smoke evidence was provided through provenance. It does not independently re-run the smoke. |
| Synthetic identity transport | Implemented but Approx | `analysis/AEX_PROBE_FIXTURE_IDENTITY_SMOKE_SCHEMA_2026-06-01.json` defines `identity_transport`, `rgba8`, three synthetic images, `broker_invoked=true`, and `aex_render_correctness_evidence=false`. | This is broker/synthetic PNG identity transport evidence only. It is useful readiness plumbing, not real AEX rendering or effect correctness. |
| Loader slice review packet relationship | Measured | `analysis/AEX_LOADER_SLICE_REVIEW_SCHEMA_2026-06-01.json` requires a sanitized no-load handoff packet with `loader_slice_approved=false`, `loader_enabled=false`, `broker_may_load_aex=false`, `render_performed=false`, and `ofx_route_allowed=false`. | The current readiness gate can support a later manual review packet, but the packet itself remains no-load and non-approving. |
| Manual approval receipt boundary | Blocked by Native Oracle | `analysis/AEX_LOADER_APPROVAL_RECEIPT_SCHEMA_2026-06-01.json` says an accepted receipt approves only a separate loader implementation review slice and still requires `allow_native_aex_load=false`, `allow_worker_plugin_load=false`, `allow_render_png=false`, and `allow_ofx_route=false`. | Operator approval is still required before any separate loader implementation review slice, and even that approval does not permit actual native AEX loading. |
| Native AEX loading | Blocked by Native Oracle | `analysis/AEX_LOADER_READINESS_GATE_SCHEMA_2026-06-03.json`; `analysis/AEX_LOADER_PREFLIGHT_REPORT_SCHEMA_2026-06-01.json`; `analysis/AEX_NO_LOAD_PROVENANCE_AUDIT_SCHEMA_2026-06-01.json`; `analysis/AEX_LOADER_APPROVAL_RECEIPT_SCHEMA_2026-06-01.json` all require no-load values. | No `.aex` file may be opened, hashed, copied, loaded, executed, described, or rendered by this gate. |
| Selector execution and pixel rendering | Blocked by Native Oracle | `analysis/AEX_LOADER_READINESS_GATE_SCHEMA_2026-06-03.json` and `analysis/AEX_NO_LOAD_PROVENANCE_AUDIT_SCHEMA_2026-06-01.json` require `selectors_executed=false` and `render_performed=false`; readiness notes say this is not selector, render, OFX route, or pixel parity evidence. | No PF selector behavior, render callback behavior, framebuffer correctness, or pixel parity is proven. |
| OFX route to AEX | Blocked by Native Oracle | `analysis/AEX_NO_LOAD_PROVENANCE_AUDIT_SCHEMA_2026-06-01.json` requires `ofx_route_allowed=false`; `analysis/AEX_LOADER_SLICE_REVIEW_SCHEMA_2026-06-01.json` keeps `ofx_route_allowed=false`; `analysis/AEX_LOADER_APPROVAL_RECEIPT_SCHEMA_2026-06-01.json` keeps `allow_ofx_route=false`. | AviUtlas may not route through OFX to reach AEX from this readiness evidence. |
| Adobe SDK/native ABI implementation | Blocked by Native Oracle | `analysis/AEX_LOADER_APPROVAL_RECEIPT_SCHEMA_2026-06-01.json` keeps `allow_aex_sdk_or_abi_import=false`; the readiness gate only consumes JSON summaries. | No SDK headers, ABI layouts, `EffectMain` calls, or loader implementation facts are approved or proven here. |

## What The Gate Proves

- `Merged/Ready`: the final readiness gate artifact/schema/test lane exists and
  is contract-guarded as a local-only metadata report.
- `Measured`: given preflight and provenance JSON, the gate checks required
  no-load evidence, blocks missing/promoted evidence, and preserves
  `final_gate_closed=true` plus `may_load_aex=false`.
- `Measured`: the report can distinguish a complete no-load evidence chain from
  a blocked no-load evidence chain without enabling native execution.

## What The Gate Does Not Prove

- It does not prove that an AEX binary can be loaded safely or correctly.
- It does not prove selector dispatch, `render_png`, parameter discovery,
  framebuffer correctness, or pixel parity.
- It does not prove OFX-to-AEX routing.
- It does not replace local operator approval, code review, license review,
  cleanroom review, worker isolation review, or Native Oracle evidence.
- It does not make private paths, AEX payloads, binaries, hashes, rendered
  pixels, or SDK/ABI-derived details publishable.

## Blocked By Native Oracle / Operator Approval

These items remain `Blocked by Native Oracle`:

- Any actual `.aex` open/hash/copy/load/execute/describe/render operation.
- Any worker plug-in load or broker permission to call native loader APIs.
- Any PF selector execution or pixel-buffer exchange with a native effect.
- Any AEX render correctness or pixel parity claim.
- Any OFX facade route that reaches an AEX loader.
- Any import of Adobe SDK headers, native ABI layouts, or native loader symbols.
- Any promotion from no-load readiness evidence to implementation approval
  without a separate reviewed approval receipt and operator decision.

## Classification Boundary

- `Merged/Ready` means the local schema/example/test readiness-gate lane exists.
- `Measured` means a contract or schema pins the no-load metadata behavior.
- `Implemented but Approx` means the current implementation exercises synthetic
  or broker-only readiness plumbing, not native AEX behavior.
- `Blocked by Native Oracle` means the claim requires real native/oracle
  evidence and explicit approval outside the current no-load gate.
