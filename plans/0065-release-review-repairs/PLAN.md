# Plan 0065 — Complete 1.0 review repairs and review LAN/2

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| Implemented — available checks complete; fresh review/native Windows and attended release evidence pending | 2026-11-16 | 2026-11-16, owner: finish known review issues and review LAN; continue previous agent's work | 0002 §5.4; 0004 §1.4, §5.4; 0007 §3; 0008 §1, §4; 0012 §1–3; 0013 §15; 0015 §6 |

## Intent

Finish the outstanding actionable findings from the 1.0 reviews, preserving
the earlier agent's staged repairs, and perform a separate source review of
the current LAN/2 composition. Record remaining target/release evidence
honestly. No commit, push, branch or release publication.

## Scope

The findings in reviews 0045, 0049, 0050, 0052, 0053, 0055, 0059, 0060,
0063 and 0064, the inherited
16-file staged patch, and LAN authentication, framing, exchange and enrollment.
Attended phone/Windows tests and signing/CI remain subject to available targets;
an agent source review does not supply an independent-party audit.

## Spec basis

| Behaviour | Spec | Status | Implementation |
|-----------|------|--------|----------------|
| Complete, cancellable compaction | 0004 §1.4; 0008 §1.4 | Accepted | Amend §1.4 first: compare full ciphertext; interrupt copying/verification on cancellation; state Windows replacement lock interval accurately |
| Bound hostile picked files | 0004 §5.4, §4 | Accepted | Check fixed SQLite header before staging beyond 100 bytes; bound by declared page count and global cap; authenticate normally afterwards |
| Picker request isolation | 0009 §1.8; 0008 §4 | Accepted / owner-approved app scope | Retain inherited per-request JNI read/write/close identities |
| Rotation copy atomicity | 0007 §3.2 | Accepted | Retain single session guard through recovery copy and open-state handover |
| Media authority and availability | 0003 attachment previews; 0008 §4 | Accepted | No CORS for null origin; bound pending handshakes without idle-peer starvation; document residual flooding limit |
| Quick-unlock errors and remembered view | 0002 §5.4, §3.3 | Accepted | Retain inherited transient storage classification; recompute view tag when recording |
| Frozen compatibility | 0015 §6; 0012 §3 | Accepted | Keep staged fixed encoding fixtures; exercise password restore of golden archive, legacy schema and generation-2 reopening |
| LAN/2 review | 0013 §15; 0012 §3 | Accepted | Trace exact production source, hostile peers, key transitions, publication, deadlines and cleanup; write scoped review |

## Decisions needed

None for these repairs: the owner explicitly requested completion on the
current tree; plan 0056 records delegation of implementation details. No
cryptographic parameter, frozen byte layout, new dependency or owner open
question is decided here. Independent-party review and attended release
evidence cannot be inferred from this session's checks.

## Design

- Keep component boundaries and existing public interfaces where possible.
  `compact` retains its borrowed cancellation callback. Use scoped concurrency
  to supply a `'static` progress-handler signal without capturing borrowed
  session state in SQLite. The watcher joins before the call returns; no
  unsafe lifetime extension. Check cancellation during row comparisons too.
- Stream ciphertext comparison in sorted row order using borrowed SQLite
  values, at most one bounded row from each connection at a time. Copy,
  verify, cancellation check, atomic replacement, reopen; no anchor changes.
  Windows closes only SQLite's source connection, retaining both process
  locks through replacement and reopening (see Revisions).
- Validate only the fixed public SQLite header during picked-file staging:
  exact magic, page size 4096, declared pages nonzero and bounded by
  `MAX_DB_BYTES`; reject truncation/overrun, clean the same leased staging
  directory on error. This header conveys no authentication.
- All existing protected containers, redacted errors, actual-byte limits,
  durable publication and per-operation cancellation rules remain. No new
  dependencies, formats, keys, parameters or secret output.

## Steps

### Step 1 — Reconcile inherited repairs and complete storage fixes

- [x] **Do:** inspect inherited patch; amend compaction spec before code;
  compare all ciphertext, interrupt copy/verification and test restoration
  of hardening and unchanged source after failures; add Windows concurrent
  opener guard test; fix restore using an outcome-preserving operation and Unix non-UTF8
  paths. Retain snapshot replay/downgrade/crash/full tests.
