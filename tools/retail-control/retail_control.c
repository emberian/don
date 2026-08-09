/*
 * retail_control.dll -- fail-closed, main-thread command ingress for the one
 * supported Rise of Nations: Extended Edition executable.
 *
 * The worker thread only parses requests and writes observations.  Retail
 * methods are called from a five-byte detour at TurnControl::do_frame, so the
 * CommandManager and CommandPackage are never mutated from our worker thread.
 * STOP removes the detour; deleting STOP re-arms the already-loaded DLL.
 *
 * Build: zig cc -target x86-windows-gnu -O2 -shared -o retail_control.dll retail_control.c
 */

#include <windows.h>
#include <tlhelp32.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define ROOT "C:\\Users\\Public\\don-retail-control"
#define REQUEST_PATH ROOT "\\request.txt"
#define EVENTS_PATH  ROOT "\\events.ndjson"
#define READY_PATH   ROOT "\\ready.txt"
#define STOP_PATH    ROOT "\\STOP"
#define LOG_PATH     ROOT "\\retail-control.log"

/* Supported image identity, independently present in donscan::live. */
#define EXPECTED_MACHINE   0x014cu
#define EXPECTED_ENTRY_RVA 0x0015d699u
#define EXPECTED_IMAGE_SIZE 0x00bb4000u
#define PREFERRED_BASE     0x00400000u

/* Shipped-PDB VAs expressed as RVAs. */
#define RVA_TURN_DO_FRAME  (0x00957dd0u - PREFERRED_BASE)
/* Sole direct call in Game::loop; its rel32 target is TurnControl::do_frame. */
#define RVA_TURN_CALL_SITE (0x00591686u - PREFERRED_BASE)
#define RVA_GAME_PTR       (0x00c061ecu - PREFERRED_BASE)
#define RVA_TURN_PTR       (0x00c06180u - PREFERRED_BASE)
#define RVA_OBJECTS_PTR    (0x00c0618cu - PREFERRED_BASE)
#define RVA_COMMAND_MANAGER (0x00e8ff60u - PREFERRED_BASE)
#define RVA_COMMAND_PACKAGE (0x00e8ff88u - PREFERRED_BASE)
#define RVA_ISSUE_CHECKSUM (0x00940770u - PREFERRED_BASE)
#define RVA_ISSUE_SPEED_DOWN (0x00940b00u - PREFERRED_BASE)
#define RVA_ISSUE_SPEED_UP (0x00940b30u - PREFERRED_BASE)
#define RVA_ISSUE_SPEED_SET (0x00940b60u - PREFERRED_BASE)
#define RVA_ISSUE_PAUSE    (0x00940ba0u - PREFERRED_BASE)
#define RVA_ISSUE_ATTACK   (0x009415e0u - PREFERRED_BASE)
#define RVA_ISSUE_MOVE     (0x00941720u - PREFERRED_BASE)
#define RVA_ISSUE_HALT     (0x009418d0u - PREFERRED_BASE)

#define OFF_GAME_FRAME 0x550u
#define OFF_GAME_SECONDS 0x560u
#define OFF_GAME_SEMAPHORE 0x820u
#define OFF_TURN_FLAGS 0x10u
#define OFF_OBJECTS_LISTS 0x04u
#define OBJECTS_ARRAY_STRIDE 28u
#define OFF_ARRAY_LIST 0x10u
#define OFF_UNIT_MARK 0x15cu
#define OFF_OBJ_FLAGS 0x08u
#define OFF_UNIT_CURRENT_ORDER 0xccu
#define OFF_UNIT_ORDER_LENGTH 0xd8u

#define PACKAGE_LENGTH 0x10u
#define PACKAGE_BYTES  0x12u
#define PACKAGE_CAP    0x201u
#define GROUP_NUM      0x0cu
#define GROUP_WHO      0x4au
#define GROUP_LIST     0x8ccu
#define GROUP_SIZE     0x9d0u

