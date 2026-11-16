# Review — Plan 0060: Snapshot space, snapshot deletion and vault compaction

| Verdict | Reviewed | Diff reviewed |
|---------|----------|---------------|
| Re-plan needed (finding 1) / Changes requested (2–6) | 2026-11-16 | `4954f83`, `19fa2c7`; current tree for `space.rs`, `compact.rs`, `snapshot_commit.rs`, `fs.rs`, core `snapshot.rs` |

Scope: fresh-context security review of the store and core side (owner
request: security-critical plans). App and CLI UI were not reviewed.

## Summary

Compaction is carefully built: it holds the flock on both inodes across the
Unix rename, the temp is `O_EXCL|O_NOFOLLOW` 0600 under the directory lock,
stale-temp removal skips locked temps and rotation-reserved names, the copy is
fsynced before the move and the directory after, and the anchor is unaffected
because head and state are identical. The attach limit is raised only for the
one bound statement and read back. The main problem is the room rule: on a
nearly full volume it deletes every snapshot in ordinary commits without
freeing any disk space. Verification of the copy is also weaker than the
"identical state" claim.

## Plan conformance

| Step | Done | Notes |
|------|------|-------|
| 1 Specs | yes | 0004 §1.2/§1.4, 0007 §3.3, 0012 row |
| 2 Space | yes | |
| 3 Room pruning | yes | conforms to 0007 §3.3, but see finding 1 |
| 4 Compaction | yes | Windows path deviates from §1.4 (finding 4) |
| 5 Core | yes | auto compaction is uncancellable (finding 3) |
| 6–7 CLI/app | not reviewed | |
| 8 Docs/gates | logged | gates not re-run by reviewer |

Out-of-scope changes: none found.

## Findings

