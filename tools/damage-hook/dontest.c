/*
 * dontest.exe — a self-started x86-32 target that mimics FUN_00644130's ABI exactly, so
 * the detour machinery can be proven under the ARM64 x86 emulator before anything touches
 * the running game.
 *
 * `donhook_target` reproduces the real function's shape byte for byte where it matters:
 *   - the same six-byte prologue `55 8B EC 83 EC 1C` (push ebp / mov ebp,esp / sub esp,0x1c)
 *   - __thiscall: ECX = this, six stack dwords, `ret 0x18`
 * so the same stolen-byte count and the same trampoline path are exercised.
 *
 * It writes its own module base and the function's RVA to a file, then calls the function
 * in a loop with values whose expected return is fully determined:
 *     result = *(int*)this + a1 + 2*a2 + 3*a3 + 4*a4 + 5*a5 + 6*a6
 * A hook that logs the wrong ECX, the wrong argument order or the wrong return will
 * disagree with that closed form immediately.
 *
 *   zig cc -target x86-windows-gnu -O2 -o dontest.exe dontest.c
 */

#include <windows.h>
#include <stdio.h>
#include <stdlib.h>

__asm__(
    ".text\n"
    ".globl _donhook_target\n"
    "_donhook_target:\n"
    "  push %ebp\n"                    /* 55          */
    "  mov  %esp, %ebp\n"              /* 8B EC       */
    "  sub  $0x1c, %esp\n"             /* 83 EC 1C    */
    "  push %ebx\n"
    "  push %esi\n"
    "  mov  (%ecx), %eax\n"            /* this->field */
    "  mov  0x08(%ebp), %esi\n"  "  add %esi, %eax\n"
    "  mov  0x0c(%ebp), %esi\n"  "  lea (%eax,%esi,2), %eax\n"
    "  mov  0x10(%ebp), %esi\n"  "  lea (%esi,%esi,2), %ebx\n" "  add %ebx, %eax\n"
    "  mov  0x14(%ebp), %esi\n"  "  lea (%eax,%esi,4), %eax\n"
    "  mov  0x18(%ebp), %esi\n"  "  lea (%esi,%esi,4), %ebx\n" "  add %ebx, %eax\n"
    "  mov  0x1c(%ebp), %esi\n"  "  lea (%esi,%esi,2), %ebx\n" "  lea (%eax,%ebx,2), %eax\n"
    "  pop  %esi\n"
    "  pop  %ebx\n"
    "  mov  %ebp, %esp\n"
    "  pop  %ebp\n"
    "  ret  $0x18\n"
);

extern void donhook_target(void);

/*
 * A second target whose prologue is shaped like Unit::get_attack (0x006103C0):
 * four bytes of preamble and then a `call rel32`, so a five-byte detour must swallow the
 * call and the trampoline must relocate its displacement. This exists purely to prove the
 * relfix path before it is pointed at the live game.
 */
__asm__(
    ".text\n"
    ".globl _dh_helper\n"
    "_dh_helper:\n"
    "  mov (%esi), %eax\n"
    "  ret\n"
    ".globl _donhook_target2\n"
    "_donhook_target2:\n"
    "  push %esi\n"                 /* 56       */
    "  push %edi\n"                 /* 57       */
    "  mov  %ecx, %esi\n"           /* 89 CE    */
    "  call _dh_helper\n"           /* E8 rel32 : offset 4, stolen must be 9 */
    "  mov  0x0c(%esp), %edi\n"  "  add %edi, %eax\n"
    "  mov  0x10(%esp), %edi\n"  "  lea (%eax,%edi,2), %eax\n"
    "  mov  0x14(%esp), %edi\n"  "  lea (%edi,%edi,2), %ecx\n" "  add %ecx, %eax\n"
    "  mov  0x18(%esp), %edi\n"  "  lea (%eax,%edi,4), %eax\n"
    "  mov  0x1c(%esp), %edi\n"  "  lea (%edi,%edi,4), %ecx\n" "  add %ecx, %eax\n"
    "  mov  0x20(%esp), %edi\n"  "  lea (%edi,%edi,2), %ecx\n" "  lea (%eax,%ecx,2), %eax\n"
    "  pop  %edi\n"
    "  pop  %esi\n"
    "  ret  $0x18\n"
);
extern void donhook_target2(void);

