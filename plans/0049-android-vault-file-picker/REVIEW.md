# Review — Plan 0049: Android vault-file picker and verified import (partial)

| Verdict | Reviewed | Diff reviewed |
|---------|----------|---------------|
| Partial — not a verdict on the whole plan | 2026-11-14 | commit `d4ce6ab`: `crates/qv-store/src/vault_import.rs` |

## Summary

Plan 0056 (1.0 preparation) asked for focused reviews of the riskiest
unreviewed code. This covers the store side of importing an untrusted
user-selected document: bounded staging, authentication and publication.
`qv-core/src/vault_import.rs`, the app controller, the Kotlin document glue
and the device qualification remain; the plan stays `In progress`.

## Findings

Checked: staging writes only ciphertext into a leased private temporary,
reads at most `MAX_DB_BYTES` plus one probe byte and refuses more without
writing it; every failure drops the lease before the staging guard reaps
unleased temporaries (test `vault_import_bounds_actual_bytes_and_cleans_failed_staging`);
a wrong password keeps the same candidate and its lock, so the provider is
never reread; the full `verify_all` pass and the keyslot-descriptor
cross-check run before a `VerifiedVaultImport` exists; publication goes
through the ordinary verified archive restore path with a fresh writer,
replica, head and anchor, and a post-publication failure is reported as
`PublicationIncomplete`, never as a clean failure.

1. **[low] A cancellation or integrity failure after the password consumes
   the candidate** — `crates/qv-store/src/vault_import.rs:107`. After
   `held.take()`, a failing `finish_unlock`/`verify_all` drops the file, so a
   retry needs the document picked again. Fails safe. Action: recorded.
2. **[low] Cancellation is reported as `StoreError::Io`** — `check_cancel`.
   Callers must check their own cancel token to tell a cancel from an I/O
   failure; core does (`qv-core` maps by token). Action: recorded.

No critical, high or medium findings in the reviewed file.

## Fixes applied by reviewer

None.

## Gates

See plan 0056's execution log for the 2026-11-14 tree.

## Escalations for the owner

None blocking. The real-phone picker qualification is plan 0056 step 5.

---

# Review — Plan 0049: core, app and Android glue (fresh context)

| Verdict | Reviewed | Diff reviewed |
|---------|----------|---------------|
| Changes requested | 2026-11-16 | `d4ce6ab` plus later edits to the same files in the current tree: `qv-core/src/vault_import.rs`, `qv-app/src/{vault_import,controller,android,documents}.rs`, `ui/{mod,shell}.rs`, `android/MainActivity.kt` |

## Summary

This covers the parts the 2026-11-14 store review left out: the core API, the
controller flow, the SAF/JNI glue and the UI. Nothing is trusted before
authentication: only ciphertext is staged, the provider's name and size are
hints, and the published file goes through verified restore with a fresh local
identity. The VK and the decrypted state live only inside one worker call, and
password inputs are moved out and cleared on each attempt. A stale epoch or a
lock drops staged results; publication uses no-replace (the earlier
`symlink_metadata` check is only for the UI message), and late publication is
remembered without opening a session. No blocker or high finding. The plan
stays `In progress`: step 4 (real-device provider and picker run) and step 5
are open. One test and its evidence no longer match the current limits.

## Plan conformance

| Step | Done | Notes |
|------|------|-------|
| 1 | yes | 0004 §5.4 / 0009 §1.8 / 0007 §3.2 amendments present |
| 2 | yes | Store/core opaque phases. The near-budget qualification is now stale (finding 1) |
| 3 | yes | Welcome, locked and switcher actions; consent, retry and collision handled |
| 4 | partial | Provider fixture built but never run on a device |
| 5 | partial | This review |

Out-of-scope changes: none found. Later commits (`291330a` raise vault limits,
`01243dd` picker deferral) touched these files. Only their effect on this flow
was checked.

## Findings

1. **[medium] The near-budget import qualification no longer qualifies anything** —
   `crates/qv-core/src/vault_import.rs:145-220`. Its fixture is fixed at
   99 × 10 MiB. Since `291330a`, `MAX_TOTAL_CIPHERTEXT` is 8 GiB and
   `MAX_DB_BYTES` is 16 GiB (`qv-store/src/limits.rs:10,42`). Run explicitly,
   the test fails at its `>= 99 %` assertion, and its "hundredth is refused"
   step now succeeds. The plan's "explicit near-budget qualification passes"
   and VALIDATION.md describe the old 1 GiB / 2 GiB limits. Fix: derive the
   fixture from the constants (or mark the test as historical) and re-record
   the evidence. Do not cite it for 1.0 as it stands.
