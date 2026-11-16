// SPDX-License-Identifier: AGPL-3.0-or-later

//! Compaction: rewriting the vault file without its free pages (0004 §1.4;
//! plan 0060).
//!
//! SQLite reuses freed pages but never returns them to the volume.
//! [`UnlockedVault::compact`] writes a copy with `VACUUM INTO` to the
//! private, locked temporary sibling that new vaults use, verifies it
//! against the open file, and moves it over the vault path. Both files hold
//! the same complete state with the same head, so a crash anywhere leaves a
//! valid vault and the external anchor needs no change. A copy left behind
//! by a crash is a stale temporary sibling, removed by the next open or
//! compaction.

use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

use rusqlite::Connection;
use rusqlite::limits::Limit;

use crate::error::{StoreError, sqlite_err, write_err};
use crate::fs;
use crate::space;
use crate::vault::{UnlockedVault, VaultFile, WriteGate};

/// What a compaction did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Compaction {
    /// The file's size before.
    pub before: u64,
    /// Its size after.
    pub after: u64,
}

#[cfg(test)]
thread_local! {
    /// Test hook: 1 stops (as a crash would) after the verified copy exists,
    /// before the move; 2 stops right after the move; 3 changes the copy
    /// before it is verified; 4 interrupts the running copy; 5 changes one
    /// ciphertext byte without changing its length; 6 checks Windows
    /// exclusion after SQLite closes but while both file locks remain held.
    pub(crate) static FAULT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn fault(point: u8) -> bool {
    FAULT.with(std::cell::Cell::get) == point
}

impl UnlockedVault {
    /// Rewrites the vault file without its free pages (0004 §1.4). `None`
    /// when no page is free. [`StoreError::StorageCapacity`] when the volume
    /// lacks room for the copy and 256 MiB to spare; the vault is unchanged
    /// on every error before the move.
    pub fn compact(
        &mut self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<Compaction>, StoreError> {
        if let WriteGate::Blocked(e) = self.gate {
            return Err(e);
        }
        self.finish_pending_anchor()?;
        // The head and replica on disk are still the ones this session holds.
        self.begin_read()?;
        let measured = space::pages(&self.file.conn);
        self.end_read()?;
        let (before, free) = measured?;
        if free == 0 {
            return Ok(None);
        }
        let path = self.file.path().to_owned();
        let parent = fs::parent_of(&path).to_owned();
        if let Some(disk) = space::disk_space(&parent)
            && disk.free < (before - free).saturating_add(space::COMPACT_SPARE)
        {
            return Err(StoreError::StorageCapacity);
        }
        if cancelled() {
            return Err(StoreError::Io);
        }
        fs::remove_stale_temps(&path)?;
        let (lock, temp) = fs::create_temp(&path)?;
        let result =
            self.write_copy_cancellable(&temp, cancelled)
                .and_then(|()| {
                    lock.sync()?;
                    #[cfg(test)]
                    if fault(3) {
                        let c = Connection::open(&temp).map_err(|e| sqlite_err(&e))?;
                        c.execute("UPDATE replica SET next_counter = next_counter + 1", [])
                            .map_err(|e| sqlite_err(&e))?;
                    }
                    #[cfg(test)]
                    if fault(5) {
                        let c = Connection::open(&temp).map_err(|e| sqlite_err(&e))?;
                        let (id, mut ct): (Vec<u8>, Vec<u8>) = c.query_row(
                    "SELECT object_id, ciphertext FROM object ORDER BY object_id LIMIT 1", [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                ).map_err(|e| sqlite_err(&e))?;
                        ct[0] ^= 1;
                        c.execute(
                            "UPDATE object SET ciphertext = ?1 WHERE object_id = ?2",
                            rusqlite::params![ct, id],
                        )
                        .map_err(|e| sqlite_err(&e))?;
                    }
                    let copy = VaultFile::open_locked(&temp, lock)?;
                    self.check_copy(&copy, cancelled)?;
                    Ok(copy)
                });
        let copy = match result {
            Ok(copy) => copy,
            Err(e) => {
                discard(&temp);
                return Err(e);
            }
        };
        #[cfg(test)]
        if fault(1) {
            return Err(StoreError::Io);
        }
        if cancelled() {
            drop(copy);
            discard(&temp);
            return Err(StoreError::Io);
        }
        self.install(&temp, &path, copy)?;
        let (after, _) = space::pages(&self.file.conn)?;
        Ok(Some(Compaction { before, after }))
    }

    /// SQLite requires a 'static progress callback. The caller keeps polling
    /// its borrowed cancellation check while a scoped worker runs the copy;
    /// only an atomic flag crosses into the callback. Joining the worker and
    /// disarming the callback precede every return, including interruption.
    fn write_copy_cancellable(
        &mut self,
        temp: &Path,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), StoreError> {
        let interrupted = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&interrupted);
        #[cfg(test)]
        let force = fault(4);
        let armed = self.file.budget.interrupt_when(Arc::new(move || {
            #[cfg(test)]
            if force {
                return true;
            }
            signal.load(Ordering::SeqCst)
        }));
        let result = std::thread::scope(|scope| {
            let (send, receive) = mpsc::sync_channel(1);
            let worker = std::thread::Builder::new()
                .name("qv-compact".into())
                .spawn_scoped(scope, move || {
                    let _ = send.send(self.write_copy(temp));
                })
                .map_err(crate::error::io_err)?;
            let result = loop {
                if cancelled() {
                    interrupted.store(true, Ordering::SeqCst);
                }
                match receive.recv_timeout(Duration::from_millis(5)) {
                    Ok(result) => break result,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break Err(StoreError::Io),
                }
            };
            worker.join().map_err(|_| StoreError::Internal)?;
            result
        });
        drop(armed);
        if cancelled() || interrupted.load(Ordering::SeqCst) {
            return Err(StoreError::Io);
        }
        result
    }

    /// `VACUUM INTO` the empty temporary file, with the attached-database
    /// limit at 1 for that statement only.
    fn write_copy(&mut self, temp: &Path) -> Result<(), StoreError> {
        // SQLite filenames use platform bytes on Unix. Bind as a BLOB and
        // explicitly cast to TEXT to preserve non-UTF8 private paths.
        #[cfg(unix)]
        let target = {
            use std::os::unix::ffi::OsStrExt;
            temp.as_os_str().as_bytes()
        };
        #[cfg(not(unix))]
        let target = temp.to_str().ok_or(StoreError::Unsupported)?.as_bytes();
        let conn = &self.file.conn;
        self.file.budget.reset();
        conn.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 1)
            .map_err(|e| sqlite_err(&e))?;
        let vacuumed = conn.execute("VACUUM INTO CAST(?1 AS TEXT)", [target]);
        let restored = conn
            .set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)
            .and_then(|_| conn.limit(Limit::SQLITE_LIMIT_ATTACHED));
        if !matches!(restored, Ok(0)) {
            // Hardening is no longer what unlock verified: no more writes.
            self.gate = WriteGate::Blocked(StoreError::Hardening);
            return Err(StoreError::Hardening);
        }
        vacuumed.map_err(|e| write_err(conn, &e, 0))?;
        Ok(())
    }

    /// The copy holds exactly this vault's control rows and every stored
    /// object and manifest row, including the complete ciphertext.
    fn check_copy(&self, copy: &VaultFile, cancelled: &dyn Fn() -> bool) -> Result<(), StoreError> {
        if copy.header_body != self.file.header_body
            || copy.header_mac.as_bytes() != self.file.header_mac.as_bytes()
        {
            return Err(StoreError::Integrity);
        }
        self.file.budget.reset();
        copy.budget.reset();
        let (a, b) = (&self.file.conn, &copy.conn);
        for sql in CONTROL_ROWS {
            same_rows(a, b, sql, cancelled)?;
        }
        if has_index(a)? != has_index(b)? {
            return Err(StoreError::Integrity);
        }
        if has_index(a)? {
            same_rows(a, b, "SELECT sealed FROM snapshot_index", cancelled)?;
        }
        same_rows(
            a,
            b,
            "SELECT key_generation, object_id, data_profile, nonce, ciphertext \
             FROM object ORDER BY key_generation, object_id",
            cancelled,
        )?;
        same_rows(
            a,
            b,
            "SELECT manifest_hash, key_generation, data_profile, nonce, ciphertext \
             FROM manifest ORDER BY manifest_hash",
            cancelled,
        )?;
        Ok(())
    }

    /// Moves the verified copy over the vault and reopens the session there.
    /// Both locks are held across the move, so no other process can open
    /// either file in between.
    fn install(&mut self, temp: &Path, path: &Path, copy: VaultFile) -> Result<(), StoreError> {
        self.check_identity()?;
        let lock = copy.into_lock()?;
        lock.same_as_path(temp)?;
        #[cfg(windows)]
        self.file.close_connection()?;
        #[cfg(all(test, windows))]
        if fault(6) {
            // SQLite is closed, but the source lease still excludes another
            // opener. This was the old unlocked Windows replacement interval.
            assert_eq!(VaultFile::open(path).err(), Some(StoreError::Locked));
        }
        if let Err(e) = fs::replace_vault(temp, path) {
            #[cfg(windows)]
            if self.file.reopen().is_err() {
                self.gate = WriteGate::Blocked(StoreError::Replaced);
            }
            drop(lock);
            discard(temp);
            return Err(e);
        }
        #[cfg(test)]
        if fault(2) {
            self.gate = WriteGate::Blocked(StoreError::Replaced);
            return Err(StoreError::Io);
        }
        // From here the copy is the vault. Should reopening fail, this
        // session keeps reading the identical old state but writes no more.
        let reopened = fs::sync_dir(fs::parent_of(path))
            .and_then(|()| VaultFile::open_locked(path, lock))
            .and_then(|file| {
                if file.header_body != self.file.header_body
                    || file.header_mac.as_bytes() != self.file.header_mac.as_bytes()
                    || file.slots.len() != self.file.slots.len()
                {
                    return Err(StoreError::Replaced);
                }
                Ok(file)
            });
        match reopened {
            Ok(file) => {
                self.file = file;
                Ok(())
            }
            Err(e) => {
                self.gate = WriteGate::Blocked(StoreError::Replaced);
                Err(e)
            }
        }
    }
}

