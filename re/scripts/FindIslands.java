// Classify functions by reachability, per the taxonomy in docs/oracle-architecture.md.
//
//   ISLAND     no calls, no references off .text  -> callable with fabricated inputs
//   DATA_ONLY  no calls, but reads constant data  -> callable; inputs must respect the tables
//   SELF_CALL  calls only other mapped functions  -> callable, transitively
//   REACHES_IMPORT / WRITES_GLOBAL                -> NOT callable from fabricated inputs
//
// Emits JSONL. Small pure-arithmetic ISLANDs are the first differential-testing targets.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.block.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.*;
import java.io.*;
import java.util.*;

public class FindIslands extends GhidraScript {

    private String blockName(Address a) {
        MemoryBlock b = currentProgram.getMemory().getBlock(a);
        return b == null ? "?" : b.getName();
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String outPath = args.length > 0 ? args[0] : "islands.jsonl";
        int maxBody = args.length > 1 ? Integer.parseInt(args[1]) : 4096;

        FunctionManager fm = currentProgram.getFunctionManager();
        Listing listing = currentProgram.getListing();
        int counts[] = new int[5];   // island, dataonly, selfcall, import, other

        try (PrintWriter out = new PrintWriter(new BufferedWriter(new FileWriter(outPath)))) {
            for (Function f : fm.getFunctions(true)) {
                if (monitor.isCancelled()) break;
                long size = f.getBody().getNumAddresses();
                if (size == 0 || size > maxBody) continue;

                boolean hasCall = false, offText = false, reachesImport = false, writesData = false;
                int insns = 0, fpOps = 0, intMulDiv = 0;

                for (Instruction ins : listing.getInstructions(f.getBody(), true)) {
                    insns++;
                    String mn = ins.getMnemonicString().toUpperCase();
                    if (mn.equals("CALL")) hasCall = true;
                    if (mn.startsWith("MOVS") || mn.startsWith("MULS") || mn.startsWith("ADDS")
                        || mn.startsWith("SUBS") || mn.startsWith("DIVS") || mn.startsWith("CVT")
                        || mn.startsWith("COMIS") || mn.startsWith("UCOMIS")) fpOps++;
                    if (mn.equals("IMUL") || mn.equals("IDIV") || mn.equals("MUL") || mn.equals("DIV"))
                        intMulDiv++;

                    for (Reference r : ins.getReferencesFrom()) {
                        Address t = r.getToAddress();
                        if (!t.isMemoryAddress()) continue;
                        String blk = blockName(t);
                        if (r.getReferenceType().isCall()) hasCall = true;
                        if (!".text".equals(blk)) {
                            offText = true;
                            if (".data".equals(blk) || ".tls".equals(blk)) writesData = true;
                            Symbol s = getSymbolAt(t);
                            if (s != null && s.getName().toLowerCase().contains("thunk")) reachesImport = true;
                        }
                    }
                }
                if (insns == 0) continue;

                String cls;
                if (reachesImport) { cls = "REACHES_IMPORT"; counts[3]++; }
                else if (hasCall)  { cls = "SELF_CALL";      counts[2]++; }
                else if (writesData) { cls = "WRITES_GLOBAL"; counts[4]++; }
                else if (offText)  { cls = "DATA_ONLY";      counts[1]++; }
                else               { cls = "ISLAND";         counts[0]++; }

                out.println("{\"name\":\"" + f.getName() + "\",\"ea\":\"" + f.getEntryPoint()
                        + "\",\"size\":" + size + ",\"insns\":" + insns
                        + ",\"class\":\"" + cls + "\",\"fp_ops\":" + fpOps
                        + ",\"int_muldiv\":" + intMulDiv + "}");
            }
        }
        println("ISLAND=" + counts[0] + " DATA_ONLY=" + counts[1] + " SELF_CALL=" + counts[2]
                + " REACHES_IMPORT=" + counts[3] + " WRITES_GLOBAL=" + counts[4]
                + " -> " + outPath);
    }
}
