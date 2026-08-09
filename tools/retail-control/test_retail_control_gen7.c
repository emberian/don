/*
 * Offline native lifecycle harness:
 *   cc -std=c11 -Wall -Wextra -Werror -pedantic \
 *      test_retail_control_gen7.c -o /tmp/test_retail_control_gen7
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "retail_control_lifecycle_core.h"

#define CHECK(expr) do { \
    if (!(expr)) { \
        fprintf(stderr, "FAIL line %d: %s\n", __LINE__, #expr); \
        return 1; \
    } \
} while (0)

typedef struct fake_writer {
    char output[64];
    size_t used;
    size_t chunk;
    int fail_call;
    int zero_call;
    int over_report_call;
    int calls;
} fake_writer;

static int fake_write(void *opaque, const char *bytes, size_t length,
                      size_t *written) {
    fake_writer *writer = (fake_writer *)opaque;
    size_t amount = length;
    writer->calls++;
    if (writer->calls == writer->fail_call) return 0;
    if (writer->calls == writer->zero_call) {
        *written = 0;
        return 1;
    }
    if (writer->calls == writer->over_report_call) {
        *written = length + 1;
        return 1;
    }
    if (writer->chunk && amount > writer->chunk) amount = writer->chunk;
    if (writer->used + amount > sizeof(writer->output)) return 0;
    memcpy(writer->output + writer->used, bytes, amount);
    writer->used += amount;
    *written = amount;
    return 1;
}

static int test_stop_before_claim(void) {
    int gate = RC_DISPATCH_OPEN;
    CHECK(rc_gate_try_fence(&gate));
    CHECK(gate == RC_DISPATCH_FENCED);
    CHECK(!rc_gate_try_enter_dispatch(&gate));
    return 0;
}

static int test_claim_before_stop(void) {
    int gate = RC_DISPATCH_OPEN;
    int fence_requested = 0;
    CHECK(rc_gate_try_enter_dispatch(&gate));
    fence_requested = 1;
    CHECK(!rc_gate_try_fence(&gate));
    CHECK(rc_gate_leave_dispatch(&gate));
    /* A new callback may win OPEN before the worker, but the published fence
       request makes that callback release without dispatching. */
    CHECK(rc_gate_try_enter_dispatch(&gate));
    CHECK(!rc_dispatch_context_allows(fence_requested, 0,
                                      RC_LIFECYCLE_ARMED, 7, 11));
    CHECK(rc_gate_leave_dispatch(&gate));
    CHECK(rc_gate_try_fence(&gate));
    CHECK(!rc_gate_try_enter_dispatch(&gate));
    return 0;
}

static int test_epoch_rollover(void) {
    int gate = RC_DISPATCH_FENCED;
    unsigned old_attempt = 7, old_epoch = 11;
    unsigned new_attempt = 8, new_epoch = 13;
    CHECK(rc_request_epoch_is_current(7, 11, old_attempt, old_epoch));
    CHECK(rc_gate_open_fresh(&gate));
    CHECK(!rc_request_epoch_is_current(7, 11, new_attempt, new_epoch));
    CHECK(rc_request_epoch_is_current(8, 13, new_attempt, new_epoch));
    CHECK(rc_dispatch_context_allows(0, 0, RC_LIFECYCLE_ARMED,
                                     new_attempt, new_epoch));
    CHECK(!rc_dispatch_context_allows(0, 1, RC_LIFECYCLE_ARMED,
                                      new_attempt, new_epoch));
    return 0;
}

