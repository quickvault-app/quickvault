// SPDX-License-Identifier: AGPL-3.0-or-later

//! What one commit does to the local snapshots (0004 §2.5, 0007 §3.2–3.3;
//! plan 0059).
//!
//! A commit may keep the manifest it replaces as a snapshot, remove named
//! snapshots, remove the snapshots that contain given records, and prune by
//! age, count and room (plan 0060: live plus snapshot-only data within the
//! room [`crate::space`] measures). [`plan`] works all of that out before any
//! SQL changes: the new index, the objects and manifest rows nothing needs
//! any more, and what to report. Kept manifests are read back and
//! authenticated as at unlock; bytes count only what snapshots keep beyond
//! the new live manifest (snapshot-only manifest rows and objects). A commit
//! that asks nothing of the snapshots still prunes the oldest when they no
//! longer fit the room, checked first from stored lengths alone.

use std::collections::{BTreeMap, BTreeSet};

use qv_model::{ObjectId, RecordId, Timestamp};
use rusqlite::Connection;

use crate::SnapshotKind;
use crate::error::StoreError;
use crate::header::Header;
use crate::manifest::{Manifest, envelope_hash, open_manifest};
use crate::schema;
use crate::snapshot_index::{SnapshotIndex, SnapshotRow};
use crate::vault::{Keys, accepted_profile, scope};

/// The snapshot budgets a commit that keeps a snapshot prunes to (0007
/// §3.3). Space is not a budget: snapshot-only data uses the room the vault
/// does not need, in every commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotBudgets {
    /// Most snapshots kept.
    pub max_count: u32,
    /// Oldest age a snapshot may reach, in milliseconds.
    pub max_age_ms: i64,
}

/// What a commit was asked to do with snapshots.
#[derive(Clone, Debug, Default)]
pub(crate) struct Request {
    /// Keep the replaced manifest as a snapshot of this kind, made at this
    /// time, then prune to these budgets.
    pub(crate) keep: Option<(SnapshotKind, Timestamp, SnapshotBudgets)>,
    /// Remove these snapshot IDs.
    pub(crate) remove: BTreeSet<[u8; 16]>,
    /// Remove every snapshot whose manifest has any of these records.
    pub(crate) containing: BTreeSet<RecordId>,
}

impl Request {
    pub(crate) fn is_empty(&self) -> bool {
        self.keep.is_none() && self.remove.is_empty() && self.containing.is_empty()
    }
}

/// What a commit did with snapshots, for the caller's report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapshotOutcome {
    /// The snapshot this commit kept, if any.
    pub kept: Option<SnapshotRow>,
    /// The commit was asked to keep one, but it would not fit the room even
    /// alone, so it was not kept (ordinary snapshots only).
    pub skipped_for_space: bool,
    /// The snapshot-only bytes the skipped snapshot needed; 0 otherwise.
    pub needed: u64,
    /// The snapshot-only bytes the room allowed at that commit; 0 unless
    /// skipped.
    pub available: u64,
    /// The snapshots this commit removed (explicitly or by pruning).
    pub removed: Vec<SnapshotRow>,
}

/// The decided change.
pub(crate) struct Planned {
    pub(crate) index: SnapshotIndex,
    /// Objects the old index retained that nothing needs any more.
    pub(crate) released: BTreeSet<ObjectId>,
    /// Manifest rows nothing names any more (besides the replaced one,
    /// which the commit handles).
    pub(crate) drop_manifests: BTreeSet<[u8; 32]>,
    pub(crate) outcome: SnapshotOutcome,
}

/// Reads and authenticates one kept manifest row.
pub(crate) fn open_kept(
    conn: &Connection,
    keys: &Keys,
    header: &Header,
    hash: &[u8; 32],
) -> Result<(Manifest, u64), StoreError> {
    let m = schema::read_manifest(conn, hash)?;
    if m.hash != *hash || m.key_generation != header.key_generation {
        return Err(StoreError::Integrity);
    }
    let profile = accepted_profile(header, m.data_profile)?;
    if envelope_hash(m.key_generation, profile, &m.nonce, &m.ciphertext)? != m.hash {
        return Err(StoreError::Integrity);
    }
    let manifest = open_manifest(
        &keys.manifest,
        scope(header),
        profile,
        &m.nonce,
        &m.ciphertext,
    )?;
    Ok((manifest, m.ciphertext.len() as u64))
}

