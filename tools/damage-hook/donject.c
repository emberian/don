/*
 * donject.exe -- fail-closed 32-bit x86 DLL injector and read-only process probe.
 *
 *   donject.exe base <pid> <module.exe>
 *   donject.exe inject <pid> <dll path> <expected-sha256>
 *   donject.exe threads <pid>
 *   donject.exe chain <pid> <module.exe> <rva> [offset ...]
 *   donject.exe watch <pid> <module.exe> <rva> [offset ...] <reps> <delay-ms>
 *   donject.exe peek <pid> <module.exe> <rva> <derefs> <offset> <length>
 *   donject.exe selftest
 *
 * The injector is intentionally built as PE32/i386. Injection accepts only the exact
 * supported retail executable or the repository's dontest.exe validation target. The
 * retail digest below is established in README-LLM.md and docs/binary-ground-truth.md.
 *
 * LoadLibraryW is resolved as an RVA in the local module which actually owns the export
 * (normally kernel32 or kernelbase), then rebased into that module in the remote process.
 * No equality of local and remote module bases is assumed.
 * DLL path arguments are restricted to 7-bit ASCII before conversion to UTF-16. This keeps
 * the narrow main() boundary independent of the guest's active ANSI code page.
 *
 *   zig cc -target x86-windows-gnu -O2 -o donject.exe donject.c
 */

#include <windows.h>
#include <tlhelp32.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <wchar.h>

#if defined(_WIN64)
#error donject must be built as a 32-bit executable
#endif

#define INJECT_WAIT_MS 15000u
#define MODULE_ENUM_LIMIT 4096u
#define SUPPORTED_RETAIL_SHA256 \
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"

typedef struct ModuleInfo_ {
    uintptr_t base;
    DWORD size;
    wchar_t name[MAX_MODULE_NAME32 + 1];
    wchar_t path[MAX_PATH];
} ModuleInfo;

typedef struct PeIdentity_ {
    WORD machine;
    WORD optional_magic;
    WORD characteristics;
    DWORD timestamp;
    DWORD image_size;
} PeIdentity;

typedef struct Sha256_ {
    uint32_t state[8];
    uint64_t bytes;
    unsigned char block[64];
    size_t used;
} Sha256;

typedef struct OpenedImage_ {
    HANDLE file;
    wchar_t *path;
    BY_HANDLE_FILE_INFORMATION identity;
    PeIdentity pe;
} OpenedImage;

enum LoadedState {
    LOADED_SCAN_ERROR = -1,
    LOADED_ABSENT = 0,
    LOADED_EXACT = 1,
    LOADED_BASENAME_COLLISION = 2,
    LOADED_EXACT_SIZE_MISMATCH = 3
};

static int stricmp_a(const char *a, const char *b) {
    for (; *a && *b; a++, b++) {
        int ca = *a >= 'A' && *a <= 'Z' ? *a + 32 : *a;
        int cb = *b >= 'A' && *b <= 'Z' ? *b + 32 : *b;
        if (ca != cb) return ca - cb;
    }
    return (unsigned char)*a - (unsigned char)*b;
}

static int stricmp_w(const wchar_t *a, const wchar_t *b) {
    for (; *a && *b; a++, b++) {
        wchar_t ca = *a >= L'A' && *a <= L'Z' ? *a + (L'a' - L'A') : *a;
        wchar_t cb = *b >= L'A' && *b <= L'Z' ? *b + (L'a' - L'A') : *b;
        if (ca != cb) return ca < cb ? -1 : 1;
    }
    return *a == *b ? 0 : (*a ? 1 : -1);
}

static const wchar_t *path_leaf_w(const wchar_t *path) {
    const wchar_t *leaf = path;
    const wchar_t *p;
    for (p = path; *p; p++) {
        if (*p == L'\\' || *p == L'/') leaf = p + 1;
    }
    return leaf;
}

static int parse_pid(const char *text, DWORD *out) {
    char *end = NULL;
    unsigned long value;
    if (!text || !*text || *text == '-') return 0;
    value = strtoul(text, &end, 10);
    if (!end || *end || value == 0 || value > 0xfffffffful) return 0;
    *out = (DWORD)value;
    return 1;
}

static int parse_sha256(const char *text, char normalized[65]) {
    size_t i;
    if (!text || strlen(text) != 64) return 0;
    for (i = 0; i < 64; i++) {
        char c = text[i];
        if (c >= '0' && c <= '9') normalized[i] = c;
        else if (c >= 'a' && c <= 'f') normalized[i] = c;
        else if (c >= 'A' && c <= 'F') normalized[i] = (char)(c + ('a' - 'A'));
        else return 0;
    }
    normalized[64] = 0;
    return 1;
}

static int ascii_argument(const char *text) {
    const unsigned char *p = (const unsigned char *)text;
    if (!p || !*p) return 0;
    for (; *p; p++) {
        if (*p > 0x7fu) return 0;
    }
    return 1;
}

static HANDLE module_snapshot(DWORD pid) {
    HANDLE snap = INVALID_HANDLE_VALUE;
    int tries;
    DWORD error = ERROR_SUCCESS;
    for (tries = 0; tries < 20; tries++) {
        snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
        if (snap != INVALID_HANDLE_VALUE) return snap;
        error = GetLastError();
        if (error != ERROR_BAD_LENGTH) break;
        Sleep(50);
    }
    SetLastError(error);
    return INVALID_HANDLE_VALUE;
}

static int module_by_name_w(DWORD pid, const wchar_t *name, ModuleInfo *out) {
    HANDLE snap = module_snapshot(pid);
    MODULEENTRY32W me;
    int found = 0;
    if (snap == INVALID_HANDLE_VALUE) return -1;
    memset(&me, 0, sizeof(me));
    me.dwSize = sizeof(me);
    if (!Module32FirstW(snap, &me)) {
        DWORD error = GetLastError();
        CloseHandle(snap);
        SetLastError(error);
        return -1;
    }
    do {
        if (!stricmp_w(me.szModule, name)) {
            if (out) {
                memset(out, 0, sizeof(*out));
                out->base = (uintptr_t)me.modBaseAddr;
                out->size = me.modBaseSize;
                wcsncpy(out->name, me.szModule, MAX_MODULE_NAME32);
                out->name[MAX_MODULE_NAME32] = 0;
                wcsncpy(out->path, me.szExePath, MAX_PATH - 1);
                out->path[MAX_PATH - 1] = 0;
            }
            found = 1;
            break;
        }
    } while (Module32NextW(snap, &me));
    if (!found && GetLastError() != ERROR_NO_MORE_FILES) {
        DWORD error = GetLastError();
        CloseHandle(snap);
        SetLastError(error);
        return -1;
    }
    CloseHandle(snap);
    return found;
}

static int multibyte_to_wide(const char *text, wchar_t **out) {
    int count;
    wchar_t *wide;
    count = MultiByteToWideChar(CP_ACP, 0, text, -1, NULL, 0);
    if (count <= 0) return 0;
    wide = (wchar_t *)malloc((size_t)count * sizeof(*wide));
    if (!wide) {
        SetLastError(ERROR_OUTOFMEMORY);
        return 0;
    }
    if (!MultiByteToWideChar(CP_ACP, 0, text, -1, wide, count)) {
        DWORD error = GetLastError();
        free(wide);
        SetLastError(error);
        return 0;
    }
    *out = wide;
    return 1;
}

