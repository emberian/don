/*
 * donhook.dll — a 32-bit x86 inline-detour logger for Rise of Nations: Extended Edition.
 *
 * Purpose: turn ordinary play into ground-truth samples of the damage pipeline.
 * Hooks FUN_00644130 (damage), Object::get_attack (0x006469F0) and Object::get_armor
 * (0x00647DB0); records ECX, the six stack dwords, the call site and the EAX return, and
 * — for damage calls — a block-read snapshot of the attacker and defender Object and
 * UnitType records plus the live balance-table cell.
 *
 * Design constraints, each deliberate:
 *   - Every derived read goes through ReadProcessMemory on our own process handle, which
 *     returns FALSE on a bad address instead of faulting. The hook must never be able to
 *     kill the game with a stray dereference.
 *   - No formatting and no file I/O on the game thread: the hook writes fixed-size binary
 *     records into a ring; a worker thread formats CSV and writes it.
 *   - The patch is removable at runtime: drop a STOP file and the worker restores the
 *     original prologue bytes and closes the log.
 *   - FlushInstructionCache after every code write. Under the ARM64 x86 emulator the
 *     translation cache must be told, or the patch is invisible to already-translated code.
 *
 * Build (macOS host, zig 0.16):
 *   zig cc -target x86-windows-gnu -shared -O2 -o donhook.dll donhook.c
 */

#include <windows.h>
#include <tlhelp32.h>
#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>

#define MAX_HOOKS      8
#define MAX_STOLEN     24
#define RING_CAP       32768
#define RING_MARGIN    64
#define SHADOW_MAX     64

#define TGT_DAMAGE     0
#define TGT_GETATTACK  1
#define TGT_GETARMOR   2
#define TGT_UNIT_ATK   3   /* Unit::vtbl[+0x120]  0x006103C0 */
#define TGT_UNIT_ARM   4   /* Unit::vtbl[+0x124]  0x00610160 */
#define TGT_BUILD_ATK  5   /* Build::vtbl[+0x120] 0x0062E610 */
#define TGT_BUILD_ARM  6   /* Build/Wall::vtbl[+0x124] 0x0063FA60 */

/* ------------------------------------------------------------------ records ---- */

typedef struct {
    unsigned seq;
    unsigned tid;
    unsigned target;        /* TGT_* */
    unsigned depth;         /* damage nesting depth at entry */
    unsigned parent_seq;    /* seq of the enclosing damage record, 0 if none */
    unsigned this_ptr;
    unsigned retaddr;       /* call site, absolute; subtract delta for the static VA */
    unsigned args[6];
    int      result;
    unsigned flags;         /* bit0 result valid, bit2 forced-close (never returned) */

    /* derived; damage records only */
    unsigned derived_mask;  /* 1 atkobj 2 atktype 4 defobj 8 deftype 16 balance 32 game 64 tabB */
    unsigned atk_obj, atk_obj_tab, atk_type;
    unsigned def_obj, def_obj_b, def_type;
    unsigned atk_player, atk_index, def_player, def_index;
    int      atk_type_id, def_type_id;
    int      balance_pct;
    int      type_attack, type_armor;
    unsigned atk_masks, def_masks;
    int      atk_domain, def_domain;
    unsigned atk_obj08, atk_obj0c;
    unsigned def_obj0c, def_obj4c, def_obj50, def_obj5c, def_obj68, def_obj6c;
    int      def_word_a4;
    int      atk_splash_pct, def_splash_div, def_type_2b8, atk_type_40;
    unsigned cur_frame;
} rec_t;

static rec_t          g_ring[RING_CAP];
static volatile LONG  g_ready[RING_CAP];
static unsigned       g_stamp[RING_CAP];
static volatile LONG  g_head = 0;
static volatile LONG  g_tail = 0;
static volatile LONG  g_dropped = 0;
static volatile LONG  g_forced = 0;

/* ------------------------------------------------------------------ config ----- */

typedef struct {
    unsigned rva;
    unsigned stolen;
    char     name[32];
    unsigned target;
    BYTE    *addr;
    BYTE     orig[MAX_STOLEN];
    BYTE    *tramp;
    int      installed;
    /* Byte offset inside the stolen region of a rel32 `call`/`jmp`, or -1. Relocating the
     * displacement is mandatory: `Unit::get_attack` opens with a four-byte preamble and
     * then `call get_attack`, so a five-byte detour cannot avoid swallowing the call, and
     * copying its rel32 verbatim would send the trampoline to a wrong absolute target.
     * Declared per hook rather than sniffed, because guessing where an opcode starts in a
     * byte stream is exactly the kind of silent error this project cannot absorb. */
    int      relfix;
} hook_t;

