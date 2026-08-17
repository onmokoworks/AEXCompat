# Ghidra headless post-scripts

Post-scripts for `analyzeHeadless` used to produce the reverse-engineering
observation records under `docs/` (first: `docs/BEE_SCENE_OBJECT_ABI_2026-08-17.md`,
issue #1210; then `docs/MASKLESS_PATH_EFFECTS_OBSERVATION_2026-08-17.md`, issue #1253). They need only a Ghidra install and a project that holds the
binary; no MCP bridge, no GUI. Adobe binaries are read in place from the
local After Effects install and are never copied into this repository.

```powershell
& '<ghidra>\support\analyzeHeadless.bat' <projectDir> <projectName> `
  -import '<AE Support Files>\BEE.dll' -noanalysis
& '<ghidra>\support\analyzeHeadless.bat' <projectDir> <projectName> `
  -process BEE.dll -noanalysis -scriptPath tools\ghidra `
  -postScript DumpFromSymbols.java out.txt 2 'sym:BEE_GetSourceTimeFormat@@' 1802a2960
& '<ghidra>\support\analyzeHeadless.bat' <projectDir> <projectName> `
  -process BEE.dll -noanalysis -scriptPath tools\ghidra `
  -postScript DumpVtable.java vtables.txt 180f79128:400
```

| script | what it does |
| --- | --- |
| `DumpDecomp.java` | decompile functions selected by string reference (`str:`), import reference (`imp:`), vtable (`vt:`), symbol / export name (`sym:`), reference to an address (`ref:`), address, `mk:<addr>` (create the function first: code auto-analysis left undiscovered, e.g. MSVC FH4 catch funclets, #1253), and one-level `callees` / `callers` expansion; for analysed programs |
| `DumpListing.java` | disassembly listing of an address range (`<out> <startHex> <endHex>`), disassembling undefined bytes first; for reading around a fault or a call site when the decompiler drops a return value (#1253) |
| `DumpFromSymbols.java` | for programs imported with `-noanalysis`: create functions at symbols (`sym:<substring>`, export names work because the PE loader keeps them) or addresses, decompile, and follow direct callees up to a depth |
| `DumpVtable.java` | list a vtable's slots (index, byte offset, target, symbol) until the first entry that does not point into executable memory; BEE.dll exports its virtuals by name, so the slot names come straight from the export table |