static unsigned module_base(DWORD pid, const char *name) {
    wchar_t *wide = NULL;
    ModuleInfo info;
    int status;
    if (!multibyte_to_wide(name, &wide)) {
        fprintf(stderr, "module name conversion failed err=%lu\n", GetLastError());
        return 0;
    }
    status = module_by_name_w(pid, wide, &info);
    free(wide);
    if (status < 0) {
        fprintf(stderr, "module snapshot failed pid=%lu err=%lu\n", pid, GetLastError());
        return 0;
    }
    return status ? (unsigned)info.base : 0;
}

/* Stable machine-readable module query. A missing module is not a Toolhelp error:
 * callers use exit 10 to distinguish absence from exit 11 diagnostics. */
static int base_query(DWORD pid, const char *name) {
    wchar_t *wide = NULL;
    ModuleInfo info;
    int status;
    if (!multibyte_to_wide(name, &wide)) {
        fprintf(stderr,
                "protocol=donject.v2 command=base status=error stage=module-name-conversion "
                "pid=%lu win32_error=%lu\n",
                pid, GetLastError());
        return 11;
    }
    status = module_by_name_w(pid, wide, &info);
    if (status < 0) {
        DWORD error = GetLastError();
        fprintf(stderr,
                "protocol=donject.v2 command=base status=error stage=module-snapshot "
                "pid=%lu module_name=\"%ls\" win32_error=%lu\n",
                pid, wide, error);
        free(wide);
        return 11;
    }
    if (!status) {
        printf("protocol=donject.v2 command=base status=absent pid=%lu "
               "module_name=\"%ls\"\n", pid, wide);
        free(wide);
        return 10;
    }
    printf("protocol=donject.v2 command=base status=mapped pid=%lu "
           "module_name=\"%ls\" module_path=\"%ls\" module_base=0x%08lX "
           "module_size=0x%08lX\n",
           pid, info.name, info.path, (unsigned long)info.base, (unsigned long)info.size);
    free(wide);
    return 0;
}

static int modules_query(DWORD pid) {
    HANDLE snap = module_snapshot(pid);
    MODULEENTRY32W me;
    ModuleInfo *modules = NULL;
    size_t count = 0;
    size_t capacity = 0;
    size_t i;
    int result = 12;
    if (snap == INVALID_HANDLE_VALUE) {
        fprintf(stderr,
                "protocol=donject.v2 command=modules status=error stage=module-snapshot "
                "pid=%lu win32_error=%lu\n",
                pid, GetLastError());
        return result;
    }
    memset(&me, 0, sizeof(me));
    me.dwSize = sizeof(me);
    if (!Module32FirstW(snap, &me)) {
        DWORD error = GetLastError();
        fprintf(stderr,
                "protocol=donject.v2 command=modules status=error stage=module-first "
                "pid=%lu win32_error=%lu\n",
                pid, error);
        CloseHandle(snap);
        return result;
    }
    for (;;) {
        if (count == MODULE_ENUM_LIMIT) {
            fprintf(stderr,
                    "protocol=donject.v2 command=modules status=error stage=module-limit "
                    "pid=%lu limit=%u win32_error=%lu\n",
                    pid, MODULE_ENUM_LIMIT, (unsigned long)ERROR_BUFFER_OVERFLOW);
            goto done;
        }
        if (count == capacity) {
            size_t next = capacity ? capacity * 2 : 64;
            ModuleInfo *grown;
            if (next > MODULE_ENUM_LIMIT) next = MODULE_ENUM_LIMIT;
            grown = (ModuleInfo *)realloc(modules, next * sizeof(*modules));
            if (!grown) {
                fprintf(stderr,
                        "protocol=donject.v2 command=modules status=error "
                        "stage=allocation pid=%lu win32_error=%lu\n",
                        pid, (unsigned long)ERROR_OUTOFMEMORY);
                goto done;
            }
            modules = grown;
            capacity = next;
        }
        memset(&modules[count], 0, sizeof(modules[count]));
        modules[count].base = (uintptr_t)me.modBaseAddr;
        modules[count].size = me.modBaseSize;
        wcsncpy(modules[count].name, me.szModule, MAX_MODULE_NAME32);
        modules[count].name[MAX_MODULE_NAME32] = 0;
        wcsncpy(modules[count].path, me.szExePath, MAX_PATH - 1);
        modules[count].path[MAX_PATH - 1] = 0;
        count++;
        if (!Module32NextW(snap, &me)) break;
    }
    if (GetLastError() != ERROR_NO_MORE_FILES) {
        fprintf(stderr,
                "protocol=donject.v2 command=modules status=error stage=module-next "
                "pid=%lu win32_error=%lu\n",
                pid, GetLastError());
        goto done;
    }
    printf("protocol=donject.v2 command=modules status=ok pid=%lu count=%lu\n",
           pid, (unsigned long)count);
    for (i = 0; i < count; i++) {
        printf("protocol=donject.v2 command=modules status=module pid=%lu index=%lu "
               "module_name=\"%ls\" module_path=\"%ls\" module_base=0x%08lX "
               "module_size=0x%08lX\n",
               pid, (unsigned long)i, modules[i].name, modules[i].path,
               (unsigned long)modules[i].base, (unsigned long)modules[i].size);
    }
    result = 0;
done:
    free(modules);
    CloseHandle(snap);
    return result;
}

static int read_exact_at(HANDLE file, LONGLONG offset, void *buf, DWORD size) {
    LARGE_INTEGER where;
    DWORD got = 0;
    where.QuadPart = offset;
    if (!SetFilePointerEx(file, where, NULL, FILE_BEGIN)) return 0;
    return ReadFile(file, buf, size, &got, NULL) && got == size;
}

static int read_pe_identity(HANDLE file, PeIdentity *out) {
    IMAGE_DOS_HEADER dos;
    DWORD signature;
    IMAGE_FILE_HEADER header;
    IMAGE_OPTIONAL_HEADER32 optional32;
    WORD magic;
    LARGE_INTEGER size;
    LONGLONG nt;
    if (!GetFileSizeEx(file, &size) || size.QuadPart < (LONGLONG)sizeof(dos)) return 0;
    if (!read_exact_at(file, 0, &dos, sizeof(dos)) || dos.e_magic != IMAGE_DOS_SIGNATURE)
        return 0;
    nt = (LONGLONG)dos.e_lfanew;
    if (nt < (LONGLONG)sizeof(dos) ||
        nt > size.QuadPart - (LONGLONG)(sizeof(signature) + sizeof(header) + sizeof(magic)))
        return 0;
    if (!read_exact_at(file, nt, &signature, sizeof(signature)) ||
        signature != IMAGE_NT_SIGNATURE)
        return 0;
    if (!read_exact_at(file, nt + (LONGLONG)sizeof(signature), &header, sizeof(header)))
        return 0;
    if (header.SizeOfOptionalHeader < sizeof(magic) ||
        !read_exact_at(file, nt + (LONGLONG)sizeof(signature) + (LONGLONG)sizeof(header),
                       &magic, sizeof(magic)))
        return 0;
    out->machine = header.Machine;
    out->optional_magic = magic;
    out->characteristics = header.Characteristics;
    out->timestamp = header.TimeDateStamp;
    out->image_size = 0;
    if (magic == IMAGE_NT_OPTIONAL_HDR32_MAGIC &&
        header.SizeOfOptionalHeader >= sizeof(optional32) &&
        read_exact_at(file,
                      nt + (LONGLONG)sizeof(signature) + (LONGLONG)sizeof(header),
                      &optional32, sizeof(optional32)))
        out->image_size = optional32.SizeOfImage;
    return 1;
}

