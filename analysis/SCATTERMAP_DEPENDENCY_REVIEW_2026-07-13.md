# ScatterMap Dependency Review (2026-07-13)

Status: **candidate dependencies clear for isolated L1 loading**.

Reviewed identity:

- byte size: `201216`;
- SHA-256: `223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB`;
- PE type: x64 DLL (`PE32+`);
- inspection method: MSVC `dumpbin /headers /dependents` (no DLL load).

The import table contains only Windows system/API-set libraries and the release
Visual C++ runtime: `kernel32.dll`, `api-ms-win-core-synch-l1-2-0.dll`,
`bcryptprimitives.dll`, `ntdll.dll`, `VCRUNTIME140.dll`, and the math, runtime,
and heap Universal CRT API sets. Case-duplicate `kernel32.dll` entries are one
Windows dependency. Required concrete system libraries are present in
`C:\Windows\System32`.

There are no debug CRT, plug-in-local, GPU-vendor, network, or third-party
imports. Candidate blocker count is zero. This review permits only staged,
isolated loading after hash revalidation; it does not permit selector dispatch
or rendering beyond the separately approved stage.