#define MAX_IDS 128
#define MAX_COMMAND_CAPTURE 160
#define EVENT_CAP 256

enum Verb {
    V_NONE = 0,
    V_OBSERVE,
    V_PAUSE,
    V_SPEED_SET,
    V_SPEED_UP,
    V_SPEED_DOWN,
    V_CHECKSUM,
    V_MOVE,
    V_HALT,
    V_ATTACK
};

typedef struct {
    unsigned seq;
    unsigned verb;
    int arg[10];
    int num_ids;
    short ids[MAX_IDS];
} request_t;

typedef struct {
    unsigned seq;
    unsigned verb;
    unsigned phase;             /* 0 observed, 1 queued, 2 applied, 3 timeout, 4 rejected */
    unsigned win_tick;
    unsigned game;
    unsigned frame;
    unsigned seconds;
    unsigned network;
    int paused;
    int speed;
    int package_before;
    int package_after;
    unsigned command_len;
    unsigned char command[MAX_COMMAND_CAPTURE];
    unsigned first_object;
    int order_length;
    unsigned current_order;
    unsigned current_order_vtable;
    unsigned note;
} event_t;

typedef struct {
    int active;
    request_t req;
    unsigned started;
    int initial_order_length;
    unsigned initial_order_vtable;
} verify_t;

static unsigned g_base;
static HANDLE g_self_process;
static volatile LONG g_pending;
static volatile LONG g_stopping;
static request_t g_request;
static verify_t g_verify;

static event_t g_events[EVENT_CAP];
static volatile LONG g_event_head;
static volatile LONG g_event_tail;

static BYTE *g_hook_addr;
static BYTE g_hook_orig[5];
static BYTE *g_trampoline;
static int g_hook_installed;

