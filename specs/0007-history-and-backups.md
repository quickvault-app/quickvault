# 0007 — History and Backups

| Status | Created | Depends on |
|--------|---------|------------|
| Accepted | 2026-10-14 | 0004, 0005 |

## Summary

History, trash and independent consistent snapshots reduce data-loss risk.
Unresolved conflicts and permanent deletion knowledge are not expiring history.
Purging retained payloads is not cryptographic erasure of external copies.

## Specification

### 1. Entry history and suppression

1.1. Dominated complete versions become history. Keep the last N (default 10)
by deterministic (modified_at, version_id) order, subject to resource budgets.
Unresolved heads remain active and MUST NOT be pruned, including during rotation.
Dates do not establish causality or override suppression rules. Permanent
tombstones are retained causal metadata, not restorable historical payloads;
payload-history listings omit those roles.

1.2. Each record retains a monotonic `history_floor` version vector in the
manifest. Merge floors componentwise by maximum. A historical version whose
clock is at most the floor is suppressed, even if reintroduced by an old peer.
The floor does not hide active heads or remove permanent tombstones.
Marked historical tombstones (0004 §3.4.1) remain indefinitely, including
when covered by a joined or explicitly purged floor; later final deletion
preserves earlier permanent tombstone versions as well.
Missing floor metadata means zero only for a genuinely new record, not when
a reference to existing suppression metadata is missing or fails verification.

1.3. Pruning may advance per-writer floor components only when all historical
versions covered by that advance are eligible for removal. If a retained version
would also be covered, leave that component unchanged. N is a retention target,
not permission to forget causal/suppression metadata or silently lose a conflict.
Write floor/reference changes in the same manifest transaction before deleting
unreferenced payloads. Recheck floors when merging imported historical objects.

1.4. Explicit history purge advances the floor to the join of all known active
head clocks, drops covered historical payloads and publishes the new manifest.
If those heads later become history they are also covered. Explain this boundary
to the user. Concurrent unseen versions are preserved and can need a later purge.
Purging must not resolve a conflict or delete its active versions implicitly.

1.5. Restore creates a new version covering all known heads, with explicit
resolution if needed. A restored historical value is an intentional new edit,
not reactivation of a suppressed version ID. Available history is synchronized
as authenticated references; it is not assumed present merely because the server
once received it. The current manifest declares exact retained references.

### 2. Trash

2.1. Soft deletion sets deleted_at as a causal edit. Default trash retention
is 30 days, measured by client policy; clock anomalies require confirmation
before destructive expiry. Bulk deletion of a group applies explicit entry/
group edits in one transaction, rather than silently hiding inaccessible children.

One delete operation assigns a fresh random `delete_operation` ID shared by its
entry/group edits (0003 §4/§5). Restoring a group restores only that group and
its trashed, unconflicted descendants carrying the same operation ID, connected
through groups with that ID by their source parents. Separately deleted records
stay in trash even when their timestamps coincide. A group without a legacy
operation ID restores that group alone; restore its children explicitly.

2.2. Final deletion removes secret payload/history and installs a minimal causal
tombstone; owned attachment metadata receives tombstones in the same transaction.
Release chunks only when no retained reference needs them. Retain each tombstone's
ID, version and clock **indefinitely**. Old covered edits
stay deleted after years offline. Concurrent deletion/edit is a visible conflict
in trash until explicit resolution (0005). Deletion knowledge is included in
snapshots, rotation, manifests and the resource budget.

### 3. Local snapshots

3.1. A local snapshot is a past manifest the vault keeps, together with the
objects it references (0004 §2.5, plan 0059): it shares every object the vault
still holds, so it costs only its manifest and what the vault has since
dropped. It never contains device-local quick slots, signing keys, session
tokens, sync state or external anchors. It is restored, exported or inspected
only through the vault that holds it, with that vault's key. Restoring writes
an ordinary slotless archive (0004 §6) of the snapshot to a private temporary
file beside the new path and restores that; the temporary file is removed.
Exported archives remain the backup against losing or corrupting the vault
file; frontends say so where snapshots are listed. No slot is added to any
archive unless the user explicitly chooses a self-contained export (0004
§6.4).

