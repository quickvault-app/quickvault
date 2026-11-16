// SPDX-License-Identifier: AGPL-3.0-or-later

//! Local snapshots and the full `verify` report (spec 0007 §3; 0011
//! *Snapshots and verification*).
//!
//! Since plan 0059 an ordinary local snapshot is a manifest the vault keeps
//! after a commit replaces it (0004 §2.5, `qv-store`). This module owns the
//! **policy** around them:
//!
//! - **Automatic ordinary snapshots** (0007 §3.2): every mutation arms the
//!   store when one is due (none yet, the newest over an hour old, or none
//!   from today), so its first commit keeps the replaced manifest. Read
//!   commands write nothing. A snapshot skipped for space is reported through
//!   [`Session::snapshot_status`]; it never fails the requested command.
//! - **Recovery snapshots** (0007 §3.2): archive merges, baseline adoption and
//!   a conflict resolution that installs a final deletion arm a recovery
//!   snapshot for their own commit. Explicit deletions take none; unless the
//!   caller keeps them, they remove the snapshots holding the deleted data
//!   ([`Session::snapshots_holding`]).
//! - **Budgets** (0007 §3.3): 30 days, 48 snapshots and 1 GiB of
//!   snapshot-only data per vault, pruned oldest first inside the commit.
//! - **Snapshot files**: a VK rotation still writes its recovery copy as an
//!   archive file ([`Session::begin_destructive`], pinned until
//!   [`Session::finish_destructive`]); files written before plan 0059 stay
//!   listable and restorable, and the unlock pass ages them out by count,
//!   bytes and age. A `recover-` file left by a crash is reclassified as
//!   ordinary before it can be pruned.
//! - **`verify`** (0004 §3.6, 0011 §4): one read-only full pass, with the
//!   unlock-time anchor comparison and fixed notes.
//!
//! Every snapshot file is authenticated with the vault key before it can
//! affect policy or appear in a listing. Files that do not authenticate
//! (another vault or generation, a slot-bearing archive, damage) are ignored
//! by policy and counted as unreadable by [`Session::snapshots`], which the
//! CLI reports as warnings. The role of "today" is the UTC date: core has no
//! time-zone source and adds no dependency (development value, phase 1.16).

use std::sync::{Arc, Mutex, PoisonError};

use qv_model::Timestamp;
use qv_store::{
    AnchorStatus, ArchiveHeader, ArchiveScope, InventoryRow, PublishedSnapshot, SnapshotDir,
    SnapshotEntry, SnapshotKind, StoreError, UnlockedVault, VerifyCounts,
};

use crate::error::CoreError;
use crate::session::{CancelToken, Open, Session};

/// Ordinary snapshots kept per vault (0007 §3.3, development value).
pub const DEFAULT_MAX_COUNT: u32 = 48;

/// Byte budget of the snapshot files outside the vault: those from before
/// plan 0059 and the VK rotation's recovery copy (0007 §3.1). Snapshots
/// inside the vault have no byte budget; they use the room the vault does
/// not need (0007 §3.3, plan 0060).
pub const DEFAULT_MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// Automatic compaction runs once at least this much of the file is free
/// pages, and at least a quarter of it (0004 §1.4, plan 0060).
pub const AUTO_COMPACT_MIN_BYTES: u64 = 32 * 1024 * 1024;

/// Maximum age of an ordinary snapshot (0007 §3.3, development value: 30
/// days).
pub const DEFAULT_MAX_AGE_SECONDS: i64 = 30 * 24 * 60 * 60;

/// The coalescing interval of automatic ordinary snapshots (0007 §3.2).
pub const SNAPSHOT_INTERVAL_SECONDS: i64 = 60 * 60;

const MILLIS_PER_SECOND: i64 = 1000;
const MILLIS_PER_DAY: i64 = 24 * 60 * 60 * MILLIS_PER_SECOND;

/// Where local snapshots live and how many are kept (0007 §3.3).
pub struct SnapshotPolicy {
    /// The private snapshot directory (0007 §3.1).
    pub dir: SnapshotDir,
    /// Most ordinary snapshots kept.
    pub max_count: u32,
    /// Aggregate budget of the snapshot files outside the vault (see
    /// [`DEFAULT_MAX_BYTES`]).
    pub max_bytes: u64,
    /// Oldest age an ordinary snapshot may reach.
    pub max_age_seconds: i64,
}

impl SnapshotPolicy {
    /// The default budgets in `dir`.
    pub fn new(dir: SnapshotDir) -> Self {
        Self {
            dir,
            max_count: DEFAULT_MAX_COUNT,
            max_bytes: DEFAULT_MAX_BYTES,
            max_age_seconds: DEFAULT_MAX_AGE_SECONDS,
        }
    }

    /// The default policy in the private application directory
    /// (`$XDG_STATE_HOME/quickvault/snapshots`), creating it if needed.
    pub fn default_dir() -> Result<Self, StoreError> {
        let dir = SnapshotDir::default_dir()?;
        Ok(Self::new(SnapshotDir::open(&dir)?))
    }

    /// The budgets the vault's own snapshots are pruned to (plan 0059).
    pub(crate) fn budgets(&self) -> qv_store::SnapshotBudgets {
        qv_store::SnapshotBudgets {
            max_count: self.max_count,
            max_age_ms: self.max_age_seconds.saturating_mul(MILLIS_PER_SECOND),
        }
    }
}

/// The default budgets, for a recovery snapshot without a configured policy.
fn default_budgets() -> qv_store::SnapshotBudgets {
    qv_store::SnapshotBudgets {
        max_count: DEFAULT_MAX_COUNT,
        max_age_ms: DEFAULT_MAX_AGE_SECONDS.saturating_mul(MILLIS_PER_SECOND),
    }
}

/// Whether an ordinary snapshot is due (0007 §3.2): none yet, the newest is
/// over an hour old, or none is from today (UTC).
fn due(rows: &[qv_store::SnapshotRow], now: Timestamp) -> bool {
    let newest = rows
        .iter()
        .filter(|r| r.kind == SnapshotKind::Ordinary)
        .map(|r| r.created_at)
        .max();
    newest.is_none_or(|t| {
        now.0.saturating_sub(t.0) >= SNAPSHOT_INTERVAL_SECONDS * MILLIS_PER_SECOND
            || day_of(t.0) != day_of(now.0)
    })
}

/// Before a mutation: arm the hourly ordinary snapshot when one is due and a
/// policy is configured (plan 0059). Read-only sessions keep none.
pub(crate) fn arm_automatic(open: &mut Open, now: Timestamp) {
    let armed = match &open.snapshots {
        Some(policy) if !open.read_only && due(open.vault.snapshot_rows(), now) => {
            Some((SnapshotKind::Ordinary, now, policy.budgets()))
        }
        _ => None,
    };
    open.vault.arm_snapshot(armed);
}

/// Inside an operation that changes what the vault holds without the user
/// choosing it (0007 §3.2): its commit keeps the pre-operation state as a
/// recovery snapshot, whatever the hourly rule says.
pub(crate) fn arm_recovery(open: &mut Open, now: Timestamp) {
    let budgets = open
        .snapshots
        .as_ref()
        .map_or_else(default_budgets, |p| p.budgets());
    open.vault
        .arm_snapshot(Some((SnapshotKind::Recovery, now, budgets)));
}

/// After a mutation: disarm, and report what its commits did.
pub(crate) fn after_mutation(open: &mut Open) {
    open.vault.arm_snapshot(None);
    if let Some(outcome) = open.vault.take_snapshot_outcome() {
        record_outcome(open, &outcome);
    }
}

/// Reports what a commit did with snapshots.
fn record_outcome(open: &mut Open, outcome: &qv_store::SnapshotOutcome) {
    {
        open.auto_snapshot = Some(SnapshotReport {
            published: outcome.kept.map(|row| SnapshotSummary {
                archive_id: row.id_hex(),
                created_at: row.created_at,
                kind: row.kind,
                size: 0,
                objects: 0,
                total_bytes: 0,
            }),
            pruned: outcome
                .removed
                .iter()
                .map(qv_store::SnapshotRow::id_hex)
                .collect(),
            skipped: outcome.skipped_for_space.then_some(SnapshotSkip::NoRoom {
                needed: outcome.needed,
                available: outcome.available,
            }),
            failed: false,
        });
        if outcome.skipped_for_space {
            open.skipped_snapshot = Some(SnapshotSpace {
                needed: outcome.needed,
                available: outcome.available,
            });
        }
    }
}

/// After a successful mutation: compact the file when its free pages reach
/// [`AUTO_COMPACT_MIN_BYTES`] and a quarter of it (0004 §1.4). Best effort:
/// any failure (too little disk space, a pending anchor) leaves the vault as
/// it is, and a later commit tries again. A lock request (`cancelled`)
/// interrupts the copy through SQLite's progress handler and discards it
/// before replacement (review of plan 0060, finding 3).
pub(crate) fn auto_compact(open: &mut Open, cancelled: &dyn Fn() -> bool) {
    if open.read_only || cancelled() {
        return;
    }
    if let Ok((file, free)) = open.vault.file_pages()
        && free >= AUTO_COMPACT_MIN_BYTES
        && free.saturating_mul(4) >= file
    {
        let _ = open.vault.compact(cancelled);
    }
}