- **Files:** `crates/qv-store/src/{compact,sqlite,write,manifest,snapshot_tests,crash_tests,fs_windows}.rs`,
  `crates/qv-core/src/{snapshot,session,rotation,rotation_tests}.rs`,
  `specs/0004-local-storage.md`, `specs/README.md`.
- **Done when:** same-length ciphertext corruption is refused; cancellation
  interrupts SQLite before completion; source remains usable; snapshot and
  compaction rejection/crash tests pass; restoration never compacts source.
- **Verify:** focused qv-store compaction/snapshot/crash and qv-core rotation/snapshot tests.

### Step 2 — Finish import, media, quick-unlock and compatibility findings

- [x] **Do:** early import-header checks and rejection tests; review inherited
  Kotlin request guards, media listener changes and quick-unlock tests; fix
  stale view tags; finish golden restore/legacy/rotation coverage.
- **Files:** `crates/qv-store/src/{vault_import,golden_tests}.rs`,
  `crates/qv-core/src/quick_unlock.rs`, `crates/qv-core/tests/golden.rs`,
  `crates/qv-app/src/{android,controller}.rs`, `crates/qv-app/android/MainActivity.kt`,
  `third-party/dioxus-desktop/src/{edits,hardened_media}.rs`,
  `third-party/README.md`, `crates/qv-app/WEBKIT.md`, `specs/0002-cryptography.md`.
- **Done when:** inherited fixes verified; wrong files rejected within header;
  null-origin media response has no CORS; idle peers cannot exhaust listener;
  golden archive restores without source; remaining unavailable checks named.
- **Verify:** focused import/quick-unlock/golden tests; vendor `hardened` tests;
  app feature Clippy/real-engine validation; Android build if tooling exists.

### Step 3 — Review LAN/2 and repair demonstrated local implementation defects

- [x] **Do:** read 0013 §15 against crypto/engine/protocol/sync/core/store;
  inspect vectors and hostile-peer tests; record findings, scope, exclusions
  and verdict in `LAN-REVIEW.md`. Add specific regression tests and only
  spec-prescribed repairs; design changes require a plan revision first.
- **Files:** LAN source/tests in `qv-{crypto,protocol,sync,core,store}`, exact
  file scope to be recorded in the review; no protocol byte/layout change.
- **Done when:** each review-package question has a source trace and disposition;
  protocol/crypto/sync/core LAN tests pass; release record states review limits.
- **Verify:** focused qv-crypto LAN, qv-protocol, qv-sync and qv-core LAN tests.

### Step 4 — Verify final tree and reconcile release records

- [x] **Do:** required gates, matrix, vendor/feature tests, Android/Windows
  checks available locally; update original review responses, plan index,
  0056 log, release notes and CHANGELOG. Write repair review with explicit
  same-session limitation; no fresh-context implementation review claim.
- **Files:** this plan/review, original review documents, `plans/README.md`,
  `plans/0056-release-1-0/PLAN.md`, `docs/release-notes/1.0.0.md`, `CHANGELOG.md`.
- **Done when:** every known finding has a fix/test or precise remaining
  evidence/blocker, and all possible gates have current results.
- **Verify:** `sh scripts/check-spdx.sh`, `cargo fmt --check`,
  `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`,
  `sh scripts/check-matrix.sh`, `git diff --check`; no dependency changes.

## Test plan

Run focused checks first and final gates once with full logs in `/tmp`.
Keep KDF concurrency at its default 1. Matrix rows include Snapshot disk
growth, hardware assumptions, interrupted mutations and hostile LAN peers.
Never use personal vaults or weaken assertions. Cross-compilation and Wine
remain development evidence, not attended Windows checks.

## Risks and stop conditions

Stop dependent work for a spec ambiguity affecting security/data, a frozen
format change, an unresolved owner decision or a required new dependency.
Keep release blockers separate from repaired code and passing automated tests.

## Documentation

- [x] CHANGELOG and release notes: fixes and actual validation.
- [x] Architecture: no component/trust-boundary change intended.
- [x] Specs: compaction clarification and inherited quick-unlock amendment,
  Changelogs and index; no status promotion from partial evidence.
- [x] Original review responses, plan 0056 and index kept current.

## Revisions