The VK rotation (0002 §3.5), like any profile or generation migration that
builds a new file, still writes its recovery copy as a slotless
archive in the private snapshot directory and inventories it across the
generation change, because it builds a new file that carries no snapshots.
Snapshot files written before plan 0059 stay listable and restorable and are
pruned by age and count until none remain; no new ordinary file is written.

3.2. **When snapshots are taken.** Snapshots are taken inside commits, never by
a read: when a commit is about to change the vault and the newest snapshot is
more than an hour old, or none is from the current UTC day, the commit first
retains the pre-commit manifest as an ordinary snapshot. A commit that leaves
the vault as the newest snapshot has it takes none. LAN and archive merges
commit through the same rule.

Take a recovery snapshot of the pre-operation state, in the same transaction,
before an archive import into an existing vault, baseline adoption and a
conflict resolution that installs a final deletion: automatic merges where the
user did not choose what disappears. A recovery snapshot is pruned like an
ordinary one once its operation has committed; its kind remains a label.
Explicit deletions (final deletion, emptying the trash, removing an
attachment, history purge) take no recovery copy (owner, 2026-11-15): the
trash is their safety net, and they commit atomically.

Core enforces these rules. A snapshot of an operation is only ever the exact
pre-operation manifest; frontends cannot skip, substitute or postpone it, and
it needs no separate consent because it never copies the vault.

An import into an existing vault requires that recovery snapshot. Explicit
creation of an independent new path under 0004 §5.4 changes neither the selected
document nor an existing local vault: fully verified leased staging and the
no-replace restore protocol protect publication. It requires explicit new-baseline
acknowledgement (§3.4), without taking a snapshot of an unmodified local vault.

3.3. Defaults are 30 days and at most 48 snapshots per vault. Snapshot-only
data (retained manifests other than the live one, plus objects only snapshots
reference) has no fixed budget: it may use the **room** the vault does not
need (owner, 2026-11-15; plan 0060). At each commit the room is the lesser of
the vault file cap (0004 §4.1, `MAX_DB_BYTES`) and the file's current size plus
the volume's free space minus a floor of 5% of the volume, at least 2 GiB and
at most 4 GiB (the volume term is left out when free space cannot be measured).
A commit fits when its live data plus snapshot-only data, plus 1/64 of that sum
and 64 MiB of allowance, is within the room. Snapshots therefore give way to
live data and to the device's free space; neither can fail a commit because of
them.

Prune the oldest snapshots first until the budgets fit. Any commit that no
longer fits the room prunes, whether or not it takes a snapshot; age and count
are applied by the commit that takes a new one. A snapshot taken by the current
operation is never pruned by it. When an ordinary snapshot cannot fit even
alone, the commit proceeds without it and reports how much it needed and how
much room there was; frontends tell the user that the change was saved without
a snapshot. A recovery snapshot is always kept.

The user may delete any snapshot, or all of them, at any time; one commit
removes them, and their snapshot-only data is released like any unreferenced
object. Deleted data leaves the file's free pages for reuse; compaction (0004
§1.4) returns them to the volume.

**Permanent deletion and snapshots (owner, 2026-11-15).** Final deletion and
history purge also remove every snapshot that contains the deleted versions,
unless the user explicitly keeps them; frontends say how many snapshots
contain them before asking. Keeping them leaves the data recoverable until
those snapshots are pruned. Neither choice is secure erasure (Security
Considerations).

3.4. Restore writes a new path, preserving the active vault and its external
anchor. Opening a restored copy is an explicit recovery baseline, and subsequent
merge with an existing device preserves that device's deletion/suppression
knowledge. A restore never proves that the snapshot is the latest global state.

3.5. **Password changes and existing snapshots.** Changing the master
password (0002 §3.4) leaves the vault key and key generation unchanged, so
snapshots taken before the change remain decryptable with that key. Because
the old password may be the reason for the change, the frontend MUST, after a
successful password change, offer to delete the snapshots created before it,
naming how many exist. It MUST delete them only on explicit confirmation, and
it MUST NOT describe the result as cryptographic erasure: copies held
elsewhere, on other media or by other devices are unaffected (§4.3), and a
party that obtains the vault key from an old vault-file copy or an old keyslot
can still open them. Declining the offer keeps them under the ordinary
snapshot budgets (§3.2–3.3).

### 4. Server retention and purge acknowledgements

