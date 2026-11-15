# Plan 0059 — Larger vaults and files, snapshots inside the vault, buffered playback

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| Implemented — Android build, phone and Windows checks, fresh-context review pending | 2026-11-15 | 2026-11-15 (owner request; numbers and design details delegated by the owner, standing 2026-11-02) | 0003 §6.3, 0004 §1.2 §2 §3.6 §4.1–4.3, 0007 §2.2 §3, 0009 §1.8, 0011 §4, 0013 §7.4 (amended) |

## Intent

Owner discussion, 2026-11-15:

- "Why the 1GiB limit anyways for the vault? And why the 10MiB limit for the
  files? … we can warn the user when doing a sync of a huge vault and other
  stuff." → raise the limits; keep only large hard ceilings for untrusted
  input; warn where an operation is large. Owner: "you can pick higher numbers
  already. Those suggestions you gave me are reasonable."
- "Cleaning your vault would double its size? What logic is that." → explicit
  deletions take no recovery copy; permanent deletion also removes the item
  from snapshots unless the user keeps them.
- "changing the fields of an entry would also make snapshots of that entry's
  files? I'm not sure I buy that" → snapshots share the vault's stored
  objects instead of copying the whole vault.
- "Cant you fragment the MP4 in a buffered way?" → Linux MP4 preview streams
  fragments around the playback position instead of holding the whole file.
- "make it on the same plan, but with multiple steps … I'm gonna be away".

## Decisions (delegated by the owner, standing 2026-11-02, unless marked)

### D1 — Numbers

| Limit | Before | After | Why |
|-------|--------|-------|-----|
| One attachment | 10 MiB | **2 GiB** (32,768 chunks) | Owner-approved range 1–2 GiB. Its chunk list (32,768 × 17 B ≈ 557 KB) fits the 1 MiB record. |
| Referenced ciphertext (vault total) | 1 GiB | **8 GiB** | Owner-approved range 8–16 GiB. 16 GiB of chunks would not fit the 64 MiB manifest (below). |
| Records | 50,000 | 50,000 | Owner: "it's fine for now". |
| Version objects | 250,000 | 250,000 | Unchanged; now counts every non-chunk object. |
| Chunk objects | (in the 250,000) | **≤ 131,072** (8 GiB / 64 KiB) | Counted on their own so files never crowd out history. |
| Manifest | 64 MiB | 64 MiB | Worst case 53.5 MB today + 131,072 chunks × ~75 B ≈ 63.3 MB. |
| Vault file (pre-open check) | 2 GiB | **16 GiB** | 8 GiB live + snapshot-only objects + history/manifests. |
| LAN staging (0013) | 2 GiB | **16 GiB** | A first sync of a full vault must fit. |
| Snapshots | 48 / 30 days / 512 MiB of whole copies | 48 / 30 days / **1 GiB of snapshot-only data** | Bytes now measure only what snapshots keep beyond the live vault. |

Rejected: unlimited (every untrusted length still needs a bound; a vault
without a total needs a tree manifest, out of scope); 16 GiB total (manifest
over 64 MiB; every edit would rewrite ~20 MB).

### D2 — Explicit deletions take no recovery copy

Final deletion (empty trash, delete permanently, remove attachment), history
purge: no mandatory recovery copy. The trash (30 days) is the safety net; the
change commits in one transaction. Kept: archive import into an existing
vault, baseline adoption, VK rotation, and conflict resolution that installs a
final deletion (automatic merges where the user did not choose what
disappears; the rotation still writes its external recovery archive because
it builds a new file).

### D3 — Snapshots inside the vault file

A local snapshot becomes a retained manifest inside the vault database:

- `manifest` keeps the live row (named by the head) plus one row per
  retained snapshot manifest (rows shared when hashes are equal).
- New singleton table `snapshot_index` holds one sealed blob (local-state
  key, AAD bound to vault ID and generation): the snapshot list
  `[snapshot_id, created_at, kind, manifest_hash]` and the sorted set of
  object IDs any snapshot references (`retained`).
- Commit pruning deletes an object only when neither the new manifest nor
  `retained` references it; a manifest row is deleted only when no snapshot
  names it.
