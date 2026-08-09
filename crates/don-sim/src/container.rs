//! Growable containers with the engine's own capacity behaviour.
//!
//! # Why a `Vec` is not good enough
//!
//! `Array<T>::walk_data` emits **`length`, `size`, `increment`, `flags`, then the
//! elements** [measured, `schema/state-schema.json`: `SimpleArray<int>` walks `[12,14)`
//! for `increment` plus stack-built words for the rest; `PtrArray<Guy>` walks `[8,14)`
//! = `size` + `increment`; `Stack<PathData>` walks `[4,13)` = `size` + `length` +
//! `increment`]. Since `CheckSum`, `SaveGame` and `LoadGame` are the only `DataWalk`
//! implementations, **capacity and the growth hint are lockstep-critical state**.
//!
//! Two clients with identical logical contents but different capacities produce
//! different checksums and desync. A Rust `Vec` grows 0 -> 4 -> 8 -> 16; the engine grows
//! 5 -> 10 -> 20 -> 40. So a `Vec` in walked sim state is a latent desync, always.
//!
//! # The measured policy
//!
//! `ArrayBase<T>::add` `0x0042DAF0`:
//!
//! ```text
//! if (length >= size) increase_size((short)this->increment);
//! list[length] = v;
//! return length++;
//! ```
//!
//! `increase_size(n)` `0x004220E0` / `0x00422300`:
//!
//! ```text
//! if (n == 0) return;                      ; 0x004220EC — a zero hint never grows
//! if (n < 0)  n = (size != 0) ? size : 4;  ; 0x004220F5 — "double, or 4 from empty"
//! size += n;                               ; 0x00422117
//! realloc; memcpy(length elements); free
//! if ((i8)flags < 0) length = size;        ; 0x00422148 — bit 7 keeps length pinned to size
//! ```
//!
//! Initial state, from the constructors:
//!
//! | container | VA | initial `size` | initial `increment` |
//! |---|---|---:|---:|
//! | `SimpleArray<int>` | `0x0042DA80` | **5** | **-1** (doubling) |
//! | `Stack<PathData>` | `0x0046D720` (`init`) | **10** | caller's byte |
//!
//! So the default array capacity sequence is 5, 10, 20, 40, 80 — and an array that was
//! *constructed* empty (`size == 0`) takes its first growth to 4, not 5.
//!
//! # Fidelity
//!
//! **Tier C.** Transcribed from the disassembly of `increase_size`, `ArrayBase::add`,
//! `Stack::push` and the two constructors; unit-tested here against the transcription.
//! Never executed against retail. The `flags` bit-7 behaviour is implemented but no
//! caller in this crate sets it yet.

/// `increment == -1`: grow by the current capacity, or 4 from empty.
pub const INCREMENT_DOUBLE: i16 = -1;
/// `SimpleArray<T>`'s constructor capacity — `0x0042DA98`.
pub const ARRAY_INITIAL_SIZE: i32 = 5;
/// `Stack<T>::init`'s capacity — `0x0046D7BD`.
pub const STACK_INITIAL_SIZE: i32 = 10;
/// The growth an empty array takes when `increment` is negative — `0x004220EE`.
pub const EMPTY_GROWTH: i32 = 4;
/// `ArrayBase::flags` bit 7: after a grow, `length` is pinned to `size`.
pub const FLAG_LENGTH_TRACKS_SIZE: u8 = 0x80;

/// `size += increase_by(n, size)` — the whole of `increase_size`'s sizing decision.
///
/// Split out because it is the one line that a `Vec` gets wrong, and it deserves to be
/// testable without a container around it.
#[inline]
pub fn increase_by(hint: i16, size: i32) -> i32 {
    if hint == 0 {
        0
    } else if hint < 0 {
        if size != 0 {
            size
        } else {
            EMPTY_GROWTH
        }
    } else {
        hint as i32
    }
}

/// `ArrayBase<T>` / `SimpleArray<T>` with retail's capacity behaviour.
///
/// Elements live in a `Vec` whose Rust capacity is irrelevant; what is modelled — and
/// what is checksummed — is [`EngineArray::size`], the engine's own `size` field.
#[derive(Clone, Debug)]
pub struct EngineArray<T> {
    items: Vec<T>,
    /// `+8` `size`: the engine's allocated capacity. **In the checksum.**
    size: i32,
    /// `+12` `increment`: the growth hint. **In the checksum.** Negative doubles.
    increment: i16,
    /// `+20` `flags`.
    flags: u8,
}

impl<T: Clone + Default> Default for EngineArray<T> {
    fn default() -> Self {
        EngineArray::new()
    }
}

impl<T: Clone + Default> EngineArray<T> {
    /// A default-constructed `SimpleArray<T>`: `size = 5`, `increment = -1`.
    pub fn new() -> EngineArray<T> {
        EngineArray {
            items: Vec::with_capacity(ARRAY_INITIAL_SIZE as usize),
            size: ARRAY_INITIAL_SIZE,
            increment: INCREMENT_DOUBLE,
            flags: 0,
        }
    }