This section applies only to configured remote server sync. Offline and LAN-only
vaults require no server purge acknowledgement. Local history suppression and
snapshot budgets apply in every mode; LAN peers merge suppression frontiers
under 0005 and may retain independent copies. A peer's receipt is not a server
purge acknowledgement and does not prove erasure.

4.1. The current checkpoint's manifest and every referenced object are pinned,
including retained history, unresolved versions, tombstones and chunks. An honest
server MUST NOT prune these to satisfy an admin history policy. A lower server
retention/quota requires a client-produced manifest explicitly releasing eligible
history; otherwise reject new writes or request client cleanup. Never silently
invalidate a published manifest.

4.2. Default unreferenced-object retention is 30 days; quotas may shorten it.
An authenticated explicit purge requests immediate logical removal of released
objects, after checking that no pinned state references them. Publish references
and the purge result atomically/idempotently. The server cannot inspect
encrypted timestamps/deletion flags
to implement client retention policy on its own.

4.3. Clients track remote purge acknowledgement and local snapshot status.
Offline devices and exported/backed-up copies may retain secrets; show these
limits. Other clients apply suppression floors before importing old histories.
No mechanism can force a malicious server, independent backup or departed device
to erase a retained copy.

## Security Considerations

The CLI calls this operation history purge, not secure shred. Derived record
keys remain reconstructible from VK, and SSD/copy-on-write storage can retain
old ciphertext. Secure erasure cannot be claimed. A leaked old password slot
can still reveal same-generation VK; offer generation rotation for future-data
protection, and separately identify remaining local snapshots (0002).

## Testing Requirements

Test suppression after old-backup import, years-old deleted entries, resolution
after delete/edit races, preservation of every unresolved head, snapshot
count/bytes and disk-full failures, and server GC against the complete current
reference set. Purge acknowledgements must not imply independent copies vanished.

## Open Questions

1. **Password slots in local snapshots.** *Resolved 2026-10-25 (owner decision
   D6, roadmap 0002): no password slot by default. On password change, offer to
   delete pre-change snapshots (§3.1, §3.5). Self-contained archives remain an
   explicit, warned user choice (0004 §6.4).*

## Changelog

- 2026-11-15: §3.3: snapshot-only data uses the room the vault does not need
  (file cap and volume free space less a floor) instead of a 1 GiB budget; any
  commit prunes when it no longer fits; skipped snapshots are reported to the
  user; the user may delete one or all snapshots (owner request, plan 0060;
  numbers delegated).

- 2026-11-15: §2.2, §3.1–3.3: snapshots live inside the vault and share its
  objects; they are taken inside commits; explicit deletions take no recovery
  copy; permanent deletion removes the snapshots that contain the deleted data
  unless the user keeps them; budgets 48 / 30 days / 1 GiB snapshot-only
  (owner request, plan 0059; details delegated).

- 2026-11-11: §3.2 distinguishes Android independent new-path import from an
  import that mutates an existing vault; source/local files remain intact (0049).

- 2026-11-09: Specify core-enforced recovery-copy authorization and cross-process
  lifetime leases; source repairs and qualification are tracked in plan0050.
- 2026-11-09: Restore recursive deletions by explicit operation identity;
  timestamps never restore separately deleted records (QV-004, plan0050).
- 2026-10-30: Clarify indefinite preservation of dominated marked tombstones
  through purge and later final deletion, and omit non-restorable tombstones
  from payload-history views (combined 2.2b, plan 0027).

- 2026-10-27: Resolve OQ1 per owner decision D6. Local snapshots carry no
  password slot and open with the vault key or the recovery phrase; add §3.5
  (offer to delete pre-change snapshots on password change) and the
  private-directory requirement (plan 0018).
- 2026-10-24: Apply ordinary snapshot coalescing to LAN and server merges;
  scope server retention/purge acknowledgements to optional remote sync and
  preserve suppression/independent-copy limits for LAN-only use.
- 2026-10-14: Initial accepted history/trash/snapshot policy.
- 2026-10-15: Amendment: permanent tombstones, monotonic history suppression,
  bounded/coalesced snapshots and reference-based server retention. Define purge
  propagation and avoid claims of cryptographic erasure.
- 2026-10-15: Security review amendment: snapshots use the authenticated
  archive format instead of SQLite backup copies; no checkpoint-chain retention.