/// Per-object reference counts over the distinct kept manifests.
struct Counts {
    /// Object → (kept manifests referencing it, ciphertext length).
    objects: BTreeMap<ObjectId, (u32, u64)>,
    /// Manifest hash → (rows naming it, stored ciphertext length).
    manifests: BTreeMap<[u8; 32], (u32, u64)>,
}

impl Counts {
    fn add_manifest(&mut self, m: &Manifest) {
        for r in m.references() {
            self.objects
                .entry(r.object_id())
                .or_insert((0, r.length()))
                .0 += 1;
        }
    }

    fn remove_manifest(&mut self, m: &Manifest) {
        for r in m.references() {
            if let Some(entry) = self.objects.get_mut(&r.object_id()) {
                entry.0 = entry.0.saturating_sub(1);
                if entry.0 == 0 {
                    self.objects.remove(&r.object_id());
                }
            }
        }
    }

    /// Snapshot-only bytes beside the live manifest.
    fn bytes(&self, live: &BTreeSet<ObjectId>, live_hash: &[u8; 32]) -> u64 {
        let objects: u64 = self
            .objects
            .iter()
            .filter(|(id, _)| !live.contains(id))
            .map(|(_, (_, len))| *len)
            .sum();
        let manifests: u64 = self
            .manifests
            .iter()
            .filter(|(hash, _)| *hash != live_hash)
            .map(|(_, (_, len))| *len)
            .sum();
        objects.saturating_add(manifests)
    }
}

