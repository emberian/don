#[path = "../src/systems/sparse_object_bands_authority_frontier.rs"]
mod frontier;

use frontier::*;

fn identity(id: u32) -> DenseIdentity {
    DenseIdentity { id, generation: 0 }
}

fn reserve(
    registry: &mut SparseObjectBands<DenseIdentity>,
    owner: u8,
    band: RetailBand,
) -> FindFreeReceipt {
    registry.find_free(FindFreeRequest { owner, band }).unwrap()
}

fn commit(
    registry: &mut SparseObjectBands<DenseIdentity>,
    owner: u8,
    band: RetailBand,
    identity: DenseIdentity,
) -> CommitReceipt<DenseIdentity> {
    let receipt = reserve(registry, owner, band);
    let FindFreeOutcome::Reserved(reservation) = receipt.outcome else {
        panic!("expected reservation")
    };
    registry.commit(reservation, identity).unwrap()
}

fn tombstone(hold_frames: u16, o_up: i16) -> TombstoneFacts {
    TombstoneFacts {
        flags: 0,
        hold_frames,
        is_unit: true,
        o_up,
    }
}

#[test]
fn band_ranges_and_ten_owner_identity_are_frozen() {
    assert_eq!(OWNER_SLOTS, 10);
    assert_eq!(BANDED_OWNER_SLOTS, 8);
    assert_eq!(
        (RetailBand::Unit.base(), RetailBand::Unit.limit()),
        (0, 2_000)
    );
    assert_eq!(
        (RetailBand::Build.base(), RetailBand::Build.limit()),
        (2_000, 3_000)
    );
    assert_eq!(
        (RetailBand::Wall.base(), RetailBand::Wall.limit()),
        (3_000, 32_768)
    );
    assert!(RetailObjectAddress::new(9, RetailBand::Unit, 1_999).is_well_formed());
    assert!(!RetailObjectAddress::new(10, RetailBand::Unit, 0).is_well_formed());
    assert!(!RetailObjectAddress::new(0, RetailBand::Unit, 2_000).is_well_formed());
}

#[test]
fn every_owner_and_band_has_an_independent_high_water_mark() {
    let mut registry = SparseObjectBands::new();
    for owner in 0..OWNER_SLOTS as u8 {
        commit(
            &mut registry,
            owner,
            RetailBand::Unit,
            identity(owner as u32),
        );
    }
    commit(&mut registry, 3, RetailBand::Build, identity(100));
    commit(&mut registry, 3, RetailBand::Wall, identity(101));
    assert_eq!(registry.live_count(), OWNER_SLOTS + 2);
    assert_eq!(registry.total_retained_storage(), OWNER_SLOTS + 2);

    for owner in 0..OWNER_SLOTS {
        assert_eq!(registry.is_active(owner), Some(true));
        assert_eq!(registry.mark(owner, RetailBand::Unit), Some(1));
        assert_eq!(
            registry.mark(owner, RetailBand::Build),
            Some(if owner == 3 { 2_001 } else { 2_000 })
        );
        assert_eq!(
            registry.mark(owner, RetailBand::Wall),
            Some(if owner == 3 { 3_001 } else { 3_000 })
        );
    }
}

#[test]
fn unit_construction_registers_parallel_projection_and_owner_class_split() {
    let mut registry = SparseObjectBands::new();
    let player = commit(&mut registry, 7, RetailBand::Unit, identity(1));
    let animal = commit(&mut registry, 8, RetailBand::Unit, identity(2));
    let build = commit(&mut registry, 7, RetailBand::Build, identity(3));

    assert_eq!(player.storage.class, ObjectStorageClass::Unit);
    assert!(player.storage.unit.is_some());
    assert_eq!(animal.storage.class, ObjectStorageClass::Animal);
    assert!(animal.storage.unit.is_some());
    assert_eq!(build.storage.class, ObjectStorageClass::Build);
    assert_eq!(build.storage.unit, None);
    assert_ne!(player.storage.object, animal.storage.object);
}

