# PiPL entrypoint discovery + static AEX listing (2026-07-21)

Issue #84 replaces export-name guessing at the PF Effect boundary with the
Adobe PiPL contract, and adds a no-load static listing so an arbitrary AEX can
be enumerated by identity before any dispatch.

This is a reintegration of the reviewed implementation from the closed PR #87
branch (`codex/issue84-pipl-entrypoint`) onto the current, heavily refactored
`main` (worker `l2_main.cpp` shrank from ~25k to ~2.3k lines via TU
extraction), plus a new Python static parser for the listing use case.

## Worker discovery (native, load-based)

The worker resolves the Effect entrypoint from the plug-in's own PiPL instead
of a fixed `EffectMain` export name. It enumerates bounded `PiPL` resources,
parses the Windows 10-byte list header and padded property records, scopes
standard properties to Adobe's serialized `MIB8` vendor signature, requires
`Kind` (`AEEffect` / serialized `TKFe`) plus `CodeWin64X86` (`8664` /
serialized `4668`), validates a bounded ASCII export identifier, and only then
resolves that declared symbol. `AEGP` (`xgEA`) is never cast to the Effect ABI.
The former `GetProcAddress(module, "EffectMain")` lookup and the
`EntryPointFunc`-presence AEGP inference are removed from the worker dispatch.

Malformed, truncated, oversized, duplicate, mixed Effect/AEGP, multi-Effect, or
otherwise ambiguous resources fail closed. The classification `invalid_pipl` is
distinct from `aegp_candidate` and `unknown_no_effect_entrypoint`. No raw PiPL
bytes or private plug-in paths enter reports.

Implementation lives in `minihost/src/l2_main.cpp` (`parse_pipl_entrypoint`,
`discover_pipl_entrypoint`, and the `--self-test-pipl-entrypoint` self-test,
shared by the L2 / Classic-render / SmartFX workers through `worker_main_impl`).

## Static listing (no-load)

`tools/aex_pipl_identity.py` decodes the same PiPL byte layout directly out of
the PE resource section without loading the module, calling an entrypoint,
starting After Effects, or rendering. It additionally reads the identity
properties `name`, `catg`, `eMNA` (Match Name), `eVER`/`eSVR`/`ePVR`,
`eGLO`/`eGL2` for display. `tools/aex_list.py` prints a table (or JSON) for a
single `.aex` or a recursively scanned directory, with a fail-closed dispatch
classification that mirrors the worker's decision as a static approximation
(only `effect` is dispatchable; ambiguous/invalid resources are surfaced but
never treated as runnable).

```
uv run python tools/aex_list.py --input path/to/plugins
```

## Windows PiPL byte layout (confirmed)

10-byte header: little-endian u32 version (0/1), `[4..5]=0`, little-endian u16
count at `[6]`, `[8..9]=0`. Each property: vendor 4cc + key 4cc (both stored
little-endian, i.e. ASCII-reversed: `8BIM`→`MIB8`, `kind`→`dnik`,
`8664`→`4668`), u32 propertyID (0), u32 length, then `length` data bytes padded
to a 4-byte boundary (pad bytes must be zero). Kind and version values are
themselves little-endian. `AE_Effect_Version` packs
`vers<<19 | subvers<<15 | bugvers<<11 | stage<<9 | build` (verified against
Paramarama `1081345` = 2.1).

SDK anchors: `Examples/Resources/AE_General.r` (`AEEffect = 'eFKT'`,
`AEGP = 'AEgx'`, property keys) and `Examples/Headers/SP/SPPiPL.h`.

## Local evidence (Windows x64, 2026-07-21)

- Native Release build (VS 18 2026 / MSVC 19.51): L1, L2, Classic render, and
  SmartFX workers built at `target/minihost-build`.
- `--self-test-pipl-entrypoint` passes on all three effect workers: covers a
  lowercase nonstandard Effect entrypoint (`entryPointFunc`), AEGP
  discrimination, hostile length, invalid identifier, truncation, and an
  ignored non-Adobe vendor property.
- Static listing verified against SDK `ColorGrid` (Effect, `EffectMain`,
  3.3.0), `SDK_Backwards` (Effect, `EffectMain`, 1.5.0), and `Grabba`
  (Kind=AEGP, classified `aegp`, never `effect`).
- 14 machine-portable Python unit tests for the bounded, fail-closed decode.

## Real-corpus evidence: lowercase entrypoint discovery (issue #84 criterion 1)

Verified against a locally available OLM corpus (private plug-ins; only their
own public PiPL identity is recorded here, never paths or bytes). Both plug-ins
declare a Kind of AEEffect with a lowercase `entryPointFunc` CodeWin64X86 symbol
— exactly the case the old fixed-`EffectMain` lookup misclassified as an AEGP
candidate:

| plug-in (Match Name, version) | Kind | CodeWin64X86 | static class | worker `--inspect-experimental` |
|---|---|---|---|---|
| OLM Blur (`OLM OLM Blur`, 1.2.0) | AEEffect | `entryPointFunc` | effect | reaches PARAMS_SETUP; 5 params (float/integer) |
| OLM Color Key (`OLM Color Key`, 2.3.0) | AEEffect | `entryPointFunc` | effect | reaches PARAMS_SETUP; 223 params (color/float/group/integer) |

Both are discovered from PiPL as PF entrypoints and dispatch past GLOBAL_SETUP
into PARAMS_SETUP, confirming criterion 1 on a real third-party plug-in.

## Pending (corpus-gated) — tracked in #279

- Rowbyte Data Glitch / Fast Bokeh discovery (criterion 2) and the full 5-AEX
  Classic/SmartFX matrix rerun (criterion 6) require the Rowbyte corpus, which is
  not present on this machine (commercial plug-ins). For criterion 2 specifically,
  Rowbyte's uppercase `EntryPointFunc` is accepted by the same bounded symbol
  validation just verified for OLM, so its entrypoint discovery is expected to
  behave the same way. The criterion 6 matrix is left unpredicted: it exercises
  render/lifecycle paths beyond export-symbol discovery and can fail for unrelated
  reasons, so it must actually be run on the corpus. Tracked as follow-up in #279.
