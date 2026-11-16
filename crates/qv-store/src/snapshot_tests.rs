// SPDX-License-Identifier: AGPL-3.0-or-later

//! Local snapshots inside the vault (0004 §2.5, 0007 §3; plan 0059).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use qv_crypto::RecordId;
use qv_model::{
    Clock, Extensions, RandomId, RecordBody, RecordKind, RecordVersion, Timestamp, VersionId,
};

use crate::fs::test_dir::TestDir;
use crate::vault::tests::{PASSWORD, copy_fixture, edit, err, unlock};
use crate::vault::{Keys, VaultFile, anchor_of, sign_head};
use crate::{SnapshotBudgets, SnapshotKind, StoreError, UnlockedVault};

pub(crate) const DAY: i64 = 24 * 60 * 60 * 1000;

pub(crate) fn budgets() -> SnapshotBudgets {
    SnapshotBudgets {
        max_count: 48,
        max_age_ms: 30 * DAY,
    }
}

/// Forces the room every commit measures until dropped.
pub(crate) struct Room;
impl Room {
    pub(crate) fn set(room: u64) -> Self {
        crate::space::ROOM.with(|r| r.set(Some(room)));
        Self
    }
}
impl Drop for Room {
    fn drop(&mut self) {
        crate::space::ROOM.with(|r| r.set(None));
    }
}

/// The smallest room in which `total` live and snapshot-only bytes fit.
pub(crate) fn room_for(total: u64) -> u64 {
    total + total / 64 + crate::space::ALLOWANCE
}

/// The live manifest's referenced bytes plus its sealed length.
pub(crate) fn live_bytes(u: &UnlockedVault) -> u64 {
    let len: i64 = u
        .file
        .conn
        .query_row(
            "SELECT octet_length(ciphertext) FROM manifest WHERE manifest_hash = ?1",
            [u.state.head.manifest_hash.as_slice()],
            |r| r.get(0),
        )
        .unwrap();
    u.manifest()
        .references()
        .iter()
        .map(|r| r.length())
        .sum::<u64>()
        + len as u64
}

