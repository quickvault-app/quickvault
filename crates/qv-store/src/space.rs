// SPDX-License-Identifier: AGPL-3.0-or-later

//! Space accounting (0007 §3.3, 0004 §1.4; plan 0060): the volume's free
//! space, the vault file's free pages, and the room live and snapshot-only
//! data may use together.
//!
//! The room is the lesser of the file cap ([`MAX_DB_BYTES`]) and what the
//! file may grow to with the volume's free space: QuickVault reserves none
//! of the user's disk (owner, 2026-11-15), so snapshots give way only when
//! the volume is actually full. A commit fits when live plus snapshot-only
//! data, plus 1/64 of that sum for SQLite's own pages and [`ALLOWANCE`], is
//! within the room.

use std::collections::BTreeSet;
use std::path::Path;

use qv_model::{ObjectId, RandomId};
use rusqlite::Connection;

use crate::error::StoreError;
use crate::limits::{MAX_DB_BYTES, PAGE_SIZE};
use crate::schema;
use crate::snapshot_index::SnapshotIndex;

/// Fixed allowance beside the 1/64 overhead share (plan 0060).
pub(crate) const ALLOWANCE: u64 = 64 * 1024 * 1024;

/// What a compaction leaves free on the volume at least (0004 §1.4).
pub(crate) const COMPACT_SPARE: u64 = 256 * 1024 * 1024;

/// A volume's space as this user sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DiskSpace {
    /// Bytes this user may still write.
    pub(crate) free: u64,
}

#[cfg(test)]
thread_local! {
    /// Test hook: the volume space every measurement reports
    /// (`Some(None)`: unmeasurable).
    pub(crate) static DISK: std::cell::Cell<Option<Option<DiskSpace>>> =
        const { std::cell::Cell::new(None) };
    /// Test hook: the room every commit measures.
    pub(crate) static ROOM: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// The space on the volume holding the directory `dir`; `None` when it
/// cannot be measured.
pub(crate) fn disk_space(dir: &Path) -> Option<DiskSpace> {
    #[cfg(test)]
    if let Some(forced) = DISK.with(std::cell::Cell::get) {
        return forced;
    }
    measure(dir)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn measure(dir: &Path) -> Option<DiskSpace> {
    let st = rustix::fs::statvfs(dir).ok()?;
    Some(DiskSpace {
        free: st.f_bavail.saturating_mul(st.f_frsize),
    })
}

#[cfg(windows)]
fn measure(dir: &Path) -> Option<DiskSpace> {
    let (free, _total) = qv_win::fs::disk_space(dir).ok()?;
    Some(DiskSpace { free })
}

/// The room live and snapshot-only data may use together, for a file of
/// `file_bytes` on a volume with `disk` space.
pub(crate) fn room(file_bytes: u64, disk: Option<DiskSpace>) -> u64 {
    match disk {
        Some(d) => MAX_DB_BYTES.min(file_bytes.saturating_add(d.free)),
        None => MAX_DB_BYTES,
    }
}

/// The room for a commit to the vault file at `path`: its size on disk now
/// and its volume's free space, which the same writes have changed alike.
pub(crate) fn commit_room(path: &Path) -> u64 {
    #[cfg(test)]
    if let Some(forced) = ROOM.with(std::cell::Cell::get) {
        return forced;
    }
    let file_bytes = std::fs::metadata(path).map_or(0, |m| m.len());
    room(file_bytes, disk_space(crate::fs::parent_of(path)))
}

/// Whether `live` and `snapshots` bytes fit `room` with the overhead.
pub(crate) fn fits(live: u64, snapshots: u64, room: u64) -> bool {
    let sum = live.saturating_add(snapshots);
    sum.saturating_add(sum / 64).saturating_add(ALLOWANCE) <= room
}

/// The most snapshot-only bytes that fit beside `live` (0 if none).
pub(crate) fn available(live: u64, room: u64) -> u64 {
    // fits(live, s) ⇔ t + t/64 + ALLOWANCE ≤ room for t = live + s.
    let mut total = room.saturating_sub(ALLOWANCE) / 65 * 64;
    while total > 0 && !fits(total, 0, room) {
        total -= 1;
    }
    while fits(total.saturating_add(1), 0, room) {
        total += 1;
    }
    total.saturating_sub(live)
}

/// The file's size and its free pages' bytes.
pub(crate) fn pages(conn: &Connection) -> Result<(u64, u64), StoreError> {
    let read = |name: &str| -> Result<u64, StoreError> {
        let v: i64 = conn
            .pragma_query_value(None, name, |r| r.get(0))
            .map_err(|e| schema::validation_err(&e))?;
        u64::try_from(v).map_err(|_| StoreError::Integrity)
    };
    let pages = read("page_count")?;
    let free = read("freelist_count")?;
    Ok((
        pages.saturating_mul(PAGE_SIZE),
        free.min(pages).saturating_mul(PAGE_SIZE),
    ))
}

/// Snapshot-only bytes beside the live manifest, from stored lengths alone:
/// retained objects the live manifest does not reference, plus kept manifest
/// rows other than the live one. Nothing is decrypted.
pub(crate) fn snapshot_only_bytes(
    conn: &Connection,
    index: &SnapshotIndex,
    live: &BTreeSet<ObjectId>,
    live_hash: &[u8; 32],
    generation: qv_crypto::KeyGeneration,
) -> Result<u64, StoreError> {
    let e = |e: rusqlite::Error| schema::validation_err(&e);
    let mut total = 0u64;
    let mut object = conn
        .prepare(
            "SELECT octet_length(ciphertext) FROM object \
             WHERE key_generation = ?1 AND object_id = ?2",
        )
        .map_err(e)?;
    for id in index.retained().iter().filter(|id| !live.contains(id)) {
        let len: Option<i64> = object
            .query_row(
                rusqlite::params![i64::from(generation.0), id.to_bytes().as_slice()],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                err => Err(e(err)),
            })?;
        // A retained object should be stored, but unlock checks only the
        // live manifest's objects. A missing one takes no room here, so a
        // damaged snapshot never blocks every later commit, including the
        // one that deletes it; `verify` reports it (review of plan 0059).
        let Some(len) = len else { continue };
        total = total.saturating_add(u64::try_from(len).map_err(|_| StoreError::Integrity)?);
    }
    let named: BTreeSet<[u8; 32]> = index
        .rows()
        .iter()
        .map(|r| r.manifest_hash)
        .filter(|h| h != live_hash)
        .collect();
    let mut manifest = conn
        .prepare("SELECT octet_length(ciphertext) FROM manifest WHERE manifest_hash = ?1")
        .map_err(e)?;
    for hash in named {
        let len: i64 = manifest
            .query_row([hash.as_slice()], |r| r.get(0))
            .map_err(e)?;
        total = total.saturating_add(u64::try_from(len).map_err(|_| StoreError::Integrity)?);
    }
    Ok(total)
}

/// The vault file's space (0009 §1.8 Storage, 0011 `compact`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileSpace {
    /// The vault file's size.
    pub file_bytes: u64,
    /// The free pages inside it, which compaction returns to the volume.
    pub free_bytes: u64,
    /// What snapshots keep beside the live vault.
    pub snapshot_bytes: u64,
    /// Free space on the volume; `None` when it cannot be measured.
    pub disk_free: Option<u64>,
}

