# Review follow-up — Plan 0065: 1.0 repairs and LAN/2

| Verdict | Date | Diff reviewed |
|---------|------|---------------|
| Changes requested for release clearance: native Windows validation and fresh review of these authored repairs remain | 2026-11-16 | `e177ba15801eac0eb7716a247415bc05a72bd605` plus inherited 16-file staged patch and unstaged plan 0065 repairs |

## Scope and conformance

This session reconciled the previous fresh-context reviews, planned and
implemented their remaining actionable repairs, and reviewed LAN/2 source
and composition. The same agent authored and checked these repairs; this
record is **not** a new fresh-context or independent-party approval. Plan
status stays Implemented after available gates, rather than Reviewed.

| Step | Result |
|------|--------|
| Storage/snapshot/rotation repairs | Full ciphertext equality; interruptible VACUUM; restored hardening; snapshot rollback/downgrade/crash/full-storage cases; no source compaction during restore; atomic recovery-copy handover |
| Import/platform/media/compatibility | Early bounded header checks; JNI stream identities; transient quick-unlock diagnostics; cancelled-prompt budget; last-view tags; CORS and idle listener admission; Wayland deadlines/offers; clipboard hint ordering; golden restore/schema/generation fixtures |
| LAN/2 | Separate source/composition review, exact baseline, two medium defects repaired and regressed; see LAN-REVIEW.md |
| Evidence/docs | Current commands/results in PLAN.md; original review follow-ups, release notes, spec Changelogs and index |

All source changes are in the plan or its recorded revisions. No frozen
format, cryptographic parameter, dependency, owner decision, commit, branch,
push or release publication was introduced. Original frozen golden files
are unchanged; new variants are separately named and hash-pinned. Original
staging is preserved. `REPAIR-BASELINE.sha256` pins every changed
source/spec/fixture/maintenance file; `SOURCE-BASELINE.sha256` separately
pins the broader LAN review scope.

## Findings and disposition

1. **Medium, fixed — LAN initiator deadline did not cover local acceptance.**
   Share one deadline between core staging/merge and the sync driver. The
   regression checks no connection or sequence advance after expiry.
2. **Medium, fixed — LAN progress required quadratic reference scans.**
   Cache the verified total and count only successful peer appends. The
   regression covers mixed local/peer inputs and rejected corruption.
3. **Release evidence pending — Windows locked replacement.** The old
   unlocked interval is removed. FileRenameInfoEx POSIX replacement retains
   both locks and fails closed if unavailable. The FFI uses aligned initialized
   storage, checked UTF-16 sizes, separate documented unsafe operations, a
   write-through handle and flush. The native identity/lock regression remains
   enabled. Wine rejects this API operation in a standalone probe too;
   cross-compilation cannot establish native runtime behavior. Obtain native
   Windows CI and attended checklist results before Windows clearance.
4. **Accepted residual limits.** Unicode lowercase intermediate allocations
   and Windows/framework/OS copies remain best-effort cleanup limits. Duplicate
   title cache residency is charged to the existing budget. Sustained local
   connection flooding may delay the bounded listener. No unverified custom
   erasure, Unicode, protocol or cache design was substituted.

The original findings are individually answered in reviews 0045, 0049,
0050, 0052, 0053, 0055, 0059, 0060, 0063 and 0064. Initial verification caught
and corrected test setup assumptions, a missing Debug-free assertion and a
late-lock restore outcome regression; assertions and publication checks were
retained. LAN-REVIEW.md documents the composition/vector limitations without
claiming that regression tests prove security.

## Gates

| Check | Result |
|-------|--------|
| SPDX, fmt, matrix, diff whitespace | pass |
| Workspace all-target Clippy | pass |
| Full workspace tests | 1,091 pass, 0 fail, 8 ignored |
| App desktop/validation all-target Clippy | pass |
| App validation | 113 unit + 3 real-engine flows pass |
| Windows GNU all-target cross-Clippy | pass |
| Android app/instrumentation APK build | pass, no connected device |
| Vendor hardening / player JS | 14 / 11 pass |
| Actual composited/software Linux media flow | pass, no sampled download files |
| Wine targeted runtime | clipboard pass; locked replacement fails access denied |
| Exact source baselines | LAN 39 / repair 44 files match |

Exact current commands, scopes, results and temporary logs are recorded in
PLAN.md. Dependency manifests and both lockfiles are unchanged; no new
cargo-deny/vet exemption or dependency review is involved.

## Documentation and remaining release evidence

CHANGELOG, spec 0004 and index, inherited spec 0002, vendor maintenance
record, WEBKIT notes, review follow-ups, plan index, 0056 and release notes
are updated. Architecture components and trust boundaries are unchanged.

Native Windows/CI, attended phone/picker/media/Wayland and two-device
interruption checks, phone KDF and near-budget qualification remain
outstanding. An independent-party composition/dependency assessment and
complete production/error-transition vectors are not supplied by an agent
source review. Release publication remains the owner's action. No external
party was contacted or permission requested for routine authorized repairs.
