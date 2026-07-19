# PiPL entrypoint discovery evidence (2026-07-19)

Issue #84 replaces export-name guessing at the PF Effect boundary with the
Adobe PiPL contract. The worker enumerates bounded `PiPL` resources, parses the
Windows 10-byte list header and padded property records, requires `Kind`
(`AEEffect` / serialized `TKFe`) plus `CodeWin64X86` (`8664` / serialized
`4668`), validates a bounded ASCII export identifier, and only then resolves
that declared symbol. `AEGP` (`xgEA`) is never cast to the Effect ABI.

Malformed, truncated, oversized, duplicate, mixed Effect/AEGP, multi-Effect,
or otherwise ambiguous resources fail closed. The diagnostic classification
`invalid_pipl` is preserved through the broker and conformance report schema.
No raw PiPL bytes or private plug-in paths enter reports.

Local evidence on Windows x64:

- Native Release build: L1, L2, Classic render, and SmartFX workers built.
- Synthetic native self-test covers lowercase nonstandard Effect entrypoint,
  AEGP discrimination, hostile length, invalid identifier, and truncation.
- Installed `ntsc-rs-ae.aex`: PiPL `EffectMain`, 81 parameters inspected.
- Installed `OLMBlur.aex`: PiPL `entryPointFunc`, 5 parameters inspected.
- Installed `ColorKeep.aex`: PiPL `entryPointFunc`, inspection completed.

The Adobe SDK anchors are `Examples/Headers/SP/SPPiPL.h` for the property-list
and `CodeWin64X86` definitions, and `Examples/Resources/AE_General.r` for the
`AEEffect = 'eFKT'` and `AEGP = 'AEgx'` Kind values.
