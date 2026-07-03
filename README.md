# AEXCompat

AEXCompat is a no-load research lab and staged compatibility host plan for
After Effects `.aex` plug-in behavior.

It is not a full After Effects emulator. The current repository focuses on
static analysis, metadata-only gates, deterministic fixtures, local-only review
packets, and staged evidence for a future isolated native probe.

## Start Here

- [Project Design](docs/PROJECT_DESIGN_2026-07-03.md) explains the target
  architecture, migration plan, safety gate, and implementation roadmap.
- [Docs Index](docs/README.md) describes the documentation layout.
- [Imported AviUtlas Contracts](imports/aviutlas-rust-contracts/README.md)
  preserves AEX/AEPX/OFX planning assets copied from AviUtlas as provenance.
- [Contract Provenance](contracts/PROVENANCE.md) tracks promoted contract
  documents and their source files.

## Safety Boundary

By default, this lab must not:

- open, copy, hash, or load real `.aex` files;
- call `EffectMain`, PF selectors, AE, OFX, or native plug-in entry points;
- write `.aepx` or `.aep` projects;
- overwrite existing `target/` artifacts;
- publish private paths, binary payloads, raw payloads, or private image data.

Native loading remains blocked until the staged safety gate in the project
design is satisfied and explicitly reviewed.

## Repository Layout

- `tools/`: Python no-load analysis, gate, fixture, worker, and audit tools.
- `tests/`: unittest coverage for no-load contracts and fail-closed behavior.
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
