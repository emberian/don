//! `Achieve::add_event` 0x007AF660, `AchieveData::record_data` 0x007AF270
//! mode 1 (append + min/max), `AchieveData::condense` 0x007AF140 and the
//! `Achieve::capture_data` 0x007AF980 interval tail (times halving, `rate
//! <<= 1`), plus the exact `SimpleArray<int>::increase_size` 0x004220E0 /
//! `ObjectArray<AchieveEvent>::increase_size` 0x0042F650 growth they go
//! through. Leaf module: free functions for the Leader end-pass
//! (`leaders_end_process`) and step 18 (`misc_steps`) to call; never edits
//! sibling modules.
//!
//! # Image ↔ state
//!
//! `Achieve` (0x00e87d40, PDB `Achieve` sizeof 0x864):
//!
//! ```text
//!   +0x004  times            SimpleArray<int>           Save.achieve.list
//!   +0x020  data[6]          AchieveData, stride 0x140  Save.achieve.data[r]
//!   +0x7a0  events[8]        ObjectArray<AchieveEvent>  Save.achieve.events[who]
//!   +0x860  max_record_times u16 ─┐                      Save.achieve.head (low u16)
//!   +0x862  rate             u16 ─┘                      Save.achieve.head (high u16)
//! AchieveData (stride 0x140):
//!   +0x004  values[8]        SimpleArray<int>, stride 0x1c   data[r].l[p]
//!   +0x0f8  nation_max[8]    int                             data[r].b[p*4..]
//!   +0x118  nation_min[8]    int                             data[r].c[p*4..]
//!   +0x138  max_value        int                             data[r].a[0..4]
//!   +0x13c  min_value        int                             data[r].a[4..8]
//! AchieveEvent (stride 0x1c): frame i32, type i32, String   events[who].elems[i].{head, s}
//! ```
//!
//! Container headers (`length, size, increment, flags`) are walked only
//! when `length != 0` (`SimpleArray<int>::walk_data` 0x00473120,
//! `ObjectArray<AchieveEvent>::walk_data` 0x00495130, which also clears
//! flag bit 6 in memory and in the stream); on load of an empty array the
//! engine `close()`s it (`size = 0, flags = 0`) and keeps the in-memory
//! `increment` — the constructor default `-1` (`SimpleArray<int>` ctor
//! 0x00433c00, `ObjectArray<AchieveEvent>` ctor 0x0042F5B0). Every array
//! here is constructed once at static-init and only ever `init`'d by
//! `Achieve::clear` 0x007AFB80 (which touches lengths, not increments), so
//! an empty array grows from `(size 0, inc -1)`.
//!
//! # Growth (`increase_size(increment)` when `size <= length`)
//!
//! ```text
//! SimpleArray<int>  0x004220E0: if n == 0: nothing (then the store overruns — retail UB)
//!                               if n <  0: n = size != 0 ? size : 4
//!                               size += n; if flags & 0x80: length = size
//! ObjectArray<T>    0x0042F650: if n == 0: if size == 0: return; (else grows by 0)
//!                               if n <  0: n = size != 0 ? size : 4
//!                               size += n; if flags & 0x80: length = size
//! ```
//!
//! # Bodies
//!
//! ```text
//! Achieve::add_event(type, who, &str) 0x007AF660:
//!   frame = Game.frame; tmp = str
//!   a = events[who]; if a.size <= a.length: a.increase_size(a.increment)
//!   a.length += 1; e = a.list[a.length-1]; e.frame = frame; e.type = type; e.string = tmp
//!   (then a Log line — no state)
//! AchieveData::record_data(p, value, 1) 0x007AF270:
//!   v = values[p]; last = v.length - 1
//!   if v.size <= v.length: v.increase_size(v.increment)
//!   v.list[v.length] = value; v.length += 1
//!   if last >= 0: prev = v.list[last]                 ; the sample being FINALISED, not `value`
//!     nation_max[p] = max(nation_max[p], prev); nation_min[p] = min(nation_min[p], prev)
//!     min_value = min(min_value, prev);           max_value = max(max_value, prev)
//! Achieve::record_data(1) tail 0x007AED4C: times.push(Game.frame) with the same growth
//! Achieve::capture_data interval tail 0x007AF9B4 (times.length >= max_record_times):
//!   for r in 0..6: data[r].condense()
//!   times[0] = times[1]; for i in 1..times.length/2: times[i] = times[2i]; times.length = max(1, len/2)
//!   rate <<= 1
//! AchieveData::condense() 0x007AF140:
//!   max_value = INT_MIN; min_value = INT_MAX
//!   for p in 0..8 where LeaderData[p].flags & 1:
//!     nation_max[p] = INT_MIN; nation_min[p] = INT_MAX
//!     v = values[p]; v[0] = v[1]; fold v[0] into the four extrema
//!     for i in 1..v.length/2: v[i] = v[2i]; fold v[i]
//!     v.length = max(1, v.length/2)              ; size/increment untouched
//! ```
//!
//! Note the halving keeps `v[1]` (not `v[0]`) as the new first sample, and
//! `times.length` becomes `max(1, len/2)` because the loop variable starts
//! at 1 (`i` is stored even when the loop body never runs).
//!
//! # Oracle
//!
//! `Achieve.rate` is 15 in every capture, so `record_data(1)` fires on
//! `frame % 15 == 0` (the stride-15 capture spans several). The
//! `achieve_arrays_grow_like_retail` test replays the append into
//! `Save.achieve` from frame N and compares every walked header
//! (`len/cap/inc/flags`) and sample against frame N+15; it is gated to the
//! pairs where the interval frame's *other* records (3 income Σ, 4 tech
//! hash — `misc_steps` stopping points) can be copied from retail, since
//! only the container mechanics are under test here. `introduced == 0`.

