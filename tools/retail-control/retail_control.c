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
#define RVA_WORLD_PTR      (0x00c061d0u - PREFERRED_BASE)
#define RVA_LEADERS_PTR    (0x00c061e0u - PREFERRED_BASE)
#define RVA_BUILDTYPES_PTR (0x00c061f4u - PREFERRED_BASE)
#define RVA_COORD_LOOKUP   (0x00cae5fcu - PREFERRED_BASE)
#define RVA_CIRCLE_X       (0x00cb7e90u - PREFERRED_BASE)
#define RVA_CIRCLE_Y       (0x00cbb0e0u - PREFERRED_BASE)
#define RVA_CIRCLE_RADIUS  (0x00cbe330u - PREFERRED_BASE)
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
#define RVA_GROUP_ISSUE_BUILD (0x00708c60u - PREFERRED_BASE)
#define RVA_GROUP_ISSUE_QUEUE (0x00708c90u - PREFERRED_BASE)
#define RVA_GROUP_ISSUE_GATHER (0x0070aa20u - PREFERRED_BASE)
#define RVA_GROUP_VALIDATE_BUILD (0x00708620u - PREFERRED_BASE)
#define RVA_BUILD_CAN_QUEUE (0x0062da00u - PREFERRED_BASE)
#define RVA_BUILD_SNAP_CENTER (0x00636190u - PREFERRED_BASE)
#define RVA_BUILD_TILE_CORNER (0x00636440u - PREFERRED_BASE)
#define RVA_BUILD_GATHER_RADIUS (0x0063bc60u - PREFERRED_BASE)
#define RVA_BUILD_MAX_GATHERERS (0x0063c430u - PREFERRED_BASE)
#define RVA_WORLD_IS_REALLY_SEEN (0x006b42c0u - PREFERRED_BASE)
#define RVA_VECTOR_DIST_COORDS (0x0046d060u - PREFERRED_BASE)
#define RVA_MOVE_ORDER_VTABLE (0x00b4a12cu - PREFERRED_BASE)
#define RVA_GATHER_ORDER_VTABLE (0x00b49c1cu - PREFERRED_BASE)
#define RVA_BUILD_ORDER_VTABLE (0x00b4834cu - PREFERRED_BASE)
#define RVA_BUILD_VTABLE (0x00b42174u - PREFERRED_BASE)

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
#define OFF_UNIT_ANGLE 0x50u
#define OFF_UNIT_DEST_ANGLE 0x58u
#define OFF_UNIT_ORDERS_X 0x70u
#define OFF_UNIT_ORDERS_Y 0x74u
#define OBJECT_COORD_XOR 0x00063637u
#define MOVE_ORDER_VBASE_OFFSET 84u

#define PACKAGE_LENGTH 0x10u
#define PACKAGE_BYTES  0x12u
#define PACKAGE_CAP    0x201u
#define GROUP_NUM      0x0cu
#define GROUP_WHO      0x4au
#define GROUP_LIST     0x8ccu
#define GROUP_SIZE     0x9d0u

#define MAX_IDS 128
#define MAX_GUYS 32
#define MAX_PUBLIC_OBJECTS 256
#define MAX_BUILD_QUEUE 16
#define MAX_QUEUED_TYPES 128
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
    V_ATTACK,
    V_TRACE_MOVE,
    V_OBSERVE_GUYS,
    V_OBSERVE_PLAYER,
    V_GATHER,
    V_QUEUE_UP,
    V_BUILD,
    V_VALIDATE_QUEUE,
    V_VALIDATE_BUILD,
    V_RUN_FRAMES,
    V_FIND_BUILD,
    V_FIND_GATHER_BUILD
};

typedef struct {
    unsigned pointer;
    int type;
    int x;
    int y;
    int z;
    unsigned angle;
    int des_x;
    int des_y;
    unsigned des_angle;
    int last_x;
    int last_y;
    short off_x;
    short off_y;
    unsigned guy_num;
} guy_sample_t;

typedef struct {
    int elapsed;
    int type;
} queue_sample_t;

typedef struct {
    int type;
    unsigned count;
} queued_type_sample_t;

typedef struct {
    int id;
    unsigned pointer;
    unsigned category;          /* 1 unit, 2 building, 3 wall */
    unsigned flags;
    unsigned uid;
    int type;
    int type_valid;
    int x;
    int y;
    int z;
    int hits;
    unsigned class_vtable;
    unsigned angle;
    int order_length;
    unsigned order_vtable;
    unsigned order_flags;
    unsigned order_metric;
    int order_target_id;
    unsigned order_target_uid;
    int order_target_valid;
    int queued_build_target_id;
    unsigned queued_build_target_uid;
    int queued_build_target_valid;
    int queued_build_order_seen;
    int guy_length;
    int queue_logical;
    int queue_size;
    int queue_count;
    int queue_truncated;
    int gather_max;
    queue_sample_t queue[MAX_BUILD_QUEUE];
} public_object_t;

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
    unsigned phase;             /* observed/queued/applied/timeout/rejected/trace-* */
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
    unsigned order_flags;
    unsigned order_metric;
    int unit_x;
    int unit_y;
    unsigned unit_x_stored;
    unsigned unit_y_stored;
    unsigned unit_type_pointer;
    int unit_type;
    unsigned unit_masks;
    unsigned unit_form;
    unsigned unit_guy_mark;
    int unit_type_guy_spacing;
    unsigned unit_angle;
    unsigned unit_dest_angle;
    int unit_orders_x;
    int unit_orders_y;
    int move_valid;
    int move_x;
    int move_y;
    int move_angle;
    int move_dest;
    int move_tolerance;
    int move_pause;
    int move_retry;
    int move_attempts;
    int move_timer;
    int move_facing;
    int move_dest_x;
    int move_dest_y;
    int move_last_x;
    int move_last_y;
    int move_coll_x;
    int move_coll_y;
    int move_orig_x;
    int move_orig_y;
    short move_off_x;
    short move_off_y;
    int guy_length;
    int guy_capacity;
    int guy_count;
    int guy_truncated;
    guy_sample_t guys[MAX_GUYS];
    int local_player;
    int world_tile_xs;
    int world_tile_ys;
    int player_pop;
    int player_pop_cap;
    unsigned player_leader_flags;
    unsigned player_identity_flags;
    int player_tribe;
    int player_team;
    int player_resources[6];
    int player_resource_caps[6];
    int player_over_cap[6];
    int player_age;
    int player_epochs[4];
    unsigned char player_tech_bits[101];
    int player_queued_type_count;
    int player_queued_type_truncated;
    queued_type_sample_t player_queued_types[MAX_QUEUED_TYPES];
    int player_slots;
    int player_unit_mark;
    int player_build_mark;
    int player_wall_mark;
    int player_object_count;
    int player_object_truncated;
    public_object_t player_objects[MAX_PUBLIC_OBJECTS];
    int validation_result;
    int placement_x;
    int placement_y;
    int placement_tested;
    int placement_legal;
    int placement_seen;
    int placement_capacity;
    int placement_snap_x;
    int placement_snap_y;
    int placement_ring;
    unsigned note;
} event_t;

typedef struct {
    int active;
    request_t req;
    unsigned started;
    int initial_order_length;
    unsigned initial_order_vtable;
} verify_t;

typedef struct {
    int active;
    int finishing;
    int bounded;
    int saw_order;
    request_t req;
    unsigned start_frame;
    unsigned last_frame;
} trace_t;

static unsigned g_base;
static HANDLE g_self_process;
static char g_root[MAX_PATH];
static char g_request_path[MAX_PATH];
static char g_events_path[MAX_PATH];
static char g_ready_path[MAX_PATH];
static char g_stop_path[MAX_PATH];
static char g_log_path[MAX_PATH];
static volatile LONG g_pending;
static volatile LONG g_stopping;
static request_t g_request;
static verify_t g_verify;
static trace_t g_trace;

static event_t g_events[EVENT_CAP];
static volatile LONG g_event_head;
static volatile LONG g_event_tail;

static BYTE *g_hook_addr;
static BYTE g_hook_orig[5];
static BYTE *g_trampoline;
static int g_hook_installed;

static int join_path(char out[MAX_PATH], const char *root, const char *leaf) {
    int n = _snprintf(out, MAX_PATH, "%s\\%s", root, leaf);
    if (n < 0 || n >= MAX_PATH) { out[0] = 0; return 0; }
    return 1;
}

/*
 * Every mapped generation derives its control directory from its own DLL path.
 * This deliberately avoids a shared STOP/request namespace and permits a new,
 * uniquely named DLL to attach while an older generation remains parked.
 */