- 2026-11-16: LAN review found that existing-copy initiator core constructed
  `SessionExchange` with no deadline, allowing local verification/commit after
  the socket's ten-minute cap. Step 3 adds `sync_with_code_until(..., expires:
  Instant)` in qv-sync (existing wrapper retained), and core constructs one
  deadline before the operation and passes it both there and to SessionExchange.
  Regression: an already-expired initiated exchange does not connect; a
  staged merge at expiry changes no head/anchor. No wire change (0013 §15.6).
- 2026-11-16: Windows compaction can retain its source process lock after
  closing only SQLite's handle (qv-win handles already share delete). Step 1
  will use the same locked replacement sequence as Unix, plus a crate-private
  connection-close/reopen helper in `vault.rs`, instead of documenting a
  needless unlocked interval. Test a second opener while SQLite is closed.
- 2026-11-16: LAN review also found `peer_bytes` rescanning every manifest
  reference after each received object (quadratic work at the 381,072-reference
  cap). Step 3 maintains bounded cumulative peer byte counts in `LanStaging`,
  incremented only after successful peer appends; test local/peer accounting
  and rejected-object stability. This changes no stored/wire bytes.
- 2026-11-16: The supplemental player JS tests failed because their synthetic
  DOM predates the audio-level work (`HTMLAudioElement`/`matchMedia` missing).
  Step 2 updates only those browser API stubs in `tests/player.cjs`; all existing
  playback, cleanup and seek assertions remain unchanged.
- 2026-11-16: Full gates caught the existing restore publication/lock race
  after changing `mutate` to `read`: `read` would replace a durable publication
  with Cancelled. Step 1 uses the crate-private outcome-preserving `run`
  operation instead, without mutation hooks/automatic compaction. The existing
  before/after-publication assertions are retained.
- 2026-11-16: Reconcile earlier partial 1.0 reviews too (0045/0052/0053/0055).
  Step 2 also fixes post-success biometric cancellation counting in
  `qv-platform/src/quick_unlock.rs`, publishes Windows clipboard exclusion
  hints before text in `qv-win/src/clipboard.rs` (same documented FFI calls,
  no new unsafe operation), and bounds Wayland pipe writes by the hold deadline
  and cancellation while destroying ignored compositor offers in
  `qv-platform/src/clipboard/wayland.rs`. Use existing clipboard/fake-compositor
  tests plus targeted regressions. Duplicate title cache residency is already
  budgeted; library lowercasing and OS clipboard copies remain explicitly
  accepted best-effort erasure limits, not an excuse to introduce unverified
  custom Unicode/OS memory handling. No new on-disk/wire format or owner choice.

- 2026-11-16 design revision (architect): use `FileRenameInfoEx` with
  REPLACE_IF_EXISTS and POSIX_SEMANTICS for Windows compacted-vault replacement.
  Classic `MoveFileExW` cannot replace a destination held open by our lock.
  Add `qv-win/src/fs.rs::rename_replace_open` and call it only for vault
  compaction; anchor replacement keeps its existing API. Open the verified
  copy no-follow with DELETE/write access, sharing all, write-through; flush
  after rename. Never fall back by releasing the source lock. Unsupported
  filesystems fail closed and reopen the unchanged source. This plan permits
  the necessary unsafe operations only in the existing qv-win FFI module:
  aligned, initialized variable-length FILE_RENAME_INFO buffer access, bounded
  UTF-16 tail copy and SetFileInformationByHandle, each separately documented.
  Test both held/locked file identities across replacement. Microsoft API
  basis: [rename flags](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information),
  [write-through metadata](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilea).
  Wine's standalone probe also rejects FileRenameInfoEx with access denied;
  record that limitation without skipping assertions or claiming native proof.

## Execution log

- 2026-11-16: Owner requested all remaining 1.0 review repairs plus LAN review
  and clarified that the previous agent exhausted its usage. Initial tree:
  16 staged source/spec files, no unstaged changes. Preserving that staging;
  new edits remain unstaged. Inherited work includes snapshot crash/replay/
  downgrade/full tests, rotation-copy serialization, request-bound JNI,
  fingerprint diagnostics, fixed golden fixtures, partial SQLite interruption
  scaffolding and media admission/CORS changes. The last two still need
  integration and verification. No commit/session approval requested again.

- 2026-11-16: Source repairs complete: full ciphertext comparisons; SQLite
  progress interruption via a scoped worker and atomic signal while retaining
  the borrowed cancellation API; source locks retained on Windows; non-UTF8
  Unix temp cleanup/VACUUM paths; restore uses outcome-preserving `run` without
  mutation/compaction hooks. Early import header/declared-size rejection is
  specified before code; request-bound JNI fixes include stale native opens,
  and the instrumentation fixture covers stale stream operations. Rotation
  handover and quick-unlock diagnostics retain the inherited protected-state
  fixes; view tags are recomputed on recording.
- 2026-11-16: Golden coverage completed. One-time variant generator creates
  only a new named directory, from copies of original goldens; original frozen
  files are unchanged. Added schema4/generation2 files and anchor hashes under
  `qv-store/tests/golden/variants`, standalone archive/password restore and
  portable core golden tests. Inherited fixed encoding fixtures are retained.
- 2026-11-16: LAN source review completed in LAN-REVIEW.md; 39-file exact
  source baseline recorded. Repaired missing initiator local deadline and
  quadratic progress accounting, with regressions. No wire/crypto parameter or
  dependency change. No external contact or independent-audit claim.
- 2026-11-16: Verification in progress. First full workspace run caught the
  late-lock restore outcome regression and import-test staging assumption;
  fixed both without relaxing assertions. The final full rerun is
  `/tmp/qv65-workspace-tests-final.log`, focused correction run
  `/tmp/qv65-core-repairs.log`. Earlier storage regressions caught and corrected
  inherited compile/test setup problems (anchor reuse between cases, unsupported
  direct VACUUM under attach limit zero, and a row-count assumption replaced
  by checking actual pruned objects). No production gate was weakened.
- 2026-11-16: Current supplemental results: vendor hardened tests 14/14 pass
  (`/tmp/qv65-vendor-tests-final.log`); JS player tests 11/11 pass after browser
  API fixture updates (`/tmp/qv65-player-final.log`); SPDX/checker self-tests,
  matrix and supply-chain baseline pass. Cross-Windows all-target Clippy passes
  (`/tmp/qv65-windows-clippy-final.log`); MinGW tools were unpacked privately in
  `/tmp` after system package installation was unavailable. Wine focused
  compaction/golden/cleanup tests are running under a private prefix with
  `umask 077`, `QV_WINE=1`; native Windows evidence is not claimed.
- 2026-11-16: Android app and instrumentation APKs build with JDK21 and the
  hash-verified official Dioxus CLI0.7.10 (installed CLI was0.8 alpha). On the
  musl host a temporary tool-only `__res_init`→`res_init` alias loads the GNU
  CLI; it is not part of an application artifact. Debug APK SHA-256
  `d52ae54bea181e2348637e9ae33b786255578a1a5654392aeb35e48dc08de8e1`;
  log `/tmp/qv65-android-build-final.log`. `adb devices` has no connected phone;
  no install/device pass claimed. The build's private Gradle daemon was stopped.
- 2026-11-16: Native WebKitGTK probe observes the explicit
  `dioxus://index.html` Origin and succeeds with only that CORS grant; opaque
  null receives no grant. Actual composited media playback/seek/revocation
  flow passes; download-file sampler checked, found none, unreadable false
  (`/tmp/qv65-media-runner.log`, `/tmp/qv65-media/native.log`), peak summed
  process RSS870392KiB. This is evidence against the named WebKit download-file
  fault, not a general proof of no disk or renderer copies. Software rendering
  repeat and final app/whole-workspace checks remain in progress.

