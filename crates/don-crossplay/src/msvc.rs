// SPDX-License-Identifier: GPL-3.0-or-later
//! The MSVC x86 object boundary: reading what retail hands us, and building
//! what retail reads back.
//!
//! [`crate::abi`] pins the *shape* of every parameter — a `wstring` is 24 bytes,
//! a `std::function` is 40, a `LobbyDTO` is 208. That is enough to *call* a
//! method and not enough to *use* one: a lobby attribute arrives as an
//! `unordered_map<wstring, wstring>`, and 32 opaque bytes carry no key/value
//! pairs. This module is the layer that turns those aggregates into DoN data and
//! back, and every offset in it was read out of `CrossplayProxy.pdb` or off the
//! shipped machine code.
//!
//! # Measured layouts
//!
//! From `tools/pdb-extract` over `ron-bin/sbl/CrossplayProxy.pdb`
//! (GUID `{d36f6bf3-9aa0-4388-895e-f3e8f776e191}`), cross-checked against the
//! disassembly cited beside each one. **[measured]**
//!
//! ```text
//! std::basic_string<wchar_t>                          size 24
//!   +0   _Bx        union { wchar_t _Buf[8]; wchar_t* _Ptr; }   16
//!   +16  _Mysize    unsigned int      (code units, excluding NUL)
//!   +20  _Myres     unsigned int      (capacity; < 8 means _Buf is live)
//!
//! std::unordered_map<wstring, wstring>  ==  std::_Hash<...>     size 32
//!   +0   _Traitsobj  std::_Umap_traits<...>                      4
//!   +4   _List       std::list<pair<const wstring, wstring>>     8
//!          +4  _Myhead   _List_node*
//!          +8  _Mysize   unsigned int
//!   +12  _Vec        std::_Hash_vec<...>                        12
//!          +12 _Myfirst  _List_node**
//!          +16 _Mylast
//!          +20 _Myend
//!   +24  _Mask       unsigned int
//!   +28  _Maxidx     unsigned int
//!
//! std::_List_node<pair<const wstring, wstring>>        size 56
//!   +0   _Next     +4  _Prev     +8  key (24)     +32  value (24)
//!
//! std::vector<T>                                       size 12
//!   +0   _Myfirst  +4  _Mylast   +8  _Myend
//! ```
//!
//! The `_Hash` field offsets are confirmed a second time by the shipped code:
//! `std::_Hash<_Umap_traits<wstring, wstring, ...>>::_Find_last` at
//! `CrossplayProxy.dll` RVA `0x3580` loads `_Mask` from `[ecx+0x18]`, `_Vec`'s
//! first pointer from `[ecx+0xc]` and `_List._Myhead` from `[ecx+4]`; the
//! `unordered_map` destructor at RVA `0x3c90` zeroes `[esi+0xc]`, `[esi+0x10]`
//! and `[esi+0x14]` after `operator delete`-ing the bucket buffer, and hands
//! `[esi+4]` to the list destructor. **[measured]**
//!
//! The `wstring` small-string rule is measured in the same function: RVA
//! `0x35c0` is `cmp dword ptr [ebx+0x1c], 8` — `_Myres` of the node's key — and
//! branches to the inline buffer when it is **below** 8. So `_Myres < 8` means
//! `_Buf` is live. **[measured]**
//!
//! # The one construction decision, and why it needs no hash function
//!
//! `_Find_last(this, key, hashval)` — the single lookup primitive under `find`,
//! `at`, `count`, `operator[]` and `equal_range` — was disassembled in full:
//!
//! ```text
//! bucket = _Mask & hashval                   ; 0x3584, 0x3587
//! last   = _Vec._Myfirst[2*bucket + 1]       ; lea eax,[eax+edx*8] then [eax+4]
//! if last == _List._Myhead: return not-found ; 0x3597
//! first  = _Vec._Myfirst[2*bucket]           ; 0x35ae
//! for node = last; ; node = node->_Prev:     ; 0x3616
//!     if node->key == key: return node       ; 0x35e4 wchar-by-wchar compare
//!     if node == first: break                ; 0x3611
//! ```
//!
//! Two things fall out. Buckets are **two pointers wide** (`edx*8`), holding the
//! first and last node of that bucket. And with `_Mask == 0` the bucket index is
//! `0` for **every** `hashval`, so a map built with a single bucket is looked up
//! by a linear scan of the whole list and resolves correctly **whatever
//! `std::hash<wstring>` computes**. That is why nothing here reproduces MSVC's
//! string hash: it is not needed, and it was not derived. A DoN lobby carries a
//! few dozen attributes, so the linear scan is the entire cost.
//!
//! `bucket_count()` on such a map reports 1. Nothing in the shipped interface
//! exposes a bucket count to the game.
//!
//! `_Traitsobj` (the 4 bytes at `+0`) is written as zero. No lookup path reads
//! it — `_Find_last` touches only `+4`, `+0xc` and `+0x18` — so its contents are
//! **unobserved**, and zero is recorded rather than a guess dressed up as a
//! measurement.
//!
//! # Why the guest memory is abstracted
//!
//! Every pointer above is 32 bits. On the i686 target that is a real pointer and
//! `RawMem` uses the CRT allocator directly. On the arm64 development host a
//! real pointer does not fit, so [`ArenaMem`] provides the same interface over a
//! byte vector with a synthetic base. Writing the builders against the trait
//! means the layout code that ships on x86 is the same code the host tests
//! execute — and [`lookup_like_find_last`], a direct transcription of the
//! disassembly above, can be run over a built map to check it is findable.
//!
//! **That is a check against a transcription, not against the shipped code.** It
//! cannot fail if the transcription and the builder share a misreading. Nothing
//! in this crate has been executed by `CrossplayProxy.dll`'s callers.
//!
//! # What the tests do and do not pin
//!
//! Mutation-tested by hand, each reverted immediately after:
//!
//! | mutation | result |
//! |---|---|
//! | `_Mask` `0` → `7` (claim eight buckets, allocate two entries) | **fails** |
//! | swap the bucket pair's `{first, last}` | **fails** |
//! | move `LobbyDTO::_attributes` from `+104` to `+100` | **fails**, 4 tests |
//! | move the `wstring` small-string threshold from 7 to 8 units | **fails**, 11 tests |
//! | bucket stride `bucket * 8` → `bucket * 4` in the transcription | **passes** |
//!
//! The last row is the honest caveat, and it is a caveat about the *test*, not
//! the code: with `_Mask == 0` the bucket index is always 0, so no test can
//! distinguish a stride of 8 from any other. The `edx*8` in the disassembly is
//! the only evidence for it. Nothing here depends on the stride either —
//! [`read_attributes`] walks the intrusive list and never looks at a bucket —
//! so the single unpinned constant is also the one with no reachable
//! consequence.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::local::{Attributes, Lobby, Member};

