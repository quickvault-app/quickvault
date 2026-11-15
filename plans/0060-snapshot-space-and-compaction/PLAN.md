# Plan 0060 — Snapshot space, snapshot deletion and vault compaction

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| Implemented | 2026-11-15 | 2026-11-15 (owner: "You can go ahead and do it") | 0004 §1.4 (new), §4.1; 0007 §3.3; 0009 §1.8; 0011 §4; 0012 §1 |

## Intent

The owner wants snapshots to use whatever room the vault does not need instead
of a fixed 1 GiB, to be told when a snapshot was skipped, to be able to delete
one or all snapshots, and to get disk space back when data is deleted (today
the SQLite file never shrinks). Age stays 30 days, count stays 48.

## Scope

**In scope:**
- Snapshot room: snapshot-only data may use the room between the vault's
  needs and the lesser of the file cap and the device's free-disk allowance;
  any commit prunes the oldest snapshots when they no longer fit.
- The app's notice when a commit skipped its snapshot for space; clearer CLI
  warning.
- Deleting one snapshot or all of them (app and CLI).
- Compaction: rewriting the vault file without free pages, automatically after
  a commit leaves a large share free, and on request (app and CLI).
- A storage overview (file size, reclaimable space, snapshot space).

**Out of scope:**
- The footer History link (owner: keep it).
- Changing the 30-day age or 48 count (owner: 30 days is fine).
- `auto_vacuum`/incremental vacuum (a format change; whole-file compaction is
  enough with the threshold below).
- Snapshot files from before plan 0059 and the rotation's recovery archive:
  their file budget (`SnapshotPolicy::max_bytes`) is unchanged.

## Spec basis

| Behaviour | Spec | Spec status | Notes |
|-----------|------|-------------|-------|
| Snapshot budgets and pruning | 0007 §3.3 | Accepted | Amended in Step 1: room rule, pruning in any commit |
| Deleting snapshots | 0007 §3.3, 0011 §4 | Accepted | Amended: explicit deletion of one or all |
| Compaction | 0004 §1 | Accepted | New §1.4; §1.2 hardening exception for one statement |
| Frontend surfaces | 0009 §1.8 | Accepted | Amended: notice, delete, storage, compact |
| CLI | 0011 §4 | Accepted | `snapshot rm`, `compact` |
| Validation | 0012 §1 | Accepted | "Snapshot disk growth" row reworded; compaction crash row added |

**Spec changes required:** Step 1 (0004, 0007, 0009, 0011, 0012 with
Changelog lines).

## Decisions needed

None open. Owner approved the direction on 2026-11-15; the numbers below are
owner-delegated (standing delegation 2026-11-02) and recorded here:

1. **Disk floor:** none (owner, 2026-11-15, second revision): QuickVault
   reserves no share of the disk; the room is the file cap or the file's size
   plus all free space. A save that fails for a full disk points to deleting
   snapshots and compaction. (Earlier: max(2 GiB, 10%) ≤ 32 GiB, then 5%
   within 2–4 GiB; see Revisions.)
2. **Overhead allowance:** a commit fits when `(L + S) + (L + S)/64 + 64 MiB`
   is within the room (`L` live bytes, `S` snapshot-only bytes).
3. **Automatic compaction:** after a commit whose file then has at least
   32 MiB and at least 25% of its pages free; never when the copy would leave
   less than 256 MiB free on the volume. Silent best effort.
4. **Manual compaction** runs whenever any page is free and the copy fits on
   the volume with 256 MiB to spare.

## Design

**Components touched:** qv-store (snapshot planning, new `space` and
`compact` modules, fs replace helper), qv-win (disk-space FFI), qv-core
(policy, storage, compaction, notices), qv-cli, qv-app. Trust boundaries are
unchanged: compaction rewrites ciphertext rows only and never decrypts beyond
the control rows it already authenticates.

**Interfaces:**