static hook_t   g_hooks[MAX_HOOKS];
static int      g_nhooks = 0;
static char     g_logpath[MAX_PATH]  = "C:\\Users\\ember\\donhook\\damage.csv";
static char     g_stoppath[MAX_PATH] = "C:\\Users\\ember\\donhook\\STOP";
static char     g_diagpath[MAX_PATH] = "C:\\Users\\ember\\donhook\\donhook.log";
static int      g_derive   = 1;
static unsigned g_maxrec   = 2000000u;
static unsigned g_base     = 0;
static int      g_delta    = 0;
static DWORD    g_tls      = TLS_OUT_OF_INDEXES;
static HANDLE   g_self     = 0;
static BYTE    *g_retstub  = 0;
static volatile LONG g_stopping = 0;
static HINSTANCE g_self_mod;

static void load_cfg(HINSTANCE self);

/* ------------------------------------------------------------------ diag ------- */

static void diag(const char *fmt, ...) {
    char buf[600];
    va_list ap;
    DWORD w;
    HANDLE h;
    va_start(ap, fmt);
    _vsnprintf(buf, sizeof(buf) - 4, fmt, ap);
    va_end(ap);
    buf[sizeof(buf) - 4] = 0;
    strcat(buf, "\r\n");
    h = CreateFileA(g_diagpath, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
                    NULL, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (h != INVALID_HANDLE_VALUE) {
        SetFilePointer(h, 0, NULL, FILE_END);
        WriteFile(h, buf, (DWORD)strlen(buf), &w, NULL);
        CloseHandle(h);
    }
}

/* ------------------------------------------------------------------ safe read -- */

static int rd(unsigned addr, void *out, unsigned n) {
    SIZE_T got = 0;
    if (addr < 0x10000u || addr >= 0x80000000u) return 0;
    if (!ReadProcessMemory(g_self, (LPCVOID)addr, out, n, &got)) return 0;
    return got == n;
}
static int rd32(unsigned addr, unsigned *out) { return rd(addr, out, 4); }

/* ------------------------------------------------------------------ tls -------- */

typedef struct {
    int      depth;
    unsigned dmg_depth;
    unsigned dmg_seq;
    struct { unsigned orig_ret, slot, target, prev_seq; } sh[SHADOW_MAX];
} tstate;

static tstate *tls_get(void) {
    tstate *t;
    if (g_tls == TLS_OUT_OF_INDEXES) return NULL;
    t = (tstate *)TlsGetValue(g_tls);
    if (!t) {
        t = (tstate *)HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, sizeof(tstate));
        if (!t) return NULL;
        TlsSetValue(g_tls, t);
    }
    return t;
}

/* ------------------------------------------------------------------ derive ----- */

/* Offsets are exactly the ones FUN_00644130 reads; see docs/derivation/damage-port.md.
 * This copies what retail read. It never recomputes anything retail computed. */
#define VA_OBJTAB_A  0x00C0AB84u
#define VA_OBJTAB_B  0x00C0AEC0u
#define VA_BALANCE   0x00C06AFCu
#define VA_GAMEPTR   0x00C061E8u

#define OBJ_BLK   0x120
#define TYPE_BLK  0x320