/// A 32-bit guest pointer. Always the shipped ABI's width, on every host.
pub type Gp = u32;

/// Measured byte offsets and sizes. See the module docs for provenance.
pub mod layout {
    /// `sizeof(std::basic_string<wchar_t>)`.
    pub const WSTRING: u32 = 24;
    /// `_Bx` — 16 bytes of union: `wchar_t _Buf[8]` or `wchar_t* _Ptr`.
    pub const WSTRING_BUF: u32 = 0;
    /// `_Mysize`, in code units, excluding the NUL.
    pub const WSTRING_SIZE: u32 = 16;
    /// `_Myres`. `_Myres < WSTRING_SSO_LIMIT` means `_Buf` holds the text.
    pub const WSTRING_CAPACITY: u32 = 20;
    /// The measured small-string threshold: `cmp [x+0x1c], 8; jb inline`.
    pub const WSTRING_SSO_LIMIT: u32 = 8;
    /// `_Buf` is `wchar_t[8]`, so at most 7 code units plus the NUL.
    pub const WSTRING_SSO_UNITS: u32 = 7;

    /// `sizeof(std::unordered_map<wstring, wstring>)`, i.e. `std::_Hash<...>`.
    pub const HASH: u32 = 32;
    pub const HASH_TRAITS: u32 = 0;
    pub const HASH_LIST_HEAD: u32 = 4;
    pub const HASH_LIST_SIZE: u32 = 8;
    pub const HASH_VEC_FIRST: u32 = 12;
    pub const HASH_VEC_LAST: u32 = 16;
    pub const HASH_VEC_END: u32 = 20;
    pub const HASH_MASK: u32 = 24;
    pub const HASH_MAXIDX: u32 = 28;

    /// `sizeof(std::_List_node<pair<const wstring, wstring>>)`.
    pub const LIST_NODE: u32 = 56;
    pub const LIST_NODE_NEXT: u32 = 0;
    pub const LIST_NODE_PREV: u32 = 4;
    pub const LIST_NODE_KEY: u32 = 8;
    pub const LIST_NODE_VALUE: u32 = 32;

    /// `sizeof(std::vector<T>)` — `_Myfirst`, `_Mylast`, `_Myend`.
    pub const VECTOR: u32 = 12;
    pub const VECTOR_FIRST: u32 = 0;
    pub const VECTOR_LAST: u32 = 4;
    pub const VECTOR_END: u32 = 8;

    /// `sizeof(UserDTO)` == `sizeof(LobbyMemberDTO)`.
    pub const MEMBER_DTO: u32 = 96;
    /// `sizeof(LobbyDTO)`.
    pub const LOBBY_DTO: u32 = 208;
    /// `sizeof(TurnServerDTO)`.
    pub const TURN_SERVER_DTO: u32 = 72;
}

/// Read/write/allocate access to the address space the shipped objects live in.
///
/// Implementations must hand back 8-byte-aligned blocks: the largest alignment
/// requirement in any of these aggregates is 4 (`i32`/pointer), and `i64`
/// timestamps in the join/leave DTOs want 8.
pub trait GuestMem {
    /// Allocate `bytes`, zero-initialised. `None` when it cannot.
    fn alloc(&mut self, bytes: u32) -> Option<Gp>;
    /// Release a block previously returned by [`alloc`](Self::alloc).
    fn dealloc(&mut self, ptr: Gp, bytes: u32);
    /// Copy `out.len()` bytes from `ptr`. `false` when the range is not readable.
    fn read(&self, ptr: Gp, out: &mut [u8]) -> bool;
    /// Copy `bytes` to `ptr`. `false` when the range is not writable.
    fn write(&mut self, ptr: Gp, bytes: &[u8]) -> bool;
}

/// Read a little-endian `u32`.
pub fn read_u32<M: GuestMem + ?Sized>(mem: &M, at: Gp) -> Option<u32> {
    let mut b = [0u8; 4];
    mem.read(at, &mut b).then(|| u32::from_le_bytes(b))
}

fn write_u32<M: GuestMem + ?Sized>(mem: &mut M, at: Gp, value: u32) -> bool {
    mem.write(at, &value.to_le_bytes())
}