```rust
// qv-store
pub struct SnapshotBudgets { pub max_count: u32, pub max_age_ms: i64 } // max_bytes removed
pub struct SnapshotOutcome { .., pub skipped_for_space: bool,
    pub needed: u64,     // snapshot-only bytes the skipped snapshot needed
    pub available: u64,  // what snapshots could use at that commit
    pub removed: Vec<SnapshotRow> }
pub struct FileSpace { pub file_bytes: u64, pub free_bytes: u64,
    pub snapshot_bytes: u64, pub disk_free: Option<u64> }
pub struct Compaction { pub before: u64, pub after: u64 }
impl UnlockedVault {
    pub fn file_space(&self) -> Result<FileSpace, StoreError>;
    pub fn compact(&mut self, cancelled: &dyn Fn() -> bool) -> Result<Compaction, StoreError>;
}
// qv-win
pub fn disk_space(path: &Path) -> io::Result<(u64 /*free to caller*/, u64 /*total*/)>;
// qv-core
pub struct StorageInfo { pub file_bytes: u64, pub free_bytes: u64,
    pub snapshot_bytes: u64, pub disk_free: Option<u64> }
impl Session {
    pub fn storage(&self) -> Result<StorageInfo, CoreError>;
    pub fn compact(&self, cancel: Option<&CancelToken>) -> Result<Compaction, CoreError>;
    pub fn take_snapshot_skipped(&self) -> Option<SnapshotSpace>; // once per skip
}
pub struct SnapshotSpace { pub needed: u64, pub available: u64 }
```

**Snapshot room (0007 §3.3 as amended).** At every commit of a file with a
snapshot index the store computes:
- `L`: the new live manifest's referenced ciphertext plus its sealed length;
- `room = min(MAX_DB_BYTES, file_bytes + disk_free)` (the disk term is
  omitted when free space cannot be measured; no share of the disk is
  reserved);
- `S`: snapshot-only bytes, cheaply as the stored lengths of retained objects
  the new live manifest does not reference plus kept manifest rows other than
  the live one (SQL `octet_length`, no manifest decryption).

When nothing else was asked of the snapshots and `S` fits, nothing changes.
Otherwise the full planner runs (as in plan 0059) and drops the oldest
snapshots until count (when keeping), age (when keeping), room and the
retained cap fit. A snapshot this commit keeps is never pruned by it: an
ordinary one that cannot fit alone is skipped and reported with `needed` and
`available`; a recovery one is always kept. Room pruning applies to every
commit; age and count pruning still run only in a commit that keeps a
snapshot.

**Compaction (0004 §1.4, new).** `UnlockedVault::compact`:
1. Requires the write gate open (no pending anchor, not blocked), identity
   unchanged, no read transaction, and a free page. Removes stale
   `.<name>.tmp-*` siblings first.
2. Measures `copy = (page_count − freelist_count) × 4096`; refuses with
   `StorageCapacity` unless the volume has `copy + 256 MiB` free.
3. `fs::create_temp(vault)` makes the private, locked, empty
   `.<name>.tmp-<hex>` sibling. The session connection raises
   `SQLITE_LIMIT_ATTACHED` to 1, runs `VACUUM INTO ?1` (bound path), and
   restores the limit to 0 and reads it back; a failed restore blocks the
   session's writes (`Hardening`). The step budget is reset before.
4. fsyncs the copy, opens it as a `VaultFile` under its lock and requires byte
   equality of the header, header MAC, slots, `local_head`, `replica`, the
   snapshot index row and the live manifest row, plus equal row counts and
   total `octet_length` of `object` and `manifest`. Then closes the copy's
   connection, keeping its lock.
5. Renames the copy over the vault path (Unix `rename`; Windows closes the
   session connection first and uses `MoveFileExW` with
   `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`), syncs the directory
   and reopens the session's `VaultFile` at the vault path under the copy's
   lock, checking the header, MAC and slots again.
6. Crash points: before the rename the old file is the vault and the copy is a
   stale temp (removed by the next compaction or open); after it the copy is
   the vault. Both are complete, identical states with the same head, so the
   external anchor is unchanged and no journal is needed. A failure before the
   rename removes the copy; a failure after it leaves the session unable to
   write (identity check), and the caller reports it.

`VaultFile::open` also removes stale temps best effort.

**Automatic compaction (core).** After a mutation's commits, when the session
is writable and `free ≥ 32 MiB && free × 4 ≥ file`, core runs `compact`;
failures (including too little disk space) are ignored and retried after a
later commit.

**Deletion of snapshots.** `Session::delete_snapshots` already removes named
in-vault snapshots in one commit (and legacy files). The CLI adds
`qv snapshot rm <id>… | --all` (confirmation, `--yes`); the app adds a Delete
button per row and "Delete all…" with a confirmation naming the count and the
space it frees. After deletion automatic compaction applies.

