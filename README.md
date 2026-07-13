# AEXCompat

AEXCompat is a cleanroom compatibility lab and staged isolated host for After
Effects `.aex` plug-in behavior.

The project is working toward faithful observable host compatibility rather
than emulating the whole After Effects application. It currently includes
static analysis, safety gates, an isolated native broker, cleanroom ABI workers,
parameter discovery, classic rendering, and SmartFX rendering.

## Start Here

- [Project Design](docs/PROJECT_DESIGN_2026-07-03.md) explains the target
  architecture, migration plan, safety gate, and implementation roadmap.
- [Docs Index](docs/README.md) describes the documentation layout.
- [Imported AviUtlas Contracts](imports/aviutlas-rust-contracts/README.md)
  preserves AEX/AEPX/OFX planning assets copied from AviUtlas as provenance.
- [Contract Provenance](contracts/PROVENANCE.md) tracks promoted contract
  documents and their source files.

## Safety Boundary

Unapproved inputs must not:

- load arbitrary `.aex` files or accept caller-controlled plug-in paths;
- call native entry points outside a fixed, hash-checked, stage-approved broker route;
- write `.aepx` or `.aep` projects;
- overwrite existing `target/` artifacts;
- publish private paths, binary payloads, raw payloads, or private image data.

The self-authored ScatterMap fixture has passed the staged gate through classic
and SmartFX ARGB8 rendering. Every native run remains local-only, hash- and
size-bound, timeout-limited, Job Object-isolated, and create-new for evidence.

## Repository Layout

- `tools/`: Python analysis, gate, fixture, oracle, and audit tools.
- `tests/`: unittest coverage for contracts, boundaries, and fail-closed behavior.
- `broker/`: Rust allowlist enforcement and Windows process isolation.
- `minihost/`: cleanroom C++ ABI workers for load, setup, classic, and SmartFX calls.
- `analysis/`: historical run logs and planning documents.
- `contracts/`: promoted local contract documents used by validators and future
  broker/worker reports.
- `imports/`: frozen provenance imports from AviUtlas. Treat these as read-only.
- `target/`: ignored local artifacts. Tools should create new files only.

## Common Commands

```powershell
python -m unittest discover -s tests
python tools/contract_schema_validator.py contracts
```