/*
 * The caller, also in asm, so the __thiscall sequence is exact: push a6..a1, ECX = obj,
 * call, callee cleans 0x18. Each source operand sits at esp+0x1c at the moment it is
 * pushed, because esp falls by 4 per push exactly as the argument being read moves 4
 * further away — which is why every load below uses the same displacement.
 */
__asm__(
    ".text\n"
    ".globl _call_target\n"
    "_call_target:\n"
    "  mov 0x1c(%esp), %eax\n  push %eax\n"   /* a6 */
    "  mov 0x1c(%esp), %eax\n  push %eax\n"   /* a5 */
    "  mov 0x1c(%esp), %eax\n  push %eax\n"   /* a4 */
    "  mov 0x1c(%esp), %eax\n  push %eax\n"   /* a3 */
    "  mov 0x1c(%esp), %eax\n  push %eax\n"   /* a2 */
    "  mov 0x1c(%esp), %eax\n  push %eax\n"   /* a1 */
    "  mov 0x1c(%esp), %ecx\n"                /* this */
    "  call _donhook_target\n"
    "  ret\n"
    ".globl _call_target2\n"
    "_call_target2:\n"
    "  mov 0x1c(%esp), %eax\n  push %eax\n"
    "  mov 0x1c(%esp), %eax\n  push %eax\n"
    "  mov 0x1c(%esp), %eax\n  push %eax\n"
    "  mov 0x1c(%esp), %eax\n  push %eax\n"
    "  mov 0x1c(%esp), %eax\n  push %eax\n"
    "  mov 0x1c(%esp), %eax\n  push %eax\n"
    "  mov 0x1c(%esp), %ecx\n"
    "  call _donhook_target2\n"
    "  ret\n"
);
extern int call_target(void *obj, int a1, int a2, int a3, int a4, int a5, int a6);
extern int call_target2(void *obj, int a1, int a2, int a3, int a4, int a5, int a6);

int main(int argc, char **argv) {
    unsigned base = (unsigned)(ULONG_PTR)GetModuleHandleA(NULL);
    unsigned fn   = (unsigned)(ULONG_PTR)donhook_target;
    int obj[4]    = { 1000, 0, 0, 0 };
    int dur_ms    = (argc > 1 ? atoi(argv[1]) : 90) * 1000;
    int burst     = argc > 2 ? atoi(argv[2]) : 200000;
    unsigned t0   = GetTickCount();
    int i = 0, bad = 0, burst_done = 0;
    FILE *f;

    printf("base=%08X target=%08X rva=%06X\n", base, fn, fn - base);
    f = fopen("C:\\Users\\ember\\donhook\\dontest_rva.txt", "w");
    if (f) { fprintf(f, "base=%08X\ntarget=%08X\nrva=%06X\nrva2=%06X\npid=%lu\n",
                     base, fn, fn - base,
                     (unsigned)(ULONG_PTR)donhook_target2 - base,
                     GetCurrentProcessId()); fclose(f); }
    fflush(stdout);

#define ONE_CALL()                                                                   \
    do {                                                                             \
        int a1 = i, a2 = i * 3 + 1, a3 = -i, a4 = i & 7, a5 = 100 + i, a6 = i * 2;   \
        int want = obj[0] + a1 + 2*a2 + 3*a3 + 4*a4 + 5*a5 + 6*a6;                   \
        int got  = call_target(obj, a1, a2, a3, a4, a5, a6);                         \
        int got2 = call_target2(obj, a1, a2, a3, a4, a5, a6);                        \
        if (got2 != want) { if (bad < 5) printf("MISMATCH2 i=%d got=%d want=%d\n", i, got2, want); bad++; } \
        if (got != want) {                                                           \
            if (bad < 5) printf("MISMATCH i=%d got=%d want=%d\n", i, got, want);      \
            bad++;                                                                   \
        }                                                                            \
        i++;                                                                         \
    } while (0)

    while ((int)(GetTickCount() - t0) < dur_ms) {
        ONE_CALL();
        Sleep(1);
        /* A tight burst once the hook has had time to attach: the slow phase proves
         * correctness, the burst proves the ring and the flusher keep up. */
        if (!burst_done && (int)(GetTickCount() - t0) > 25000) {
            unsigned b0 = GetTickCount(), k;
            int i0 = i;
            for (k = 0; k < (unsigned)burst; k++) ONE_CALL();
            printf("burst: %d calls in %u ms\n", i - i0, GetTickCount() - b0);
            fflush(stdout);
            burst_done = 1;
        }
    }
    printf("done calls=%d mismatches=%d\n", i, bad);
    return bad ? 1 : 0;
}