static int test_lifecycle_and_quarantine(void) {
    static const int path[] = {
        RC_LIFECYCLE_LOADING, RC_LIFECYCLE_PREFLIGHT,
        RC_LIFECYCLE_ARMING, RC_LIFECYCLE_ARMED,
        RC_LIFECYCLE_CANCELING, RC_LIFECYCLE_UNHOOKING,
        RC_LIFECYCLE_PARKED, RC_LIFECYCLE_REARMING,
        RC_LIFECYCLE_ARMING, RC_LIFECYCLE_ARMED,
        RC_LIFECYCLE_CANCELING, RC_LIFECYCLE_UNHOOKING,
        RC_LIFECYCLE_PARKED, RC_LIFECYCLE_DETACHING,
        RC_LIFECYCLE_DETACH_READY
    };
    size_t i;
    for (i = 1; i < sizeof(path) / sizeof(path[0]); i++)
        CHECK(rc_lifecycle_transition_allowed(path[i - 1], path[i]));
    CHECK(!rc_lifecycle_transition_allowed(RC_LIFECYCLE_ARMED,
                                           RC_LIFECYCLE_DETACH_READY));
    CHECK(rc_lifecycle_transition_allowed(RC_LIFECYCLE_ARMED,
                                          RC_LIFECYCLE_QUARANTINED));
    CHECK(!rc_lifecycle_transition_allowed(RC_LIFECYCLE_QUARANTINED,
                                           RC_LIFECYCLE_ARMING));
    CHECK(!strcmp(rc_lifecycle_name(RC_LIFECYCLE_DETACH_READY),
                  "detach-ready"));
    return 0;
}

static int test_detach_constraints(void) {
    rc_detach_snapshot valid = {
        RC_LIFECYCLE_PARKED, RC_DISPATCH_FENCED, 1, 1, 0, 0, 0,
        0, 0, 1, 1, 1, 1
    };
    rc_detach_snapshot changed;
    int *fields;
    size_t i;
    CHECK(rc_detach_constraints_hold(&valid));
    changed = valid; changed.lifecycle = RC_LIFECYCLE_ARMED;
    CHECK(!rc_detach_constraints_hold(&changed));
    changed = valid; changed.dispatch_gate = RC_DISPATCH_ACTIVE;
    CHECK(!rc_detach_constraints_hold(&changed));
    fields = &changed.fence_requested;
    for (i = 0; i < 11; i++) {
        changed = valid;
        fields = &changed.fence_requested;
        fields[i] = !fields[i];
        CHECK(!rc_detach_constraints_hold(&changed));
    }
    return 0;
}

static int test_checked_short_writes(void) {
    const char payload[] = "atomic-ready-record";
    fake_writer writer;
    memset(&writer, 0, sizeof(writer));
    writer.chunk = 3;
    CHECK(rc_write_all(fake_write, &writer, payload, sizeof(payload) - 1));
    CHECK(writer.used == sizeof(payload) - 1);
    CHECK(!memcmp(writer.output, payload, sizeof(payload) - 1));

    memset(&writer, 0, sizeof(writer)); writer.fail_call = 1;
    CHECK(!rc_write_all(fake_write, &writer, payload, sizeof(payload) - 1));
    memset(&writer, 0, sizeof(writer)); writer.zero_call = 1;
    CHECK(!rc_write_all(fake_write, &writer, payload, sizeof(payload) - 1));
    memset(&writer, 0, sizeof(writer)); writer.over_report_call = 1;
    CHECK(!rc_write_all(fake_write, &writer, payload, sizeof(payload) - 1));
    CHECK(rc_ready_commit_allowed(1, 1, 1, 1));
    CHECK(!rc_ready_commit_allowed(0, 1, 1, 1));
    CHECK(!rc_ready_commit_allowed(1, 0, 1, 1));
    CHECK(!rc_ready_commit_allowed(1, 1, 0, 1));
    CHECK(!rc_ready_commit_allowed(1, 1, 1, 0));
    return 0;
}

int main(void) {
    CHECK(!test_stop_before_claim());
    CHECK(!test_claim_before_stop());
    CHECK(!test_epoch_rollover());
    CHECK(!test_lifecycle_and_quarantine());
    CHECK(!test_detach_constraints());
    CHECK(!test_checked_short_writes());
    puts("retail-control-gen7: ok");
    return 0;
}