    /// An array constructed with an explicit capacity and hint, as `init` does.
    pub fn with_size(size: i32, increment: i16) -> EngineArray<T> {
        EngineArray {
            items: Vec::with_capacity(size.max(0) as usize),
            size,
            increment,
            flags: 0,
        }
    }

    /// `+4` `length`.
    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// `+8` `size` — the engine's capacity, which is checksummed.
    #[inline]
    pub fn size(&self) -> i32 {
        self.size
    }

    /// `+12` `increment`, also checksummed.
    #[inline]
    pub fn increment(&self) -> i16 {
        self.increment
    }

    #[inline]
    pub fn flags(&self) -> u8 {
        self.flags
    }

    pub fn set_flags(&mut self, f: u8) {
        self.flags = f;
    }

    /// `ArrayBase<T>::increase_size` `0x004220E0`.
    pub fn increase_size(&mut self, hint: i16) {
        let by = increase_by(hint, self.size);
        if by == 0 {
            return;
        }
        self.size = self.size.wrapping_add(by);
        self.items
            .reserve(self.size.max(0) as usize - self.items.len().min(self.size.max(0) as usize));
        if self.flags & FLAG_LENGTH_TRACKS_SIZE != 0 {
            self.items.resize(self.size.max(0) as usize, T::default());
        }
    }

    /// `ArrayBase<T>::add` `0x0042DAF0`. Returns the index written, as retail does.
    pub fn add(&mut self, v: T) -> usize {
        if self.items.len() as i32 >= self.size {
            let inc = self.increment;
            self.increase_size(inc);
        }
        self.items.push(v);
        self.items.len() - 1
    }

    #[inline]
    pub fn get(&self, i: usize) -> Option<&T> {
        self.items.get(i)
    }

    #[inline]
    pub fn get_mut(&mut self, i: usize) -> Option<&mut T> {
        self.items.get_mut(i)
    }

    #[inline]
    pub fn as_slice(&self) -> &[T] {
        &self.items
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.items
    }

    /// Remove element `i`, shifting the tail down. **Capacity is not reduced** — the
    /// engine never shrinks, and a shrink here would be a checksum divergence.
    pub fn remove(&mut self, i: usize) -> Option<T> {
        if i < self.items.len() {
            Some(self.items.remove(i))
        } else {
            None
        }
    }

    /// Drop every element. `size` and `increment` survive, because retail's `close` is a
    /// different function from its `clear`.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// The four header words the walker emits, in walk order, before the elements.
    ///
    /// `length`, `size`, `increment`, `flags` — feed these into any checksum that claims
    /// to be the engine's.
    pub fn checksum_header(&self) -> (i32, i32, i16, u8) {
        (
            self.items.len() as i32,
            self.size,
            self.increment,
            self.flags,
        )
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.items.iter()
    }
}

impl<T> std::ops::Index<usize> for EngineArray<T> {
    type Output = T;
    fn index(&self, i: usize) -> &T {
        &self.items[i]
    }
}

impl<T> std::ops::IndexMut<usize> for EngineArray<T> {
    fn index_mut(&mut self, i: usize) -> &mut T {
        &mut self.items[i]
    }
}

impl<T: PartialEq> PartialEq for EngineArray<T> {
    /// Two arrays are equal only if their **capacities and hints** agree too. Anything
    /// looser would let a desync through a test.
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
            && self.size == other.size
            && self.increment == other.increment
            && self.flags == other.flags
    }
}
impl<T: Eq> Eq for EngineArray<T> {}

/// `Stack<T>` — the shape `UnitData::path` (`Stack<PathData>`, +184) uses.
///
/// Same growth rule, different initial capacity (`10`, `0x0046D7BD`) and a `char`
/// increment rather than a `short`. `Stack<PathData>::walk_data` walks `[4, 13)`, i.e.
/// `size`, `length` and `increment` — all three checksummed.
#[derive(Clone, Debug)]
pub struct EngineStack<T> {
    items: Vec<T>,
    size: i32,
    increment: i8,
}

impl<T: Clone + Default> Default for EngineStack<T> {
    fn default() -> Self {
        EngineStack::new(INCREMENT_DOUBLE as i8)
    }
}

