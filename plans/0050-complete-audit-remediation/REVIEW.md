# Local completion check — plan0050

2026-11-10. Developer's local check, with no reviewer agent or independent
assessment. The owner requested a quick local check and delegated correctness
decisions. No external reviewer was contacted.

**Status: qualification remains open.** Source repairs cover the fifteen tracked
implementation areas. Android media playback has not been executed because no
phone is connected. The plan stays In progress; tests alone do not close the
independent composition/dependency/device gates in0048.

The local pass follows the changed call chains for non-mutating SQLite opening,
post-durable errors/index repair, finite clock admission, deletion identity,
destructive-copy authorization, cross-process file leases and cleanup,
clipboard delivery, display/editor separation and preview source/transport/UI.
Contracts precede behavior changes; no crypto primitive, eligibility budget,
causal truncation, detached preview key or new production dependency was added.

Corrections from this pass:

- Preserve private parent-directory aliases when acquiring directory flocks;
  the earlier extra no-follow flag would have changed the owner's QV-013 policy.
  Explicit snapshot/anchor-root checks retain their existing policy.
- Escape omitted vault locations, autofill rows and editor display mirrors;
  retain raw editable controls and stored/copied values.
- Carry capacity classification through platform output creation/fsync and
  frontend adapters; provider failures without a usable errno remain generic.
- Revoke hidden/late-inserted preview sources through the existing bounded
  typed UI event as well as resetting renderer media. Avoid sending that event
  when there is no viewer on the locked screen.
- Preserve existing imported delta-audit bases rather than deleting apparently
  stale versions and breaking real audit paths. Exemption debt remains explicit.
- Correct obsolete test setup, without removing assertions: mandatory copies,
  a genuine empty-attachment content hash and explicit private Xvfb selection.

File-lifetime check: handles release their file Arc before their lease token;
the last token closes the registry's descriptor under its mutex. A dead Weak
still in the registry fails Locked. SQLite-capable descriptors are not cloned
or reopened by cleanup; opaque readers share positioned I/O with independent
cursors. Producers retain flock through publication/fsync, and recovery entries
carry a private exclusive lease until their pin is released. Child-process and
actual SQLite-lock tests exercise these invariants.

Preview boundaries: exact capability paths/numeric Host, bounded initial
headers/connections, strict single ranges and416, separate websocket credentials,
exact listener CSP, protected chunks and Weak Session authorization. Actual
GTK/WebKit playback/seeking/consent and close/lock refusal pass. Decoder/kernel/
GPU residual memory remains best effort, and the failed native attempts are
recorded in VALIDATION.md. Android compilation cannot substitute for device tests.

Final gate results and exact commands are in [VALIDATION.md](VALIDATION.md).
Final-tree gates pass:1,000 workspace tests,0 failures,6 intentional ignores;
Clippy including the desktop/validation app, fmt, SPDX, matrix, deny/vet,
locked fuzz build and the dependency baseline check. The maintained vendor
checks pass13 and standard real-engine checks pass3. Final native media passes
twice with a controlled private desktop runtime, without claiming physical audio
or Android compatibility. No commit, branch or push was made.

---

# Review — Plan 0050: storage leases, recovery-copy authorization, media capability (fresh context)

| Verdict | Reviewed | Diff reviewed |
|---------|----------|---------------|
| Changes requested (minor) | 2026-11-16 | `9e1472d`, read against the current tree, including later changes from 0057/0059/0061 to the same code |

## Summary

This review covers the source repairs: file leases and stale cleanup
(`qv-store/src/fs.rs`, `archive.rs` SnapshotDir), the immutable SQLite probe
(`sqlite.rs::identify_existing`), the recovery pin (`qv-core/src/snapshot.rs`,
`rotation.rs`), the protected preview source (`qv-app/src/preview.rs`), the
media transport (`third-party/dioxus-desktop/src/{hardened_media,edits}.rs`)
and deletion-operation identity (`qv-core/src/edit.rs`). The lease design
holds:

- creation and cleanup share a directory flock;
- the registry refuses to open or close an extra descriptor on a live SQLite
  inode in this process;
- cleanup removes only unleased, owner-only names in the exact temp pattern,
  and their journals only while it holds the base lease;
- rotation-reserved names are skipped.

The probe runs before any mutable connection and with read hardening. Preview
chunks are AEAD-checked ≤ 64 KiB `SecretBytes`; the session is held as `Weak`
and is not kept across socket writes; revocation is checked per chunk; the
token is compared in constant time. No blocker or high finding. Android media
qualification is still open, so the plan stays `In progress`.

## Findings

1. **[medium] The QV-011 repair was later removed, and the ledger still claims it** —
   `plans/0046-*/FINDINGS.md:52`. Commit `291330a` (plan 0059, owner D2,
   2026-11-15) removed `require_recovery` and the seq/generation/writer
   binding. Final deletion, history purge and attachment removal now take no
   recovery copy. That is an owner decision, but the ledger row still says
   core verifies a head-bound copy before these operations. An auditor reading
   the ledger for 1.0 would assume a control that no longer exists. Fix:
   record QV-011 as superseded by 0059 D2, and list the remaining scope
   (rotation only).
