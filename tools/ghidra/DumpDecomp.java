// Headless post-script: decompile selected functions of the current program to a text file.
// Usage (analyzeHeadless ... -process <name> -noanalysis -scriptPath <this dir> -postScript DumpDecomp.java <outPath> <sel>...):
//   <sel> = 0x-less hex address (function containing it), "str:<needle>" (functions referencing a
//   defined string containing needle), "imp:<needle>" (functions referencing an external symbol
//   whose name contains needle; "imp:" alone = every import), "vt:<needle>" (the functions behind
//   every entry of a `...::vftable` symbol whose name contains needle; needs RTTI/analysis),
//   "callees"/"callers" (expand one level from the functions selected so far).
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import ghidra.program.model.data.*;
import java.io.*;
import java.util.*;

public class DumpDecomp extends GhidraScript {
  @Override
  public void run() throws Exception {
    String[] args = getScriptArgs();
    if (args.length < 1) { println("usage: DumpDecomp <outPath> <sel>..."); return; }
    PrintWriter out = new PrintWriter(new FileWriter(args[0]));
    FunctionManager fm = currentProgram.getFunctionManager();
    ReferenceManager rm = currentProgram.getReferenceManager();
    Set<Function> targets = new LinkedHashSet<>();
    for (int i = 1; i < args.length; i++) {
      String sel = args[i];
      if (sel.startsWith("str:")) {
        String needle = sel.substring(4);
        for (Data d : currentProgram.getListing().getDefinedData(true)) {
          DataType t = d.getDataType();
          if (!(t instanceof StringDataType || t instanceof TerminatedStringDataType
              || t instanceof UnicodeDataType || t instanceof TerminatedUnicodeDataType)) continue;
          Object v = d.getValue();
          if (v == null || !v.toString().contains(needle)) continue;
          out.println("STRING " + d.getAddress() + " \"" + v + "\"");
          for (Reference r : rm.getReferencesTo(d.getAddress())) {
            Function f = fm.getFunctionContaining(r.getFromAddress());
            out.println("  xref from " + r.getFromAddress() + (f != null ? " in " + f.getName() : ""));
            if (f != null) targets.add(f);
          }
        }
      } else if (sel.startsWith("imp:")) {
        String needle = sel.substring(4);
        for (Symbol sym : currentProgram.getSymbolTable().getExternalSymbols()) {
          if (!sym.getName().contains(needle)) continue;
          out.println("IMPORT " + sym.getName() + " @" + sym.getAddress());
          Set<Function> direct = new LinkedHashSet<>();
          for (Reference r : sym.getReferences()) {
            Function f = fm.getFunctionContaining(r.getFromAddress());
            out.println("  xref from " + r.getFromAddress() + (f != null ? " in " + f.getName() : ""));
            if (f != null) direct.add(f);
          }
          for (Function f : direct) {
            if (!f.isThunk()) { targets.add(f); continue; }
            for (Reference r : rm.getReferencesTo(f.getEntryPoint())) {
              Function c = fm.getFunctionContaining(r.getFromAddress());
              if (c != null) { out.println("  via thunk " + f.getName() + " from " + r.getFromAddress() + " in " + c.getName()); targets.add(c); }
            }
          }
        }
      } else if (sel.startsWith("vt:")) {
        String needle = sel.substring(3);
        ghidra.program.model.mem.Memory mem = currentProgram.getMemory();
        for (Symbol sym : currentProgram.getSymbolTable().getAllSymbols(true)) {
          String nm = sym.getName(true);
          if (!nm.contains(needle) || !nm.contains("vftable")) continue;
          out.println("VTABLE " + nm + " @ " + sym.getAddress());
          Address a = sym.getAddress();
          for (int k = 0; k < 64; k++) {
            long v;
            try { v = mem.getLong(a.add(k * 8)); } catch (Exception e) { break; }
            Address fa = currentProgram.getAddressFactory().getDefaultAddressSpace().getAddress(v);
            Function f = fm.getFunctionAt(fa);
            if (f == null) { out.println("  [" + k + "] " + fa + " (no function) stop"); break; }
            out.println("  [" + k + "] " + f.getName() + " @ " + fa);
            targets.add(f);
          }
        }
      } else if (sel.equals("callees")) {
        Set<Function> more = new LinkedHashSet<>();
        for (Function f : targets) more.addAll(f.getCalledFunctions(monitor));
        targets.addAll(more);
      } else if (sel.equals("callers")) {
        Set<Function> more = new LinkedHashSet<>();
        for (Function f : targets) more.addAll(f.getCallingFunctions(monitor));
        targets.addAll(more);
      } else {
        Address a = currentProgram.getAddressFactory().getAddress(sel);
        Function f = a != null ? fm.getFunctionContaining(a) : null;
        if (f == null) out.println("NO FUNCTION at " + sel); else targets.add(f);
      }
    }
    DecompInterface ifc = new DecompInterface();
    ifc.setOptions(new DecompileOptions());
    ifc.openProgram(currentProgram);
    for (Function f : targets) {
      if (f.isThunk() || f.isExternal()) continue;
      out.println("\n==================== " + f.getName() + " @ " + f.getEntryPoint() + " ====================");
      DecompileResults res = ifc.decompileFunction(f, 120, monitor);
      if (res != null && res.getDecompiledFunction() != null) out.println(res.getDecompiledFunction().getC());
      else out.println("<decompile failed>");
    }
    ifc.dispose();
    out.close();
    println("wrote " + args[0] + " (" + targets.size() + " functions)");
  }
}