/// The inputs [`plan`] reads.
pub(crate) struct Inputs<'a> {
    pub(crate) conn: &'a Connection,
    pub(crate) keys: &'a Keys,
    pub(crate) header: &'a Header,
    /// The index before this commit; `None` for a file of the earlier schema.
    pub(crate) old: Option<&'a SnapshotIndex>,
    /// The manifest this commit replaces, and its row hash.
    pub(crate) replaced: (&'a Manifest, [u8; 32]),
    /// The new live manifest, and its row hash.
    pub(crate) live: (&'a Manifest, [u8; 32]),
    /// The new live manifest's sealed length.
    pub(crate) live_len: u64,
    /// The room live and snapshot-only data may use ([`crate::space::room`]).
    pub(crate) room: u64,
}

/// Decides the commit's snapshot change; `None` when the index stays as it
/// is. Reads only: nothing is written here.
pub(crate) fn plan(
    input: &Inputs<'_>,
    request: &Request,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<Planned>, StoreError> {
    let (replaced, replaced_hash) = input.replaced;
    let (live, live_hash) = input.live;
    let live_ids: BTreeSet<ObjectId> = live.references().iter().map(|r| r.object_id()).collect();
    let live_bytes = live
        .references()
        .iter()
        .fold(input.live_len, |sum, r| sum.saturating_add(r.length()));
    // Whatever was asked, prune when the snapshots no longer fit the room.
    let over_room = match input.old {
        Some(old) if !old.rows().is_empty() => {
            let held = crate::space::snapshot_only_bytes(
                input.conn,
                old,
                &live_ids,
                &live_hash,
                input.header.key_generation,
            )?;
            !crate::space::fits(live_bytes, held, input.room)
        }
        _ => false,
    };
    if request.is_empty() && !over_room {
        return Ok(None);
    }
    let old = input.old.cloned().unwrap_or_default();
    let mut index = old.clone();
    let mut outcome = SnapshotOutcome::default();

    // Opens a kept manifest, using the in-memory copy of the replaced one.
    let open = |hash: &[u8; 32]| -> Result<(Manifest, u64), StoreError> {
        if cancelled() {
            return Err(StoreError::Io);
        }
        open_kept(input.conn, input.keys, input.header, hash)
    };

    // 1. Explicit removals and removals by record.
    let mut removed: Vec<SnapshotRow> = Vec::new();
    for row in old.rows() {
        let by_id = request.remove.contains(&row.id);
        let by_record = !by_id
            && !request.containing.is_empty()
            && open(&row.manifest_hash)?
                .0
                .records()
                .iter()
                .any(|r| request.containing.contains(&r.record_id()));
        if by_id || by_record {
            index.remove(&row.id);
            removed.push(*row);
        }
    }

    // 2. Keep the replaced manifest, unless it holds the same objects as the
    //    new live one or as the newest snapshot (0007 §3.2).
    let mut kept: Option<SnapshotRow> = None;
    if let Some((kind, now, _)) = request.keep {
        let digest = replaced.reference_digest()?;
        let unchanged = live.reference_digest()? == digest;
        let duplicate = index.newest().is_some_and(|n| n.reference_digest == digest);
        if !unchanged && !duplicate {
            // The index's hard cap never fails a commit: the oldest go first.
            while index.rows().len() >= crate::snapshot_index::MAX_SNAPSHOTS {
                let Some(oldest) = index
                    .rows()
                    .iter()
                    .min_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)))
                    .copied()
                else {
                    break;
                };
                index.remove(&oldest.id);
                removed.push(oldest);
            }
            let row = SnapshotRow {
                id: qv_crypto::random_array::<16>().map_err(|_| StoreError::Rng)?,
                created_at: now,
                kind,
                manifest_hash: replaced_hash,
                reference_digest: digest,
            };
            index.insert(row)?;
            kept = Some(row);
        }
    }

    if index == old && !over_room {
        return Ok(None);
    }

    // 3. Reference counts over what remains, then pruning to the budgets.
    let mut counts = Counts {
        objects: BTreeMap::new(),
        manifests: BTreeMap::new(),
    };
    for row in index.rows() {
        let entry = counts.manifests.entry(row.manifest_hash).or_insert((0, 0));
        entry.0 += 1;
    }
    let hashes: Vec<[u8; 32]> = counts.manifests.keys().copied().collect();
    for hash in &hashes {
        let (m, len) = if *hash == replaced_hash {
            let len = stored_len(input.conn, hash)?;
            (replaced.clone(), len)
        } else {
            open(hash)?
        };
        counts.add_manifest(&m);
        if let Some(entry) = counts.manifests.get_mut(hash) {
            entry.1 = len;
        }
    }

    // An ordinary snapshot that cannot fit even alone is skipped first, so
    // no older snapshot is pruned for it.
    if let Some(row) = kept
        && row.kind == SnapshotKind::Ordinary
    {
        let alone = replaced
            .references()
            .iter()
            .filter(|r| !live_ids.contains(&r.object_id()))
            .fold(stored_len(input.conn, &replaced_hash)?, |sum, r| {
                sum.saturating_add(r.length())
            });
        if !crate::space::fits(live_bytes, alone, input.room) {
            outcome.needed = alone;
            outcome.available = crate::space::available(live_bytes, input.room);
            outcome.skipped_for_space = true;
            unkeep(&row, &mut index, &mut counts, replaced);
            kept = None;
        }
    }

    {
        let mut candidates: Vec<SnapshotRow> = index
            .rows()
            .iter()
            .filter(|r| Some(r.id) != kept.map(|k| k.id))
            .copied()
            .collect();
        candidates.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        let mut drop_row = |row: &SnapshotRow,
                            index: &mut SnapshotIndex,
                            counts: &mut Counts|
         -> Result<(), StoreError> {
            index.remove(&row.id);
            if let Some(entry) = counts.manifests.get_mut(&row.manifest_hash) {
                entry.0 = entry.0.saturating_sub(1);
                if entry.0 == 0 {
                    counts.manifests.remove(&row.manifest_hash);
                    let m = if row.manifest_hash == replaced_hash {
                        replaced.clone()
                    } else {
                        open(&row.manifest_hash)?.0
                    };
                    counts.remove_manifest(&m);
                }
            }
            removed.push(*row);
            Ok(())
        };
        // Age and count apply in a commit that keeps a snapshot; the room
        // and the retained cap in every commit.
        let mut rest = Vec::new();
        for row in candidates {
            if let Some((_, now, budgets)) = request.keep
                && now.0.saturating_sub(row.created_at.0) > budgets.max_age_ms
            {
                drop_row(&row, &mut index, &mut counts)?;
            } else {
                rest.push(row);
            }
        }
        let max_count = request
            .keep
            .map_or(usize::MAX, |(_, _, b)| b.max_count as usize);
        let fits = |index: &SnapshotIndex, counts: &Counts| {
            index.rows().len() <= max_count
                && crate::space::fits(live_bytes, counts.bytes(&live_ids, &live_hash), input.room)
                && counts.objects.len() <= crate::snapshot_index::MAX_RETAINED
        };
        for row in rest {
            if fits(&index, &counts) {
                break;
            }
            drop_row(&row, &mut index, &mut counts)?;
        }
        if !fits(&index, &counts)
            && let Some(row) = kept
            && row.kind == SnapshotKind::Ordinary
        {
            // It does not fit even alone: keep the commit, not the snapshot.
            outcome.needed = counts.bytes(&live_ids, &live_hash);
            outcome.available = crate::space::available(live_bytes, input.room);
            unkeep(&row, &mut index, &mut counts, replaced);
            kept = None;
            outcome.skipped_for_space = true;
        }
    }

    // 4. The new retained set, and what nothing needs any more.
    index.set_retained(counts.objects.keys().copied().collect())?;
    let released: BTreeSet<ObjectId> = old
        .retained()
        .iter()
        .copied()
        .filter(|id| !index.retains(*id) && !live_ids.contains(id))
        .collect();
    let still_named: BTreeSet<[u8; 32]> = index
        .rows()
        .iter()
        .map(|r| r.manifest_hash)
        .chain([live_hash])
        .collect();
    let drop_manifests: BTreeSet<[u8; 32]> = old
        .rows()
        .iter()
        .map(|r| r.manifest_hash)
        .filter(|h| !still_named.contains(h))
        .collect();
    removed.retain(|r| Some(r.id) != kept.map(|k| k.id));
    outcome.kept = kept;
    outcome.removed = removed;
    Ok(Some(Planned {
        index,
        released,
        drop_manifests,
        outcome,
    }))
}