static uint32_t rotr32(uint32_t x, unsigned n) {
    return (x >> n) | (x << (32 - n));
}

static void sha256_block(Sha256 *ctx, const unsigned char block[64]) {
    static const uint32_t k[64] = {
        0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u,
        0x3956c25bu, 0x59f111f1u, 0x923f82a4u, 0xab1c5ed5u,
        0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u,
        0x72be5d74u, 0x80deb1feu, 0x9bdc06a7u, 0xc19bf174u,
        0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu,
        0x2de92c6fu, 0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau,
        0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u,
        0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u,
        0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu, 0x53380d13u,
        0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u,
        0xa2bfe8a1u, 0xa81a664bu, 0xc24b8b70u, 0xc76c51a3u,
        0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u,
        0x19a4c116u, 0x1e376c08u, 0x2748774cu, 0x34b0bcb5u,
        0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
        0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u,
        0x90befffau, 0xa4506cebu, 0xbef9a3f7u, 0xc67178f2u
    };
    uint32_t w[64];
    uint32_t a, b, c, d, e, f, g, h;
    unsigned i;
    for (i = 0; i < 16; i++) {
        w[i] = ((uint32_t)block[i * 4] << 24) |
               ((uint32_t)block[i * 4 + 1] << 16) |
               ((uint32_t)block[i * 4 + 2] << 8) |
               (uint32_t)block[i * 4 + 3];
    }
    for (i = 16; i < 64; i++) {
        uint32_t s0 = rotr32(w[i - 15], 7) ^ rotr32(w[i - 15], 18) ^ (w[i - 15] >> 3);
        uint32_t s1 = rotr32(w[i - 2], 17) ^ rotr32(w[i - 2], 19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16] + s0 + w[i - 7] + s1;
    }
    a = ctx->state[0]; b = ctx->state[1]; c = ctx->state[2]; d = ctx->state[3];
    e = ctx->state[4]; f = ctx->state[5]; g = ctx->state[6]; h = ctx->state[7];
    for (i = 0; i < 64; i++) {
        uint32_t s1 = rotr32(e, 6) ^ rotr32(e, 11) ^ rotr32(e, 25);
        uint32_t choose = (e & f) ^ (~e & g);
        uint32_t t1 = h + s1 + choose + k[i] + w[i];
        uint32_t s0 = rotr32(a, 2) ^ rotr32(a, 13) ^ rotr32(a, 22);
        uint32_t majority = (a & b) ^ (a & c) ^ (b & c);
        uint32_t t2 = s0 + majority;
        h = g; g = f; f = e; e = d + t1;
        d = c; c = b; b = a; a = t1 + t2;
    }
    ctx->state[0] += a; ctx->state[1] += b; ctx->state[2] += c; ctx->state[3] += d;
    ctx->state[4] += e; ctx->state[5] += f; ctx->state[6] += g; ctx->state[7] += h;
}

static void sha256_init(Sha256 *ctx) {
    static const uint32_t initial[8] = {
        0x6a09e667u, 0xbb67ae85u, 0x3c6ef372u, 0xa54ff53au,
        0x510e527fu, 0x9b05688cu, 0x1f83d9abu, 0x5be0cd19u
    };
    memcpy(ctx->state, initial, sizeof(initial));
    ctx->bytes = 0;
    ctx->used = 0;
}

static void sha256_update(Sha256 *ctx, const void *input, size_t size) {
    const unsigned char *data = (const unsigned char *)input;
    ctx->bytes += size;
    while (size) {
        size_t take = sizeof(ctx->block) - ctx->used;
        if (take > size) take = size;
        memcpy(ctx->block + ctx->used, data, take);
        ctx->used += take;
        data += take;
        size -= take;
        if (ctx->used == sizeof(ctx->block)) {
            sha256_block(ctx, ctx->block);
            ctx->used = 0;
        }
    }
}

static void sha256_final(Sha256 *ctx, unsigned char digest[32]) {
    uint64_t bits = ctx->bytes * 8;
    unsigned i;
    ctx->block[ctx->used++] = 0x80;
    if (ctx->used > 56) {
        memset(ctx->block + ctx->used, 0, sizeof(ctx->block) - ctx->used);
        sha256_block(ctx, ctx->block);
        ctx->used = 0;
    }
    memset(ctx->block + ctx->used, 0, 56 - ctx->used);
    for (i = 0; i < 8; i++) ctx->block[63 - i] = (unsigned char)(bits >> (i * 8));
    sha256_block(ctx, ctx->block);
    for (i = 0; i < 8; i++) {
        digest[i * 4] = (unsigned char)(ctx->state[i] >> 24);
        digest[i * 4 + 1] = (unsigned char)(ctx->state[i] >> 16);
        digest[i * 4 + 2] = (unsigned char)(ctx->state[i] >> 8);
        digest[i * 4 + 3] = (unsigned char)ctx->state[i];
    }
}

static void digest_hex(const unsigned char digest[32], char hex[65]) {
    static const char digits[] = "0123456789abcdef";
    unsigned i;
    for (i = 0; i < 32; i++) {
        hex[i * 2] = digits[digest[i] >> 4];
        hex[i * 2 + 1] = digits[digest[i] & 15];
    }
    hex[64] = 0;
}

static int sha256_file(HANDLE file, char hex[65]) {
    unsigned char buffer[32768];
    unsigned char digest[32];
    LARGE_INTEGER zero;
    DWORD got;
    Sha256 sha;
    zero.QuadPart = 0;
    if (!SetFilePointerEx(file, zero, NULL, FILE_BEGIN)) return 0;
    sha256_init(&sha);
    for (;;) {
        if (!ReadFile(file, buffer, sizeof(buffer), &got, NULL)) return 0;
        if (!got) break;
        sha256_update(&sha, buffer, got);
    }
    sha256_final(&sha, digest);
    digest_hex(digest, hex);
    return 1;
}

static int file_identity_equal(const BY_HANDLE_FILE_INFORMATION *a,
                               const BY_HANDLE_FILE_INFORMATION *b) {
    return a->dwVolumeSerialNumber == b->dwVolumeSerialNumber &&
           a->nFileIndexHigh == b->nFileIndexHigh &&
           a->nFileIndexLow == b->nFileIndexLow;
}