impl<T: Clone + Default> EngineStack<T> {
    /// `Stack<T>::init` `0x0046D720`: capacity 10, caller's increment.
    pub fn new(increment: i8) -> EngineStack<T> {
        EngineStack {
            items: Vec::with_capacity(STACK_INITIAL_SIZE as usize),
            size: STACK_INITIAL_SIZE,
            increment,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    #[inline]
    pub fn size(&self) -> i32 {
        self.size
    }
    #[inline]
    pub fn increment(&self) -> i8 {
        self.increment
    }

    /// `Stack<T>::push` `0x0046D820`. Returns the index written.
    pub fn push(&mut self, v: T) -> usize {
        if self.items.len() as i32 >= self.size {
            let by = increase_by(self.increment as i16, self.size);
            self.size = self.size.wrapping_add(by);
            if by == 0 {
                // A zero increment cannot grow; retail would then write past the buffer.
                // Refusing is the only non-UB option, and it is louder than corrupting.
                panic!(
                    "Stack::push with increment 0 at capacity {}: retail overruns here",
                    self.size
                );
            }
        }
        self.items.push(v);
        self.items.len() - 1
    }

    /// `Stack<T>::pop` `0x0046D870`.
    pub fn pop(&mut self) -> Option<T> {
        self.items.pop()
    }

    /// `Stack<T>::peek` `0x0046D890`.
    pub fn peek(&self) -> Option<&T> {
        self.items.last()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn as_slice(&self) -> &[T] {
        &self.items
    }

    /// `size`, `length`, `increment` — the walk order of `Stack<PathData>::walk_data`.
    pub fn checksum_header(&self) -> (i32, i32, i8) {
        (self.size, self.items.len() as i32, self.increment)
    }
}

impl<T: PartialEq> PartialEq for EngineStack<T> {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items && self.size == other.size && self.increment == other.increment
    }
}
impl<T: Eq> Eq for EngineStack<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sequence a `Vec` gets wrong.
    #[test]
    fn a_default_array_grows_five_ten_twenty_forty() {
        let mut a: EngineArray<i32> = EngineArray::new();
        assert_eq!(a.size(), 5);
        assert_eq!(a.increment(), -1);
        let mut seen = vec![a.size()];
        for i in 0..80 {
            a.add(i);
            if *seen.last().unwrap() != a.size() {
                seen.push(a.size());
            }
        }
        assert_eq!(seen, vec![5, 10, 20, 40, 80]);
        assert_eq!(a.len(), 80);
    }

    /// An array whose capacity is genuinely zero takes 4 first, not 5. That asymmetry is
    /// in `increase_size` (`cmovne edi, eax` at `0x004220FC`), not in the constructor.
    #[test]
    fn growing_from_a_zero_capacity_takes_four() {
        assert_eq!(increase_by(-1, 0), 4);
        assert_eq!(increase_by(-1, 5), 5);
        assert_eq!(increase_by(-1, 40), 40);
        let mut a: EngineArray<u8> = EngineArray::with_size(0, -1);
        a.add(1);
        assert_eq!(a.size(), 4);
    }

    /// A positive hint is a literal step, not a factor.
    #[test]
    fn a_positive_increment_is_a_fixed_step() {
        let mut a: EngineArray<u8> = EngineArray::with_size(2, 3);
        for i in 0..10u8 {
            a.add(i);
        }
        assert_eq!(a.size(), 11, "2, then +3 three times");
    }

    /// Zero never grows — retail returns immediately and the next write overruns.
    #[test]
    fn a_zero_increment_never_grows() {
        assert_eq!(increase_by(0, 100), 0);
        let mut a: EngineArray<u8> = EngineArray::with_size(2, 0);
        a.add(1);
        a.add(2);
        a.add(3);
        assert_eq!(a.size(), 2, "capacity is frozen; only `length` moved");
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn the_stack_starts_at_ten() {
        let mut s: EngineStack<i32> = EngineStack::new(-1);
        assert_eq!(s.size(), 10);
        for i in 0..25 {
            s.push(i);
        }
        assert_eq!(s.size(), 40, "10 -> 20 -> 40");
        assert_eq!(s.peek(), Some(&24));
        assert_eq!(s.pop(), Some(24));
        assert_eq!(s.len(), 24);
        assert_eq!(s.size(), 40, "popping never shrinks capacity");
    }

    /// Equality has to include the header, or a desync passes a test.
    #[test]
    fn equality_includes_capacity() {
        let mut a: EngineArray<i32> = EngineArray::new();
        let mut b: EngineArray<i32> = EngineArray::with_size(100, -1);
        for i in 0..3 {
            a.add(i);
            b.add(i);
        }
        assert_eq!(a.as_slice(), b.as_slice(), "same contents");
        assert_ne!(a, b, "different capacity is a different checksum");
        assert_eq!(a.checksum_header(), (3, 5, -1, 0));
        assert_eq!(b.checksum_header(), (3, 100, -1, 0));
    }

    /// Removing elements must not shrink `size`; the engine never frees on remove.
    #[test]
    fn removal_does_not_shrink_capacity() {
        let mut a: EngineArray<i32> = EngineArray::new();
        for i in 0..12 {
            a.add(i);
        }
        assert_eq!(a.size(), 20);
        for _ in 0..12 {
            a.remove(0);
        }
        assert!(a.is_empty());
        assert_eq!(a.size(), 20);
        a.clear();
        assert_eq!(a.size(), 20, "clear is not close");
    }

    /// `flags` bit 7 pins `length` to `size` after a grow.
    #[test]
    fn the_length_tracks_size_flag_is_honoured() {
        let mut a: EngineArray<i32> = EngineArray::with_size(2, 2);
        a.set_flags(FLAG_LENGTH_TRACKS_SIZE);
        a.add(1);
        a.add(2);
        a.add(3);
        assert_eq!(a.size(), 4);
        assert_eq!(
            a.len(),
            5,
            "grew to 4, length pinned to 4, then the add appended"
        );
    }
}
