# Review — Plan 0059: Larger vaults and files, snapshots inside the vault

| Verdict | Reviewed | Diff reviewed |
|---------|----------|---------------|
| Changes requested | 2026-11-16 | `291330a` (raise vault limits), `4954f83` and `19fa2c7` where they touch 0059 code; current tree for `qv-store` (uncommitted plan 0064 edits there are comment wording only) |

Scope: fresh-context security review of the storage side only (owner request:
security-critical plans). Frontends, `player.js` and the media harness were
not reviewed.

## Summary

The in-vault snapshot design is sound: the sealed index is bound to the head
by digest (key 9) and checked before parsing, AAD binds vault ID, key
generation and profile, unlock requires exactly the manifest rows the head and
index name, and every snapshot change (keep, prune, release, schema upgrade)
happens inside the commit's single SQLite transaction before the anchor
update, so 0004 §5 / 0005 §5.4 ordering is unchanged. Object deletion is
retained-aware in the only delete path (`write.rs` commit). Budget arithmetic
saturates; untrusted lengths are bounded before reads. One availability bug
and some missing rejection and crash tests remain.

## Plan conformance

| Step | Done | Notes |
|------|------|-------|
| 1 Specs | yes | 0004 §1.1/§2.5, 0007 §3 read |
| 2 Limits | yes | `limits.rs` consistent with D1 |
| 3 D2 | yes (store side) | |
| 4 D3 store | partial | crash points for snapshot commits not tested (finding 2) |
| 5 D3 core | yes | deletions call `keep_no_snapshot` + `remove_snapshots_containing` |
| 6 D4 | yes | |
| 7–8 Frontends, MP4 | not reviewed | out of this review's scope |
| 9 Docs/gates | logged by developer | gates not re-run by reviewer (see Gates) |

Out-of-scope changes: none found in the reviewed files.

## Findings

1. **[medium] One missing snapshot-only object makes the vault permanently
   unwritable, including deleting the snapshot** — `crates/qv-store/src/space.rs:156-157`,
   called from `snapshot_commit.rs:191-203` on every commit of a file with
   snapshots. `snapshot_only_bytes` returns `Integrity` when a retained object
   row is absent; the comment says "unlock checked that", but `load_state`
   (`vault.rs:301-311`) checks only the live manifest's objects. The test
   `snapshot_removal_by_record_and_tampered_storage` deliberately documents
   that a missing kept object is "found by verify and restore, not unlock", yet
   after that damage every commit fails, and `Session::delete_snapshots` (a
   commit) fails the same way, so the obvious remedy is unavailable. Fix: in
   the room estimate, treat a missing retained row as 0 bytes (verify still
   reports it), and let an explicit `remove`/`containing` request proceed;
   or fail closed consistently at unlock. Add a test: delete a kept-only
   object, then edit and delete that snapshot. Action: escalated.

2. **[low] Missing rejection and crash tests the design depends on** —
   `crates/qv-store/src/snapshot_tests.rs`, `crash_tests.rs`. No test swaps in
   an older, authentically sealed index blob from the same vault (rollback
   inside the file), none downgrades a `0x7FFF0005` file to `0x7FFF0004`
   (drop the table, set `user_version`) while the head still carries key 9,
   and `crash_every_mutation_point` never runs a commit that keeps, prunes or
   upgrades (its edit has no snapshot request). The code handles all three
   (`snapshot_index.rs:324`, `vault.rs:268-274`, one transaction), so these are
   regression guards. Action: escalated (tests only).

## Fixes applied by reviewer

None (review files only, by instruction).

## Gates

| Check | Result |
|-------|--------|
| `sh scripts/check-spdx.sh` | not run by reviewer |
| `cargo fmt --check` | not run by reviewer |
| `cargo clippy --all-targets --locked -- -D warnings` | not run by reviewer |
| `cargo test --locked` | not run by reviewer; developer log reports 1,047 passed |

Gates were not run: the review was code reading only, at the owner's request
to avoid heavy builds. Developer logs are not reviewer evidence.

## Documentation

- [x] CHANGELOG.md
- [x] ARCHITECTURE.md
- [x] Spec statuses and Changelogs (0004, 0007, 0012 rows read)
- [ ] plans/README.md row: not updated by this review (instructed to edit only REVIEW.md); status stays Implemented

## Escalations for the owner

- Finding 1 needs a small storage-code change by the developer (no re-plan).

## Developer response (2026-11-16)

- Finding 1 (medium), fixed: a missing snapshot-only object takes no room in
  `snapshot_only_bytes`, so commits and snapshot deletion still work; the
  tampered-storage test now edits and deletes the damaged snapshot, then
  verifies.
- Finding 2 (low): open (rollback, downgrade and crash-point tests for
  snapshot commits).

## Follow-up repairs — plan 0065 (2026-11-16)

Finding 2 is implemented: replay of an authentic older sealed index is rejected
against the current head; schema/table downgrade and missing-digest variants
fail closed; keep/prune/schema-upgrade commits are interrupted at every existing
mutation/anchor crash point and reopen as a complete old or new state. The
full-storage commit regression covers ordinary/recovery keep and explicit
snapshot removal. Corrected inherited test setup gives each replay case its
own anchor baseline and checks the actual pruned objects rather than a row
count that can stay equal when a new live version is added. Current commands
and results are in 0065's execution log; no assertion of data preservation was
removed. Attended phone/Windows and large-limit qualification remain separate.