#![allow(dead_code)]

use crate::prim::{Arr, SimpleVec};
use crate::sections::{Achieve, AchieveEvent, Save};
use crate::tick::StepStatus;

pub const STATUS: StepStatus = StepStatus::Partial;

/// Constructor-default `increment` of every Achieve container (-1 =
/// "double, or 4 from empty").
pub const DEFAULT_INCREMENT: i16 = -1;

fn get_i32(v: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(v[o..o + 4].try_into().unwrap())
}

fn put_i32(v: &mut [u8], o: usize, x: i32) {
    v[o..o + 4].copy_from_slice(&x.to_le_bytes());
}

/// The one growth rule shared by `SimpleArray<int>::increase_size`
/// 0x004220E0 and `ObjectArray<T>::increase_size` 0x0042F650, applied to a
/// walked `(len, cap, inc, flags)` header. Returns the new `len` when
/// `flags & 0x80` forced `len = cap` (the caller's store then lands at the
/// old `len` index anyway, as retail's does).
fn grow(len: &mut i32, cap: &mut i32, inc: i16, flags: u8, object_array: bool) -> bool {
    let mut n = inc as i32;
    if n == 0 {
        if object_array && *cap == 0 {
            return false;
        }
        if !object_array {
            return false;
        }
    } else if n < 0 {
        n = if *cap != 0 { *cap } else { 4 };
    }
    *cap += n;
    if flags & 0x80 != 0 {
        *len = *cap;
        return true;
    }
    false
}

/// `SimpleArray<int>::add(value)` as `record_data(…, 1)` and the `times`
/// push perform it: grow when full, store at `length`, `length += 1`.
/// An empty (never-walked) array starts from the constructor header.
/// Returns `true` when the capacity changed.
pub fn simple_array_push(v: &mut SimpleVec, value: i32) -> bool {
    let mut len = (v.data.len() / 4) as i32;
    if len == 0 {
        // Header not walked for an empty array: constructor defaults.
        v.cap = 0;
        v.inc = DEFAULT_INCREMENT;
        v.flags = 0;
    }
    let old_cap = v.cap;
    let store_at = len as usize;
    if v.cap <= len {
        grow(&mut len, &mut v.cap, v.inc, v.flags, false);
    }
    // list[length] = value; length += 1  (length here is the pre-grow
    // length unless flags & 0x80 moved it — then retail stores at the new
    // length index; we mirror that by resizing to it first).
    if len as usize > store_at {
        v.data.resize(len as usize * 4, 0);
    }
    let at = v.data.len();
    v.data.resize(at + 4, 0);
    put_i32(&mut v.data, at, value);
    v.len = (v.data.len() / 4) as i32;
    v.cap != old_cap
}