static void log_line(const char *line) {
    HANDLE h;
    DWORD n;
    h = CreateFileA(LOG_PATH, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
                    NULL, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (h != INVALID_HANDLE_VALUE) {
        WriteFile(h, line, (DWORD)strlen(line), &n, NULL);
        WriteFile(h, "\r\n", 2, &n, NULL);
        CloseHandle(h);
    }
}

static int safe_read(unsigned addr, void *out, unsigned len) {
    SIZE_T got = 0;
    if (addr < 0x10000u || addr >= 0x80000000u) return 0;
    return ReadProcessMemory(g_self_process, (LPCVOID)addr, out, len, &got) && got == len;
}

static int rd32(unsigned addr, unsigned *out) { return safe_read(addr, out, 4); }
static int rd16(unsigned addr, short *out) { return safe_read(addr, out, 2); }

static unsigned object_ptr(unsigned who, int id, int *is_unit) {
    unsigned objects = 0, list = 0, object = 0, mark = 0;
    if (is_unit) *is_unit = 0;
    if (who >= 10 || id < 0 || id > 32767) return 0;
    if (!rd32(g_base + RVA_OBJECTS_PTR, &objects) || !objects) return 0;
    if (!rd32(objects + OFF_OBJECTS_LISTS + who * OBJECTS_ARRAY_STRIDE + OFF_ARRAY_LIST,
              &list) || !list) return 0;
    if (!rd32(list + (unsigned)id * 4u, &object) || !object) return 0;
    {
        unsigned char flags = 0;
        if (!safe_read(object + OFF_OBJ_FLAGS, &flags, 1) || !(flags & 1)) return 0;
    }
    if (rd32(objects + OFF_UNIT_MARK + who * 4u, &mark) && (unsigned)id < mark) {
        if (is_unit) *is_unit = 1;
    }
    return object;
}

static void observe_unit(event_t *e, const request_t *r) {
    int is_unit = 0;
    unsigned p = 0, order = 0, vtable = 0;
    int length = -1;
    e->first_object = 0;
    e->order_length = -1;
    e->current_order = 0;
    e->current_order_vtable = 0;
    if (!r || r->num_ids <= 0) return;
    p = object_ptr((unsigned)r->arg[0], r->ids[0], &is_unit);
    e->first_object = p;
    if (!p || !is_unit) return;
    if (safe_read(p + OFF_UNIT_ORDER_LENGTH, &length, 4)) e->order_length = length;
    if (rd32(p + OFF_UNIT_CURRENT_ORDER, &order)) {
        e->current_order = order;
        if (order) rd32(order, &vtable);
        e->current_order_vtable = vtable;
    }
}

static void snapshot(event_t *e, const request_t *r) {
    unsigned game = 0, turn = 0, flags = 0, sem = 0;
    e->win_tick = GetTickCount();
    e->paused = -1;
    e->speed = -1;
    if (rd32(g_base + RVA_GAME_PTR, &game) && game) {
        e->game = game;
        rd32(game + OFF_GAME_FRAME, &e->frame);
        rd32(game + OFF_GAME_SECONDS, &e->seconds);
        if (rd32(game + OFF_GAME_SEMAPHORE, &sem)) e->network = (sem & 4u) != 0;
    }
    if (rd32(g_base + RVA_TURN_PTR, &turn) && turn) {
        if (rd32(turn + OFF_TURN_FLAGS, &flags)) e->paused = (flags & 1u) != 0;
        safe_read(turn + 0x30u, &e->speed, 4);
    }
    observe_unit(e, r);
}

static void push_event(const event_t *event) {
    LONG head = g_event_head;
    LONG tail = g_event_tail;
    if (head - tail >= EVENT_CAP) return;
    g_events[(unsigned)head & (EVENT_CAP - 1)] = *event;
    MemoryBarrier();
    InterlockedIncrement(&g_event_head);
}

typedef void (__attribute__((thiscall)) *fn_void0)(void *self);
typedef void (__attribute__((thiscall)) *fn_int1)(void *self, int a);
typedef void (__attribute__((thiscall)) *fn_group1)(void *self, const void *group);
typedef void (__attribute__((thiscall)) *fn_attack)(void *self, const void *group,
                                                    int who, int id, int flags, int queued);
typedef void (__attribute__((thiscall)) *fn_move)(void *self, const void *group,
                                                  int x, int y, int queued,
                                                  int set_angle, int angle, int orders,
                                                  int form, int width, int disembark);

static void make_group(unsigned char group[GROUP_SIZE], const request_t *r) {
    int i;
    memset(group, 0, GROUP_SIZE);
    *(int *)(group + GROUP_NUM) = r->num_ids;
    group[GROUP_WHO] = (unsigned char)r->arg[0];
    for (i = 0; i < r->num_ids; i++)
        *(short *)(group + GROUP_LIST + i * 2) = r->ids[i];
}

static int package_length(void) {
    short n = -1;
    if (!rd16(g_base + RVA_COMMAND_PACKAGE + PACKAGE_LENGTH, &n)) return -1;
    if (n < 0 || (unsigned)n > PACKAGE_CAP) return -1;
    return n;
}

static void capture_append(event_t *e) {
    int delta;
    e->package_after = package_length();
    if (e->package_before < 0 || e->package_after <= e->package_before) return;
    delta = e->package_after - e->package_before;
    if (delta > MAX_COMMAND_CAPTURE) delta = MAX_COMMAND_CAPTURE;
    if (safe_read(g_base + RVA_COMMAND_PACKAGE + PACKAGE_BYTES + (unsigned)e->package_before,
                  e->command, (unsigned)delta))
        e->command_len = (unsigned)delta;
}

static int dispatch(const request_t *r, event_t *e) {
    void *manager = (void *)(g_base + RVA_COMMAND_MANAGER);
    unsigned char group[GROUP_SIZE];
    e->package_before = package_length();
    switch (r->verb) {
        case V_OBSERVE:
            return 1;
        case V_PAUSE:
            ((fn_int1)(g_base + RVA_ISSUE_PAUSE))(manager, r->arg[0]);
            break;
        case V_SPEED_SET:
            ((fn_int1)(g_base + RVA_ISSUE_SPEED_SET))(manager, r->arg[0]);
            break;
        case V_SPEED_UP:
            ((fn_void0)(g_base + RVA_ISSUE_SPEED_UP))(manager);
            break;
        case V_SPEED_DOWN:
            ((fn_void0)(g_base + RVA_ISSUE_SPEED_DOWN))(manager);
            break;
        case V_CHECKSUM:
            ((fn_void0)(g_base + RVA_ISSUE_CHECKSUM))(manager);
            break;
        case V_MOVE:
            make_group(group, r);
            ((fn_move)(g_base + RVA_ISSUE_MOVE))(manager, group,
                r->arg[1], r->arg[2], r->arg[3], r->arg[4], r->arg[5],
                r->arg[6], r->arg[7], r->arg[8], r->arg[9]);
            break;
        case V_HALT:
            make_group(group, r);
            ((fn_group1)(g_base + RVA_ISSUE_HALT))(manager, group);
            break;
        case V_ATTACK:
            make_group(group, r);
            ((fn_attack)(g_base + RVA_ISSUE_ATTACK))(manager, group,
                r->arg[1], r->arg[2], r->arg[3], r->arg[4]);
            break;
        default:
            return 0;
    }
    capture_append(e);
    return r->verb == V_OBSERVE || e->package_after > e->package_before;
}

static int verify_applied(event_t *e, const verify_t *v) {
    switch (v->req.verb) {
        case V_PAUSE:
            return e->paused == !!v->req.arg[0];
        case V_SPEED_SET:
            return e->speed == v->req.arg[0];
        case V_HALT:
            return e->order_length == 0 && v->initial_order_length != 0;
        case V_MOVE:
        case V_ATTACK:
            return e->order_length >= 0 &&
                   (e->order_length != v->initial_order_length ||
                    e->current_order_vtable != v->initial_order_vtable);
        default:
            return 0;
    }
}

/* Runs on the retail main thread immediately before and after TurnControl::do_frame. */
static void __cdecl on_turn_frame(void) {
    event_t e;
    request_t r;
    LONG verb;
    if (g_stopping) return;

    if (g_verify.active) {
        memset(&e, 0, sizeof(e));
        e.seq = g_verify.req.seq;
        e.verb = g_verify.req.verb;
        snapshot(&e, &g_verify.req);
        if (verify_applied(&e, &g_verify)) {
            e.phase = 2;
            push_event(&e);
            g_verify.active = 0;
        } else if ((unsigned)(e.win_tick - g_verify.started) >= 3000u) {
            e.phase = 3;
            push_event(&e);
            g_verify.active = 0;
        }
    }

    verb = InterlockedExchange(&g_pending, V_NONE);
    if (verb == V_NONE) return;
    MemoryBarrier();
    r = g_request;
    memset(&e, 0, sizeof(e));
    e.seq = r.seq;
    e.verb = r.verb;
    snapshot(&e, &r);
    if (dispatch(&r, &e)) {
        e.phase = r.verb == V_OBSERVE ? 0 : 1;
        push_event(&e);
        if (r.verb == V_PAUSE || r.verb == V_SPEED_SET || r.verb == V_MOVE ||
            r.verb == V_HALT || r.verb == V_ATTACK) {
            g_verify.active = 1;
            g_verify.req = r;
            g_verify.started = GetTickCount();
            g_verify.initial_order_length = e.order_length;
            g_verify.initial_order_vtable = e.current_order_vtable;
        }
    } else {
        e.phase = 4;
        e.note = 1; /* retail gate rejected it or package had no room */
        push_event(&e);
    }
}

static BYTE *emit8(BYTE *p, BYTE v) { *p++ = v; return p; }
static BYTE *emit32(BYTE *p, unsigned v) { memcpy(p, &v, 4); return p + 4; }

static BYTE *build_trampoline(void) {
    BYTE *m = (BYTE *)VirtualAlloc(NULL, 0x1000, MEM_COMMIT | MEM_RESERVE,
                                   PAGE_EXECUTE_READWRITE);
    BYTE *p;
    if (!m) return NULL;
    memset(m, 0xcc, 0x1000);
    p = m;
    /* Pre-turn: drain one request while preserving the retail caller's registers. */
    p = emit8(p, 0x9c);                         /* pushfd */
    p = emit8(p, 0x60);                         /* pushad */
    p = emit8(p, 0xe8);                         /* call rel32 */
    p = emit32(p, (unsigned)((BYTE *)on_turn_frame - (p + 4)));
    p = emit8(p, 0x61);                         /* popad */
    p = emit8(p, 0x9d);                         /* popfd */
    p = emit8(p, 0xe8);                         /* original TurnControl::do_frame */
    p = emit32(p, (unsigned)((BYTE *)(g_base + RVA_TURN_DO_FRAME) - (p + 4)));
    /* Post-turn: publish applied-state evidence, preserving original EAX. */
    p = emit8(p, 0x9c);
    p = emit8(p, 0x60);
    p = emit8(p, 0xe8);
    p = emit32(p, (unsigned)((BYTE *)on_turn_frame - (p + 4)));
    p = emit8(p, 0x61);
    p = emit8(p, 0x9d);
    p = emit8(p, 0xc3);                         /* return to Game::loop */
    FlushInstructionCache(g_self_process, m, 0x1000);
    return m;
}

static int suspend_others(HANDLE *threads, int cap) {
    HANDLE snap;
    THREADENTRY32 te;
    DWORD pid = GetCurrentProcessId(), self = GetCurrentThreadId();
    int n = 0;
    snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    if (snap == INVALID_HANDLE_VALUE) return 0;
    te.dwSize = sizeof(te);
    if (Thread32First(snap, &te)) do {
        HANDLE h;
        if (te.th32OwnerProcessID != pid || te.th32ThreadID == self || n >= cap) continue;
        h = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT, FALSE, te.th32ThreadID);
        if (h && SuspendThread(h) != (DWORD)-1) threads[n++] = h;
        else if (h) CloseHandle(h);
    } while (Thread32Next(snap, &te));
    CloseHandle(snap);
    return n;
}

