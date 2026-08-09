/*
 * Read-only channel-12 capture adapter for the supported 32-bit retail image.
 *
 * This header deliberately does not install or remove a hook.  The controller
 * owns the five-byte call-site patch and its lifecycle.  Patch check_all's sole
 * World::walk_data call at 0x00936a03 to wwc_callsite_bridge, after proving the
 * original bytes are E8 E8 F2 D7 FF.  The bridge first performs retail's real
 * checksum walk.  Only when that result equals the armed peer value does it
 * immediately repeat the same World::walk_data(-1) traversal with an append-only
 * visitor on the same main thread.
 *
 * No retail buffer is written, no retail DataWalk vtable is replaced, and no
 * file I/O or allocation occurs on the retail main thread.  A worker may copy
 * the frozen buffer only after wwc_snapshot reports FROZEN and inflight == 0.
 */

#ifndef DON_WORLD_WALK_CAPTURE_ADAPTER_H
#define DON_WORLD_WALK_CAPTURE_ADAPTER_H

#include <windows.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

#if !defined(_WIN32) || !defined(__i386__)
#error "world_walk_capture_adapter requires 32-bit Windows"
#endif

#if defined(__GNUC__)
#define WWC_STDCALL __attribute__((stdcall))
#define WWC_THISCALL __attribute__((thiscall))
#else
#define WWC_STDCALL __stdcall
#define WWC_THISCALL __thiscall
#endif

#define WWC_PREFERRED_BASE       0x00400000u
#define WWC_WORLD_WALK_VA        0x006b5cf0u
#define WWC_WORLD_WALK_RVA       (WWC_WORLD_WALK_VA - WWC_PREFERRED_BASE)
#define WWC_WORLD_WALK_BYTES     903u
#define WWC_WORLD_CALLSITE_VA    0x00936a03u
#define WWC_WORLD_CALLSITE_RVA   (WWC_WORLD_CALLSITE_VA - WWC_PREFERRED_BASE)
#define WWC_CHECKSUM_VTABLE_VA   0x00b3f920u
#define WWC_MAX_IMAGE_BYTES      (16u * 1024u * 1024u)

/* SHA-256 of riseofnations.exe[World::walk_data, +903).  The host sealer
 * authenticates this whole range; the in-process adapter only compares the
 * exact callsite and a bounded prologue/trailer, keeping SHA and file I/O off
 * the retail main thread. */
#define WWC_WORLD_WALK_SHA256 \
    "adf50b4197020da3932b562442b1cf7d8e80443da42f6181ebd23276bd080e01"
#define WWC_RETAIL_EXE_SHA256 \
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"

enum wwc_phase {
    WWC_IDLE = 0,
    WWC_ARMED = 1,
    WWC_CAPTURING = 2,
    WWC_FROZEN = 3,
    WWC_DISCARDED = 4
};

enum wwc_fault {
    WWC_FAULT_NONE = 0,
    WWC_FAULT_BAD_ARGUMENT = 1,
    WWC_FAULT_UNSUPPORTED_IMAGE = 2,
    WWC_FAULT_REENTRANT = 3,
    WWC_FAULT_BAD_RANGE = 4,
    WWC_FAULT_OVERFLOW = 5,
    WWC_FAULT_VISITOR_DRIFT = 6,
    WWC_FAULT_CHECKSUM_DRIFT = 7
};

typedef struct wwc_data_walk wwc_data_walk;

typedef void (WWC_THISCALL *wwc_walk_fn)(
    wwc_data_walk *self, const unsigned char *begin, const unsigned char *end);
typedef void (WWC_THISCALL *wwc_tag_fn)(wwc_data_walk *self, const void *tag);

typedef struct wwc_vtable {
    wwc_walk_fn walk;
    wwc_tag_fn walk_tag;
} wwc_vtable;

/* Exact six-dword DataWalk / CheckSum prefix recovered at 0x0093657e. */
struct wwc_data_walk {
    const wwc_vtable *vtable; /* +0x00 */
    uint32_t reading;         /* +0x04: zero for checksum/save direction */
    uint32_t checksum_mode;   /* +0x08: one in CheckSum */
    int32_t section_mask;     /* +0x0c: -1 for all sections */
    uint32_t checksum;        /* +0x10: adler-32, initialized to one */
    uint32_t byte_count;      /* +0x14 */
};

typedef void (WWC_STDCALL *wwc_world_walk_fn)(wwc_data_walk *walk, int section);

typedef struct wwc_capture_state {
    wwc_data_walk visitor;
    const wwc_vtable *visitor_vtable;
    wwc_world_walk_fn original_world_walk;
    unsigned char *image;
    uint32_t image_capacity;
    volatile LONG phase;
    volatile LONG inflight;
    volatile LONG fault;
    uint32_t target_checksum;
    uint32_t original_checksum;
    uint32_t original_byte_count;
    uint32_t captured_checksum;
    uint32_t captured_bytes;
    uint32_t walk_calls;
    uint32_t tag_calls;
    uint32_t capture_thread_id;
    uint32_t capture_sequence;
} wwc_capture_state;