2. **[low] The worst-case staging write grew 8× with no early rejection** —
   `crates/qv-store/src/vault_import.rs:146-188`. A wrong pick (a video, a
   non-SQLite file) or an endless provider stream writes up to 16 GiB into
   app-private storage before SQLite rejects it. Archive and destination then
   add more on top. Failure is safe and cleanup works, but on a phone this can
   fill the disk. Fix: check the 100-byte SQLite header (magic, 4096-byte page
   size, header page count × page size ≤ `MAX_DB_BYTES`) within the first read,
   and stop once the actual bytes exceed the declared size. This is not
   payload parsing; SQLite reads the same header anyway.
3. **[low] A stale reader can close a newer document stream** —
   `crates/qv-app/src/android.rs:268-274`, `android/MainActivity.kt:350-372`.
   Kotlin keeps one unsynchronized `reader`/`writer` pair that is not tied to
   a request. `qvRead` and `qvCloseDoc` take no id, and every `DocReader` drop
   calls `qvCloseDoc(false)` without checking which stream is open. Scenario:
   an import blocked on a slow cloud provider is cancelled by a lock. The user
   unlocks and starts an attachment import or export. The old thread's drop
   then closes the new stream, and that operation fails spuriously. One read
   already in flight can also read from the new stream. That data is
   discarded, because the token is cancelled. Fix: pass the request id to
   `qvRead`/`qvWrite`/`qvCloseDoc`, act only when it matches, and guard the
   fields with the activity monitor (or `@Volatile` plus id checks).
4. **[low] A retry after a post-password integrity failure reports the wrong
   error** — `crates/qv-app/src/controller.rs:2068-2087` with
   `qv-store/src/vault_import.rs:107`. After `held.take()`, a failure in
   `finish_unlock`/`verify_all` drops the candidate, but the controller keeps
   `pending_import`. The next "Verify and import" then returns
   `StoreError::Unsupported`, not the integrity failure. Fix: clear
   `pending_import` (or return to Unlock mode) for any error other than
   `WrongPassword`/`Cancelled`, and add a controller test for a tampered
   staged file.

Checked with no finding: bounded JNI chunks (≤ 64 KiB, zeroized
intermediates); name bounds (255 in Kotlin, 1–64 with no leading dot or
separators in `file_name`); no broad storage permission and no persistable
grant; the URI is released after staging; abandoned staging is reaped at
startup and on drop; post-publication cancel, platform-binding failure and
list-write failure are all reported; the source vault and its anchors are
never touched.

## Fixes applied by reviewer

None (review-only instruction; another agent applies fixes).

## Gates

Not run: the reviewer was asked to read code rather than build on a shared,
nearly full disk. The 2026-11-11 developer logs predate `291330a` and are not
current evidence.

## Escalations for the owner

- Step 4 (real-phone provider and system-picker run) is still required before
  this plan can be `Reviewed`.
- Finding 1 invalidates the recorded near-budget evidence under the raised limits.

## Developer response (2026-11-16)

- Finding 1 (medium), fixed: the ignored near-budget test fills whatever
  `MAX_TOTAL_CIPHERTEXT` is, with 64 MiB attachments until one is refused,
  and checks 99% of the budget. Not run here (about 16 GiB of disk); the
  earlier qualification numbers are stale until it is.
- Finding 4 (low), fixed: an integrity, format or incomplete-publication
  error now clears the pending import (`controller.rs`); a wrong password,
  a taken name or a cancel still leave it for another try.
- Findings 2 and 3 (low): open, listed as known limitations in
  `docs/release-notes/1.0.0.md`. Finding 3 changes the Kotlin glue and needs
  an Android build and a phone to verify.

## Follow-up repairs — plan 0065 (2026-11-16)

- Finding 2: fixed. The public 100-byte SQLite header is checked before any
  payload staging; magic/page size/nonzero bounded page count must match, and
  actual length must equal the declared size. Wrong picks stop within the
  header; overrun probes one byte without writing it. Authentication and full
  validation still follow. Regressions cover magic, page size, zero/oversized
  page counts, truncation and overrun.
- Finding 3: fixed. Read/write/close use unique native stream handles. Stale
  opens check the picker ID before detaching anything; blocking provider I/O
  runs outside the monitor, and installation rechecks the pick. The Android
  instrumentation suite includes stale read/open/close/cleanup regression
  calls. The app and test APKs compile; no connected phone was available to
  execute this device test.
- Finding 1's near-budget qualification remains unrun under the raised limits;
  corrected test code is not qualification evidence. Real-device provider and
  picker checks remain open. See 0065's execution log for current gates.