static void resume_all(HANDLE *threads, int n) {
    int i;
    for (i = 0; i < n; i++) { ResumeThread(threads[i]); CloseHandle(threads[i]); }
}

static int eip_in_patch(HANDLE *threads, int n) {
    int i;
    CONTEXT c;
    for (i = 0; i < n; i++) {
        memset(&c, 0, sizeof(c));
        c.ContextFlags = CONTEXT_CONTROL;
        if (GetThreadContext(threads[i], &c) &&
            c.Eip >= (DWORD)g_hook_addr && c.Eip < (DWORD)g_hook_addr + 5)
            return 1;
    }
    return 0;
}

static int quiesce(HANDLE *threads, int cap) {
    int n = 0, tries;
    for (tries = 0; tries < 30; tries++) {
        n = suspend_others(threads, cap);
        if (!eip_in_patch(threads, n)) return n;
        resume_all(threads, n);
        Sleep(2);
    }
    return -1;
}

static int write_code(BYTE *dst, const BYTE *src, unsigned n) {
    DWORD old;
    SIZE_T wrote = 0;
    if (!VirtualProtect(dst, n, PAGE_EXECUTE_READWRITE, &old)) return 0;
    memcpy(dst, src, n);
    VirtualProtect(dst, n, old, &old);
    FlushInstructionCache(g_self_process, dst, n);
    WriteProcessMemory(g_self_process, dst, src, n, &wrote);
    FlushInstructionCache(g_self_process, dst, n);
    return wrote == n;
}