/// Takes back the snapshot this commit was about to keep.
fn unkeep(row: &SnapshotRow, index: &mut SnapshotIndex, counts: &mut Counts, replaced: &Manifest) {
    index.remove(&row.id);
    if let Some(entry) = counts.manifests.get_mut(&row.manifest_hash) {
        entry.0 = entry.0.saturating_sub(1);
        if entry.0 == 0 {
            counts.manifests.remove(&row.manifest_hash);
            counts.remove_manifest(replaced);
        }
    }
}

/// A stored manifest row's ciphertext length.
fn stored_len(conn: &Connection, hash: &[u8; 32]) -> Result<u64, StoreError> {
    let len: i64 = conn
        .query_row(
            "SELECT octet_length(ciphertext) FROM manifest WHERE manifest_hash = ?1",
            [hash.as_slice()],
            |r| r.get(0),
        )
        .map_err(|e| schema::validation_err(&e))?;
    u64::try_from(len).map_err(|_| StoreError::Integrity)
}

/// One kept snapshot with its sizes (0011 `snapshot ls`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotDetail {
    /// The index row.
    pub row: SnapshotRow,
    /// How many objects its manifest references.
    pub objects: u64,
    /// Their total ciphertext length.
    pub total_bytes: u64,
    /// What it alone keeps: its manifest row and the objects no other
    /// snapshot and not the live vault references.
    pub own_bytes: u64,
}

impl crate::UnlockedVault {
    /// Makes every following commit keep the manifest it replaces as a
    /// snapshot of `kind` made at `now`, pruned to `budgets`, as
    /// [`crate::Write::keep_snapshot`] does for one write. Once a commit keeps
    /// an ordinary snapshot, later commits keep none until armed again (one
    /// per coalescing interval). `None` disarms.
    pub fn arm_snapshot(&mut self, armed: Option<(SnapshotKind, Timestamp, SnapshotBudgets)>) {
        self.state.armed = armed;
    }