/// Longest string this module will read out of guest memory.
///
/// A lobby attribute is a game setting, a decimal number or a lobby name; a
/// megabyte-long `_Mysize` is a corrupt or hostile object, not a value we should
/// try to allocate for. **[DoN policy]**
pub const MAX_WSTRING_UNITS: u32 = 8192;

/// Read a `std::basic_string<wchar_t>` living at `at`.
///
/// Returns `None` for an unreadable object, an implausible length, or text that
/// is not valid UTF-16 — every one of those fails closed rather than producing a
/// lossily-repaired string that would then be published to other peers.
pub fn read_wstring<M: GuestMem + ?Sized>(mem: &M, at: Gp) -> Option<String> {
    let size = read_u32(mem, at + layout::WSTRING_SIZE)?;
    let capacity = read_u32(mem, at + layout::WSTRING_CAPACITY)?;
    if size > MAX_WSTRING_UNITS || capacity < size {
        return None;
    }
    let data = if capacity < layout::WSTRING_SSO_LIMIT {
        at + layout::WSTRING_BUF
    } else {
        read_u32(mem, at + layout::WSTRING_BUF)?
    };
    if size == 0 {
        return Some(String::new());
    }
    if data == 0 {
        return None;
    }
    let mut raw = vec![0u8; size as usize * 2];
    if !mem.read(data, &mut raw) {
        return None;
    }
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

/// Read an `unordered_map<wstring, wstring>` living at `at` by walking its
/// intrusive list — `_Mysize` entries starting at `_Myhead->_Next`.
///
/// This is the inbound half of `CreateLobby`'s and `UpdateLobby`'s attribute
/// parameter, and therefore how `game_seed` and the 26 setting keys recovered in
/// `crates/don-net/src/lobby.rs` reach DoN. It never consults the bucket vector,
/// so it is independent of the hash function in either direction.
pub fn read_attributes<M: GuestMem + ?Sized>(mem: &M, at: Gp) -> Option<Attributes> {
    let head = read_u32(mem, at + layout::HASH_LIST_HEAD)?;
    let count = read_u32(mem, at + layout::HASH_LIST_SIZE)?;
    if head == 0 {
        return None;
    }
    if count > MAX_ATTRIBUTES {
        return None;
    }
    let mut out = Attributes::new();
    let mut node = read_u32(mem, head + layout::LIST_NODE_NEXT)?;
    for _ in 0..count {
        if node == 0 || node == head {
            return None;
        }
        let key = read_wstring(mem, node + layout::LIST_NODE_KEY)?;
        let value = read_wstring(mem, node + layout::LIST_NODE_VALUE)?;
        out.insert(key, value);
        node = read_u32(mem, node + layout::LIST_NODE_NEXT)?;
    }
    // A well-formed circular list arrives back at the sentinel after _Mysize
    // hops. Anything else means _Mysize and the chain disagree; refuse it.
    (node == head).then_some(out)
}

/// Most attributes this module will read from one map. `don-net`'s recovered
/// schema is 26 settings keys plus 3 lobby keys plus 8 per-player keys for up to
/// a couple of dozen slots, so this is a wide margin over the real ceiling and
/// still refuses a corrupt `_Mysize`. **[DoN policy]**
pub const MAX_ATTRIBUTES: u32 = 4096;

/// A direct transcription of `_Find_last` (`CrossplayProxy.dll` RVA `0x3580`),
/// used to check that a map this module builds is one the shipped algorithm can
/// look keys up in.
///
/// `hashval` is whatever `std::hash<wstring>` would produce; for a map built by
/// [`Scratch::attributes`] the answer does not depend on it, which is the point.
/// Returns the address of the matching node.
///
/// This is a *reference transcription*, not the shipped code. A test using it
/// proves the builder agrees with this reading of the disassembly, and nothing
/// stronger.
pub fn lookup_like_find_last<M: GuestMem + ?Sized>(
    mem: &M,
    map_at: Gp,
    key: &str,
    hashval: u32,
) -> Option<Gp> {
    let mask = read_u32(mem, map_at + layout::HASH_MASK)?;
    let head = read_u32(mem, map_at + layout::HASH_LIST_HEAD)?;
    let vec_first = read_u32(mem, map_at + layout::HASH_VEC_FIRST)?;
    let bucket = mask & hashval;
    let last = read_u32(mem, vec_first + bucket * 8 + 4)?;
    if last == head {
        return None;
    }
    let first = read_u32(mem, vec_first + bucket * 8)?;
    let mut node = last;
    // The shipped loop has no hop bound because a real `_Hash` is well formed.
    // This transcription is also pointed at deliberately corrupted maps by the
    // tests, so it refuses to spin.
    for _ in 0..=MAX_ATTRIBUTES {
        if read_wstring(mem, node + layout::LIST_NODE_KEY)?.as_str() == key {
            return Some(node);
        }
        if node == first {
            return None;
        }
        node = read_u32(mem, node + layout::LIST_NODE_PREV)?;
    }
    None
}

// ---------------------------------------------------------------------------
// Building objects retail will read.
// ---------------------------------------------------------------------------

/// An allocation arena for one outbound call.
///
/// Every DTO handed to a game callback is a `const&` — retail reads it and
/// copies out whatever it wants, and never takes ownership. So the whole graph
/// belongs to us and can be released in one step once the callback returns,
/// which is exactly what [`release`](Self::release) does.
pub struct Scratch<'m, M: GuestMem + ?Sized> {
    mem: &'m mut M,
    owned: Vec<(Gp, u32)>,
}

impl<'m, M: GuestMem + ?Sized> Scratch<'m, M> {
    pub fn new(mem: &'m mut M) -> Self {
        Self {
            mem,
            owned: Vec::new(),
        }
    }