static void derive(rec_t *r) {
    BYTE ao[OBJ_BLK], dobj[OBJ_BLK], at[TYPE_BLK], dt[TYPE_BLK];
    unsigned arr = 0, p, idx, gp = 0, frame = 0, cell;
    short bal = 0;

    if (!rd(r->this_ptr, ao, OBJ_BLK)) return;
    r->derived_mask |= 1;
    r->atk_obj    = r->this_ptr;
    r->atk_player = ao[9];
    r->atk_index  = (unsigned)(int)*(short *)(ao + 10);
    r->atk_obj08  = *(unsigned *)(ao + 8);
    r->atk_obj0c  = *(unsigned *)(ao + 0x0C);
    r->atk_type   = *(unsigned *)(ao + 0x18);
    if (rd(r->atk_type, at, TYPE_BLK)) {
        r->derived_mask |= 2;
        r->atk_type_id    = *(int *)(at + 4);
        r->atk_masks      = *(unsigned *)(at + 0x1E4);
        r->type_attack    = *(int *)(at + 0x1E8);
        r->atk_domain     = *(int *)(at + 0x218);
        r->atk_splash_pct = *(int *)(at + 0x204);
        r->atk_type_40    = *(int *)(at + 0x40);
    }
    /* the attacker re-fetched through table A, exactly as retail does at 0x00644150 */
    if (r->atk_player < 64u &&
        rd32(VA_OBJTAB_A + (unsigned)g_delta + r->atk_player * 28u, &arr) && arr)
        rd32(arr + r->atk_index * 4u, &r->atk_obj_tab);

    p   = r->args[1];
    idx = r->args[0];
    r->def_player = p;
    r->def_index  = idx;
    if (p < 64u && idx < 65536u) {
        arr = 0;
        if (rd32(VA_OBJTAB_A + (unsigned)g_delta + p * 28u, &arr) && arr &&
            rd32(arr + idx * 4u, &r->def_obj) && r->def_obj) {
            if (rd(r->def_obj, dobj, OBJ_BLK)) {
                r->derived_mask |= 4;
                r->def_obj0c   = *(unsigned *)(dobj + 0x0C);
                r->def_obj4c   = *(unsigned *)(dobj + 0x4C);
                r->def_obj50   = *(unsigned *)(dobj + 0x50);
                r->def_obj5c   = *(unsigned *)(dobj + 0x5C);
                r->def_obj68   = *(unsigned *)(dobj + 0x68);
                r->def_obj6c   = *(unsigned *)(dobj + 0x6C);
                r->def_word_a4 = (int)*(short *)(dobj + 0xA4);
                r->def_type    = *(unsigned *)(dobj + 0x18);
                if (rd(r->def_type, dt, TYPE_BLK)) {
                    r->derived_mask |= 8;
                    r->def_type_id    = *(int *)(dt + 4);
                    r->def_masks      = *(unsigned *)(dt + 0x1E4);
                    r->type_armor     = *(int *)(dt + 0x214);
                    r->def_domain     = *(int *)(dt + 0x218);
                    r->def_splash_div = *(int *)(dt + 0x308);
                    r->def_type_2b8   = *(int *)(dt + 0x2B8);
                }
            }
        }
        /* the second object table — damage-port.md §5.6 assumes it aliases table A.
         * Capturing both settles that from the live process instead of assuming it. */
        arr = 0;
        if (rd32(VA_OBJTAB_B + (unsigned)g_delta + p * 28u, &arr) && arr &&
            rd32(arr + idx * 4u, &r->def_obj_b))
            r->derived_mask |= 64;
    }

    if ((r->derived_mask & 0xA) == 0xA) {
        cell = VA_BALANCE + (unsigned)g_delta +
               2u * (unsigned)(r->atk_type_id * 493 + r->def_type_id);
        if (rd(cell, &bal, 2)) { r->balance_pct = bal; r->derived_mask |= 16; }
    }
    if (rd32(VA_GAMEPTR + (unsigned)g_delta, &gp) && gp && rd32(gp + 0x550, &frame)) {
        r->cur_frame = frame; r->derived_mask |= 32;
    }
}

/* ------------------------------------------------------------------ handlers --- */

typedef struct {
    unsigned edi, esi, ebp, esp_, ebx, edx, ecx, eax, eflags;
    unsigned retaddr;
    unsigned a[6];
} enter_ctx;

typedef struct {
    unsigned edi, esi, ebp, esp_, ebx, edx, ecx, eax, eflags;
    unsigned ret_slot;
} ret_ctx;