/// `ObjectArray<AchieveEvent>::add` as `add_event` performs it.
pub fn event_array_push(a: &mut Arr<AchieveEvent>, ev: AchieveEvent) -> bool {
    let mut len = a.elems.len() as i32;
    if len == 0 {
        a.cap = 0;
        a.inc = DEFAULT_INCREMENT;
        a.flags = 0;
    }
    let old_cap = a.cap;
    let store_at = len as usize;
    if a.cap <= len {
        grow(&mut len, &mut a.cap, a.inc, a.flags, true);
    }
    while a.elems.len() < store_at.max(len as usize) {
        a.elems.push(AchieveEvent::default());
    }
    a.elems.push(ev);
    a.len = a.elems.len() as i32;
    a.cap != old_cap
}

/// `Achieve::add_event(type, who, &str)` 0x007AF660. `frame` is
/// `Game.frame` at the call. The Leader end-pass callers (0x006ec180,
/// 0x006ec9b0, 0x006ecb00, 0x00730ef0) all pass `String::EMPTY_STRING`
/// 0x00eb437c; the city callers (0x00623e20, 0x00733380) pass the city
/// name — the stored string *is* the argument, not always `""`.
pub fn add_event(ach: &mut Achieve, ty: i32, who: usize, frame: i32, s: &[u16], effects: &mut Vec<String>) {
    if ach.events.len() < 8 {
        ach.events.resize_with(8, Arr::default);
    }
    let Some(a) = ach.events.get_mut(who) else { return };
    let mut head = vec![0u8; 8];
    put_i32(&mut head, 0, frame);
    put_i32(&mut head, 4, ty);
    let grew = event_array_push(a, AchieveEvent { head, s: s.to_vec() });
    effects.push(format!(
        "Achieve.events[{who}] += {{frame {frame}, type {ty}, {:?}}} (len {}, cap {}{})",
        String::from_utf16_lossy(s),
        a.elems.len(),
        a.cap,
        if grew { ", grew" } else { "" }
    ));
}

/// `AchieveData::record_data(p, value, 1)` 0x007AF270 on `Save.achieve.data[r]`.
pub fn record_data_append(ach: &mut Achieve, r: usize, p: usize, value: i32, effects: &mut Vec<String>) {
    let Some(rec) = ach.data.get_mut(r) else { return };
    if rec.l.len() < 8 {
        rec.l.resize_with(8, SimpleVec::default);
    }
    if rec.b.len() < 32 || rec.c.len() < 32 || rec.a.len() < 8 {
        return;
    }
    let v = &mut rec.l[p];
    let last = (v.data.len() / 4) as i32 - 1;
    let grew = simple_array_push(v, value);
    if last >= 0 {
        let prev = get_i32(&v.data, last as usize * 4);
        let nmax = get_i32(&rec.b, p * 4).max(prev);
        let nmin = get_i32(&rec.c, p * 4).min(prev);
        put_i32(&mut rec.b, p * 4, nmax);
        put_i32(&mut rec.c, p * 4, nmin);
        let min_v = get_i32(&rec.a, 4).min(prev);
        let max_v = get_i32(&rec.a, 0).max(prev);
        put_i32(&mut rec.a, 4, min_v);
        put_i32(&mut rec.a, 0, max_v);
    }
    effects.push(format!(
        "Achieve.data[{r}].values[{p}] += {value} (len {}, cap {}{})",
        rec.l[p].data.len() / 4,
        rec.l[p].cap,
        if grew { ", grew" } else { "" }
    ));
}

