// Headless post-script: print a vtable's slots as "index offset target symbol".
// Usage: ... -postScript DumpVtable.java <outPath> <vtableAddrHex>[:<maxSlots>] ...
// Stops at the first entry that does not point into an executable block (or at maxSlots).
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.mem.*;
import ghidra.program.model.symbol.*;
import java.io.*;

public class DumpVtable extends GhidraScript {
  @Override
  public void run() throws Exception {
    String[] args = getScriptArgs();
    PrintWriter out = new PrintWriter(new FileWriter(args[0]));
    Memory mem = currentProgram.getMemory();
    SymbolTable st = currentProgram.getSymbolTable();
    for (int i = 1; i < args.length; i++) {
      String spec = args[i];
      int max = 4096;
      String addrText = spec;
      int colon = spec.indexOf(':');
      if (colon >= 0) { addrText = spec.substring(0, colon); max = Integer.parseInt(spec.substring(colon + 1)); }
      Address vt = currentProgram.getAddressFactory().getAddress(addrText);
      out.println("VTABLE " + vt);
      for (int slot = 0; slot < max; slot++) {
        Address entryAddr = vt.add((long) slot * 8);
        long target;
        try { target = mem.getLong(entryAddr); } catch (Exception e) { break; }
        Address t = currentProgram.getAddressFactory().getDefaultAddressSpace().getAddress(target);
        MemoryBlock b = mem.getBlock(t);
        if (b == null || !b.isExecute()) { out.println("  (end at slot " + slot + ": " + Long.toHexString(target) + ")"); break; }
        StringBuilder names = new StringBuilder();
        for (Symbol s : st.getSymbols(t)) { if (names.length() > 0) names.append(" | "); names.append(s.getName()); }
        out.println(String.format("  %4d 0x%04x %s %s", slot, slot * 8, t, names.length() > 0 ? names : "?"));
      }
    }
    out.close();
    println("wrote " + args[0]);
  }
}