    /// The kept local snapshots, ascending by ID (0004 §2.5).
    pub fn snapshot_rows(&self) -> &[SnapshotRow] {
        self.state.snapshots.as_ref().map_or(&[], |i| i.rows())
    }

    /// What this session's last commit did with snapshots, once.
    pub fn take_snapshot_outcome(&mut self) -> Option<SnapshotOutcome> {
        self.state.last_snapshot.take()
    }

    /// Every kept snapshot with its sizes, ascending by ID. Each kept
    /// manifest is authenticated; objects are not re-read (`verify` does).
    pub fn snapshot_details(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<SnapshotDetail>, StoreError> {
        let rows = self.snapshot_rows();
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        self.begin_read()?;
        let result = self.details_pass(rows, cancelled);
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

    fn details_pass(
        &self,
        rows: &[SnapshotRow],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<SnapshotDetail>, StoreError> {
        let header = self.file.header;
        let keys = Keys::derive(&self.vk, &header)?;
        let live: BTreeSet<ObjectId> = self
            .state
            .manifest
            .references()
            .iter()
            .map(|r| r.object_id())
            .collect();
        let mut hashes: BTreeMap<[u8; 32], u32> = BTreeMap::new();
        for r in rows {
            *hashes.entry(r.manifest_hash).or_insert(0) += 1;
        }
        // Pass 1: in how many distinct kept manifests each object appears.
        let mut seen: BTreeMap<ObjectId, u32> = BTreeMap::new();
        for hash in hashes.keys() {
            if cancelled() {
                return Err(StoreError::Io);
            }
            let (m, _) = open_kept(&self.file.conn, &keys, &header, hash)?;
            for r in m.references() {
                *seen.entry(r.object_id()).or_insert(0) += 1;
            }
        }
        // Pass 2: per manifest, its totals and what only it keeps.
        let mut per_hash: BTreeMap<[u8; 32], (u64, u64, u64)> = BTreeMap::new();
        for (hash, n) in &hashes {
            if cancelled() {
                return Err(StoreError::Io);
            }
            let (m, len) = open_kept(&self.file.conn, &keys, &header, hash)?;
            let mut totals = (0u64, 0u64, if *n == 1 { len } else { 0 });
            for r in m.references() {
                totals.0 += 1;
                totals.1 = totals.1.saturating_add(r.length());
                if *n == 1 && !live.contains(&r.object_id()) && seen.get(&r.object_id()) == Some(&1)
                {
                    totals.2 = totals.2.saturating_add(r.length());
                }
            }
            per_hash.insert(*hash, totals);
        }
        rows.iter()
            .map(|row| {
                let (objects, total_bytes, own_bytes) = *per_hash
                    .get(&row.manifest_hash)
                    .ok_or(StoreError::Internal)?;
                Ok(SnapshotDetail {
                    row: *row,
                    objects,
                    total_bytes,
                    own_bytes,
                })
            })
            .collect()
    }

    /// The kept snapshots whose manifest has any of `records` (0007 §3.3:
    /// what a permanent deletion would also remove).
    pub fn snapshots_containing(
        &self,
        records: &BTreeSet<RecordId>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<SnapshotRow>, StoreError> {
        let rows = self.snapshot_rows();
        if rows.is_empty() || records.is_empty() {
            return Ok(Vec::new());
        }
        let header = self.file.header;
        let keys = Keys::derive(&self.vk, &header)?;
        self.begin_read()?;
        let result = (|| {
            let mut hits: BTreeMap<[u8; 32], bool> = BTreeMap::new();
            let mut out = Vec::new();
            for row in rows {
                if cancelled() {
                    return Err(StoreError::Io);
                }
                let hit = match hits.get(&row.manifest_hash) {
                    Some(hit) => *hit,
                    None => {
                        let (m, _) =
                            open_kept(&self.file.conn, &keys, &header, &row.manifest_hash)?;
                        let hit = m.records().iter().any(|r| records.contains(&r.record_id()));
                        hits.insert(row.manifest_hash, hit);
                        hit
                    }
                };
                if hit {
                    out.push(*row);
                }
            }
            Ok(out)
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