/// `Achieve::record_data(1)` tail 0x007AED4C: `times.push(Game.frame)`.
pub fn times_append(ach: &mut Achieve, frame: i32, effects: &mut Vec<String>) {
    let grew = simple_array_push(&mut ach.list, frame);
    effects.push(format!("Achieve.times += {frame} (len {}, cap {}{})", ach.list.data.len() / 4, ach.list.cap, if grew { ", grew" } else { "" }));
}

/// `max_record_times` (low u16 of `Achieve.head`) / `rate` (high u16).
pub fn max_record_times(ach: &Achieve) -> i32 {
    (ach.head as u32 & 0xffff) as i32
}
pub fn rate(ach: &Achieve) -> i32 {
    ((ach.head as u32) >> 16) as i32
}

/// Halve a `SimpleArray<int>` in place the way `condense` / the `times`
/// tail do: `v[0] = v[1]; v[i] = v[2i] for i in 1..len/2; len = max(1, len/2)`.
/// Returns the retained samples (for the extrema fold). Size/increment/flags
/// are untouched.
fn halve(v: &mut SimpleVec) -> Vec<i32> {
    let len = (v.data.len() / 4) as i32;
    let mut kept = Vec::new();
    if len >= 2 {
        let v1 = get_i32(&v.data, 4);
        put_i32(&mut v.data, 0, v1);
        kept.push(v1);
    } else if len == 1 {
        // `v[0] = v[1]` reads one past the end in retail (heap garbage);
        // unreachable under the caller's `len >= max_record_times` gate.
        kept.push(get_i32(&v.data, 0));
    } else {
        return kept;
    }
    let mut i = 1;
    while i < len / 2 {
        let x = get_i32(&v.data, i as usize * 8);
        put_i32(&mut v.data, i as usize * 4, x);
        kept.push(x);
        i += 1;
    }
    v.data.truncate(i as usize * 4);
    v.len = i;
    kept
}

/// `AchieveData::condense()` 0x007AF140 on `Save.achieve.data[r]`;
/// `active` is `LeaderData[p].flags & 1` for `p in 0..8`.
pub fn condense(ach: &mut Achieve, r: usize, active: [bool; 8], effects: &mut Vec<String>) {
    let Some(rec) = ach.data.get_mut(r) else { return };
    if rec.l.len() < 8 || rec.b.len() < 32 || rec.c.len() < 32 || rec.a.len() < 8 {
        return;
    }
    put_i32(&mut rec.a, 0, i32::MIN);
    put_i32(&mut rec.a, 4, i32::MAX);
    for p in 0..8 {
        if !active[p] {
            continue;
        }
        put_i32(&mut rec.b, p * 4, i32::MIN);
        put_i32(&mut rec.c, p * 4, i32::MAX);
        let kept = halve(&mut rec.l[p]);
        for x in kept {
            let nmax = get_i32(&rec.b, p * 4).max(x);
            let nmin = get_i32(&rec.c, p * 4).min(x);
            put_i32(&mut rec.b, p * 4, nmax);
            put_i32(&mut rec.c, p * 4, nmin);
            let min_v = get_i32(&rec.a, 4).min(x);
            let max_v = get_i32(&rec.a, 0).max(x);
            put_i32(&mut rec.a, 4, min_v);
            put_i32(&mut rec.a, 0, max_v);
        }
    }
    effects.push(format!("Achieve.data[{r}] condensed"));
}