/* Returns the address to install as the new return address, or 0 to leave it alone. */
unsigned __cdecl donhook_on_enter(enter_ctx *c, unsigned hid) {
    tstate  *t;
    hook_t  *h;
    LONG     idx;
    unsigned slot, seq;
    rec_t   *r;

    if (g_stopping) return 0;
    t = tls_get();
    if (!t || t->depth >= SHADOW_MAX) return 0;
    h = &g_hooks[hid];

    /* get_attack / get_armor are recorded only while a damage frame is live: they are
     * called from all over the engine and only the nested ones are damage evidence. */
    if (h->target != TGT_DAMAGE && t->dmg_depth == 0) return 0;

    /* Claim only when there is provably room; claiming and then bailing would leave a
     * hole the flusher has to time out on. A benign race can overshoot by a few slots,
     * which the margin absorbs. */
    if ((unsigned)g_head >= g_maxrec) return 0;
    if (g_head - g_tail >= RING_CAP - RING_MARGIN) {
        InterlockedIncrement(&g_dropped);
        return 0;
    }
    idx  = InterlockedIncrement(&g_head) - 1;
    slot = (unsigned)idx & (RING_CAP - 1);
    seq  = (unsigned)idx + 1;

    r = &g_ring[slot];
    memset(r, 0, sizeof(*r));
    r->seq        = seq;
    r->tid        = GetCurrentThreadId();
    r->target     = h->target;
    r->depth      = t->dmg_depth;
    r->parent_seq = (h->target == TGT_DAMAGE) ? 0 : t->dmg_seq;
    r->this_ptr   = c->ecx;
    r->retaddr    = c->retaddr;
    memcpy(r->args, c->a, sizeof(r->args));
    if (g_derive && h->target == TGT_DAMAGE) derive(r);

    g_stamp[slot] = GetTickCount();

    t->sh[t->depth].orig_ret = c->retaddr;
    t->sh[t->depth].slot     = slot;
    t->sh[t->depth].target   = h->target;
    t->sh[t->depth].prev_seq = t->dmg_seq;
    t->depth++;
    if (h->target == TGT_DAMAGE) { t->dmg_depth++; t->dmg_seq = seq; }

    return (unsigned)g_retstub;
}

void __cdecl donhook_on_return(ret_ctx *c) {
    tstate  *t;
    unsigned slot;
    t = tls_get();
    if (!t || t->depth <= 0) { c->ret_slot = 0; return; }
    t->depth--;
    slot = t->sh[t->depth].slot;
    if (t->sh[t->depth].target == TGT_DAMAGE && t->dmg_depth) t->dmg_depth--;
    t->dmg_seq = t->sh[t->depth].prev_seq;
    g_ring[slot].result = (int)c->eax;
    g_ring[slot].flags |= 1;
    __sync_synchronize();
    g_ready[slot] = 1;
    c->ret_slot = t->sh[t->depth].orig_ret;
}

/* ------------------------------------------------------------------ codegen ---- */

static BYTE *emit_u8 (BYTE *p, BYTE v)     { *p++ = v; return p; }
static BYTE *emit_u32(BYTE *p, unsigned v) { memcpy(p, &v, 4); return p + 4; }

/*
 * Per-target trampoline. Entered by an E9 from the patched prologue, so the stack is
 * exactly as the callee saw it: [esp]=retaddr, [esp+4..+0x18]=the six dwords, ECX=this.
 *
 *   pushfd / pushad                                   -> the ctx block
 *   push <hid> ; lea eax,[esp+4] ; push eax ; call on_enter ; add esp,8
 *   test eax,eax ; jz +4 ; mov [esp+36],eax           -> swap the return address
 *   popad / popfd ; <stolen bytes> ; jmp [resume_cell]
 */
static BYTE *build_tramp(hook_t *h) {
    BYTE *m = (BYTE *)VirtualAlloc(NULL, 0x1000, MEM_COMMIT | MEM_RESERVE,
                                   PAGE_EXECUTE_READWRITE);
    BYTE *p, *cell;
    unsigned hid = (unsigned)(h - g_hooks);
    if (!m) return NULL;
    memset(m, 0xCC, 0x1000);
    cell = m + 0x800;
    *(unsigned *)cell = (unsigned)(h->addr + h->stolen);

    p = m;
    p = emit_u8(p, 0x9C);                                              /* pushfd          */
    p = emit_u8(p, 0x60);                                              /* pushad          */
    p = emit_u8(p, 0x68); p = emit_u32(p, hid);                        /* push hid        */
    p = emit_u8(p, 0x8D); p = emit_u8(p, 0x44);
    p = emit_u8(p, 0x24); p = emit_u8(p, 0x04);                        /* lea eax,[esp+4] */
    p = emit_u8(p, 0x50);                                              /* push eax        */
    p = emit_u8(p, 0xE8);
    p = emit_u32(p, (unsigned)((BYTE *)donhook_on_enter - (p + 4)));   /* call on_enter   */
    p = emit_u8(p, 0x83); p = emit_u8(p, 0xC4); p = emit_u8(p, 0x08);  /* add esp,8       */
    p = emit_u8(p, 0x85); p = emit_u8(p, 0xC0);                        /* test eax,eax    */
    p = emit_u8(p, 0x74); p = emit_u8(p, 0x04);                        /* jz +4           */
    p = emit_u8(p, 0x89); p = emit_u8(p, 0x44);
    p = emit_u8(p, 0x24); p = emit_u8(p, 0x24);                        /* mov [esp+36],eax*/
    p = emit_u8(p, 0x61);                                              /* popad           */
    p = emit_u8(p, 0x9D);                                              /* popfd           */
    memcpy(p, h->orig, h->stolen);                                     /* stolen bytes    */
    if (h->relfix >= 0 && h->relfix + 5 <= (int)h->stolen) {
        /* rewrite the displacement for the copy's new address */
        int rel; BYTE *site = p + h->relfix;
        memcpy(&rel, h->orig + h->relfix + 1, 4);
        {
            unsigned target = (unsigned)(h->addr + h->relfix + 5) + (unsigned)rel;
            int nrel = (int)(target - (unsigned)(site + 5));
            memcpy(site + 1, &nrel, 4);
        }
    }
    p += h->stolen;
    p = emit_u8(p, 0xFF); p = emit_u8(p, 0x25);
    p = emit_u32(p, (unsigned)cell);                                   /* jmp [cell]      */

    FlushInstructionCache(g_self, m, 0x1000);
    return m;
}