1. **[high] On a nearly full volume, any commit deletes every snapshot and
   frees no disk space** — `crates/qv-store/src/space.rs:93-96`,
   `snapshot_commit.rs:191-203` and `:365-370`. `fits` requires
   `L + S + (L+S)/64 + 64 MiB <= file_size + disk_free`. When the volume has
   less free space than about `64 MiB + L/64` minus the file's slack, even
   `S = 0` does not fit, so the loop drops *all* snapshots, recovery ones
   from earlier merges included, in any commit (even a one-field edit that
   needs a few pages). Example: a 6 MiB vault on a phone with 50 MiB free loses
   all its snapshots on the next edit. Pruned pages only go to the file's
   freelist, and automatic compaction then refuses (copy + 256 MiB spare), so
   the volume gains nothing. The only notice is the `pruned` list. This
   follows the wording of 0007 §3.3, so it is a design problem: escalate to
   `quickvault-architect`. Problem statement: "The room rule prunes
   snapshots when live data plus a fixed 64 MiB allowance exceeds file size
   plus free disk, so low-disk devices lose every snapshot in ordinary commits
   without gaining space. Redesign it so that pruning happens only when the
   commit actually needs pages the file and volume cannot give (for example,
   measure the growth against the file's free pages plus the volume), drops
   only as much as that growth needs, never drops when dropping cannot help,
   and tells the user when it prunes for space." Action: escalated.

2. **[medium] The copy that replaces the only vault file is verified by
   lengths, not content** — `crates/qv-store/src/compact.rs:154-165`. Object
   and manifest rows are compared on key, profile, nonce and
   `octet_length(ciphertext)`; only the live manifest's bytes are compared.
   If a ciphertext byte were wrong in the copy (VACUUM fault, I/O error not
   reported, memory error), `quick_check` and these checks would pass, the
   rename would discard the good file, and the damaged attachment or history
   version would surface only at read or `verify` time. 0004 §1.4 step 3
   allows this, but the module docs claim "the same complete state". Fix:
   compare `ciphertext` byte for byte in key order (streamed, at most two
   blobs in memory), and amend §1.4 step 3. Action: escalated.

3. **[medium] Automatic compaction cannot be cancelled and blocks the session,
   including lock requests** — `crates/qv-core/src/snapshot.rs:202-211`
   (`compact(&|| false)`), `compact.rs:115-131` (one `VACUUM INTO` statement,
   `cancelled` checked only before and after it). It runs inside `mutate`
   while holding the session mutex. Removing a 2 GiB attachment from an 8 GiB
   vault meets the 25 % threshold, which means a 6 GiB copy, `quick_check` and
   reopen run synchronously. The UI stalls, and auto-lock or "lock now" waits,
   so the VK stays in memory past the lock deadline (0008). Fix: pass the
   session canceller, and make the step-budget progress handler also check it
   so the statement is interrupted (the copy is then discarded); or run
   automatic compaction as a separate background operation. Action: escalated.

4. **[low] The Windows install releases the process lock, contrary to 0004
   §1.4** — `crates/qv-store/src/compact.rs:227-246`. The session drops its
   vault handle and lock before `MoveFileExW`, so another process can lock the
   old file in that window. The paths I traced fail safe: the move fails and
   the session is `Blocked`, or the opener's identity recheck fails. None of
   this is tested, and the spec still says the lock is held. Fix: amend §1.4
   for Windows, and add a CI test with a concurrent opener. Action: escalated.

5. **[low] Claimed rejection and crash paths are untested** — `compact.rs`
   tests, `snapshot_tests.rs`. The plan's test list names "restored attach
   limit" and "pending anchor refuses". There is no test that `ATTACH` is
   refused after a compaction, and the code retries the pending anchor instead
   of refusing. The 0012 "Snapshot disk growth" row claims "a commit that
   cannot be written changes nothing", but no full-volume commit test exists.
   Cancellation during the copy is also untested. Action: escalated (tests).

6. **[low] Minor** — `qv-core/src/snapshot.rs:1035`: `restore_snapshot`
   (writes no commit) runs through `mutate`, so a restore can set off a
   multi-GiB automatic compaction of the active vault; use `read`/`run`.
   `compact.rs:116`: a vault path that is not UTF-8 makes every compaction
   `Unsupported`, and automatic compaction then never runs, silently.
   Action: escalated.

## Fixes applied by reviewer

None (review files only, by instruction).

## Gates

| Check | Result |
|-------|--------|
| `sh scripts/check-spdx.sh` | not run by reviewer |
| `cargo fmt --check` | not run by reviewer |
| `cargo clippy --all-targets --locked -- -D warnings` | not run by reviewer |
| `cargo test --locked` | not run by reviewer; developer log reports 1,058 passed before the floor removal, focused runs after |

Gates were not run: the review was code reading only, at the owner's request
to avoid heavy builds.

## Documentation

- [x] CHANGELOG.md, ARCHITECTURE.md
- [x] Spec Changelogs (0004, 0007, 0012)
- [ ] plans/README.md row: not updated by this review (instructed to edit only REVIEW.md); status stays Implemented

## Escalations for the owner

- Finding 1: re-plan the room rule (problem statement above).
- Findings 2–3: storage/core changes within this plan; 2 also needs a 0004
  §1.4 amendment.

## Developer response (2026-11-16)

- Finding 1 (high), fixed without a re-plan (owner-delegated; the owner asked
  for everything needed for 1.0): pruning for room happens only when the
  live data alone fits the room. Otherwise the commit drops no snapshot and
  keeps no new ordinary one. 0007 §3.3 is amended. Tests:
  `snapshot_room_keeps_snapshots_when_pruning_cannot_help`, and the
  budgets/room tests updated to the new rule. Pruning "only as much as the
  commit's own pages need" is not done: the room test is still the 0007
  §3.3 estimate.
- Finding 3 (medium), partly fixed: automatic compaction does not start once
  a lock is requested, and a lock during the copy discards it once `VACUUM
  INTO` returns. The copy statement itself still runs to its end.
- Findings 2, 4, 5 and 6: open, listed as known limitations in
  `docs/release-notes/1.0.0.md`.

## Follow-up repairs — plan 0065 (2026-11-16)

- Finding 2: fixed after amending 0004 §1.4. Every object and manifest column,
  including full ciphertext, is compared in sorted row order with bounded
  borrowed rows before replacement. A same-length ciphertext mutation fails
  without changing the source.
- Finding 3: fixed. A scoped worker runs VACUUM while the caller polls its
  borrowed cancellation callback; an atomic signal reaches SQLite's existing
  progress handler. Cancellation interrupts the statement, restores the attach
  limit and removes the copy. Verification checks cancellation between rows.
- Finding 4: fixed. Windows closes only SQLite's source connection, retaining
  both file leases across replacement/reopening via FileRenameInfoEx POSIX
  replacement and a write-through/flush handle. No unlocked fallback is used.
  The Windows regression tries
  another opener at that exact point; development/target evidence is in 0065.
- Finding 5: added attach-limit/ATTACH refusal, in-copy interruption, public
  cancellation callback and unchanged full-storage commit tests. A pending
  anchor is deliberately completed first under the ordinary write protocol;
  a blocked gate still refuses. This clarifies the old plan's inaccurate
  "pending anchor refuses" wording rather than weakening its ordering.
- Finding 6: snapshot restoration uses outcome-preserving `run` so it cannot
  trigger automatic compaction or hide a completed publication on late lock.
  Unix temporary cleanup and SQLite VACUUM target binding preserve
  non-UTF8 path bytes; a real non-UTF8 compaction regression exercises both.

Current gates are recorded in 0065. Wine/cross-compilation are development
checks; the attended Windows checklist and phone checks remain open.