    /// Free everything this scratch allocated, newest first.
    pub fn release(mut self) {
        while let Some((ptr, bytes)) = self.owned.pop() {
            self.mem.dealloc(ptr, bytes);
        }
    }

    /// Hand the block list to a caller that needs the graph to outlive this
    /// scratch — `GetPlayerGuid` returns a `wstring&` to storage that must stay
    /// valid until the guid changes, so it cannot be released at call exit.
    /// The caller must eventually pass the list to [`free_blocks`].
    pub fn into_blocks(self) -> Vec<(Gp, u32)> {
        self.owned
    }

    /// Number of live blocks — the leak check the tests assert on.
    pub fn live_blocks(&self) -> usize {
        self.owned.len()
    }

    fn alloc(&mut self, bytes: u32) -> Option<Gp> {
        let ptr = self.mem.alloc(bytes)?;
        self.owned.push((ptr, bytes));
        Some(ptr)
    }

    /// Copy `bytes` into a fresh guest block and return its address.
    pub fn place(&mut self, bytes: &[u8]) -> Option<Gp> {
        let ptr = self.alloc(bytes.len() as u32)?;
        self.mem.write(ptr, bytes).then_some(ptr)
    }

    /// Build a `std::basic_string<wchar_t>` holding `text`, as 24 inline bytes.
    ///
    /// Text of seven code units or fewer goes in `_Buf` with `_Myres = 7`; longer
    /// text gets a heap block with `_Myres = _Mysize`. Both forms are
    /// NUL-terminated, because a consumer is entitled to call `c_str()`.
    pub fn wstring(&mut self, text: &str) -> Option<[u8; 24]> {
        let units: Vec<u16> = text.encode_utf16().collect();
        if units.len() as u32 > MAX_WSTRING_UNITS {
            return None;
        }
        let mut out = [0u8; 24];
        let size = units.len() as u32;
        if size <= layout::WSTRING_SSO_UNITS {
            for (i, u) in units.iter().enumerate() {
                out[i * 2..i * 2 + 2].copy_from_slice(&u.to_le_bytes());
            }
            out[layout::WSTRING_CAPACITY as usize..][..4]
                .copy_from_slice(&layout::WSTRING_SSO_UNITS.to_le_bytes());
        } else {
            let bytes = (size + 1) * 2;
            let ptr = self.alloc(bytes)?;
            let mut raw = Vec::with_capacity(bytes as usize);
            for u in &units {
                raw.extend_from_slice(&u.to_le_bytes());
            }
            raw.extend_from_slice(&0u16.to_le_bytes());
            if !self.mem.write(ptr, &raw) {
                return None;
            }
            out[..4].copy_from_slice(&ptr.to_le_bytes());
            out[layout::WSTRING_CAPACITY as usize..][..4].copy_from_slice(&size.to_le_bytes());
        }
        out[layout::WSTRING_SIZE as usize..][..4].copy_from_slice(&size.to_le_bytes());
        Some(out)
    }

    /// Build an `unordered_map<wstring, wstring>` as 32 inline bytes.
    ///
    /// One bucket, so lookup is correct for any `std::hash<wstring>` — see the
    /// module docs. The node list is circular through a sentinel exactly as
    /// `std::list` keeps it, and `_Vec` holds `{first, last}` for the single
    /// bucket. An empty map still gets a sentinel and a bucket pair, with
    /// `_Vec[1] == _Myhead` so `_Find_last`'s empty test at RVA `0x3597` fires.
    pub fn attributes(&mut self, attributes: &Attributes) -> Option<[u8; 32]> {
        let head = self.alloc(layout::LIST_NODE)?;
        let mut nodes = Vec::with_capacity(attributes.len());
        for (key, value) in attributes {
            let node = self.alloc(layout::LIST_NODE)?;
            let k = self.wstring(key)?;
            let v = self.wstring(value)?;
            self.mem.write(node + layout::LIST_NODE_KEY, &k).then_some(())?;
            self.mem
                .write(node + layout::LIST_NODE_VALUE, &v)
                .then_some(())?;
            nodes.push(node);
        }
        // Link the ring: head -> n0 -> n1 -> ... -> head.
        let chain: Vec<Gp> = core::iter::once(head)
            .chain(nodes.iter().copied())
            .collect();
        for (i, &node) in chain.iter().enumerate() {
            let next = chain[(i + 1) % chain.len()];
            let prev = chain[(i + chain.len() - 1) % chain.len()];
            write_u32(self.mem, node + layout::LIST_NODE_NEXT, next).then_some(())?;
            write_u32(self.mem, node + layout::LIST_NODE_PREV, prev).then_some(())?;
        }
        // One bucket, two pointers: {first, last}. Empty means last == head.
        let buckets = self.alloc(8)?;
        let (first, last) = match (nodes.first(), nodes.last()) {
            (Some(&f), Some(&l)) => (f, l),
            _ => (head, head),
        };
        write_u32(self.mem, buckets, first).then_some(())?;
        write_u32(self.mem, buckets + 4, last).then_some(())?;

        let mut out = [0u8; 32];
        // _Traitsobj stays zero: no measured reader.
        out[layout::HASH_LIST_HEAD as usize..][..4].copy_from_slice(&head.to_le_bytes());
        out[layout::HASH_LIST_SIZE as usize..][..4]
            .copy_from_slice(&(nodes.len() as u32).to_le_bytes());
        out[layout::HASH_VEC_FIRST as usize..][..4].copy_from_slice(&buckets.to_le_bytes());
        out[layout::HASH_VEC_LAST as usize..][..4].copy_from_slice(&(buckets + 8).to_le_bytes());
        out[layout::HASH_VEC_END as usize..][..4].copy_from_slice(&(buckets + 8).to_le_bytes());
        out[layout::HASH_MASK as usize..][..4].copy_from_slice(&0u32.to_le_bytes());
        out[layout::HASH_MAXIDX as usize..][..4].copy_from_slice(&1u32.to_le_bytes());
        Some(out)
    }