**Notices.** `after_mutation` stores a `SnapshotSpace` when a commit skipped
its snapshot; `take_snapshot_skipped` hands it out once. The app shows it in
place of the operation's notice: "Saved, but there wasn't enough free space to
keep a snapshot of the vault before this change." The CLI warning names both
numbers.

**Storage pane.** Settings → Vault → "Storage": file size, reclaimable space
inside the file, space snapshots use, free disk space, and "Compact now"
(disabled when nothing is reclaimable).

**Secrets and hostile input:** compaction copies ciphertext pages; the copy is
owner-only from creation (`create_temp`), and its verification uses the same
hardened open as any vault. No new parsing of untrusted data. The temporary
attach limit is raised only around one fixed statement with a bound
parameter on our own validated connection.

**Errors:** compaction maps a full disk to `StorageCapacity` (CLI exit 1 with
fixed guidance), an open anchor gate to the existing pending/blocked errors,
and verification mismatches to `Integrity` (copy removed, vault untouched).

**Dependencies:** none new. `rustix` (already a qv-store dependency) gains
nothing; `fstatvfs` is in its `fs` feature. qv-win uses `GetDiskFreeSpaceExW`
from the already enabled `Win32_Storage_FileSystem` feature.

## Steps

### Step 1 — Spec amendments

- [x] **Do:** 0007 §3.3 (room rule, floor, pruning in any commit, skip notice,
  explicit deletion); 0004 new §1.4 Compaction plus a §1.2 sentence for the
  one-statement attach exception, §4.1 wording; 0009 §1.8 Snapshots bullet
  (delete, notice, Storage pane, compaction); 0011 §4 (`snapshot rm`,
  `compact`); 0012 §1 rows. Changelog lines.
- **Files:** `specs/0004-local-storage.md`, `specs/0007-history-and-backups.md`,
  `specs/0009-clients-and-platforms.md`, `specs/0011-cli.md`,
  `specs/0012-security-validation.md`
- **Done when:** every behaviour below cites amended text.
- **Verify:** reading; `sh scripts/check-matrix.sh` after Step 8.

### Step 2 — Disk space and file space

- [x] **Do:** qv-win `disk_space`; qv-store `space.rs` with `disk_space(fd/path)`,
  `disk_floor(total)`, `file_space` (page_count, freelist_count) and the
  cheap `snapshot_only_bytes`.
- **Files:** `crates/qv-win/src/fs.rs`, `crates/qv-store/src/space.rs`,
  `crates/qv-store/src/lib.rs`
- **Done when:** unit tests cover the floor bounds and file-space counts.
- **Verify:** `cargo test --locked -p qv-store space`

### Step 3 — Room-based snapshot pruning

- [x] **Do:** drop `max_bytes` from `SnapshotBudgets`; pass `room` into the
  planner; run room pruning in commits that ask nothing else; report
  `available`; core policy and callers updated.
- **Files:** `crates/qv-store/src/snapshot_commit.rs`, `write.rs`,
  `snapshot_tests.rs`, `crates/qv-core/src/snapshot.rs`
- **Done when:** tests show: a kept snapshot beyond the old 1 GiB-style budget
  stays when room allows; a commit with no snapshot request prunes the oldest
  when room shrinks (test hook for room); a too-large ordinary snapshot is
  skipped with `needed`/`available`; recovery kept.
- **Verify:** `cargo test --locked -p qv-store snapshot`, `-p qv-core snapshot`

### Step 4 — Compaction in the store

- [x] **Do:** `compact.rs` as designed; `fs::replace` (Unix/Windows);
  stale-temp removal in `VaultFile::open`; crash hooks before/after rename.
- **Files:** `crates/qv-store/src/compact.rs`, `fs.rs`, `fs_windows.rs`,
  `vault.rs`, `lib.rs`, `crates/qv-win/src/fs.rs`
- **Done when:** tests: compaction shrinks a file after deleting a large
  attachment, keeps the head/anchor and unlocks again with all data; a crash
  before the rename leaves the old file valid and the temp is cleaned; a crash
  after leaves the compacted file valid; a pending anchor gate refuses;
  verification mismatch (test hook) removes the copy and changes nothing.
- **Verify:** `cargo test --locked -p qv-store compact`

### Step 5 — Core: storage, compaction, notices

- [x] **Do:** `Session::storage`, `compact`, automatic compaction after
  mutations, `take_snapshot_skipped`.
