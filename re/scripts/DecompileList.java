// Decompile an explicit list of already-analyzed functions to C, one file per function.
//
// This is the backfill companion to BulkDecomp.java. BulkDecomp skips any function whose
// body exceeds its maxBodyBytes argument (default 8,192) and records the skip as
// "skipped_large"; those functions are decompilable, they were simply never attempted.
// Feed their entry points to this script with a real per-function budget.
//
// Usage: -postScript DecompileList.java <listfile> <outdir> <perFuncTimeoutSec> <manifestOut>
//                                       [maxPayloadMB]
//
//   listfile   one hex VA per line ("00570170" or "0x00570170"); blank lines and #-comments
//              are ignored.
//   outdir     receives <ea>.c, matching re/decomp-all's naming.
//   manifestOut  JSONL, one row per input VA, in the same shape BulkDecomp emits plus a
//              "secs" field, so it can be merged straight back into MANIFEST.jsonl.
//   maxPayloadMB  DecompileOptions.setMaxPayloadMBytes. The default 50 MB is NOT enough for
//              the largest bodies: Constants::log_data 0x00570170 (63,382 B) spends 1,529 s
//              and then dies with "Response buffer size exceeded", which reads like a
//              timeout and is not one. Raise this before raising the timeout.
//
// @category Decompiler
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.*;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;

public class DecompileList extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        if (a.length < 2) {
            printerr("usage: DecompileList.java <listfile> <outdir> [timeoutSec] [manifestOut]");
            return;
        }
        File listFile = new File(a[0]);
        File outdir = new File(a[1]);
        int tmo = a.length > 2 ? Integer.parseInt(a[2]) : 1800;
        File manifestOut = new File(a.length > 3 ? a[3] : new File(outdir, "BACKFILL.jsonl").getPath());
        int payloadMB = a.length > 4 ? Integer.parseInt(a[4]) : 0;
        outdir.mkdirs();

        List<String> targets = new ArrayList<>();
        for (String line : Files.readAllLines(listFile.toPath(), StandardCharsets.UTF_8)) {
            String s = line.trim();
            if (s.isEmpty() || s.startsWith("#")) continue;
            targets.add(s.replace("0x", "").replace("0X", ""));
        }

        DecompileOptions opts = new DecompileOptions();
        if (payloadMB > 0) opts.setMaxPayloadMBytes(payloadMB);
        println("maxPayloadMBytes = " + opts.getMaxPayloadMBytes() + ", timeout = " + tmo + "s");
        DecompInterface di = new DecompInterface();
        di.setOptions(opts);
        di.toggleCCode(true);
        di.setSimplificationStyle("decompile");
        if (!di.openProgram(currentProgram)) { println("ERR open: " + di.getLastMessage()); return; }

        int ok = 0, fail = 0, missing = 0;
        try (PrintWriter man = new PrintWriter(new BufferedWriter(new FileWriter(manifestOut)))) {
            for (String hex : targets) {
                Address addr = toAddr(Long.parseUnsignedLong(hex, 16));
                Function f = getFunctionAt(addr);
                if (f == null) {
                    missing++;
                    man.println("{\"ea\":\"" + hex + "\",\"size\":0,\"status\":\"failed\",\"reason\":\"no function at address\"}");
                    man.flush();
                    println("MISSING " + hex);
                    continue;
                }
                String ea = f.getEntryPoint().toString();
                long size = f.getBody().getNumAddresses();
                long t0 = System.currentTimeMillis();
                DecompileResults r = di.decompileFunction(f, tmo, monitor);
                double secs = (System.currentTimeMillis() - t0) / 1000.0;
                if (!r.decompileCompleted()) {
                    fail++;
                    String msg = String.valueOf(r.getErrorMessage()).replace("\\", "\\\\").replace("\"", "\\\"").replace("\n", " ");
                    man.println("{\"ea\":\"" + ea + "\",\"size\":" + size + ",\"status\":\"failed\",\"reason\":\""
                            + msg + "\",\"secs\":" + String.format("%.1f", secs) + "}");
                    man.flush();
                    println("FAIL " + ea + " (" + size + " B, " + secs + "s): " + msg);
                    continue;
                }
                String c = r.getDecompiledFunction().getC();
                try (PrintWriter pw = new PrintWriter(new File(outdir, ea + ".c"))) { pw.print(c); }
                ok++;
                int lines = c.split("\n").length;
                man.println("{\"ea\":\"" + ea + "\",\"size\":" + size + ",\"status\":\"ok\",\"lines\":" + lines
                        + ",\"secs\":" + String.format("%.1f", secs) + "}");
                man.flush();
                println("OK " + ea + " (" + size + " B -> " + lines + " lines, " + secs + "s)");
            }
        } finally {
            di.dispose();
        }
        println("DONE ok=" + ok + " failed=" + fail + " missing=" + missing + " -> " + outdir);
    }
}