    /// Build a `LobbyMemberDTO` (= `UserDTO`) as 96 inline bytes.
    pub fn member(&mut self, member: &Member) -> Option<[u8; 96]> {
        let mut out = [0u8; 96];
        for (i, text) in [
            member.user_id.as_str(),
            member.user_name.as_str(),
            member.platform.as_str(),
            member.platform_account_id.as_str(),
        ]
        .iter()
        .enumerate()
        {
            out[i * 24..i * 24 + 24].copy_from_slice(&self.wstring(text)?);
        }
        Some(out)
    }

    /// Build a `std::vector<T>` over `elements`, each already serialised to
    /// `stride` bytes. Contiguous storage with `_Mylast == _Myend`, which is
    /// what a `shrink_to_fit` vector looks like.
    fn vector(&mut self, elements: &[u8], stride: u32) -> Option<[u8; 12]> {
        debug_assert_eq!(elements.len() as u32 % stride, 0, "ragged element buffer");
        let mut out = [0u8; 12];
        if elements.is_empty() {
            // A default-constructed vector is three null pointers.
            return Some(out);
        }
        let ptr = self.place(elements)?;
        let end = ptr + elements.len() as u32;
        out[layout::VECTOR_FIRST as usize..][..4].copy_from_slice(&ptr.to_le_bytes());
        out[layout::VECTOR_LAST as usize..][..4].copy_from_slice(&end.to_le_bytes());
        out[layout::VECTOR_END as usize..][..4].copy_from_slice(&end.to_le_bytes());
        Some(out)
    }

    /// Build a `LobbyDTO` as 208 inline bytes.
    ///
    /// `_turnServer` is three empty `wstring`s: DoN peers address each other
    /// directly and there is no relay to describe. **[DoN policy]**
    pub fn lobby(&mut self, lobby: &Lobby) -> Option<[u8; 208]> {
        let mut out = [0u8; 208];
        out[0..24].copy_from_slice(&self.wstring(&lobby.id)?);
        out[24..48].copy_from_slice(&self.wstring(&lobby.owner_user_id)?);
        out[48..72].copy_from_slice(&self.wstring(&lobby.session_reference)?);
        out[72..76].copy_from_slice(&lobby.max_members.to_le_bytes());
        out[76..80].copy_from_slice(&lobby.available_slots().to_le_bytes());
        out[80..84].copy_from_slice(&lobby.bot_count.to_le_bytes());
        out[84..88].copy_from_slice(&lobby.attribute_version.to_le_bytes());
        out[88..92].copy_from_slice(&lobby.visibility.to_le_bytes());

        let mut members = Vec::with_capacity(lobby.members.len() * 96);
        for m in &lobby.members {
            members.extend_from_slice(&self.member(m)?);
        }
        out[92..104].copy_from_slice(&self.vector(&members, layout::MEMBER_DTO)?);
        out[104..136].copy_from_slice(&self.attributes(&lobby.attributes)?);
        for i in 0..3 {
            out[136 + i * 24..136 + i * 24 + 24].copy_from_slice(&self.wstring("")?);
        }
        Some(out)
    }

    /// Build a `LobbySearchResultDTO` — one `vector<LobbyDTO>` — as 12 bytes.
    pub fn search_result(&mut self, lobbies: &[Lobby]) -> Option<[u8; 12]> {
        let mut raw = Vec::with_capacity(lobbies.len() * 208);
        for l in lobbies {
            raw.extend_from_slice(&self.lobby(l)?);
        }
        self.vector(&raw, layout::LOBBY_DTO)
    }

    /// Place a built `LobbyDTO` in guest memory and return the address to pass
    /// as the callback's `const LobbyDTO&`.
    pub fn lobby_ref(&mut self, lobby: &Lobby) -> Option<Gp> {
        let dto = self.lobby(lobby)?;
        self.place(&dto)
    }

    /// Place a built `LobbySearchResultDTO` and return its address.
    pub fn search_result_ref(&mut self, lobbies: &[Lobby]) -> Option<Gp> {
        let dto = self.search_result(lobbies)?;
        self.place(&dto)
    }

    /// Place a `wstring` and return its address, for the many callbacks whose
    /// parameter is a `const wstring&` or a by-value `wstring`.
    pub fn wstring_ref(&mut self, text: &str) -> Option<Gp> {
        let s = self.wstring(text)?;
        self.place(&s)
    }
}

/// Release a block list taken from [`Scratch::into_blocks`], newest first.
pub fn free_blocks<M: GuestMem + ?Sized>(mem: &mut M, mut blocks: Vec<(Gp, u32)>) {
    while let Some((ptr, bytes)) = blocks.pop() {
        mem.dealloc(ptr, bytes);
    }
}

// ---------------------------------------------------------------------------
// The two implementations.
// ---------------------------------------------------------------------------

/// Guest memory backed by a plain byte vector at a synthetic base.
///
/// Exists so the layout code that ships on i686 is executed by the tests on the
/// development host, where a real allocation would not fit in a `u32`.
pub struct ArenaMem {
    base: Gp,
    bytes: Vec<u8>,
    /// (offset, size, live)
    blocks: BTreeMap<Gp, (u32, bool)>,
}

