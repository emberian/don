// Report what a PDB pass actually landed. Run as a **postScript** after
// ApplyPdb.java (see that file for the full invocation).
//
// @category PDB

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.SourceType;
import ghidra.program.model.symbol.Symbol;

public class PdbReport extends GhidraScript {

	@Override
	public void run() throws Exception {
		int total = 0, named = 0;
		var it = currentProgram.getFunctionManager().getFunctions(true);
		while (it.hasNext()) {
			Function f = it.next();
			total++;
			if (!f.getName().startsWith("FUN_")) {
				named++;
			}
		}
		println("PdbReport: functions=" + total + " named=" + named +
			" still-FUN_=" + (total - named));

		int userSyms = 0;
		var sit = currentProgram.getSymbolTable().getAllSymbols(false);
		while (sit.hasNext()) {
			Symbol s = sit.next();
			if (s.getSource() != SourceType.DEFAULT) {
				userSyms++;
			}
		}
		println("PdbReport: non-default symbols=" + userSyms);
		println("PdbReport: data types=" +
			currentProgram.getDataTypeManager().getDataTypeCount(true));

		// Spot-check the addresses this project derived independently.
		String[] probes = { "00644130", "00a39cf0", "00a39d70", "0094a700", "00936560",
			"00a1d110", "00570170", "0065fc00", "00a46830", "009459d0", "0061c490",
			"00846450", "00569a90" };
		for (String p : probes) {
			Address a = toAddr(p);
			Function f = getFunctionAt(a);
			String sig = "<no function>";
			if (f != null) {
				sig = f.getName(true) + "  |  " + f.getSignature().getPrototypeString();
			}
			println("  0x" + p + "  " + sig);
		}
	}
}
