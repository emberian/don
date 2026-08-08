/*
 * donject.exe — minimal 32-bit x86 DLL injector, plus a remote-module-base query.
 *
 *   donject.exe base <pid> <module.exe>     print the runtime base of a module in a process
 *   donject.exe inject <pid> <abs\dll path> LoadLibraryA it into the process
 *   donject.exe threads <pid>               list the process's threads (sanity check)
 *
 * Built for x86-32 so it shares an address space layout with the target: on ARM64 Windows
 * every x86 process in a boot session maps SysWOW64\kernel32.dll at the same base, so the
 * local LoadLibraryA address is valid remotely. Verified empirically before use — the
 * remote base is printed by `base <pid> kernel32.dll` and compared with the local one.
 *
 *   zig cc -target x86-windows-gnu -O2 -o donject.exe donject.c
 */

#include <windows.h>
#include <tlhelp32.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int stricmp_(const char *a, const char *b) {
    for (; *a && *b; a++, b++) {
        int ca = *a >= 'A' && *a <= 'Z' ? *a + 32 : *a;
        int cb = *b >= 'A' && *b <= 'Z' ? *b + 32 : *b;
        if (ca != cb) return ca - cb;
    }
    return (unsigned char)*a - (unsigned char)*b;
}

static unsigned module_base(DWORD pid, const char *name) {
    HANDLE snap;
    MODULEENTRY32 me;
    unsigned found = 0;
    int tries;
    for (tries = 0; tries < 20; tries++) {
        snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
        if (snap != INVALID_HANDLE_VALUE) break;
        Sleep(50);
    }
    if (snap == INVALID_HANDLE_VALUE) {
        fprintf(stderr, "snapshot failed err=%lu\n", GetLastError());
        return 0;
    }
    me.dwSize = sizeof(me);
    if (Module32First(snap, &me)) {
        do {
            if (!stricmp_(me.szModule, name)) { found = (unsigned)(ULONG_PTR)me.modBaseAddr; break; }
        } while (Module32Next(snap, &me));
    }
    CloseHandle(snap);
    return found;
}

static int inject(DWORD pid, const char *dll) {
    HANDLE proc, thr;
    void  *remote;
    SIZE_T wrote = 0;
    DWORD  exitcode = 0;
    size_t n = strlen(dll) + 1;
    FARPROC ll;

    proc = OpenProcess(PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION |
                       PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ,
                       FALSE, pid);
    if (!proc) { fprintf(stderr, "OpenProcess(%lu) failed err=%lu\n", pid, GetLastError()); return 2; }

    remote = VirtualAllocEx(proc, NULL, n, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!remote) { fprintf(stderr, "VirtualAllocEx failed err=%lu\n", GetLastError()); return 3; }
    if (!WriteProcessMemory(proc, remote, dll, n, &wrote) || wrote != n) {
        fprintf(stderr, "WriteProcessMemory failed err=%lu\n", GetLastError()); return 4;
    }

    ll = GetProcAddress(GetModuleHandleA("kernel32.dll"), "LoadLibraryA");
    printf("local kernel32=%08X LoadLibraryA=%08X remote kernel32=%08X\n",
           (unsigned)(ULONG_PTR)GetModuleHandleA("kernel32.dll"),
           (unsigned)(ULONG_PTR)ll, module_base(pid, "kernel32.dll"));
    if ((unsigned)(ULONG_PTR)GetModuleHandleA("kernel32.dll") != module_base(pid, "kernel32.dll")) {
        fprintf(stderr, "REFUSING: remote kernel32 base differs from local; "
                        "LoadLibraryA address would be wrong.\n");
        return 5;
    }

    thr = CreateRemoteThread(proc, NULL, 0, (LPTHREAD_START_ROUTINE)ll, remote, 0, NULL);
    if (!thr) { fprintf(stderr, "CreateRemoteThread failed err=%lu\n", GetLastError()); return 6; }
    WaitForSingleObject(thr, 15000);
    GetExitCodeThread(thr, &exitcode);
    CloseHandle(thr);
    printf("remote LoadLibraryA returned %08X\n", (unsigned)exitcode);
    /* deliberately not freeing `remote`: the loader may still reference the path string */
    return exitcode ? 0 : 7;
}

/* Follow a pointer chain in another process, read-only. Purely observational: used to
 * decide whether a live game is actually simulating before anything is patched into it. */
