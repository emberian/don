#ifndef DON_RETAIL_CONTROL_LIFECYCLE_CORE_H
#define DON_RETAIL_CONTROL_LIFECYCLE_CORE_H

#include <stddef.h>

/*
 * Portable, side-effect-free lifecycle rules shared by the injected DLL and
 * its native host test.  The DLL supplies the Interlocked operations around
 * these state values; keeping the transition and detach predicates here makes
 * the fail-closed policy directly executable off-Windows.
 */
enum rc_lifecycle_state {
    RC_LIFECYCLE_LOADING = 0,
    RC_LIFECYCLE_PREFLIGHT,
    RC_LIFECYCLE_ARMING,
    RC_LIFECYCLE_ARMED,
    RC_LIFECYCLE_CANCELING,
    RC_LIFECYCLE_UNHOOKING,
    RC_LIFECYCLE_PARKED,
    RC_LIFECYCLE_REARMING,
    RC_LIFECYCLE_DETACHING,
    RC_LIFECYCLE_DETACH_READY,
    RC_LIFECYCLE_QUARANTINED
};

enum rc_dispatch_gate {
    RC_DISPATCH_FENCED = -1,
    RC_DISPATCH_OPEN = 0,
    RC_DISPATCH_ACTIVE = 1
};

static inline const char *rc_lifecycle_name(int state) {
    switch (state) {
        case RC_LIFECYCLE_LOADING: return "loading";
        case RC_LIFECYCLE_PREFLIGHT: return "preflight";
        case RC_LIFECYCLE_ARMING: return "arming";
        case RC_LIFECYCLE_ARMED: return "armed";
        case RC_LIFECYCLE_CANCELING: return "canceling";
        case RC_LIFECYCLE_UNHOOKING: return "unhooking";
        case RC_LIFECYCLE_PARKED: return "parked";
        case RC_LIFECYCLE_REARMING: return "rearming";
        case RC_LIFECYCLE_DETACHING: return "detaching";
        case RC_LIFECYCLE_DETACH_READY: return "detach-ready";
        case RC_LIFECYCLE_QUARANTINED: return "quarantined";
        default: return "invalid";
    }
}

static inline int rc_lifecycle_transition_allowed(int from, int to) {
    if (to == RC_LIFECYCLE_QUARANTINED)
        return from != RC_LIFECYCLE_QUARANTINED;
    switch (from) {
        case RC_LIFECYCLE_LOADING:
            return to == RC_LIFECYCLE_PREFLIGHT;
        case RC_LIFECYCLE_PREFLIGHT:
            return to == RC_LIFECYCLE_ARMING;
        case RC_LIFECYCLE_ARMING:
            return to == RC_LIFECYCLE_ARMED;
        case RC_LIFECYCLE_ARMED:
            return to == RC_LIFECYCLE_CANCELING;
        case RC_LIFECYCLE_CANCELING:
            return to == RC_LIFECYCLE_UNHOOKING;
        case RC_LIFECYCLE_UNHOOKING:
            return to == RC_LIFECYCLE_PARKED;
        case RC_LIFECYCLE_PARKED:
            return to == RC_LIFECYCLE_REARMING ||
                   to == RC_LIFECYCLE_DETACHING;
        case RC_LIFECYCLE_REARMING:
            return to == RC_LIFECYCLE_ARMING;
        case RC_LIFECYCLE_DETACHING:
            return to == RC_LIFECYCLE_DETACH_READY;
        default:
            return 0;
    }
}

/* Pure gate transitions used by the deterministic race interleaving test. */
static inline int rc_gate_try_enter_dispatch(int *gate) {
    if (*gate != RC_DISPATCH_OPEN) return 0;
    *gate = RC_DISPATCH_ACTIVE;
    return 1;
}

static inline int rc_gate_leave_dispatch(int *gate) {
    if (*gate != RC_DISPATCH_ACTIVE) return 0;
    *gate = RC_DISPATCH_OPEN;
    return 1;
}

/* Returns 1 when fenced, 0 while an earlier dispatch owns the gate. */
static inline int rc_gate_try_fence(int *gate) {
    if (*gate == RC_DISPATCH_FENCED) return 1;
    if (*gate != RC_DISPATCH_OPEN) return 0;
    *gate = RC_DISPATCH_FENCED;
    return 1;
}

static inline int rc_gate_open_fresh(int *gate) {
    if (*gate != RC_DISPATCH_FENCED) return 0;
    *gate = RC_DISPATCH_OPEN;
    return 1;
}

static inline int rc_request_epoch_is_current(unsigned request_attempt,
                                              unsigned request_epoch,
                                              unsigned active_attempt,
                                              unsigned active_epoch) {
    return request_attempt != 0 && request_epoch != 0 &&
           request_attempt == active_attempt &&
           request_epoch == active_epoch;
}

static inline int rc_dispatch_context_allows(int fence_requested, int stopping,
                                             int lifecycle,
                                             int active_attempt,
                                             int active_epoch) {
    return !fence_requested && !stopping &&
           lifecycle == RC_LIFECYCLE_ARMED &&
           active_attempt > 0 && active_epoch > 0;
}

typedef struct rc_detach_snapshot {
    int lifecycle;
    int dispatch_gate;
    int fence_requested;
    int stopping;
    int hook_installed;
    int hook_inflight;
    int pending;
    int verify_active;
    int trace_active;
    int hook_bytes_original;
    int request_files_absent;
    int events_drained;
    int stop_file_present;
} rc_detach_snapshot;

static inline int rc_detach_constraints_hold(const rc_detach_snapshot *snapshot) {
    return snapshot && snapshot->lifecycle == RC_LIFECYCLE_PARKED &&
           snapshot->dispatch_gate == RC_DISPATCH_FENCED &&
           snapshot->fence_requested && snapshot->stopping &&
           !snapshot->hook_installed &&
           !snapshot->hook_inflight && !snapshot->pending &&
           !snapshot->verify_active && !snapshot->trace_active &&
           snapshot->hook_bytes_original && snapshot->request_files_absent &&
           snapshot->events_drained && snapshot->stop_file_present;
}

typedef int (*rc_write_chunk_fn)(void *context, const char *bytes,
                                 size_t length, size_t *written);

/* Exact write loop: short writes are allowed, zero/over-reported writes fail. */
static inline int rc_write_all(rc_write_chunk_fn write_chunk, void *context,
                               const char *bytes, size_t length) {
    size_t offset = 0;
    if (!write_chunk || (!bytes && length)) return 0;
    while (offset < length) {
        size_t written = 0;
        if (!write_chunk(context, bytes + offset, length - offset, &written) ||
            written == 0 || written > length - offset)
            return 0;
        offset += written;
    }
    return 1;
}

static inline int rc_ready_commit_allowed(int complete_write, int flushed,
                                          int closed, int replaced) {
    return !!complete_write && !!flushed && !!closed && !!replaced;
}

#endif