impl ArenaMem {
    /// A fresh arena. The base is deliberately not zero so a null pointer is
    /// distinguishable from the first allocation.
    pub fn new() -> Self {
        Self {
            base: 0x1000_0000,
            bytes: Vec::new(),
            blocks: BTreeMap::new(),
        }
    }

    /// Blocks allocated and not yet released.
    pub fn live_blocks(&self) -> usize {
        self.blocks.values().filter(|(_, live)| *live).count()
    }

    fn offset(&self, ptr: Gp, len: usize) -> Option<usize> {
        let off = ptr.checked_sub(self.base)? as usize;
        (off.checked_add(len)? <= self.bytes.len()).then_some(off)
    }
}

impl Default for ArenaMem {
    fn default() -> Self {
        Self::new()
    }
}

impl GuestMem for ArenaMem {
    fn alloc(&mut self, bytes: u32) -> Option<Gp> {
        if bytes == 0 {
            return None;
        }
        // 8-byte alignment, matching what a CRT allocator guarantees.
        let pad = (8 - (self.bytes.len() % 8)) % 8;
        self.bytes.resize(self.bytes.len() + pad, 0);
        let offset = self.bytes.len() as u32;
        self.bytes.resize(self.bytes.len() + bytes as usize, 0);
        let ptr = self.base + offset;
        self.blocks.insert(ptr, (bytes, true));
        Some(ptr)
    }

    fn dealloc(&mut self, ptr: Gp, bytes: u32) {
        match self.blocks.get_mut(&ptr) {
            Some(entry) => {
                debug_assert_eq!(entry.0, bytes, "size mismatch freeing {ptr:#x}");
                debug_assert!(entry.1, "double free of {ptr:#x}");
                entry.1 = false;
            }
            None => debug_assert!(false, "freeing unknown block {ptr:#x}"),
        }
    }

    fn read(&self, ptr: Gp, out: &mut [u8]) -> bool {
        match self.offset(ptr, out.len()) {
            Some(off) => {
                out.copy_from_slice(&self.bytes[off..off + out.len()]);
                true
            }
            None => false,
        }
    }