/// Why a commit kept no snapshot (0007 §3.3), for the frontend's notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotSpace {
    /// The snapshot-only bytes the snapshot needed.
    pub needed: u64,
    /// What snapshots could use at that commit.
    pub available: u64,
}

/// The vault file's space (0009 §1.8 Storage; 0011 `compact`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageInfo {
    /// The vault file's size.
    pub file_bytes: u64,
    /// Free pages inside it, which compaction returns to the volume.
    pub free_bytes: u64,
    /// What snapshots keep beside the live vault.
    pub snapshot_bytes: u64,
    /// Free space on the volume; `None` when it cannot be measured.
    pub disk_free: Option<u64>,
}

impl core::fmt::Debug for SnapshotPolicy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SnapshotPolicy")
            .field("dir", &self.dir)
            .field("max_count", &self.max_count)
            .field("max_bytes", &self.max_bytes)
            .field("max_age_seconds", &self.max_age_seconds)
            .finish()
    }
}

/// One authenticated local snapshot (0011 `snapshot ls`).
///
/// It carries no secret: the archive ID is random and non-linkable and the
/// counts are aggregate integers.
#[derive(Clone, PartialEq, Eq)]
pub struct SnapshotSummary {
    /// The archive's 32 lowercase hex identifier.
    pub archive_id: String,
    /// Its authenticated creation time.
    pub created_at: Timestamp,
    /// Whether it is an ordinary or a recovery copy.
    pub kind: SnapshotKind,
    /// Its size on disk in bytes.
    pub size: u64,
    /// How many objects it carries.
    pub objects: u64,
    /// Their total referenced ciphertext length.
    pub total_bytes: u64,
}

impl core::fmt::Debug for SnapshotSummary {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SnapshotSummary")
            .field("archive_id", &self.archive_id)
            .field("created_at", &self.created_at)
            .field("kind", &self.kind)
            .field("size", &self.size)
            .field("objects", &self.objects)
            .field("total_bytes", &self.total_bytes)
            .finish()
    }
}

/// Why the automatic policy did not take a snapshot (0007 §3.2–3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SnapshotSkip {
    /// No snapshot policy is configured; snapshots are unavailable.
    Disabled,
    /// The newest ordinary snapshot is less than an hour old.
    OnePerHour,
    /// The first-of-day copy already exists and the hourly one is not due.
    /// The default rule takes the first-of-day copy even inside the hourly
    /// window, so it reports [`Self::OnePerHour`] instead; this variant is
    /// reserved for a daily-only policy.
    AlreadyToday,
    /// The copy needs `needed` bytes and does not fit the ordinary budgets
    /// (0007 §3.3). A destructive operation asks for approval.
    OverBudget {
        /// The size the copy needs on disk.
        needed: u64,
    },
    /// A snapshot inside the vault did not fit the room the vault leaves
    /// (0007 §3.3): the change was saved without it.
    NoRoom {
        /// The snapshot-only bytes it needed.
        needed: u64,
        /// What snapshots could use at that commit.
        available: u64,
    },
}

/// The outcome of one automatic-policy attempt (0011 §4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapshotReport {
    /// The copy that was written, if any.
    pub published: Option<SnapshotSummary>,
    /// The archive IDs of the snapshots pruning removed.
    pub pruned: Vec<String>,
    /// Why no copy was written, if it was skipped on purpose.
    pub skipped: Option<SnapshotSkip>,
    /// The copy could not be written (disk full, unsafe directory, an I/O
    /// failure). A warning, never an error for the requested command.
    pub failed: bool,
}

impl SnapshotReport {
    /// The report of a session without a snapshot policy.
    pub(crate) fn disabled() -> Self {
        Self {
            skipped: Some(SnapshotSkip::Disabled),
            ..Self::default()
        }
    }

    /// The report of an automatic copy that could not be written.
    pub(crate) fn failed() -> Self {
        Self {
            failed: true,
            ..Self::default()
        }
    }
}

/// An authenticated listing of the local snapshots (0011 `snapshot ls`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapshotListing {
    /// The snapshots of this vault ID and key generation, newest first by
    /// authenticated `created_at` and then archive ID.
    pub snapshots: Vec<SnapshotSummary>,
    /// How many files in the directory could not be authenticated and were
    /// therefore not listed. The CLI reports them as warnings.
    pub unreadable: u64,
}

/// A fixed-label note of the `verify` report (0011 §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum VerifyNote {
    /// A history floor covers versions absent by design after a purge
    /// (0007 §1.2, §1.4): expected state, never damage.
    SuppressionFloorCoversDominatedVersions,
}

impl VerifyNote {
    /// The stable label the CLI prints (0011 §4).
    pub fn label(self) -> &'static str {
        match self {
            Self::SuppressionFloorCoversDominatedVersions => {
                "suppression-floor-covers-dominated-versions"
            }
        }
    }
}

/// One full `verify` pass (0011 §4; 0004 §3.6).
///
/// It carries counts, the vault's authenticated identity and the anchor
/// comparison only: no object ID, hash, record ID, payload or path.
#[derive(Clone, PartialEq, Eq)]
pub struct VerifyReport {
    /// The vault ID, lowercase hex.
    pub vault_id: String,
    /// The current key generation.
    pub key_generation: u32,
    /// The committed local sequence number.
    pub local_seq: u64,
    /// The head-versus-anchor comparison made at unlock (0005 §5.4).
    pub anchor: AnchorStatus,
    /// What the pass counted.
    pub counts: VerifyCounts,
    /// Fixed-label notes, never damage.
    pub notes: Vec<VerifyNote>,
}

impl VerifyReport {
    /// Whether every check passed and the anchor is current or one step
    /// behind (0011 §4: exit 0 only for `verified` or `advanced`).
    pub fn ok(&self) -> bool {
        matches!(self.anchor, AnchorStatus::Verified | AnchorStatus::Advanced)
    }
}

impl core::fmt::Debug for VerifyReport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VerifyReport")
            .field("vault_id", &self.vault_id)
            .field("key_generation", &self.key_generation)
            .field("local_seq", &self.local_seq)
            .field("anchor", &self.anchor)
            .field("counts", &self.counts)
            .field("notes", &self.notes)
            .finish()
    }
}

/// A pinned recovery snapshot of an in-progress destructive operation
/// (0007 §3.2). Dropping it without [`Session::finish_destructive`] releases
/// the pin; the copy then stays as a `recover-` file that the next policy
/// pass reclassifies as ordinary.
pub struct RecoverySnapshot {
    pin: Arc<Mutex<Option<RecoveryPin>>>,
    summary: SnapshotSummary,
    entry: SnapshotEntry,
}

/// The pinned copy of a VK rotation in progress (plan 0059: the only
/// operation that still writes a recovery file). Its containing Arc binds it
/// to this exact session; the handle's leased entry keeps the file protected
/// until the handle is released.
#[derive(Clone)]
pub(crate) struct RecoveryPin {
    pub(crate) archive_id: String,
}

impl RecoverySnapshot {
    /// The published copy's authenticated facts.
    pub fn summary(&self) -> &SnapshotSummary {
        &self.summary
    }
}

impl core::fmt::Debug for RecoverySnapshot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RecoverySnapshot")
            .field("archive_id", &self.summary.archive_id)
            .finish_non_exhaustive()
    }
}

impl Drop for RecoverySnapshot {
    fn drop(&mut self) {
        let mut pin = self.pin.lock().unwrap_or_else(PoisonError::into_inner);
        if pin.as_ref().map(|p| p.archive_id.as_str()) == Some(self.summary.archive_id.as_str()) {
            *pin = None;
        }
    }
}

/// Earlier generations' snapshots, from the current generation's verified
/// inventory (plan 0023; 0007 §3.2–3.3).
enum OldRows {
    /// The rows whose file is still present (stale rows are ignored).
    Rows(Vec<InventoryRow>),
    /// Generation ≥ 2 without a verifying inventory: policy fails closed,
    /// writing no ordinary copy and pruning nothing.
    Unavailable,
}

/// The current generation's inventory rows. Generation 1 has none.
fn old_rows(policy: &SnapshotPolicy, vault: &UnlockedVault) -> OldRows {
    if vault.header().key_generation.0 < 2 {
        return OldRows::Rows(Vec::new());
    }
    let (Ok(Some(inventory)), Ok(entries)) = (policy.dir.inventory(vault), policy.dir.entries())
    else {
        return OldRows::Unavailable;
    };
    OldRows::Rows(
        inventory
            .rows()
            .iter()
            .filter(|r| {
                let id = r.archive_id_hex();
                entries.iter().any(|e| e.archive_id() == id)
            })
            .cloned()
            .collect(),
    )
}

/// The archive IDs of the verified inventory, for listings: those files are
/// accounted for, not unreadable.
fn inventoried(policy: &SnapshotPolicy, vault: &UnlockedVault) -> Vec<String> {
    match old_rows(policy, vault) {
        OldRows::Rows(rows) => rows.iter().map(InventoryRow::archive_id_hex).collect(),
        OldRows::Unavailable => Vec::new(),
    }
}

