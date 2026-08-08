// Point the program at a PDB and disable every analyzer except "PDB Universal".
//
// Run this as a **preScript**, then let headless analysis run (i.e. do NOT pass
// -noanalysis). The PDB applicator reads TransientProgramProperties scoped to an
// active analysis session, so calling PdbUniversalAnalyzer.doAnalysis() straight
// from a postScript dies with "No active analysis session" — it has to run as an
// analyzer inside AutoAnalysisManager.
//
// The in-tree Ghidra README_PDB.html describes an obsolete Windows-only path
// (pdb.exe / XML). Ghidra's "PDB Universal" reader is pure Java and works fine on
// arm64 macOS.
//
//   analyzeHeadless <proj-dir> <proj> -process riseofnations.exe \
//     -scriptPath /Users/ember/dev/don/re/scripts \
//     -preScript ApplyPdb.java /path/to/rise.pdb
//
// Ghidra verifies the PDB's GUID/age against the program's CodeView record and
// refuses a mismatch, so a successful run is itself evidence of identity.
//
// @category PDB

import java.io.File;
import java.util.Map;

import ghidra.app.plugin.core.analysis.PdbUniversalAnalyzer;
import ghidra.app.script.GhidraScript;

public class ApplyPdb extends GhidraScript {

	/** Analyzers we allow to run. Everything else is switched off so that this
	 *  pass only layers PDB information onto the already-analyzed program. */
	private static final String PDB_ANALYZER = "PDB Universal";

	@Override
	public void run() throws Exception {
		String[] args = getScriptArgs();
		if (args.length < 1) {
			println("ApplyPdb: usage: -preScript ApplyPdb.java <path-to.pdb>");
			return;
		}
		File pdbFile = new File(args[0]);
		if (!pdbFile.isFile()) {
			println("ApplyPdb: no such file: " + pdbFile);
			return;
		}

		PdbUniversalAnalyzer.setPdbFileOption(currentProgram, pdbFile);
		PdbUniversalAnalyzer.setAllowUntrustedOption(currentProgram, true);

		int off = 0;
		Map<String, String> opts = getCurrentAnalysisOptionsAndValues(currentProgram);
		for (String name : opts.keySet()) {
			if (name.contains(".")) {
				continue; // sub-option of an analyzer, not the enablement flag
			}
			if (name.equals(PDB_ANALYZER)) {
				setAnalysisOption(currentProgram, name, "true");
				continue;
			}
			if ("true".equals(opts.get(name))) {
				setAnalysisOption(currentProgram, name, "false");
				off++;
			}
		}

		// Turn on every boolean sub-option the PDB analyzer exposes — source line
		// info in particular is the whole point of having the real PDB. The exact
		// option names move between Ghidra releases, so discover them rather than
		// hardcoding; they are printed below for the record.
		for (var e : opts.entrySet()) {
			String name = e.getKey();
			if (!name.startsWith(PDB_ANALYZER + ".")) {
				continue;
			}
			println("ApplyPdb: option [" + name + "] = " + e.getValue());
			if ("false".equals(e.getValue())) {
				setAnalysisOption(currentProgram, name, "true");
				println("ApplyPdb:   -> set true");
			}
		}

		println("ApplyPdb: pdb=" + pdbFile);
		println("ApplyPdb: disabled " + off + " other analyzers; " + PDB_ANALYZER + " enabled");
		println("ApplyPdb: functions before = " +
			currentProgram.getFunctionManager().getFunctionCount());
	}
}
