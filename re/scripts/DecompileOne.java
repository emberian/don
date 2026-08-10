// Decompile one already-analyzed function to stdout.
//
// Usage:
//   -postScript DecompileOne.java 00617c10 300 [/tmp/00617c10.c]
//
// @category Decompiler

import ghidra.app.decompiler.DecompileOptions;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import java.io.File;
import java.io.PrintWriter;

public class DecompileOne extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 1) {
            printerr("usage: DecompileOne.java <hex-va> [timeout-seconds] [output-file]");
            return;
        }
        Address address = toAddr(Long.parseUnsignedLong(args[0].replace("0x", ""), 16));
        int timeout = args.length > 1 ? Integer.parseInt(args[1]) : 300;
        Function function = getFunctionAt(address);
        if (function == null) {
            printerr("no function at " + address);
            return;
        }

        DecompInterface decompiler = new DecompInterface();
        decompiler.setOptions(new DecompileOptions());
        decompiler.toggleCCode(true);
        decompiler.setSimplificationStyle("decompile");
        try {
            if (!decompiler.openProgram(currentProgram)) {
                printerr("failed to open program: " + decompiler.getLastMessage());
                return;
            }
            DecompileResults result = decompiler.decompileFunction(function, timeout, monitor);
            if (!result.decompileCompleted()) {
                printerr("decompile failed: " + result.getErrorMessage());
                return;
            }
            String c = result.getDecompiledFunction().getC();
            if (args.length > 2) {
                File output = new File(args[2]);
                try (PrintWriter writer = new PrintWriter(output)) {
                    writer.print(c);
                }
                println("wrote " + output + " (" + c.length() + " chars)");
            }
            else {
                println(c);
            }
        }
        finally {
            decompiler.dispose();
        }
    }
}
