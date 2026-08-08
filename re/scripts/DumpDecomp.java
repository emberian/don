// Decompiles named functions to C and writes them under re/decomp/.
// Usage: -postScript DumpDecomp.java <outdir> <addr> [<addr> ...]
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import java.io.*;

public class DumpDecomp extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 2) { println("ERR: need outdir and >=1 address"); return; }
        File outdir = new File(args[0]);
        outdir.mkdirs();

        DecompInterface di = new DecompInterface();
        DecompileOptions opts = new DecompileOptions();
        di.setOptions(opts);
        di.toggleCCode(true);
        di.toggleSyntaxTree(true);
        di.setSimplificationStyle("decompile");
        if (!di.openProgram(currentProgram)) {
            println("ERR: decompiler failed to open: " + di.getLastMessage());
            return;
        }
        try {
            for (int i = 1; i < args.length; i++) {
                Address a = currentProgram.getAddressFactory().getAddress(args[i]);
                Function f = getFunctionAt(a);
                if (f == null) { println("ERR: no function at " + args[i]); continue; }
                long t0 = System.currentTimeMillis();
                DecompileResults res = di.decompileFunction(f, 600, monitor);
                if (!res.decompileCompleted()) {
                    println("ERR: decompile failed for " + f.getName() + ": " + res.getErrorMessage());
                    continue;
                }
                String c = res.getDecompiledFunction().getC();
                File out = new File(outdir, f.getName() + ".c");
                try (PrintWriter pw = new PrintWriter(out)) { pw.print(c); }
                println("OK " + f.getName() + " @" + a + "  bytes=" + c.length()
                        + "  lines=" + c.split("\n").length
                        + "  body=" + f.getBody().getNumAddresses()
                        + "  ms=" + (System.currentTimeMillis() - t0)
                        + "  -> " + out.getAbsolutePath());
            }
        } finally {
            di.dispose();
        }
    }
}