static void close_opened_image(OpenedImage *image) {
    if (image->file != INVALID_HANDLE_VALUE) CloseHandle(image->file);
    free(image->path);
    memset(image, 0, sizeof(*image));
    image->file = INVALID_HANDLE_VALUE;
}

static int open_canonical_image(const wchar_t *input, DWORD share, OpenedImage *out) {
    DWORD needed;
    wchar_t *absolute = NULL;
    wchar_t *canonical = NULL;
    HANDLE file = INVALID_HANDLE_VALUE;
    int ok = 0;
    memset(out, 0, sizeof(*out));
    out->file = INVALID_HANDLE_VALUE;
    needed = GetFullPathNameW(input, 0, NULL, NULL);
    if (!needed) goto done;
    absolute = (wchar_t *)malloc(((size_t)needed + 1) * sizeof(*absolute));
    if (!absolute) { SetLastError(ERROR_OUTOFMEMORY); goto done; }
    if (!GetFullPathNameW(input, needed + 1, absolute, NULL)) goto done;
    file = CreateFileW(absolute, GENERIC_READ, share, NULL, OPEN_EXISTING,
                       FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) goto done;
    if (!GetFileInformationByHandle(file, &out->identity)) goto done;
    if (out->identity.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) {
        SetLastError(ERROR_DIRECTORY);
        goto done;
    }
    needed = GetFinalPathNameByHandleW(file, NULL, 0,
                                       FILE_NAME_NORMALIZED | VOLUME_NAME_DOS);
    if (!needed) goto done;
    canonical = (wchar_t *)malloc(((size_t)needed + 1) * sizeof(*canonical));
    if (!canonical) { SetLastError(ERROR_OUTOFMEMORY); goto done; }
    if (!GetFinalPathNameByHandleW(file, canonical, needed + 1,
                                    FILE_NAME_NORMALIZED | VOLUME_NAME_DOS))
        goto done;
    if (!read_pe_identity(file, &out->pe)) {
        SetLastError(ERROR_BAD_EXE_FORMAT);
        goto done;
    }
    out->file = file;
    out->path = canonical;
    file = INVALID_HANDLE_VALUE;
    canonical = NULL;
    ok = 1;
done:
    if (file != INVALID_HANDLE_VALUE) CloseHandle(file);
    free(absolute);
    free(canonical);
    return ok;
}

static int inspect_target(HANDLE process, DWORD pid, wchar_t **canonical_path) {
    wchar_t path[32768];
    DWORD count = (DWORD)(sizeof(path) / sizeof(path[0]));
    OpenedImage image;
    const wchar_t *leaf;
    char hash[65];
    if (!QueryFullProcessImageNameW(process, 0, path, &count)) {
        fprintf(stderr, "inject: REFUSED target-image-query pid=%lu err=%lu\n",
                pid, GetLastError());
        return 0;
    }
    if (!open_canonical_image(path, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                              &image)) {
        fprintf(stderr, "inject: REFUSED target-image-open pid=%lu path=%ls err=%lu\n",
                pid, path, GetLastError());
        return 0;
    }
    leaf = path_leaf_w(image.path);
    if (image.pe.machine != IMAGE_FILE_MACHINE_I386 ||
        image.pe.optional_magic != IMAGE_NT_OPTIONAL_HDR32_MAGIC ||
        !image.pe.image_size ||
        (image.pe.characteristics & IMAGE_FILE_DLL)) {
        fprintf(stderr,
                "inject: REFUSED target-architecture pid=%lu path=%ls machine=%04X "
                "optional=%04X characteristics=%04X expected=PE32/i386-exe\n",
                pid, image.path, image.pe.machine, image.pe.optional_magic,
                image.pe.characteristics);
        close_opened_image(&image);
        return 0;
    }
    if (!stricmp_w(leaf, L"riseofnations.exe")) {
        if (!sha256_file(image.file, hash)) {
            fprintf(stderr, "inject: REFUSED retail-hash-read pid=%lu path=%ls err=%lu\n",
                    pid, image.path, GetLastError());
            close_opened_image(&image);
            return 0;
        }
        if (stricmp_a(hash, SUPPORTED_RETAIL_SHA256)) {
            fprintf(stderr,
                    "inject: REFUSED unsupported-retail pid=%lu path=%ls sha256=%s expected=%s\n",
                    pid, image.path, hash, SUPPORTED_RETAIL_SHA256);
            close_opened_image(&image);
            return 0;
        }
        printf("inject: target=retail pid=%lu path=%ls sha256=%s architecture=PE32/i386\n",
               pid, image.path, hash);
    } else if (!stricmp_w(leaf, L"dontest.exe")) {
        printf("inject: target=dontest pid=%lu path=%ls architecture=PE32/i386\n",
               pid, image.path);
    } else {
        fprintf(stderr,
                "inject: REFUSED unsupported-target pid=%lu path=%ls "
                "allowed=riseofnations.exe,dontest.exe\n",
                pid, image.path);
        close_opened_image(&image);
        return 0;
    }
    *canonical_path = image.path;
    image.path = NULL;
    close_opened_image(&image);
    return 1;
}

typedef BOOL (WINAPI *IsWow64Process2Fn)(HANDLE, USHORT *, USHORT *);

static int require_x86_process_pair(HANDLE target, DWORD pid) {
    HMODULE kernel32 = GetModuleHandleW(L"kernel32.dll");
    IsWow64Process2Fn query;
    USHORT self_machine = IMAGE_FILE_MACHINE_UNKNOWN;
    USHORT self_native = IMAGE_FILE_MACHINE_UNKNOWN;
    USHORT target_machine = IMAGE_FILE_MACHINE_UNKNOWN;
    USHORT target_native = IMAGE_FILE_MACHINE_UNKNOWN;
    if (!kernel32) return 0;
    query = (IsWow64Process2Fn)(uintptr_t)GetProcAddress(kernel32, "IsWow64Process2");
    if (!query) {
        fprintf(stderr,
                "inject: REFUSED architecture-api-missing api=IsWow64Process2 err=%lu\n",
                GetLastError());
        return 0;
    }
    if (!query(GetCurrentProcess(), &self_machine, &self_native)) {
        fprintf(stderr, "inject: REFUSED self-architecture-query err=%lu\n", GetLastError());
        return 0;
    }
    if (!query(target, &target_machine, &target_native)) {
        fprintf(stderr, "inject: REFUSED target-architecture-query pid=%lu err=%lu\n",
                pid, GetLastError());
        return 0;
    }
    if (self_machine != IMAGE_FILE_MACHINE_I386 ||
        target_machine != IMAGE_FILE_MACHINE_I386) {
        fprintf(stderr,
                "inject: REFUSED process-architecture pid=%lu self_machine=%04X "
                "self_native=%04X target_machine=%04X target_native=%04X expected=I386\n",
                pid, self_machine, self_native, target_machine, target_native);
        return 0;
    }
    printf("inject: process_architecture=I386 self_native=%04X target_native=%04X\n",
           self_native, target_native);
    return 1;
}