/*
 * The shared return stub. Reached by the target's own `ret N`, so ESP is already unwound;
 * reserve a slot, hand the context to C, and `ret` through the slot the C code fills.
 *   sub esp,4 ; pushfd ; pushad ; push esp ; call on_return ; add esp,4 ; popad ; popfd ; ret
 */
static BYTE *build_retstub(void) {
    BYTE *m = (BYTE *)VirtualAlloc(NULL, 0x1000, MEM_COMMIT | MEM_RESERVE,
                                   PAGE_EXECUTE_READWRITE);
    BYTE *p;
    if (!m) return NULL;
    memset(m, 0xCC, 0x1000);
    p = m;
    p = emit_u8(p, 0x83); p = emit_u8(p, 0xEC); p = emit_u8(p, 0x04);  /* sub esp,4 */
    p = emit_u8(p, 0x9C);                                              /* pushfd    */
    p = emit_u8(p, 0x60);                                              /* pushad    */
    p = emit_u8(p, 0x54);                                              /* push esp  */
    p = emit_u8(p, 0xE8);
    p = emit_u32(p, (unsigned)((BYTE *)donhook_on_return - (p + 4)));
    p = emit_u8(p, 0x83); p = emit_u8(p, 0xC4); p = emit_u8(p, 0x04);  /* add esp,4 */
    p = emit_u8(p, 0x61);                                              /* popad     */
    p = emit_u8(p, 0x9D);                                              /* popfd     */
    p = emit_u8(p, 0xC3);                                              /* ret       */
    FlushInstructionCache(g_self, m, 0x1000);
    return m;
}

/* ------------------------------------------------------------------ patching --- */

static int suspend_others(HANDLE *out, int cap) {
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    THREADENTRY32 te;
    DWORD me = GetCurrentThreadId(), pid = GetCurrentProcessId();
    int n = 0;
    if (snap == INVALID_HANDLE_VALUE) return 0;
    te.dwSize = sizeof(te);
    if (Thread32First(snap, &te)) {
        do {
            HANDLE h;
            if (te.th32OwnerProcessID != pid || te.th32ThreadID == me) continue;
            if (n >= cap) break;
            h = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT, FALSE, te.th32ThreadID);
            if (h) { if (SuspendThread(h) != (DWORD)-1) out[n++] = h; else CloseHandle(h); }
        } while (Thread32Next(snap, &te));
    }
    CloseHandle(snap);
    return n;
}

static void resume_all(HANDLE *h, int n) {
    int i;
    for (i = 0; i < n; i++) { ResumeThread(h[i]); CloseHandle(h[i]); }
}

/* Is any suspended thread parked *inside* a range we are about to overwrite? Patching
 * under such a thread is the one way an inline detour reliably crashes a process. */
static int any_eip_inside(HANDLE *h, int n) {
    CONTEXT ctx;
    int i, k;
    for (i = 0; i < n; i++) {
        memset(&ctx, 0, sizeof(ctx));
        ctx.ContextFlags = CONTEXT_CONTROL;
        if (!GetThreadContext(h[i], &ctx)) continue;
        for (k = 0; k < g_nhooks; k++) {
            unsigned lo = (unsigned)g_hooks[k].addr;
            if (ctx.Eip > lo && ctx.Eip < lo + g_hooks[k].stolen) return 1;
        }
    }
    return 0;
}