pub(crate) fn rows(u: &UnlockedVault, table: &str) -> i64 {
    u.file
        .conn
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// A final tombstone of `r`, committed with `keep` applied to the write.
pub(crate) fn tombstone(
    u: &mut UnlockedVault,
    r: RecordId,
    keep: impl FnOnce(&mut crate::Write<'_>),
) {
    let mut w = u.write().unwrap();
    keep(&mut w);
    let c = w.reserve_counter().unwrap();
    let heads = w.head_clocks(r);
    let tomb = RecordVersion {
        version_id: VersionId::generate().unwrap(),
        clock: Clock::for_edit(&heads, w.writer_id(), c).unwrap(),
        modified_at: Timestamp(9),
        body: RecordBody::Tombstone(RecordKind::Entry),
        ext: Extensions::new(),
    };
    w.put_local(r, tomb).unwrap();
    w.commit().unwrap();
}

/// An edit of `r` committed with `keep` applied first.
pub(crate) fn edit_with(
    u: &mut UnlockedVault,
    r: RecordId,
    title: &str,
    keep: impl FnOnce(&mut crate::Write<'_>),
) {
    let root = u.manifest().root_group_id();
    let mut w = u.write().unwrap();
    keep(&mut w);
    let c = w.reserve_counter().unwrap();
    let heads = w.head_clocks(r);
    let v = crate::vault::tests::entry_version(root, title, w.writer_id(), c, &heads);
    w.put_local(r, v).unwrap();
    w.commit().unwrap();
}

/// Rewrites a fresh fixture as a file of the schema before
/// `snapshot_index`, with a head and anchor to match.
pub(crate) fn make_v4(u: &mut UnlockedVault) {
    let keys = Keys::derive(&u.vk, &u.file.header).unwrap();
    let mut head = u.state.head.clone();
    head.snapshot_index_digest = None;
    let (stored, digest) = sign_head(&keys, &u.file.header, &head).unwrap();
    let conn = &u.file.conn;
    conn.execute_batch("BEGIN IMMEDIATE; DROP TABLE snapshot_index;")
        .unwrap();
    conn.execute(
        "UPDATE local_head SET body = ?1, mac = ?2 WHERE id = 1",
        rusqlite::params![stored.body.as_slice(), stored.mac.as_slice()],
    )
    .unwrap();
    conn.pragma_update(None, "user_version", crate::schema::SCHEMA_V4)
        .unwrap();
    conn.execute_batch("COMMIT").unwrap();
    u.state.head = head;
    u.state.stored_head = stored;
    u.state.digest = digest;
    u.state.snapshots = None;
    u.anchors
        .store(&u.state.replica.replica_id, &anchor_of(&u.state))
        .unwrap();
}

// 0012 §1: Ambiguous trust boundary
#[test]
fn snapshot_keeps_dropped_objects_until_removed_and_restores() {
    let (d, a) = (TestDir::new(), TestDir::new());
    let (p, dir) = copy_fixture(&d, &a, "v.qvault");
    let mut u = unlock(&p, &dir, PASSWORD).unwrap();
    let r = RecordId::generate().unwrap();
    edit(&mut u, r, "one");
    edit(&mut u, r, "two");
    let before = rows(&u, "object");

    // The tombstone drops both versions, but the snapshot keeps them.
    tombstone(&mut u, r, |w| {
        w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(1_000), budgets());
    });
    let kept = u.take_snapshot_outcome().unwrap().kept.unwrap();
    assert_eq!(u.snapshot_rows(), &[kept]);
    assert_eq!(kept.kind, SnapshotKind::Ordinary);
    assert_eq!(rows(&u, "object"), before + 1);
    assert_eq!(rows(&u, "manifest"), 2);
    let details = u.snapshot_details(&|| false).unwrap();
    assert_eq!(details.len(), 1);
    assert!(details[0].own_bytes > 0 && details[0].objects > 0);
    assert_eq!(
        u.snapshots_containing(&[r].into_iter().collect(), &|| false)
            .unwrap(),
        vec![kept]
    );
    drop(u);

    // It survives a reopen, verifies, and restores into a new vault.
    let u = unlock(&p, &dir, PASSWORD).unwrap();
    assert_eq!(u.snapshot_rows(), &[kept]);
    u.verify_all(&|| false).unwrap();
    let restored = d.join("restored.qvault");
    u.restore_snapshot(&kept.id, &restored, &|| false).unwrap();
    assert!(!d.join("restored.qvault.qvsnap").exists());
    let back = unlock(&restored, &dir, PASSWORD).unwrap();
    let rec = back.manifest().record(r).unwrap();
    assert_eq!(rec.heads().len(), 1);
    assert!(back.snapshot_rows().is_empty());
    drop(back);

    // Removing it releases what only it kept.
    let mut u = u;
    let mut w = u.write().unwrap();
    w.remove_snapshots(&[kept.id]);
    w.commit().unwrap();
    let outcome = u.take_snapshot_outcome().unwrap();
    assert_eq!(outcome.removed, vec![kept]);
    assert!(u.snapshot_rows().is_empty());
    assert_eq!(rows(&u, "object"), before - 2 + 1);
    assert_eq!(rows(&u, "manifest"), 1);
    assert_eq!(
        err(u.restore_snapshot(&kept.id, &d.join("again.qvault"), &|| false)),
        StoreError::NotFound
    );
}

#[test]
fn snapshot_skips_unchanged_and_duplicate_and_prunes_to_budgets() {
    let (d, a) = (TestDir::new(), TestDir::new());
    let (p, dir) = copy_fixture(&d, &a, "v.qvault");
    let mut u = unlock(&p, &dir, PASSWORD).unwrap();
    let r = RecordId::generate().unwrap();
    edit(&mut u, r, "one");

    // A commit that changes no stored object keeps nothing.
    let mut w = u.write().unwrap();
    w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(1), budgets());
    w.commit().unwrap();
    assert!(u.snapshot_rows().is_empty());
    assert_eq!(u.take_snapshot_outcome(), None);

    // Count budget 1: the next snapshot replaces the previous one.
    let one = SnapshotBudgets {
        max_count: 1,
        ..budgets()
    };
    edit_with(&mut u, r, "two", |w| {
        w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(10), one);
    });
    let first = u.take_snapshot_outcome().unwrap().kept.unwrap();
    edit_with(&mut u, r, "three", |w| {
        w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(20), one);
    });
    let outcome = u.take_snapshot_outcome().unwrap();
    let second = outcome.kept.unwrap();
    assert_eq!(outcome.removed, vec![first]);
    assert_eq!(u.snapshot_rows(), &[second]);

    // Age: an older snapshot goes once it is past the age budget.
    let young = SnapshotBudgets {
        max_age_ms: 5,
        ..budgets()
    };
    edit_with(&mut u, r, "four", |w| {
        w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(30), young);
    });
    let outcome = u.take_snapshot_outcome().unwrap();
    assert_eq!(outcome.removed, vec![second]);
    let third = outcome.kept.unwrap();

    // No room at all: an ordinary snapshot is skipped, never the commit.
    // Pruning cannot help (the live data does not fit either), so the rest
    // stay; a recovery snapshot is kept anyway.
    let full = Room::set(0);
    edit_with(&mut u, r, "five", |w| {
        w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(40), budgets());
    });
    let outcome = u.take_snapshot_outcome().unwrap();
    assert!(outcome.skipped_for_space && outcome.kept.is_none() && outcome.needed > 0);
    assert_eq!(outcome.available, 0);
    assert!(outcome.removed.is_empty());
    assert_eq!(u.snapshot_rows(), &[third]);
    edit_with(&mut u, r, "six", |w| {
        w.keep_snapshot(SnapshotKind::Recovery, Timestamp(50), budgets());
    });
    let kept = u.take_snapshot_outcome().unwrap().kept.unwrap();
    assert_eq!(kept.kind, SnapshotKind::Recovery);
    assert_eq!(u.snapshot_rows().len(), 2);
    assert!(u.snapshot_rows().contains(&kept) && u.snapshot_rows().contains(&third));
    drop(full);
    u.verify_all(&|| false).unwrap();
}