#[test]
fn find_free_reuses_lowest_eligible_tombstone_without_advancing_mark() {
    let mut registry = SparseObjectBands::new();
    let first = commit(&mut registry, 2, RetailBand::Unit, identity(10));
    let second = commit(&mut registry, 2, RetailBand::Unit, identity(11));
    let first_address = first.reservation.address;
    let second_address = second.reservation.address;
    registry
        .retire(first_address, identity(10), tombstone(0, -1))
        .unwrap();
    registry
        .retire(second_address, identity(11), tombstone(0, -1))
        .unwrap();

    let receipt = reserve(&mut registry, 2, RetailBand::Unit);
    assert_eq!((receipt.mark_before, receipt.mark_after), (2, 2));
    assert_eq!(receipt.ineligible_prefix, 0);
    assert_eq!(receipt.storage, Some(StorageDisposition::ReusedTombstone));
    let FindFreeOutcome::Reserved(reservation) = receipt.outcome else {
        unreachable!()
    };
    assert_eq!(reservation.address, first_address);
    let recommit = registry.commit(reservation, identity(12)).unwrap();
    assert_eq!(
        recommit.storage, first.storage,
        "slot storage must survive reuse"
    );
    assert_eq!(registry.address_of(identity(12)), Some(first_address));
}

#[test]
fn hold_frames_and_subordinate_link_block_reuse_until_released() {
    let mut registry = SparseObjectBands::new();
    let held = commit(&mut registry, 0, RetailBand::Unit, identity(1));
    let subordinate = commit(&mut registry, 0, RetailBand::Unit, identity(2));
    registry
        .retire(held.reservation.address, identity(1), tombstone(2, -1))
        .unwrap();
    registry
        .retire(
            subordinate.reservation.address,
            identity(2),
            tombstone(0, 7),
        )
        .unwrap();

    let extension = reserve(&mut registry, 0, RetailBand::Unit);
    assert_eq!((extension.mark_before, extension.mark_after), (2, 3));
    assert_eq!(extension.ineligible_prefix, 2);
    let FindFreeOutcome::Reserved(extension_reservation) = extension.outcome else {
        unreachable!()
    };
    registry.cancel(extension_reservation).unwrap();

    assert_eq!(
        registry.tick_tombstone_hold(held.reservation.address),
        Ok(1)
    );
    assert_eq!(
        registry.tick_tombstone_hold(held.reservation.address),
        Ok(0)
    );
    let reuse = reserve(&mut registry, 0, RetailBand::Unit);
    let FindFreeOutcome::Reserved(reuse) = reuse.outcome else {
        unreachable!()
    };
    assert_eq!(reuse.address, held.reservation.address);
}

#[test]
fn lowering_mark_keeps_tombstone_storage_for_later_extension() {
    let mut registry = SparseObjectBands::new();
    let first = commit(&mut registry, 1, RetailBand::Unit, identity(1));
    let second = commit(&mut registry, 1, RetailBand::Unit, identity(2));
    registry
        .retire(second.reservation.address, identity(2), tombstone(0, 5))
        .unwrap();
    let retained = registry.slot(second.reservation.address).unwrap().storage;
    let mark = registry.lower_mark(1, RetailBand::Unit, 1).unwrap();
    assert_eq!((mark.before, mark.after, mark.retained_slots), (2, 1, 2));
    assert_eq!(
        registry.live_identity(first.reservation.address),
        Some(identity(1))
    );

    let receipt = reserve(&mut registry, 1, RetailBand::Unit);
    assert_eq!(
        receipt.storage,
        Some(StorageDisposition::ExtendedRetainedStorage)
    );
    let FindFreeOutcome::Reserved(reservation) = receipt.outcome else {
        unreachable!()
    };
    assert_eq!(reservation.address, second.reservation.address);
    assert_eq!(
        registry.commit(reservation, identity(3)).unwrap().storage,
        retained
    );
}