- 2026-11-16: Reconciled older partial reviews 0045/0052/0053/0055. Focused
  platform run `cargo test --locked -j 1 -p qv-platform --lib -- quick_
  wayland_clipboard` passes 15 tests (`/tmp/qv65-platform-regressions.log`);
  final workspace run below includes the subsequent single cancellation
  sampling cleanup. Clipboard exclusions precede plaintext, Wayland writes
  obey hold/cancel and destroy offers, successful cancelled biometric prompts
  refund their reserved attempt. Accepted Unicode/cache/OS-copy limits remain
  documented. No native HGLOBAL lifetime assumption was added.
- 2026-11-16: Final Windows all-target cross-Clippy passes with GNU target
  and private MinGW (`/tmp/qv65-windows-clippy-complete.log`). Wine focused
  qv-win run (`/tmp/qv65-wine-platform.log`) has 1 pass (clipboard publication,
  replacement and each failed exclusion) and 1 failure (locked file
  replacement, OS access denied 5). Both standalone MoveFileExW and
  FileRenameInfoEx probes also reject replacing an open destination under
  this Wine prefix. The earlier focused store run has 12 passes, 3 compaction
  replacement failures and 1 ignored one-time fixture generator
  (`/tmp/qv65-wine-store-final.log`). No assertion was disabled, and this is
  not recorded as a native Windows pass. The FileRenameInfoEx source repair
  requires native Windows CI/attended validation.