/// Removes one earlier-generation snapshot through its verified row: only an
/// exact hash and size match is deleted. `false` (kept, reported as failed)
/// on any mismatch.
fn remove_row(
    policy: &SnapshotPolicy,
    vault: &UnlockedVault,
    row: &InventoryRow,
    report: &mut SnapshotReport,
    cancelled: &dyn Fn() -> bool,
) -> Result<bool, CoreError> {
    match policy.dir.remove_inventoried(vault, row, cancelled) {
        Ok(()) | Err(StoreError::NotFound) => {
            report.pruned.push(row.archive_id_hex());
            Ok(true)
        }
        Err(StoreError::Integrity) => {
            report.failed = true;
            Ok(false)
        }
        Err(StoreError::Locked) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// The auth outcome of one policy pass: only authenticated local snapshots
/// of this vault ID and generation count.
fn authenticated(
    policy: &SnapshotPolicy,
    vault: &UnlockedVault,
) -> Result<(Vec<(SnapshotEntry, ArchiveHeader)>, u64), StoreError> {
    let scope = ArchiveScope::new(vault.header().vault_id, vault.header().key_generation);
    let mut rows = Vec::new();
    let mut unreadable = 0;
    for entry in policy.dir.entries()? {
        match policy.dir.read_header(&entry, scope, vault.vault_key()) {
            Ok(header) => rows.push((entry, header)),
            Err(_) => unreadable += 1,
        }
    }
    Ok((rows, unreadable))
}

fn summary(entry: &SnapshotEntry, header: &ArchiveHeader) -> SnapshotSummary {
    SnapshotSummary {
        archive_id: header.archive_id_hex(),
        created_at: header.created_at(),
        kind: entry.kind(),
        size: entry.size(),
        objects: header.object_count(),
        total_bytes: header.total_length(),
    }
}

/// The authenticated facts of a copy this process just published.
fn published_summary(published: &PublishedSnapshot) -> SnapshotSummary {
    SnapshotSummary {
        archive_id: published.archive_id().to_owned(),
        created_at: published.created_at(),
        kind: published.kind(),
        size: published.size(),
        objects: published.objects(),
        total_bytes: published.total_bytes(),
    }
}

/// The UTC day of `ms` (core has no time-zone source; development value).
fn day_of(ms: i64) -> i64 {
    ms.div_euclid(MILLIS_PER_DAY)
}

/// One policy pass over the snapshot files: reclassify stale recovery
/// copies, prune expired and over-budget files, then, with `kind`, write a
/// new copy (since plan 0059 only a VK rotation's recovery copy). Without
/// `kind` it only prunes: ordinary snapshots live in the vault now, and the
/// files written before stay listable until they age out.
///
/// `pinned` is the archive ID an in-progress operation protects. The pass
/// never removes it, and never removes the copy it just wrote.
fn snapshot_pass(
    policy: &SnapshotPolicy,
    vault: &mut UnlockedVault,
    kind: Option<SnapshotKind>,
    now: Timestamp,
    allow_over_budget: bool,
    pinned: Option<&str>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Taken, CoreError> {
    let dir = &policy.dir;
    let scope = ArchiveScope::new(vault.header().vault_id, vault.header().key_generation);

    // 1. A `recover-` file that no operation pins (a crash, or a finished
    //    operation whose reclassify failed) is ordinary before any pruning so
    //    recovery copies cannot accumulate forever (0007 §3.2).
    for entry in dir.entries()? {
        if entry.kind() != SnapshotKind::Recovery {
            continue;
        }
        let Ok(header) = dir.read_header(&entry, scope, vault.vault_key()) else {
            continue;
        };
        if pinned == Some(header.archive_id_hex().as_str()) {
            continue;
        }
        match dir.reclassify_as_ordinary(&entry, scope, vault.vault_key()) {
            Ok(_) | Err(StoreError::Locked | StoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
    }

    let mut report = SnapshotReport::default();
    let max_age_ms = policy.max_age_seconds.saturating_mul(MILLIS_PER_SECOND);

    // Earlier generations count only through a verified inventory. Without
    // one, no ordinary copy and no pruning; a recovery copy needs consent.
    let mut old = match old_rows(policy, vault) {
        OldRows::Rows(rows) => rows,
        OldRows::Unavailable => {
            let Some(kind) = kind else {
                return Ok(Taken {
                    report: SnapshotReport::failed(),
                    entry: None,
                });
            };
            if kind == SnapshotKind::Ordinary {
                return Err(StoreError::Integrity.into());
            }
            let published = dir.publish(vault, kind, now, cancelled)?;
            let new = published_summary(&published);
            if !allow_over_budget {
                match dir.remove(published.entry()) {
                    Ok(()) | Err(StoreError::NotFound) => {}
                    Err(e) => return Err(e.into()),
                }
                return Err(CoreError::SnapshotOverBudget { needed: new.size });
            }
            report.failed = true;
            report.published = Some(new);
            return Ok(Taken {
                report,
                entry: Some(published.entry().clone()),
            });
        }
    };

    // 2. Write and verify the new copy (0004 §6.5).
    let published = match kind {
        Some(kind) => Some(dir.publish(vault, kind, now, cancelled)?),
        None => None,
    };
    let new = published.as_ref().map(published_summary);
    let new_entry = published.as_ref().map(|p| p.entry().clone());
    let new_id = new.as_ref().map(|n| n.archive_id.clone());

    // 3. Age pruning, oldest first, authenticated headers only.
    let (rows, _) = authenticated(policy, vault)?;
    for (entry, header) in &rows {
        if cancelled() {
            return Err(CoreError::Cancelled);
        }
        if entry.kind() != SnapshotKind::Ordinary {
            continue;
        }
        let id = header.archive_id_hex();
        if pinned == Some(id.as_str()) {
            continue;
        }
        if now.0.saturating_sub(header.created_at().0) <= max_age_ms {
            continue;
        }
        remove_entry(dir, entry, &id, &mut report)?;
    }
    // Earlier generations age out the same way; a mismatch stops pruning.
    let mut kept = Vec::with_capacity(old.len());
    for row in old.drain(..) {
        if cancelled() {
            return Err(CoreError::Cancelled);
        }
        if !report.failed && now.0.saturating_sub(row.created_at.0) > max_age_ms {
            if !remove_row(policy, vault, &row, &mut report, cancelled)? {
                kept.push(row);
            }
        } else {
            kept.push(row);
        }
    }
    let old = kept;

    // 4. Count and byte pruning as if the new copy were ordinary, oldest
    //    first; the pinned copy and the new copy are never candidates.
    let (rows, _) = authenticated(policy, vault)?;
    let mut count = 0u64;
    let mut bytes = 0u64;
    let mut candidates = Vec::new();
    for (entry, header) in &rows {
        let id = header.archive_id_hex();
        let is_new = new_id.as_deref() == Some(id.as_str());
        if entry.kind() != SnapshotKind::Ordinary && !is_new {
            continue;
        }
        count = count.checked_add(1).ok_or(StoreError::LimitExceeded)?;
        bytes = bytes
            .checked_add(entry.size())
            .ok_or(StoreError::LimitExceeded)?;
        if !is_new && pinned != Some(id.as_str()) {
            candidates.push((header.created_at(), id, Candidate::Current(entry.clone())));
        }
    }
    // Earlier generations' copies are ordinary once no journal pins them
    // (0007 §3.2), whatever their file prefix.
    for row in old {
        count = count.checked_add(1).ok_or(StoreError::LimitExceeded)?;
        bytes = bytes
            .checked_add(row.size)
            .ok_or(StoreError::LimitExceeded)?;
        candidates.push((row.created_at, row.archive_id_hex(), Candidate::Old(row)));
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let fits =
        |count: u64, bytes: u64| count <= u64::from(policy.max_count) && bytes <= policy.max_bytes;
    // A copy that cannot fit even alone never justifies deleting others.
    let new_size = new.as_ref().map_or(0, |n| n.size);
    if new_size <= policy.max_bytes && policy.max_count > 0 {
        for (_, id, candidate) in candidates {
            if fits(count, bytes) || report.failed {
                break;
            }
            let size = match &candidate {
                Candidate::Current(entry) => {
                    if !remove_entry(dir, entry, &id, &mut report)? {
                        continue;
                    }
                    entry.size()
                }
                Candidate::Old(row) => {
                    if !remove_row(policy, vault, row, &mut report, cancelled)? {
                        break;
                    }
                    row.size
                }
            };
            count = count.saturating_sub(1);
            bytes = bytes.saturating_sub(size);
        }
    }

    // 5. What does not fit is a budget refusal, not damage: remove the copy
    //    we just wrote unless the caller approved exceeding the budget.
    let (Some(new), Some(new_entry)) = (new, new_entry) else {
        return Ok(Taken {
            report,
            entry: None,
        });
    };
    if !fits(count, bytes) && !allow_over_budget {
        match dir.remove(&new_entry) {
            Ok(()) | Err(StoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
        if new_entry.kind() == SnapshotKind::Recovery {
            return Err(CoreError::SnapshotOverBudget { needed: new.size });
        }
        report.skipped = Some(SnapshotSkip::OverBudget { needed: new.size });
        return Ok(Taken {
            report,
            entry: None,
        });
    }
    report.published = Some(new);
    Ok(Taken {
        report,
        entry: Some(new_entry),
    })
}

/// Removes one snapshot and records its archive ID as pruned.
fn remove_entry(
    dir: &SnapshotDir,
    entry: &SnapshotEntry,
    id: &str,
    report: &mut SnapshotReport,
) -> Result<bool, CoreError> {
    match dir.remove(entry) {
        Ok(()) | Err(StoreError::NotFound) => {
            report.pruned.push(id.to_owned());
            Ok(true)
        }
        Err(StoreError::Locked) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// A pruning candidate of the count/byte pass.
enum Candidate {
    /// A snapshot of the current generation, authenticated by its header.
    Current(SnapshotEntry),
    /// An earlier generation's snapshot, by its verified inventory row.
    Old(InventoryRow),
}

/// What one policy pass produced.
struct Taken {
    report: SnapshotReport,
    entry: Option<SnapshotEntry>,
}

/// The caller already holds the session for the entire protected operation.
pub(crate) fn begin_recovery(
    open: &mut Open,
    now: Timestamp,
    allow_over_budget: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<RecoverySnapshot, CoreError> {
    if cancelled() {
        return Err(CoreError::Cancelled);
    }
    if open.pinned().is_some() {
        return Err(CoreError::NotAllowed);
    }
    let policy = open.snapshots.clone().ok_or(CoreError::NotAllowed)?;
    let taken = snapshot_pass(
        &policy,
        &mut open.vault,
        Some(SnapshotKind::Recovery),
        now,
        allow_over_budget,
        None,
        cancelled,
    )?;
    let summary = taken.report.published.clone().ok_or(CoreError::Internal)?;
    let entry = taken.entry.ok_or(CoreError::Internal)?;
    *open.pin.lock().unwrap_or_else(PoisonError::into_inner) = Some(RecoveryPin {
        archive_id: summary.archive_id.clone(),
    });
    Ok(RecoverySnapshot {
        pin: Arc::clone(&open.pin),
        summary,
        entry,
    })
}

pub(crate) fn finish_recovery(
    open: &mut Open,
    snapshot: RecoverySnapshot,
) -> Result<(), CoreError> {
    if !Arc::ptr_eq(&open.pin, &snapshot.pin) {
        return Err(CoreError::StaleHandle);
    }
    {
        let mut pin = open.pin.lock().unwrap_or_else(PoisonError::into_inner);
        if pin.as_ref().map(|p| p.archive_id.as_str()) != Some(snapshot.summary.archive_id.as_str())
        {
            return Err(CoreError::StaleHandle);
        }
        *pin = None;
    }
    let policy = open.snapshots.clone().ok_or(CoreError::Internal)?;
    let scope = ArchiveScope::new(
        open.vault.header().vault_id,
        open.vault.header().key_generation,
    );
    policy
        .dir
        .reclassify_as_ordinary(&snapshot.entry, scope, open.vault.vault_key())?;
    Ok(())
}

/// The vault's own snapshots as listing rows (plan 0059): `size` is what
/// each alone keeps.
fn in_vault(
    vault: &UnlockedVault,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<SnapshotSummary>, CoreError> {
    Ok(vault
        .snapshot_details(cancelled)?
        .into_iter()
        .map(|d| SnapshotSummary {
            archive_id: d.row.id_hex(),
            created_at: d.row.created_at,
            kind: d.row.kind,
            size: d.own_bytes,
            objects: d.objects,
            total_bytes: d.total_bytes,
        })
        .collect())
}

/// What a permanent deletion would take out of the snapshots (0007 §3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeletionTarget<'a> {
    /// Every non-conflicted item in the trash ([`Session::empty_trash`]).
    Trash,
    /// These trashed items ([`Session::purge`]).
    Items(&'a [crate::ItemHandle]),
    /// One attachment ([`Session::remove_attachment`]).
    Attachment(crate::AttachmentHandle),
    /// One entry's history ([`Session::purge_history`]).
    History(crate::EntryHandle),
}

impl Session {
    /// The authenticated local snapshots of this vault (0011 `snapshot ls`).
    ///
    /// Files that cannot be authenticated are counted in
    /// [`SnapshotListing::unreadable`] and never listed. An empty listing is
    /// returned when no snapshot policy is configured.
    pub fn snapshots(&self) -> Result<SnapshotListing, CoreError> {
        self.read(|ctx| {
            let c = ctx.canceller();
            let mut snapshots = in_vault(&ctx.open.vault, &|| c.is_cancelled())?;
            let Some(policy) = ctx.open.snapshots.clone() else {
                snapshots.sort_by(|a, b| {
                    b.created_at
                        .cmp(&a.created_at)
                        .then_with(|| a.archive_id.cmp(&b.archive_id))
                });
                return Ok(SnapshotListing {
                    snapshots,
                    unreadable: 0,
                });
            };
            let (rows, unreadable) = authenticated(&policy, &ctx.open.vault)?;
            // Earlier generations' copies are accounted for by the verified
            // inventory: neither listed nor reported as unreadable.
            let accounted = inventoried(&policy, &ctx.open.vault);
            let unreadable = unreadable.saturating_sub(accounted.len() as u64);
            snapshots.extend(rows.iter().map(|(e, h)| summary(e, h)));
            snapshots.sort_by(|a, b| {
                b.created_at
                    .cmp(&a.created_at)
                    .then_with(|| a.archive_id.cmp(&b.archive_id))
            });
            Ok(SnapshotListing {
                snapshots,
                unreadable,
            })
        })
    }

    /// How many of the vault's snapshots hold what a permanent deletion of
    /// `target` would delete, for its confirmation (0007 §3.3): unless the
    /// user keeps them, the deletion removes them too.
    pub fn snapshots_holding(&self, target: DeletionTarget<'_>) -> Result<usize, CoreError> {
        self.read(|ctx| {
            let index = &ctx.open.index;
            let mut records = std::collections::BTreeSet::new();
            match target {
                DeletionTarget::Trash => {
                    for (id, r) in &index.records {
                        if r.status.in_trash
                            && !r.status.conflict
                            && matches!(
                                r.kind,
                                qv_model::RecordKind::Entry | qv_model::RecordKind::Group
                            )
                        {
                            records.insert(*id);
                        }
                    }
                }
                DeletionTarget::Items(items) => {
                    for item in items {
                        records.insert(match item {
                            crate::ItemHandle::Entry(h) => h.record,
                            crate::ItemHandle::Group(h) => h.record,
                        });
                    }
                }
                DeletionTarget::Attachment(h) => {
                    records.insert(h.record);
                }
                DeletionTarget::History(e) => {
                    records.insert(e.record);
                }
            }
            // An entry's attachments go with it.
            let entries = records.clone();
            for (id, r) in &index.records {
                if let crate::index::Data::Attachment { entry } = &r.data
                    && entries.contains(entry)
                {
                    records.insert(*id);
                }
            }
            let c = ctx.canceller();
            Ok(ctx
                .open
                .vault
                .snapshots_containing(&records, &|| c.is_cancelled())?
                .len())
        })
    }

    /// Matches local snapshot filename IDs, including damaged/foreign files.
    /// Authentication is required by `restore_snapshot` before acceptance.
    pub fn snapshot_matches(&self, prefix: &str) -> Result<Vec<String>, CoreError> {
        let prefix = prefix.strip_prefix('@').unwrap_or(prefix);
        if !(6..=32).contains(&prefix.len()) || !prefix.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CoreError::InvalidInput);
        }
        let prefix = prefix.to_ascii_lowercase();
        self.read(|ctx| {
            let mut ids: Vec<String> = ctx
                .open
                .vault
                .snapshot_rows()
                .iter()
                .map(qv_store::SnapshotRow::id_hex)
                .filter(|id| id.starts_with(&prefix))
                .collect();
            if let Some(policy) = ctx.open.snapshots.clone() {
                ids.extend(
                    policy
                        .dir
                        .entries()?
                        .iter()
                        .map(|e| e.archive_id().to_owned())
                        .filter(|id| id.starts_with(&prefix)),
                );
            }
            ids.sort();
            ids.dedup();
            Ok(ids)
        })
    }

    /// Explicitly restores a full local archive ID into a new path and fresh
    /// local baseline. The active vault and its anchor are preserved. This
    /// does not establish that the snapshot is the latest global state.
    pub fn restore_snapshot(
        &self,
        id: &str,
        path: &std::path::Path,
        cancel: Option<&CancelToken>,
    ) -> Result<(), CoreError> {
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CoreError::InvalidInput);
        }
        let id = id.to_ascii_lowercase();
        // This publishes a separate file, but does not mutate/compact the
        // source. Preserve a completed publication if a lock arrives later.
        let result = self.run(|ctx| {
            ctx.check_cancel()?;
            if cancel.is_some_and(CancelToken::is_cancelled) {
                return Err(CoreError::Cancelled);
            }
            // A snapshot inside the vault first (plan 0059), then a file.
            if let Some(row) = ctx
                .open
                .vault
                .snapshot_rows()
                .iter()
                .find(|r| r.id_hex() == id)
                .copied()
            {
                #[cfg(test)]
                crate::session::hooks::reached(crate::session::hooks::Point::BeforeCommit);
                ctx.check_cancel()?;
                let c = ctx.canceller();
                ctx.open.vault.restore_snapshot(&row.id, path, &|| {
                    c.is_cancelled() || cancel.is_some_and(CancelToken::is_cancelled)
                })?;
                #[cfg(test)]
                crate::session::hooks::reached(crate::session::hooks::Point::AfterCommit);
                return Ok(());
            }
            let policy = ctx.open.snapshots.clone().ok_or(CoreError::NotFound)?;
            let entries = policy.dir.entries()?;
            let mut matches = entries.iter().filter(|e| e.archive_id() == id);
            let entry = matches.next().ok_or(CoreError::NotFound)?;
            if matches.next().is_some() {
                return Err(CoreError::Store(StoreError::Integrity));
            }
            #[cfg(test)]
            crate::session::hooks::reached(crate::session::hooks::Point::BeforeCommit);
            ctx.check_cancel()?;
            let c = ctx.canceller();
            policy.dir.restore(entry, &ctx.open.vault, path, &|| {
                c.is_cancelled() || cancel.is_some_and(CancelToken::is_cancelled)
            })?;
            #[cfg(test)]
            crate::session::hooks::reached(crate::session::hooks::Point::AfterCommit);
            Ok(())
        });
        if result.is_ok() {
            self.run_automatic_snapshot(cancel);
        }
        result
    }

    /// The snapshots created before `cutoff` (1.14 `passwd` support, 0007
    /// §3.5), newest first. Includes recovery copies.
    pub fn snapshots_before(&self, cutoff: Timestamp) -> Result<Vec<SnapshotSummary>, CoreError> {
        self.read(|ctx| {
            let c = ctx.canceller();
            let mut snapshots: Vec<SnapshotSummary> =
                in_vault(&ctx.open.vault, &|| c.is_cancelled())?
                    .into_iter()
                    .filter(|s| s.created_at < cutoff)
                    .collect();
            if let Some(policy) = ctx.open.snapshots.clone() {
                let (rows, _) = authenticated(&policy, &ctx.open.vault)?;
                snapshots.extend(
                    rows.iter()
                        .map(|(e, h)| summary(e, h))
                        .filter(|s| s.created_at < cutoff),
                );
            }
            snapshots.sort_by(|a, b| {
                b.created_at
                    .cmp(&a.created_at)
                    .then_with(|| a.archive_id.cmp(&b.archive_id))
            });
            Ok(snapshots)
        })
    }

    /// Deletes the named snapshots (1.14 `passwd` support, 0007 §3.5) and
    /// returns how many were removed. Unknown archive IDs are ignored, and a
    /// copy pinned by an in-progress destructive operation is never deleted.
    pub fn delete_snapshots(&self, ids: &[String]) -> Result<usize, CoreError> {
        self.mutate(|ctx| {
            // Inside the vault: one commit removes them (plan 0059).
            let kept: Vec<[u8; 16]> = ctx
                .open
                .vault
                .snapshot_rows()
                .iter()
                .filter(|r| ids.contains(&r.id_hex()))
                .map(|r| r.id)
                .collect();
            let mut deleted = kept.len();
            if !kept.is_empty() {
                if ctx.open.read_only {
                    return Err(StoreError::NoAnchor.into());
                }
                #[cfg(test)]
                crate::session::hooks::reached(crate::session::hooks::Point::BeforeCommit);
                ctx.check_cancel()?;
                let mut w = ctx.open.vault.write()?;
                w.keep_no_snapshot();
                w.remove_snapshots(&kept);
                w.commit()?;
            }
            let Some(policy) = ctx.open.snapshots.clone() else {
                return Ok(deleted);
            };
            let pinned = ctx.open.pinned();
            let (rows, _) = authenticated(&policy, &ctx.open.vault)?;
            #[cfg(test)]
            crate::session::hooks::reached(crate::session::hooks::Point::BeforeCommit);
            ctx.check_cancel()?;
            for (entry, header) in &rows {
                let id = header.archive_id_hex();
                if !ids.contains(&id) || pinned.as_deref() == Some(id.as_str()) {
                    continue;
                }
                match policy.dir.remove(entry) {
                    Ok(()) | Err(StoreError::NotFound) => deleted += 1,
                    Err(e) => return Err(e.into()),
                }
            }
            #[cfg(test)]
            crate::session::hooks::reached(crate::session::hooks::Point::AfterCommit);
            Ok(deleted)
        })
    }

    /// The mandatory pre-operation recovery snapshot (0007 §3.2). It is
    /// written and verified before the caller performs the destructive
    /// operation, and pinned until [`Self::finish_destructive`] or the
    /// handle's drop.
    ///
    /// [`CoreError::SnapshotOverBudget`] means the copy needs `needed` bytes
    /// and does not fit the ordinary budgets: nothing is pinned and the
    /// caller must ask the user before retrying with
    /// `allow_over_budget = true`. Any other error means the copy could not
    /// be written at all, and the operation must not proceed.
    pub fn begin_destructive(
        &self,
        allow_over_budget: bool,
        cancel: Option<&CancelToken>,
    ) -> Result<RecoverySnapshot, CoreError> {
        self.read(|ctx| {
            let c = ctx.canceller();
            let now = ctx.now();
            begin_recovery(ctx.open, now, allow_over_budget, &|| {
                c.is_cancelled() || cancel.is_some_and(CancelToken::is_cancelled)
            })
        })
    }

    /// Ends a destructive operation: release its pin and make its copy ordinary.
    /// Reclassification failure leaves a recovery file for the next policy pass.
    pub fn finish_destructive(&self, snapshot: RecoverySnapshot) -> Result<(), CoreError> {
        self.mutate(|ctx| finish_recovery(ctx.open, snapshot))
    }

    /// One full integrity pass over the committed state (0011 `verify`;
    /// 0004 §3.6). It never mutates or repairs the vault.
    ///
    /// [`VerifyReport::anchor`] is the comparison made at unlock, so a
    /// session from [`crate::Core::unlock_for_recovery`] lets a missing or
    /// mismatched anchor be reported together with the counts instead of
    /// refusing before the report exists.
    pub fn verify(&self, cancel: Option<&CancelToken>) -> Result<VerifyReport, CoreError> {
        self.read(|ctx| {
            let c = ctx.canceller();
            let counts = ctx.open.vault.verify_all(&|| {
                c.is_cancelled() || cancel.is_some_and(CancelToken::is_cancelled)
            })?;
            let header = ctx.open.vault.header();
            let mut notes = Vec::new();
            if counts.suppressed_records > 0 {
                notes.push(VerifyNote::SuppressionFloorCoversDominatedVersions);
            }
            Ok(VerifyReport {
                vault_id: crate::setup::hex(&header.vault_id.0),
                key_generation: header.key_generation.0,
                local_seq: ctx.open.vault.local_seq(),
                anchor: ctx.open.anchor,
                counts,
                notes,
            })
        })
    }

    /// The notice that the last change was saved without a snapshot for lack
    /// of room (0007 §3.3), once; `None` otherwise.
    pub fn take_snapshot_skipped(&self) -> Option<SnapshotSpace> {
        self.read(|ctx| Ok(ctx.open.skipped_snapshot.take()))
            .ok()
            .flatten()
    }

    /// The vault file's size, its reclaimable space, what snapshots use and
    /// the volume's free space (0009 §1.8 Storage).
    pub fn storage(&self) -> Result<StorageInfo, CoreError> {
        self.read(|ctx| {
            let s = ctx.open.vault.file_space()?;
            Ok(StorageInfo {
                file_bytes: s.file_bytes,
                free_bytes: s.free_bytes,
                snapshot_bytes: s.snapshot_bytes,
                disk_free: s.disk_free,
            })
        })
    }

    /// Rewrites the vault file without its free pages (0004 §1.4) and
    /// returns its size before and after; `None` when nothing was free.
    /// [`StoreError::StorageCapacity`] when the volume lacks room for the
    /// copy. A recovery session (writes blocked) cannot compact.
    pub fn compact(&self, cancel: Option<&CancelToken>) -> Result<Option<(u64, u64)>, CoreError> {
        self.read(|ctx| {
            if ctx.open.read_only {
                return Err(StoreError::NoAnchor.into());
            }
            ctx.check_cancel()?;
            let c = ctx.canceller();
            let done = ctx
                .open
                .vault
                .compact(&|| c.is_cancelled() || cancel.is_some_and(CancelToken::is_cancelled))?;
            Ok(done.map(|d| (d.before, d.after)))
        })
    }

    /// The automatic policy's outcome for this session, for the frontend's
    /// stderr warnings (0011 §4). `None` before an unlock and on a locked or
    /// never-unlocked session.
    pub fn snapshot_status(&self) -> Option<SnapshotReport> {
        self.read(|ctx| Ok(ctx.open.auto_snapshot.clone()))
            .ok()
            .flatten()
    }

    /// Runs the automatic ordinary-snapshot policy (0007 §3.2). Best effort:
    /// any failure is reported as a warning through
    /// [`Self::snapshot_status`] and never fails the unlock.
    pub(crate) fn run_automatic_snapshot(&self, cancel: Option<&CancelToken>) {
        self.run_automatic_snapshot_cancellable(&|| cancel.is_some_and(CancelToken::is_cancelled));
    }

    /// LAN also supplies its absolute window lifetime to snapshot copying.
    pub(crate) fn run_automatic_snapshot_cancellable(&self, cancel: &dyn Fn() -> bool) {
        let _ = self.read(|ctx| {
            let c = ctx.canceller();
            let now = ctx.now();
            let open: &mut Open = ctx.open;
            let Some(policy) = open.snapshots.clone() else {
                return Ok(());
            };
            let pinned = open.pinned();
            let cancelled = move || c.is_cancelled() || cancel();
            if cancelled() {
                return Ok(());
            }
            // Ordinary snapshots are kept by commits inside the vault (plan
            // 0059); this pass only ages out the files written before.
            if policy.dir.entries().is_ok_and(|e| e.is_empty()) {
                return Ok(());
            }
            let report = match snapshot_pass(
                &policy,
                &mut open.vault,
                None,
                now,
                false,
                pinned.as_deref(),
                &cancelled,
            ) {
                Ok(taken) => taken.report,
                Err(_) => SnapshotReport::failed(),
            };
            if report.failed || !report.pruned.is_empty() {
                open.auto_snapshot = Some(report);
            }
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Env, add_entry, err, kdf, pw, seq};
    use crate::{CoreError, SnapshotSkip};

    fn policy(env: &Env, max_count: u32, max_bytes: u64, max_age_seconds: i64) -> SnapshotPolicy {
        SnapshotPolicy {
            dir: SnapshotDir::open(&env.dir.path().join("snapshots")).unwrap(),
            max_count,
            max_bytes,
            max_age_seconds,
        }
    }

    fn ids(listing: &SnapshotListing) -> Vec<String> {
        listing
            .snapshots
            .iter()
            .map(|s| s.archive_id.clone())
            .collect()
    }

    const HOUR: i64 = 60 * 60 * MILLIS_PER_SECOND;

    /// Sets the clock to `hours`:`minutes` UTC on `day` and returns it.
    fn at(env: &Env, day: i64, hours: i64, minutes: i64) -> i64 {
        let t = day * MILLIS_PER_DAY + hours * HOUR + minutes * 60 * MILLIS_PER_SECOND;
        env.time.set(t);
        t
    }

    fn defaults(env: &Env) -> SnapshotPolicy {
        policy(
            env,
            DEFAULT_MAX_COUNT,
            DEFAULT_MAX_BYTES,
            DEFAULT_MAX_AGE_SECONDS,
        )
    }

    // 0012 §1: Snapshot disk growth
    #[test]
    fn snapshot_commits_keep_one_per_hour_and_first_of_day_and_reads_keep_none() {
        let env = Env::new();
        let t0 = at(&env, 20_000, 12, 0);
        let s = env.unlock_with(Some(defaults(&env)));
        // An unlock and reads write nothing (plan 0059).
        let seq0 = seq(&s);
        assert!(s.snapshots().unwrap().snapshots.is_empty());
        assert_eq!(seq(&s), seq0);
        let root = s.root().unwrap();
        add_entry(&s, root, "one");
        let Some(first) = s.snapshot_status().unwrap().published else {
            panic!("the first change of the day kept no snapshot");
        };
        assert_eq!(first.kind, SnapshotKind::Ordinary);
        assert_eq!(first.created_at, Timestamp(t0));
        let rows = s.snapshots().unwrap().snapshots;
        assert_eq!(rows.len(), 1);
        assert!(rows[0].objects > 0 && rows[0].total_bytes > 0);
        // Half an hour later: no second one.
        at(&env, 20_000, 12, 30);
        add_entry(&s, root, "two");
        assert_eq!(s.snapshots().unwrap().snapshots.len(), 1);
        // More than an hour later: a second one.
        at(&env, 20_000, 13, 1);
        add_entry(&s, root, "three");
        assert_eq!(s.snapshots().unwrap().snapshots.len(), 2);
        // The first change of the next day, minutes after: a third.
        let next = at(&env, 20_001, 0, 5);
        add_entry(&s, root, "four");
        let rows = s.snapshots().unwrap().snapshots;
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].created_at, Timestamp(next));
        assert!(rows.windows(2).all(|w| w[0].created_at >= w[1].created_at));
        // No snapshot file is written; the snapshots survive a reopen.
        let dir = env.dir.path().join("snapshots");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        drop(s);
        let s = env.unlock_with(Some(defaults(&env)));
        assert_eq!(s.snapshots().unwrap().snapshots.len(), 3);
        assert!(s.verify(None).unwrap().ok());
    }

    // 0012 §1: Snapshot disk growth
    #[test]
    fn snapshot_commits_prune_to_count_age_and_bytes() {
        let env = Env::new();
        at(&env, 21_000, 8, 0);
        let s = env.unlock_with(Some(policy(
            &env,
            2,
            DEFAULT_MAX_BYTES,
            DEFAULT_MAX_AGE_SECONDS,
        )));
        let root = s.root().unwrap();
        let mut taken = Vec::new();
        for i in 0..3 {
            at(&env, 21_000, 8 + 2 * i, 0);
            add_entry(&s, root, &format!("entry {i}"));
            taken.push(s.snapshot_status().unwrap().published.unwrap().archive_id);
        }
        // Count: the oldest went in the commit that kept the third.
        let rows = ids(&s.snapshots().unwrap());
        assert_eq!(rows.len(), 2);
        assert!(!rows.contains(&taken[0]));
        // Deleting a snapshot is a commit of its own.
        let before = seq(&s);
        assert_eq!(
            s.delete_snapshots(std::slice::from_ref(&taken[1])).unwrap(),
            1
        );
        assert_eq!(seq(&s), before + 1);
        assert_eq!(ids(&s.snapshots().unwrap()), vec![taken[2].clone()]);
        drop(s);

        // Age: with an hour's budget the next kept snapshot prunes the old.
        let s = env.unlock_with(Some(policy(&env, 48, DEFAULT_MAX_BYTES, 60 * 60)));
        at(&env, 21_000, 20, 0);
        add_entry(&s, s.root().unwrap(), "late");
        let report = s.snapshot_status().unwrap();
        assert_eq!(report.pruned, vec![taken[2].clone()]);
        assert_eq!(s.snapshots().unwrap().snapshots.len(), 1);
        drop(s);

        // Room: a snapshot the store skipped for lack of room is reported,
        // and its notice is handed out once (store tests force the room).
        let s = env.unlock_with(Some(env.snapshot_policy()));
        s.read(|ctx| {
            let outcome = qv_store::SnapshotOutcome {
                skipped_for_space: true,
                needed: 5,
                available: 2,
                ..qv_store::SnapshotOutcome::default()
            };
            super::record_outcome(ctx.open, &outcome);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            s.snapshot_status().unwrap().skipped,
            Some(SnapshotSkip::NoRoom {
                needed: 5,
                available: 2
            })
        );
        let space = super::SnapshotSpace {
            needed: 5,
            available: 2,
        };
        assert_eq!(s.take_snapshot_skipped(), Some(space));
        assert_eq!(s.take_snapshot_skipped(), None);
    }

    // 0012 §1: Rotation/purge ambiguity
    #[test]
    fn permanent_deletion_takes_no_copy_and_removes_the_snapshots_holding_it_unless_kept() {
        let env = Env::new();
        at(&env, 25_000, 9, 0);
        let s = env.unlock_with(Some(env.snapshot_policy()));
        assert_eq!(err(s.adopt_baseline()), CoreError::NotAllowed);
        let root = s.root().unwrap();
        let a = add_entry(&s, root, "first");
        let b = add_entry(&s, root, "second");
        at(&env, 25_000, 11, 0);
        s.delete(crate::ItemHandle::Entry(a), false).unwrap();
        at(&env, 25_000, 13, 0);
        s.delete(crate::ItemHandle::Entry(b), false).unwrap();
        // The 11:00 and 13:00 snapshots hold `a` (live, then trashed).
        assert_eq!(s.snapshots().unwrap().snapshots.len(), 3);
        assert_eq!(
            s.snapshots_holding(DeletionTarget::Items(&[crate::ItemHandle::Entry(a)]))
                .unwrap(),
            2
        );
        // Kept on request: one commit, no copy, no snapshot removed.
        let before = seq(&s);
        s.purge(&[crate::ItemHandle::Entry(a)], true).unwrap();
        assert_eq!(seq(&s), before + 1);
        assert_eq!(s.snapshots().unwrap().snapshots.len(), 3);
        // Otherwise the snapshots holding `b` go, and none is kept.
        assert_eq!(s.snapshots_holding(DeletionTarget::Trash).unwrap(), 2);
        at(&env, 25_000, 15, 0);
        assert_eq!(s.empty_trash(false).unwrap().purged, 1);
        assert_eq!(s.snapshots().unwrap().snapshots.len(), 1);
        assert_eq!(s.snapshots_holding(DeletionTarget::Trash).unwrap(), 0);
        // No recovery file was written for any of it.
        let dir = env.dir.path().join("snapshots");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert!(s.verify(None).unwrap().ok());
    }

    #[test]
    fn recovery_snapshot_needs_approval_and_never_writes_the_vault() {
        let env = Env::new();
        let before = std::fs::read(&env.path).unwrap();
        let s = env.unlock_with(Some(policy(&env, 48, 1, DEFAULT_MAX_AGE_SECONDS)));
        let seq_before = seq(&s);
        match err(s.begin_destructive(false, None)) {
            CoreError::SnapshotOverBudget { needed } => assert!(needed > 0),
            other => panic!("expected a budget refusal, got {other:?}"),
        }
        // The refused copy is removed and nothing is pinned or listed.
        assert!(s.snapshots().unwrap().snapshots.is_empty());
        assert_eq!(seq(&s), seq_before);

        // Explicit approval takes it anyway (0007 §3.3); it stays pinned.
        let snap = s.begin_destructive(true, None).unwrap();
        assert_eq!(snap.summary().kind, SnapshotKind::Recovery);
        let rows = s.snapshots().unwrap().snapshots;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, SnapshotKind::Recovery);
        // Only one serialized recovery operation may pin a snapshot.
        assert_eq!(err(s.begin_destructive(true, None)), CoreError::NotAllowed);
        s.finish_destructive(snap).unwrap();
        assert_eq!(
            s.snapshots().unwrap().snapshots[0].kind,
            SnapshotKind::Ordinary
        );

        // Dropping an unfinished handle releases the pin.
        let dropped = s.begin_destructive(true, None).unwrap();
        drop(dropped);
        assert!(s.begin_destructive(true, None).is_ok());
        assert_eq!(seq(&s), seq_before);
        assert_eq!(std::fs::read(&env.path).unwrap(), before);
    }

    #[test]
    fn snapshot_without_a_policy_is_disabled() {
        let env = Env::new();
        let s = env.unlock_with(None);
        assert_eq!(
            s.snapshot_status().unwrap().skipped,
            Some(SnapshotSkip::Disabled)
        );
        assert!(s.snapshots().unwrap().snapshots.is_empty());
        assert_eq!(err(s.begin_destructive(false, None)), CoreError::NotAllowed);
        assert_eq!(s.delete_snapshots(&["00".repeat(16)]).unwrap(), 0);
        assert!(s.snapshots_before(Timestamp(i64::MAX)).unwrap().is_empty());
    }

    #[test]
    fn recovery_copy_left_by_a_crash_is_reclassified_before_pruning() {
        let env = Env::new();
        let t0 = at(&env, 23_000, 10, 0);
        let s = env.unlock_with(Some(defaults(&env)));
        // A crash: the handle of a rotation-style recovery file is dropped
        // without finish_destructive; a second one completes.
        let crashed = s.begin_destructive(false, None).unwrap();
        let crashed_id = crashed.summary().archive_id.clone();
        drop(crashed);
        let rows = s.snapshots().unwrap().snapshots;
        let crashed_row = rows
            .iter()
            .find(|r| r.archive_id == crashed_id)
            .expect("the crashed copy is not listed");
        assert_eq!(crashed_row.kind, SnapshotKind::Recovery);
        // The next copy's own pass reclassifies the unpinned one first.
        let done = s.begin_destructive(false, None).unwrap();
        s.finish_destructive(done).unwrap();
        assert!(
            s.snapshots()
                .unwrap()
                .snapshots
                .iter()
                .all(|r| r.kind == SnapshotKind::Ordinary)
        );
        drop(s);

        // The unlock pass reclassifies it and then prunes the files like any
        // ordinary snapshot; it writes no new file (plan 0059).
        env.time.set(t0 + HOUR);
        let s = env.unlock_with(Some(policy(
            &env,
            1,
            DEFAULT_MAX_BYTES,
            DEFAULT_MAX_AGE_SECONDS,
        )));
        let rows = s.snapshots().unwrap().snapshots;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, SnapshotKind::Ordinary);
        assert_eq!(s.snapshot_status().unwrap().pruned.len(), 1);
    }

    #[test]
    fn snapshot_listing_ignores_unauthenticated_files() {
        let env = Env::new();
        let s = env.unlock_with(Some(env.snapshot_policy()));
        add_entry(&s, s.root().unwrap(), "kept");
        let healthy = s.snapshots().unwrap();
        assert_eq!(healthy.snapshots.len(), 1);
        assert_eq!(healthy.unreadable, 0);
        let dir = env.dir.path().join("snapshots");
        // A garbage file with a valid snapshot name, and one of another kind.
        let name = "0123456789abcdef0123456789abcdef.qvsnap";
        std::fs::write(dir.join(name), b"not an archive").unwrap();
        std::fs::write(dir.join(format!("recover-{name}")), b"nor this").unwrap();
        // Files that are not snapshot names are not even considered.
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let listing = s.snapshots().unwrap();
        assert_eq!(listing.snapshots.len(), 1);
        assert_eq!(listing.unreadable, 2);
        let rows = s
            .snapshots_before(Timestamp(healthy.snapshots[0].created_at.0 + 1))
            .unwrap();
        assert_eq!(rows.len(), 1);
        let rows = s
            .snapshots_before(Timestamp(healthy.snapshots[0].created_at.0))
            .unwrap();
        assert!(rows.is_empty());
        assert_eq!(s.delete_snapshots(&["f".repeat(32)]).unwrap(), 0);
    }

    #[test]
    fn verify_report_counts_anchor_and_never_mutates() {
        let env = Env::new();
        let s = env.unlock_with(Some(env.snapshot_policy()));
        let before = std::fs::read(&env.path).unwrap();
        let seq_before = seq(&s);
        let report = s.verify(None).unwrap();
        assert!(report.ok(), "{report:?}");
        assert_eq!(report.anchor, AnchorStatus::Verified);
        assert_eq!(report.local_seq, seq_before);
        assert_eq!(report.vault_id.len(), 32);
        assert_eq!(report.key_generation, 1);
        assert_eq!(report.counts.objects, report.counts.records);
        assert_eq!(report.counts.active_heads, report.counts.records);
        assert_eq!(report.counts.retained_versions, 0);
        assert_eq!(report.counts.tombstones, 0);
        assert_eq!(report.counts.suppressed_records, 0);
        assert!(report.counts.total_bytes > 0);
        assert!(report.notes.is_empty());
        assert_eq!(std::fs::read(&env.path).unwrap(), before);

        // A purge floor covering a dominated version is expected state: it is
        // counted and labelled, never reported as damage.
        let root = s.root().unwrap();
        let e = add_entry(&s, root, "Gone");
        s.delete(crate::ItemHandle::Entry(e), false).unwrap();
        s.empty_trash(false).unwrap();
        let report = s.verify(None).unwrap();
        assert!(report.ok(), "{report:?}");
        assert!(report.counts.suppressed_records >= 1);
        assert_eq!(
            report.notes,
            vec![VerifyNote::SuppressionFloorCoversDominatedVersions]
        );
        assert_eq!(
            VerifyNote::SuppressionFloorCoversDominatedVersions.label(),
            "suppression-floor-covers-dominated-versions"
        );
    }

    #[test]
    fn snapshot_pruning_breaks_equal_timestamps_by_archive_id() {
        let env = Env::new();
        let t0 = at(&env, 24_000, 6, 0);
        // Three finished recovery files carry the same creation time.
        let s = env.unlock_with(Some(defaults(&env)));
        let mut same_time = Vec::new();
        for _ in 0..3 {
            let snap = s.begin_destructive(false, None).unwrap();
            same_time.push(snap.summary().archive_id.clone());
            s.finish_destructive(snap).unwrap();
        }
        let rows = s.snapshots().unwrap().snapshots;
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.created_at == Timestamp(t0)));
        drop(s);

        // A pass with a count budget of two keeps the two highest IDs.
        env.time.set(t0 + HOUR);
        let s = env.unlock_with(Some(policy(
            &env,
            2,
            DEFAULT_MAX_BYTES,
            DEFAULT_MAX_AGE_SECONDS,
        )));
        same_time.sort();
        let rows = ids(&s.snapshots().unwrap());
        assert_eq!(rows.len(), 2);
        assert!(rows.contains(&same_time[1]) && rows.contains(&same_time[2]));
        assert!(!rows.contains(&same_time[0]));
    }

    #[test]
    fn verify_reports_a_missing_anchor_and_keeps_checking() {
        let env = Env::new();
        for e in std::fs::read_dir(&env.anchors).unwrap() {
            std::fs::remove_file(e.unwrap().path()).unwrap();
        }
        let core: crate::Core = env.core();
        let _k = kdf();
        let (s, status) = core
            .unlock_for_recovery(&env.path, &pw(crate::testutil::PASSWORD), None)
            .unwrap();
        assert_eq!(status, AnchorStatus::Missing);
        let report = s.verify(None).unwrap();
        assert_eq!(report.anchor, AnchorStatus::Missing);
        assert!(!report.ok());
        assert!(report.counts.objects > 0);
        assert_eq!(report.notes, Vec::new());
    }

    #[test]
    fn verify_cancellation_is_an_error() {
        let env = Env::new();
        let s = env.unlock_with(None);
        let token = crate::CancelToken::new();
        token.cancel();
        assert_eq!(
            err(s.verify(Some(&token))),
            CoreError::Store(StoreError::Io)
        );
    }

    // 0012 §1: Live SQLite copying
    #[test]
    fn live_sqlite_copying_snapshots_never_copy_or_write_the_vault() {
        // 0012 §1 row "Live SQLite copying": the copy is a new archive stream
        // of verified objects, never a copy or normalization of the database,
        // and taking it writes no vault state.
        let env = Env::new();
        let before = std::fs::read(&env.path).unwrap();
        let s = env.unlock_with(Some(env.snapshot_policy()));
        let seq_before = seq(&s);
        let snapshot = s.begin_destructive(false, None).unwrap();
        s.finish_destructive(snapshot).unwrap();
        assert_eq!(seq(&s), seq_before, "a snapshot committed vault state");
        assert_eq!(
            std::fs::read(&env.path).unwrap(),
            before,
            "the vault file changed"
        );
        let dir = env.dir.path().join("snapshots");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        files.sort();
        assert!(!files.is_empty());
        for path in files {
            let bytes = std::fs::read(path).unwrap();
            assert_eq!(&bytes[..7], b"QVSNAP\0");
            assert_ne!(bytes, before);
            assert!(
                !bytes.windows(16).any(|w| w == b"SQLite format 3\0"),
                "a snapshot contains SQLite pages"
            );
        }
    }

    // 0012 §1: Snapshot disk growth
    #[test]
    fn snapshot_disk_growth_a_full_disk_aborts_the_mandatory_copy() {
        let env = Env::new();
        let s = env.unlock_with(Some(policy(
            &env,
            48,
            DEFAULT_MAX_BYTES,
            DEFAULT_MAX_AGE_SECONDS,
        )));
        let dir = env.dir.path().join("snapshots");
        let seq_before = seq(&s);
        // An unsafe private directory refuses the copy outright; this check
        // does not depend on the process's privileges.
        crate::testutil::set_mode(&dir, 0o777);
        assert_eq!(
            err(s.begin_destructive(false, None)),
            CoreError::Store(StoreError::UnsafeFile)
        );
        // A read-only directory is the same abort through the I/O path, where
        // the process cannot override permissions (ordinary unprivileged
        // runs). The store's writer covers the disk-full error path itself.
        crate::testutil::set_mode(&dir, 0o500);
        let denied = std::fs::write(dir.join("probe"), b"").is_err();
        let _ = std::fs::remove_file(dir.join("probe"));
        if denied {
            assert_eq!(
                err(s.begin_destructive(false, None)),
                CoreError::Store(StoreError::Io)
            );
        }
        crate::testutil::set_mode(&dir, 0o700);
        assert_eq!(seq(&s), seq_before);
        assert!(s.begin_destructive(true, None).is_ok());
    }
    #[test]
    fn snapshot_publication_failure_preserves_expired_recovery_material() {
        let env = Env::new();
        let s = env.unlock_with(Some(env.snapshot_policy()));
        let snap = s.begin_destructive(false, None).unwrap();
        let old = snap.summary().archive_id.clone();
        s.finish_destructive(snap).unwrap();
        drop(s);
        env.time.set(i64::MAX / 2);
        let configured = policy(&env, 48, DEFAULT_MAX_BYTES, 1);
        let dir = configured.dir.path().to_owned();
        let core = env.core_with(Some(configured));
        let _k = kdf();
        crate::testutil::set_mode(&dir, 0o777);
        let s = core
            .unlock(&env.path, &pw(crate::testutil::PASSWORD), None)
            .unwrap();
        // Pruning cannot proceed in an unsafe directory; nothing is removed.
        assert!(
            s.snapshot_status()
                .is_none_or(|r| r.failed || r.pruned.is_empty())
        );
        crate::testutil::set_mode(&dir, 0o700);
        assert_eq!(ids(&s.snapshots().unwrap()), vec![old]);
    }
    #[test]
    fn snapshot_restore_preserves_source_and_defers_pruning_the_selected_input() {
        let env = Env::new();
        let s = env.unlock_with(Some(env.snapshot_policy()));
        add_entry(&s, s.root().unwrap(), "from snapshot");
        let snap = s.begin_destructive(false, None).unwrap();
        let id = snap.summary().archive_id.clone();
        s.finish_destructive(snap).unwrap();
        let before = std::fs::read(&env.path).unwrap();
        let anchors: Vec<_> = std::fs::read_dir(&env.anchors)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (p.clone(), std::fs::read(p).unwrap())
            })
            .collect();
        drop(s);
        env.time.set(i64::MAX / 2);
        let core = env.core_with(Some(policy(&env, 48, DEFAULT_MAX_BYTES, 1)));
        let s = {
            let _k = kdf();
            core.unlock_for_snapshot_restore(&env.path, &pw(crate::testutil::PASSWORD), None)
                .unwrap()
        };
        assert!(
            s.snapshot_status().is_none(),
            "automatic pruning ran before restore"
        );
        assert!(
            s.snapshot_matches(&format!("@{}", &id[..6]))
                .unwrap()
                .contains(&id)
        );
        let target = env.path.with_file_name("restored.qvault");
        s.restore_snapshot(&id.to_ascii_uppercase(), &target, None)
            .unwrap();
        assert_eq!(std::fs::read(&env.path).unwrap(), before);
        for (p, bytes) in anchors {
            assert_eq!(std::fs::read(p).unwrap(), bytes);
        }
        let restored = {
            let _k = kdf();
            core.unlock(&target, &pw(crate::testutil::PASSWORD), None)
                .unwrap()
        };
        assert_eq!(restored.overview().unwrap().local_seq, 1);
        assert_eq!(restored.search("from snapshot").unwrap().len(), 1);
        assert!(restored.verify(None).unwrap().ok());
        assert!(
            s.snapshot_status().is_some(),
            "deferred policy did not resume after success"
        );
    }

    #[test]
    fn snapshot_restore_rejects_invalid_foreign_missing_and_cancelled_inputs() {
        let env = Env::new();
        let s = env.unlock_with(Some(env.snapshot_policy()));
        add_entry(&s, s.root().unwrap(), "kept");
        let id = s.snapshots().unwrap().snapshots[0].archive_id.clone();
        let target = env.path.with_file_name("restored.qvault");
        assert_eq!(err(s.snapshot_matches("xyz")), CoreError::InvalidInput);
        assert_eq!(
            err(s.restore_snapshot("123456", &target, None)),
            CoreError::InvalidInput
        );
        assert_eq!(
            err(s.restore_snapshot(&"f".repeat(32), &target, None)),
            CoreError::NotFound
        );
        let token = crate::CancelToken::new();
        token.cancel();
        assert!(s.restore_snapshot(&id, &target, Some(&token)).is_err());
        assert!(!target.exists());
        let foreign = "abcdef1234567890abcdef1234567890";
        let path = env
            .dir
            .path()
            .join("snapshots")
            .join(format!("{foreign}.qvsnap"));
        std::fs::write(&path, b"QVSNAP\0garbage").unwrap();
        assert_eq!(
            s.snapshot_matches("abcdef").unwrap(),
            vec![foreign.to_owned()]
        );
        assert!(s.restore_snapshot(foreign, &target, None).is_err());
        assert!(!target.exists());
        s.lock();
        assert_eq!(
            err(s.restore_snapshot(&id, &target, None)),
            CoreError::SessionLocked
        );
    }
    #[test]
    fn snapshot_restore_reads_the_vaults_own_snapshot_into_a_new_vault() {
        let env = Env::new();
        at(&env, 26_000, 9, 0);
        let s = env.unlock_with(Some(env.snapshot_policy()));
        let root = s.root().unwrap();
        add_entry(&s, root, "before");
        at(&env, 26_000, 11, 0);
        add_entry(&s, root, "later");
        let rows = s.snapshots().unwrap().snapshots;
        assert_eq!(rows.len(), 2);
        // The newest (11:00) snapshot holds "before" but not "later".
        let id = rows[0].archive_id.clone();
        let before = std::fs::read(&env.path).unwrap();
        let target = env.path.with_file_name("restored.qvault");
        s.restore_snapshot(&id, &target, None).unwrap();
        assert_eq!(std::fs::read(&env.path).unwrap(), before);
        assert!(!target.with_file_name("restored.qvault.qvsnap").exists());
        let core = env.core_with(Some(env.snapshot_policy()));
        let restored = {
            let _k = kdf();
            core.unlock(&target, &pw(crate::testutil::PASSWORD), None)
                .unwrap()
        };
        assert_eq!(restored.search("before").unwrap().len(), 1);
        assert!(restored.search("later").unwrap().is_empty());
        assert!(restored.snapshots().unwrap().snapshots.is_empty());
        assert!(restored.verify(None).unwrap().ok());
    }
}