// 0012 §1: Snapshot disk growth
#[test]
fn snapshot_room_prunes_in_any_commit_and_skips_before_pruning() {
    let (d, a) = (TestDir::new(), TestDir::new());
    let (p, dir) = copy_fixture(&d, &a, "v.qvault");
    let mut u = unlock(&p, &dir, PASSWORD).unwrap();
    let (r, big, other) = (
        RecordId::generate().unwrap(),
        RecordId::generate().unwrap(),
        RecordId::generate().unwrap(),
    );
    edit(&mut u, r, "one");
    tombstone(&mut u, r, |w| {
        w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(1_000), budgets());
    });
    let small = u.take_snapshot_outcome().unwrap().kept.unwrap();
    let held = u.file_space().unwrap().snapshot_bytes;
    assert!(held > 0);

    // Removing a large record would need a snapshot that cannot fit even
    // alone, though the small one still does: only the new one is skipped.
    let without = live_bytes(&u);
    edit(&mut u, big, &"x".repeat(150_000));
    let with = live_bytes(&u);
    let big_len = with - without;
    assert!(big_len > 100_000 && held < big_len / 4);
    let room = Room::set(room_for(with - big_len / 2));
    tombstone(&mut u, big, |w| {
        w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(2 * DAY), budgets());
    });
    let outcome = u.take_snapshot_outcome().unwrap();
    assert!(outcome.skipped_for_space && outcome.kept.is_none());
    assert!(outcome.needed > big_len / 2 && outcome.available < outcome.needed);
    assert!(outcome.available >= held);
    assert!(outcome.removed.is_empty());
    assert_eq!(u.snapshot_rows(), &[small]);
    drop(room);

    // A commit that asks nothing of the snapshots prunes the oldest once
    // they no longer fit (the device ran low on space) while the live data
    // still does. An edit like the measured one grows the live data by
    // about `delta`.
    let before = live_bytes(&u);
    edit(&mut u, other, "anything");
    assert_eq!(u.take_snapshot_outcome(), None);
    let delta = live_bytes(&u) - before;
    let room = Room::set(room_for(live_bytes(&u) + delta + held / 2));
    edit(&mut u, other, "anything");
    let outcome = u.take_snapshot_outcome().unwrap();
    assert_eq!(outcome.removed, vec![small]);
    assert!(outcome.kept.is_none() && !outcome.skipped_for_space);
    assert!(u.snapshot_rows().is_empty());
    assert_eq!(rows(&u, "manifest"), 1);
    // With nothing left to prune, later commits change no index.
    edit(&mut u, other, "again");
    assert_eq!(u.take_snapshot_outcome(), None);
    drop(room);
    drop(u);
    let u = unlock(&p, &dir, PASSWORD).unwrap();
    assert!(u.snapshot_rows().is_empty());
    u.verify_all(&|| false).unwrap();
}

