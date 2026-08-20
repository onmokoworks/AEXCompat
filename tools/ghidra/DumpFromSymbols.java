// Headless post-script for programs imported with -noanalysis: decompile from named symbols
// (export names, substring match) or hex addresses, creating functions on demand and following
// direct callees up to a depth. Writes decompiled C to a text file.
// Usage: analyzeHeadless <proj> <name> -process <prog> -noanalysis -scriptPath <dir>
//        -postScript DumpFromSymbols.java <outPath> <maxDepth> <sel>...
//   <sel> = "sym:<needle>" (symbol names containing needle) or hex address (0x-less).
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.app.cmd.function.CreateFunctionCmd;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import ghidra.program.model.pcode.*;
import java.io.*;
import java.util.*;

public class DumpFromSymbols extends GhidraScript {
  @Override
  public void run() throws Exception {
    String[] args = getScriptArgs();
    if (args.length < 3) { println("usage: DumpFromSymbols <outPath> <maxDepth> <sel>..."); return; }
    PrintWriter out = new PrintWriter(new FileWriter(args[0]));
    int maxDepth = Integer.parseInt(args[1]);
    FunctionManager fm = currentProgram.getFunctionManager();
    SymbolTable st = currentProgram.getSymbolTable();
    Listing listing = currentProgram.getListing();
    Deque<Address> work = new ArrayDeque<>();
    Map<Address, Integer> depth = new LinkedHashMap<>();
    for (int i = 2; i < args.length; i++) {
      String sel = args[i];
      if (sel.startsWith("sym:")) {
        String needle = sel.substring(4);
        for (Symbol s : st.getAllSymbols(false)) {
          if (!s.getName().contains(needle)) continue;
          if (s.getAddress().isExternalAddress()) continue;
          if (!currentProgram.getMemory().contains(s.getAddress())) continue;
          out.println("SYMBOL " + s.getName() + " @ " + s.getAddress());
          if (!depth.containsKey(s.getAddress())) { depth.put(s.getAddress(), 0); work.add(s.getAddress()); }
        }
      } else {
        Address a = currentProgram.getAddressFactory().getAddress(sel);
        if (a != null && !depth.containsKey(a)) { depth.put(a, 0); work.add(a); }
      }
    }
    DecompInterface ifc = new DecompInterface();
    ifc.setOptions(new DecompileOptions());
    ifc.openProgram(currentProgram);
    int count = 0;
    while (!work.isEmpty()) {
      Address a = work.poll();
      int d = depth.get(a);
      Function f = fm.getFunctionAt(a);
      if (f == null) {
        if (listing.getInstructionAt(a) == null) disassemble(a);
        CreateFunctionCmd cmd = new CreateFunctionCmd(a);
        cmd.applyTo(currentProgram, monitor);
        f = fm.getFunctionAt(a);
      }
      if (f == null) { out.println("\n==== NO FUNCTION @ " + a + " ===="); continue; }
      StringBuilder names = new StringBuilder();
      for (Symbol s : st.getSymbols(a)) names.append(s.getName()).append(' ');
      out.println("\n==================== " + f.getName() + " @ " + a + " depth=" + d + " [" + names + "] ====================");
      DecompileResults res = ifc.decompileFunction(f, 120, monitor);
      if (res == null || res.getDecompiledFunction() == null) { out.println("<decompile failed>"); continue; }
      out.println(res.getDecompiledFunction().getC());
      count++;
      HighFunction hf = res.getHighFunction();
      if (hf == null || d >= maxDepth) continue;
      Iterator<PcodeOpAST> ops = hf.getPcodeOps();
      while (ops.hasNext()) {
        PcodeOpAST op = ops.next();
        if (op.getOpcode() != PcodeOp.CALL) continue;
        Address callee = op.getInput(0).getAddress();
        if (!currentProgram.getMemory().contains(callee)) continue;
        // resolve thunk (jmp [iat]) targets: keep the thunk itself; its decompile shows the target
        if (!depth.containsKey(callee)) { depth.put(callee, d + 1); work.add(callee); }
      }
    }
    ifc.dispose();
    out.close();
    println("wrote " + args[0] + " (" + count + " functions)");
  }
}