static int init_paths(HINSTANCE module) {
    DWORD n = GetModuleFileNameA(module, g_root, MAX_PATH);
    char *slash;
    if (!n || n >= MAX_PATH) return 0;
    slash = strrchr(g_root, '\\');
    if (!slash || slash == g_root) return 0;
    *slash = 0;
    return join_path(g_request_path, g_root, "request.txt") &&
           join_path(g_events_path, g_root, "events.ndjson") &&
           join_path(g_ready_path, g_root, "ready.txt") &&
           join_path(g_stop_path, g_root, "STOP") &&
           join_path(g_log_path, g_root, "retail-control.log");
}

static void log_line(const char *line) {
    HANDLE h;
    DWORD n;
    h = CreateFileA(g_log_path, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
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
    int length = -1, capacity = -1;
    unsigned char owner = 0;
    short object_index = -1;
    if (is_unit) *is_unit = 0;
    if (who >= 10 || id < 0 || id > 32767) return 0;
    if (!rd32(g_base + RVA_OBJECTS_PTR, &objects) || !objects) return 0;
    if (!safe_read(objects + OFF_OBJECTS_LISTS + who * OBJECTS_ARRAY_STRIDE + 4u,
                   &length, 4) ||
        !safe_read(objects + OFF_OBJECTS_LISTS + who * OBJECTS_ARRAY_STRIDE + 8u,
                   &capacity, 4) || length < 0 || capacity < length || id >= length ||
        !rd32(objects + OFF_OBJECTS_LISTS + who * OBJECTS_ARRAY_STRIDE + OFF_ARRAY_LIST,
              &list) || !list) return 0;
    if (!rd32(list + (unsigned)id * 4u, &object) || !object) return 0;
    {
        unsigned char flags = 0;
        if (!safe_read(object + OFF_OBJ_FLAGS, &flags, 1) || !(flags & 1) ||
            !safe_read(object + 9u, &owner, 1) || owner != (unsigned char)who ||
            !rd16(object + 0x0au, &object_index) || object_index != (short)id) return 0;
    }
    if (rd32(objects + OFF_UNIT_MARK + who * 4u, &mark) && (unsigned)id < mark) {
        if (is_unit) *is_unit = 1;
    }
    return object;
}

static void observe_public_order(unsigned unit, unsigned who, public_object_t *out) {
    int length = -1;
    unsigned head = 0, node = 0, order = 0, vtable = 0;
    unsigned char b = 0;
    out->order_length = -1;
    if (!safe_read(unit + OFF_UNIT_ORDER_LENGTH, &length, 4)) return;
    out->order_length = length;
    if (length <= 0 || !rd32(unit + 0xdcu, &head) || !head ||
        !rd32(head + 4u, &node) || !node || !rd32(node + 8u, &order) || !order)
        return;
    if (rd32(order, &vtable)) out->order_vtable = vtable;
    if (safe_read(order + 4u, &b, 1)) out->order_flags = b;
    if (safe_read(node + 0xcu, &b, 1)) out->order_metric = b;
    if (vtable == g_base + RVA_GATHER_ORDER_VTABLE ||
        vtable == g_base + RVA_BUILD_ORDER_VTABLE) {
        /* GatherOrder and BuildOrder expose their UnitOrder virtual bases at
           complete+0x2c and complete+0x18 respectively.  Their common
           TargetOrder whom/o/uid identity is public only after resolving it
           through this same local owner's active object table. */
        unsigned delta = vtable == g_base + RVA_GATHER_ORDER_VTABLE ? 44u : 24u;
        unsigned complete = order - delta, target = 0;
        int target_id = -1, target_who = -1, target_is_unit = 0;
        unsigned short target_uid = 0, live_uid = 0;
        if (safe_read(complete + 0x08u, &target_id, 4) &&
            safe_read(complete + 0x0cu, &target_who, 4) &&
            safe_read(complete + 0x10u, &target_uid, 2) && target_id >= 0 &&
            target_who == (int)who &&
            (target = object_ptr(who, target_id, &target_is_unit)) != 0 &&
            !target_is_unit && safe_read(target + 0x30u, &live_uid, 2) &&
            live_uid == target_uid) {
            out->order_target_id = target_id;
            out->order_target_uid = target_uid;
            out->order_target_valid = 1;
        }
    }
    /* A distant BUILD_AT is represented as a front MoveOrder followed by a
       BuildOrder. Walk the exact bounded circular queue so the public proof can
       attribute that pending BuildOrder without treating the front move as a
       failed worker transition. */
    {
        unsigned cursor = node;
        int q;
        for (q = 0; q < length && q < 256; q++) {
            unsigned queued_order = 0, queued_vtable = 0, complete, target = 0;
            unsigned next_cursor = 0;
            int target_id = -1, target_who = -1, target_is_unit = 0;
            unsigned short target_uid = 0, live_uid = 0;
            if (!rd32(cursor + 8u, &queued_order) || !queued_order ||
                !rd32(queued_order, &queued_vtable))
                break;
            if (queued_vtable == g_base + RVA_BUILD_ORDER_VTABLE) {
                out->queued_build_order_seen = 1;
                complete = queued_order - 24u;
                if (safe_read(complete + 0x08u, &target_id, 4) &&
                    safe_read(complete + 0x0cu, &target_who, 4) &&
                    safe_read(complete + 0x10u, &target_uid, 2) && target_id >= 0 &&
                    target_who == (int)who &&
                    (target = object_ptr(who, target_id, &target_is_unit)) != 0 &&
                    !target_is_unit && safe_read(target + 0x30u, &live_uid, 2) &&
                    live_uid == target_uid) {
                    out->queued_build_target_id = target_id;
                    out->queued_build_target_uid = target_uid;
                    out->queued_build_target_valid = 1;
                }
                break;
            }
            if (!rd32(cursor + 4u, &next_cursor) || !next_cursor || next_cursor == cursor)
                break;
            cursor = next_cursor;
        }
    }
}

static void observe_player_public(event_t *e) {
    unsigned world = 0, leaders = 0, leader = 0, encrypted = 0;
    unsigned objects = 0, array = 0, list = 0, game_after = 0;
    unsigned flags = 0, flags_after = 0, encrypted_after = 0;
    unsigned frame_after = 0, objects_after = 0, list_after = 0;
    unsigned short player_flags = 0;
    unsigned char player_who = 0, player_tribe = 0;
    signed char player_team = 0;
    int leader_who = -1, leader_tribe = -1;
    int who = -1, human_count = 0, length = -1, capacity = -1, unit_mark = -1;
    int length_after = -1, capacity_after = -1, unit_after = -1;
    int build_mark = -1, wall_mark = -1, build_after = -1, wall_after = -1;
    int band_start[3], band_end[3], band_category[3];
    int band, i, r;
    e->local_player = -1;
    if (!rd32(g_base + RVA_LEADERS_PTR, &leaders) || !leaders) {
        e->note = 10;
        return;
    }
    /* Retail's exact local-console discriminator is Leader::is_human (flags bit 2).
       Require one and only one in-game, playing human; never fall back to slot zero. */
    for (i = 0; i < 8; i++) {
        unsigned candidate = leaders + (unsigned)i * 0x6eecu;
        unsigned candidate_flags = 0;
        if (!rd32(candidate, &candidate_flags)) {
            e->note = 10;
            return;
        }
        if ((candidate_flags & 7u) == 7u) {
            who = i;
            human_count++;
        }
    }
    if (human_count != 1) {
        e->note = 10;
        return;
    }
    e->local_player = who;
    leader = leaders + (unsigned)who * 0x6eecu;
    if (!rd32(leader, &flags) || !safe_read(leader + 8u, &leader_who, 4) ||
        !safe_read(leader + 0x0cu, &leader_tribe, 4) || leader_who != who ||
        !e->game) {
        e->note = 13;
        return;
    }
    {
        /* Game::info is inline at +0x0c; GameInfo::player starts at +0x38. */
        unsigned player = e->game + 0x44u + (unsigned)who * 0x8cu;
        if (!safe_read(player + 0x30u, &player_flags, 2) ||
            !safe_read(player + 0x32u, &player_tribe, 1) ||
            !safe_read(player + 0x33u, &player_who, 1) ||
            !safe_read(player + 0x34u, &player_team, 1) ||
            player_who != (unsigned char)who || (int)player_tribe != leader_tribe) {
            e->note = 13;
            return;
        }
    }
    e->player_leader_flags = flags;
    e->player_identity_flags = player_flags;
    e->player_tribe = leader_tribe;
    e->player_team = player_team;
    if (rd32(g_base + RVA_WORLD_PTR, &world) && world) {
        safe_read(world + 0x18u, &e->world_tile_xs, 4);
        safe_read(world + 0x1cu, &e->world_tile_ys, 4);
    }
    if (!safe_read(leader + 0x940u, &e->player_pop, 4) ||
        !safe_read(leader + 0x7e4u, &e->player_pop_cap, 4) ||
        !rd32(leader + 0x6eb8u, &encrypted) || !encrypted) {
        e->note = 14;
        return;
    }
    for (r = 0; r < 6; r++) {
        unsigned v = 0;
        if (!rd32(encrypted + (unsigned)r * 4u, &v)) {
            e->note = 14;
            return;
        }
        e->player_resources[r] = (int)(v ^ 0x00008221u);
        if (!rd32(encrypted + 0x30u + (unsigned)r * 4u, &v)) {
            e->note = 14;
            return;
        }
        e->player_resource_caps[r] = (int)(v ^ 0x00001281u);
        if (!rd32(encrypted + 0x4cu + (unsigned)r * 4u, &v)) {
            e->note = 14;
            return;
        }
        e->player_over_cap[r] = (int)(v ^ 0x00008932u);
    }
    {
        unsigned v = 0;
        if (!rd32(encrypted + 0xdcu, &v)) {
            e->note = 14;
            return;
        }
        e->player_age = (int)(v ^ 0x00062766u);
        for (r = 0; r < 4; r++) {
            if (!rd32(encrypted + 0xe8u + (unsigned)r * 4u, &v)) {
                e->note = 14;
                return;
            }
            e->player_epochs[r] = (int)(v ^ 0x00063187u);
        }
        if (!safe_read(leader + 0x6c18u, e->player_tech_bits,
                       sizeof(e->player_tech_bits))) {
            e->note = 14;
            return;
        }
        for (r = 0; r < 806; r++) {
            unsigned short queued = 0;
            if (!safe_read(leader + 0x5a22u + (unsigned)r * 2u, &queued, 2)) {
                e->note = 14;
                return;
            }
            if (!queued) continue;
            if (e->player_queued_type_count >= MAX_QUEUED_TYPES) {
                e->player_queued_type_truncated = 1;
                continue;
            }
            e->player_queued_types[e->player_queued_type_count].type = r;
            e->player_queued_types[e->player_queued_type_count].count = queued;
            e->player_queued_type_count++;
        }
    }
    if (!rd32(g_base + RVA_OBJECTS_PTR, &objects) || !objects) {
        e->note = 11;
        return;
    }
    array = objects + OFF_OBJECTS_LISTS + (unsigned)who * OBJECTS_ARRAY_STRIDE;
    safe_read(array + 4u, &length, 4);
    safe_read(array + 8u, &capacity, 4);
    rd32(array + OFF_ARRAY_LIST, &list);
    safe_read(objects + 0x15cu + (unsigned)who * 4u, &unit_mark, 4);
    safe_read(objects + 0x184u + (unsigned)who * 4u, &build_mark, 4);
    safe_read(objects + 0x1acu + (unsigned)who * 4u, &wall_mark, 4);
    e->player_slots = length;
    e->player_unit_mark = unit_mark;
    e->player_build_mark = build_mark;
    e->player_wall_mark = wall_mark;
    if (length < 0 || length > 32768 || capacity < length || capacity > 32768 ||
        (length && !list) || unit_mark < 0 || unit_mark > length ||
        build_mark < 2000 || build_mark > length || wall_mark < 3000 ||
        wall_mark > length) {
        e->note = 12;
        return;
    }
    band_start[0] = 0; band_end[0] = unit_mark; band_category[0] = 1;
    band_start[1] = 2000; band_end[1] = build_mark; band_category[1] = 2;
    band_start[2] = 3000; band_end[2] = wall_mark; band_category[2] = 3;
    for (band = 0; band < 3; band++) {
        for (i = band_start[band]; i < band_end[band]; i++) {
            public_object_t *out;
            unsigned p = 0, ptype = 0, raw = 0, vtable = 0;
            unsigned char object_flags = 0, owner = 0;
            unsigned short uid = 0;
            short object_index = -1;
            if (!rd32(list + (unsigned)i * 4u, &p) || !p ||
                !safe_read(p + OFF_OBJ_FLAGS, &object_flags, 1) || !(object_flags & 1u) ||
                !safe_read(p + 9u, &owner, 1) || owner != (unsigned char)who ||
                !rd16(p + 0x0au, &object_index) || object_index != (short)i)
                continue;
            if (e->player_object_count >= MAX_PUBLIC_OBJECTS) {
                e->player_object_truncated = 1;
                continue;
            }
            out = &e->player_objects[e->player_object_count++];
            out->id = i;
            out->pointer = p;
            out->category = (unsigned)band_category[band];
            out->flags = object_flags;
            safe_read(p + 0x30u, &uid, 2);
            out->uid = uid;
            if (rd32(p, &vtable)) out->class_vtable = vtable;
            if (rd32(p + 0x18u, &ptype) && ptype &&
                safe_read(ptype + 4u, &out->type, 4)) out->type_valid = 1;
            if (rd32(p + 0x0cu, &raw)) out->z = (int)(raw ^ OBJECT_COORD_XOR);
            if (rd32(p + 0x10u, &raw)) out->x = (int)(raw ^ OBJECT_COORD_XOR);
            if (rd32(p + 0x14u, &raw)) out->y = (int)(raw ^ OBJECT_COORD_XOR);
            safe_read(p + 0x20u, &out->hits, 4);
            if (out->category == 1u) {
                rd32(p + OFF_UNIT_ANGLE, &out->angle);
                safe_read(p + 0xe8u, &out->guy_length, 4);
                observe_public_order(p, (unsigned)who, out);
            } else if (out->category == 2u) {
                unsigned char logical = 0;
                signed char gather_max = 0;
                unsigned queue_list = 0;
                int q, queue_size = -1;
                if (!safe_read(p + 0x80u, &gather_max, 1) ||
                    !safe_read(p + 0x82u, &logical, 1) ||
                    !safe_read(p + 0x88u, &queue_size, 4) || queue_size < logical ||
                    queue_size > 256 || (logical && !rd32(p + 0x8cu, &queue_list))) {
                    e->note = 16;
                    e->player_object_count = 0;
                    return;
                }
                out->gather_max = (int)gather_max;
                out->queue_logical = logical;
                out->queue_size = queue_size;
                out->queue_count = logical < MAX_BUILD_QUEUE ? logical : MAX_BUILD_QUEUE;
                out->queue_truncated = logical > MAX_BUILD_QUEUE;
                for (q = 0; q < out->queue_count; q++) {
                    short type = -1;
                    if (!safe_read(queue_list + (unsigned)q * 0x14u,
                                   &out->queue[q].elapsed, 4) ||
                        !safe_read(queue_list + (unsigned)q * 0x14u + 4u, &type, 2)) {
                        e->note = 16;
                        e->player_object_count = 0;
                        return;
                    }
                    out->queue[q].type = type;
                }
            }
        }
    }
    /* Although this callback is on the main thread, reject any load-transition or
       torn root/metadata sample instead of publishing a best-effort observation. */
    if (!rd32(g_base + RVA_GAME_PTR, &game_after) || game_after != e->game ||
        !rd32(game_after + OFF_GAME_FRAME, &frame_after) || frame_after != e->frame ||
        !rd32(g_base + RVA_OBJECTS_PTR, &objects_after) || objects_after != objects ||
        !safe_read(array + 4u, &length_after, 4) || length_after != length ||
        !safe_read(array + 8u, &capacity_after, 4) || capacity_after != capacity ||
        !rd32(array + OFF_ARRAY_LIST, &list_after) || list_after != list ||
        !safe_read(objects + 0x15cu + (unsigned)who * 4u, &unit_after, 4) ||
        unit_after != unit_mark ||
        !safe_read(objects + 0x184u + (unsigned)who * 4u, &build_after, 4) ||
        build_after != build_mark ||
        !safe_read(objects + 0x1acu + (unsigned)who * 4u, &wall_after, 4) ||
        wall_after != wall_mark || !rd32(leader, &flags_after) || flags_after != flags ||
        !rd32(leader + 0x6eb8u, &encrypted_after) || encrypted_after != encrypted) {
        e->note = 15;
        e->player_object_count = 0;
        return;
    }
}

static void observe_unit(event_t *e, const request_t *r) {
    int is_unit = 0;
    unsigned p = 0, head = 0, node = 0, order = 0, vtable = 0, raw = 0;
    int length = -1;
    e->first_object = 0;
    e->order_length = -1;
    e->current_order = 0;
    e->current_order_vtable = 0;
    e->guy_length = -1;
    e->guy_capacity = -1;
    if (!r || r->num_ids <= 0) return;
    p = object_ptr((unsigned)r->arg[0], r->ids[0], &is_unit);
    e->first_object = p;
    if (!p || !is_unit) return;
    if (rd32(p + 0x10u, &raw)) {
        e->unit_x_stored = raw;
        e->unit_x = (int)(raw ^ OBJECT_COORD_XOR);
    }
    if (rd32(p + 0x14u, &raw)) {
        e->unit_y_stored = raw;
        e->unit_y = (int)(raw ^ OBJECT_COORD_XOR);
    }
    if (rd32(p + 0x18u, &e->unit_type_pointer) && e->unit_type_pointer)
        safe_read(e->unit_type_pointer + 4u, &e->unit_type, 4);
    if (e->unit_type_pointer)
        safe_read(e->unit_type_pointer + 0x224u, &e->unit_type_guy_spacing, 4);
    rd32(p + 0x68u, &e->unit_masks);
    {
        unsigned char b = 0;
        if (safe_read(p + 0xaau, &b, 1)) e->unit_form = b;
        if (safe_read(p + 0xb5u, &b, 1)) e->unit_guy_mark = b;
    }
    rd32(p + OFF_UNIT_ANGLE, &e->unit_angle);
    rd32(p + OFF_UNIT_DEST_ANGLE, &e->unit_dest_angle);
    safe_read(p + OFF_UNIT_ORDERS_X, &e->unit_orders_x, 4);
    safe_read(p + OFF_UNIT_ORDERS_Y, &e->unit_orders_y, 4);
    if (safe_read(p + OFF_UNIT_ORDER_LENGTH, &length, 4)) e->order_length = length;
    /* The +0xcc cache is stale after retirement. Resolve the canonical front
       from head->prev->data, as UnitData::get_order does. */
    if (length > 0 && rd32(p + 0xdcu, &head) && head &&
        rd32(head + 4u, &node) && node && rd32(node + 8u, &order) && order) {
        unsigned char b = 0;
        e->current_order = order;
        if (rd32(order, &vtable)) e->current_order_vtable = vtable;
        if (safe_read(order + 4u, &b, 1)) e->order_flags = b;
        if (safe_read(node + 0xcu, &b, 1)) e->order_metric = b;
    }
    if (vtable == g_base + RVA_MOVE_ORDER_VTABLE && order >= MOVE_ORDER_VBASE_OFFSET) {
        unsigned complete = order - MOVE_ORDER_VBASE_OFFSET;
        e->move_valid = 1;
        safe_read(complete + 0x04u, &e->move_x, 4);
        safe_read(complete + 0x08u, &e->move_y, 4);
        safe_read(complete + 0x0cu, &e->move_angle, 4);
        safe_read(complete + 0x10u, &e->move_dest, 4);
        safe_read(complete + 0x14u, &e->move_tolerance, 4);
        safe_read(complete + 0x18u, &e->move_pause, 4);
        safe_read(complete + 0x1cu, &e->move_retry, 4);
        safe_read(complete + 0x20u, &e->move_attempts, 4);
        safe_read(complete + 0x24u, &e->move_timer, 4);
        safe_read(complete + 0x28u, &e->move_facing, 4);
        safe_read(complete + 0x2cu, &e->move_dest_x, 4);
        safe_read(complete + 0x30u, &e->move_dest_y, 4);
        safe_read(complete + 0x34u, &e->move_last_x, 4);
        safe_read(complete + 0x38u, &e->move_last_y, 4);
        safe_read(complete + 0x3cu, &e->move_coll_x, 4);
        safe_read(complete + 0x40u, &e->move_coll_y, 4);
        safe_read(complete + 0x44u, &e->move_orig_x, 4);
        safe_read(complete + 0x48u, &e->move_orig_y, 4);
        safe_read(complete + 0x4cu, &e->move_off_x, 2);
        safe_read(complete + 0x4eu, &e->move_off_y, 2);
    }
    if (r->verb == V_OBSERVE_GUYS) {
        int length = -1, capacity = -1, count, i;
        unsigned list = 0;
        safe_read(p + 0xe8u, &length, 4);
        safe_read(p + 0xecu, &capacity, 4);
        e->guy_length = length;
        e->guy_capacity = capacity;
        if (length < 0 || length > 4096 || capacity < length || capacity > 4096 ||
            (length && !rd32(p + 0xf4u, &list))) {
            e->note = 5; /* malformed PtrArray<Guy>; publish metadata but don't follow it */
            return;
        }
        count = length < MAX_GUYS ? length : MAX_GUYS;
        e->guy_count = count;
        e->guy_truncated = length > MAX_GUYS;
        for (i = 0; i < count; i++) {
            guy_sample_t *g = &e->guys[i];
            unsigned ptr = 0;
            unsigned char num = 0;
            if (!rd32(list + (unsigned)i * 4u, &ptr) || !ptr) continue;
            g->pointer = ptr;
            safe_read(ptr + 0x08u, &g->type, 4);
            safe_read(ptr + 0x0cu, &g->x, 4);
            safe_read(ptr + 0x10u, &g->y, 4);
            safe_read(ptr + 0x14u, &g->z, 4);
            safe_read(ptr + 0x18u, &g->angle, 4);
            safe_read(ptr + 0x5cu, &g->des_x, 4);
            safe_read(ptr + 0x60u, &g->des_y, 4);
            safe_read(ptr + 0x64u, &g->des_angle, 4);
            safe_read(ptr + 0x68u, &g->last_x, 4);
            safe_read(ptr + 0x6cu, &g->last_y, 4);
            safe_read(ptr + 0x92u, &g->off_x, 2);
            safe_read(ptr + 0x94u, &g->off_y, 2);
            if (safe_read(ptr + 0xa2u, &num, 1)) g->guy_num = num;
        }
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
    if (r && r->verb == V_OBSERVE_PLAYER) observe_player_public(e);
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
typedef void (__attribute__((thiscall)) *fn_gather)(void *self, int target, int queued);
typedef void (__attribute__((thiscall)) *fn_queue)(void *self, int type, int count);
typedef void (__attribute__((thiscall)) *fn_build)(void *self, int x1, int y1,
                                                   int x2, int y2, int type, int queued);
typedef int (__attribute__((thiscall)) *fn_validate_build)(const void *self,
                                                           int x1, int y1,
                                                           int x2, int y2,
                                                           int type, int queued);
typedef int (__attribute__((thiscall)) *fn_can_queue)(const void *self, int type);
typedef void (__attribute__((thiscall)) *fn_snap_center)(const void *self,
                                                         int x, int y,
                                                         int *sx, int *sy,
                                                         int who);
typedef void (__attribute__((thiscall)) *fn_tile_corner)(const void *self,
                                                         int x, int y,
                                                         int *tx, int *ty);
typedef int (__attribute__((thiscall)) *fn_max_gatherers)(const void *self,
                                                          int object_id, int who,
                                                          int tx, int ty);
typedef int (__attribute__((thiscall)) *fn_was_seen)(const void *self,
                                                      const int *x, const int *y,
                                                      int who);
typedef int (__attribute__((thiscall)) *fn_int0)(const void *self);
typedef int (__attribute__((fastcall)) *fn_vector_dist_coords)(const int *x1,
                                                               const int *y1,
                                                               const int *x2,
                                                               const int *y2);

static void make_group(unsigned char group[GROUP_SIZE], const request_t *r) {
    int i;
    memset(group, 0, GROUP_SIZE);
    *(int *)(group + GROUP_NUM) = r->num_ids;
    group[GROUP_WHO] = (unsigned char)r->arg[0];
    for (i = 0; i < r->num_ids; i++)
        *(short *)(group + GROUP_LIST + i * 2) = r->ids[i];
}

static unsigned build_type_ptr(int type) {
    unsigned types = 0, list = 0, ptype = 0;
    int length = -1, live_type = -1;
    if (type < 414 || type > 542 ||
        !rd32(g_base + RVA_BUILDTYPES_PTR, &types) || !types ||
        !safe_read(types + 4u, &length, 4) || type >= length ||
        !rd32(types + 0x10u, &list) || !list ||
        !rd32(list + (unsigned)type * 4u, &ptype) || !ptype ||
        !safe_read(ptype + 4u, &live_type, 4) || live_type != type)
        return 0;
    return ptype;
}

static int fcell_is_really_seen(unsigned world, int fx, int fy, int who) {
    return ((fn_was_seen)(g_base + RVA_WORLD_IS_REALLY_SEEN))(
        (const void *)world, &fx, &fy, who) != 0;
}

static int gather_candidate_is_visible(unsigned world, unsigned ptype,
                                       int snap_x, int snap_y, int who) {
    unsigned circle_x = g_base + RVA_CIRCLE_X;
    unsigned circle_y = g_base + RVA_CIRCLE_Y;
    unsigned circle_radius = g_base + RVA_CIRCLE_RADIUS;
    int gather_radius, circle_index, circle_count = 0;
    int world_xs = 0, world_ys = 0, tile_xs = 0, tile_ys = 0;
    int center_wx = 0, center_wy = 0, center_tx = 0, center_ty = 0;
    int i;
    gather_radius = ((fn_int0)(g_base + RVA_BUILD_GATHER_RADIUS))(
        (const void *)ptype);
    circle_index = (gather_radius + 3) / 4;
    if (!world || who < 0 || who >= 8 || gather_radius < 0 ||
        gather_radius > 64 || circle_index < 0 || circle_index > 64 ||
        !safe_read(world + 0x00u, &world_xs, 4) ||
        !safe_read(world + 0x04u, &world_ys, 4) ||
        !safe_read(world + 0x18u, &tile_xs, 4) ||
        !safe_read(world + 0x1cu, &tile_ys, 4) ||
        world_xs <= 0 || world_ys <= 0 ||
        tile_xs != world_xs * 4 || tile_ys != world_ys * 4 ||
        snap_x < 0 || snap_y < 0 || snap_x >= tile_xs * 192 ||
        snap_y >= tile_ys * 192 ||
        !safe_read(g_base + RVA_COORD_LOOKUP + (unsigned)(snap_x >> 8) * 4u,
                   &center_wx, 4) ||
        !safe_read(g_base + RVA_COORD_LOOKUP + (unsigned)(snap_y >> 8) * 4u,
                   &center_wy, 4) ||
        !safe_read(g_base + RVA_COORD_LOOKUP + (unsigned)(snap_x >> 6) * 4u,
                   &center_tx, 4) ||
        !safe_read(g_base + RVA_COORD_LOOKUP + (unsigned)(snap_y >> 6) * 4u,
                   &center_ty, 4) ||
        !safe_read(circle_radius + (unsigned)circle_index * 4u, &circle_count, 4) ||
        circle_count < 1 || circle_count > 12873)
        return -1;

    /* Match calc_gather's exact W-block footprint before calling any oracle that
       consumes it.  Each admitted 4x4-T block is exactly four 2x2-T fog cells. */
    for (i = 0; i < circle_count; i++) {
        signed char ox = 0, oy = 0;
        int wx, wy, block_tx, block_ty, fx, fy;
        if (!safe_read(circle_x + (unsigned)i, &ox, 1) ||
            !safe_read(circle_y + (unsigned)i, &oy, 1))
            return -1;
        wx = center_wx + (int)ox;
        wy = center_wy + (int)oy;
        if (wx < 0 || wy < 0 || wx >= world_xs || wy >= world_ys)
            continue;
        block_tx = wx * 4 + 2;
        block_ty = wy * 4 + 2;
        if (((fn_vector_dist_coords)(g_base + RVA_VECTOR_DIST_COORDS))(
                &center_tx, &center_ty, &block_tx, &block_ty) > gather_radius)
            continue;
        for (fy = wy * 2; fy <= wy * 2 + 1; fy++)
            for (fx = wx * 2; fx <= wx * 2 + 1; fx++)
                if (!fcell_is_really_seen(world, fx, fy, who)) return 0;
    }
    return 1;
}

static int candidate_gather_capacity(unsigned ptype, int snap_x, int snap_y,
                                     int who) {
    int tx = 0, ty = 0;
    ((fn_tile_corner)(g_base + RVA_BUILD_TILE_CORNER))(
        (const void *)ptype, snap_x, snap_y, &tx, &ty);
    return ((fn_max_gatherers)(g_base + RVA_BUILD_MAX_GATHERERS))(
        (const void *)ptype, -1, who, tx, ty);
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
        case V_OBSERVE_GUYS:
        case V_OBSERVE_PLAYER:
            return 1;
        case V_VALIDATE_QUEUE: {
            int is_unit = 0;
            unsigned object = object_ptr((unsigned)r->arg[0], r->ids[0], &is_unit);
            unsigned vtable = 0;
            if (!object || is_unit || r->ids[0] < 2000 || r->ids[0] >= 3000 ||
                !rd32(object, &vtable) || vtable != g_base + RVA_BUILD_VTABLE) {
                e->note = 17;
                e->validation_result = 0;
            } else {
                e->validation_result = ((fn_can_queue)(g_base + RVA_BUILD_CAN_QUEUE))(
                    (const void *)object, r->arg[1]);
            }
            return 1;
        }
        case V_VALIDATE_BUILD:
            make_group(group, r);
            e->validation_result = ((fn_validate_build)(g_base + RVA_GROUP_VALIDATE_BUILD))(
                group, r->arg[1], r->arg[2], r->arg[3], r->arg[4],
                r->arg[5], r->arg[6]);
            return 1;
        case V_FIND_BUILD: {
            fn_validate_build validate =
                (fn_validate_build)(g_base + RVA_GROUP_VALIDATE_BUILD);
            int radius = r->arg[3], ring, dx, dy;
            make_group(group, r);
            e->placement_x = -1;
            e->placement_y = -1;
            /* Enumerate the exact 48-Coord UCoord lattice, nearest ring first.
               Every returned site has been accepted by retail's own validator
               with the canonical simple-pick gesture (x,y,-1,-1). */
            for (ring = 0; ring <= radius && !e->validation_result; ring++) {
                for (dy = -ring; dy <= ring && !e->validation_result; dy++) {
                    for (dx = -ring; dx <= ring; dx++) {
                        long long x, y;
                        int result;
                        if (ring && dx != -ring && dx != ring &&
                            dy != -ring && dy != ring) continue;
                        x = (long long)r->arg[1] + (long long)dx * 48ll;
                        y = (long long)r->arg[2] + (long long)dy * 48ll;
                        if (x < 0 || y < 0 || x > 0x3fffffffll || y > 0x3fffffffll)
                            continue;
                        e->placement_tested++;
                        result = validate(group, (int)x, (int)y, -1, -1,
                                          r->arg[4], 2);
                        if (result) {
                            e->validation_result = result;
                            e->placement_x = (int)x;
                            e->placement_y = (int)y;
                            break;
                        }
                    }
                }
            }
            return 1;
        }
        case V_FIND_GATHER_BUILD: {
            fn_validate_build validate =
                (fn_validate_build)(g_base + RVA_GROUP_VALIDATE_BUILD);
            unsigned world = 0, ptype = build_type_ptr(r->arg[5]);
            int first_ring = r->arg[3], last_ring = r->arg[4], ring, dx, dy;
            make_group(group, r);
            e->placement_x = -1;
            e->placement_y = -1;
            if (!ptype || !rd32(g_base + RVA_WORLD_PTR, &world) || !world) {
                e->note = 18;
                return 1;
            }
            /* This is a read-only placement preview: shipped geometry, local-slot
               current-visibility fog, and shipped prospective max_gatherers.
               Arena tile rings and stable enumeration are preserved. */
            for (ring = first_ring; ring <= last_ring; ring++) {
                for (dy = -ring; dy <= ring; dy++) {
                    for (dx = -ring; dx <= ring; dx++) {
                        long long x, y;
                        int result, capacity, visible, snap_x = 0, snap_y = 0;
                        if (ring && dx != -ring && dx != ring &&
                            dy != -ring && dy != ring) continue;
                        x = (long long)r->arg[1] + (long long)dx * 192ll;
                        y = (long long)r->arg[2] + (long long)dy * 192ll;
                        if (x < 0 || y < 0 || x > 0x3fffffffll || y > 0x3fffffffll)
                            continue;
                        e->placement_tested++;
                        ((fn_snap_center)(g_base + RVA_BUILD_SNAP_CENTER))(
                            (const void *)ptype, (int)x, (int)y,
                            &snap_x, &snap_y, r->arg[0]);
                        /* Visibility precedes even validate_build: its blocked_site
                           path calls calc_gather and could otherwise leak fog state. */
                        visible = gather_candidate_is_visible(
                            world, ptype, snap_x, snap_y, r->arg[0]);
                        if (visible < 0) {
                            e->note = 19;
                            return 1;
                        }
                        if (!visible)
                            continue;
                        e->placement_seen++;
                        result = validate(group, (int)x, (int)y, -1, -1,
                                          r->arg[5], 2);
                        if (!result) continue;
                        e->placement_legal++;
                        capacity = candidate_gather_capacity(
                            ptype, snap_x, snap_y, r->arg[0]);
                        if (capacity <= 0) continue;
                        /* Camp/Mine use Arena's capacity-first site policy. Farm
                           and other ordinary gather buildings use site_near: the
                           first legal currently-visible candidate, without a
                           terrain-capacity ranking that their policy never reads. */
                        if ((r->arg[5] == 418 || r->arg[5] == 419) &&
                            capacity <= e->placement_capacity)
                            continue;
                        e->validation_result = result;
                        e->placement_capacity = capacity;
                        e->placement_x = (int)x;
                        e->placement_y = (int)y;
                        e->placement_snap_x = snap_x;
                        e->placement_snap_y = snap_y;
                        e->placement_ring = ring;
                        if (r->arg[5] != 418 && r->arg[5] != 419) return 1;
                    }
                }
            }
            return 1;
        }
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
        case V_GATHER:
            make_group(group, r);
            ((fn_gather)(g_base + RVA_GROUP_ISSUE_GATHER))(
                group, r->arg[1], r->arg[2]);
            break;
        case V_QUEUE_UP:
            make_group(group, r);
            ((fn_queue)(g_base + RVA_GROUP_ISSUE_QUEUE))(
                group, r->arg[1], r->arg[2]);
            break;
        case V_BUILD:
            make_group(group, r);
            ((fn_build)(g_base + RVA_GROUP_ISSUE_BUILD))(
                group, r->arg[1], r->arg[2], r->arg[3], r->arg[4],
                r->arg[5], r->arg[6]);
            break;
        case V_RUN_FRAMES:
            /* The trace state owns the matching pause-on-boundary command. */
            ((fn_int1)(g_base + RVA_ISSUE_PAUSE))(manager, 0);
            break;
        case V_TRACE_MOVE:
            make_group(group, r);
            ((fn_move)(g_base + RVA_ISSUE_MOVE))(manager, group,
                r->arg[1], r->arg[2], 2, 0, 0, 1, -1, -1, 0);
            /* Never unpause unless retail actually accepted and serialized move. */
            if (package_length() <= e->package_before) return 0;
            ((fn_int1)(g_base + RVA_ISSUE_PAUSE))(manager, 0);
            break;
        default:
            return 0;
    }
    capture_append(e);
    return r->verb == V_OBSERVE || e->package_after > e->package_before;
}

static void trace_tick(void) {
    event_t e, terminal;
    void *manager = (void *)(g_base + RVA_COMMAND_MANAGER);
    if (!g_trace.active) return;
    memset(&e, 0, sizeof(e));
    e.seq = g_trace.req.seq;
    e.verb = g_trace.req.verb;
    snapshot(&e, &g_trace.req);
    if (e.frame == g_trace.last_frame) {
        if (g_trace.finishing && e.paused == 1) {
            e.phase = g_trace.bounded ? 7 : 6;
            push_event(&e);
            g_trace.active = 0;
        }
        return;
    }
    g_trace.last_frame = e.frame;
    if (g_trace.req.verb == V_TRACE_MOVE && e.order_length > 0)
        g_trace.saw_order = 1;
    e.phase = 5; /* trace-sample */

    if (!g_trace.finishing &&
        ((g_trace.req.verb == V_TRACE_MOVE && g_trace.saw_order && e.order_length == 0) ||
         (unsigned)(e.frame - g_trace.start_frame) >=
             (unsigned)(g_trace.req.verb == V_TRACE_MOVE ?
                        g_trace.req.arg[3] : g_trace.req.arg[0]))) {
        g_trace.bounded = g_trace.req.verb == V_TRACE_MOVE &&
                          !(g_trace.saw_order && e.order_length == 0);
        e.package_before = package_length();
        ((fn_int1)(g_base + RVA_ISSUE_PAUSE))(manager, 1);
        capture_append(&e);
        g_trace.finishing = 1;
    }
    push_event(&e);

    if (g_trace.finishing && e.paused == 1) {
        terminal = e;
        terminal.phase = g_trace.bounded ? 7 : 6; /* trace-bounded/trace-complete */
        terminal.command_len = 0;
        terminal.command[0] = 0;
        push_event(&terminal);
        g_trace.active = 0;
    }
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

    trace_tick();

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
    if ((r.verb == V_TRACE_MOVE || r.verb == V_RUN_FRAMES) &&
        (g_trace.active || e.paused != 1 ||
         (r.verb == V_TRACE_MOVE && !e.first_object))) {
        e.phase = 4;
        e.note = g_trace.active ? 3 : (e.paused != 1 ? 2 : 4);
        push_event(&e);
    } else if (dispatch(&r, &e)) {
        e.phase = (r.verb == V_OBSERVE || r.verb == V_OBSERVE_GUYS ||
                   r.verb == V_OBSERVE_PLAYER || r.verb == V_VALIDATE_QUEUE ||
                   r.verb == V_VALIDATE_BUILD || r.verb == V_FIND_BUILD ||
                   r.verb == V_FIND_GATHER_BUILD) ? 0 : 1;
        push_event(&e);
        if (r.verb == V_PAUSE || r.verb == V_SPEED_SET || r.verb == V_MOVE ||
            r.verb == V_HALT || r.verb == V_ATTACK) {
            g_verify.active = 1;
            g_verify.req = r;
            g_verify.started = GetTickCount();
            g_verify.initial_order_length = e.order_length;
            g_verify.initial_order_vtable = e.current_order_vtable;
        } else if (r.verb == V_TRACE_MOVE || r.verb == V_RUN_FRAMES) {
            memset(&g_trace, 0, sizeof(g_trace));
            g_trace.active = 1;
            g_trace.req = r;
            g_trace.start_frame = e.frame;
            g_trace.last_frame = e.frame;
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
 * seq trace-move WHO ID X Y MAX_FRAMES
 * seq run-frames FRAMES
 * seq observe-guys WHO ID
 * seq observe-player
 * seq validate-queue WHO PRODUCER_ID TYPE
 * seq validate-build WHO X1 Y1 X2 Y2 TYPE QUEUED ID...
 * seq find-build WHO ORIGIN_X ORIGIN_Y RADIUS TYPE WORKER_ID
 * seq find-gather-build WHO ORIGIN_X ORIGIN_Y FIRST_RING LAST_RING TYPE WORKER_ID
 * seq gather WHO TARGET_ID QUEUED ID...
 * seq queue WHO TYPE COUNT PRODUCER_ID...
 * seq build WHO X1 Y1 X2 Y2 TYPE QUEUED ID...
 */
static int parse_request(char *line, request_t *r) {
    char *t[160];
    int n = tokenize(line, t, 160), ok = 1, i, first = 0;
    memset(r, 0, sizeof(*r));
    if (n < 2) return 0;
    r->seq = parse_uint(t[0], &ok);
    if (!ok || !r->seq) return 0;
    if (!strcmp(t[1], "observe") && n == 2) r->verb = V_OBSERVE;
    else if (!strcmp(t[1], "observe-player") && n == 2) r->verb = V_OBSERVE_PLAYER;
    else if (!strcmp(t[1], "observe-guys") && n == 4) {
        int id;
        r->verb = V_OBSERVE_GUYS;
        r->arg[0] = parse_int(t[2], &ok);
        id = parse_int(t[3], &ok);
        if (id < 0 || id > 32767) ok = 0;
        r->num_ids = 1;
        r->ids[0] = (short)id;
    }
    else if (!strcmp(t[1], "validate-queue") && n == 5) {
        int id;
        r->verb = V_VALIDATE_QUEUE;
        r->arg[0] = parse_int(t[2], &ok);
        id = parse_int(t[3], &ok);
        r->arg[1] = parse_int(t[4], &ok);
        if (id < 2000 || id >= 3000 || r->arg[1] < 0 || r->arg[1] >= 806) ok = 0;
        r->num_ids = 1;
        r->ids[0] = (short)id;
    }
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
    } else if (!strcmp(t[1], "gather") && n >= 6) {
        r->verb = V_GATHER;
        r->arg[0] = parse_int(t[2], &ok);
        r->arg[1] = parse_int(t[3], &ok);
        r->arg[2] = parse_int(t[4], &ok);
        if (r->arg[1] < 0 || r->arg[1] > 32767 ||
            r->arg[2] < 0 || r->arg[2] > 2) ok = 0;
        first = 5;
    } else if (!strcmp(t[1], "queue") && n >= 6) {
        r->verb = V_QUEUE_UP;
        r->arg[0] = parse_int(t[2], &ok);
        r->arg[1] = parse_int(t[3], &ok);
        r->arg[2] = parse_int(t[4], &ok);
        if (r->arg[1] < 0 || r->arg[1] >= 806 ||
            (r->arg[2] != -1 && r->arg[2] != 1)) ok = 0;
        first = 5;
    } else if ((!strcmp(t[1], "build") || !strcmp(t[1], "validate-build")) && n >= 10) {
        r->verb = !strcmp(t[1], "build") ? V_BUILD : V_VALIDATE_BUILD;
        for (i = 0; i < 7; i++) r->arg[i] = parse_int(t[2 + i], &ok);
        if (r->arg[5] < 414 || r->arg[5] > 542 ||
            r->arg[6] < 0 || r->arg[6] > 2) ok = 0;
        first = 9;
    } else if (!strcmp(t[1], "find-build") && n == 8) {
        int id;
        r->verb = V_FIND_BUILD;
        r->arg[0] = parse_int(t[2], &ok);
        r->arg[1] = parse_int(t[3], &ok);
        r->arg[2] = parse_int(t[4], &ok);
        r->arg[3] = parse_int(t[5], &ok);
        r->arg[4] = parse_int(t[6], &ok);
        id = parse_int(t[7], &ok);
        if (r->arg[3] < 0 || r->arg[3] > 8 ||
            r->arg[4] < 414 || r->arg[4] > 542 ||
            id < 0 || id > 32767) ok = 0;
        r->num_ids = 1;
        r->ids[0] = (short)id;
    } else if (!strcmp(t[1], "find-gather-build") && n == 9) {
        int id;
        r->verb = V_FIND_GATHER_BUILD;
        r->arg[0] = parse_int(t[2], &ok);
        r->arg[1] = parse_int(t[3], &ok);
        r->arg[2] = parse_int(t[4], &ok);
        r->arg[3] = parse_int(t[5], &ok);
        r->arg[4] = parse_int(t[6], &ok);
        r->arg[5] = parse_int(t[7], &ok);
        id = parse_int(t[8], &ok);
        if (r->arg[3] < 2 || r->arg[3] > 23 ||
            r->arg[4] < r->arg[3] || r->arg[4] > 23 ||
            r->arg[4] - r->arg[3] > 1 ||
            r->arg[5] < 414 || r->arg[5] > 542 ||
            id < 0 || id > 32767) ok = 0;
        r->num_ids = 1;
        r->ids[0] = (short)id;
    } else if (!strcmp(t[1], "trace-move") && n == 7) {
        int id;
        r->verb = V_TRACE_MOVE;
        r->arg[0] = parse_int(t[2], &ok);
        id = parse_int(t[3], &ok);
        r->arg[1] = parse_int(t[4], &ok);
        r->arg[2] = parse_int(t[5], &ok);
        r->arg[3] = parse_int(t[6], &ok);
        if (id < 0 || id > 32767 || r->arg[3] < 1 || r->arg[3] > 180) ok = 0;
        r->num_ids = 1;
        r->ids[0] = (short)id;
    } else if (!strcmp(t[1], "run-frames") && n == 3) {
        r->verb = V_RUN_FRAMES;
        r->arg[0] = parse_int(t[2], &ok);
        if (r->arg[0] < 1 || r->arg[0] > 30) ok = 0;
    } else return 0;
    if (!ok) return 0;
    if ((r->verb == V_MOVE || r->verb == V_HALT || r->verb == V_ATTACK ||
         r->verb == V_TRACE_MOVE || r->verb == V_OBSERVE_GUYS ||
         r->verb == V_VALIDATE_QUEUE || r->verb == V_VALIDATE_BUILD ||
         r->verb == V_FIND_BUILD || r->verb == V_FIND_GATHER_BUILD ||
         r->verb == V_GATHER || r->verb == V_QUEUE_UP || r->verb == V_BUILD) &&
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
    h = CreateFileA(g_request_path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE,
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
        case V_ATTACK: return "attack"; case V_TRACE_MOVE: return "trace-move";
        case V_OBSERVE_GUYS: return "observe-guys";
        case V_OBSERVE_PLAYER: return "observe-player";
        case V_GATHER: return "gather"; case V_QUEUE_UP: return "queue";
        case V_BUILD: return "build";
        case V_VALIDATE_QUEUE: return "validate-queue";
        case V_VALIDATE_BUILD: return "validate-build";
        case V_RUN_FRAMES: return "run-frames";
        case V_FIND_BUILD: return "find-build";
        case V_FIND_GATHER_BUILD: return "find-gather-build";
        default: return "unknown";
    }
}

static const char *phase_name(unsigned phase) {
    switch (phase) {
        case 0: return "observed"; case 1: return "queued"; case 2: return "applied";
        case 3: return "timeout"; case 4: return "rejected";
        case 5: return "trace-sample"; case 6: return "trace-complete";
        case 7: return "trace-bounded"; default: return "unknown";
    }
}

static void write_event(const event_t *e) {
    char line[262144], hex[MAX_COMMAND_CAPTURE * 2 + 1], tech_hex[203];
    unsigned i;
    size_t used;
    HANDLE h;
    DWORD wrote;
    for (i = 0; i < e->command_len && i < MAX_COMMAND_CAPTURE; i++)
        sprintf(hex + i * 2, "%02x", e->command[i]);
    hex[i * 2] = 0;
    for (i = 0; i < sizeof(e->player_tech_bits); i++)
        sprintf(tech_hex + i * 2, "%02x", e->player_tech_bits[i]);
    tech_hex[sizeof(e->player_tech_bits) * 2] = 0;
    _snprintf(line, sizeof(line) - 1,
        "{\"seq\":%u,\"verb\":\"%s\",\"phase\":\"%s\","
        "\"tick\":%u,\"game\":\"0x%08x\",\"frame\":%u,\"seconds\":%u,"
        "\"paused\":%d,\"speed\":%d,\"network\":%u,\"package_before\":%d,"
        "\"package_after\":%d,\"command_hex\":\"%s\","
        "\"first_object\":\"0x%08x\",\"order_length\":%d,"
        "\"current_order\":\"0x%08x\",\"order_vtable\":\"0x%08x\","
        "\"order_flags\":%u,\"order_metric\":%u,"
        "\"unit_x\":%d,\"unit_y\":%d,"
        "\"unit_x_stored\":\"0x%08x\",\"unit_y_stored\":\"0x%08x\","
        "\"unit_type_pointer\":\"0x%08x\",\"unit_type\":%d,"
        "\"unit_masks\":%u,\"unit_form\":%u,\"unit_guy_mark\":%u,"
        "\"unit_type_guy_spacing\":%d,\"unit_angle\":%u,"
        "\"unit_dest_angle\":%u,\"unit_orders_x\":%d,\"unit_orders_y\":%d,"
        "\"move_valid\":%d,\"move_x\":%d,\"move_y\":%d,"
        "\"move_angle\":%d,\"move_dest\":%d,\"move_tolerance\":%d,"
        "\"move_pause\":%d,\"move_retry\":%d,\"move_attempts\":%d,"
        "\"move_timer\":%d,\"move_facing\":%d,"
        "\"move_dest_x\":%d,\"move_dest_y\":%d,"
        "\"move_last_x\":%d,\"move_last_y\":%d,"
        "\"move_coll_x\":%d,\"move_coll_y\":%d,"
        "\"move_orig_x\":%d,\"move_orig_y\":%d,"
        "\"move_off_x\":%d,\"move_off_y\":%d,"
        "\"note\":%u,\"guy_length\":%d,\"guy_capacity\":%d,"
        "\"guy_count\":%d,\"guy_truncated\":%d,\"guys\":[",
        e->seq, verb_name(e->verb), phase_name(e->phase), e->win_tick, e->game,
        e->frame, e->seconds, e->paused, e->speed, e->network, e->package_before,
        e->package_after, hex, e->first_object, e->order_length,
        e->current_order, e->current_order_vtable, e->order_flags, e->order_metric,
        e->unit_x, e->unit_y, e->unit_x_stored, e->unit_y_stored,
        e->unit_type_pointer, e->unit_type, e->unit_masks, e->unit_form,
        e->unit_guy_mark, e->unit_type_guy_spacing, e->unit_angle, e->unit_dest_angle,
        e->unit_orders_x, e->unit_orders_y,
        e->move_valid, e->move_x, e->move_y, e->move_angle, e->move_dest,
        e->move_tolerance, e->move_pause, e->move_retry, e->move_attempts,
        e->move_timer, e->move_facing, e->move_dest_x, e->move_dest_y,
        e->move_last_x, e->move_last_y, e->move_coll_x, e->move_coll_y,
        e->move_orig_x, e->move_orig_y, e->move_off_x, e->move_off_y, e->note,
        e->guy_length, e->guy_capacity, e->guy_count, e->guy_truncated);
    line[sizeof(line) - 1] = 0;
    used = strlen(line);
    for (i = 0; i < (unsigned)e->guy_count && i < MAX_GUYS; i++) {
        const guy_sample_t *g = &e->guys[i];
        int n = _snprintf(line + used, sizeof(line) - used,
            "%s{\"index\":%u,\"pointer\":\"0x%08x\",\"type\":%d,"
            "\"x\":%d,\"y\":%d,\"z\":%d,\"angle\":%u,"
            "\"des_x\":%d,\"des_y\":%d,\"des_angle\":%u,"
            "\"last_x\":%d,\"last_y\":%d,\"off_x\":%d,\"off_y\":%d,"
            "\"guy_num\":%u}",
            i ? "," : "", i, g->pointer, g->type, g->x, g->y, g->z, g->angle,
            g->des_x, g->des_y, g->des_angle, g->last_x, g->last_y,
            g->off_x, g->off_y, g->guy_num);
        if (n < 0 || (size_t)n >= sizeof(line) - used) break;
        used += (size_t)n;
    }
    {
        int n = _snprintf(line + used, sizeof(line) - used,
            "],\"local_player\":%d,\"world_tile_xs\":%d,\"world_tile_ys\":%d,"
            "\"player_pop\":%d,\"player_pop_cap\":%d,"
            "\"player_leader_flags\":%u,\"player_identity_flags\":%u,"
            "\"player_tribe\":%d,\"player_team\":%d,"
            "\"player_resources\":[%d,%d,%d,%d,%d,%d],"
            "\"player_resource_caps\":[%d,%d,%d,%d,%d,%d],"
            "\"player_over_cap\":[%d,%d,%d,%d,%d,%d],"
            "\"player_age\":%d,\"player_epochs\":[%d,%d,%d,%d],"
            "\"player_tech_bits_hex\":\"%s\","
            "\"player_queued_type_count\":%d,"
            "\"player_queued_type_truncated\":%d,"
            "\"player_slots\":%d,\"player_unit_mark\":%d,"
            "\"player_build_mark\":%d,\"player_wall_mark\":%d,"
            "\"player_object_count\":%d,\"player_object_truncated\":%d,"
            "\"validation_result\":%d,\"placement_x\":%d,"
            "\"placement_y\":%d,\"placement_tested\":%d,"
            "\"placement_legal\":%d,\"placement_seen\":%d,"
            "\"placement_capacity\":%d,\"placement_snap_x\":%d,"
            "\"placement_snap_y\":%d,\"placement_ring\":%d,"
            "\"player_queued_types\":[",
            e->local_player, e->world_tile_xs, e->world_tile_ys,
            e->player_pop, e->player_pop_cap, e->player_leader_flags,
            e->player_identity_flags, e->player_tribe, e->player_team,
            e->player_resources[0], e->player_resources[1], e->player_resources[2],
            e->player_resources[3], e->player_resources[4], e->player_resources[5],
            e->player_resource_caps[0], e->player_resource_caps[1],
            e->player_resource_caps[2], e->player_resource_caps[3],
            e->player_resource_caps[4], e->player_resource_caps[5],
            e->player_over_cap[0], e->player_over_cap[1], e->player_over_cap[2],
            e->player_over_cap[3], e->player_over_cap[4],
            e->player_over_cap[5], e->player_age, e->player_epochs[0],
            e->player_epochs[1], e->player_epochs[2], e->player_epochs[3], tech_hex,
            e->player_queued_type_count, e->player_queued_type_truncated,
            e->player_slots, e->player_unit_mark,
            e->player_build_mark, e->player_wall_mark, e->player_object_count,
            e->player_object_truncated, e->validation_result,
            e->placement_x, e->placement_y, e->placement_tested,
            e->placement_legal, e->placement_seen, e->placement_capacity,
            e->placement_snap_x, e->placement_snap_y, e->placement_ring);
        if (n > 0 && (size_t)n < sizeof(line) - used) used += (size_t)n;
    }
    for (i = 0; i < (unsigned)e->player_queued_type_count && i < MAX_QUEUED_TYPES; i++) {
        const queued_type_sample_t *q = &e->player_queued_types[i];
        int n = _snprintf(line + used, sizeof(line) - used,
                          "%s{\"type\":%d,\"count\":%u}",
                          i ? "," : "", q->type, q->count);
        if (n < 0 || (size_t)n >= sizeof(line) - used) break;
        used += (size_t)n;
    }
    {
        int n = _snprintf(line + used, sizeof(line) - used, "],\"player_objects\":[");
        if (n > 0 && (size_t)n < sizeof(line) - used) used += (size_t)n;
    }
    for (i = 0; i < (unsigned)e->player_object_count && i < MAX_PUBLIC_OBJECTS; i++) {
        const public_object_t *o = &e->player_objects[i];
        unsigned q;
        int n = _snprintf(line + used, sizeof(line) - used,
            "%s{\"id\":%d,\"pointer\":\"0x%08x\",\"category\":%u,"
            "\"flags\":%u,\"uid\":%u,\"type\":%d,\"type_valid\":%d,"
            "\"x\":%d,\"y\":%d,\"z\":%d,\"hits\":%d,"
            "\"class_vtable\":\"0x%08x\",\"angle\":%u,\"order_length\":%d,"
            "\"order_vtable\":\"0x%08x\",\"order_flags\":%u,"
            "\"order_metric\":%u,\"order_target_id\":%d,"
            "\"order_target_uid\":%u,\"order_target_valid\":%d,"
            "\"queued_build_target_id\":%d,\"queued_build_target_uid\":%u,"
            "\"queued_build_target_valid\":%d,\"queued_build_order_seen\":%d,"
            "\"guy_length\":%d,\"gather_max\":%d,"
            "\"queue_logical\":%d,\"queue_size\":%d,"
            "\"queue_count\":%d,\"queue_truncated\":%d,\"queue\":[",
            i ? "," : "", o->id, o->pointer, o->category, o->flags, o->uid,
            o->type, o->type_valid, o->x, o->y, o->z, o->hits,
            o->class_vtable, o->angle, o->order_length, o->order_vtable,
            o->order_flags, o->order_metric, o->order_target_id,
            o->order_target_uid, o->order_target_valid,
            o->queued_build_target_id, o->queued_build_target_uid,
            o->queued_build_target_valid, o->queued_build_order_seen, o->guy_length,
            o->gather_max, o->queue_logical, o->queue_size,
            o->queue_count, o->queue_truncated);
        if (n < 0 || (size_t)n >= sizeof(line) - used) break;
        used += (size_t)n;
        for (q = 0; q < (unsigned)o->queue_count && q < MAX_BUILD_QUEUE; q++) {
            n = _snprintf(line + used, sizeof(line) - used,
                          "%s{\"elapsed\":%d,\"type\":%d}", q ? "," : "",
                          o->queue[q].elapsed, o->queue[q].type);
            if (n < 0 || (size_t)n >= sizeof(line) - used) break;
            used += (size_t)n;
        }
        n = _snprintf(line + used, sizeof(line) - used, "]}");
        if (n < 0 || (size_t)n >= sizeof(line) - used) break;
        used += (size_t)n;
    }
    _snprintf(line + used, sizeof(line) - used, "]}\r\n");
    line[sizeof(line) - 1] = 0;
    h = CreateFileA(g_events_path, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
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
    char buf[768];
    HANDLE h;
    DWORD wrote;
    _snprintf(buf, sizeof(buf), "state=%s\r\npid=%lu\r\nroot=%s\r\nbase=0x%08x\r\n"
              "turn_call_site=0x%08x\r\nturn_do_frame=0x%08x\r\n", state,
              GetCurrentProcessId(), g_root,
              g_base, g_base + RVA_TURN_CALL_SITE,
              g_base + RVA_TURN_DO_FRAME);
    h = CreateFileA(g_ready_path, GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE,
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
    CreateDirectoryA(g_root, NULL);
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
        if (GetFileAttributesA(g_stop_path) != INVALID_FILE_ATTRIBUTES) {
            remove_hook();
            write_ready("parked");
            while (GetFileAttributesA(g_stop_path) != INVALID_FILE_ATTRIBUTES) {
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
        if (!init_paths(module)) return TRUE;
        g_base = (unsigned)(ULONG_PTR)GetModuleHandleA(NULL);
        g_self_process = GetCurrentProcess();
        g_hook_addr = (BYTE *)(g_base + RVA_TURN_CALL_SITE);
        thread = CreateThread(NULL, 0, worker, NULL, 0, NULL);
        if (thread) CloseHandle(thread);
    }
    return TRUE;
}