// 0012 §1: Snapshot disk growth
#[test]
fn snapshot_room_keeps_snapshots_when_pruning_cannot_help() {
    // Too little room for even the live data: dropping snapshots would free
    // nothing, so none is dropped and no new one is kept (review of plan
    // 0060, finding 1).
    let (d, a) = (TestDir::new(), TestDir::new());
    let (p, dir) = copy_fixture(&d, &a, "v.qvault");
    let mut u = unlock(&p, &dir, PASSWORD).unwrap();
    let (r, other) = (RecordId::generate().unwrap(), RecordId::generate().unwrap());
    edit(&mut u, r, "one");
    tombstone(&mut u, r, |w| {
        w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(1_000), budgets());
    });
    let small = u.take_snapshot_outcome().unwrap().kept.unwrap();
    let room = Room::set(live_bytes(&u) / 2);
    edit(&mut u, other, "anything");
    assert!(
        u.take_snapshot_outcome()
            .is_none_or(|o| o.removed.is_empty())
    );
    assert_eq!(u.snapshot_rows(), &[small]);
    edit_with(&mut u, other, "again", |w| {
        w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(2 * DAY), budgets());
    });
    let outcome = u.take_snapshot_outcome().unwrap();
    assert!(outcome.kept.is_none() && outcome.skipped_for_space);
    assert!(outcome.removed.is_empty());
    assert_eq!(u.snapshot_rows(), &[small]);
    drop(room);
    drop(u);
    let u = unlock(&p, &dir, PASSWORD).unwrap();
    assert_eq!(u.snapshot_rows(), &[small]);
    u.verify_all(&|| false).unwrap();
}

