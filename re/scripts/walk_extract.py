"""Enumerate every DataWalk visitor-consumer (walk_data / walk_*_data) in
riseofnations.exe and recover its ORDERED operation list at the instruction level.

A walk_data(DataWalk* w) calls w->vt[0](begin,end) ("walk_function", raw byte range)
and w->vt[1](const String& tag) ("walk_test", one tag byte).  We linearly scan each
function tracking abstract register values.

Output: schema/walkops.json (override with $WALKOPS_OUT)
"""
import json
import os
import pefile
from capstone import *
from capstone.x86 import *

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
BIN = os.path.join(ROOT, "ron-bin", "riseofnations.exe")
OUT = os.environ.get("WALKOPS_OUT", os.path.join(ROOT, "schema", "walkops.json"))

pe = pefile.PE(BIN)
base = pe.OPTIONAL_HEADER.ImageBase
img = pe.get_memory_mapped_image()

# Function bounds: every .text symbol start in the PDB public-symbol table,
# unioned with Ghidra's islands.jsonl (which has gaps -- WalkDataGame::walk_data
# at 0x005a2360 is missing from it).  Size = distance to the next start.
starts = set()
for line in open(os.path.join(ROOT, "schema", "rise-symbols.tsv")):
    va, sec, _n = line.rstrip("\n").split("\t")
    if sec == ".text":
        starts.add(int(va, 16))
for line in open(os.path.join(ROOT, "schema", "islands.jsonl")):
    starts.add(int(json.loads(line)["ea"], 16))
ss = sorted(starts)
funcs = [(e, min((ss[i + 1] if i + 1 < len(ss) else e + 64) - e, 300000))
         for i, e in enumerate(ss)]

md = Cs(CS_ARCH_X86, CS_MODE_32)
md.detail = True

R = {X86_REG_EAX: 'eax', X86_REG_EBX: 'ebx', X86_REG_ECX: 'ecx', X86_REG_EDX: 'edx',
     X86_REG_ESI: 'esi', X86_REG_EDI: 'edi'}
SUB = {X86_REG_AL: 'eax', X86_REG_AH: 'eax', X86_REG_AX: 'eax',
       X86_REG_BL: 'ebx', X86_REG_BH: 'ebx', X86_REG_BX: 'ebx',
       X86_REG_CL: 'ecx', X86_REG_CH: 'ecx', X86_REG_CX: 'ecx',
       X86_REG_DL: 'edx', X86_REG_DH: 'edx', X86_REG_DX: 'edx',
       X86_REG_SI: 'esi', X86_REG_DI: 'edi'}
BASEREL = ('this', 'walker', 'frame', 'esp', 'strpool')
BASEREL3 = ('gload',)   # ('gload', global_va, disp) == *(void**)global_va + disp


def wide(r):
    return R.get(r) or SUB.get(r)


def addoff(v, d):
    if v is None:
        return None
    if v[0] in BASEREL:
        return (v[0], v[1] + d)
    if v[0] in BASEREL3:
        return (v[0], v[1], v[2] + d)
    return None


