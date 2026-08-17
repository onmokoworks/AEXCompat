// Headless post-script: print the disassembly listing of an address range to a text file,
// disassembling first where the range holds no instructions (undiscovered code such as MSVC
// catch funclets). Usage: -postScript DumpListing.java <outPath> <startHex> <endHex>
// Added 2026-08-17 (#1253).
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import java.io.*;

public class DumpListing extends GhidraScript {
  @Override
  public void run() throws Exception {
    String[] args = getScriptArgs();
    if (args.length < 3) { println("usage: DumpListing <outPath> <startHex> <endHex>"); return; }
    PrintWriter out = new PrintWriter(new FileWriter(args[0]));
    AddressFactory af = currentProgram.getAddressFactory();
    Address start = af.getAddress(args[1]);
    Address end = af.getAddress(args[2]);
    Listing listing = currentProgram.getListing();
    FunctionManager fm = currentProgram.getFunctionManager();
    ReferenceManager rm = currentProgram.getReferenceManager();
    Address a = start;
    while (a.compareTo(end) < 0) {
      Instruction ins = listing.getInstructionAt(a);
      if (ins == null) {
        Data d = listing.getDefinedDataAt(a);
        if (d != null) { out.println(a + "  DATA " + d.getDataType().getName() + " " + d.getDefaultValueRepresentation()); a = a.add(d.getLength()); continue; }
        disassemble(a);
        ins = listing.getInstructionAt(a);
        if (ins == null) { out.println(a + "  ?? " + String.format("%02x", currentProgram.getMemory().getByte(a) & 0xff)); a = a.add(1); continue; }
      }
      Function f = fm.getFunctionAt(a);
      if (f != null) out.println("---- FUNCTION " + f.getName() + " ----");
      Symbol s = currentProgram.getSymbolTable().getPrimarySymbol(a);
      if (s != null && f == null) out.println(s.getName() + ":");
      StringBuilder refs = new StringBuilder();
      for (Reference r : rm.getReferencesFrom(a)) {
        Symbol ts = currentProgram.getSymbolTable().getPrimarySymbol(r.getToAddress());
        if (ts != null) refs.append("  ; -> ").append(ts.getName(true));
      }
      out.println(a + "  " + ins.toString() + refs);
      a = a.add(ins.getLength());
    }
    out.close();
    println("wrote " + args[0]);
  }
}
