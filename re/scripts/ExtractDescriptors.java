// Instruction-level extraction of the descriptor/visitor binding pattern.
//
// The engine binds each named rule to a struct field by building a stack descriptor and
// making one virtual call through the visitor's vtable:
//
//     mov  eax, 8                          ; type tag, often hoisted and reused
//     mov  [ebp-0x84], offset L"recharge"  ; descriptor+0 = name
//     mov  word ptr [ebp-0x80], ax         ; descriptor+4 = tag
//     push dword ptr [ebx+0x1f4]           ; the bound field (this+0x1F4)
//     mov  ecx, esi                        ; visitor
//     call dword ptr [eax+0x1c]            ; visitor->bind(...)
//
// Because the tag is materialised into a register that may be set far earlier and reused
// across many descriptors, a fixed backward window is not sufficient: we carry a
// register->constant map through a single linear pass and resolve the tag at the store.
//
// `this` arrives in ECX; MSVC copies it into a callee-saved register in the prologue, so
// we track the alias set and only accept [thisReg + disp] as a field offset.
//
// Usage: -postScript ExtractDescriptors.java <outfile.jsonl> (auto|<addr> [<addr>...])
//   auto = every function referencing >= MIN_STRINGS distinct wide strings.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.lang.Register;
import ghidra.program.model.listing.*;
import ghidra.program.model.scalar.Scalar;
import ghidra.program.model.symbol.Reference;
import java.io.*;
import java.util.*;

public class ExtractDescriptors extends GhidraScript {

    static final int WINDOW = 64;       // max instructions from name store to the CALL
    static final int MIN_STRINGS = 3;   // auto-discovery threshold

    // [measured] Every binding site pushes this same constant immediately AFTER pushing
    // the bound field, e.g.  `push dword ptr [edi+4]` ; `push 0xeb437c` ; `push ecx`.
    // Anchoring on it is far more robust than tracking which register aliases `this`,
    // and unlike a `disp > 0` heuristic it does not silently drop field offset 0.
    static final long FIELD_LANDMARK = 0xeb437cL;

    private String wideStringAt(Instruction ins) {
        for (Reference r : ins.getReferencesFrom()) {
            Data d = currentProgram.getListing().getDataAt(r.getToAddress());
            if (d == null) continue;
            if (d.getValue() instanceof String) {
                String dt = d.getDataType().getName().toLowerCase();
                if (dt.contains("unicode") || dt.contains("wchar")) return (String) d.getValue();
            }
        }
        return null;
    }

    private static String base(Register r) {
        Register b = r.getBaseRegister();
        return (b == null ? r : b).getName();
    }

    private static String esc(String s) {
        StringBuilder b = new StringBuilder();
        for (char c : s.toCharArray()) {
            switch (c) {
                case '\\': b.append("\\\\"); break;
                case '"':  b.append("\\\""); break;
                case '\n': b.append("\\n");  break;
                case '\r': b.append("\\r");  break;
                case '\t': b.append("\\t");  break;
                default:
                    if (c < 0x20 || c > 0x7e) b.append(String.format("\\u%04x", (int) c));
                    else b.append(c);
            }
        }
        return b.toString();
    }

    /** One in-flight descriptor being assembled. */
    private static class Rec {
        String name, refEa, callEa = null;
        Long tag = null;
        Long fieldOff = null;        // from the landmark anchor -- authoritative
        String fieldReg = null;
        List<Long> offs = new ArrayList<>();
        List<String> offRegs = new ArrayList<>();
        List<Long> otherImms = new ArrayList<>();
        int startIdx;
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 2) { println("ERR: need <outfile> and (auto|addrs)"); return; }
        FunctionManager fm = currentProgram.getFunctionManager();
        Listing listing = currentProgram.getListing();

        List<Function> targets = new ArrayList<>();
        if (args[1].equalsIgnoreCase("auto")) {
            for (Function f : fm.getFunctions(true)) {
                if (monitor.isCancelled()) break;
                Set<String> seen = new HashSet<>();
                for (Instruction ins : listing.getInstructions(f.getBody(), true)) {
                    String s = wideStringAt(ins);
                    if (s != null && seen.add(s) && seen.size() >= MIN_STRINGS) break;
                }
                if (seen.size() >= MIN_STRINGS) targets.add(f);
            }
            println("AUTO: " + targets.size() + " candidate loader functions");
        } else {
            for (int i = 1; i < args.length; i++) {
                Function f = getFunctionAt(currentProgram.getAddressFactory().getAddress(args[i]));
                if (f == null) { println("ERR: no function at " + args[i]); continue; }
                targets.add(f);
            }
        }