impl crate::UnlockedVault {
    /// The vault file's size and its free pages' bytes: two page counts,
    /// for deciding whether to compact (0004 §1.4).
    pub fn file_pages(&self) -> Result<(u64, u64), StoreError> {
        self.check_identity()?;
        pages(&self.file.conn)
    }

    /// The vault file's size, its free pages, the space snapshots use and
    /// the volume's free space.
    pub fn file_space(&self) -> Result<FileSpace, StoreError> {
        self.begin_read()?;
        let result = (|| {
            let (file_bytes, free_bytes) = pages(&self.file.conn)?;
            let snapshot_bytes = match &self.state.snapshots {
                Some(index) => {
                    let live: BTreeSet<ObjectId> = self
                        .state
                        .manifest
                        .references()
                        .iter()
                        .map(|r| r.object_id())
                        .collect();
                    snapshot_only_bytes(
                        &self.file.conn,
                        index,
                        &live,
                        &self.state.head.manifest_hash,
                        self.file.header.key_generation,
                    )?
                }
                None => 0,
            };
            Ok(FileSpace {
                file_bytes,
                free_bytes,
                snapshot_bytes,
                disk_free: disk_space(crate::fs::parent_of(self.file.path())).map(|d| d.free),
            })
        })();
        match result {
            Ok(out) => {
                self.end_read()?;
                Ok(out)
            }
            Err(e) => {
                self.abort_read();
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn space_room_is_the_file_cap_or_all_free_space() {
        assert_eq!(room(GIB, None), MAX_DB_BYTES);
        let big = DiskSpace { free: 500 * GIB };
        assert_eq!(room(GIB, Some(big)), MAX_DB_BYTES);
        // No share of the disk is held back: a nearly full disk still
        // leaves all its free space.
        let nearly_full = DiskSpace { free: 10 * GIB };
        assert_eq!(room(GIB, Some(nearly_full)), 11 * GIB);
        // A full disk leaves only what the file already holds.
        let full = DiskSpace { free: 0 };
        assert_eq!(room(4 * GIB, Some(full)), 4 * GIB);
    }

    #[test]
    fn space_available_is_the_largest_fitting_amount() {
        for (live, room) in [(0, 4 * GIB), (GIB, 4 * GIB), (GIB, MAX_DB_BYTES), (0, 0)] {
            let a = available(live, room);
            if a > 0 {
                assert!(fits(live, a, room));
            }
            assert!(!fits(live, a + 1, room) || room == 0);
        }
        assert_eq!(available(5 * GIB, 4 * GIB), 0);
        assert!(!fits(0, 0, ALLOWANCE - 1));
        assert!(fits(0, 0, ALLOWANCE));
    }
}
