// SPDX-License-Identifier: GPL-3.0-or-later
//! Side-by-side sparse owner for retail `(who, o)` object identity.
//!
//! Dense simulation rows remain free to compact. This owner stores only a stable generational
//! identity supplied by the caller, while each retail slot retains its own object-storage and
//! optional Unit-projection identities across tombstoning and reuse. It is intentionally not
//! installed in `World` yet: current save, checksum, traversal, script lookup, and production
//! adapters all assume dense object bands.

use std::collections::BTreeMap;

pub const OWNER_SLOTS: usize = 10;
pub const BANDED_OWNER_SLOTS: usize = 8;
pub const UNIT_BAND_BASE: i32 = 0;
pub const UNIT_BAND_LIMIT: i32 = 2_000;
pub const BUILD_BAND_BASE: i32 = 2_000;
pub const BUILD_BAND_LIMIT: i32 = 3_000;
pub const WALL_BAND_BASE: i32 = 3_000;
/// Object indices are stored in a signed 16-bit field; 32,768 is the exclusive bound.
pub const WALL_BAND_LIMIT: i32 = i16::MAX as i32 + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RetailBand {
    Unit,
    Build,
    Wall,
}

impl RetailBand {
    pub const ALL: [Self; 3] = [Self::Unit, Self::Build, Self::Wall];

    pub const fn base(self) -> i32 {
        match self {
            Self::Unit => UNIT_BAND_BASE,
            Self::Build => BUILD_BAND_BASE,
            Self::Wall => WALL_BAND_BASE,
        }
    }

    pub const fn limit(self) -> i32 {
        match self {
            Self::Unit => UNIT_BAND_LIMIT,
            Self::Build => BUILD_BAND_LIMIT,
            Self::Wall => WALL_BAND_LIMIT,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Unit => 0,
            Self::Build => 1,
            Self::Wall => 2,
        }
    }

