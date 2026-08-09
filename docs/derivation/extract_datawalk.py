"""Extract DataWalk byte-ranges from every walk_data-style function in riseofnations.exe.

A walk_data(DataWalk* w) calls w->vtable[0](begin, end) to hash a contiguous byte
range of `this`, and w->vtable[1](p) to note a pointer/label.  We linearly scan each
function, tracking which registers hold `this` (entry ECX) or a `lea this+disp`, and
record the (begin, end) pairs pushed immediately before each `call dword ptr [r]`.
"""
import pefile, struct, json, bisect, sys, re
from capstone import *
from capstone.x86 import *

BIN = "/Users/ember/dev/don/ron-bin/riseofnations.exe"
pe = pefile.PE(BIN)
base = pe.OPTIONAL_HEADER.ImageBase
img = pe.get_memory_mapped_image()

funcs = []
for line in open("/Users/ember/dev/don/schema/islands.jsonl"):
    d = json.loads(line)
    funcs.append((int(d["ea"], 16), d["size"], d["name"]))
funcs.sort()

vt = json.load(open("<local-recovery-scratchpad>/chk/vtables.json"))
# map function ea -> list of (class, slot)
slot_of = {}
for vft, (nm, slots) in vt.items():
    for i, s in enumerate(slots):
        slot_of.setdefault(int(s, 16), []).append((nm, i * 4))

md = Cs(CS_ARCH_X86, CS_MODE_32)
md.detail = True

R = {X86_REG_EAX: 'eax', X86_REG_EBX: 'ebx', X86_REG_ECX: 'ecx', X86_REG_EDX: 'edx',
     X86_REG_ESI: 'esi', X86_REG_EDI: 'edi', X86_REG_EBP: 'ebp', X86_REG_ESP: 'esp'}


def analyse(ea, size):
    """Return list of dicts describing visitor calls found in this function."""
    code = img[ea - base:ea - base + size]
    # abstract values: ('this', disp) | ('arg0', disp) | ('other', ...) | None
    val = {r: None for r in R.values()}
    val['ecx'] = ('this', 0)
    pushes = []          # stack of abstract values, most recent last
    out = []
    seen_ebp_frame = False
    for i in md.disasm(code, ea):
        m, ops = i.mnemonic, i.operands
        if m == 'push':
            o = ops[0]
            if o.type == X86_OP_REG:
                pushes.append(val.get(R.get(o.reg), None))
            elif o.type == X86_OP_IMM:
                pushes.append(('imm', o.imm))
            elif o.type == X86_OP_MEM:
                # push dword ptr [ebp+8] => arg0
                if o.mem.base == X86_REG_EBP and o.mem.disp == 8:
                    pushes.append(('arg0', 0))
                else:
                    pushes.append(None)
            else:
                pushes.append(None)
            continue
        if m == 'call':
            o = ops[0]
            if o.type == X86_OP_MEM and o.mem.index == 0:
                d = o.mem.disp
                recv = val.get('ecx')
                if d == 0 and len(pushes) >= 2:
                    # stdcall, args pushed right-to-left: last push = arg1 = begin
                    b, a = pushes[-2], pushes[-1]
                    if (a and b and a[0] == b[0] and a[0] in ('this', 'arg0')
                            and isinstance(a[1], int) and isinstance(b[1], int)):
                        out.append(dict(kind='range', at=i.address, base=a[0],
                                        begin=a[1], end=b[1], recv=recv))
                    else:
                        out.append(dict(kind='range?', at=i.address,
                                        args=[a, b], recv=recv))
                    pushes = pushes[:-2]
                elif d == 4 and len(pushes) >= 1:
                    out.append(dict(kind='ptr', at=i.address, arg=pushes[-1], recv=recv))
                    pushes = pushes[:-1]
                else:
                    out.append(dict(kind='vcall', at=i.address, slot=d))
                    pushes = []
            else:
                pushes = []
            # calls clobber
            for r in ('eax', 'ecx', 'edx'):
                val[r] = None
            continue
        if m == 'lea' and len(ops) == 2 and ops[0].type == X86_OP_REG and ops[1].type == X86_OP_MEM:
            dst = R.get(ops[0].reg)
            mem = ops[1].mem
            src = R.get(mem.base)
            if dst:
                if mem.index == 0 and src and val.get(src) and val[src][0] in ('this', 'arg0'):
                    val[dst] = (val[src][0], val[src][1] + mem.disp)
                elif mem.base == X86_REG_EBP and mem.index == 0:
                    val[dst] = ('frame', mem.disp)
                else:
                    val[dst] = None
            continue
        if m == 'mov' and len(ops) == 2 and ops[0].type == X86_OP_REG:
            dst = R.get(ops[0].reg)
            if not dst:
                continue
            if ops[1].type == X86_OP_REG:
                val[dst] = val.get(R.get(ops[1].reg))
            elif ops[1].type == X86_OP_MEM and ops[1].mem.base == X86_REG_EBP \
                    and ops[1].mem.index == 0 and ops[1].mem.disp == 8:
                val[dst] = ('arg0', 0)
            else:
                val[dst] = None
            continue
        # anything else writing a reg kills it
        for o in ops:
            if o.type == X86_OP_REG and o.access & CS_AC_WRITE:
                r = R.get(o.reg)
                if r:
                    val[r] = None
    return out


results = {}
for ea, size, name in funcs:
    if size <= 0 or size > 60000:
        continue
    try:
        res = analyse(ea, size)
    except Exception:
        continue
    ranges = [r for r in res if r['kind'] == 'range']
    if not ranges:
        continue
    results[name] = dict(ea="%08x" % ea, size=size,
                         vslots=slot_of.get(ea, []),
                         calls=[{k: (v if not isinstance(v, tuple) else list(v))
                                 for k, v in r.items()} for r in res])

json.dump(results, open("<local-recovery-scratchpad>/chk/walkranges.json", "w"), indent=0)
print("functions with >=1 clean this-relative range:", len(results))
nr = sum(len([c for c in v['calls'] if c['kind'] == 'range']) for v in results.values())
print("total clean ranges:", nr)
# how many are at vtable slot 0x7c
s7c = [k for k, v in results.items() if any(s == 0x7c for _, s in v['vslots'])]
print("of which sit at vtable slot 0x7c:", len(s7c))
