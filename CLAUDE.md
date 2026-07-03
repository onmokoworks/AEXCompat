# AEXCompat Working Notes

This repository is a no-load AEX compatibility lab and staged host plan. It is
not a full After Effects emulator.

## Default Safety Rules

- Do not open, copy, hash, or load real `.aex` files unless a later reviewed
  native gate explicitly allows it.
- Do not call `EffectMain`, PF selectors, AE, OFX, or native plug-in entry
  points.
- Do not write `.aepx` or `.aep` projects.
- Do not edit `imports/`; it is frozen provenance from AviUtlas.
- Do not overwrite existing `target/` artifacts. Tools should be create-new
  only under their owned target subdirectory.
- Do not serialize private absolute paths, binary payloads, raw payloads, or
  private image contents into shareable reports.

## Useful Commands

```powershell
python -m unittest discover -s tests
python tools/contract_schema_validator.py contracts
```

## Project Direction

The next implementation track is:

1. Keep documentation and imported provenance navigable.
2. Promote selected imported contract documents into `contracts/`.
3. Validate contract documents with warning-first tooling.
4. Close the manual fixture provenance intake without opening native loading.
5. Build any future broker/worker stages behind the safety gate in
   `docs/PROJECT_DESIGN_2026-07-03.md`.