#[test]
fn retail_address_survives_dense_row_compaction_without_registry_repoint() {
    let mut registry = SparseObjectBands::new();
    let stable = DenseIdentity {
        id: 7,
        generation: 4,
    };
    let committed = commit(&mut registry, 4, RetailBand::Unit, stable);
    let address = committed.reservation.address;

    let before_row = registry.resolve_dense_row(address, |id| (id == stable).then_some(19));
    let after_row = registry.resolve_dense_row(address, |id| (id == stable).then_some(3));
    assert_eq!((before_row, after_row), (Some(19), Some(3)));
    assert_eq!(registry.live_identity(address), Some(stable));
    assert_eq!(registry.address_of(stable), Some(address));
}

#[test]
fn outstanding_reservation_is_not_saveable_but_cancel_retains_find_free_effects() {
    let mut registry = SparseObjectBands::new();
    let receipt = reserve(&mut registry, 0, RetailBand::Unit);
    let FindFreeOutcome::Reserved(reservation) = receipt.outcome else {
        unreachable!()
    };
    assert_eq!(
        registry.snapshot(),
        Err(SparseRegistryError::OutstandingReservation)
    );
    registry.cancel(reservation).unwrap();
    assert_eq!(registry.mark(0, RetailBand::Unit), Some(1));
    assert_eq!(registry.retained_slots(0, RetailBand::Unit), Some(1));
    assert!(registry.snapshot().is_ok());
}

#[test]
fn snapshot_roundtrip_preserves_future_allocation_but_rebuilds_pointer_tokens() {
    let mut registry = SparseObjectBands::new();
    let first = commit(&mut registry, 2, RetailBand::Unit, identity(1));
    let second = commit(&mut registry, 2, RetailBand::Unit, identity(2));
    registry
        .retire(first.reservation.address, identity(1), tombstone(4, -1))
        .unwrap();
    registry
        .retire(second.reservation.address, identity(2), tombstone(0, -1))
        .unwrap();
    registry.lower_mark(2, RetailBand::Unit, 1).unwrap();
    let snapshot = registry.snapshot().unwrap();
    let rebuilt = SparseObjectBands::from_snapshot(snapshot.clone()).unwrap();
    assert_eq!(rebuilt.snapshot().unwrap(), snapshot);
    assert_eq!(rebuilt.mark(2, RetailBand::Unit), Some(1));
    assert_eq!(rebuilt.retained_slots(2, RetailBand::Unit), Some(2));
    assert_eq!(rebuilt.live_count(), 0);
}

#[test]
fn dense_conversion_binds_stable_identities_and_rejects_gaps() {
    let entries = vec![
        DenseRegistryEntry {
            address: RetailObjectAddress::new(2, RetailBand::Unit, 1),
            identity: identity(11),
        },
        DenseRegistryEntry {
            address: RetailObjectAddress::new(2, RetailBand::Unit, 0),
            identity: identity(10),
        },
        DenseRegistryEntry {
            address: RetailObjectAddress::new(2, RetailBand::Build, 2_000),
            identity: identity(12),
        },
    ];
    let (registry, receipt) =
        SparseObjectBands::from_dense_entries([true; OWNER_SLOTS], entries).unwrap();
    assert_eq!(receipt.converted_live_entries, 3);
    assert!(receipt.stores_stable_identity_not_row);
    assert_eq!(receipt.marks[2].unit, 2);
    assert_eq!(receipt.marks[2].build, 2_001);
    assert_eq!(registry.address_of(identity(11)).unwrap().o, 1);

    let gap = vec![DenseRegistryEntry {
        address: RetailObjectAddress::new(0, RetailBand::Unit, 1),
        identity: identity(1),
    }];
    assert_eq!(
        SparseObjectBands::from_dense_entries([false; OWNER_SLOTS], gap),
        Err(SparseRegistryError::DenseBandGap)
    );
}