2. **[low] Rotation's recovery copy is not tied to the state that gets rotated** —
   `crates/qv-core/src/rotation.rs:115-116`. `begin_destructive` takes the
   copy inside a `read`, and `take_open` runs afterwards in a separate lock
   acquisition. A concurrent `mutate` in between can commit, for example a LAN
   merge or another app thread. The pinned copy then lacks that change. Its
   name and the 0007 §3.2 wording say it is the pre-operation state, so
   restoring from it after a failed rotation silently drops the change. Fix:
   record `local_seq`/generation in `RecoveryPin`, and fail with `StaleHandle`
   after `take_open` if they differ. Better, take the copy and the open state
   under a single guard. Add a test with an interleaved mutation.
3. **[low] Media CORS on Linux is granted to every opaque origin** —
   `third-party/dioxus-desktop/src/hardened_media.rs:114-123,161-165`. On Linux
   `origin()` accepts `null` and echoes `Access-Control-Allow-Origin: null`.
   Sandboxed iframes, `data:` documents and opaque redirects also send
   `null`, so this is wider than 0003's "CORS to the app's own origin only".
   The 128-bit token is the only barrier left, so the risk is limited. Fix:
   state this limit in 0003/0009 (WebKitGTK's custom scheme serializes as
   `null`), or send no CORS header when the origin is `null` if Linux playback
   from memory can do without it.
4. **[low] Any local process can starve media requests** —
   `third-party/dioxus-desktop/src/edits.rs:284-293`. HTTP media and websocket
   handshakes share a cap of four connections, and an unauthenticated peer
   can hold each slot for 3 s, again and again. On Android every app with
   INTERNET can reach 127.0.0.1, and on Linux every local user can. Fix:
   reserve a slot for the webview's own websocket, or give media and
   handshakes separate small caps. At minimum, document it as a known
   availability limit. There is no confidentiality impact.
5. **[low] One oversized temp aborts the whole cleanup pass** —
   `crates/qv-store/src/fs.rs:652-655`. `remove_stale_temps` returns any
   `open_existing` error except Locked/NotFound. `check_file`'s
   `LimitExceeded` (a temp larger than `MAX_DB_BYTES`) or an `UnsafeFile`
   race therefore stops the pass, and the app reports `PrivateStorage` at
   every start (`controller.rs:796-801`). Fix: skip that candidate and move
   on. Report an error only for directory-level failures.

Checked with no finding: `byte_range` (one range only; suffix, open-ended,
reversed and empty forms are rejected with 416, and `Content-Range: */total`
is sent); the header is capped at 8192 bytes, 64 lines, an exact
numeric Host and GET/HEAD with no body; the response sets no-store,
nosniff and `default-src 'none'`. Preview close is wired into
`State::invalidate`, and image consent is bound to the exact handle.
Deletion-operation ids are random per call, ordinary drafts cannot set them,
restore clears them and restore follows only matching ids.

## Fixes applied by reviewer

None (review-only instruction).

## Gates

Not run (code reading only, on a shared, nearly full disk). Earlier logs
predate the 0057/0059/0061 edits to this code.

## Escalations for the owner

- Android playback, seek and revocation evidence (step 6) is still open.
- Finding 1 needs the ledger corrected before any 1.0 security summary cites QV-011.

## Developer response (2026-11-16)

- Finding 1 (medium), fixed: the QV-011 row in
  `plans/0046-audit-remediation/FINDINGS.md` is marked superseded by plan
  0059; only rotation still takes a recovery copy.
- Finding 5 (low), fixed on Linux and Android: one temporary file that
  cannot be checked or removed is skipped and the pass goes on
  (`fs.rs`, test `stale_cleanup_skips_a_file_it_cannot_check`). The Windows
  twin in `fs_windows.rs` is unchanged: no Windows target here to build it.
- Findings 2, 3 and 4 (low): open, listed as known limitations in
  `docs/release-notes/1.0.0.md`.

## Follow-up repairs — plan 0065 (2026-11-16)

- Finding 2: fixed. Rotation takes its recovery copy and removes the open
  state under one session guard (`take_open_after`); a concurrent save cannot
  commit between them. The interleaved-mutation regression confirms exclusion.
- Finding 3: fixed. The loopback adapter grants no CORS to `Origin: null`;
  explicit app origins remain allowed. Transport tests cover both. A native
  WebKitGTK probe sends `dioxus://index.html` and fetches with that own-origin
  grant; actual media qualification is recorded in 0065's execution log.
- Finding 4: fixed for the idle-socket case. Four pending initial-header slots
  evict the oldest idle peer to admit a new connection, under a total thread
  cap of sixteen. The page websocket connects while idle peers occupy the
  slots. Continuous connection flooding remains a local availability limit.
- Finding 5: the Windows cleanup twin now also skips individual unsuitable
  candidates, with an oversized-file regression; directory-level errors remain
  failures. Platform execution evidence and its limits are in 0065.
- The QV-011 ledger correction remains in force. Real Android media checks
  under the disposable UID remain open; these code repairs do not close them.
