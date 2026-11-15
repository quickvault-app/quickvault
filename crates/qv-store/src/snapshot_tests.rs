// SPDX-License-Identifier: AGPL-3.0-or-later

//! Local snapshots inside the vault (0004 §2.5, 0007 §3; plan 0059).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use qv_crypto::RecordId;
use qv_model::{
    Clock, Extensions, RandomId, RecordBody, RecordKind, RecordVersion, Timestamp, VersionId,
};

use crate::fs::test_dir::TestDir;
use crate::vault::tests::{PASSWORD, copy_fixture, edit, err, unlock};
use crate::vault::{Keys, anchor_of, sign_head};
use crate::{SnapshotBudgets, SnapshotKind, StoreError, UnlockedVault};

const DAY: i64 = 24 * 60 * 60 * 1000;

fn budgets() -> SnapshotBudgets {
    SnapshotBudgets {
        max_count: 48,
        max_age_ms: 30 * DAY,
    }
}

/// Forces the room every commit measures until dropped.
struct Room;
impl Room {
    fn set(room: u64) -> Self {
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
fn room_for(total: u64) -> u64 {
    total + total / 64 + crate::space::ALLOWANCE
}

/// The live manifest's referenced bytes plus its sealed length.
fn live_bytes(u: &UnlockedVault) -> u64 {
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

fn rows(u: &UnlockedVault, table: &str) -> i64 {
    u.file
        .conn
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// A final tombstone of `r`, committed with `keep` applied to the write.
fn tombstone(u: &mut UnlockedVault, r: RecordId, keep: impl FnOnce(&mut crate::Write<'_>)) {
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
fn edit_with(
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
fn make_v4(u: &mut UnlockedVault) {
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

    // No room at all: an ordinary snapshot is skipped and the rest pruned,
    // never the commit; a recovery snapshot is kept anyway.
    let full = Room::set(0);
    edit_with(&mut u, r, "five", |w| {
        w.keep_snapshot(SnapshotKind::Ordinary, Timestamp(40), budgets());
    });
    let outcome = u.take_snapshot_outcome().unwrap();
    assert!(outcome.skipped_for_space && outcome.kept.is_none() && outcome.needed > 0);
    assert_eq!(outcome.available, 0);
    assert_eq!(outcome.removed, vec![third]);
    assert!(u.snapshot_rows().is_empty());
    edit_with(&mut u, r, "six", |w| {
        w.keep_snapshot(SnapshotKind::Recovery, Timestamp(50), budgets());
    });
    let kept = u.take_snapshot_outcome().unwrap().kept.unwrap();
    assert_eq!(kept.kind, SnapshotKind::Recovery);
    assert_eq!(u.snapshot_rows(), &[kept]);
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
    // they no longer fit (the device ran low on space).
    let room = Room::set(room_for(live_bytes(&u)));
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
    let u = unlock(&q, &dir, PASSWORD).unwrap();
    assert_eq!(err(u.verify_all(&|| false)), StoreError::Integrity);
    assert!(
        u.restore_snapshot(&without_r.id, &d.join("broken.qvault"), &|| false)
            .is_err()
    );
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