static int quiesce(HANDLE *th, int cap) {
    int n = 0, tries;
    for (tries = 0; tries < 20; tries++) {
        n = suspend_others(th, cap);
        if (!any_eip_inside(th, n)) return n;
        resume_all(th, n);
        n = 0;
        Sleep(2);
    }
    return suspend_others(th, cap);
}

static int write_code(BYTE *dst, const BYTE *src, unsigned n) {
    DWORD old;
    SIZE_T w = 0;
    if (!VirtualProtect(dst, n, PAGE_EXECUTE_READWRITE, &old)) return 0;
    memcpy(dst, src, n);
    VirtualProtect(dst, n, old, &old);
    /* Two paths on purpose. FlushInstructionCache is the documented barrier; the
     * WriteProcessMemory of the same bytes goes through NtWriteVirtualMemory, which is
     * the path a debugger uses and which the WOW64 pluggable-CPU layer wires to
     * BTCpuFlushInstructionCache2 under the ARM64 x86 emulator. */
    FlushInstructionCache(g_self, dst, n);
    WriteProcessMemory(g_self, dst, src, n, &w);
    FlushInstructionCache(g_self, dst, n);
    return 1;
}

static void install_all(void) {
    HANDLE th[256];
    int n, i;
    BYTE patch[MAX_STOLEN];

    n = quiesce(th, 256);
    for (i = 0; i < g_nhooks; i++) {
        hook_t *h = &g_hooks[i];
        unsigned rel;
        memcpy(h->orig, h->addr, h->stolen);
        h->tramp = build_tramp(h);
        if (!h->tramp) { diag("tramp alloc failed for %s", h->name); continue; }
        memset(patch, 0x90, h->stolen);
        rel = (unsigned)(h->tramp - (h->addr + 5));
        patch[0] = 0xE9;
        memcpy(patch + 1, &rel, 4);
        if (!write_code(h->addr, patch, h->stolen)) { diag("patch failed %s", h->name); continue; }
        h->installed = 1;
        diag("hooked %s at %08X (rva %06X) stolen=%u tramp=%08X orig=%02X %02X %02X %02X %02X %02X",
             h->name, (unsigned)h->addr, h->rva, h->stolen, (unsigned)h->tramp,
             h->orig[0], h->orig[1], h->orig[2], h->orig[3], h->orig[4], h->orig[5]);
    }
    resume_all(th, n);
}

static void remove_all(void) {
    HANDLE th[256];
    int n, i, any = 0;
    for (i = 0; i < g_nhooks; i++) if (g_hooks[i].installed) any = 1;
    InterlockedExchange(&g_stopping, 1);
    if (!any) return;
    Sleep(50);
    n = quiesce(th, 256);
    for (i = 0; i < g_nhooks; i++) {
        hook_t *h = &g_hooks[i];
        if (!h->installed) continue;
        write_code(h->addr, h->orig, h->stolen);
        h->installed = 0;
    }
    resume_all(th, n);
    diag("unhooked all");
}

/* ------------------------------------------------------------------ csv -------- */

static const char *CSV_HDR =
"seq,tid,target,depth,parent_seq,this,retaddr,a1,a2,a3,a4,a5,a6,result,flags,derived_mask,"
"atk_obj,atk_obj_tab,atk_type,def_obj,def_obj_b,def_type,atk_player,atk_index,def_player,"
"def_index,atk_type_id,def_type_id,balance_pct,type_attack,type_armor,atk_masks,def_masks,"
"atk_domain,def_domain,atk_obj08,atk_obj0c,def_obj0c,def_obj4c,def_obj50,def_obj5c,"
"def_obj68,def_obj6c,def_word_a4,atk_splash_pct,def_splash_div,def_type_2b8,atk_type_40,"
"cur_frame\r\n";