- 2026-11-16: Current Android app **and instrumentation APKs** build and
  inspection pass (`/tmp/qv65-android-build-complete.log`), including all latest
  Rust/Kotlin/Java repairs. Debug app SHA-256
  `358e55b3d75d0ff53c9a29e8ba53643b0a82f4111ae4ec1f7c53c671492ff24f`.
  A stalled prior rerun was terminated after its thread dump showed Kotlin
  walking the default daemon directory; rerun used private
  `/tmp/qv65-kotlin` plus in-process compilation via GRADLE_OPTS, no product
  change. No device is connected, no install performed, and no release
  signing key was used. Private Gradle daemon stop requested after completion.
- 2026-11-16: Both actual Linux media flows pass with download-file sampling:
  composited peak summed RSS870392KiB and software peak609184KiB; checked true,
  found false, unreadable false. Logs `/tmp/qv65-media-runner.log` and
  `/tmp/qv65-media-software-runner.log`; native playback/seek/revocation logs
  under their named output directories. No isolated Android UID or general
  absence-of-disk-copy claim follows.

- 2026-11-16: Current non-test gates exit 0: `sh scripts/check-spdx.sh`,
  `cargo fmt --check`, `sh scripts/check-matrix.sh`, `git diff --check`,
  `sha256sum -c plans/0065-release-review-repairs/SOURCE-BASELINE.sha256`
  (all 39 reviewed files match). Logs `/tmp/qv65-{spdx,fmt,matrix,diff}-complete.log`
  and `/tmp/qv65-lan-baseline.log`. Workspace all-target Clippy exits 0
  (`/tmp/qv65-clippy-complete.log`). Final test/feature gate results follow.
  Vendor hardening 14/14, player JS 11/11, SPDX checker self-tests and
  supply-chain baseline checks from above still cover unchanged inputs.
  Both lockfiles and dependency manifests remain unchanged, so deny/vet are
  not rerun or represented as new audits. Private Gradle daemon stop exited 0.

- 2026-11-16: Desktop and validation app all-target Clippy both exit 0,
  `cargo clippy --locked -j 1 -p qv-app --all-targets --features desktop
  -- -D warnings` and the same with `--features validation`.
  Logs `/tmp/qv65-app-desktop-clippy-complete.log` and
  `/tmp/qv65-app-validation-clippy-complete.log`. The unchanged vendored
  upstream show_toast dead-code warning is emitted as a dependency warning;
  product Clippy warnings remain denied.

- 2026-11-16: `REPAIR-BASELINE.sha256` pins changed source/spec/fixture and
  component maintenance files for handoff, including all inherited staged
  source changes and new golden variants. The broader LAN review scope stays
  separately pinned by SOURCE-BASELINE.sha256. Repair review is a same-session
  follow-up, not a fresh-context approval; no staging changed.

- 2026-11-16: `RUST_TEST_THREADS=1 cargo test --locked -j 1 -p qv-app
  --features validation` exits 0: 113 unit tests and 3 real WebKitGTK engine
  flows pass (`/tmp/qv65-app-validation-complete.log`). Synthetic flow
  directories were cleaned by the harness; no personal vault was used.

- 2026-11-16 final handoff: `cargo test --locked -j 1` exits 0 on the
  final source tree: **1,091 passed, 0 failed, 8 ignored**, including 279
  store and 48 platform unit tests. Log `/tmp/qv65-workspace-tests-complete.log`.
  The ignored entries are one near-budget disk qualification, two generators
  and five child-process helpers; the parent tests exercise the helpers.
  No test was disabled for Wine or to reach this result. Earlier successful
  workspace run (1,088 passed) is superseded by this run after the three
  additional platform regressions.
- 2026-11-16 final disposition: all four implementation/documentation steps
  completed within available targets. REVIEW.md records a same-session
  assessment with release clearance requested, so the plan is Implemented,
  not Reviewed. All actionable source findings from the named reviews have
  repairs and targeted checks; remaining native Windows replacement behavior
  is explicitly unverified. Wine's access-denied failure remains recorded.
  Attended Android/Wayland/Windows/LAN evidence, native Windows CI, phone KDF,
  near-budget and independent-party composition/dependency qualification remain
  release gates. The owner alone commits, pushes, tags or publishes. Original
  staging is preserved, and no such action was performed here.