static int chain(DWORD pid, const char *mod, char **offs, int noffs, int reps, int delay) {
    HANDLE proc;
    unsigned base = module_base(pid, mod);
    int r, k;
    if (!base) { fprintf(stderr, "module %s not found in %lu\n", mod, pid); return 1; }
    proc = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid);
    if (!proc) { fprintf(stderr, "OpenProcess failed err=%lu\n", GetLastError()); return 2; }
    printf("base=%08X delta=%08X\n", base, base - 0x400000u);
    for (r = 0; r < reps; r++) {
        unsigned cur = base + (unsigned)strtoul(offs[0], NULL, 16);
        printf("%u:", (unsigned)GetTickCount());
        for (k = 1; k < noffs; k++) {
            unsigned off = (unsigned)strtoul(offs[k], NULL, 16), v = 0;
            SIZE_T got = 0;
            if (!ReadProcessMemory(proc, (LPCVOID)(cur + off), &v, 4, &got) || got != 4) {
                printf(" [%08X+%X]=<unreadable>", cur, off); cur = 0; break;
            }
            printf(" [%08X+%X]=%08X", cur, off, v);
            cur = v;
        }
        printf("\n");
        fflush(stdout);
        if (r + 1 < reps) Sleep(delay);
    }
    CloseHandle(proc);
    return 0;
}

/* Read-only hex dump: addr = base+rva, dereferenced `nderef` times, then `len` bytes
 * from addr+off. Used to capture the RULES block and the player array without writing
 * anything to the process. */
static int peek(DWORD pid, const char *mod, unsigned rva, int nderef,
                unsigned off, unsigned len) {
    HANDLE proc;
    unsigned base = module_base(pid, mod), cur;
    unsigned char *buf;
    SIZE_T got = 0;
    unsigned i;
    if (!base) { fprintf(stderr, "module %s not found\n", mod); return 1; }
    proc = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid);
    if (!proc) { fprintf(stderr, "OpenProcess failed err=%lu\n", GetLastError()); return 2; }
    cur = base + rva;
    for (i = 0; i < (unsigned)nderef; i++) {
        unsigned v = 0;
        if (!ReadProcessMemory(proc, (LPCVOID)cur, &v, 4, &got) || got != 4) {
            fprintf(stderr, "deref %u at %08X failed\n", i, cur); return 3;
        }
        cur = v;
    }
    cur += off;
    buf = (unsigned char *)malloc(len);
    if (!buf) return 4;
    if (!ReadProcessMemory(proc, (LPCVOID)cur, buf, len, &got) || got != len) {
        fprintf(stderr, "read %u bytes at %08X failed (got %u)\n", len, cur, (unsigned)got);
        return 5;
    }
    printf("# base=%08X addr=%08X len=%X\n", base, cur, len);
    for (i = 0; i < len; i += 16) {
        unsigned k;
        printf("%08X:", cur + i);
        for (k = 0; k < 16 && i + k < len; k++) printf(" %02X", buf[i + k]);
        printf("\n");
    }
    free(buf);
    CloseHandle(proc);
    return 0;
}

int main(int argc, char **argv) {
    if (argc >= 8 && !strcmp(argv[1], "peek"))
        return peek((DWORD)strtoul(argv[2], NULL, 10), argv[3],
                    (unsigned)strtoul(argv[4], NULL, 16), atoi(argv[5]),
                    (unsigned)strtoul(argv[6], NULL, 16),
                    (unsigned)strtoul(argv[7], NULL, 16));
    if (argc >= 5 && !strcmp(argv[1], "chain"))
        return chain((DWORD)strtoul(argv[2], NULL, 10), argv[3], argv + 4, argc - 4, 1, 0);
    if (argc >= 7 && !strcmp(argv[1], "watch")) {
        int reps  = atoi(argv[argc - 2]);
        int delay = atoi(argv[argc - 1]);
        return chain((DWORD)strtoul(argv[2], NULL, 10), argv[3], argv + 4, argc - 6, reps, delay);
    }
    if (argc >= 4 && !strcmp(argv[1], "base")) {
        unsigned b = module_base((DWORD)strtoul(argv[2], NULL, 10), argv[3]);
        printf("%08X\n", b);
        return b ? 0 : 1;
    }
    if (argc >= 4 && !strcmp(argv[1], "inject"))
        return inject((DWORD)strtoul(argv[2], NULL, 10), argv[3]);
    if (argc >= 3 && !strcmp(argv[1], "threads")) {
        DWORD pid = (DWORD)strtoul(argv[2], NULL, 10);
        HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        THREADENTRY32 te; int n = 0;
        te.dwSize = sizeof(te);
        if (snap != INVALID_HANDLE_VALUE && Thread32First(snap, &te)) {
            do { if (te.th32OwnerProcessID == pid) { printf("tid %lu\n", te.th32ThreadID); n++; } }
            while (Thread32Next(snap, &te));
        }
        printf("%d threads\n", n);
        return 0;
    }
    fprintf(stderr, "usage: donject base|inject|threads ...\n");
    return 1;
}