- **Files:** `crates/qv-core/src/snapshot.rs`, `session.rs`, `lib.rs`
- **Done when:** core tests cover automatic compaction after emptying a trash
  that frees a large attachment, and the skip notice taken once.
- **Verify:** `cargo test --locked -p qv-core`

### Step 6 — CLI

- [x] **Do:** `qv snapshot rm <id>… | --all` and `qv compact`; warning text.
- **Files:** `crates/qv-cli/src/cli.rs`, `cmd/snapshot.rs`, `cmd/mod.rs`,
  new `cmd/compact.rs`, `tests/snapshots.rs`
- **Done when:** CLI tests cover rm by prefix, `--all`, confirmation and
  `--yes`, ambiguity (5), absence (4), and compact output.
- **Verify:** `cargo test --locked -p qv-cli --test snapshots`

### Step 7 — App

- [x] **Do:** Snapshots page summary, per-row Delete, Delete all with confirm;
  Storage pane with Compact; skip notice after operations.
- **Files:** `crates/qv-app/src/workspace.rs`, `ops.rs`, `views.rs`,
  `ui/pages.rs`, `ui/shell.rs`, `workspace_tests.rs`
- **Done when:** workspace tests cover delete one/all and compact; validation
  clippy/tests pass.
- **Verify:** qv-app clippy with `validation` and `desktop`; validation tests.

### Step 8 — Documentation and gates

- [x] **Do:** CHANGELOG, ARCHITECTURE (qv-store row), plans index; all gates.
- **Files:** `CHANGELOG.md`, `ARCHITECTURE.md`, `plans/README.md`
- **Verify:** all `AGENTS.md` gates.

## Test plan

- Final verification: all `AGENTS.md` gates plus the qv-app feature clippy
  and validation tests; step checks above are focused.
- Rejection/crash paths: compaction crash before/after rename, verification
  mismatch, pending anchor, insufficient disk (hook), restored attach limit.
- 0012 §1 rows: "Snapshot disk growth" (room pruning), new "Compaction
  interrupted" row.

## Risks and stop conditions

- If `VACUUM INTO` output differs in schema validation (e.g. page size or
  header fields) from what `validate_schema` accepts: stop, do not relax
  validation.
- Windows rename over a file our own handle holds may fail on some systems;
  CI's Windows job is the first compile/run there (no local MinGW). If CI
  fails, fix there rather than weakening the identity checks.
- Any need to keep a secret-bearing value during compaction: stop.

## Documentation

- [x] `CHANGELOG.md` — Unreleased: snapshot room, delete snapshots, compaction
- [x] `ARCHITECTURE.md` — qv-store row: compaction; snapshot prose; CLI list
- [x] Specs — Step 1
- [ ] `AGENTS.md` — no change

## Revisions

- 2026-11-15: Disk floor lowered from max(2 GiB, 10%) ≤ 32 GiB to 5% within
  2–4 GiB. The real-window flow showed the first rule switching snapshots
  off entirely on a large, nearly full development disk (about 10 GB free of
  about 1 TB): a 10% floor there exceeds the free space. Owner-delegated
  numbers; to be reported to the owner.
