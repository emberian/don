//! DoNSave v13 leaf for the receive-side Group selection cache.
//!
//! The retail globals are eight fixed rows of at most 128 `(i16 o,u16 uid)` entries. Handles,
//! package scratch, serials, and transaction revisions are deliberately not persisted.

use super::{Reader, SaveError, Writer};
use crate::systems::canonical_group_move_host::{
    CachedSelection, CommandPackageState, NETWORK_PLAYERS, RECEIVED_SELECTION_CAPACITY,
};

const LEAF_VERSION: u8 = 1;

pub(super) fn write(state: &CommandPackageState) -> Result<Vec<u8>, SaveError> {
    let selections = state.saved_selections();
    let mut writer = Writer::default();
    writer.u8(LEAF_VERSION);
    for row in &selections {
        if row.len() > RECEIVED_SELECTION_CAPACITY {
            return Err(SaveError::Limit("command selection cache"));
        }
        if row.iter().any(|entry| entry.o < 0) {
            return Err(SaveError::Invalid("negative cached command object"));
        }
        writer.u16(row.len() as u16);
        for entry in row {
            writer.i16(entry.o);
            writer.u16(entry.uid);
        }
    }
    Ok(writer.0)
}

pub(super) fn read(data: &[u8]) -> Result<CommandPackageState, SaveError> {
    let mut reader = Reader::new(data);
    if reader.u8()? != LEAF_VERSION {
        return Err(SaveError::Invalid("command package state version"));
    }
    let mut selections: [Vec<CachedSelection>; NETWORK_PLAYERS] =
        std::array::from_fn(|_| Vec::new());
    for row in &mut selections {
        let count = usize::from(reader.u16()?);
        if count > RECEIVED_SELECTION_CAPACITY {
            return Err(SaveError::Limit("command selection cache"));
        }
        row.reserve(count);
        for _ in 0..count {
            let o = reader.i16()?;
            if o < 0 {
                return Err(SaveError::Invalid("negative cached command object"));
            }
            row.push(CachedSelection {
                o,
                uid: reader.u16()?,
            });
        }
    }
    reader.finish()?;
    CommandPackageState::from_saved_selections(selections)
        .map_err(|_| SaveError::Invalid("command selection cache"))
}