// 0012 §1: Ambiguous trust boundary
#[test]
fn snapshot_removal_by_record_and_tampered_storage() {
    let (d, a) = (TestDir::new(), TestDir::new());
    let (p, dir) = copy_fixture(&d, &a, "v.qvault");
    let mut u = unlock(&p, &dir, PASSWORD).unwrap();
    let (r, other) = (RecordId::generate().unwrap(), RecordId::generate().unwrap());
    edit(&mut u, other, "kept");
    edit_with(&mut u, r, "secret", |w| {
        w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(10), budgets());
    });
    let without_r = u.take_snapshot_outcome().unwrap().kept.unwrap();
    edit_with(&mut u, r, "secret 2", |w| {
        w.keep_snapshot(
            SnapshotKind::Ordinary,
            Timestamp(4 * 60 * 60 * 1000),
            budgets(),
        );
    });
    let with_r = u.take_snapshot_outcome().unwrap().kept.unwrap();
    // Permanent deletion removes the snapshots that hold the record.
    tombstone(&mut u, r, |w| w.remove_snapshots_containing([r]));
    assert_eq!(u.take_snapshot_outcome().unwrap().removed, vec![with_r]);
    assert_eq!(u.snapshot_rows(), &[without_r]);
    // Deleting `other` without that removal leaves its version only in the
    // remaining snapshot.
    tombstone(&mut u, other, |_| {});
    assert_eq!(u.snapshot_rows(), &[without_r]);
    u.verify_all(&|| false).unwrap();
    drop(u);

    let copy = |name: &str| {
        let q = d.join(name);
        std::fs::copy(&p, &q).unwrap();
        q
    };
    // A changed index row, or a missing kept manifest, refuses unlock.
    let q = copy("index.qvault");
    {
        let u = unlock(&q, &dir, PASSWORD).unwrap();
        u.file
            .conn
            .execute(
                "UPDATE snapshot_index SET sealed = substr(sealed, 2) WHERE id = 1",
                [],
            )
            .unwrap();
    }
    assert_eq!(err(unlock(&q, &dir, PASSWORD)), StoreError::Integrity);
    let q = copy("manifest.qvault");
    {
        let u = unlock(&q, &dir, PASSWORD).unwrap();
        u.file
            .conn
            .execute(
                "DELETE FROM manifest WHERE manifest_hash = ?1",
                [without_r.manifest_hash.as_slice()],
            )
            .unwrap();
    }
    assert_eq!(err(unlock(&q, &dir, PASSWORD)), StoreError::Integrity);
    // A missing kept object is found by verify and restore, not unlock.
    let q = copy("object.qvault");
    let u = unlock(&q, &dir, PASSWORD).unwrap();
    let live: Vec<[u8; 16]> = u
        .manifest()
        .references()
        .iter()
        .map(|r| r.object_id().to_bytes())
        .collect();
    let mut stmt = u.file.conn.prepare("SELECT object_id FROM object").unwrap();
    let ids: Vec<Vec<u8>> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    drop(stmt);
    let only_kept = ids
        .iter()
        .find(|id| !live.iter().any(|l| l.as_slice() == id.as_slice()))
        .expect("an object only the snapshot keeps");
    u.file
        .conn
        .execute("DELETE FROM object WHERE object_id = ?1", [only_kept])
        .unwrap();
    drop(u);
    let mut u = unlock(&q, &dir, PASSWORD).unwrap();
    assert_eq!(err(u.verify_all(&|| false)), StoreError::Integrity);
    assert!(
        u.restore_snapshot(&without_r.id, &d.join("broken.qvault"), &|| false)
            .is_err()
    );
    // It blocks no commit: an edit still saves, and deleting the damaged
    // snapshot repairs the file (review of plan 0059, finding 1).
    edit(&mut u, RecordId::generate().unwrap(), "after damage");
    let mut w = u.write().unwrap();
    w.remove_snapshots(&[without_r.id]);
    w.commit().unwrap();
    assert!(u.snapshot_rows().is_empty());
    u.verify_all(&|| false).unwrap();
}

#[test]
fn snapshot_schema_v4_file_opens_and_upgrades_on_first_snapshot() {
    let (d, a) = (TestDir::new(), TestDir::new());
    let (p, dir) = copy_fixture(&d, &a, "v.qvault");
    {
        let mut u = unlock(&p, &dir, PASSWORD).unwrap();
        make_v4(&mut u);
    }
    let mut u = unlock(&p, &dir, PASSWORD).unwrap();
    let v4 = u32::try_from(crate::schema::SCHEMA_V4).unwrap();
    let v5 = u32::try_from(crate::schema::SCHEMA_VERSION).unwrap();
    assert_eq!(u.file.schema_version(), v4);
    assert!(u.snapshot_rows().is_empty());
    let r = RecordId::generate().unwrap();
    // A commit without snapshot work leaves the file as it was.
    edit(&mut u, r, "one");
    assert_eq!(u.file.schema_version(), v4);
    edit_with(&mut u, r, "two", |w| {
        w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(10), budgets());
    });
    assert_eq!(u.file.schema_version(), v5);
    let kept = u.take_snapshot_outcome().unwrap().kept.unwrap();
    drop(u);
    let u = unlock(&p, &dir, PASSWORD).unwrap();
    assert_eq!(u.snapshot_rows(), &[kept]);
    u.verify_all(&|| false).unwrap();
}

