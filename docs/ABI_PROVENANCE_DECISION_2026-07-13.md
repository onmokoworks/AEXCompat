# ABI Provenance Decision (2026-07-13)

Decision owner: repository owner, via direct Codex task response.

Decision: **public-document cleanroom for `minihost/`**.

The owner prefers cleanroom implementation and permits SDK use only where its
terms allow. AEXCompat therefore adopts the following hard boundary:

1. `minihost/` ABI definitions and behavior are derived from public
   documentation, independently authored contracts, synthetic fixtures, and
   black-box traces captured by human-operated instruments.
2. Adobe SDK headers and source may be read only by the instrument/build side
   under `instruments/`; they may not be copied, included, translated, or used
   as source material in `minihost/`.
3. Instrument output may cross the boundary only after A-5 redaction and A-4
   schema validation. It contains observed behavior and numeric metadata, not
   SDK source, header text, private paths, pointers, or payloads.
4. SDK-dependent plug-ins may be distributed only as permitted object code.
   SDK headers/source are never committed or redistributed by this repository.
5. Any future proposal to use SDK-derived ABI declarations in `minihost/`
   requires a new dated human/legal decision and cannot silently weaken this
   boundary.

This satisfies the H-3 provenance choice but does not open the Safety Gate.
The existing native-code guard continues to enforce the directory boundary.