        int total = 0, withTag = 0, withOff = 0;
        try (PrintWriter out = new PrintWriter(new BufferedWriter(new FileWriter(args[0])))) {
            for (Function f : targets) {
                if (monitor.isCancelled()) break;
                List<Instruction> body = new ArrayList<>();
                for (Instruction ins : listing.getInstructions(f.getBody(), true)) body.add(ins);

                Map<String, Long> regConst = new HashMap<>();
                String prevPushReg = null; long prevPushDisp = 0L;
                Set<String> thisRegs = new HashSet<>(Collections.singletonList("ECX"));
                Rec cur = null;

                for (int i = 0; i < body.size(); i++) {
                    Instruction ins = body.get(i);
                    String mn = ins.getMnemonicString().toUpperCase();

                    String name = wideStringAt(ins);
                    if (name != null) {
                        if (cur != null) { emit(out, f, cur); total++;
                            if (cur.tag != null) withTag++; if (cur.fieldOff != null) withOff++; }
                        cur = new Rec();
                        cur.name = name;
                        cur.refEa = ins.getAddress().toString();
                        cur.startIdx = i;
                    }

                    if (cur != null && i - cur.startIdx > WINDOW) {
                        emit(out, f, cur); total++;
                        if (cur.tag != null) withTag++; if (cur.fieldOff != null) withOff++;
                        cur = null;
                    }

                    if (cur != null && name == null) {
                        // Landmark anchor: `push <field>` immediately precedes `push 0xeb437c`.
                        if (mn.equals("PUSH")) {
                            Object[] o0 = ins.getOpObjects(0);
                            if (o0.length == 1 && o0[0] instanceof Scalar
                                    && ((Scalar) o0[0]).getUnsignedValue() == FIELD_LANDMARK) {
                                if (prevPushReg != null) {
                                    cur.fieldOff = prevPushDisp;
                                    cur.fieldReg = prevPushReg;
                                }
                            } else {
                                Register pr = null; Scalar ps = null;
                                for (Object o : o0) {
                                    if (o instanceof Register) { if (pr == null) pr = (Register) o; }
                                    else if (o instanceof Scalar) ps = (Scalar) o;
                                }
                                if (pr != null) {   // memory operand [reg] or [reg+disp]
                                    prevPushReg = base(pr);
                                    // Displacements can be negative (base register pointing
                                    // into the middle of a structure), so sign-extend --
                                    // getUnsignedValue() reported -8 as 4294967288.
                                    prevPushDisp = (ps == null) ? 0L : ps.getSignedValue();
                                } else {
                                    prevPushReg = null; prevPushDisp = 0L;
                                }
                            }
                        }
                        for (int op = 0; op < ins.getNumOperands(); op++) {
                            Register br = null; Scalar sc = null; Register lone = null;
                            Object[] objs = ins.getOpObjects(op);
                            for (Object o : objs) {
                                if (o instanceof Register) { if (br == null) br = (Register) o; }
                                else if (o instanceof Scalar) sc = (Scalar) o;
                            }
                            if (objs.length == 1 && objs[0] instanceof Register) lone = (Register) objs[0];

                            if (br != null && sc != null && objs.length > 1 && thisRegs.contains(base(br))) {
                                long v = sc.getUnsignedValue();
                                if (v > 0 && !cur.offs.contains(v)) { cur.offs.add(v); cur.offRegs.add(base(br)); }
                            }
                            // descriptor tag: `mov word ptr [ebp-X], ax` -> resolve ax
                            if (mn.startsWith("MOV") && op == 1 && lone != null && cur.tag == null) {
                                Long c = regConst.get(base(lone));
                                if (c != null && c > 0 && c < 0x1000) cur.tag = c;
                            }
                            if (mn.startsWith("MOV") && op == 1 && objs.length == 1
                                    && objs[0] instanceof Scalar) {
                                long v = ((Scalar) objs[0]).getUnsignedValue();
                                if (v != 0 && cur.otherImms.size() < 8) cur.otherImms.add(v);
                            }
                        }
                        if (mn.equals("CALL")) {
                            cur.callEa = ins.getAddress().toString();
                            emit(out, f, cur); total++;
                            if (cur.tag != null) withTag++; if (cur.fieldOff != null) withOff++;
                            cur = null;
                        }
                    }

                    // ---- register state update (after use, so a store sees the prior value)
                    if (mn.equals("MOV") && ins.getNumOperands() == 2 && ins.getRegister(0) != null) {
                        Register dst = ins.getRegister(0);
                        Object[] src = ins.getOpObjects(1);
                        if (src.length == 1 && src[0] instanceof Scalar) {
                            regConst.put(base(dst), ((Scalar) src[0]).getUnsignedValue());
                        } else {
                            regConst.remove(base(dst));
                        }
                        Register srcReg = ins.getRegister(1);
                        if (srcReg != null && thisRegs.contains(base(srcReg))) thisRegs.add(base(dst));
                        else thisRegs.remove(base(dst));
                    } else {
                        for (Object o : ins.getResultObjects()) {
                            if (o instanceof Register) {
                                regConst.remove(base((Register) o));
                                thisRegs.remove(base((Register) o));
                            }
                        }
                    }
                }
                if (cur != null) { emit(out, f, cur); total++;
                    if (cur.tag != null) withTag++; if (cur.fieldOff != null) withOff++; }
            }
        }
        println("RECORDS: " + total + "  with_tag: " + withTag + "  with_offset: " + withOff
                + " -> " + args[0]);
    }

    private void emit(PrintWriter out, Function f, Rec r) {
        StringBuilder sb = new StringBuilder();
        sb.append("{\"func\":\"").append(f.getName()).append("\"")
          .append(",\"func_ea\":\"").append(f.getEntryPoint()).append("\"")
          .append(",\"name\":\"").append(esc(r.name)).append("\"")
          .append(",\"ref_ea\":\"").append(r.refEa).append("\"")
          .append(",\"call_ea\":").append(r.callEa == null ? "null" : "\"" + r.callEa + "\"")
          .append(",\"tag\":").append(r.tag == null ? "null" : r.tag)
          .append(",\"field_off\":").append(r.fieldOff == null ? "null" : r.fieldOff)
          .append(",\"field_reg\":").append(r.fieldReg == null ? "null" : "\"" + r.fieldReg + "\"")
          .append(",\"offsets\":").append(r.offs)
          .append(",\"off_regs\":[");
        for (int i = 0; i < r.offRegs.size(); i++) {
            if (i > 0) sb.append(",");
            sb.append("\"").append(r.offRegs.get(i)).append("\"");
        }
        sb.append("]").append(",\"imms\":").append(r.otherImms).append("}");
        out.println(sb);
    }
}