static int fmt_rec(char *b, int cap, const rec_t *r) {
    return _snprintf(b, cap,
        "%u,%u,%u,%u,%u,%08X,%08X,%d,%d,%d,%d,%d,%d,%d,%u,%u,"
        "%08X,%08X,%08X,%08X,%08X,%08X,%u,%u,%u,"
        "%u,%d,%d,%d,%d,%d,%08X,%08X,"
        "%d,%d,%08X,%08X,%08X,%08X,%08X,%08X,"
        "%08X,%08X,%d,%d,%d,%d,%d,"
        "%u\r\n",
        r->seq, r->tid, r->target, r->depth, r->parent_seq, r->this_ptr, r->retaddr,
        (int)r->args[0], (int)r->args[1], (int)r->args[2], (int)r->args[3],
        (int)r->args[4], (int)r->args[5], r->result, r->flags, r->derived_mask,
        r->atk_obj, r->atk_obj_tab, r->atk_type, r->def_obj, r->def_obj_b, r->def_type,
        r->atk_player, r->atk_index, r->def_player,
        r->def_index, r->atk_type_id, r->def_type_id, r->balance_pct,
        r->type_attack, r->type_armor, r->atk_masks, r->def_masks,
        r->atk_domain, r->def_domain, r->atk_obj08, r->atk_obj0c, r->def_obj0c,
        r->def_obj4c, r->def_obj50, r->def_obj5c,
        r->def_obj68, r->def_obj6c, r->def_word_a4, r->atk_splash_pct, r->def_splash_div,
        r->def_type_2b8, r->atk_type_40,
        r->cur_frame);
}

/* ------------------------------------------------------------------ worker ----- */

static HANDLE g_log = INVALID_HANDLE_VALUE;

static unsigned drain(int force) {
    char line[900];
    DWORD w;
    unsigned n = 0;
    while (g_tail < g_head) {
        unsigned slot = (unsigned)g_tail & (RING_CAP - 1);
        int len;
        if (!g_ready[slot]) {
            /* A frame that never returned — an SEH unwind past our stub — must not stall
             * the pipe. Publish after 3 s with the forced-close flag set, never silently. */
            if (!force && GetTickCount() - g_stamp[slot] < 3000) break;
            g_ring[slot].flags |= 4;
            InterlockedIncrement(&g_forced);
        }
        len = fmt_rec(line, sizeof(line), &g_ring[slot]);
        if (len > 0) WriteFile(g_log, line, (DWORD)len, &w, NULL);
        g_ready[slot] = 0;
        InterlockedIncrement(&g_tail);
        n++;
    }
    return n;
}

/*
 * One capture session, then park. The DLL stays resident and re-arms when the STOP file
 * is deleted, re-reading the config — so a capture can be reconfigured without a second
 * injection (LoadLibrary on an already-loaded DLL will not re-run DllMain, so a
 * re-injectable design would silently do nothing).
 */
static DWORD WINAPI worker(LPVOID unused) {
    DWORD w;
    (void)unused;

    for (;;) {
        unsigned written = 0;

        g_log = CreateFileA(g_logpath, GENERIC_WRITE, FILE_SHARE_READ, NULL, CREATE_ALWAYS,
                            FILE_ATTRIBUTE_NORMAL, NULL);
        if (g_log == INVALID_HANDLE_VALUE) {
            diag("cannot open log %s err=%u", g_logpath, (unsigned)GetLastError());
            return 0;
        }
        WriteFile(g_log, CSV_HDR, (DWORD)strlen(CSV_HDR), &w, NULL);

        install_all();

        for (;;) {
            unsigned n = drain(0);
            written += n;
            if (n) FlushFileBuffers(g_log);
            if (GetFileAttributesA(g_stoppath) != INVALID_FILE_ATTRIBUTES) break;
            if ((unsigned)g_head >= g_maxrec && g_tail >= g_head) break;
            Sleep(200);
        }

        remove_all();
        Sleep(300);
        written += drain(1);
        FlushFileBuffers(g_log);
        CloseHandle(g_log);
        g_log = INVALID_HANDLE_VALUE;
        diag("session done: written=%u dropped=%d forced=%d head=%d",
             written, (int)g_dropped, (int)g_forced, (int)g_head);

        /* Park until the operator has created *and then removed* the STOP file. The two
         * phases keep a maxrec-terminated session from re-arming on its own. */
        while (GetFileAttributesA(g_stoppath) == INVALID_FILE_ATTRIBUTES) Sleep(500);
        while (GetFileAttributesA(g_stoppath) != INVALID_FILE_ATTRIBUTES) Sleep(500);

        g_head = 0; g_tail = 0; g_dropped = 0; g_forced = 0;
        memset((void *)g_ready, 0, sizeof(g_ready));
        g_nhooks = 0;
        InterlockedExchange(&g_stopping, 0);
        load_cfg(g_self_mod);
        diag("re-armed: hooks=%d log=%s", g_nhooks, g_logpath);
        if (!g_nhooks) return 0;
    }
}