static int install_hook(void) {
    static const BYTE expected[5] = {0xe8, 0x45, 0x67, 0x3c, 0x00};
    BYTE patch[5];
    HANDLE threads[128];
    int n, ok;
    if (g_hook_installed) return 1;
    if (memcmp(g_hook_addr, expected, 5) != 0) {
        log_line("REFUSED: Game::loop TurnControl call site does not match supported image");
        return 0;
    }
    memcpy(g_hook_orig, g_hook_addr, 5);
    if (!g_trampoline) g_trampoline = build_trampoline();
    if (!g_trampoline) return 0;
    patch[0] = 0xe8;
    *(int *)(patch + 1) = (int)(g_trampoline - (g_hook_addr + 5));
    n = quiesce(threads, 128);
    if (n < 0) return 0;
    ok = write_code(g_hook_addr, patch, 5);
    resume_all(threads, n);
    if (ok) { g_hook_installed = 1; log_line("hook installed"); }
    return ok;
}

static int remove_hook(void) {
    HANDLE threads[128];
    int n, ok;
    if (!g_hook_installed) return 1;
    InterlockedExchange(&g_stopping, 1);
    Sleep(30);
    n = quiesce(threads, 128);
    if (n < 0) return 0;
    ok = write_code(g_hook_addr, g_hook_orig, 5);
    resume_all(threads, n);
    if (ok) { g_hook_installed = 0; log_line("hook removed; DLL parked"); }
    return ok;
}