    pub const fn contains(self, o: i32) -> bool {
        o >= self.base() && o < self.limit()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RetailObjectAddress {
    pub owner: u8,
    pub band: RetailBand,
    pub o: i32,
}

impl RetailObjectAddress {
    pub const fn new(owner: u8, band: RetailBand, o: i32) -> Self {
        Self { owner, band, o }
    }

    pub const fn is_well_formed(self) -> bool {
        (self.owner as usize) < OWNER_SLOTS && self.band.contains(self.o)
    }
}

/// Structural mirror of the stable portion of `World::Handle`, independent of dense row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DenseIdentity {
    pub id: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObjectStorageId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnitProjectionId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectStorageClass {
    Unit,
    Animal,
    Build,
    Wall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParallelStorage {
    /// Analogue of `Objects::lists[owner][o]`.
    pub object: ObjectStorageId,
    pub class: ObjectStorageClass,
    /// Analogue of `Units::lists[owner][o]`; present only for the Unit band.
    pub unit: Option<UnitProjectionId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TombstoneFacts {
    pub flags: u8,
    pub hold_frames: u16,
    pub is_unit: bool,
    pub o_up: i16,
}

impl TombstoneFacts {
    pub const fn fresh_for(band: RetailBand) -> Self {
        Self {
            flags: 0,
            hold_frames: 0,
            is_unit: matches!(band, RetailBand::Unit),
            o_up: -1,
        }
    }

    /// Exact `Objects::find_free` reuse predicate at `0x0065ADB9..0x0065ADE8`.
    pub const fn is_reusable(self) -> bool {
        self.flags & 1 == 0 && self.hold_frames == 0 && (!self.is_unit || self.o_up < 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SparseSlotLifecycle<I> {
    Tombstone(TombstoneFacts),
    Reserved { ticket: u64, prior: TombstoneFacts },
    Live(I),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SparseSlot<I> {
    pub storage: ParallelStorage,
    pub lifecycle: SparseSlotLifecycle<I>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SparseBand<I> {
    mark: i32,
    slots: Vec<SparseSlot<I>>,
}

impl<I> SparseBand<I> {
    fn new(band: RetailBand) -> Self {
        Self {
            mark: band.base(),
            slots: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SparseOwner<I> {
    bands: [SparseBand<I>; 3],
}

impl<I> SparseOwner<I> {
    fn new() -> Self {
        Self {
            bands: std::array::from_fn(|index| SparseBand::new(RetailBand::ALL[index])),
        }
    }

    fn band(&self, band: RetailBand) -> &SparseBand<I> {
        &self.bands[band.index()]
    }

    fn band_mut(&mut self, band: RetailBand) -> &mut SparseBand<I> {
        &mut self.bands[band.index()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindFreeRequest {
    pub owner: u8,
    pub band: RetailBand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reservation {
    pub address: RetailObjectAddress,
    pub ticket: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageDisposition {
    ReusedTombstone,
    ExtendedRetainedStorage,
    ConstructedAndRegistered(ParallelStorage),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindFreeOutcome {
    Reserved(Reservation),
    CapacityFailure(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindFreeReceipt {
    pub request: FindFreeRequest,
    pub mark_before: i32,
    pub mark_after: i32,
    /// Number of lower indices rejected before the selected slot, or the full scanned band on
    /// capacity failure.
    pub ineligible_prefix: u32,
    pub storage: Option<StorageDisposition>,
    pub outcome: FindFreeOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitReceipt<I> {
    pub reservation: Reservation,
    pub identity: I,
    pub storage: ParallelStorage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetireReceipt<I> {
    pub address: RetailObjectAddress,
    pub identity: I,
    pub retained_storage: ParallelStorage,
    pub tombstone: TombstoneFacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkReceipt {
    pub owner: u8,
    pub band: RetailBand,
    pub before: i32,
    pub after: i32,
    pub retained_slots: usize,
}

/// Phase-1 receipt for mirroring one already-committed append in the legacy dense registry.
/// This does not scan tombstones or exercise the sparse allocation authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DenseMirrorInsertReceipt<I> {
    pub address: RetailObjectAddress,
    pub identity: I,
    pub storage: ParallelStorage,
}

/// Phase-1 receipt for mirroring legacy swap-removal. `moved` is the stable identity which
/// inherited the removed dense address, if the removed entry was not already the tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DenseMirrorRemoveReceipt<I> {
    pub removed_address: RetailObjectAddress,
    pub removed_identity: I,
    pub moved: Option<(I, RetailObjectAddress)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SparseRegistryError {
    InvalidOwner,
    InvalidObjectIndex,
    InvalidMark,
    MissingStorage,
    LiveTailBelowMark,
    ReservationMismatch,
    DuplicateIdentity,
    IdentityMismatch,
    SlotNotLive,
    SlotNotTombstone,
    OutstandingReservation,
    DenseBandGap,
    DuplicateAddress,
    DenseMirrorMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SparseObjectBands<I> {
    owners: [SparseOwner<I>; OWNER_SLOTS],
    active: [bool; OWNER_SLOTS],
    reverse: BTreeMap<I, RetailObjectAddress>,
    next_storage: u64,
    next_ticket: u64,
}

impl<I: Copy + Ord> Default for SparseObjectBands<I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: Copy + Ord> SparseObjectBands<I> {
    pub fn new() -> Self {
        Self {
            owners: std::array::from_fn(|_| SparseOwner::new()),
            active: [false; OWNER_SLOTS],
            reverse: BTreeMap::new(),
            next_storage: 1,
            next_ticket: 1,
        }
    }

    pub fn is_active(&self, owner: usize) -> Option<bool> {
        self.active.get(owner).copied()
    }

    pub fn set_active(&mut self, owner: usize, active: bool) -> Result<(), SparseRegistryError> {
        let slot = self
            .active
            .get_mut(owner)
            .ok_or(SparseRegistryError::InvalidOwner)?;
        *slot = active;
        Ok(())
    }

    pub fn mark(&self, owner: usize, band: RetailBand) -> Option<i32> {
        self.owners.get(owner).map(|owner| owner.band(band).mark)
    }

    pub fn retained_slots(&self, owner: usize, band: RetailBand) -> Option<usize> {
        self.owners
            .get(owner)
            .map(|owner| owner.band(band).slots.len())
    }

    pub fn slot(&self, address: RetailObjectAddress) -> Option<&SparseSlot<I>> {
        let index = self.slot_index(address).ok()?;
        self.owners[address.owner as usize]
            .band(address.band)
            .slots
            .get(index)
    }

    pub fn address_of(&self, identity: I) -> Option<RetailObjectAddress> {
        self.reverse.get(&identity).copied()
    }

    pub fn live_identity(&self, address: RetailObjectAddress) -> Option<I> {
        match self.slot(address)?.lifecycle {
            SparseSlotLifecycle::Live(identity) => Some(identity),
            SparseSlotLifecycle::Tombstone(_) | SparseSlotLifecycle::Reserved { .. } => None,
        }
    }

    /// Resolve through the caller's stable-identity table. Dense row compaction changes only the
    /// resolver's answer, never this registry or the retail address.
    pub fn resolve_dense_row(
        &self,
        address: RetailObjectAddress,
        resolve: impl FnOnce(I) -> Option<u32>,
    ) -> Option<u32> {
        resolve(self.live_identity(address)?)
    }

    /// Mirror a legacy gap-free append without invoking `find_free` or creating a reservation.
    ///
    /// This narrow bridge exists only for the live dual-read phase. It refuses sparse retained
    /// tails/tombstones so current dense allocation cannot silently erase future sparse state.
    pub fn mirror_dense_append(
        &mut self,
        owner: u8,
        band_kind: RetailBand,
        identity: I,
    ) -> Result<DenseMirrorInsertReceipt<I>, SparseRegistryError> {
        let owner_index = owner as usize;
        if owner_index >= OWNER_SLOTS {
            return Err(SparseRegistryError::InvalidOwner);
        }
        if self.reverse.contains_key(&identity) {
            return Err(SparseRegistryError::DuplicateIdentity);
        }
        let band = self.owners[owner_index].band(band_kind);
        if band.mark != band_kind.base() + band.slots.len() as i32 || band.mark >= band_kind.limit()
        {
            return Err(SparseRegistryError::DenseMirrorMismatch);
        }
        let address = RetailObjectAddress::new(owner, band_kind, band.mark);
        let storage = self.allocate_storage(owner, band_kind);
        let band = self.owners[owner_index].band_mut(band_kind);
        band.slots.push(SparseSlot {
            storage,
            lifecycle: SparseSlotLifecycle::Live(identity),
        });
        band.mark += 1;
        self.reverse.insert(identity, address);
        self.active[owner_index] = true;
        Ok(DenseMirrorInsertReceipt {
            address,
            identity,
            storage,
        })
    }

    /// Mirror the legacy registry's swap-remove while allocation still belongs to that registry.
    /// Sparse-native retirement must use [`Self::retire`] instead and preserve the address.
    pub fn mirror_dense_swap_remove(
        &mut self,
        address: RetailObjectAddress,
        identity: I,
    ) -> Result<DenseMirrorRemoveReceipt<I>, SparseRegistryError> {
        let index = self.slot_index(address)?;
        if self.reverse.get(&identity).copied() != Some(address) {
            return Err(SparseRegistryError::IdentityMismatch);
        }
        let owner_index = address.owner as usize;
        let band = self.owners[owner_index].band(address.band);
        if band.mark != address.band.base() + band.slots.len() as i32
            || index >= band.slots.len()
            || !matches!(band.slots[index].lifecycle, SparseSlotLifecycle::Live(value) if value == identity)
        {
            return Err(SparseRegistryError::DenseMirrorMismatch);
        }
        let last = band.slots.len() - 1;
        let moved = if index == last {
            None
        } else {
            let SparseSlotLifecycle::Live(moved_identity) = band.slots[last].lifecycle else {
                return Err(SparseRegistryError::DenseMirrorMismatch);
            };
            Some((moved_identity, address))
        };

        let band = self.owners[owner_index].band_mut(address.band);
        if let Some((moved_identity, _)) = moved {
            // Storage belongs to the retail slot. Only the live stable identity follows the
            // legacy tail entry into this address during phase 1.
            band.slots[index].lifecycle = SparseSlotLifecycle::Live(moved_identity);
        }
        band.slots.pop();
        band.mark -= 1;
        self.reverse.remove(&identity);
        if let Some((moved_identity, moved_address)) = moved {
            self.reverse.insert(moved_identity, moved_address);
        }
        Ok(DenseMirrorRemoveReceipt {
            removed_address: address,
            removed_identity: identity,
            moved,
        })
    }

    pub fn find_free(
        &mut self,
        request: FindFreeRequest,
    ) -> Result<FindFreeReceipt, SparseRegistryError> {
        let owner_index = request.owner as usize;
        if owner_index >= OWNER_SLOTS {
            return Err(SparseRegistryError::InvalidOwner);
        }
        let band = &self.owners[owner_index].bands[request.band.index()];
        let mark_before = band.mark;
        if mark_before < request.band.base() || mark_before > request.band.limit() {
            return Err(SparseRegistryError::InvalidMark);
        }
        let marked_len = (mark_before - request.band.base()) as usize;
        if marked_len > band.slots.len() {
            return Err(SparseRegistryError::MissingStorage);
        }

        let reusable = band.slots[..marked_len]
            .iter()
            .position(|slot| matches!(slot.lifecycle, SparseSlotLifecycle::Tombstone(facts) if facts.is_reusable()));
        let (slot_index, mark_after, disposition, prior) = if let Some(slot_index) = reusable {
            let slot = band.slots[slot_index];
            let SparseSlotLifecycle::Tombstone(prior) = slot.lifecycle else {
                unreachable!("position predicate fixed lifecycle")
            };
            (
                slot_index,
                mark_before,
                StorageDisposition::ReusedTombstone,
                prior,
            )
        } else if mark_before >= request.band.limit() {
            return Ok(FindFreeReceipt {
                request,
                mark_before,
                mark_after: mark_before,
                ineligible_prefix: marked_len as u32,
                storage: None,
                outcome: FindFreeOutcome::CapacityFailure(-1),
            });
        } else {
            let slot_index = marked_len;
            if slot_index < band.slots.len() {
                let slot = band.slots[slot_index];
                let SparseSlotLifecycle::Tombstone(prior) = slot.lifecycle else {
                    return Err(SparseRegistryError::LiveTailBelowMark);
                };
                (
                    slot_index,
                    mark_before + 1,
                    StorageDisposition::ExtendedRetainedStorage,
                    prior,
                )
            } else if slot_index == band.slots.len() {
                let storage = self.allocate_storage(request.owner, request.band);
                (
                    slot_index,
                    mark_before + 1,
                    StorageDisposition::ConstructedAndRegistered(storage),
                    TombstoneFacts::fresh_for(request.band),
                )
            } else {
                return Err(SparseRegistryError::MissingStorage);
            }
        };

        let ticket = self.next_ticket;
        self.next_ticket = self.next_ticket.wrapping_add(1).max(1);
        let address = RetailObjectAddress::new(
            request.owner,
            request.band,
            request.band.base() + slot_index as i32,
        );
        let owner = &mut self.owners[owner_index];
        let band = owner.band_mut(request.band);
        if let StorageDisposition::ConstructedAndRegistered(storage) = disposition {
            band.slots.push(SparseSlot {
                storage,
                lifecycle: SparseSlotLifecycle::Reserved { ticket, prior },
            });
        } else {
            band.slots[slot_index].lifecycle = SparseSlotLifecycle::Reserved { ticket, prior };
        }
        band.mark = mark_after;

        Ok(FindFreeReceipt {
            request,
            mark_before,
            mark_after,
            ineligible_prefix: slot_index as u32,
            storage: Some(disposition),
            outcome: FindFreeOutcome::Reserved(Reservation { address, ticket }),
        })
    }

    pub fn commit(
        &mut self,
        reservation: Reservation,
        identity: I,
    ) -> Result<CommitReceipt<I>, SparseRegistryError> {
        if self.reverse.contains_key(&identity) {
            return Err(SparseRegistryError::DuplicateIdentity);
        }
        let index = self.slot_index(reservation.address)?;
        let slot = self.owners[reservation.address.owner as usize]
            .band_mut(reservation.address.band)
            .slots
            .get_mut(index)
            .ok_or(SparseRegistryError::MissingStorage)?;
        match slot.lifecycle {
            SparseSlotLifecycle::Reserved { ticket, .. } if ticket == reservation.ticket => {}
            _ => return Err(SparseRegistryError::ReservationMismatch),
        }
        slot.lifecycle = SparseSlotLifecycle::Live(identity);
        let storage = slot.storage;
        self.reverse.insert(identity, reservation.address);
        self.active[reservation.address.owner as usize] = true;
        Ok(CommitReceipt {
            reservation,
            identity,
            storage,
        })
    }

    pub fn cancel(&mut self, reservation: Reservation) -> Result<(), SparseRegistryError> {
        let index = self.slot_index(reservation.address)?;
        let slot = self.owners[reservation.address.owner as usize]
            .band_mut(reservation.address.band)
            .slots
            .get_mut(index)
            .ok_or(SparseRegistryError::MissingStorage)?;
        let SparseSlotLifecycle::Reserved { ticket, prior } = slot.lifecycle else {
            return Err(SparseRegistryError::ReservationMismatch);
        };
        if ticket != reservation.ticket {
            return Err(SparseRegistryError::ReservationMismatch);
        }
        slot.lifecycle = SparseSlotLifecycle::Tombstone(prior);
        Ok(())
    }

    pub fn retire(
        &mut self,
        address: RetailObjectAddress,
        identity: I,
        tombstone: TombstoneFacts,
    ) -> Result<RetireReceipt<I>, SparseRegistryError> {
        if self.reverse.get(&identity).copied() != Some(address) {
            return Err(SparseRegistryError::IdentityMismatch);
        }
        let index = self.slot_index(address)?;
        let slot = self.owners[address.owner as usize]
            .band_mut(address.band)
            .slots
            .get_mut(index)
            .ok_or(SparseRegistryError::MissingStorage)?;
        if slot.lifecycle != SparseSlotLifecycle::Live(identity) {
            return Err(SparseRegistryError::SlotNotLive);
        }
        slot.lifecycle = SparseSlotLifecycle::Tombstone(tombstone);
        let retained_storage = slot.storage;
        self.reverse.remove(&identity);
        Ok(RetireReceipt {
            address,
            identity,
            retained_storage,
            tombstone,
        })
    }

    pub fn tick_tombstone_hold(
        &mut self,
        address: RetailObjectAddress,
    ) -> Result<u16, SparseRegistryError> {
        let index = self.slot_index(address)?;
        let slot = self.owners[address.owner as usize]
            .band_mut(address.band)
            .slots
            .get_mut(index)
            .ok_or(SparseRegistryError::MissingStorage)?;
        let SparseSlotLifecycle::Tombstone(mut facts) = slot.lifecycle else {
            return Err(SparseRegistryError::SlotNotTombstone);
        };
        facts.hold_frames = facts.hold_frames.saturating_sub(1);
        slot.lifecycle = SparseSlotLifecycle::Tombstone(facts);
        Ok(facts.hold_frames)
    }

    /// Lower only the independent high-water mark; retained object/projection storage remains.
    pub fn lower_mark(
        &mut self,
        owner: u8,
        band_kind: RetailBand,
        new_mark: i32,
    ) -> Result<MarkReceipt, SparseRegistryError> {
        let owner_index = owner as usize;
        if owner_index >= OWNER_SLOTS {
            return Err(SparseRegistryError::InvalidOwner);
        }
        let band = self.owners[owner_index].band_mut(band_kind);
        if new_mark < band_kind.base() || new_mark > band.mark {
            return Err(SparseRegistryError::InvalidMark);
        }
        let new_len = (new_mark - band_kind.base()) as usize;
        let old_len = (band.mark - band_kind.base()) as usize;
        if band.slots[new_len..old_len]
            .iter()
            .any(|slot| !matches!(slot.lifecycle, SparseSlotLifecycle::Tombstone(_)))
        {
            return Err(SparseRegistryError::LiveTailBelowMark);
        }
        let before = band.mark;
        band.mark = new_mark;
        Ok(MarkReceipt {
            owner,
            band: band_kind,
            before,
            after: new_mark,
            retained_slots: band.slots.len(),
        })
    }

    pub fn live_count(&self) -> usize {
        self.reverse.len()
    }

    pub fn total_retained_storage(&self) -> usize {
        self.owners
            .iter()
            .flat_map(|owner| owner.bands.iter())
            .map(|band| band.slots.len())
            .sum()
    }

    /// Retail traversal addresses through each mark, including tombstones whose hold count must
    /// tick. Unit owners rotate; Build and Wall visit only owners 0--7 in fixed order.
    pub fn traversal(&self, frame: i32) -> Vec<TraversalEntry<I>> {
        let mut out = Vec::new();
        for offset in 0..OWNER_SLOTS {
            let owner = frame
                .wrapping_add(offset as i32)
                .rem_euclid(OWNER_SLOTS as i32) as usize;
            if self.active[owner] {
                self.append_marked(owner, RetailBand::Unit, &mut out);
            }
        }
        for owner in 0..BANDED_OWNER_SLOTS {
            if self.active[owner] {
                self.append_marked(owner, RetailBand::Build, &mut out);
                self.append_marked(owner, RetailBand::Wall, &mut out);
            }
        }
        out
    }

    pub fn snapshot(&self) -> Result<SparseRegistrySnapshot<I>, SparseRegistryError> {
        let mut owners = Vec::with_capacity(OWNER_SLOTS);
        for owner in &self.owners {
            let mut bands = Vec::with_capacity(3);
            for band in &owner.bands {
                let mut slots = Vec::with_capacity(band.slots.len());
                for slot in &band.slots {
                    let lifecycle = match slot.lifecycle {
                        SparseSlotLifecycle::Tombstone(facts) => {
                            SnapshotLifecycle::Tombstone(facts)
                        }
                        SparseSlotLifecycle::Live(identity) => SnapshotLifecycle::Live(identity),
                        SparseSlotLifecycle::Reserved { .. } => {
                            return Err(SparseRegistryError::OutstandingReservation)
                        }
                    };
                    slots.push(lifecycle);
                }
                bands.push(SparseBandSnapshot {
                    mark: band.mark,
                    slots,
                });
            }
            owners.push(SparseOwnerSnapshot { bands });
        }
        Ok(SparseRegistrySnapshot {
            active: self.active,
            owners,
        })
    }

    /// Rebuild non-semantic storage tokens while preserving every future-affecting mark,
    /// tombstone, and stable live identity.
    pub fn from_snapshot(snapshot: SparseRegistrySnapshot<I>) -> Result<Self, SparseRegistryError> {
        if snapshot.owners.len() != OWNER_SLOTS {
            return Err(SparseRegistryError::InvalidOwner);
        }
        let mut registry = Self::new();
        registry.active = snapshot.active;
        for (owner_index, owner_snapshot) in snapshot.owners.into_iter().enumerate() {
            if owner_snapshot.bands.len() != 3 {
                return Err(SparseRegistryError::InvalidMark);
            }
            for (band_index, band_snapshot) in owner_snapshot.bands.into_iter().enumerate() {
                let band_kind = RetailBand::ALL[band_index];
                if band_snapshot.mark < band_kind.base()
                    || band_snapshot.mark > band_kind.limit()
                    || (band_snapshot.mark - band_kind.base()) as usize > band_snapshot.slots.len()
                    || band_snapshot.slots.len() > (band_kind.limit() - band_kind.base()) as usize
                {
                    return Err(SparseRegistryError::InvalidMark);
                }
                let mut slots = Vec::with_capacity(band_snapshot.slots.len());
                for (offset, lifecycle) in band_snapshot.slots.into_iter().enumerate() {
                    let storage = registry.allocate_storage(owner_index as u8, band_kind);
                    let lifecycle = match lifecycle {
                        SnapshotLifecycle::Tombstone(facts) => {
                            SparseSlotLifecycle::Tombstone(facts)
                        }
                        SnapshotLifecycle::Live(identity) => {
                            let address = RetailObjectAddress::new(
                                owner_index as u8,
                                band_kind,
                                band_kind.base() + offset as i32,
                            );
                            if registry.reverse.insert(identity, address).is_some() {
                                return Err(SparseRegistryError::DuplicateIdentity);
                            }
                            SparseSlotLifecycle::Live(identity)
                        }
                    };
                    slots.push(SparseSlot { storage, lifecycle });
                }
                registry.owners[owner_index].bands[band_index] = SparseBand {
                    mark: band_snapshot.mark,
                    slots,
                };
            }
        }
        Ok(registry)
    }

    /// Convert the current dense, gap-free band representation without retaining row numbers.
    pub fn from_dense_entries(
        active: [bool; OWNER_SLOTS],
        mut entries: Vec<DenseRegistryEntry<I>>,
    ) -> Result<(Self, DenseConversionReceipt), SparseRegistryError> {
        entries.sort_by_key(|entry| entry.address);
        let mut registry = Self::new();
        registry.active = active;
        let mut previous_address = None;
        for entry in &entries {
            if !entry.address.is_well_formed() {
                return Err(SparseRegistryError::InvalidObjectIndex);
            }
            if previous_address == Some(entry.address) {
                return Err(SparseRegistryError::DuplicateAddress);
            }
            previous_address = Some(entry.address);
            if registry.reverse.contains_key(&entry.identity) {
                return Err(SparseRegistryError::DuplicateIdentity);
            }
            let owner = entry.address.owner as usize;
            let band = registry.owners[owner].band(entry.address.band);
            if entry.address.o != band.mark {
                return Err(SparseRegistryError::DenseBandGap);
            }
            let storage = registry.allocate_storage(entry.address.owner, entry.address.band);
            let band = registry.owners[owner].band_mut(entry.address.band);
            band.slots.push(SparseSlot {
                storage,
                lifecycle: SparseSlotLifecycle::Live(entry.identity),
            });
            band.mark += 1;
            registry.reverse.insert(entry.identity, entry.address);
        }
        let marks = std::array::from_fn(|owner| registry.owner_marks(owner));
        Ok((
            registry,
            DenseConversionReceipt {
                converted_live_entries: entries.len(),
                marks,
                stores_stable_identity_not_row: true,
            },
        ))
    }

    fn owner_marks(&self, owner: usize) -> OwnerMarks {
        OwnerMarks {
            unit: self.owners[owner].band(RetailBand::Unit).mark,
            build: self.owners[owner].band(RetailBand::Build).mark,
            wall: self.owners[owner].band(RetailBand::Wall).mark,
        }
    }

    fn slot_index(&self, address: RetailObjectAddress) -> Result<usize, SparseRegistryError> {
        if address.owner as usize >= OWNER_SLOTS {
            return Err(SparseRegistryError::InvalidOwner);
        }
        if !address.band.contains(address.o) {
            return Err(SparseRegistryError::InvalidObjectIndex);
        }
        Ok((address.o - address.band.base()) as usize)
    }

    fn allocate_storage(&mut self, owner: u8, band: RetailBand) -> ParallelStorage {
        let object = ObjectStorageId(self.next_storage);
        self.next_storage = self.next_storage.wrapping_add(1).max(1);
        let (class, unit) = match band {
            RetailBand::Unit => {
                let unit = UnitProjectionId(self.next_storage);
                self.next_storage = self.next_storage.wrapping_add(1).max(1);
                let class = if owner as usize >= BANDED_OWNER_SLOTS {
                    ObjectStorageClass::Animal
                } else {
                    ObjectStorageClass::Unit
                };
                (class, Some(unit))
            }
            RetailBand::Build => (ObjectStorageClass::Build, None),
            RetailBand::Wall => (ObjectStorageClass::Wall, None),
        };
        ParallelStorage {
            object,
            class,
            unit,
        }
    }

    fn append_marked(&self, owner: usize, band_kind: RetailBand, out: &mut Vec<TraversalEntry<I>>) {
        let band = self.owners[owner].band(band_kind);
        let marked_len = (band.mark - band_kind.base()) as usize;
        for (offset, slot) in band.slots[..marked_len].iter().enumerate() {
            out.push(TraversalEntry {
                address: RetailObjectAddress::new(
                    owner as u8,
                    band_kind,
                    band_kind.base() + offset as i32,
                ),
                storage: slot.storage,
                lifecycle: slot.lifecycle,
            });
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraversalEntry<I> {
    pub address: RetailObjectAddress,
    pub storage: ParallelStorage,
    pub lifecycle: SparseSlotLifecycle<I>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotLifecycle<I> {
    Tombstone(TombstoneFacts),
    Live(I),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SparseBandSnapshot<I> {
    pub mark: i32,
    pub slots: Vec<SnapshotLifecycle<I>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SparseOwnerSnapshot<I> {
    pub bands: Vec<SparseBandSnapshot<I>>,
}

/// Save/checksum-shaped state. Storage tokens and reservation tickets are deliberately absent:
/// raw pointer identity is not retail simulation state, while marks/tombstones/live identities
/// determine future allocation and therefore are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SparseRegistrySnapshot<I> {
    pub active: [bool; OWNER_SLOTS],
    pub owners: Vec<SparseOwnerSnapshot<I>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DenseRegistryEntry<I> {
    pub address: RetailObjectAddress,
    pub identity: I,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct OwnerMarks {
    pub unit: i32,
    pub build: i32,
    pub wall: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DenseConversionReceipt {
    pub converted_live_entries: usize,
    pub marks: [OwnerMarks; OWNER_SLOTS],
    pub stores_stable_identity_not_row: bool,
}