/* ------------------------------------------------------------------ config io -- */

static void trim(char *s) {
    int n = (int)strlen(s);
    while (n && (s[n-1] == '\r' || s[n-1] == '\n' || s[n-1] == ' ' || s[n-1] == '\t')) s[--n] = 0;
}

static void load_cfg(HINSTANCE self) {
    char path[MAX_PATH], line[512];
    FILE *fp;
    char *p;
    /* <dll basename>.cfg, so a second copy loaded under another filename gets its own
     * config and its own STOP file and cannot fight the first over the same hooks. */
    GetModuleFileNameA(self, path, sizeof(path));
    p = strrchr(path, '.');
    if (p && (int)(p - path) > 0 && strlen(p) <= 5) strcpy(p, ".cfg");
    else strcat(path, ".cfg");
    fp = fopen(path, "r");
    if (!fp) { diag("no cfg at %s - defaults only", path); return; }
    while (fgets(line, sizeof(line), fp)) {
        trim(line);
        if (line[0] == '#' || line[0] == 0) continue;
        if      (!strncmp(line, "log=",    4)) strncpy(g_logpath,  line + 4, MAX_PATH - 1);
        else if (!strncmp(line, "stop=",   5)) strncpy(g_stoppath, line + 5, MAX_PATH - 1);
        else if (!strncmp(line, "diag=",   5)) strncpy(g_diagpath, line + 5, MAX_PATH - 1);
        else if (!strncmp(line, "derive=", 7)) g_derive = atoi(line + 7);
        else if (!strncmp(line, "maxrec=", 7)) g_maxrec = (unsigned)strtoul(line + 7, NULL, 10);
        else if (!strncmp(line, "hook=",   5)) {
            unsigned rva = 0, stolen = 0; char nm[32]; int fix = -1;
            nm[0] = 0;
            if (sscanf(line + 5, "%x,%u,%31[^,],%d", &rva, &stolen, nm, &fix) >= 3 &&
                g_nhooks < MAX_HOOKS && stolen >= 5 && stolen <= MAX_STOLEN) {
                hook_t *h = &g_hooks[g_nhooks];
                memset(h, 0, sizeof(*h));
                h->rva = rva; h->stolen = stolen; h->relfix = fix;
                strncpy(h->name, nm, 31);
                h->target = !strcmp(nm, "damage")           ? TGT_DAMAGE
                          : !strcmp(nm, "get_attack")       ? TGT_GETATTACK
                          : !strcmp(nm, "get_armor")        ? TGT_GETARMOR
                          : !strcmp(nm, "unit_attack")      ? TGT_UNIT_ATK
                          : !strcmp(nm, "unit_armor")       ? TGT_UNIT_ARM
                          : !strcmp(nm, "build_attack")     ? TGT_BUILD_ATK
                          : !strcmp(nm, "build_armor")      ? TGT_BUILD_ARM : 9u;
                h->addr = (BYTE *)(g_base + rva);
                g_nhooks++;
            } else diag("bad hook line: %s", line);
        }
    }
    fclose(fp);
}

/* ------------------------------------------------------------------ entry ------ */

static DWORD WINAPI boot(LPVOID unused) {
    (void)unused;
    g_self  = GetCurrentProcess();
    g_base  = (unsigned)GetModuleHandleA(NULL);
    g_delta = (int)(g_base - 0x400000u);
    g_tls   = TlsAlloc();
    diag("donhook boot: main base=%08X delta=%08X pid=%u",
         g_base, (unsigned)g_delta, (unsigned)GetCurrentProcessId());
    load_cfg(g_self_mod);
    if (!g_nhooks) { diag("no hooks configured, idling"); return 0; }
    g_retstub = build_retstub();
    if (!g_retstub) { diag("retstub alloc failed"); return 0; }
    diag("retstub at %08X, hooks=%d", (unsigned)g_retstub, g_nhooks);
    worker(NULL);
    return 0;
}

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        g_self_mod = h;
        DisableThreadLibraryCalls(h);
        CreateThread(NULL, 0, boot, NULL, 0, NULL);
    } else if (reason == DLL_PROCESS_DETACH) {
        remove_all();
    }
    return TRUE;
}

__declspec(dllexport) void donhook_stop(void) { remove_all(); }