static int image_supported(void) {
    IMAGE_DOS_HEADER *dos = (IMAGE_DOS_HEADER *)g_base;
    IMAGE_NT_HEADERS32 *nt;
    if (dos->e_magic != IMAGE_DOS_SIGNATURE) return 0;
    nt = (IMAGE_NT_HEADERS32 *)(g_base + (unsigned)dos->e_lfanew);
    return nt->Signature == IMAGE_NT_SIGNATURE &&
           nt->FileHeader.Machine == EXPECTED_MACHINE &&
           nt->OptionalHeader.AddressOfEntryPoint == EXPECTED_ENTRY_RVA &&
           nt->OptionalHeader.SizeOfImage == EXPECTED_IMAGE_SIZE;
}

static unsigned parse_uint(const char *s, int *ok) {
    char *end;
    unsigned long v = strtoul(s, &end, 0);
    if (!s[0] || *end) { *ok = 0; return 0; }
    return (unsigned)v;
}

static int parse_int(const char *s, int *ok) {
    char *end;
    long v = strtol(s, &end, 0);
    if (!s[0] || *end || v < -2147483647L || v > 2147483647L) { *ok = 0; return 0; }
    return (int)v;
}

static int tokenize(char *line, char **tok, int cap) {
    int n = 0;
    char *p = strtok(line, " \t\r\n");
    while (p && n < cap) { tok[n++] = p; p = strtok(NULL, " \t\r\n"); }
    return n;
}

/* Protocol:
 * seq observe
 * seq pause 0|1
 * seq speed N | speed-up | speed-down | checksum
 * seq halt WHO ID...
 * seq move WHO X Y QUEUED ORDER FORM WIDTH DISEMBARK ID...
 * seq attack WHO TARGET_WHO TARGET_ID FLAGS QUEUED ID...
 */