- 2026-11-15: Floor removed (owner: "QuickVault is not a storage manager. If
  the user wants to use that last 5% disk, so be it"). The room is
  `min(MAX_DB_BYTES, file size + free space)`; the full-disk error in the app
  and the CLI hint now point to deleting snapshots and compaction. No
  proactive low-disk warning (the OS already gives one; Storage shows free
  space). Compaction keeps its own check that the copy fits with 256 MiB to
  spare, since it writes a whole copy before the swap.

## Execution log

- 2026-11-15: Plan written and approved by the owner's go-ahead in the same
  conversation (30 days kept, footer link kept). A scratch check against the
  bundled SQLite 3.54.1 showed `VACUUM INTO` works under defensive mode with
  attach create/write disabled once `SQLITE_LIMIT_ATTACHED` is 1, keeps
  `application_id`/`user_version`, and accepts a pre-created empty target.
- 2026-11-15: Steps 1–7 done.
  - Step 1: 0004 §1.4 (new) and §1.2 sentence; 0007 §3.3 rewritten; 0009
    §1.8 Snapshots and new Storage bullets; 0011 `compact`, `snapshot rm`
    and the warning; 0012 "Snapshot disk growth" row reworded (no new matrix
    row, so `scripts/matrix-offline.txt` is unchanged).
  - Step 2: `qv_win::fs::disk_space` (`GetDiskFreeSpaceExW`, one SAFETY
    block); `qv-store/src/space.rs` (statvfs, floor, room, fits, available,
    pages, stored-length snapshot bytes, `UnlockedVault::file_space` and
    `file_pages`). Test hooks `space::DISK` and `space::ROOM`.
  - Step 3: `SnapshotBudgets` lost `max_bytes`; `SnapshotOutcome` gained
    `available`. Deviation from the 0059 order: an ordinary snapshot that
    cannot fit even alone is now skipped *before* pruning, so older
    snapshots are not dropped for it; pruning still follows when they no
    longer fit themselves. Every commit with kept snapshots runs the cheap
    stored-length check (`octet_length` per retained object outside the
    live manifest, no decryption); age and count still apply only when a
    snapshot is kept. The room uses the file's size on disk (not the
    in-transaction page count) with the volume's free space.
  - Step 4: `compact.rs`; `fs::replace_vault` (Unix `rename`, Windows
    `rename_replace` after closing SQLite's handle, as `relocate` does);
    `VaultFile::reopen` made crate-visible for Windows; `VaultFile::open`
    removes stale temporary siblings best effort. Verification compares the
    control rows byte for byte and every object/manifest row's key, profile,
    nonce and stored length in key order, plus the live manifest's bytes.
  - Step 5: `Session::{storage, compact, take_snapshot_skipped}`,
    `SnapshotSkip::NoRoom`, `StorageInfo`, `SnapshotSpace`,
    `AUTO_COMPACT_MIN_BYTES`; automatic compaction after successful
    mutations. The core budget test that forced a skip with `max_bytes = 0`
    now checks the report/notice mapping directly (`record_outcome`), since
    the room hook is store-internal; the store test forces the room.
  - Step 6: `qv snapshot rm <id>… | --all`, `qv compact [--json]`.
  - Step 7: app `DeleteSnapshot`, `DeleteAllSnapshots`, `Compact` actions,
    `ConfirmKind::DeleteSnapshots`, the skipped notice in `run_op`,
    Settings → Storage pane, Snapshots summary with "Delete all…" and a
    per-row delete button.
  - Focused checks run: `cargo test -p qv-store` (266 + lock + tamper, all
    pass), `-p qv-core` (156 + offline pass), `-p qv-cli --test snapshots`
    (9 pass), qv-app clippy `validation` and focused validation lib tests.
  - Not testable here: the app's skipped-snapshot notice end to end (no
    room hook outside qv-store; covered by core's mapping test and the store
    test), and the Windows paths (no local Windows target; CI's Windows job
    compiles and runs them first).
- 2026-11-15: Windows `install` split from Unix: Windows cannot replace an
  open file, so the session releases the vault and moves the copy (open
  under its own lock) onto the path via `VaultFile::replace_onto`; on a
  failed move it reopens the vault, or blocks writes if it cannot. Unix
  keeps both locks across the rename. Not compiled locally (no Windows
  target); CI's Windows job is the first check.
- 2026-11-15: The real-window flow failed at stage 61 with the first floor:
  the app's notice became the skipped-snapshot notice because the
  development disk (about 10 GB free of about 1 TB) was under a 10% floor.
  Floor revised (see Revisions); the flow then passed.
- 2026-11-15: Step 8 and final gates on the final tree: `cargo fmt --check`,
  `check-spdx.sh`, `check-matrix.sh`, workspace clippy `-D warnings`, qv-app
  clippy with `desktop` and with `validation`, `cargo test --locked`
  (1,058 passed, 0 failed, 7 ignored), qv-app `validation` tests (106 lib +
  3 real-window engine tests). Not run: Windows clippy/tests (CI), APK
  build, phone check. Fresh-context review pending.
- 2026-11-15: Floor removal checked with focused runs only, at the owner's
  request (the full suite made the PC struggle; that run was stopped before
  its tests started): `cargo fmt --check`, `check-spdx.sh`,
  `check-matrix.sh`, workspace clippy `-D warnings`, qv-app clippy with
  `desktop` and with `validation`, `cargo test -p qv-store --lib -- space
  compact snapshot` (22 passed), `cargo test -p qv-cli --bin qv --
  exit_messages` (1 passed). The last full `cargo test --locked` (1,058
  passed) and the real-window engine tests ran on the tree before this
  change; the change touches only the room formula, two message strings and
  their tests.