/// Control rows the copy must hold byte for byte.
const CONTROL_ROWS: [&str; 6] = [
    "SELECT body, mac FROM header",
    "SELECT slot_id, type, body FROM keyslot ORDER BY slot_id",
    "SELECT body, mac FROM local_head",
    "SELECT replica_id, writer_id, next_counter FROM replica",
    "SELECT sealed FROM archive_receipts",
    "SELECT link_id, key_generation, sealed FROM lan_link ORDER BY link_id",
];

/// Whether the file has the `snapshot_index` table (schema `0x7FFF0005`).
fn has_index(c: &Connection) -> Result<bool, StoreError> {
    let n: i64 = c
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = 'snapshot_index'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| sqlite_err(&e))?;
    Ok(n == 1)
}

/// Steps the same query on both connections and requires equal rows.
fn same_rows(
    a: &Connection,
    b: &Connection,
    sql: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), StoreError> {
    let e = |e: rusqlite::Error| sqlite_err(&e);
    let (mut sa, mut sb) = (a.prepare(sql).map_err(e)?, b.prepare(sql).map_err(e)?);
    let columns = sa.column_count();
    if sb.column_count() != columns {
        return Err(StoreError::Integrity);
    }
    let (mut ra, mut rb) = (sa.query([]).map_err(e)?, sb.query([]).map_err(e)?);
    loop {
        if cancelled() {
            return Err(StoreError::Io);
        }
        match (ra.next().map_err(e)?, rb.next().map_err(e)?) {
            (None, None) => return Ok(()),
            (Some(x), Some(y)) => {
                for i in 0..columns {
                    if x.get_ref(i).map_err(e)? != y.get_ref(i).map_err(e)? {
                        return Err(StoreError::Integrity);
                    }
                }
            }
            _ => return Err(StoreError::Integrity),
        }
    }
}