#[test]
fn traversal_rotates_units_including_tombstones_and_skips_nature_builds() {
    let mut registry = SparseObjectBands::new();
    for owner in 0..OWNER_SLOTS as u8 {
        commit(
            &mut registry,
            owner,
            RetailBand::Unit,
            identity(owner as u32),
        );
        registry.set_active(owner as usize, true).unwrap();
    }
    let tombstoned = registry.address_of(identity(3)).unwrap();
    registry
        .retire(tombstoned, identity(3), tombstone(2, -1))
        .unwrap();
    commit(&mut registry, 8, RetailBand::Build, identity(100));

    let traversal = registry.traversal(3);
    let mut retained = Vec::with_capacity(64);
    registry.traversal_into(3, &mut retained);
    assert_eq!(retained, traversal);
    let unit_owners: Vec<u8> = traversal
        .iter()
        .filter(|entry| entry.address.band == RetailBand::Unit)
        .map(|entry| entry.address.owner)
        .collect();
    assert_eq!(unit_owners, [3, 4, 5, 6, 7, 8, 9, 0, 1, 2]);
    assert!(matches!(
        traversal[0].lifecycle,
        SparseSlotLifecycle::Tombstone(_)
    ));
    assert!(!traversal
        .iter()
        .any(|entry| { entry.address.owner >= 8 && entry.address.band != RetailBand::Unit }));
}

#[test]
fn full_unit_band_reports_the_native_capacity_minus_one() {
    let mut slots = Vec::with_capacity(UNIT_BAND_LIMIT as usize);
    for _ in UNIT_BAND_BASE..UNIT_BAND_LIMIT {
        slots.push(SnapshotLifecycle::Tombstone(TombstoneFacts {
            flags: 0,
            hold_frames: 1,
            is_unit: true,
            o_up: -1,
        }));
    }
    let mut owners: Vec<SparseOwnerSnapshot<DenseIdentity>> = (0..OWNER_SLOTS)
        .map(|_| SparseOwnerSnapshot {
            bands: vec![
                SparseBandSnapshot {
                    mark: UNIT_BAND_BASE,
                    slots: Vec::new(),
                },
                SparseBandSnapshot {
                    mark: BUILD_BAND_BASE,
                    slots: Vec::new(),
                },
                SparseBandSnapshot {
                    mark: WALL_BAND_BASE,
                    slots: Vec::new(),
                },
            ],
        })
        .collect();
    owners[0].bands[0] = SparseBandSnapshot {
        mark: UNIT_BAND_LIMIT,
        slots,
    };
    let mut registry = SparseObjectBands::from_snapshot(SparseRegistrySnapshot {
        active: [true; OWNER_SLOTS],
        owners,
    })
    .unwrap();
    let receipt = reserve(&mut registry, 0, RetailBand::Unit);
    assert_eq!(receipt.mark_after, UNIT_BAND_LIMIT);
    assert_eq!(receipt.ineligible_prefix, UNIT_BAND_LIMIT as u32);
    assert_eq!(receipt.storage, None);
    assert_eq!(receipt.outcome, FindFreeOutcome::CapacityFailure(-1));
}

#[test]
fn dense_phase_mirror_appends_and_swap_removes_without_sparse_allocation() {
    let mut registry = SparseObjectBands::new();
    let first = registry
        .mirror_dense_append(2, RetailBand::Unit, identity(1))
        .unwrap();
    let second = registry
        .mirror_dense_append(2, RetailBand::Unit, identity(2))
        .unwrap();
    assert_eq!(first.address.o, 0);
    assert_eq!(second.address.o, 1);

    let removed = registry
        .mirror_dense_swap_remove(first.address, identity(1))
        .unwrap();
    assert_eq!(removed.moved, Some((identity(2), first.address)));
    assert_eq!(registry.address_of(identity(2)), Some(first.address));
    assert_eq!(registry.slot(first.address).unwrap().storage, first.storage);
    assert_eq!(registry.mark(2, RetailBand::Unit), Some(1));

    let appended = registry
        .mirror_dense_append(2, RetailBand::Unit, identity(3))
        .unwrap();
    assert_eq!(appended.address.o, 1);
    assert_eq!(registry.mark(2, RetailBand::Unit), Some(2));
}