static int parse_request(char *line, request_t *r) {
    char *t[160];
    int n = tokenize(line, t, 160), ok = 1, i, first = 0;
    memset(r, 0, sizeof(*r));
    if (n < 2) return 0;
    r->seq = parse_uint(t[0], &ok);
    if (!ok || !r->seq) return 0;
    if (!strcmp(t[1], "observe") && n == 2) r->verb = V_OBSERVE;
    else if (!strcmp(t[1], "pause") && n == 3) {
        r->verb = V_PAUSE; r->arg[0] = parse_int(t[2], &ok);
        if (r->arg[0] != 0 && r->arg[0] != 1) ok = 0;
    } else if (!strcmp(t[1], "speed") && n == 3) {
        r->verb = V_SPEED_SET; r->arg[0] = parse_int(t[2], &ok);
        if (r->arg[0] < 0 || r->arg[0] > 4) ok = 0;
    } else if (!strcmp(t[1], "speed-up") && n == 2) r->verb = V_SPEED_UP;
    else if (!strcmp(t[1], "speed-down") && n == 2) r->verb = V_SPEED_DOWN;
    else if (!strcmp(t[1], "checksum") && n == 2) r->verb = V_CHECKSUM;
    else if (!strcmp(t[1], "halt") && n >= 4) {
        r->verb = V_HALT; r->arg[0] = parse_int(t[2], &ok); first = 3;
    } else if (!strcmp(t[1], "move") && n >= 11) {
        r->verb = V_MOVE;
        /* Protocol omits retail's optional set_angle/angle pair, both set to zero. */
        r->arg[0] = parse_int(t[2], &ok); r->arg[1] = parse_int(t[3], &ok);
        r->arg[2] = parse_int(t[4], &ok); r->arg[3] = parse_int(t[5], &ok);
        r->arg[4] = 0; r->arg[5] = 0; r->arg[6] = parse_int(t[6], &ok);
        r->arg[7] = parse_int(t[7], &ok); r->arg[8] = parse_int(t[8], &ok);
        r->arg[9] = parse_int(t[9], &ok); first = 10;
    } else if (!strcmp(t[1], "attack") && n >= 8) {
        r->verb = V_ATTACK;
        for (i = 0; i < 5; i++) r->arg[i] = parse_int(t[2 + i], &ok);
        first = 7;
    } else return 0;
    if (!ok) return 0;
    if ((r->verb == V_MOVE || r->verb == V_HALT || r->verb == V_ATTACK) &&
        (r->arg[0] < 0 || r->arg[0] >= 10)) return 0;
    if (first) {
        r->num_ids = n - first;
        if (r->num_ids <= 0 || r->num_ids > MAX_IDS) return 0;
        for (i = 0; i < r->num_ids; i++) {
            int id = parse_int(t[first + i], &ok);
            if (!ok || id < 0 || id > 32767) return 0;
            r->ids[i] = (short)id;
        }
    }
    return 1;
}