typedef struct wwc_capture_snapshot {
    uint32_t phase;
    uint32_t inflight;
    uint32_t fault;
    uint32_t target_checksum;
    uint32_t original_checksum;
    uint32_t original_byte_count;
    uint32_t captured_checksum;
    uint32_t captured_bytes;
    uint32_t walk_calls;
    uint32_t tag_calls;
    uint32_t capture_thread_id;
    uint32_t capture_sequence;
} wwc_capture_snapshot;

_Static_assert(sizeof(void *) == 4, "capture ABI requires PE32 pointers");
_Static_assert(sizeof(wwc_data_walk) == 0x18, "DataWalk prefix drift");
_Static_assert(offsetof(wwc_data_walk, reading) == 0x04, "DataWalk direction drift");
_Static_assert(offsetof(wwc_data_walk, checksum) == 0x10, "CheckSum value drift");
_Static_assert(offsetof(wwc_data_walk, byte_count) == 0x14, "CheckSum count drift");

static wwc_capture_state *wwc_active_state;

static uint32_t wwc_adler32(uint32_t adler, const unsigned char *buf, uint32_t len) {
    const uint32_t base = 65521u;
    uint32_t s1 = adler & 0xffffu;
    uint32_t s2 = adler >> 16;
    while (len != 0) {
        uint32_t block = len > 5552u ? 5552u : len;
        len -= block;
        while (block-- != 0) {
            s1 += *buf++;
            s2 += s1;
        }
        s1 %= base;
        s2 %= base;
    }
    return (s2 << 16) | s1;
}

static void WWC_THISCALL wwc_append(
    wwc_data_walk *visitor, const unsigned char *begin, const unsigned char *end) {
    wwc_capture_state *state = wwc_active_state;
    uintptr_t first = (uintptr_t)begin;
    uintptr_t last = (uintptr_t)end;
    uint32_t len;
    if (state == NULL || visitor != &state->visitor || first > last ||
        last - first > UINT32_MAX) {
        if (state != NULL)
            InterlockedCompareExchange(&state->fault, WWC_FAULT_BAD_RANGE, WWC_FAULT_NONE);
        return;
    }
    len = (uint32_t)(last - first);
    state->walk_calls++;
    if (len > state->image_capacity - state->captured_bytes) {
        InterlockedCompareExchange(&state->fault, WWC_FAULT_OVERFLOW, WWC_FAULT_NONE);
        return;
    }
    if (len != 0)
        memcpy(state->image + state->captured_bytes, begin, len);
    state->captured_checksum = wwc_adler32(state->captured_checksum, begin, len);
    state->captured_bytes += len;
    visitor->checksum = state->captured_checksum;
    visitor->byte_count = state->captured_bytes;
}

static void WWC_THISCALL wwc_tag(wwc_data_walk *visitor, const void *tag) {
    wwc_capture_state *state = wwc_active_state;
    (void)tag;
    if (state == NULL || visitor != &state->visitor) {
        if (state != NULL)
            InterlockedCompareExchange(&state->fault, WWC_FAULT_VISITOR_DRIFT,
                                       WWC_FAULT_NONE);
        return;
    }
    state->tag_calls++;
}

static const wwc_vtable wwc_append_vtable = {wwc_append, wwc_tag};

/* Called off the retail main thread before the controller installs its callsite
 * patch.  The caller retains ownership of buffer for the whole armed lifetime. */
static int wwc_initialize(wwc_capture_state *state, uintptr_t image_base,
                          unsigned char *buffer, uint32_t capacity) {
    static const unsigned char walk_prefix[16] = {
        0x55, 0x8b, 0xec, 0x6a, 0xff, 0x68, 0x03, 0xd0,
        0xa8, 0x00, 0x64, 0xa1, 0x00, 0x00, 0x00, 0x00
    };
    static const unsigned char walk_suffix[8] = {
        0x00, 0x5b, 0x8b, 0xe5, 0x5d, 0xc2, 0x08, 0x00
    };
    const unsigned char *walk;
    if (state == NULL || buffer == NULL || capacity == 0 ||
        capacity > WWC_MAX_IMAGE_BYTES || image_base == 0)
        return 0;
    memset(state, 0, sizeof(*state));
    walk = (const unsigned char *)(image_base + WWC_WORLD_WALK_RVA);
    if (memcmp(walk, walk_prefix, sizeof(walk_prefix)) != 0 ||
        memcmp(walk + WWC_WORLD_WALK_BYTES - sizeof(walk_suffix), walk_suffix,
               sizeof(walk_suffix)) != 0) {
        state->fault = WWC_FAULT_UNSUPPORTED_IMAGE;
        state->phase = WWC_DISCARDED;
        return 0;
    }
    state->visitor_vtable = &wwc_append_vtable;
    state->original_world_walk = (wwc_world_walk_fn)walk;
    state->image = buffer;
    state->image_capacity = capacity;
    state->phase = WWC_IDLE;
    return 1;
}

/* Publish/remove the initialized state before/after the controller owns the
 * callsite.  Deactivation is legal only after exact callsite restoration and a
 * zero-inflight proof. */