/// The `Achieve::capture_data` interval tail 0x007AF9B4 after
/// `record_data(1)`: when `times.length >= max_record_times`, condense all
/// six records, halve `times`, `rate <<= 1`. Returns whether it fired.
pub fn condense_if_full(save: &mut Save, effects: &mut Vec<String>) -> bool {
    let len = (save.achieve.list.data.len() / 4) as i32;
    if len < max_record_times(&save.achieve) {
        return false;
    }
    let mut active = [false; 8];
    for (p, slot) in active.iter_mut().enumerate() {
        *slot = save.leaders.slots.get(p).map(|l| l.flags & 1 != 0).unwrap_or(false);
    }
    for r in 0..6 {
        condense(&mut save.achieve, r, active, effects);
    }
    halve(&mut save.achieve.list);
    let new_rate = (rate(&save.achieve) << 1) & 0xffff;
    save.achieve.head = ((new_rate as u32) << 16 | (save.achieve.head as u32 & 0xffff)) as i32;
    effects.push(format!("Achieve: times halved to {}, rate -> {new_rate}", save.achieve.list.data.len() / 4));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{container, load};
    use std::path::{Path, PathBuf};

    fn sv(vals: &[i32], cap: i32, inc: i16, flags: u8) -> SimpleVec {
        let mut v = SimpleVec { len: vals.len() as i32, cap, inc, flags, data: Vec::new() };
        for x in vals {
            v.data.extend_from_slice(&x.to_le_bytes());
        }
        v
    }

    fn vals(v: &SimpleVec) -> Vec<i32> {
        v.data.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()
    }

    #[test]
    fn simple_array_growth_sequence() {
        // From empty: 4, then doubling (increment -1).
        let mut v = SimpleVec::default();
        let caps: Vec<i32> = (0..10)
            .map(|i| {
                simple_array_push(&mut v, i);
                v.cap
            })
            .collect();
        assert_eq!(caps, vec![4, 4, 4, 4, 8, 8, 8, 8, 16, 16]);
        assert_eq!(vals(&v), (0..10).collect::<Vec<_>>());
        assert_eq!(v.inc, -1);
        // Positive increment: fixed steps.
        let mut v = sv(&[1, 2, 3], 3, 10, 0);
        assert!(simple_array_push(&mut v, 4));
        assert_eq!((v.cap, vals(&v)), (13, vec![1, 2, 3, 4]));
        assert!(!simple_array_push(&mut v, 5));
        // Walked header with room: no growth.
        let mut v = sv(&[1], 8, -1, 0);
        assert!(!simple_array_push(&mut v, 2));
        assert_eq!(v.cap, 8);
        // Full with doubling: cap doubles.
        let mut v = sv(&[1, 2, 3, 4, 5, 6, 7, 8], 8, -1, 0);
        assert!(simple_array_push(&mut v, 9));
        assert_eq!(v.cap, 16);
    }

    #[test]
    fn event_array_growth_sequence() {
        let mut a: Arr<AchieveEvent> = Arr::default();
        let mut eff = Vec::new();
        let mut ach = Achieve::default();
        for i in 0..6 {
            add_event(&mut ach, 9, 3, 100 + i, &[], &mut eff);
        }
        a.elems = ach.events[3].elems.clone();
        assert_eq!(a.elems.len(), 6);
        assert_eq!(ach.events[3].cap, 8);
        assert_eq!(ach.events[3].inc, -1);
        assert_eq!(get_i32(&ach.events[3].elems[5].head, 0), 105);
        assert_eq!(get_i32(&ach.events[3].elems[5].head, 4), 9);
        assert!(ach.events[3].elems[5].s.is_empty());
        // Other owners untouched.
        assert!(ach.events[0].elems.is_empty());
    }

    #[test]
    fn record_data_finalises_previous_sample() {
        let mut ach = Achieve::default();
        ach.data.resize_with(6, Default::default);
        let rec = &mut ach.data[2];
        rec.l = (0..8).map(|_| SimpleVec::default()).collect();
        rec.a = vec![0; 8];
        rec.b = vec![0; 32];
        rec.c = vec![0; 32];
        put_i32(&mut rec.a, 0, i32::MIN); // max_value
        put_i32(&mut rec.a, 4, i32::MAX); // min_value
        for p in 0..8 {
            put_i32(&mut rec.b, p * 4, i32::MIN);
            put_i32(&mut rec.c, p * 4, i32::MAX);
        }
        let mut eff = Vec::new();
        // First append: no previous sample, extrema untouched.
        record_data_append(&mut ach, 2, 5, 40, &mut eff);
        assert_eq!(vals(&ach.data[2].l[5]), vec![40]);
        assert_eq!(get_i32(&ach.data[2].b, 20), i32::MIN);
        // Second append finalises 40 (not 70).
        record_data_append(&mut ach, 2, 5, 70, &mut eff);
        assert_eq!(vals(&ach.data[2].l[5]), vec![40, 70]);
        assert_eq!(get_i32(&ach.data[2].b, 20), 40, "nation_max[5]");
        assert_eq!(get_i32(&ach.data[2].c, 20), 40, "nation_min[5]");
        assert_eq!(get_i32(&ach.data[2].a, 0), 40, "max_value");
        assert_eq!(get_i32(&ach.data[2].a, 4), 40, "min_value");
        record_data_append(&mut ach, 2, 5, 10, &mut eff);
        assert_eq!(get_i32(&ach.data[2].b, 20), 70);
        assert_eq!(get_i32(&ach.data[2].c, 20), 40);
        // Another leader's extrema are separate; the record-wide ones shared.
        record_data_append(&mut ach, 2, 1, -5, &mut eff);
        record_data_append(&mut ach, 2, 1, 0, &mut eff);
        assert_eq!(get_i32(&ach.data[2].c, 4), -5);
        assert_eq!(get_i32(&ach.data[2].a, 4), -5, "min_value across leaders");
        assert_eq!(get_i32(&ach.data[2].a, 0), 70, "max_value across leaders");
    }

    #[test]
    fn condense_halves_and_refolds() {
        let mut ach = Achieve::default();
        ach.data.resize_with(6, Default::default);
        let rec = &mut ach.data[0];
        rec.l = (0..8).map(|_| SimpleVec::default()).collect();
        rec.a = vec![0; 8];
        rec.b = vec![0; 32];
        rec.c = vec![0; 32];
        rec.l[0] = sv(&[10, 20, 30, 40, 50, 60, 70, 80], 8, -1, 0);
        rec.l[1] = sv(&[5, 6, 7], 4, -1, 0);
        rec.l[2] = sv(&[99, 98], 4, -1, 0); // inactive leader: untouched
        let mut active = [false; 8];
        active[0] = true;
        active[1] = true;
        let mut eff = Vec::new();
        condense(&mut ach, 0, active, &mut eff);
        let rec = &ach.data[0];
        // v[0] = v[1]; v[i] = v[2i] for i in 1..4  -> [20, 30, 50, 70]
        assert_eq!(vals(&rec.l[0]), vec![20, 30, 50, 70]);
        assert_eq!(rec.l[0].cap, 8, "size untouched");
        assert_eq!((get_i32(&rec.b, 0), get_i32(&rec.c, 0)), (70, 20));
        // len 3: v[0] = v[1]; loop 1..1 empty; len = 1 -> [6]
        assert_eq!(vals(&rec.l[1]), vec![6]);
        assert_eq!((get_i32(&rec.b, 4), get_i32(&rec.c, 4)), (6, 6));
        assert_eq!(vals(&rec.l[2]), vec![99, 98]);
        assert_eq!((get_i32(&rec.a, 0), get_i32(&rec.a, 4)), (70, 6));
    }

    #[test]
    fn condense_if_full_halves_times_and_doubles_rate() {
        let mut s = Save::default();
        s.achieve.data.resize_with(6, Default::default);
        for r in 0..6 {
            let rec = &mut s.achieve.data[r];
            rec.l = (0..8).map(|_| SimpleVec::default()).collect();
            rec.a = vec![0; 8];
            rec.b = vec![0; 32];
            rec.c = vec![0; 32];
        }
        s.achieve.list = sv(&[0, 15, 30, 45, 60, 75], 8, -1, 0);
        s.achieve.head = ((15u32 << 16) | 6) as i32; // rate 15, max_record_times 6
        let mut eff = Vec::new();
        assert!(condense_if_full(&mut s, &mut eff));
        assert_eq!(vals(&s.achieve.list), vec![15, 30, 60]);
        assert_eq!(rate(&s.achieve), 30);
        assert_eq!(max_record_times(&s.achieve), 6);
        assert_eq!(s.achieve.list.cap, 8);
        assert!(!condense_if_full(&mut s, &mut eff));
    }

    // -----------------------------------------------------------------------
    // Capture oracle
    // -----------------------------------------------------------------------

    fn capture_dirs() -> Vec<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/live/frame-pairs");
        let Ok(rd) = std::fs::read_dir(&root) else { return Vec::new() };
        let mut v: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.join("manifest.json").exists()).collect();
        v.sort();
        v
    }

    fn steps(dir: &Path) -> Vec<(i64, String)> {
        let text = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
        let mut out = Vec::new();
        for seg in text.split("\"frame\":").skip(1) {
            let frame = seg.trim_start().split(|c: char| !c.is_ascii_digit()).next().unwrap().parse::<i64>().unwrap();
            let save = seg.split("\"save_name\":").nth(1).and_then(|s| s.split('"').nth(1)).unwrap().to_string();
            out.push((frame, save));
        }
        out
    }

    fn load_save(dir: &Path, name: &str) -> Save {
        let raw = container::load_svx(&dir.join(format!("{name}.svx"))).unwrap();
        load(&raw).unwrap().state
    }

    fn hdr(v: &SimpleVec) -> (usize, i32, i16, u8) {
        (v.data.len() / 4, v.cap, v.inc, v.flags)
    }

    /// Every capture pair that crosses a `frame % rate == 0` interval frame:
    /// replay `record_data(1)` on frame-N state with retail's own new
    /// sample values (records 3/4 come from `misc_steps`' untranscribed
    /// readers; the container mechanics are what is under test), plus the
    /// `times` push, and require the walked headers and every sample to
    /// equal frame N+k. Also checks that `events` arrays never change
    /// between captures (no Leader event fired) or, when they do, that the
    /// growth matches [`event_array_push`].
    #[test]
    fn achieve_arrays_grow_like_retail() {
        let dirs = capture_dirs();
        if dirs.is_empty() {
            eprintln!("no captures; skipping");
            return;
        }
        let mut checked = 0;
        let mut grew = 0;
        for dir in dirs {
            let st = steps(&dir);
            for k in 0..st.len().saturating_sub(1) {
                let a = load_save(&dir, &st[k].1);
                let b = load_save(&dir, &st[k + 1].1);
                let r = rate(&a.achieve);
                if r == 0 {
                    continue;
                }
                let (fa, fb) = (st[k].0 as i32, st[k + 1].0 as i32);
                // Interval frames strictly inside (fa, fb]: capture_data runs
                // with Game.frame == f before frame++ (step 18 precedes 20),
                // so a capture at frame f already contains f's sample.
                let intervals: Vec<i32> = (fa + 1..=fb).filter(|f| f % r == 0).collect();
                let ta = vals(&a.achieve.list);
                let tb = vals(&b.achieve.list);
                if intervals.is_empty() {
                    assert_eq!(ta.len(), tb.len(), "{}: f{fa}->f{fb} times grew with no interval frame", dir.display());
                    continue;
                }
                assert_eq!(
                    tb.len(),
                    ta.len() + intervals.len(),
                    "{}: f{fa}->f{fb} times {} -> {} over intervals {intervals:?}",
                    dir.display(),
                    ta.len(),
                    tb.len()
                );
                let mut ours = a.clone();
                let mut eff = Vec::new();
                for (n, f) in intervals.iter().enumerate() {
                    // The sample appended at interval n for record r / leader p
                    // is retail's value at index len_a + n.
                    for rr in 0..6 {
                        for p in 0..8 {
                            let va = &a.achieve.data[rr].l[p];
                            let vb = &b.achieve.data[rr].l[p];
                            let la = va.data.len() / 4;
                            if vb.data.len() / 4 <= la + n {
                                continue; // inactive leader: never appended
                            }
                            let value = get_i32(&vb.data, (la + n) * 4);
                            record_data_append(&mut ours.achieve, rr, p, value, &mut eff);
                        }
                    }
                    times_append(&mut ours.achieve, *f, &mut eff);
                    condense_if_full(&mut ours, &mut eff);
                }
                // Mode-0 overwrites of the last sample between intervals are
                // misc_steps' writes: copy retail's final last sample so the
                // comparison isolates the container mechanics.
                for rr in 0..6 {
                    for p in 0..8 {
                        let vo = &mut ours.achieve.data[rr].l[p];
                        let vb = &b.achieve.data[rr].l[p];
                        if !vo.data.is_empty() && vo.data.len() == vb.data.len() {
                            let n = vo.data.len() - 4;
                            let last = get_i32(&vb.data, n);
                            put_i32(&mut vo.data, n, last);
                        }
                        assert_eq!(
                            hdr(vo),
                            hdr(vb),
                            "{}: f{fa}->f{fb} data[{rr}].values[{p}] header (len,cap,inc,flags)",
                            dir.display()
                        );
                        assert_eq!(vals(vo), vals(vb), "{}: f{fa}->f{fb} data[{rr}].values[{p}]", dir.display());
                        if a.achieve.data[rr].l[p].cap != vb.cap {
                            grew += 1;
                        }
                    }
                    // Extrema: nation_max/min and max/min_value for records
                    // 0,1,2,5 whose samples misc_steps fully derives; 3/4 too,
                    // since their appended values were copied from retail.
                    assert_eq!(ours.achieve.data[rr].b, b.achieve.data[rr].b, "{}: f{fa}->f{fb} data[{rr}].nation_max", dir.display());
                    assert_eq!(ours.achieve.data[rr].c, b.achieve.data[rr].c, "{}: f{fa}->f{fb} data[{rr}].nation_min", dir.display());
                    assert_eq!(ours.achieve.data[rr].a, b.achieve.data[rr].a, "{}: f{fa}->f{fb} data[{rr}].max/min_value", dir.display());
                }
                if !ours.achieve.list.data.is_empty() {
                    let n = ours.achieve.list.data.len() - 4;
                    put_i32(&mut ours.achieve.list.data, n, get_i32(&b.achieve.list.data, n));
                }
                assert_eq!(hdr(&ours.achieve.list), hdr(&b.achieve.list), "{}: f{fa}->f{fb} times header", dir.display());
                assert_eq!(vals(&ours.achieve.list), tb, "{}: f{fa}->f{fb} times", dir.display());
                assert_eq!(ours.achieve.head, b.achieve.head, "{}: f{fa}->f{fb} rate/max_record_times", dir.display());
                // Events: growth, if any, must be reproducible.
                for who in 0..8 {
                    let (ea, eb) = (&a.achieve.events[who], &b.achieve.events[who]);
                    if ea.elems.len() == eb.elems.len() {
                        assert_eq!((ea.cap, ea.inc, ea.flags), (eb.cap, eb.inc, eb.flags), "events[{who}] header drift");
                        continue;
                    }
                    let mut e = ea.clone();
                    for i in ea.elems.len()..eb.elems.len() {
                        event_array_push(&mut e, eb.elems[i].clone());
                    }
                    assert_eq!((e.cap, e.inc, e.flags), (eb.cap, eb.inc, eb.flags), "{}: f{fa}->f{fb} events[{who}] growth", dir.display());
                    grew += 1;
                }
                checked += 1;
                eprintln!("{}: f{fa}->f{fb} intervals {intervals:?}: times {} -> {} ok", dir.file_name().unwrap().to_string_lossy(), ta.len(), tb.len());
            }
        }
        eprintln!("achieve interval pairs checked: {checked}; arrays that grew: {grew}");
        assert!(checked > 0, "no capture pair crossed an Achieve interval frame");
    }
}