    fn write(&mut self, ptr: Gp, bytes: &[u8]) -> bool {
        match self.offset(ptr, bytes.len()) {
            Some(off) => {
                self.bytes[off..off + bytes.len()].copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }
}

/// Guest memory that *is* the process address space, for the i686 target.
///
/// Only compiled where a pointer is 32 bits, because that is the only place the
/// identity `Gp == *mut u8` holds. Allocation goes through the CRT the shim is
/// linked against; the game never frees any of it, because everything this crate
/// hands over is a `const&` that retail copies out of.
#[cfg(target_pointer_width = "32")]
pub struct RawMem;

#[cfg(target_pointer_width = "32")]
extern "C" {
    fn malloc(size: usize) -> *mut core::ffi::c_void;
    fn free(ptr: *mut core::ffi::c_void);
}

#[cfg(target_pointer_width = "32")]
impl GuestMem for RawMem {
    fn alloc(&mut self, bytes: u32) -> Option<Gp> {
        if bytes == 0 {
            return None;
        }
        // SAFETY: a plain sized allocation; the result is checked for null and
        // zeroed before anything reads it.
        let ptr = unsafe { malloc(bytes as usize) };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: `ptr` owns `bytes` writable bytes we just obtained.
        unsafe { core::ptr::write_bytes(ptr.cast::<u8>(), 0, bytes as usize) };
        Some(ptr as usize as Gp)
    }

    fn dealloc(&mut self, ptr: Gp, _bytes: u32) {
        if ptr == 0 {
            return;
        }
        // SAFETY: `ptr` came from this allocator's `alloc`.
        unsafe { free(ptr as usize as *mut core::ffi::c_void) }
    }

    fn read(&self, ptr: Gp, out: &mut [u8]) -> bool {
        if ptr == 0 {
            return false;
        }
        // SAFETY: the caller is passing an address retail handed us, or one we
        // allocated. There is no way to validate a raw guest pointer here; the
        // length bounds above (`MAX_WSTRING_UNITS`, `MAX_ATTRIBUTES`) are what
        // keep a corrupt length from turning into an unbounded read.
        unsafe {
            core::ptr::copy_nonoverlapping(ptr as usize as *const u8, out.as_mut_ptr(), out.len())
        };
        true
    }

    fn write(&mut self, ptr: Gp, bytes: &[u8]) -> bool {
        if ptr == 0 {
            return false;
        }
        // SAFETY: as `read`; only ever called on blocks this module allocated.
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as usize as *mut u8, bytes.len())
        };
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::VISIBILITY_PUBLIC;
    use alloc::string::ToString;

    fn sample_lobby() -> Lobby {
        let mut attributes = Attributes::new();
        // Real keys from the schema `crates/don-net/src/lobby.rs` recovered.
        attributes.insert("game_seed".to_string(), "3735928559".to_string());
        attributes.insert("starting_resources2".to_string(), "3".to_string());
        attributes.insert("echowin".to_string(), "1".to_string());
        attributes.insert("steam_ready_0".to_string(), "1".to_string());
        attributes.insert("name".to_string(), "a lobby with a long-ish name".to_string());
        Lobby {
            id: "7".to_string(),
            owner_user_id: "host".to_string(),
            session_reference: "session-reference-that-is-long".to_string(),
            max_members: 8,
            bot_count: 2,
            attribute_version: 4,
            visibility: VISIBILITY_PUBLIC,
            members: vec![Member::new("host", "Host"), Member::new("peer", "A Peer")],
            attributes,
            game_started: false,
        }
    }

    #[test]
    fn wstring_round_trips_through_both_storage_forms() {
        let mut mem = ArenaMem::new();
        let mut s = Scratch::new(&mut mem);
        for text in [
            "",
            "a",
            "1234567",         // exactly the SSO limit
            "12345678",        // one past it
            "3735928559",
            "a lobby with a rather long name indeed",
            "καλημέρα",        // non-ASCII BMP
            "𝄞music",          // surrogate pair
        ] {
            let obj = s.wstring(text).expect("build");
            let at = s.place(&obj).expect("place");
            assert_eq!(
                read_wstring(s.mem, at).as_deref(),
                Some(text),
                "round trip failed for {text:?}"
            );
        }
        s.release();
        assert_eq!(mem.live_blocks(), 0);
    }

    #[test]
    fn the_sso_boundary_is_where_the_disassembly_puts_it() {
        let mut mem = ArenaMem::new();
        let mut s = Scratch::new(&mut mem);
        let short = s.wstring("1234567").unwrap();
        let long = s.wstring("12345678").unwrap();
        let cap = |o: &[u8; 24]| {
            u32::from_le_bytes(o[layout::WSTRING_CAPACITY as usize..][..4].try_into().unwrap())
        };
        // _Myres < 8 selects the inline buffer, so a 7-unit string must report 7
        // and an 8-unit string must report at least 8.
        assert_eq!(cap(&short), 7);
        assert!(cap(&long) >= layout::WSTRING_SSO_LIMIT);
        // ...and the inline form carries the text in _Buf, not a pointer: the
        // first code unit of "1234567" is U+0031, little-endian.
        assert_eq!(&short[0..4], &[0x31, 0x00, 0x32, 0x00]);
        // The heap form's first four bytes are the _Ptr, which must not be a
        // character.
        let ptr = u32::from_le_bytes(long[0..4].try_into().unwrap());
        assert_ne!(ptr, 0);
        let at = s.place(&long).unwrap();
        assert_eq!(read_wstring(s.mem, at).as_deref(), Some("12345678"));
        s.release();
    }

    #[test]
    fn attributes_round_trip_and_are_findable_by_the_transcribed_lookup() {
        let mut mem = ArenaMem::new();
        let lobby = sample_lobby();
        let mut s = Scratch::new(&mut mem);
        let map = s.attributes(&lobby.attributes).expect("build map");
        let at = s.place(&map).expect("place");

        // 1. Walking the list gives every pair back.
        assert_eq!(read_attributes(s.mem, at).as_ref(), Some(&lobby.attributes));

        // 2. The transcription of the shipped _Find_last finds every key, for
        //    an arbitrary hash value — that is what the single bucket buys.
        for hashval in [0u32, 1, 0xdead_beef, u32::MAX] {
            for key in lobby.attributes.keys() {
                let node = lookup_like_find_last(s.mem, at, key, hashval)
                    .unwrap_or_else(|| panic!("{key} not found with hashval {hashval:#x}"));
                let value = read_wstring(s.mem, node + layout::LIST_NODE_VALUE).unwrap();
                assert_eq!(&value, lobby.attributes.get(key).unwrap());
            }
            assert!(lookup_like_find_last(s.mem, at, "no_such_key", hashval).is_none());
        }
        s.release();
        assert_eq!(mem.live_blocks(), 0);
    }

    #[test]
    fn an_empty_attribute_map_is_well_formed_and_looks_up_as_absent() {
        let mut mem = ArenaMem::new();
        let mut s = Scratch::new(&mut mem);
        let map = s.attributes(&Attributes::new()).unwrap();
        let at = s.place(&map).unwrap();
        assert_eq!(read_attributes(s.mem, at), Some(Attributes::new()));
        // _Vec[1] == _Myhead, so _Find_last's empty-bucket test at 0x3597 fires.
        let head = read_u32(s.mem, at + layout::HASH_LIST_HEAD).unwrap();
        let vec_first = read_u32(s.mem, at + layout::HASH_VEC_FIRST).unwrap();
        assert_eq!(read_u32(s.mem, vec_first + 4), Some(head));
        assert!(lookup_like_find_last(s.mem, at, "game_seed", 0).is_none());
        s.release();
        assert_eq!(mem.live_blocks(), 0);
    }

    #[test]
    fn the_node_ring_is_circular_in_both_directions() {
        let mut mem = ArenaMem::new();
        let lobby = sample_lobby();
        let mut s = Scratch::new(&mut mem);
        let map = s.attributes(&lobby.attributes).unwrap();
        let at = s.place(&map).unwrap();
        let head = read_u32(s.mem, at + layout::HASH_LIST_HEAD).unwrap();
        let n = lobby.attributes.len();
        let mut node = head;
        for _ in 0..=n {
            node = read_u32(s.mem, node + layout::LIST_NODE_NEXT).unwrap();
        }
        assert_eq!(node, head, "forward ring must close after size+1 hops");
        let mut node = head;
        for _ in 0..=n {
            node = read_u32(s.mem, node + layout::LIST_NODE_PREV).unwrap();
        }
        assert_eq!(node, head, "backward ring must close after size+1 hops");
        s.release();
    }

    #[test]
    fn lobby_dto_field_offsets_match_the_generated_abi() {
        let mut mem = ArenaMem::new();
        let lobby = sample_lobby();
        let mut s = Scratch::new(&mut mem);
        let at = s.lobby_ref(&lobby).unwrap();

        assert_eq!(read_wstring(s.mem, at).as_deref(), Some(lobby.id.as_str()));
        assert_eq!(
            read_wstring(s.mem, at + 24).as_deref(),
            Some(lobby.owner_user_id.as_str())
        );
        assert_eq!(
            read_wstring(s.mem, at + 48).as_deref(),
            Some(lobby.session_reference.as_str())
        );
        assert_eq!(read_u32(s.mem, at + 72), Some(lobby.max_members as u32));
        assert_eq!(read_u32(s.mem, at + 76), Some(lobby.available_slots() as u32));
        assert_eq!(read_u32(s.mem, at + 80), Some(lobby.bot_count as u32));
        assert_eq!(read_u32(s.mem, at + 84), Some(lobby.attribute_version as u32));
        assert_eq!(read_u32(s.mem, at + 88), Some(lobby.visibility as u32));

        // _members: a contiguous vector of 96-byte LobbyMemberDTOs.
        let first = read_u32(s.mem, at + 92).unwrap();
        let last = read_u32(s.mem, at + 96).unwrap();
        assert_eq!(last - first, lobby.members.len() as u32 * layout::MEMBER_DTO);
        assert_eq!(read_u32(s.mem, at + 100), Some(last), "_Myend == _Mylast");
        for (i, member) in lobby.members.iter().enumerate() {
            let m = first + i as u32 * layout::MEMBER_DTO;
            assert_eq!(read_wstring(s.mem, m).as_deref(), Some(member.user_id.as_str()));
            assert_eq!(
                read_wstring(s.mem, m + 24).as_deref(),
                Some(member.user_name.as_str())
            );
        }
        // _attributes at +104, _turnServer at +136.
        assert_eq!(
            read_attributes(s.mem, at + 104).as_ref(),
            Some(&lobby.attributes)
        );
        for i in 0..3 {
            assert_eq!(
                read_wstring(s.mem, at + 136 + i * 24).as_deref(),
                Some(""),
                "TurnServerDTO field {i} must be an empty string, not garbage"
            );
        }
        s.release();
        assert_eq!(mem.live_blocks(), 0);
    }

    #[test]
    fn a_search_result_is_a_contiguous_vector_of_lobby_dtos() {
        let mut mem = ArenaMem::new();
        let lobbies = vec![sample_lobby(), sample_lobby()];
        let mut s = Scratch::new(&mut mem);
        let at = s.search_result_ref(&lobbies).unwrap();
        let first = read_u32(s.mem, at).unwrap();
        let last = read_u32(s.mem, at + 4).unwrap();
        assert_eq!(last - first, lobbies.len() as u32 * layout::LOBBY_DTO);
        for i in 0..lobbies.len() as u32 {
            let dto = first + i * layout::LOBBY_DTO;
            assert_eq!(read_wstring(s.mem, dto).as_deref(), Some("7"));
            assert_eq!(
                read_attributes(s.mem, dto + 104).as_ref(),
                Some(&lobbies[0].attributes)
            );
        }
        s.release();
        assert_eq!(mem.live_blocks(), 0);
    }

    #[test]
    fn an_empty_search_result_is_three_null_pointers() {
        let mut mem = ArenaMem::new();
        let mut s = Scratch::new(&mut mem);
        let dto = s.search_result(&[]).unwrap();
        assert_eq!(dto, [0u8; 12]);
        s.release();
    }

    #[test]
    fn reading_refuses_a_map_whose_size_disagrees_with_its_chain() {
        let mut mem = ArenaMem::new();
        let mut attrs = Attributes::new();
        attrs.insert("a".to_string(), "1".to_string());
        attrs.insert("b".to_string(), "2".to_string());
        let mut s = Scratch::new(&mut mem);
        let map = s.attributes(&attrs).unwrap();
        let at = s.place(&map).unwrap();
        assert!(read_attributes(s.mem, at).is_some());
        // Claim one more entry than the ring holds.
        write_u32(s.mem, at + layout::HASH_LIST_SIZE, 3);
        assert!(
            read_attributes(s.mem, at).is_none(),
            "a truncated or over-long chain must fail closed, not return partial data"
        );
        // ...and one fewer.
        write_u32(s.mem, at + layout::HASH_LIST_SIZE, 1);
        assert!(read_attributes(s.mem, at).is_none());
        s.release();
    }

    #[test]
    fn reading_refuses_an_implausible_string_length() {
        let mut mem = ArenaMem::new();
        let mut s = Scratch::new(&mut mem);
        let obj = s.wstring("hello there").unwrap();
        let at = s.place(&obj).unwrap();
        assert!(read_wstring(s.mem, at).is_some());
        write_u32(s.mem, at + layout::WSTRING_SIZE, MAX_WSTRING_UNITS + 1);
        assert!(read_wstring(s.mem, at).is_none());
        // capacity below size is incoherent
        write_u32(s.mem, at + layout::WSTRING_SIZE, 4);
        write_u32(s.mem, at + layout::WSTRING_CAPACITY, 2);
        assert!(read_wstring(s.mem, at).is_none());
        s.release();
    }

    #[test]
    fn scratch_release_frees_every_block_it_took() {
        let mut mem = ArenaMem::new();
        let lobby = sample_lobby();
        let mut s = Scratch::new(&mut mem);
        let _ = s.lobby_ref(&lobby).unwrap();
        let _ = s.search_result_ref(&[lobby.clone(), lobby]).unwrap();
        assert!(s.live_blocks() > 10);
        s.release();
        assert_eq!(mem.live_blocks(), 0);
    }
}