static int loaded_dll_state(DWORD pid, const OpenedImage *dll, ModuleInfo *found) {
    HANDLE snap = module_snapshot(pid);
    MODULEENTRY32W me;
    const wchar_t *leaf = path_leaf_w(dll->path);
    if (snap == INVALID_HANDLE_VALUE) return LOADED_SCAN_ERROR;
    memset(&me, 0, sizeof(me));
    me.dwSize = sizeof(me);
    if (!Module32FirstW(snap, &me)) {
        DWORD error = GetLastError();
        CloseHandle(snap);
        SetLastError(error);
        return LOADED_SCAN_ERROR;
    }
    do {
        HANDLE file;
        BY_HANDLE_FILE_INFORMATION identity;
        if (stricmp_w(me.szModule, leaf)) continue;
        file = CreateFileW(me.szExePath, GENERIC_READ,
                           FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                           NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
        if (file == INVALID_HANDLE_VALUE || !GetFileInformationByHandle(file, &identity)) {
            DWORD error = GetLastError();
            if (file != INVALID_HANDLE_VALUE) CloseHandle(file);
            CloseHandle(snap);
            SetLastError(error);
            return LOADED_BASENAME_COLLISION;
        }
        CloseHandle(file);
        if (found) {
            memset(found, 0, sizeof(*found));
            found->base = (uintptr_t)me.modBaseAddr;
            found->size = me.modBaseSize;
            wcsncpy(found->name, me.szModule, MAX_MODULE_NAME32);
            found->name[MAX_MODULE_NAME32] = 0;
            wcsncpy(found->path, me.szExePath, MAX_PATH - 1);
            found->path[MAX_PATH - 1] = 0;
        }
        if (file_identity_equal(&identity, &dll->identity)) {
            if (me.modBaseSize != dll->pe.image_size) {
                CloseHandle(snap);
                return LOADED_EXACT_SIZE_MISMATCH;
            }
            CloseHandle(snap);
            return LOADED_EXACT;
        }
        CloseHandle(snap);
        return LOADED_BASENAME_COLLISION;
    } while (Module32NextW(snap, &me));
    if (GetLastError() != ERROR_NO_MORE_FILES) {
        DWORD error = GetLastError();
        CloseHandle(snap);
        SetLastError(error);
        return LOADED_SCAN_ERROR;
    }
    CloseHandle(snap);
    return LOADED_ABSENT;
}

static int local_module_owning(const void *address, ModuleInfo *out) {
    HANDLE snap = module_snapshot(GetCurrentProcessId());
    MODULEENTRY32W me;
    uintptr_t needle = (uintptr_t)address;
    if (snap == INVALID_HANDLE_VALUE) return 0;
    memset(&me, 0, sizeof(me));
    me.dwSize = sizeof(me);
    if (!Module32FirstW(snap, &me)) {
        CloseHandle(snap);
        return 0;
    }
    do {
        uintptr_t base = (uintptr_t)me.modBaseAddr;
        uintptr_t end = base + me.modBaseSize;
        if (needle >= base && needle < end && end >= base) {
            memset(out, 0, sizeof(*out));
            out->base = base;
            out->size = me.modBaseSize;
            wcsncpy(out->name, me.szModule, MAX_MODULE_NAME32);
            out->name[MAX_MODULE_NAME32] = 0;
            wcsncpy(out->path, me.szExePath, MAX_PATH - 1);
            out->path[MAX_PATH - 1] = 0;
            CloseHandle(snap);
            return 1;
        }
    } while (Module32NextW(snap, &me));
    if (GetLastError() != ERROR_NO_MORE_FILES) {
        DWORD error = GetLastError();
        CloseHandle(snap);
        SetLastError(error);
        return 0;
    }
    CloseHandle(snap);
    return 0;
}

static int executable_protection(DWORD protection) {
    protection &= 0xffu;
    return protection == PAGE_EXECUTE || protection == PAGE_EXECUTE_READ ||
           protection == PAGE_EXECUTE_READWRITE || protection == PAGE_EXECUTE_WRITECOPY;
}

static int remote_load_library_w(HANDLE process, DWORD pid,
                                 LPTHREAD_START_ROUTINE *remote_proc) {
    HMODULE kernel32 = GetModuleHandleW(L"kernel32.dll");
    FARPROC local_proc;
    ModuleInfo local_owner, remote_owner;
    OpenedImage local_image, remote_image;
    MEMORY_BASIC_INFORMATION memory;
    uintptr_t rva;
    int status;
    int images_open = 0;
    memset(&local_image, 0, sizeof(local_image));
    memset(&remote_image, 0, sizeof(remote_image));
    local_image.file = INVALID_HANDLE_VALUE;
    remote_image.file = INVALID_HANDLE_VALUE;
    if (!kernel32) return 0;
    local_proc = GetProcAddress(kernel32, "LoadLibraryW");
    if (!local_proc || !local_module_owning((const void *)local_proc, &local_owner)) return 0;
    rva = (uintptr_t)local_proc - local_owner.base;
    status = module_by_name_w(pid, local_owner.name, &remote_owner);
    if (status <= 0 || rva >= local_owner.size || rva >= remote_owner.size) {
        if (!status) SetLastError(ERROR_MOD_NOT_FOUND);
        return 0;
    }
    if (!open_canonical_image(local_owner.path,
                              FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                              &local_image) ||
        !open_canonical_image(remote_owner.path,
                              FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                              &remote_image))
        goto fail;
    images_open = 1;
    if (!file_identity_equal(&local_image.identity, &remote_image.identity) ||
        local_image.pe.machine != IMAGE_FILE_MACHINE_I386 ||
        remote_image.pe.machine != IMAGE_FILE_MACHINE_I386 ||
        local_image.pe.timestamp != remote_image.pe.timestamp ||
        local_image.pe.image_size != remote_image.pe.image_size ||
        !local_image.pe.image_size ||
        local_owner.size != local_image.pe.image_size ||
        remote_owner.size != remote_image.pe.image_size) {
        SetLastError(ERROR_EXE_MACHINE_TYPE_MISMATCH);
        goto fail;
    }
    *remote_proc = (LPTHREAD_START_ROUTINE)(remote_owner.base + rva);
    if (!VirtualQueryEx(process, (LPCVOID)(uintptr_t)*remote_proc, &memory, sizeof(memory)) ||
        memory.State != MEM_COMMIT || memory.Type != MEM_IMAGE ||
        !executable_protection(memory.Protect) ||
        (uintptr_t)memory.AllocationBase != remote_owner.base) {
        SetLastError(ERROR_INVALID_ADDRESS);
        goto fail;
    }
    printf("inject: loader=LoadLibraryW owner=%ls local_base=%08lX remote_base=%08lX "
           "rva=%08lX remote_proc=%08lX image_timestamp=%08lX image_size=%lu\n",
           local_owner.name, (unsigned long)local_owner.base,
           (unsigned long)remote_owner.base, (unsigned long)rva,
           (unsigned long)(remote_owner.base + rva),
           (unsigned long)local_image.pe.timestamp,
           (unsigned long)local_image.pe.image_size);
    close_opened_image(&remote_image);
    close_opened_image(&local_image);
    return 1;
fail:
    if (images_open || remote_image.file != INVALID_HANDLE_VALUE || remote_image.path)
        close_opened_image(&remote_image);
    if (images_open || local_image.file != INVALID_HANDLE_VALUE || local_image.path)
        close_opened_image(&local_image);
    return 0;
}

static int inject(DWORD pid, const char *dll_argument, const char *expected_sha_argument) {
    HANDLE process = NULL;
    HANDLE thread = NULL;
    void *remote_path = NULL;
    int remote_thread_finished = 0;
    wchar_t *dll_input = NULL;
    wchar_t *target_path = NULL;
    OpenedImage dll;
    ModuleInfo loaded;
    LPTHREAD_START_ROUTINE loader = NULL;
    SIZE_T path_bytes, wrote = 0;
    DWORD wait_status, exit_code = 0;
    char expected_sha[65], actual_sha[65];
    int loaded_state;
    int result = 1;
    memset(&dll, 0, sizeof(dll));
    dll.file = INVALID_HANDLE_VALUE;
    memset(&loaded, 0, sizeof(loaded));

    if (sizeof(void *) != 4) {
        fprintf(stderr, "inject: REFUSED injector-architecture pointer_size=%u expected=4\n",
                (unsigned)sizeof(void *));
        return 2;
    }
    if (!parse_sha256(expected_sha_argument, expected_sha)) {
        fprintf(stderr,
                "inject: REFUSED expected-dll-sha256 format=required-64-hex\n");
        return 2;
    }
    process = OpenProcess(PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION |
                          PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ,
                          FALSE, pid);
    if (!process) {
        fprintf(stderr, "inject: REFUSED process-open pid=%lu err=%lu\n", pid, GetLastError());
        return 3;
    }
    if (!require_x86_process_pair(process, pid)) {
        result = 4;
        goto done;
    }
    if (!inspect_target(process, pid, &target_path)) {
        result = 5;
        goto done;
    }
    if (!ascii_argument(dll_argument)) {
        fprintf(stderr,
                "inject: REFUSED dll-path-encoding boundary=ASCII argument=<redacted>\n");
        result = 5;
        goto done;
    }
    if (!multibyte_to_wide(dll_argument, &dll_input)) {
        fprintf(stderr, "inject: REFUSED dll-path-conversion argument=%s err=%lu\n",
                dll_argument, GetLastError());
        result = 5;
        goto done;
    }
    /* Excluding FILE_SHARE_WRITE/DELETE holds the exact validated DLL image stable
     * from PE inspection until the remote loader has returned. */
    if (!open_canonical_image(dll_input, FILE_SHARE_READ, &dll)) {
        fprintf(stderr, "inject: REFUSED dll-open argument=%s err=%lu\n",
                dll_argument, GetLastError());
        result = 6;
        goto done;
    }
    if (dll.pe.machine != IMAGE_FILE_MACHINE_I386 ||
        dll.pe.optional_magic != IMAGE_NT_OPTIONAL_HDR32_MAGIC ||
        !dll.pe.image_size ||
        !(dll.pe.characteristics & IMAGE_FILE_DLL)) {
        fprintf(stderr,
                "inject: REFUSED dll-architecture path=%ls machine=%04X optional=%04X "
                "characteristics=%04X expected=PE32/i386-dll\n",
                dll.path, dll.pe.machine, dll.pe.optional_magic, dll.pe.characteristics);
        result = 7;
        goto done;
    }
    if (!sha256_file(dll.file, actual_sha)) {
        fprintf(stderr, "inject: REFUSED dll-hash-read path=%ls err=%lu\n",
                dll.path, GetLastError());
        result = 7;
        goto done;
    }
    if (strcmp(actual_sha, expected_sha)) {
        fprintf(stderr,
                "inject: REFUSED dll-hash-mismatch path=%ls sha256=%s expected=%s\n",
                dll.path, actual_sha, expected_sha);
        result = 7;
        goto done;
    }
    printf("inject: dll=%ls architecture=PE32/i386 sha256=%s\n", dll.path, actual_sha);

    loaded_state = loaded_dll_state(pid, &dll, &loaded);
    if (loaded_state == LOADED_SCAN_ERROR) {
        fprintf(stderr, "inject: REFUSED module-preflight pid=%lu err=%lu\n",
                pid, GetLastError());
        result = 8;
        goto done;
    }
    if (loaded_state == LOADED_EXACT) {
        fprintf(stderr,
               "inject: result=already-loaded status=refused reason=DllMain-not-rerun "
               "pid=%lu module=%ls base=%08lX path=%ls sha256=%s\n",
               pid, loaded.name, (unsigned long)loaded.base, loaded.path, actual_sha);
        result = 22;
        goto done;
    }
    if (loaded_state == LOADED_EXACT_SIZE_MISMATCH) {
        fprintf(stderr,
                "inject: REFUSED loaded-module-size-mismatch pid=%lu module=%ls "
                "base=%08lX enumerated=%lu validated=%lu path=%ls\n",
                pid, loaded.name, (unsigned long)loaded.base,
                (unsigned long)loaded.size, (unsigned long)dll.pe.image_size,
                loaded.path);
        result = 23;
        goto done;
    }
    if (loaded_state == LOADED_BASENAME_COLLISION) {
        fprintf(stderr,
                "inject: REFUSED module-basename-collision requested=%ls loaded=%ls path=%ls\n",
                dll.path, loaded.name[0] ? loaded.name : L"<unreadable>",
                loaded.path[0] ? loaded.path : L"<unreadable>");
        result = 9;
        goto done;
    }
    if (!remote_load_library_w(process, pid, &loader)) {
        fprintf(stderr, "inject: REFUSED loader-resolution pid=%lu err=%lu\n",
                pid, GetLastError());
        result = 10;
        goto done;
    }
    if (wcslen(dll.path) > (((SIZE_T)-1) / sizeof(wchar_t)) - 1) {
        fprintf(stderr, "inject: REFUSED dll-path-too-long\n");
        result = 11;
        goto done;
    }
    path_bytes = (wcslen(dll.path) + 1) * sizeof(wchar_t);
    remote_path = VirtualAllocEx(process, NULL, path_bytes,
                                 MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!remote_path) {
        fprintf(stderr, "inject: FAILED remote-path-allocate bytes=%lu err=%lu\n",
                (unsigned long)path_bytes, GetLastError());
        result = 12;
        goto done;
    }
    if (!WriteProcessMemory(process, remote_path, dll.path, path_bytes, &wrote) ||
        wrote != path_bytes) {
        fprintf(stderr,
                "inject: FAILED remote-path-write address=%08lX expected=%lu wrote=%lu err=%lu\n",
                (unsigned long)(uintptr_t)remote_path, (unsigned long)path_bytes,
                (unsigned long)wrote, GetLastError());
        result = 13;
        goto done;
    }
    thread = CreateRemoteThread(process, NULL, 0, loader, remote_path, 0, NULL);
    if (!thread) {
        fprintf(stderr, "inject: FAILED remote-thread-create loader=%08lX err=%lu\n",
                (unsigned long)(uintptr_t)loader, GetLastError());
        result = 14;
        goto done;
    }
    wait_status = WaitForSingleObject(thread, INJECT_WAIT_MS);
    if (wait_status == WAIT_TIMEOUT) {
        fprintf(stderr,
                "inject: INDETERMINATE remote-thread-timeout wait_ms=%u "
                "remote_path=%08lX allocation=retained thread_handle=closed "
                "target_state=tainted restart_required=1\n",
                INJECT_WAIT_MS, (unsigned long)(uintptr_t)remote_path);
        result = 15;
        goto done;
    }
    if (wait_status != WAIT_OBJECT_0) {
        DWORD wait_error = GetLastError();
        if (GetExitCodeThread(thread, &exit_code) && exit_code != STILL_ACTIVE)
            remote_thread_finished = 1;
        fprintf(stderr,
                "inject: INDETERMINATE remote-thread-wait status=%08lX err=%lu "
                "thread=%s allocation=%s\n",
                (unsigned long)wait_status, wait_error,
                remote_thread_finished ? "exited" : "possibly-active",
                remote_thread_finished ? "released" : "retained");
        result = 16;
        goto done;
    }
    remote_thread_finished = 1;
    if (!GetExitCodeThread(thread, &exit_code) || exit_code == STILL_ACTIVE) {
        fprintf(stderr, "inject: FAILED remote-thread-exit-query err=%lu code=%08lX\n",
                GetLastError(), (unsigned long)exit_code);
        result = 17;
        goto done;
    }
    if (!exit_code) {
        fprintf(stderr, "inject: FAILED LoadLibraryW-returned-null path=%ls\n", dll.path);
        result = 18;
        goto done;
    }
    loaded_state = loaded_dll_state(pid, &dll, &loaded);
    if (loaded_state == LOADED_EXACT_SIZE_MISMATCH) {
        fprintf(stderr,
                "inject: FAILED loaded-module-size-mismatch thread_exit=%08lX "
                "enumerated=%lu validated=%lu path=%ls\n",
                (unsigned long)exit_code, (unsigned long)loaded.size,
                (unsigned long)dll.pe.image_size, dll.path);
        result = 19;
        goto done;
    }
    if (loaded_state != LOADED_EXACT) {
        fprintf(stderr,
                "inject: FAILED loaded-module-confirmation state=%d thread_exit=%08lX path=%ls err=%lu\n",
                loaded_state, (unsigned long)exit_code, dll.path, GetLastError());
        result = 19;
        goto done;
    }
    if ((DWORD)loaded.base != exit_code) {
        fprintf(stderr,
                "inject: FAILED module-base-disagreement thread_exit=%08lX enumerated=%08lX path=%ls\n",
                (unsigned long)exit_code, (unsigned long)loaded.base, dll.path);
        result = 20;
        goto done;
    }
    printf("inject: result=loaded status=ok pid=%lu module=%ls base=%08lX path=%ls sha256=%s\n",
           pid, loaded.name, (unsigned long)loaded.base, loaded.path, actual_sha);
    result = 0;

done:
    if (remote_path) {
        if (!thread || remote_thread_finished) {
            if (!VirtualFreeEx(process, remote_path, 0, MEM_RELEASE)) {
                if (!result) {
                    fprintf(stderr,
                            "inject: INDETERMINATE result=loaded "
                            "cleanup=remote-path-release-failed address=%08lX err=%lu "
                            "target_state=tainted restart_required=1 sha256=%s\n",
                            (unsigned long)(uintptr_t)remote_path, GetLastError(), actual_sha);
                    result = 21;
                } else {
                    fprintf(stderr,
                            "inject: WARNING remote-path-release address=%08lX err=%lu\n",
                            (unsigned long)(uintptr_t)remote_path, GetLastError());
                }
            }
        }
    }
    if (thread) CloseHandle(thread);
    close_opened_image(&dll);
    free(dll_input);
    free(target_path);
    if (process) CloseHandle(process);
    return result;
}

/* Follow a pointer chain in another process, read-only. */
static int chain(DWORD pid, const char *mod, char **offs, int noffs, int reps, int delay) {
    HANDLE process;
    unsigned base = module_base(pid, mod);
    int r, k;
    if (!base) { fprintf(stderr, "module %s not found in %lu\n", mod, pid); return 1; }
    process = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid);
    if (!process) { fprintf(stderr, "OpenProcess failed err=%lu\n", GetLastError()); return 2; }
    printf("base=%08X delta=%08X\n", base, base - 0x400000u);
    for (r = 0; r < reps; r++) {
        unsigned cur = base + (unsigned)strtoul(offs[0], NULL, 16);
        printf("%u:", (unsigned)GetTickCount());
        for (k = 1; k < noffs; k++) {
            unsigned off = (unsigned)strtoul(offs[k], NULL, 16), value = 0;
            SIZE_T got = 0;
            if (!ReadProcessMemory(process, (LPCVOID)(uintptr_t)(cur + off),
                                   &value, 4, &got) || got != 4) {
                printf(" [%08X+%X]=<unreadable>", cur, off);
                cur = 0;
                break;
            }
            printf(" [%08X+%X]=%08X", cur, off, value);
            cur = value;
        }
        printf("\n");
        fflush(stdout);
        if (r + 1 < reps) Sleep((DWORD)delay);
    }
    CloseHandle(process);
    return 0;
}