static int wwc_activate(wwc_capture_state *state) {
    if (state == NULL || state->original_world_walk == NULL ||
        InterlockedCompareExchange(&state->inflight, 0, 0) != 0 ||
        wwc_active_state != NULL)
        return 0;
    InterlockedExchangePointer((PVOID volatile *)&wwc_active_state, state);
    return wwc_active_state == state;
}

static int wwc_deactivate(wwc_capture_state *state) {
    if (state == NULL || wwc_active_state != state ||
        InterlockedCompareExchange(&state->inflight, 0, 0) != 0)
        return 0;
    return InterlockedCompareExchangePointer(
               (PVOID volatile *)&wwc_active_state, NULL, state) == state;
}

/* Arms one content target.  A checksum is a selector here, never evidence of
 * byte agreement: the host sealer still requires the frozen image and distinct
 * same-group peer packets from the replay. */
static int wwc_arm(wwc_capture_state *state, uint32_t target_checksum,
                   uint32_t capture_sequence) {
    if (state == NULL || target_checksum == 0 ||
        InterlockedCompareExchange(&state->inflight, 0, 0) != 0 ||
        InterlockedCompareExchange(&state->phase, WWC_IDLE, WWC_IDLE) != WWC_IDLE)
        return 0;
    state->target_checksum = target_checksum;
    state->capture_sequence = capture_sequence;
    state->fault = WWC_FAULT_NONE;
    state->original_checksum = 0;
    state->original_byte_count = 0;
    state->captured_checksum = 1;
    state->captured_bytes = 0;
    state->walk_calls = 0;
    state->tag_calls = 0;
    state->capture_thread_id = 0;
    InterlockedExchange(&state->phase, WWC_ARMED);
    return 1;
}

/* Same stdcall ABI as the original callee (ret 8).  The controller patches the
 * single call at 0x00936a03 to this bridge and sets wwc_active_state before
 * exposing the hook. */
static void WWC_STDCALL wwc_callsite_bridge(wwc_data_walk *retail, int section) {
    wwc_capture_state *state = wwc_active_state;
    LONG armed = state != NULL
        ? InterlockedCompareExchange(&state->phase, WWC_ARMED, WWC_ARMED)
        : WWC_IDLE;
    if (state == NULL || state->original_world_walk == NULL) return;

    state->original_world_walk(retail, section);
    if (armed != WWC_ARMED || section != -1 || retail == NULL ||
        retail->reading != 0 || retail->checksum != state->target_checksum)
        return;
    if (InterlockedCompareExchange(&state->inflight, 1, 0) != 0) {
        InterlockedCompareExchange(&state->fault, WWC_FAULT_REENTRANT, WWC_FAULT_NONE);
        return;
    }
    if (InterlockedCompareExchange(&state->phase, WWC_CAPTURING, WWC_ARMED) != WWC_ARMED) {
        InterlockedExchange(&state->inflight, 0);
        return;
    }

    state->original_checksum = retail->checksum;
    state->original_byte_count = retail->byte_count;
    state->capture_thread_id = GetCurrentThreadId();
    state->captured_checksum = 1;
    state->captured_bytes = 0;
    state->walk_calls = 0;
    state->tag_calls = 0;
    state->visitor.vtable = state->visitor_vtable;
    state->visitor.reading = 0;
    state->visitor.checksum_mode = 1;
    state->visitor.section_mask = -1;
    state->visitor.checksum = 1;
    state->visitor.byte_count = 0;

    state->original_world_walk(&state->visitor, -1);
    if (state->fault == WWC_FAULT_NONE &&
        state->visitor.checksum == state->captured_checksum &&
        state->visitor.byte_count == state->captured_bytes &&
        state->captured_checksum == state->original_checksum &&
        state->captured_bytes == state->original_byte_count) {
        InterlockedExchange(&state->phase, WWC_FROZEN);
    } else {
        if (state->fault == WWC_FAULT_NONE)
            state->fault = WWC_FAULT_CHECKSUM_DRIFT;
        InterlockedExchange(&state->phase, WWC_DISCARDED);
    }
    InterlockedExchange(&state->inflight, 0);
}

static wwc_capture_snapshot wwc_snapshot(const wwc_capture_state *state) {
    wwc_capture_snapshot out;
    memset(&out, 0, sizeof(out));
    if (state == NULL) return out;
    out.phase = (uint32_t)InterlockedCompareExchange((volatile LONG *)&state->phase, 0, 0);
    out.inflight = (uint32_t)InterlockedCompareExchange((volatile LONG *)&state->inflight, 0, 0);
    out.fault = (uint32_t)InterlockedCompareExchange((volatile LONG *)&state->fault, 0, 0);
    out.target_checksum = state->target_checksum;
    out.original_checksum = state->original_checksum;
    out.original_byte_count = state->original_byte_count;
    out.captured_checksum = state->captured_checksum;
    out.captured_bytes = state->captured_bytes;
    out.walk_calls = state->walk_calls;
    out.tag_calls = state->tag_calls;
    out.capture_thread_id = state->capture_thread_id;
    out.capture_sequence = state->capture_sequence;
    return out;
}

#endif /* DON_WORLD_WALK_CAPTURE_ADAPTER_H */