/// The stored snapshot index blob.
fn index_blob(u: &UnlockedVault) -> Vec<u8> {
    u.file
        .conn
        .query_row("SELECT sealed FROM snapshot_index WHERE id = 1", [], |r| {
            r.get(0)
        })
        .unwrap()
}

/// Replaces the stored snapshot index blob behind the session.
fn put_index_blob(u: &UnlockedVault, blob: &[u8]) {
    u.file
        .conn
        .execute("UPDATE snapshot_index SET sealed = ?1 WHERE id = 1", [blob])
        .unwrap();
}

#[test]
fn snapshot_index_rollback_inside_the_file_is_refused() {
    // An older index blob of the same vault, sealed under the same key and
    // AAD, swapped in after a later commit: only the head's digest binds the
    // index to its state (review of plan 0059, finding 2).
    let (d, a) = (TestDir::new(), TestDir::new());
    let (p, dir) = copy_fixture(&d, &a, "v.qvault");
    let mut u = unlock(&p, &dir, PASSWORD).unwrap();
    let r = RecordId::generate().unwrap();
    edit(&mut u, r, "one");
    edit_with(&mut u, r, "two", |w| {
        w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(10), budgets());
    });
    let first = u.take_snapshot_outcome().unwrap().kept.unwrap();
    let old_blob = index_blob(&u);
    let old_digest = u.state.head.snapshot_index_digest.unwrap();
    edit_with(&mut u, r, "three", |w| {
        w.keep_snapshot(
            SnapshotKind::Ordinary,
            Timestamp(4 * 60 * 60 * 1000),
            budgets(),
        );
    });
    let second = u.take_snapshot_outcome().unwrap().kept.unwrap();
    let rows_now = u.snapshot_rows().to_vec();
    assert_eq!(rows_now.len(), 2);
    let head_digest = u.state.head.snapshot_index_digest.unwrap();
    assert_ne!(old_digest, head_digest);

    // The old blob is authentic: it opens against its own digest, and is
    // refused only against the head's.
    let keys = Keys::derive(&u.vk, &u.file.header).unwrap();
    let header = u.file.header;
    let old = crate::snapshot_index::open(&old_blob, &old_digest, &keys, &header).unwrap();
    assert_eq!(old.rows(), &[first]);
    assert_eq!(
        err(crate::snapshot_index::open(
            &old_blob,
            &head_digest,
            &keys,
            &header
        )),
        StoreError::Integrity
    );
    drop(u);

    // Each copied file needs its own copy of the same external baseline:
    // a successful session mutation must not advance the next case's anchor.
    let anchor_files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (e.file_name(), std::fs::read(e.path()).unwrap())
        })
        .collect();

    // Swapped into the closed file: unlock and recovery opening refuse it.
    let q = d.join("rolled.qvault");
    std::fs::copy(&p, &q).unwrap();
    {
        let u = unlock(&q, &dir, PASSWORD).unwrap();
        put_index_blob(&u, &old_blob);
    }
    assert_eq!(err(unlock(&q, &dir, PASSWORD)), StoreError::Integrity);
    let recovery = {
        let _k = crate::vault::tests::kdf();
        VaultFile::open(&q)
            .and_then(|f| f.unlock_for_recovery(&crate::vault::tests::pw(PASSWORD), &dir))
    };
    assert_eq!(err(recovery), StoreError::Integrity);

    // Swapped under an open session: the commit works from the session's
    // authenticated index, never from the stored blob. Whatever it does, the
    // file never opens to the older snapshot list.
    for keep in [false, true] {
        for (name, bytes) in &anchor_files {
            std::fs::write(dir.path().join(name), bytes).unwrap();
        }
        let q = d.join(&format!("session-{keep}.qvault"));
        std::fs::copy(&p, &q).unwrap();
        let mut u = unlock(&q, &dir, PASSWORD).unwrap();
        put_index_blob(&u, &old_blob);
        let other = RecordId::generate().unwrap();
        let root = u.manifest().root_group_id();
        let result = (|| {
            let mut w = u.write()?;
            if keep {
                w.keep_snapshot(SnapshotKind::Recovery, Timestamp(5 * DAY), budgets());
            }
            let c = w.reserve_counter()?;
            let v = crate::vault::tests::entry_version(root, "after", w.writer_id(), c, &[]);
            w.put_local(other, v)?;
            w.commit()
        })();
        let expected = u.snapshot_rows().to_vec();
        assert!(expected.contains(&first) && expected.contains(&second));
        drop(u);
        match unlock(&q, &dir, PASSWORD) {
            Ok(v) => {
                assert!(result.is_ok(), "{keep}");
                assert_eq!(v.snapshot_rows(), expected.as_slice(), "{keep}");
                assert_ne!(v.snapshot_rows(), &[first], "{keep}");
                v.verify_all(&|| false).unwrap();
            }
            Err(e) => assert_eq!(e, StoreError::Integrity, "{keep}"),
        }
    }
}