/* Read-only hex dump at base+rva after `nderef` pointer reads. */
static int peek(DWORD pid, const char *mod, unsigned rva, int nderef,
                unsigned off, unsigned len) {
    HANDLE process = NULL;
    unsigned base = module_base(pid, mod), cur, root;
    unsigned root_before = 0, root_after = 0;
    unsigned char *buf = NULL;
    SIZE_T got = 0;
    unsigned i;
    int stable = -1;
    int result = 0;
    if (!base) { fprintf(stderr, "module %s not found\n", mod); return 1; }
    if (nderef < 0 || nderef > 32 || !len || len > 16u * 1024u * 1024u ||
        base > 0xffffffffu - rva) {
        fprintf(stderr,
                "peek: invalid bounds nderef=%d len=%u base=%08X rva=%X\n",
                nderef, len, base, rva);
        return 2;
    }
    process = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid);
    if (!process) { fprintf(stderr, "OpenProcess failed err=%lu\n", GetLastError()); return 3; }
    root = base + rva;
    cur = root;
    for (i = 0; i < (unsigned)nderef; i++) {
        unsigned value = 0;
        if (!ReadProcessMemory(process, (LPCVOID)(uintptr_t)cur, &value, 4, &got) || got != 4) {
            fprintf(stderr, "deref %u at %08X failed\n", i, cur);
            result = 4;
            goto done;
        }
        if (!i) root_before = value;
        cur = value;
    }
    if (cur > 0xffffffffu - off) {
        fprintf(stderr, "peek: address overflow cur=%08X off=%X\n", cur, off);
        result = 5;
        goto done;
    }
    cur += off;
    buf = (unsigned char *)malloc(len);
    if (!buf) { result = 6; goto done; }
    if (!ReadProcessMemory(process, (LPCVOID)(uintptr_t)cur, buf, len, &got) || got != len) {
        fprintf(stderr, "read %u bytes at %08X failed (got %u)\n",
                len, cur, (unsigned)got);
        result = 7;
        goto done;
    }
    if (nderef > 0 &&
        ReadProcessMemory(process, (LPCVOID)(uintptr_t)root, &root_after, 4, &got) && got == 4)
        stable = root_before == root_after ? 1 : 0;
    printf("# base=%08X addr=%08X len=%X module=%s rva=%X deref=%d nderef=%d "
           "off=%X root=%08X pointer_addr=%08X root_value=%08X stable=%d\n",
           base, cur, len, mod, rva, nderef, nderef, off, root, root,
           root_before, stable);
    for (i = 0; i < len; i += 16) {
        unsigned k;
        printf("%08X:", cur + i);
        for (k = 0; k < 16 && i + k < len; k++) printf(" %02X", buf[i + k]);
        printf("\n");
    }