static int read_request(char *buf, unsigned cap) {
    HANDLE h;
    DWORD got = 0;
    h = CreateFileA(REQUEST_PATH, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE,
                    NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (h == INVALID_HANDLE_VALUE) return 0;
    if (!ReadFile(h, buf, cap - 1, &got, NULL)) got = 0;
    CloseHandle(h);
    buf[got] = 0;
    return got != 0;
}

static const char *verb_name(unsigned verb) {
    switch (verb) {
        case V_OBSERVE: return "observe"; case V_PAUSE: return "pause";
        case V_SPEED_SET: return "speed"; case V_SPEED_UP: return "speed-up";
        case V_SPEED_DOWN: return "speed-down"; case V_CHECKSUM: return "checksum";
        case V_MOVE: return "move"; case V_HALT: return "halt";
        case V_ATTACK: return "attack"; default: return "unknown";
    }
}

static const char *phase_name(unsigned phase) {
    switch (phase) {
        case 0: return "observed"; case 1: return "queued"; case 2: return "applied";
        case 3: return "timeout"; case 4: return "rejected"; default: return "unknown";
    }
}

static void write_event(const event_t *e) {
    char line[1800], hex[MAX_COMMAND_CAPTURE * 2 + 1];
    unsigned i;
    HANDLE h;
    DWORD wrote;
    for (i = 0; i < e->command_len && i < MAX_COMMAND_CAPTURE; i++)
        sprintf(hex + i * 2, "%02x", e->command[i]);
    hex[i * 2] = 0;
    _snprintf(line, sizeof(line) - 1,
        "{\"seq\":%u,\"verb\":\"%s\",\"phase\":\"%s\","
        "\"tick\":%u,\"game\":\"0x%08x\",\"frame\":%u,\"seconds\":%u,"
        "\"paused\":%d,\"speed\":%d,\"network\":%u,\"package_before\":%d,"
        "\"package_after\":%d,\"command_hex\":\"%s\","
        "\"first_object\":\"0x%08x\",\"order_length\":%d,"
        "\"current_order\":\"0x%08x\",\"order_vtable\":\"0x%08x\","
        "\"note\":%u}\r\n",
        e->seq, verb_name(e->verb), phase_name(e->phase), e->win_tick, e->game,
        e->frame, e->seconds, e->paused, e->speed, e->network, e->package_before,
        e->package_after, hex, e->first_object, e->order_length,
        e->current_order, e->current_order_vtable, e->note);
    line[sizeof(line) - 1] = 0;
    h = CreateFileA(EVENTS_PATH, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
                    NULL, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (h != INVALID_HANDLE_VALUE) {
        WriteFile(h, line, (DWORD)strlen(line), &wrote, NULL);
        FlushFileBuffers(h);
        CloseHandle(h);
    }
}

static void drain_events(void) {
    while (g_event_tail < g_event_head) {
        LONG tail = g_event_tail;
        MemoryBarrier();
        write_event(&g_events[(unsigned)tail & (EVENT_CAP - 1)]);
        InterlockedIncrement(&g_event_tail);
    }
}

static void write_ready(const char *state) {
    char buf[256];
    HANDLE h;
    DWORD wrote;
    _snprintf(buf, sizeof(buf), "state=%s\r\npid=%lu\r\nbase=0x%08x\r\n"
              "turn_call_site=0x%08x\r\nturn_do_frame=0x%08x\r\n", state,
              GetCurrentProcessId(), g_base, g_base + RVA_TURN_CALL_SITE,
              g_base + RVA_TURN_DO_FRAME);
    h = CreateFileA(READY_PATH, GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE,
                    NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (h != INVALID_HANDLE_VALUE) {
        WriteFile(h, buf, (DWORD)strlen(buf), &wrote, NULL);
        CloseHandle(h);
    }
}

static DWORD WINAPI worker(LPVOID unused) {
    char line[4096];
    unsigned last_seq = 0;
    (void)unused;
    CreateDirectoryA(ROOT, NULL);
    if (!image_supported()) {
        log_line("REFUSED: PE identity mismatch");
        write_ready("refused-image");
        return 0;
    }
    if (!install_hook()) {
        write_ready("refused-hook");
        return 0;
    }
    write_ready("armed");
    for (;;) {
        request_t r;
        drain_events();
        if (GetFileAttributesA(STOP_PATH) != INVALID_FILE_ATTRIBUTES) {
            remove_hook();
            write_ready("parked");
            while (GetFileAttributesA(STOP_PATH) != INVALID_FILE_ATTRIBUTES) {
                drain_events(); Sleep(100);
            }
            InterlockedExchange(&g_stopping, 0);
            if (!install_hook()) { write_ready("refused-rearm"); return 0; }
            write_ready("armed");
        }
        if (read_request(line, sizeof(line)) && parse_request(line, &r) &&
            r.seq != last_seq && g_pending == V_NONE) {
            g_request = r;
            MemoryBarrier();
            InterlockedExchange(&g_pending, (LONG)r.verb);
            last_seq = r.seq;
        }
        Sleep(20);
    }
}

BOOL WINAPI DllMain(HINSTANCE module, DWORD reason, LPVOID reserved) {
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        HANDLE thread;
        DisableThreadLibraryCalls(module);
        g_base = (unsigned)(ULONG_PTR)GetModuleHandleA(NULL);
        g_self_process = GetCurrentProcess();
        g_hook_addr = (BYTE *)(g_base + RVA_TURN_CALL_SITE);
        thread = CreateThread(NULL, 0, worker, NULL, 0, NULL);
        if (thread) CloseHandle(thread);
    }
    return TRUE;
}
