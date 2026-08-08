// Bulk-decompile the simulation corpus to C, one file per function.
//
// Usage: -postScript BulkDecomp.java <outdir> <maxBodyBytes> <perFuncTimeoutSec>
//
// Skips functions larger than maxBodyBytes (the rules.xml loader alone exceeds a 600s
// decompiler budget) and records every skip/failure in a manifest, so the corpus is
// honest about what it does not contain.
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import java.io.*;

public class BulkDecomp extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        File outdir = new File(a.length > 0 ? a[0] : "decomp");
        long maxBody = a.length > 1 ? Long.parseLong(a[1]) : 8192;
        int tmo = a.length > 2 ? Integer.parseInt(a[2]) : 30;
        outdir.mkdirs();

        DecompInterface di = new DecompInterface();
        di.setOptions(new DecompileOptions());
        di.toggleCCode(true);
        di.setSimplificationStyle("decompile");
        if (!di.openProgram(currentProgram)) { println("ERR open: " + di.getLastMessage()); return; }

        int ok = 0, skipBig = 0, fail = 0;
        try (PrintWriter man = new PrintWriter(new BufferedWriter(
                new FileWriter(new File(outdir, "MANIFEST.jsonl"))))) {
            for (Function f : currentProgram.getFunctionManager().getFunctions(true)) {
                if (monitor.isCancelled()) break;
                long size = f.getBody().getNumAddresses();
                String ea = f.getEntryPoint().toString();
                if (size == 0 || size > maxBody) {
                    skipBig++;
                    man.println("{\"ea\":\"" + ea + "\",\"size\":" + size + ",\"status\":\"skipped_large\"}");
                    continue;
                }
                DecompileResults r = di.decompileFunction(f, tmo, monitor);
                if (!r.decompileCompleted()) {
                    fail++;
                    man.println("{\"ea\":\"" + ea + "\",\"size\":" + size + ",\"status\":\"failed\"}");
                    continue;
                }
                String c = r.getDecompiledFunction().getC();
                try (PrintWriter pw = new PrintWriter(new File(outdir, ea + ".c"))) { pw.print(c); }
                ok++;
                man.println("{\"ea\":\"" + ea + "\",\"size\":" + size + ",\"status\":\"ok\",\"lines\":"
                        + c.split("\n").length + "}");
                if (ok % 2000 == 0) println("progress: " + ok + " decompiled");
            }
        } finally {
            di.dispose();
        }
        println("DONE ok=" + ok + " skipped_large=" + skipBig + " failed=" + fail + " -> " + outdir);
    }
}