done:
    free(buf);
    CloseHandle(process);
    return result;
}

static int selftest(void) {
    static const char multi[] =
        "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    unsigned char digest[32];
    char hex[65];
    Sha256 sha;

    sha256_init(&sha);
    sha256_final(&sha, digest);
    digest_hex(digest, hex);
    if (strcmp(hex, "e3b0c44298fc1c149afbf4c8996fb924"
                    "27ae41e4649b934ca495991b7852b855")) {
        fprintf(stderr, "selftest: FAILED vector=empty actual=%s\n", hex);
        return 1;
    }
    sha256_init(&sha);
    sha256_update(&sha, "abc", 3);
    sha256_final(&sha, digest);
    digest_hex(digest, hex);
    if (strcmp(hex, "ba7816bf8f01cfea414140de5dae2223"
                    "b00361a396177a9cb410ff61f20015ad")) {
        fprintf(stderr, "selftest: FAILED vector=abc actual=%s\n", hex);
        return 1;
    }
    sha256_init(&sha);
    sha256_update(&sha, multi, sizeof(multi) - 1);
    sha256_final(&sha, digest);
    digest_hex(digest, hex);
    if (strcmp(hex, "248d6a61d20638b8e5c026930c3e6039"
                    "a33ce45964ff2167f6ecedd419db06c1")) {
        fprintf(stderr, "selftest: FAILED vector=multi-block actual=%s\n", hex);
        return 1;
    }
    if (sizeof(void *) != 4) {
        fprintf(stderr, "selftest: FAILED pointer_size=%u expected=4\n",
                (unsigned)sizeof(void *));
        return 1;
    }
    if (!ascii_argument("C:\\Users\\Public\\donhook.dll") ||
        ascii_argument("C:\\Users\\Public\\donhook-\x80.dll")) {
        fprintf(stderr, "selftest: FAILED path_boundary=ASCII\n");
        return 1;
    }
    printf("selftest: status=ok architecture=PE32/i386 "
           "sha256_vectors=empty,abc,multi-block path_boundary=ASCII\n");
    return 0;
}