#[test]
fn snapshot_downgrade_with_head_index_digest_is_refused() {
    // A 0x7FFF0005 file turned back into 0x7FFF0004 (table dropped,
    // user_version lowered) while the head still names an index (key 9), and
    // the reverse: the table without the head's digest (review of plan 0059,
    // finding 2).
    let (d, a) = (TestDir::new(), TestDir::new());
    let v4 = u32::try_from(crate::schema::SCHEMA_V4).unwrap();
    let v5 = u32::try_from(crate::schema::SCHEMA_VERSION).unwrap();
    for with_snapshot in [false, true] {
        let (p, dir) = copy_fixture(&d, &a, &format!("v-{with_snapshot}.qvault"));
        {
            let mut u = unlock(&p, &dir, PASSWORD).unwrap();
            if with_snapshot {
                let r = RecordId::generate().unwrap();
                edit(&mut u, r, "one");
                edit_with(&mut u, r, "two", |w| {
                    w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(10), budgets());
                });
                assert_eq!(u.snapshot_rows().len(), 1);
            }
            assert!(u.state.head.snapshot_index_digest.is_some());
            let conn = &u.file.conn;
            conn.execute_batch("BEGIN IMMEDIATE; DROP TABLE snapshot_index;")
                .unwrap();
            conn.pragma_update(None, "user_version", crate::schema::SCHEMA_V4)
                .unwrap();
            conn.execute_batch("COMMIT").unwrap();
        }
        // The file is a well-formed file of the earlier schema, so only the
        // head can tell.
        assert_eq!(VaultFile::open(&p).unwrap().schema_version(), v4);
        assert_eq!(err(unlock(&p, &dir, PASSWORD)), StoreError::Integrity);
    }

    // The table is present but the head names no index.
    let (p, dir) = copy_fixture(&d, &a, "no-digest.qvault");
    {
        let u = unlock(&p, &dir, PASSWORD).unwrap();
        let keys = Keys::derive(&u.vk, &u.file.header).unwrap();
        let mut head = u.state.head.clone();
        head.snapshot_index_digest = None;
        let (stored, _) = sign_head(&keys, &u.file.header, &head).unwrap();
        u.file
            .conn
            .execute(
                "UPDATE local_head SET body = ?1, mac = ?2 WHERE id = 1",
                rusqlite::params![stored.body.as_slice(), stored.mac.as_slice()],
            )
            .unwrap();
    }
    assert_eq!(VaultFile::open(&p).unwrap().schema_version(), v5);
    assert_eq!(err(unlock(&p, &dir, PASSWORD)), StoreError::Integrity);
}

/// Reports a full volume to every space measurement until dropped.
struct FullDisk;
impl FullDisk {
    fn set() -> Self {
        crate::space::DISK.with(|d| d.set(Some(Some(crate::space::DiskSpace { free: 0 }))));
        Self
    }
}
impl Drop for FullDisk {
    fn drop(&mut self) {
        crate::space::DISK.with(|d| d.set(None));
    }
}

