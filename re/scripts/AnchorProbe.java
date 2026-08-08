// Validates the UTF-16 rule-name anchor methodology: find a rule-name string in the
// analyzed program and walk xrefs back to the consuming function.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.ReferenceManager;

public class AnchorProbe extends GhidraScript {
    @Override
    public void run() throws Exception {
        FunctionManager fm = currentProgram.getFunctionManager();
        println("FUNCTIONS: " + fm.getFunctionCount());

        String[] probes = {"flank_bonus", "cavalry_flank_bonus", "vehicle_flank_bonus",
                           "progression", "recharge", "siege_attrition", "accel_train"};
        java.util.List<Data> hits = new java.util.ArrayList<>();
        int strCount = 0;
        DataIterator it = currentProgram.getListing().getDefinedData(true);
        while (it.hasNext()) {
            Data d = it.next();
            Object v = d.getValue();
            if (v instanceof String) {
                strCount++;
                String s = ((String) v).trim();
                for (String p : probes) {
                    if (s.equals(p)) { hits.add(d); break; }
                }
            }
        }
        println("DEFINED STRINGS: " + strCount);
        println("ANCHOR HITS: " + hits.size());

        ReferenceManager rm = currentProgram.getReferenceManager();
        for (Data d : hits) {
            println("ANCHOR '" + d.getValue() + "' @ " + d.getAddress()
                    + " type=" + d.getDataType().getName());
            int n = 0;
            for (Reference r : rm.getReferencesTo(d.getAddress())) {
                Function f = fm.getFunctionContaining(r.getFromAddress());
                println("    xref " + r.getFromAddress() + " -> "
                        + (f != null ? f.getName() + " @" + f.getEntryPoint() : "(no function)"));
                if (++n >= 6) { println("    ..."); break; }
            }
            if (n == 0) println("    (no xrefs -- string may be referenced dynamically)");
        }
    }
}