static void usage(void) {
    fprintf(stderr,
            "usage:\n"
            "  donject base <pid> <module.exe>\n"
            "  donject modules <pid>\n"
            "  donject inject <pid> <dll path> <expected-sha256>\n"
            "  donject threads <pid>\n"
            "  donject chain <pid> <module.exe> <rva> [offset ...]\n"
            "  donject watch <pid> <module.exe> <rva> [offset ...] <reps> <delay-ms>\n"
            "  donject peek <pid> <module.exe> <rva> <derefs> <offset> <length>\n"
            "  donject selftest\n");
}

int main(int argc, char **argv) {
    DWORD pid_value;
    if (argc == 2 && !strcmp(argv[1], "selftest")) return selftest();
    if (argc >= 2 && !strcmp(argv[1], "base")) {
        if (argc != 4 || !parse_pid(argc >= 3 ? argv[2] : NULL, &pid_value)) {
            fprintf(stderr,
                    "protocol=donject.v2 command=base status=error stage=arguments "
                    "win32_error=%lu\n", (unsigned long)ERROR_INVALID_PARAMETER);
            return 11;
        }
        return base_query(pid_value, argv[3]);
    }
    if (argc >= 2 && !strcmp(argv[1], "modules")) {
        if (argc != 3 || !parse_pid(argc >= 3 ? argv[2] : NULL, &pid_value)) {
            fprintf(stderr,
                    "protocol=donject.v2 command=modules status=error stage=arguments "
                    "win32_error=%lu\n", (unsigned long)ERROR_INVALID_PARAMETER);
            return 12;
        }
        return modules_query(pid_value);
    }
    if (argc < 3 || !parse_pid(argv[2], &pid_value)) {
        usage();
        return 1;
    }
    if (!strcmp(argv[1], "inject")) {
        if (argc != 5) {
            fprintf(stderr,
                    "inject: REFUSED arguments expected=inject-pid-dll-path-sha256\n");
            return 2;
        }
        return inject(pid_value, argv[3], argv[4]);
    }
    if (argc >= 8 && !strcmp(argv[1], "peek"))
        return peek(pid_value, argv[3], (unsigned)strtoul(argv[4], NULL, 16), atoi(argv[5]),
                    (unsigned)strtoul(argv[6], NULL, 16),
                    (unsigned)strtoul(argv[7], NULL, 16));
    if (argc >= 5 && !strcmp(argv[1], "chain"))
        return chain(pid_value, argv[3], argv + 4, argc - 4, 1, 0);
    if (argc >= 7 && !strcmp(argv[1], "watch")) {
        int reps = atoi(argv[argc - 2]);
        int delay = atoi(argv[argc - 1]);
        if (reps <= 0 || delay < 0) {
            fprintf(stderr, "watch: invalid reps/delay\n");
            return 1;
        }
        return chain(pid_value, argv[3], argv + 4, argc - 6, reps, delay);
    }
    if (argc == 3 && !strcmp(argv[1], "threads")) {
        HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        THREADENTRY32 te;
        int count = 0;
        if (snap == INVALID_HANDLE_VALUE) {
            fprintf(stderr, "thread snapshot failed err=%lu\n", GetLastError());
            return 2;
        }
        memset(&te, 0, sizeof(te));
        te.dwSize = sizeof(te);
        if (!Thread32First(snap, &te)) {
            DWORD error = GetLastError();
            CloseHandle(snap);
            fprintf(stderr, "thread enumeration failed stage=first err=%lu\n", error);
            return 2;
        }
        do {
            if (te.th32OwnerProcessID == pid_value) {
                printf("tid %lu\n", te.th32ThreadID);
                count++;
            }
        } while (Thread32Next(snap, &te));
        if (GetLastError() != ERROR_NO_MORE_FILES) {
            DWORD error = GetLastError();
            CloseHandle(snap);
            fprintf(stderr, "thread enumeration failed stage=next err=%lu\n", error);
            return 2;
        }
        CloseHandle(snap);
        printf("%d threads\n", count);
        return count ? 0 : 1;
    }
    usage();
    return 1;
}