- **When:** inside a commit. If the newest snapshot is over an hour old, or
  none is from today, the commit first retains the pre-commit manifest as an
  ordinary snapshot. No write on read-only commands, and no snapshot when
  nothing changed. A destructive merge (D2's kept list except rotation)
  retains the pre-state as a `recovery` snapshot in the same transaction, so
  pins, leases and over-budget consent are no longer needed for them.
- **Budgets:** count, age and bytes (snapshot-only manifests + objects
  referenced only by snapshots), pruned oldest first in the same commit;
  removal rebuilds `retained` from the remaining snapshot manifests.
- **Restore:** stream the snapshot as an ordinary archive (same writer, the
  snapshot's manifest) to a private temporary file beside the target, then the
  existing archive restore; the temporary file is removed.
- **Rotation** builds a new file and carries no snapshots (documented); its
  external recovery archive and inventory stay as they are.
- **Legacy snapshot files** (`.qvsnap` in the snapshot directory): no new ones
  except the rotation copy; the automatic pass keeps pruning them by age and
  count, and they stay listable/restorable until then.
- **Schema:** `snapshot_index` is schema `0x7FFF0005` (`user_version`); the
  header format stays `0x7FFF0004`. A `0x7FFF0004` file with exactly the old
  tables opens and is upgraded in its first write transaction. Old builds
  refuse an upgraded file as unsupported (owner action: update every device).
- Trade-off (told to the owner): snapshots no longer protect against losing
  or corrupting the vault file; exported archives are the backup.

Rejected: an external content-addressed chunk store (keeps independence from
the vault file but still copies the whole vault once; needs cross-process GC
leases); full copies with dedup by hard links (platform-specific).

### D4 — Permanent deletion and snapshots

Final deletion and history purge remove every snapshot that contains the
deleted items, unless the user chooses to keep them (the CLI keeps them only
with `--keep-snapshots`; the app's confirmation has a "Keep in snapshots"
choice, default off). Frontends say how many snapshots contain them. Never
described as secure erasure.

### D5 — Warnings instead of small limits

- Attaching a file over 256 MiB asks for confirmation (time and space).
- A LAN sync or join that will receive over 256 MiB shows the size before it
  starts receiving and progress while it does.
- Linux previews of non-MP4 media over 256 MiB are refused with "Too large to
  preview on this system — save a copy" (they play from memory whole).

### D6 — Buffered MP4 preview on Linux

`player.js` reads the MP4 index with range requests, then fragments and
appends only ~30 s ahead of the playback position from range reads, removes
data more than ~30 s behind, and restarts at the keyframe before a seek
target. The replay-after-end rebuild stays.

## Steps

1. Spec amendments with Changelog lines (0003, 0004, 0007, 0009, 0011, 0013).
2. Limits (D1): qv-model, qv-store `limits.rs`, chunk accounting, worst-case
   test, archive totals, LAN staging, app/CLI file bounds; tests.
3. D2: drop `require_recovery` from purge paths; CLI/app stop taking copies
   there; tests.
4. D3 store: schema `0x7FFF0005` + upgrade, `snapshot_index` seal/open,
   retained-aware commit pruning and manifest-row retention, snapshot create
   inside commits, prune/GC, list, delete, archive stream of a snapshot,
   verify; crash points; tests.
5. D3 core: policy moves into commits (hourly/daily rule, budgets),
   destructive merges retain a recovery snapshot, rotation unchanged,
   legacy files pruned only; listing/restore/delete APIs keep their shapes.
6. D4: snapshots containing deleted records; purge API option; tests.
7. Frontends: CLI and app snapshot listings/restore, deletion confirmations
   (D4), D5 warnings.
8. D6 buffered MP4 playback + non-MP4 Linux preview bound; media harness.
9. Docs (ARCHITECTURE if affected, CHANGELOG, README index), full gates,
   APK build. Fresh-context review stays pending.

## Execution log

- 2026-11-15: plan written; owner away, executing.
- 2026-11-15: step 1 (specs): 0003 §6.3/§6.4, 0004 §1.1/§2/§2.5/§3.6/§4.1–4.3/
  §5.4/§6.1/§7.1, 0007 §3.1–3.3, 0009 §1.8 (large data, snapshots), 0011 §4,
  0012 (fixture boundary, "Snapshot disk growth" row), 0013 §7.4, each with a
  Changelog line. ARCHITECTURE §3 and the snapshot prose follow.
- 2026-11-15: step 2 (limits): `MAX_ATTACHMENT_BYTES` 2 GiB, `MAX_CHUNKS`
  32,768, `MAX_TOTAL_CIPHERTEXT` 8 GiB, `MAX_CHUNK_OBJECTS` 131,072 apart from
  `MAX_OBJECTS`, `MAX_REFERENCES`, `MAX_DB_BYTES` 16 GiB (SQLite page cap
  4,194,304), LAN `MAX_WANT_MESSAGES` 94. Worst-case manifest rebuilt with the
  chunks the total leaves: 62.9 MB sealed; an upper bound over every budget
  (5-byte lengths, 131,072 chunks) is 64.0 MB, under the 64 MiB (67.1 MB)
  limit. The 2 GiB bound is tested with a smaller stand-in bound
  (`stage_attachment_up_to`), since streaming 2 GiB would be too slow.
- 2026-11-15: steps 3–6 (store and core): schema `0x7FFF0005` with
  `snapshot_index` (`snapshot_index.rs`, sealed under the local-state key,
  head key 9 binds its digest; a `0x7FFF0004` file opens and is upgraded by
  its first commit that keeps a snapshot); manifest rows keyed by hash, unlock
  requires exactly the head's and the index's rows; commit planning
  (`snapshot_commit.rs`): keep, prune (count, age, snapshot-only bytes),
  remove by ID or by record, release objects and manifest rows nothing names;
  restore through a temporary archive beside the target; `verify` checks kept
  manifests and their objects. Core arms each mutation (hourly/daily rule),
  arms a recovery snapshot for archive/LAN merges, baseline adoption and a
  final-deletion conflict resolution, drops `require_recovery`; deletions take
  `keep_snapshots`; `snapshots_holding(DeletionTarget)` counts for
  confirmations; the password change keeps no snapshot of the pre-change
  state. The file pass only prunes (rotation copies and older files).
  `merge_archive` lost its budget argument. Store tests:
  `snapshot_tests.rs` (keep/release/restore, skips and budgets, removal by
  record and tampering, schema upgrade); core snapshot tests rewritten for
  the new policy.
- 2026-11-15: step 7 (frontends): CLI `--keep-snapshots` on `trash empty`,
  `history purge`, `attach rm`, confirmations name the snapshots; no recovery
  prompts; `merge --allow-over-budget` removed; rows say "bytes kept". App:
  confirmation counts and a "Keep in snapshots" checkbox, no budget consent
  for baseline adoption, snapshot page and trash text, large-attachment
  confirmation (Android reports the provider's size), LAN receive progress
  over 256 MiB (passive byte counters in qv-sync), Linux non-MP4 preview bound
  256 MiB (`PreviewTooLarge`).
- 2026-11-15: step 8 (buffered MP4): `player.js` reads the `moov` with range
  requests, plans fragments, appends about 30 s ahead from ≤ 32 MiB range
  windows, removes data 30 s behind, and rebuilds the MediaSource for a seek
  outside the buffered data (WebKitGTK presents no frames after its buffer is
  emptied and refilled: measured, same fault class as plan 0057's replay; it
  also needs the start position set after the first window, one second
  early, because B-frames present a fragment's first frame after its decode
  time). Whole-file fallback remains for small files the engine cannot
  stream. New fixture `preview-long.mp4` (100 s); the media flow checks the
  ranged path, the ~30 s window, a seek to 70 s with moving frames and that
  old data is dropped. Media flow: 8 of the last 9 runs passed across both
  rendering modes; the two failures were WAV-audio stage flakes (no media
  source at stage 2 or 10), not the MP4 path.
- 2026-11-15: an accidental bulk rewrite truncated 13 files mid-session; they
  were restored from `HEAD` and every edit was re-applied (recorded so a
  reviewer checks those files: core `edit.rs`, `snapshot.rs`, `session.rs`,
  `history.rs`, `attachment.rs`, `views.rs`, `conflict_tests.rs`,
  `tests/offline.rs`; app `ops.rs`; CLI `attach.rs`, `history.rs`,
  `tests/conflicts.rs`, `tests/exchange.rs`).
- 2026-11-15 (gates): SPDX, `cargo fmt --check`, matrix check,
  `cargo clippy --all-targets --locked -D warnings`, qv-app clippy with
  `validation` and `desktop`, `cargo test --locked` (1,047 passed, 0 failed,
  7 ignored), `cargo test -p qv-app --features validation` (105 + 3 engine
  tests), `node --test` player (11/11).
- 2026-11-15 (final): a snapshot skipped for space reports the bytes it
  needed; the index cap (256 rows) and the retained-ID cap prune the oldest
  instead of failing a commit. Re-run on the final tree: fmt, SPDX, matrix,
  workspace Clippy, `cargo test --locked` (1,047 passed, 0 failed, 7
  ignored), qv-app `validation` tests and Clippy (`validation`, `desktop`),
  player JS tests; the debug APK builds. Pending: Windows Clippy (CI; no
  MinGW here), install and checks on the owner's phone, fresh-context
  review.