// 0012 §1: Snapshot disk growth
#[test]
fn snapshot_commit_on_a_full_volume_changes_nothing() {
    // The volume is full: the room is the file itself, and SQLite cannot
    // grow the file once the commit's objects are written. Every kind of
    // snapshot commit then fails and leaves the file byte for byte as it
    // was (review of plan 0060, finding 5).
    let (d, a) = (TestDir::new(), TestDir::new());
    let (p, dir) = copy_fixture(&d, &a, "v.qvault");
    let mut u = unlock(&p, &dir, PASSWORD).unwrap();
    let (r, other) = (RecordId::generate().unwrap(), RecordId::generate().unwrap());
    // Enough records that each new manifest row needs pages of its own.
    let root = u.manifest().root_group_id();
    let mut w = u.write().unwrap();
    for i in 0..100 {
        let c = w.reserve_counter().unwrap();
        let v =
            crate::vault::tests::entry_version(root, &format!("entry {i}"), w.writer_id(), c, &[]);
        w.put_local(RecordId::generate().unwrap(), v).unwrap();
    }
    w.commit().unwrap();
    edit(&mut u, r, "one");
    tombstone(&mut u, r, |w| {
        w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(1_000), budgets());
    });
    let small = u.take_snapshot_outcome().unwrap().kept.unwrap();
    edit(&mut u, other, "base");
    let _ = u.take_snapshot_outcome();
    // No free page inside the file either: whatever a commit adds must grow it.
    u.compact(&|| false).unwrap();
    assert_eq!(crate::space::pages(&u.file.conn).unwrap().1, 0);
    let limit: i64 = u
        .file
        .conn
        .pragma_query_value(None, "max_page_count", |r| r.get(0))
        .unwrap();

    let before = std::fs::read(&p).unwrap();
    let (seq, head, objects) = (u.local_seq(), u.state.head.clone(), rows(&u, "object"));
    let full = FullDisk::set();
    for case in 0..4 {
        let result = (|| {
            let mut w = u.write()?;
            match case {
                0 => {}
                1 => w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(2 * DAY), budgets()),
                2 => w.keep_snapshot(SnapshotKind::Recovery, Timestamp(2 * DAY), budgets()),
                _ => w.remove_snapshots(&[small.id]),
            }
            let c = w.reserve_counter()?;
            let heads = w.head_clocks(other);
            let v = crate::vault::tests::entry_version(root, "full", w.writer_id(), c, &heads);
            w.put_local(other, v)?;
            // The volume fills up now: the commit's own writes find no page.
            let pages: i64 = w
                .conn()
                .pragma_query_value(None, "page_count", |r| r.get(0))
                .unwrap();
            w.conn()
                .pragma_update(None, "max_page_count", pages)
                .unwrap();
            w.commit()
        })();
        u.file
            .conn
            .pragma_update(None, "max_page_count", limit)
            .unwrap();
        // `max_page_count` stands in for the full volume, so the error is the
        // page-limit one; a real full volume maps to `StorageCapacity`
        // (`error::write_err`).
        assert_eq!(result, Err(StoreError::LimitExceeded), "case {case}");
        assert_eq!(std::fs::read(&p).unwrap(), before, "case {case}");
        assert!(!crate::fs::with_suffix(&p, "-journal").exists());
        assert_eq!(u.local_seq(), seq, "case {case}");
        assert_eq!(u.state.head, head, "case {case}");
        assert_eq!(u.snapshot_rows(), &[small], "case {case}");
        assert_eq!(rows(&u, "object"), objects, "case {case}");
        assert_eq!(u.take_snapshot_outcome(), None, "case {case}");
    }
    u.verify_all(&|| false).unwrap();
    drop(full);
    drop(u);
    let mut u = unlock(&p, &dir, PASSWORD).unwrap();
    assert_eq!((u.local_seq(), &u.state.head), (seq, &head));
    assert_eq!(u.snapshot_rows(), &[small]);
    u.verify_all(&|| false).unwrap();
    // With space again, the same commit saves.
    edit(&mut u, other, "full");
    assert_eq!(u.local_seq(), seq + 1);
}