def analyse(ea, size):
    code = img[ea - base:ea - base + size]
    insns = list(md.disasm(code, ea))
    if not insns:
        return [], [], []
    # --- pass 1: control flow annotation -------------------------------------
    backedges = []
    fwdjcc = []          # (from, to) for conditional forward jumps
    for i in insns:
        if i.mnemonic.startswith('j') and i.operands and i.operands[0].type == X86_OP_IMM:
            t = i.operands[0].imm
            if ea <= t < i.address:
                backedges.append((i.address, t))
            elif i.mnemonic != 'jmp' and i.address < t <= ea + size:
                fwdjcc.append((i.address, t))

    def depth(a):
        """how many conditional branches jump over address a (guard nesting)"""
        return sum(1 for f, t in fwdjcc if f < a < t)

    def inloop(a):
        return sum(1 for f, t in backedges if t <= a <= f)

    # --- pass 2: abstract interpretation -------------------------------------
    val = {r: None for r in R.values()}
    val['ecx'] = ('this', 0)
    stack = {}
    pushes = []
    ops = []
    # MSVC parks `this` and the walker in callee-saved registers for the whole
    # function.  A linear scan walks straight through the epilogue of an early
    # return (`pop ebx` etc.), which would otherwise destroy those bindings for
    # every basic block that follows.  So: snapshot the state once the walker
    # binding is established, and restore it at every `ret`.
    snap = None
    for i in insns:
        m, o = i.mnemonic, i.operands
        if m == 'ret':
            if snap is not None:
                val = dict(snap)
            pushes = []
            continue
        if m.startswith('j') or m in ('leave', 'nop', 'int3'):
            continue
        if m == 'push':
            a = o[0]
            if a.type == X86_OP_REG:
                pushes.append(val.get(wide(a.reg)))
            elif a.type == X86_OP_IMM:
                pushes.append(('imm', a.imm))
            elif a.type == X86_OP_MEM:
                mm = a.mem
                if mm.base == X86_REG_EBP and mm.index == 0 and mm.disp == 8:
                    pushes.append(('walker', 0))
                elif mm.base == X86_REG_EBP and mm.index == 0:
                    pushes.append(stack.get(mm.disp))
                else:
                    pushes.append(None)
            else:
                pushes.append(None)
            continue
        if m == 'call':
            a = o[0]
            rec = dict(at="%08x" % i.address, g=depth(i.address), L=inloop(i.address))
            if a.type == X86_OP_IMM:
                rec.update(k='call', target="%08x" % a.imm,
                           this=val.get('ecx'), args=pushes[-4:])
                pushes = []
            elif a.type == X86_OP_MEM and a.mem.index == 0:
                d = a.mem.disp
                rv = val.get(wide(a.mem.base)) if a.mem.base else None
                if a.mem.base == 0:
                    rec.update(k='iat', target="%08x" % d)
                    pushes = []
                elif rv and rv[0] == 'walkervt' and d == 0:
                    b = pushes[-2] if len(pushes) >= 2 else None
                    e = pushes[-1] if len(pushes) >= 1 else None
                    rec.update(k='walk', begin=e, end=b)
                    pushes = pushes[:-2]
                elif rv and rv[0] == 'walkervt' and d == 4:
                    rec.update(k='tag', arg=pushes[-1] if pushes else None)
                    pushes = pushes[:-1]
                elif rv and rv[0] == 'thisvt':
                    rec.update(k='vcall', slot=d, this=val.get('ecx'), args=pushes[-3:])
                    pushes = []
                else:
                    rec.update(k='vcall?', slot=d, recv=rv, this=val.get('ecx'),
                               args=pushes[-3:])
                    pushes = []
            else:
                rec.update(k='icall')
                pushes = []
            ops.append(rec)
            if snap is None and rec['k'] in ('walk', 'tag'):
                # first confirmed visitor call: the prologue is over and both
                # `this` and the walker are parked in callee-saved registers
                snap = dict(val)
                snap['ecx'] = ('this', 0)
            for r in ('eax', 'ecx', 'edx'):
                val[r] = None
            continue
        if m == 'lea' and len(o) == 2 and o[0].type == X86_OP_REG and o[1].type == X86_OP_MEM:
            dst = wide(o[0].reg)
            mm = o[1].mem
            if not dst:
                continue
            if mm.base == X86_REG_EBP and mm.index == 0:
                val[dst] = ('frame', mm.disp)
            elif mm.base == X86_REG_ESP and mm.index == 0:
                val[dst] = ('esp', mm.disp)
            elif mm.index == 0 and mm.base:
                val[dst] = addoff(val.get(wide(mm.base)), mm.disp)
            else:
                val[dst] = None
            continue
        if m == 'mov' and len(o) == 2 and o[0].type == X86_OP_REG:
            dst = wide(o[0].reg)
            if not dst:
                continue
            src = o[1]
            if src.type == X86_OP_REG:
                val[dst] = val.get(wide(src.reg))
            elif src.type == X86_OP_IMM:
                val[dst] = ('imm', src.imm)
            elif src.type == X86_OP_MEM:
                mm = src.mem
                if mm.base == X86_REG_EBP and mm.index == 0 and mm.disp == 8:
                    val[dst] = ('walker', 0)
                elif mm.base == X86_REG_EBP and mm.index == 0:
                    val[dst] = stack.get(mm.disp)
                elif mm.base == 0 and mm.index == 0:
                    val[dst] = ('gload', mm.disp, 0)
                elif mm.base and mm.index == 0:
                    s = val.get(wide(mm.base))
                    if s is None:
                        val[dst] = None
                    elif s[0] == 'walker' and s[1] == 0 and mm.disp == 0:
                        val[dst] = ('walkervt', 0)
                    elif s[0] == 'this' and s[1] == 0 and mm.disp == 0:
                        val[dst] = ('thisvt', 0)
                    elif s[0] == 'gload' and s[1] == 0xc06378 and mm.disp == 0x10:
                        val[dst] = ('strpool', 0)
                    elif s[0] == 'gload' and s[2] == 0:
                        val[dst] = ('gderef', s[1], mm.disp)
                    elif s[0] == 'this':
                        val[dst] = ('thisload', s[1] + mm.disp)
                    else:
                        val[dst] = None
                else:
                    val[dst] = None
            else:
                val[dst] = None
            continue
        if m == 'mov' and len(o) == 2 and o[0].type == X86_OP_MEM:
            mm = o[0].mem
            if mm.base == X86_REG_EBP and mm.index == 0:
                if o[1].type == X86_OP_REG:
                    stack[mm.disp] = val.get(wide(o[1].reg))
                elif o[1].type == X86_OP_IMM:
                    stack[mm.disp] = ('imm', o[1].imm)
                else:
                    stack[mm.disp] = None
            continue
        if m in ('add', 'sub') and len(o) == 2 and o[0].type == X86_OP_REG \
                and o[1].type == X86_OP_IMM:
            dst = wide(o[0].reg)
            if dst:
                k = o[1].imm if m == 'add' else -o[1].imm
                val[dst] = addoff(val.get(dst), k)
            continue
        for a in o:
            if a.type == X86_OP_REG and (a.access & CS_AC_WRITE):
                r = wide(a.reg)
                if r:
                    val[r] = None
    return ops, backedges, fwdjcc


results = {}
nerr = 0
for ea, size in funcs:
    if size <= 0 or size > 300000:
        continue
    try:
        ops, be, fj = analyse(ea, size)
    except Exception as e:
        nerr += 1
        continue
    if not any(o['k'] in ('walk', 'tag') for o in ops):
        continue
    results["%08x" % ea] = dict(size=size,
                                backedges=[["%08x" % a, "%08x" % b] for a, b in be],
                                ops=ops)

json.dump(results, open(OUT, "w"), indent=0)
print("functions with walk/tag calls:", len(results), "errors:", nerr)
print("walk calls:", sum(len([o for o in v['ops'] if o['k'] == 'walk']) for v in results.values()))
print("tag  calls:", sum(len([o for o in v['ops'] if o['k'] == 'tag']) for v in results.values()))