/// Removes a copy that will not be used, and any journal SQLite left.
fn discard(temp: &Path) {
    fs::remove_best_effort(&fs::with_suffix(temp, "-journal"));
    fs::remove_best_effort(temp);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use qv_crypto::RecordId;
    use qv_model::{Clock, Extensions, RandomId, RecordBody, RecordKind, RecordVersion, Timestamp};

    use super::*;
    use crate::fs::test_dir::TestDir;
    use crate::vault::tests::{PASSWORD, copy_fixture, edit, err, unlock};

    fn tombstone(u: &mut UnlockedVault, r: RecordId) {
        let mut w = u.write().unwrap();
        let c = w.reserve_counter().unwrap();
        let heads = w.head_clocks(r);
        let tomb = RecordVersion {
            version_id: qv_model::VersionId::generate().unwrap(),
            clock: Clock::for_edit(&heads, w.writer_id(), c).unwrap(),
            modified_at: Timestamp(9),
            body: RecordBody::Tombstone(RecordKind::Entry),
            ext: Extensions::new(),
        };
        w.put_local(r, tomb).unwrap();
        w.commit().unwrap();
    }

    /// A vault whose file has free pages: large records added, then
    /// deleted for good. Returns a record that stays.
    fn with_free_pages(u: &mut UnlockedVault) -> RecordId {
        let kept = RecordId::generate().unwrap();
        edit(u, kept, "kept");
        let big: Vec<RecordId> = (0..6).map(|_| RecordId::generate().unwrap()).collect();
        for r in &big {
            edit(u, *r, &"x".repeat(150_000));
        }
        for r in big {
            tombstone(u, r);
        }
        assert!(u.file_space().unwrap().free_bytes > 500_000);
        kept
    }

    fn temps(d: &TestDir) -> usize {
        std::fs::read_dir(d.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".v.qvault.tmp-")
            })
            .count()
    }

    fn reset() {
        FAULT.with(|f| f.set(0));
        crate::space::DISK.with(|d| d.set(None));
    }

    #[test]
    fn compact_shrinks_the_file_and_keeps_everything() {
        let (d, a) = (TestDir::new(), TestDir::new());
        let (p, dir) = copy_fixture(&d, &a, "v.qvault");
        let mut u = unlock(&p, &dir, PASSWORD).unwrap();
        let kept = with_free_pages(&mut u);
        let head = u.state.digest;
        let space = u.file_space().unwrap();
        let done = u.compact(&|| false).unwrap().unwrap();
        assert_eq!(done.before, space.file_bytes);
        assert!(done.after + space.free_bytes / 2 <= done.before);
        assert_eq!(u.file_space().unwrap().free_bytes, 0);
        assert_eq!(std::fs::metadata(&p).unwrap().len(), done.after);
        assert_eq!(temps(&d), 0);
        assert_eq!(u.file.conn.limit(Limit::SQLITE_LIMIT_ATTACHED).unwrap(), 0);
        assert!(
            u.file
                .conn
                .execute("ATTACH DATABASE ':memory:' AS forbidden", [])
                .is_err()
        );
        // Nothing left to free.
        assert_eq!(u.compact(&|| false).unwrap(), None);
        // The session still reads and writes the compacted file.
        assert_eq!(u.state.digest, head);
        assert!(u.manifest().record(kept).is_some());
        edit(&mut u, kept, "after");
        drop(u);
        let u = unlock(&p, &dir, PASSWORD).unwrap();
        assert!(u.manifest().record(kept).is_some());
        u.verify_all(&|| false).unwrap();
    }

    #[cfg(windows)]
    // 0012 §1: Snapshot disk growth
    #[test]
    fn compact_retains_windows_source_lock_after_closing_sqlite() {
        let (d, a) = (TestDir::new(), TestDir::new());
        let (p, dir) = copy_fixture(&d, &a, "v.qvault");
        let mut u = unlock(&p, &dir, PASSWORD).unwrap();
        with_free_pages(&mut u);
        FAULT.with(|f| f.set(6));
        u.compact(&|| false).unwrap().unwrap();
        reset();
        u.verify_all(&|| false).unwrap();
        drop(u);
        unlock(&p, &dir, PASSWORD)
            .unwrap()
            .verify_all(&|| false)
            .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn compact_preserves_non_utf8_path_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let (d, a) = (TestDir::new(), TestDir::new());
        let (p, dir) = copy_fixture(&d, &a, "v.qvault");
        let non_utf8 = d
            .path()
            .join(std::ffi::OsString::from_vec(b"vault-\xff.qvault".to_vec()));
        std::fs::rename(p, &non_utf8).unwrap();
        let mut u = unlock(&non_utf8, &dir, PASSWORD).unwrap();
        with_free_pages(&mut u);
        u.compact(&|| false).unwrap().unwrap();
        u.verify_all(&|| false).unwrap();
    }

    // 0012 §1: Snapshot disk growth
    #[test]
    fn compact_interrupted_before_or_after_the_move_leaves_a_valid_vault() {
        let (d, a) = (TestDir::new(), TestDir::new());
        let (p, dir) = copy_fixture(&d, &a, "v.qvault");
        let mut u = unlock(&p, &dir, PASSWORD).unwrap();
        let kept = with_free_pages(&mut u);
        let before = std::fs::read(&p).unwrap();

        // Before the move: the old file is the vault, the copy a stale temp
        // that the next open removes.
        FAULT.with(|f| f.set(1));
        assert_eq!(err(u.compact(&|| false)), StoreError::Io);
        reset();
        assert_eq!(temps(&d), 1);
        assert_eq!(std::fs::read(&p).unwrap(), before);
        drop(u);
        let mut u = unlock(&p, &dir, PASSWORD).unwrap();
        assert_eq!(temps(&d), 0);
        assert!(u.manifest().record(kept).is_some());

        // After the move: the copy is the vault; this session writes no
        // more, and the next unlock opens the compacted file.
        FAULT.with(|f| f.set(2));
        assert_eq!(err(u.compact(&|| false)), StoreError::Io);
        reset();
        assert!(std::fs::metadata(&p).unwrap().len() < before.len() as u64);
        assert_eq!(temps(&d), 0);
        assert_eq!(err(u.write().map(|_| ())), StoreError::Replaced);
        drop(u);
        let u = unlock(&p, &dir, PASSWORD).unwrap();
        assert!(u.manifest().record(kept).is_some());
        assert_eq!(u.file_space().unwrap().free_bytes, 0);
        u.verify_all(&|| false).unwrap();
    }

    // 0012 §1: Snapshot disk growth
    #[test]
    fn compact_refuses_a_copy_that_differs_and_changes_nothing() {
        let (d, a) = (TestDir::new(), TestDir::new());
        let (p, dir) = copy_fixture(&d, &a, "v.qvault");
        let mut u = unlock(&p, &dir, PASSWORD).unwrap();
        let kept = with_free_pages(&mut u);
        let before = std::fs::read(&p).unwrap();
        FAULT.with(|f| f.set(3));
        assert_eq!(err(u.compact(&|| false)), StoreError::Integrity);
        reset();
        assert_eq!(temps(&d), 0);
        assert_eq!(std::fs::read(&p).unwrap(), before);
        edit(&mut u, kept, "still writable");

        // Same-length ciphertext corruption must be found before replacement.
        let before = std::fs::read(&p).unwrap();
        FAULT.with(|f| f.set(5));
        assert_eq!(err(u.compact(&|| false)), StoreError::Integrity);
        reset();
        assert_eq!(std::fs::read(&p).unwrap(), before);
        assert_eq!(temps(&d), 0);

        // The progress handler interrupts inside VACUUM, not after the copy.
        // Enough rows to cross the normal progress granularity even though
        // the compacted ciphertext itself is small.
        for i in 0..100 {
            edit(&mut u, RecordId::generate().unwrap(), &format!("row {i}"));
        }
        let before = std::fs::read(&p).unwrap();
        FAULT.with(|f| f.set(4));
        assert_eq!(u.compact(&|| false), Err(StoreError::LimitExceeded));
        reset();
        assert_eq!(std::fs::read(&p).unwrap(), before);
        assert_eq!(temps(&d), 0);
        assert_eq!(u.file.conn.limit(Limit::SQLITE_LIMIT_ATTACHED).unwrap(), 0);
        // Exercise the public borrowed callback too: cancellation arriving
        // after the initial pre-copy check is forwarded to the worker, and
        // can never install its output.
        let checks = std::sync::atomic::AtomicUsize::new(0);
        assert_eq!(
            u.compact(&|| checks.fetch_add(1, Ordering::SeqCst) > 0),
            Err(StoreError::Io)
        );
        assert_eq!(std::fs::read(&p).unwrap(), before);
        assert_eq!(temps(&d), 0);
        assert_eq!(u.file.conn.limit(Limit::SQLITE_LIMIT_ATTACHED).unwrap(), 0);
        edit(&mut u, kept, "writable after interruption");

        // Too little disk space for the copy: refused before any file.
        crate::space::DISK.with(|s| {
            s.set(Some(Some(crate::space::DiskSpace { free: 1024 })));
        });
        assert_eq!(err(u.compact(&|| false)), StoreError::StorageCapacity);
        reset();
        assert_eq!(temps(&d), 0);

        // A blocked session never compacts; cancellation changes nothing.
        assert_eq!(err(u.compact(&|| true)), StoreError::Io);
        assert_eq!(temps(&d), 0);
        u.gate = WriteGate::Blocked(StoreError::Rollback);
        assert_eq!(err(u.compact(&|| false)), StoreError::Rollback);
        assert_eq!(temps(&d), 0);
        drop(u);
        unlock(&p, &dir, PASSWORD)
            .unwrap()
            .verify_all(&|| false)
            .unwrap();
    }
}
